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
//! * paper E2 lane ([`crate::e2`]) inside the RPC budget;
//! * ArmSet reload on file change; feed health -> rslog + decode-miss kill;
//! * Prometheus text endpoint on the configured listen addresses.

use crate::hot::Shared;
use crate::msgs::{ColdMsg, HotIn};
use crate::setup::{load_armset_file, ArmState};
use crate::stats::HotStats;
use arc_swap::ArcSwap;
use prometheus_client::registry::Registry;
use pump_codec::Pubkey;
use rugsnipe_core::config::Config;
use rugsnipe_core::kill::KillFile;
use rugsnipe_core::risk::{KillReason, RiskGate};
use rugsnipe_core::rslog::{Rec, RslogProducer};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::Mode;
use rugsnipe_exit::{PositionState, Venue};
use rugsnipe_feed::FeedHealth;
use rugsnipe_state::lease::{Acquire, Lease};
use rugsnipe_state::rows::*;
use rugsnipe_state::{Ledger, RpcBudget, Write};
use rugsnipe_tx::nonce::{next_value, NoncePool, NonceState};
use serde::Serialize;
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
        let n = p.nonces.len();
        Self {
            p,
            lease: None,
            e2_tx,
            e2_rx,
            e2_pending: 0,
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
            let _ = self.p.to_hot.push(msg);
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
                            err: (!filled).then(|| "unfilled: E2 above v_max".to_string()),
                            buyers_ahead,
                            rival_sol_ahead,
                            late,
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
                lock(&self.p.report).exits += 1;
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
            } => {
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
            ColdMsg::Closed { pnl, .. } => {
                let mut r = lock(&self.p.report);
                r.positions_closed += 1;
                r.realized_lamports += pnl;
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
