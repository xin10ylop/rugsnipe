//! The cold thread: a tokio current-thread runtime that owns everything the
//! hot loop must never wait for (docs/06 sections 2, 7.3, 8, 9.6, 12, 13).
//!
//! * ledger: hot/sender messages -> `state.db` rows (one writer thread);
//! * kill latch: kill file (one `stat` per 100 ms) and `SIGUSR1`;
//! * exits lease: renewed only while the hot loop's heartbeat advances, so
//!   the guard can take over when the loop stalls;
//! * nonce keeper (paper: simulated): burns at `since + burn_after_slots`
//!   or on the first dump signal, then the value changes;
//! * funding watcher: feed funding transfers -> `Watchlist::on_funding`
//!   (copy-on-write, so it stays off the hot thread) -> `watch_dyn`;
//! * held mints: LogsWs per-address subscriptions (watch / unwatch);
//! * paper fill lane inside the RPC budget: E1 verdicts and replay tapes
//!   ([`crate::e1`]) or E2 states ([`crate::e2`]); shadow rows, failed
//!   attempts and the session's paper totals ([`PaperReport`]);
//! * ArmSet reload on file change; feed health -> rslog + decode-miss kill;
//! * Prometheus text endpoint on the configured listen addresses.

use crate::hot::Shared;
use crate::msgs::{ColdMsg, HotIn, ShadowInfo};
use crate::setup::{load_armset_file, ArmState};
use crate::stats::HotStats;
use arc_swap::ArcSwap;
use prometheus_client::registry::Registry;
use pump_codec::Pubkey;
use rugsnipe_core::config::Config;
use rugsnipe_core::kill::KillFile;
use rugsnipe_core::risk::{KillReason, RiskGate};
use rugsnipe_core::rslog::{Rec, RslogProducer, ShadowOutcome};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::Mode;
use rugsnipe_exit::{PositionState, Venue};
use rugsnipe_feed::FeedHealth;
use rugsnipe_state::lease::{Acquire, Lease};
use rugsnipe_state::rows::*;
use rugsnipe_state::{Ledger, RpcBudget, Write};
use rugsnipe_tx::nonce::{next_value, NoncePool, NonceState};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Paper fill lane settings.
#[derive(Debug, Clone)]
pub struct E2Cfg {
    pub rpc_url: String,
    pub budget: Arc<RpcBudget>,
    /// Wait after the decision before reading history (confirmation).
    pub delay_ms: u64,
    pub max_tx: usize,
}

/// ArmSet hot reload.
#[derive(Debug, Clone)]
pub struct ArmReload {
    pub path: PathBuf,
    pub cfg: Config,
    pub regime: Option<String>,
}

/// Cold-thread settings.
#[derive(Debug, Clone)]
pub struct ColdCfg {
    pub box_id: String,
    pub mode: Mode,
    pub owner: Pubkey,
    pub kill_file: PathBuf,
    pub metrics_listen: Vec<String>,
    /// `(state.db path, lease owner name, takeover_after_ms)`.
    pub lease: Option<(PathBuf, String, u64)>,
    pub e2: Option<E2Cfg>,
    /// E1 lane (`paper.fill_model = "e1"`); takes precedence over `e2`.
    pub e1: Option<crate::e1::E1Cfg>,
    /// Fixed cost of a landed round trip (summary net P&L).
    pub round_trip_lamports: u64,
    pub armset: Option<ArmReload>,
    pub armset_max_age_s: u64,
    pub burn_after_slots: u64,
    pub decode_miss_kill_ratio: f64,
    pub paper_start_balance: u64,
}

/// Counters of the cold side (summary).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ColdReport {
    pub intents: u64,
    pub sends: u64,
    pub fills: u64,
    pub unfilled: u64,
    pub positions_opened: u64,
    pub positions_closed: u64,
    pub exits: u64,
    pub realized_lamports: i64,
    pub watch_mint: u64,
    pub unwatch_mint: u64,
    pub funding_events: u64,
    pub funded_entries: u64,
    pub e2_requested: u64,
    pub e2_resolved: u64,
    pub e2_failed: u64,
    pub nonce_burns: u64,
    pub killed: bool,
    pub kill_reason: Option<String>,
    pub lease_held: bool,
    pub lease_lost: u64,
    pub armset_reloads: u64,
    pub metrics_listening: Vec<String>,
    pub e2_errors: Vec<String>,
    /// E1 lane: requests, verdicts, RPC calls, errors (first few).
    pub e1_requested: u64,
    pub e1_resolved: u64,
    pub e1_rpc_calls: u64,
    pub e1_errors: Vec<String>,
    /// Shadow-mode totals of the session (docs/04 section 7).
    pub paper: PaperReport,
}

/// Paper (shadow) totals: verdicts, landing rate, premium, exits, P&L and
/// EV per attempt with the failed attempts' cost (docs/04 sections 3 and 7).
#[derive(Debug, Clone, Default, Serialize)]
pub struct PaperReport {
    /// Resolved paper intents by outcome (`filled`, `premium`, `slippage`,
    /// `not_in_slot0`, `class`, `unresolved`).
    pub outcomes: BTreeMap<String, u64>,
    /// Intents in the entry class (resolved, not `class`, not `unresolved`).
    pub in_class: u64,
    /// Sends that would have been made: fills + failed attempts.
    pub attempts: u64,
    pub fills: u64,
    pub failed_attempts: u64,
    /// fills / attempts.
    pub landing_rate: f64,
    /// Mean premium over the post-bundle price, fills and all attempts (bps).
    pub mean_premium_bps_fills: f64,
    pub mean_premium_bps_attempts: f64,
    /// Priority fees of the failed attempts.
    pub failed_cost_lamports: u64,
    pub closed: u64,
    pub exits_by_reason: BTreeMap<String, u64>,
    /// Realized P&L of the closed positions (proceeds - cost, before fixed costs).
    pub realized_lamports: i64,
    /// `closed x paper.round_trip_lamports`.
    pub round_trip_cost_lamports: u64,
    /// realized - round trips - failed attempts.
    pub net_lamports: i64,
    /// Stake of the closed positions.
    pub closed_spend_lamports: u64,
    /// Mean realized return per closed position (fraction of the stake), gross
    /// and net of the round-trip cost.
    pub return_per_fill_gross: f64,
    pub return_per_fill_net: f64,
    /// EV per attempt: net / (closed + failed attempts), as a fraction of the
    /// mean stake.
    pub ev_per_attempt: f64,
    #[serde(skip)]
    premium_sum_fills: u64,
    #[serde(skip)]
    premium_sum_attempts: u64,
    #[serde(skip)]
    spend_of: HashMap<Pubkey, u64>,
    #[serde(skip)]
    spend_sum_attempts: u64,
}

impl PaperReport {
    /// One resolved intent.
    pub fn on_shadow(
        &mut self,
        mint: Pubkey,
        outcome: ShadowOutcome,
        premium_bps: u32,
        spend: u64,
        cost: u64,
    ) {
        *self.outcomes.entry(outcome.as_str().into()).or_default() += 1;
        if !matches!(outcome, ShadowOutcome::Class | ShadowOutcome::Unresolved) {
            self.in_class += 1;
        }
        if outcome == ShadowOutcome::Filled {
            self.fills += 1;
            self.premium_sum_fills += premium_bps as u64;
            self.spend_of.insert(mint, spend);
        }
        if outcome == ShadowOutcome::Filled || outcome.is_failed_attempt() {
            self.attempts += 1;
            self.premium_sum_attempts += premium_bps as u64;
        }
        if outcome.is_failed_attempt() {
            self.failed_attempts += 1;
            self.failed_cost_lamports += cost;
            self.spend_sum_attempts += spend;
        }
    }

    pub fn on_exit(&mut self, reason: &str) {
        *self.exits_by_reason.entry(reason.into()).or_default() += 1;
    }

    pub fn on_closed(&mut self, mint: &Pubkey, pnl: i64) {
        if let Some(spend) = self.spend_of.remove(mint) {
            self.closed += 1;
            self.realized_lamports += pnl;
            self.closed_spend_lamports += spend;
            self.spend_sum_attempts += spend;
        }
    }

    /// Recompute the derived numbers.
    pub fn derive(&mut self, round_trip_lamports: u64) {
        let f = |a: f64, b: f64| if b > 0.0 { a / b } else { 0.0 };
        self.landing_rate = f(self.fills as f64, self.attempts as f64);
        self.mean_premium_bps_fills = f(self.premium_sum_fills as f64, self.fills as f64);
        self.mean_premium_bps_attempts = f(self.premium_sum_attempts as f64, self.attempts as f64);
        self.round_trip_cost_lamports = self.closed * round_trip_lamports;
        self.net_lamports = self.realized_lamports
            - self.round_trip_cost_lamports as i64
            - self.failed_cost_lamports as i64;
        let stake = self.closed_spend_lamports as f64;
        self.return_per_fill_gross = f(self.realized_lamports as f64, stake);
        self.return_per_fill_net = f(
            (self.realized_lamports - self.round_trip_cost_lamports as i64) as f64,
            stake,
        );
        let n = (self.closed + self.failed_attempts) as f64;
        let mean_stake = f(self.spend_sum_attempts as f64, n);
        self.ev_per_attempt = f(f(self.net_lamports as f64, n), mean_stake);
    }
}

/// An E1 verdict as the hot loop's messages, in order.
pub fn e1_messages(r: &crate::e1::E1Res) -> Vec<HotIn> {
    let req = r.req;
    let mut info = ShadowInfo {
        create_slot: req.slot,
        s0_failed: r.s0_failed,
        premium_bps: r.premium_bps,
        bundle_trades: r.intx_trades,
        ..Default::default()
    };
    if let Some(s0) = &r.slot0 {
        info.create_index = Some(s0.create_index);
        info.block_txs = Some(s0.block_txs);
        info.bundle_trades = s0.bundle_trades;
        info.bundle_wallets = s0.bundle_wallets.len() as u32;
        info.our_index = Some(s0.our_index);
        info.tp_ahead = s0.tp_ahead;
        info.tp_behind = s0.tp_behind;
        info.post_bundle_v = s0.ref_v;
        info.post_bundle_t = s0.ref_t;
        info.post_bundle_mcap = s0.mcap();
    }
    let outcome = if r.outcome == ShadowOutcome::Filled && r.slot0.is_none() {
        ShadowOutcome::Unresolved
    } else {
        r.outcome
    };
    let mut out = vec![HotIn::E1Open {
        intent: req.intent,
        mint: req.mint,
        outcome,
        info,
    }];
    if outcome != ShadowOutcome::Filled {
        return out;
    }
    if let Some(s0) = &r.slot0 {
        for k in &s0.bundle_wallets {
            out.push(HotIn::E1Insider {
                intent: req.intent,
                key: *k,
                creator: false,
            });
        }
        for k in &s0.creator_set {
            out.push(HotIn::E1Insider {
                intent: req.intent,
                key: *k,
                creator: true,
            });
        }
    }
    for t in &r.tape {
        out.push(HotIn::E1Trade {
            intent: req.intent,
            trade: rugsnipe_exit::Trade {
                venue: Venue::Curve,
                mint: req.mint,
                user: t.user,
                is_buy: t.is_buy,
                sol_amount: t.sol,
                token_amount: t.tok,
                fee_lamports: t.sol * 125 / 10_000,
                reserve_sol: t.v,
                reserve_token: t.t,
                slot: t.slot,
            },
        });
    }
    out.push(HotIn::E1End {
        intent: req.intent,
        horizon: r.horizon,
    });
    out
}

/// Everything the cold thread owns.
pub struct ColdParts {
    pub cfg: ColdCfg,
    pub from_hot: rtrb::Consumer<ColdMsg>,
    pub from_sender: std::sync::mpsc::Receiver<ColdMsg>,
    pub to_hot: rtrb::Producer<HotIn>,
    pub ledger: Option<Ledger>,
    pub logsws: Option<rugsnipe_feed::ws::LogsWsHandle>,
    pub healths: Vec<Arc<FeedHealth>>,
    pub watch: Arc<Watchlist>,
    pub arm: Arc<ArcSwap<ArmState>>,
    pub risk: Arc<RiskGate>,
    pub nonces: Arc<NoncePool>,
    pub shared: Arc<Shared>,
    pub stats: Arc<HotStats>,
    pub registry: Arc<Registry>,
    pub rslog: RslogProducer,
    pub sigusr1: Arc<AtomicBool>,
    pub report: Arc<Mutex<ColdReport>>,
}

/// Start the cold thread; it returns when `shared.stop` is set and the
/// rings are drained.
pub fn spawn(p: ColdParts) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("cold".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("cold: tokio runtime: {e}");
                    return;
                }
            };
            rt.block_on(Cold::new(p).run());
        })
}

struct Cold {
    p: ColdParts,
    lease: Option<(rusqlite::Connection, Lease, u64)>,
    e2_tx: tokio::sync::mpsc::UnboundedSender<(u64, Pubkey, Result<crate::e2::E2, String>, u64)>,
    e2_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Pubkey, Result<crate::e2::E2, String>, u64)>,
    e2_pending: usize,
    e1_tx: tokio::sync::mpsc::UnboundedSender<crate::e1::E1Res>,
    e1_rx: tokio::sync::mpsc::UnboundedReceiver<crate::e1::E1Res>,
    /// Session set of third-party slot-0 buyers (E1 sniper detection).
    snipers: Arc<Mutex<HashSet<Pubkey>>>,
    /// Messages for the hot loop waiting for ring space (in order).
    to_hot_q: VecDeque<HotIn>,
    last_hb: u64,
    was_killed: bool,
    armset_mtime: Option<std::time::SystemTime>,
    nonce_intent: Vec<u64>,
    sender_gone: bool,
}

fn lock(r: &Mutex<ColdReport>) -> std::sync::MutexGuard<'_, ColdReport> {
    r.lock().unwrap_or_else(|e| e.into_inner())
}

fn pos_state(s: PositionState) -> Option<PosState> {
    match s {
        PositionState::Open => Some(PosState::Open),
        PositionState::Exiting => Some(PosState::Exiting),
        PositionState::Closed => Some(PosState::Closed),
        PositionState::Pending => None,
    }
}

impl Cold {
    fn new(p: ColdParts) -> Self {
        let (e2_tx, e2_rx) = tokio::sync::mpsc::unbounded_channel();
        let (e1_tx, e1_rx) = tokio::sync::mpsc::unbounded_channel();
        let n = p.nonces.len();
        Self {
            p,
            lease: None,
            e2_tx,
            e2_rx,
            e2_pending: 0,
            e1_tx,
            e1_rx,
            snipers: Arc::new(Mutex::new(HashSet::new())),
            to_hot_q: VecDeque::new(),
            last_hb: 0,
            was_killed: false,
            armset_mtime: None,
            nonce_intent: vec![0; n],
            sender_gone: false,
        }
    }

    async fn run(mut self) {
        self.start_metrics().await;
        self.start_lease();
        self.write_nonce_rows(0);
        if let Some(a) = &self.p.cfg.armset {
            self.armset_mtime = std::fs::metadata(&a.path).and_then(|m| m.modified()).ok();
        }
        let mut iv = tokio::time::interval(Duration::from_millis(50));
        let mut n: u64 = 0;
        loop {
            iv.tick().await;
            n += 1;
            self.drain();
            if n.is_multiple_of(2) {
                self.kill_checks();
                self.nonce_keeper();
            }
            if n.is_multiple_of(20) {
                self.every_second();
            }
            if n.is_multiple_of(100) {
                self.every_5s();
            }
            if n.is_multiple_of(200) {
                self.armset_reload();
            }
            if self.p.shared.stop.load(Acquire) {
                // final drain once the hot loop and the sender dropped their ends
                for _ in 0..150 {
                    self.drain();
                    if self.p.from_hot.is_abandoned() && self.sender_gone {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                self.drain();
                self.every_5s();
                self.write_risk_row();
                if let Some((c, l, _)) = self.lease.as_mut() {
                    let _ = l.release(c);
                }
                if let Some(l) = &self.p.ledger {
                    l.flush();
                }
                return;
            }
        }
    }

    async fn start_metrics(&mut self) {
        for addr in self.p.cfg.metrics_listen.clone() {
            match tokio::net::TcpListener::bind(&addr).await {
                Ok(l) => {
                    lock(&self.p.report).metrics_listening.push(addr.clone());
                    let reg = self.p.registry.clone();
                    tokio::spawn(async move {
                        loop {
                            let Ok((mut s, _)) = l.accept().await else {
                                continue;
                            };
                            let reg = reg.clone();
                            tokio::spawn(async move {
                                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                                let mut req = [0u8; 2048];
                                let _ =
                                    tokio::time::timeout(Duration::from_secs(2), s.read(&mut req))
                                        .await;
                                let body = crate::stats::encode(&reg);
                                let head = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/openmetrics-text; version=1.0.0; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    body.len()
                                );
                                let _ = s.write_all(head.as_bytes()).await;
                                let _ = s.write_all(body.as_bytes()).await;
                                let _ = s.shutdown().await;
                            });
                        }
                    });
                }
                Err(e) => eprintln!("metrics: cannot listen on {addr}: {e} (skipped)"),
            }
        }
    }

    fn start_lease(&mut self) {
        let Some((path, owner, takeover)) = self.p.cfg.lease.clone() else {
            self.p.shared.exits_owned.store(true, Release);
            return;
        };
        let conn = match rugsnipe_state::store::open_conn(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("lease: cannot open {}: {e}", path.display());
                return;
            }
        };
        let mut lease = Lease::new("exits", &owner);
        let held = match lease.acquire(&conn, crate::now_ms(), takeover) {
            Ok(Acquire::Held { took_over_from, .. }) => {
                if let Some(o) = took_over_from {
                    eprintln!("lease: took the exits lease over from {o}");
                }
                true
            }
            Ok(Acquire::Busy(row)) => {
                eprintln!(
                    "lease: exits lease held by {} (heartbeat {} ms ago); sells suppressed until it is free",
                    row.owner,
                    row.age_ms(crate::now_ms())
                );
                false
            }
            Err(e) => {
                eprintln!("lease: {e}");
                false
            }
        };
        self.p.shared.exits_owned.store(held, Release);
        self.p.stats.lease_owned.set(held as i64);
        lock(&self.p.report).lease_held = held;
        self.lease = Some((conn, lease, takeover));
    }

    fn drain(&mut self) {
        while let Ok(m) = self.p.from_hot.pop() {
            self.on_msg(m);
        }
        loop {
            match self.p.from_sender.try_recv() {
                Ok(m) => self.on_msg(m),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.sender_gone = true;
                    break;
                }
            }
        }
        while let Ok((intent, mint, r, slot)) = self.e2_rx.try_recv() {
            self.e2_pending = self.e2_pending.saturating_sub(1);
            let msg = match r {
                Ok(e) => {
                    lock(&self.p.report).e2_resolved += 1;
                    HotIn::PaperFill {
                        intent,
                        mint,
                        v: e.v,
                        t: e.t,
                        slot: slot + 1,
                        buyers_ahead: e.buyers_ahead,
                        rival_sol_ahead: e.rival_sol_ahead,
                        ok: true,
                    }
                }
                Err(err) => {
                    let mut rep = lock(&self.p.report);
                    rep.e2_failed += 1;
                    if rep.e2_errors.len() < 8 {
                        rep.e2_errors.push(err);
                    }
                    HotIn::PaperFill {
                        intent,
                        mint,
                        v: 0,
                        t: 0,
                        slot,
                        buyers_ahead: 0,
                        rival_sol_ahead: 0,
                        ok: false,
                    }
                }
            };
            self.to_hot_q.push_back(msg);
        }
        while let Ok(r) = self.e1_rx.try_recv() {
            {
                let mut rep = lock(&self.p.report);
                rep.e1_resolved += 1;
                rep.e1_rpc_calls += r.calls as u64;
                if let Some(e) = &r.error {
                    if rep.e1_errors.len() < 8 {
                        rep.e1_errors.push(e.clone());
                    }
                }
            }
            self.to_hot_q.extend(e1_messages(&r));
        }
        // in order, as ring space allows (a full ring waits for the next drain)
        while let Some(m) = self.to_hot_q.front() {
            if self.p.to_hot.push(*m).is_err() {
                break;
            }
            self.to_hot_q.pop_front();
        }
    }

    fn mode_str(&self) -> &'static str {
        self.p.cfg.mode.as_str()
    }

    fn on_msg(&mut self, m: ColdMsg) {
        let ledger = self.p.ledger.clone();
        match m {
            ColdMsg::Intent {
                intent,
                mint,
                op_id,
                armset_ver,
                slot,
                bank_id,
                via,
                src,
                spend,
                v_max,
                min_out,
                nonce_idx,
                nonce_value,
                decided_ns,
                real,
            } => {
                lock(&self.p.report).intents += 1;
                if let Some(i) = self.nonce_intent.get_mut(nonce_idx as usize) {
                    *i = intent;
                }
                if let Some(l) = &ledger {
                    l.intent(IntentRow {
                        intent_id: intent as i64,
                        mint,
                        op_id,
                        box_id: self.p.cfg.box_id.clone(),
                        mode: self.mode_str().into(),
                        armset_ver,
                        slot,
                        bank_id,
                        via: match via {
                            2 => Via::FeePayer,
                            3 => Via::Funding,
                            4 => Via::Class,
                            _ => Via::Signer,
                        },
                        feed_class: src.name().into(),
                        spend,
                        v_max,
                        min_tokens_out: min_out,
                        nonce_idx,
                        nonce_value,
                        decided_ns,
                        state: if real {
                            IntentState::Sent
                        } else {
                            IntentState::Paper
                        },
                    });
                }
                self.write_nonce_row(nonce_idx as usize);
            }
            ColdMsg::Sent {
                intent,
                sig,
                variant,
                sender,
                prio,
                tip,
                sent_ns,
                result,
            } => {
                lock(&self.p.report).sends += 1;
                if let Some(l) = &ledger {
                    l.sent(SendRow {
                        sig,
                        intent_id: intent as i64,
                        variant,
                        sender: sender_name(sender).into(),
                        prio,
                        tip,
                        sent_ns,
                        result: match result {
                            0 => "recorded",
                            1 => "accepted",
                            _ => "failed",
                        }
                        .into(),
                    });
                }
            }
            ColdMsg::Fill {
                intent,
                mint,
                sig,
                slot,
                tokens,
                sol_delta,
                filled,
                late,
                buyers_ahead,
                rival_sol_ahead,
                fail,
                fail_cost,
            } => {
                {
                    let mut r = lock(&self.p.report);
                    if filled {
                        r.fills += 1;
                    } else {
                        r.unfilled += 1;
                    }
                }
                if let Some(l) = &ledger {
                    l.fill(
                        FillRow {
                            sig,
                            intent_id: intent as i64,
                            mint,
                            slot,
                            tx_index: None,
                            tokens,
                            sol_delta,
                            err: match (filled, fail) {
                                (true, _) => None,
                                (false, Some(f)) => Some(format!("failed: {}", f.as_str())),
                                (false, None) => Some("unfilled: E2 above v_max".to_string()),
                            },
                            buyers_ahead,
                            rival_sol_ahead,
                            late,
                            fail_reason: fail.map(|f| f.as_str().to_string()),
                            fail_cost,
                        },
                        None,
                    );
                }
            }
            ColdMsg::Position(p) => {
                let Some(state) = pos_state(p.state) else {
                    return;
                };
                if state == PosState::Open && p.next_order_id == 1 {
                    // the snapshot sent by `Opened` (no order issued yet)
                    lock(&self.p.report).positions_opened += 1;
                }
                if let Some(l) = &ledger {
                    l.position(PositionRow {
                        mint: p.mint,
                        intent_id: p.intent_id as i64,
                        opened_slot: p.opened_slot,
                        tokens: p.tokens,
                        cost: p.cost,
                        v_fill: p.v_fill,
                        peak_v: p.peak_v,
                        bundle_json: p.bundle_json(),
                        policy_json: p.policy_json(),
                        origin: match p.origin {
                            Some(rugsnipe_exit::Origin::Orphan) => Origin::Orphan,
                            _ => Origin::Intent,
                        },
                        state,
                        sell_spend: p.sell_spend,
                        closed_slot: p.closed_slot,
                    });
                }
            }
            ColdMsg::Exit {
                mint,
                sig,
                reason,
                venue,
                tokens,
                sol_delta,
                slot,
                owner,
                try_no,
            } => {
                {
                    let mut r = lock(&self.p.report);
                    r.exits += 1;
                    r.paper.on_exit(&reason.label());
                }
                if let Some(l) = &ledger {
                    l.exit(ExitRow {
                        sig,
                        mint,
                        reason: reason.label(),
                        venue: match venue {
                            Venue::Curve => "curve",
                            Venue::PumpSwap => "pumpswap",
                        }
                        .into(),
                        tokens,
                        sol_delta,
                        slot,
                        owner,
                        try_no,
                    });
                }
            }
            ColdMsg::WatchMint(m) => {
                lock(&self.p.report).watch_mint += 1;
                if let Some(h) = &self.p.logsws {
                    h.watch_mint(m);
                }
            }
            ColdMsg::UnwatchMint(m) => {
                lock(&self.p.report).unwatch_mint += 1;
                if let Some(h) = &self.p.logsws {
                    h.unwatch_mint(m);
                }
            }
            ColdMsg::PaperPending {
                intent,
                mint,
                bc,
                create_sig,
                slot,
                v0,
                t0,
                decided_unix_ms,
                creator,
                user,
                intx_buys,
            } => {
                if let Some(e1) = self.p.cfg.e1.clone() {
                    lock(&self.p.report).e1_requested += 1;
                    let tx = self.e1_tx.clone();
                    let snipers = self.snipers.clone();
                    let req = crate::e1::E1Req {
                        intent,
                        mint,
                        bc,
                        create_sig,
                        slot,
                        creator,
                        user,
                        decided_unix_ms,
                    };
                    tokio::spawn(async move {
                        let r = crate::e1::resolve(&e1, req, intx_buys as u32, snipers).await;
                        let _ = tx.send(r);
                    });
                    return;
                }
                let Some(e2) = self.p.cfg.e2.clone() else {
                    return;
                };
                lock(&self.p.report).e2_requested += 1;
                self.e2_pending += 1;
                let tx = self.e2_tx.clone();
                tokio::spawn(async move {
                    let r = crate::e2::resolve(
                        &e2.rpc_url,
                        e2.budget,
                        mint,
                        bc,
                        create_sig,
                        slot,
                        v0,
                        t0,
                        decided_unix_ms,
                        e2.delay_ms,
                        e2.max_tx,
                    )
                    .await;
                    let _ = tx.send((intent, mint, r, slot));
                });
            }
            ColdMsg::Funding(f) => {
                lock(&self.p.report).funding_events += 1;
                let now = crate::now_unix();
                if let Some(e) = self.p.watch.on_funding(
                    &f.funder,
                    &f.recipient,
                    f.lamports,
                    f.recipient_pre_lamports,
                    f.slot,
                    now,
                ) {
                    lock(&self.p.report).funded_entries += 1;
                    if let Some(l) = &ledger {
                        l.writer().send(Write::WatchDyn(watch_row(&e)));
                    }
                }
            }
            ColdMsg::BurnNonce { idx, intent } => {
                let i = idx as usize;
                if i < self.p.nonces.len()
                    && self.p.nonces.packed(i).state == NonceState::InFlight
                    && self.p.nonces.intent(i) == intent
                {
                    self.burn(i);
                }
            }
            ColdMsg::Closed { mint, pnl, .. } => {
                let mut r = lock(&self.p.report);
                r.positions_closed += 1;
                r.realized_lamports += pnl;
                r.paper.on_closed(&mint, pnl);
                r.paper.derive(self.p.cfg.round_trip_lamports);
            }
            ColdMsg::Shadow {
                intent,
                mint,
                e1,
                outcome,
                info,
                spend,
                cost,
            } => {
                {
                    let mut r = lock(&self.p.report);
                    r.paper
                        .on_shadow(mint, outcome, info.premium_bps, spend, cost);
                    r.paper.derive(self.p.cfg.round_trip_lamports);
                }
                if let Some(l) = &ledger {
                    l.shadow(ShadowRow {
                        intent_id: intent as i64,
                        mint,
                        model: if e1 { "e1" } else { "e2" }.into(),
                        outcome: outcome.as_str().into(),
                        create_slot: info.create_slot,
                        create_index: info.create_index,
                        block_txs: info.block_txs,
                        bundle_trades: info.bundle_trades,
                        bundle_wallets: info.bundle_wallets,
                        our_index: info.our_index,
                        tp_ahead: info.tp_ahead,
                        tp_behind: info.tp_behind,
                        s0_failed: info.s0_failed,
                        premium_bps: info.premium_bps,
                        post_bundle_v: info.post_bundle_v,
                        post_bundle_t: info.post_bundle_t,
                        post_bundle_mcap: info.post_bundle_mcap,
                        cost,
                    });
                }
            }
        }
    }

    /// Paper keeper: an AdvanceNonce-only burn that "lands" at once.
    fn burn(&mut self, i: usize) {
        let pool = &self.p.nonces;
        if !pool.start_burn(i) {
            return;
        }
        let slot = self.p.shared.slot_now.load(Relaxed);
        let prev = pool.value(i);
        let mut seed = prev;
        seed[..8].copy_from_slice(&slot.to_le_bytes());
        let value = next_value(&seed);
        if self.p.cfg.mode.is_real() {
            // Real modes: the keeper sends the burn; the account stream reports the value.
            return;
        }
        pool.on_value(i, value, slot);
        lock(&self.p.report).nonce_burns += 1;
        self.p.rslog.push(Rec::NonceEvt {
            idx: i as u16,
            from: NonceState::Burning as u8,
            to: NonceState::Free as u8,
            slot,
            value,
            intent: self.nonce_intent.get(i).copied().unwrap_or(0),
        });
        self.write_nonce_row(i);
    }

    fn nonce_keeper(&mut self) {
        let slot = self.p.shared.slot_now.load(Relaxed);
        if slot == 0 {
            return;
        }
        for i in self
            .p
            .nonces
            .due_for_burn(slot, self.p.cfg.burn_after_slots.max(1))
        {
            self.burn(i);
        }
    }

    fn write_nonce_row(&self, i: usize) {
        let Some(l) = &self.p.ledger else { return };
        let pool = &self.p.nonces;
        if i >= pool.len() {
            return;
        }
        let pk = pool.packed(i);
        let known = pk.last_change_slot != rugsnipe_tx::nonce::UNKNOWN_SLOT;
        l.nonce(NonceRow {
            pubkey: pool.account(i),
            pool: "buy",
            idx: i as u16,
            value: pool.value(i),
            last_change_slot: known.then_some(pk.last_change_slot),
            state: NonceRowState::from_pool(pk.state),
            intent_id: (pk.state == NonceState::InFlight).then(|| pool.intent(i) as i64),
            since_slot: None,
        });
    }

    fn write_nonce_rows(&self, _slot: u64) {
        for i in 0..self.p.nonces.len() {
            self.write_nonce_row(i);
        }
    }

    fn kill_checks(&mut self) {
        let kf = KillFile::new(&self.p.cfg.kill_file);
        kf.poll(&self.p.risk);
        if self.p.sigusr1.swap(false, AcqRel) {
            self.p.risk.kill(KillReason::Signal);
        }
        let killed = self.p.risk.is_killed();
        self.p.stats.killed.set(killed as i64);
        if killed && !self.was_killed {
            self.was_killed = true;
            let reason = self.p.risk.kill_reason().unwrap_or(KillReason::Manual);
            eprintln!(
                "KILL latch set ({reason:?}): buys stop, exits continue; unkill is local only"
            );
            self.p.rslog.push(Rec::Risk {
                kind: 1,
                value: reason as u8 as i64,
            });
            let mut r = lock(&self.p.report);
            r.killed = true;
            r.kill_reason = Some(format!("{reason:?}"));
            drop(r);
            self.write_risk_row();
        }
    }

    fn write_risk_row(&self) {
        let Some(l) = &self.p.ledger else { return };
        let r = lock(&self.p.report).clone();
        l.writer().send(Write::Risk(RiskRow {
            day: utc_day(crate::now_unix()),
            start_balance: self.p.cfg.paper_start_balance,
            deposits: 0,
            sweeps: 0,
            realized: r.realized_lamports,
            buys: r.intents,
            killed: r.killed,
            kill_reason: r.kill_reason.clone(),
        }));
    }

    fn every_second(&mut self) {
        let hb = self.p.shared.heartbeat.load(Relaxed);
        let alive = hb != self.last_hb;
        self.last_hb = hb;
        if let Some((c, l, takeover)) = self.lease.as_mut() {
            let now = crate::now_ms();
            if alive {
                let ok = if l.is_held() {
                    l.renew(c, now).unwrap_or(false)
                } else {
                    matches!(l.acquire(c, now, *takeover), Ok(Acquire::Held { .. }))
                };
                let was = self.p.shared.exits_owned.swap(ok, AcqRel);
                if was && !ok {
                    lock(&self.p.report).lease_lost += 1;
                    eprintln!("lease: exits lease lost (guard took over); sells suppressed");
                }
                self.p.stats.lease_owned.set(ok as i64);
                self.p.rslog.push(Rec::Lease {
                    name_hash: 0x6578_6974, // "exit"
                    owner: rugsnipe_core::rslog::box_id(&self.p.cfg.box_id),
                    epoch: l.epoch,
                    hb_unix_ms: now,
                });
            }
        }
        if alive {
            let _ = sd_notify::notify(&[sd_notify::NotifyState::Watchdog]);
        }
        let arm = self.p.arm.load();
        self.p.stats.armset_ver.set(arm.ver as i64);
        self.p
            .stats
            .armset_age_s
            .set(crate::now_unix().saturating_sub(arm.created_at) as i64);
        self.p
            .stats
            .watch_dynamic
            .set(self.p.watch.dynamic_entries().len() as i64);
        if let Some(l) = &self.p.ledger {
            let st = l.writer().stats();
            self.p
                .stats
                .state_written
                .set(st.written.load(Relaxed) as i64);
            self.p
                .stats
                .state_errors
                .set(st.errors.load(Relaxed) as i64);
        }
    }

    fn every_5s(&mut self) {
        for h in &self.p.healths {
            let s = h.snapshot();
            let i = s.src as usize;
            self.p.stats.feed_connected[i].set(s.connected as i64);
            self.p.stats.feed_decode_miss[i].set(s.decode_miss as i64);
            self.p.stats.feed_since_frame_ms[i].set(s.since_frame_ms.min(i64::MAX as u64) as i64);
            self.p.stats.feed_reconnects[i].set(s.reconnects as i64);
            let n = s.creates + s.decode_miss;
            let ppm = (s.decode_miss * 1_000_000).checked_div(n).unwrap_or(0) as u32;
            self.p.rslog.push(Rec::FeedHealth {
                src: s.src as u8,
                ok: (s.connected && !s.stale) as u8,
                lag_ms: s.since_frame_ms.min(u32::MAX as u64) as u32,
                decode_miss_ppm: ppm,
            });
            if n >= 500 && (s.decode_miss as f64 / n as f64) > self.p.cfg.decode_miss_kill_ratio {
                self.p.risk.kill(KillReason::DecodeMiss);
            }
        }
        let now = crate::now_unix();
        let gone = self.p.watch.expire(now);
        if let Some(l) = &self.p.ledger {
            for k in gone {
                l.writer().send(Write::WatchDynDelete(k));
            }
            for e in self.p.watch.dynamic_entries() {
                l.writer().send(Write::WatchDyn(watch_row(&e)));
            }
        }
        // a stale ArmSet keeps buys out of real modes (exits only)
        let arm = self.p.arm.load();
        if arm.verified && now.saturating_sub(arm.created_at) > self.p.cfg.armset_max_age_s {
            let mut a = (**arm).clone();
            a.verified = false;
            a.gate_passed = false;
            a.probe_allowed = false;
            self.p.arm.store(Arc::new(a));
            eprintln!("armset: v{} is stale: buys stay out of real modes", arm.ver);
        }
    }

    fn armset_reload(&mut self) {
        let Some(a) = self.p.cfg.armset.clone() else {
            return;
        };
        let m = std::fs::metadata(&a.path).and_then(|m| m.modified()).ok();
        if m.is_none() || m == self.armset_mtime {
            return;
        }
        self.armset_mtime = m;
        let r = load_armset_file(&a.path, &a.cfg, crate::now_unix(), a.regime.as_deref());
        match r.armset {
            Some(set) => {
                self.p.watch.load_armset(&set);
                self.p
                    .arm
                    .store(Arc::new(ArmState::from_armset(&set, true, &r.sha)));
                lock(&self.p.report).armset_reloads += 1;
                if let Some(l) = &self.p.ledger {
                    l.kv("armset_ver", set.ver.to_string());
                    l.kv("armset_sha", r.sha.clone());
                }
                eprintln!("armset: reloaded v{} ({})", set.ver, r.sha);
            }
            None => eprintln!(
                "armset: reload of {} refused ({:?}): keeping the previous set",
                a.path.display(),
                r.error
            ),
        }
    }
}

fn sender_name(id: u8) -> &'static str {
    match id {
        0 => "paper",
        1 => "rpcsend",
        2 => "helius",
        3 => "nozomi",
        4 => "astralane",
        5 => "jito",
        _ => "nextblock",
    }
}

fn watch_row(e: &rugsnipe_core::watchlist::DynEntry) -> WatchDynRow {
    WatchDynRow {
        key: e.key,
        kind: format!("{:?}", e.kind).to_lowercase(),
        op_id: e.op_id,
        rule_id: e.rule_id,
        added_slot: e.added_slot,
        expires_unix: e.expires_unix,
        remove_if_no_launch_unix: e.remove_if_no_launch_unix,
        keep: e.keep,
        parent: e.parent,
        state: if e.launched { "launched" } else { "active" }.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e1::{E1Req, E1Res, Slot0};
    use crate::e2::TradeObs;

    fn res(outcome: ShadowOutcome, slot0: Option<Slot0>, tape: Vec<TradeObs>) -> E1Res {
        E1Res {
            req: E1Req {
                intent: 7,
                mint: Pubkey::new_unique(),
                bc: Pubkey::new_unique(),
                create_sig: [1; 64],
                slot: 500,
                creator: Pubkey::new_unique(),
                user: Pubkey::new_unique(),
                decided_unix_ms: 0,
            },
            outcome,
            slot0,
            premium_bps: 110,
            s0_failed: 2,
            intx_trades: 1,
            tape,
            horizon: 512,
            calls: 9,
            error: None,
        }
    }

    #[test]
    fn e1_verdict_becomes_ordered_hot_messages() {
        let (b, c) = (Pubkey::new_unique(), Pubkey::new_unique());
        let s0 = Slot0 {
            create_index: 40,
            block_txs: 900,
            bundle_trades: 3,
            bundle_wallets: vec![b],
            creator_set: vec![c],
            our_index: 43,
            tp_behind: 2,
            ref_v: 33_000_000_000,
            ref_t: 975_000_000_000_000,
            ..Default::default()
        };
        let t = TradeObs {
            slot: 501,
            is_buy: true,
            sol: 1_000_000,
            tok: 10,
            v: 1,
            t: 2,
            user: b,
        };
        let m = e1_messages(&res(ShadowOutcome::Filled, Some(s0.clone()), vec![t, t]));
        assert_eq!(m.len(), 6);
        match m[0] {
            HotIn::E1Open { outcome, info, .. } => {
                assert_eq!(outcome, ShadowOutcome::Filled);
                assert_eq!(
                    (
                        info.create_index,
                        info.our_index,
                        info.bundle_wallets,
                        info.tp_behind
                    ),
                    (Some(40), Some(43), 1, 2)
                );
                assert_eq!((info.premium_bps, info.s0_failed), (110, 2));
                assert_eq!(info.post_bundle_mcap, s0.mcap());
            }
            x => panic!("{x:?}"),
        }
        assert!(matches!(m[1], HotIn::E1Insider { key, creator: false, .. } if key == b));
        assert!(matches!(m[2], HotIn::E1Insider { key, creator: true, .. } if key == c));
        assert!(
            matches!(m[3], HotIn::E1Trade { trade, .. } if trade.slot == 501 && trade.reserve_token == 2)
        );
        assert!(matches!(m[5], HotIn::E1End { horizon: 512, .. }));
        // a failure is one message; a fill without a creation slot is unresolved
        let f = e1_messages(&res(ShadowOutcome::Premium, Some(s0), vec![t]));
        assert_eq!(f.len(), 1);
        let u = e1_messages(&res(ShadowOutcome::Filled, None, vec![]));
        assert!(matches!(
            u[..],
            [HotIn::E1Open {
                outcome: ShadowOutcome::Unresolved,
                ..
            }]
        ));
    }

    #[test]
    fn paper_totals_count_failures_into_the_ev_per_attempt() {
        let mut p = PaperReport::default();
        let (a, b) = (Pubkey::new_unique(), Pubkey::new_unique());
        p.on_shadow(a, ShadowOutcome::Filled, 110, 500_000_000, 0);
        p.on_shadow(b, ShadowOutcome::Filled, 150, 500_000_000, 0);
        p.on_shadow(
            Pubkey::new_unique(),
            ShadowOutcome::Premium,
            340,
            500_000_000,
            3_000_000,
        );
        p.on_shadow(
            Pubkey::new_unique(),
            ShadowOutcome::Class,
            110,
            500_000_000,
            0,
        );
        p.on_shadow(
            Pubkey::new_unique(),
            ShadowOutcome::Unresolved,
            110,
            500_000_000,
            0,
        );
        p.on_exit("tp0");
        p.on_closed(&a, 50_000_000);
        p.on_closed(&Pubkey::new_unique(), 1); // not a paper fill of this session
        p.derive(4_000_000);
        assert_eq!(
            (p.attempts, p.fills, p.failed_attempts, p.in_class),
            (3, 2, 1, 3)
        );
        assert!((p.landing_rate - 2.0 / 3.0).abs() < 1e-12);
        assert_eq!(p.mean_premium_bps_fills, 130.0);
        assert_eq!(p.mean_premium_bps_attempts, 200.0);
        // b is still open: it counts in neither the numerator nor the denominator
        assert_eq!(p.closed, 1);
        assert_eq!(p.net_lamports, 50_000_000 - 4_000_000 - 3_000_000);
        assert!((p.return_per_fill_gross - 0.1).abs() < 1e-12);
        assert!((p.return_per_fill_net - 0.092).abs() < 1e-12);
        assert!((p.ev_per_attempt - 43_000_000.0 / 2.0 / 500_000_000.0).abs() < 1e-12);
        assert_eq!(p.exits_by_reason.get("tp0"), Some(&1));
    }
}
