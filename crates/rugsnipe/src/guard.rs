//! `rugsnipe-guard` (docs/06 section 8 "Guard"), skeleton: it watches the
//! `exits` lease in `state.db`. While the hot box's exit loop renews it the
//! guard idles; if the heartbeat is older than `takeover_after_ms` **and** a
//! position is open, it takes the lease over by compare-and-set (`epoch + 1`)
//! and sells on a >= `drop_from_peak_bps` drop from the peak price (price ∝
//! V², so `(v / peak_v)^2`), or after `grace_s` of holding the lease.
//!
//! `dry_run` (the default) only reports what it would sell; otherwise each
//! sell is patched into the final-sell template (dump floor bounds, the
//! dump bid split once by `BidPolicy::plan_sell`), signed and recorded by the
//! paper sender. In probe mode with [`GuardReal`] (`rugsnipe-guard
//! --real-sell`) the same sell goes on a recent blockhash to every real
//! sender, re-targeted per sender like the hot loop's
//! ([`crate::sender::plan_variants`]: the provider's tip account and
//! minimum tip), signed with the probe key.
//!
//! **A real sell is done only when the chain says so**: the mint stays
//! pending after a send and is re-sent every `retry_ms` on a fresh
//! blockhash (size from the chain balance) until the wallet's token balance
//! reads 0, then the position is closed in `state.db` (`positions.state =
//! 'closed'`, an `exits` row with the sale's proceeds); a zero balance the
//! guard did not cause (the hot box's sell landed after it exited) is booked
//! from the sale found on chain ([`GuardReal::find_sale`], reason
//! `guard_hot_sold`), and loudly as unknown when none is found; after
//! `max_sell_spend` lamports of bids the
//! guard stops retrying (`SellGaveUp`, a critical line). **The lease goes
//! back to the hot box** when its liveness heartbeat
//! ([`rugsnipe_state::lease::HOT_HEARTBEAT`]) is fresh again and no sell is
//! in flight, so a restarted hot box resumes its exits instead of finding
//! them "lost".
//!
//! **Handed-off positions are sold while the hot box is alive.** A
//! `guard_handoff:<mint>` row in `kv` (the hot loop's sell of that mint has
//! not landed 5 s after its first try) makes the guard sell that mint at
//! once, without the exits lease and whatever the heartbeat says, through
//! the same real sell path (re-sent every `retry_ms` until the balance
//! reads 0 or `max_sell_spend`); the exit is booked when the balance reads
//! 0 and the row is cleared (also when the position is already closed).

use pump_codec::constants::{TOKEN_2022_PROGRAM_ID, WSOL_MINT};
use pump_codec::curve::FeeBps;
use pump_codec::{pda, Pubkey};
use rugsnipe_exit::bounds::{dump_min_out, Market};
use rugsnipe_exit::{ExitPolicy, Venue};
use rugsnipe_state::lease::{Acquire, Lease};
use rugsnipe_state::recovery::load_open_positions;
use rugsnipe_state::rows::{ExitRow, PosState, PositionRow};
use rugsnipe_tx::senders::paper::Paper;
use rugsnipe_tx::senders::{Sender as _, Variant, VariantKind};
use rugsnipe_tx::signer::HotSigner;
use rugsnipe_tx::template::{MintKeys, Patch, Template};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Guard settings (`[guard]` plus the process identity).
#[derive(Debug, Clone)]
pub struct GuardCfg {
    pub owner: String,
    pub takeover_after_ms: u64,
    pub drop_from_peak_bps: u64,
    pub grace_s: u64,
    pub dry_run: bool,
    /// Real sells: re-send a sell not yet confirmed (balance > 0) after this long.
    pub retry_ms: u64,
    /// Real sells: stop retrying a position once its bids add up to this
    /// (`[risk] max_sell_spend_lamports`; 0 = no cap).
    pub max_sell_spend: u64,
}

/// What one guard step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardAction {
    /// Nothing to guard.
    Idle(&'static str),
    /// The hot box holds a fresh lease.
    HotAlive {
        owner: String,
        age_ms: u64,
    },
    TookOver {
        epoch: u64,
        from: Option<String>,
    },
    WouldSell {
        mint: Pubkey,
        tokens: u64,
        reason: &'static str,
    },
    Sold {
        mint: Pubkey,
        tokens: u64,
        reason: &'static str,
        sig: [u8; 64],
    },
    /// Real sell variants handed to the senders (`accepted` of `sent`);
    /// `try_no` counts from 1 (a retry re-sends on a fresh blockhash).
    SentReal {
        mint: Pubkey,
        tokens: u64,
        reason: &'static str,
        sent: usize,
        accepted: usize,
        try_no: u32,
    },
    /// The wallet holds none of the mint any more: the position is closed
    /// in `state.db`.
    Closed {
        mint: Pubkey,
        tries: u32,
    },
    /// The wallet holds none and the guard sent no sell: the sale was found
    /// on chain (the hot box sent it and exited before its fill) and booked
    /// at its proceeds (`exits` reason `guard_hot_sold`).
    ClosedSaleFound {
        mint: Pubkey,
        sig: [u8; 64],
        sol_delta: i64,
        slot: u64,
    },
    /// The wallet holds none, no sale was found on chain and none is booked:
    /// closed with the proceeds unknown (critical: the risk day counts the
    /// whole cost as lost until the exit is booked by hand).
    ClosedProceedsUnknown {
        mint: Pubkey,
    },
    /// The sell's bids reached `max_sell_spend` without the balance going
    /// to 0: no more retries (critical: sell by hand).
    SellGaveUp {
        mint: Pubkey,
        tries: u32,
        spent: u64,
    },
    /// The hot box's heartbeat is back: the exits lease is handed back.
    ReleasedToHot {
        owner: String,
    },
    /// A real sell could not be built (no recent blockhash): retried next step.
    SellDeferred {
        mint: Pubkey,
        why: &'static str,
    },
    /// A `guard_handoff:<mint>` row whose position is no longer open was
    /// removed.
    HandoffCleared {
        mint: Pubkey,
    },
    Released,
}

/// `kv` key prefix of a position the hot loop handed to the guard.
pub const HANDOFF_PREFIX: &str = "guard_handoff:";

/// Mints with a `guard_handoff:<mint>` row.
pub fn handoff_mints(c: &rusqlite::Connection) -> Result<HashSet<Pubkey>, String> {
    let mut st = c
        .prepare("SELECT k FROM kv WHERE k LIKE 'guard_handoff:%'")
        .map_err(|e| e.to_string())?;
    let keys = st
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = HashSet::new();
    for k in keys {
        let k = k.map_err(|e| e.to_string())?;
        if let Some(m) = k
            .strip_prefix(HANDOFF_PREFIX)
            .and_then(|m| m.parse::<Pubkey>().ok())
        {
            out.insert(m);
        }
    }
    Ok(out)
}

/// Remove `guard_handoff:<mint>` (no-op without one).
fn clear_handoff(c: &rusqlite::Connection, mint: &Pubkey) -> Result<(), String> {
    let w = rugsnipe_state::store::Write::KvDelete(format!("{HANDOFF_PREFIX}{mint}"));
    rugsnipe_state::store::apply(c, &w)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The paper sell path of a non-dry-run guard.
pub struct PaperSell {
    pub template: Template,
    pub signer: Arc<HotSigner>,
    pub paper: Arc<Paper>,
    pub tip_account: Pubkey,
    /// `[bid]`: the dump bid is split by leader class like every sell
    /// (the guard knows no leader: planned as priority-ordered).
    pub bid: rugsnipe_core::bid::BidPolicy,
    /// Probe: send for real (else paper only).
    pub real: Option<GuardReal>,
}

/// The real sell path of the guard (probe): the gated sender set and a
/// source of recent blockhashes.
pub struct GuardReal {
    pub senders: Arc<rugsnipe_tx::senders::SenderSet>,
    pub blockhash: Arc<dyn Fn() -> Option<[u8; 32]> + Send + Sync>,
    /// The wallet's token balance of a mint (`Some(0)` = none left, `None` =
    /// unknown). Without it a sell can never be confirmed: it is retried
    /// until `max_sell_spend`.
    pub token_balance: Option<TokenBalanceFn>,
    /// What our sell did to the wallet, fee included (`(lamports, slot)`),
    /// from its signature: the exit row then carries the proceeds the
    /// risk day is computed from (2026-10-23: a guard sale booked at 0
    /// counted the whole buy as lost and tripped the daily kill).
    pub proceeds: Option<ProceedsFn>,
    /// The newest sale of a mint by the wallet since a slot (`(sig,
    /// lamports, slot)`, [`rugsnipe_state::ChainRead::find_owner_sell`]):
    /// books a sale the guard did not send when the wallet holds none
    /// (2026-10-23: the hot box was restarted between its sell and the
    /// fill; the position was closed as a -100% trade).
    pub find_sale: Option<FindSaleFn>,
}

/// The wallet's token balance of a mint ([`GuardReal::token_balance`]).
pub type TokenBalanceFn = Arc<dyn Fn(&Pubkey) -> Option<u64> + Send + Sync>;
/// A sale's lamport delta and slot from its signature ([`GuardReal::proceeds`]).
pub type ProceedsFn = Arc<dyn Fn(&[u8; 64]) -> Option<(i64, u64)> + Send + Sync>;
/// The newest sale of `mint` since `since_slot` ([`GuardReal::find_sale`]).
pub type FindSaleFn = Arc<dyn Fn(&Pubkey, u64) -> Option<([u8; 64], i64, u64)> + Send + Sync>;

impl core::fmt::Debug for GuardReal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GuardReal")
            .field("senders", &self.senders)
            .finish_non_exhaustive()
    }
}

impl core::fmt::Debug for PaperSell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaperSell")
            .field("signer", &self.signer)
            .finish_non_exhaustive()
    }
}

/// A real sell sent but not yet confirmed by a zero balance.
#[derive(Debug, Clone)]
struct Pending {
    reason: &'static str,
    tries: u32,
    last_ms: u64,
    spent: u64,
    last_sig: [u8; 64],
    gave_up: bool,
}

/// The guard state machine.
#[derive(Debug)]
pub struct Guard {
    cfg: GuardCfg,
    lease: Lease,
    held_since_ms: Option<u64>,
    sell: Option<PaperSell>,
    sold: HashSet<Pubkey>,
    pending: HashMap<Pubkey, Pending>,
    policy: ExitPolicy,
}

impl Guard {
    pub fn new(cfg: GuardCfg, sell: Option<PaperSell>) -> Self {
        let lease = Lease::new("exits", &cfg.owner);
        Self {
            cfg,
            lease,
            held_since_ms: None,
            sell,
            sold: HashSet::new(),
            pending: HashMap::new(),
            policy: ExitPolicy::default(),
        }
    }

    pub fn holds_lease(&self) -> bool {
        self.lease.is_held()
    }

    /// Real sells sent and not yet confirmed (balance > 0), given-up ones included.
    pub fn pending_sells(&self) -> usize {
        self.pending.len()
    }

    /// The hot box's liveness heartbeat is fresh and newer than our takeover.
    fn hot_back(&self, c: &rusqlite::Connection, now_ms: u64) -> Result<Option<String>, String> {
        let row = rugsnipe_state::lease::read(c, rugsnipe_state::lease::HOT_HEARTBEAT)
            .map_err(|e| e.to_string())?;
        Ok(row
            .filter(|r| {
                r.owner != self.cfg.owner
                    && r.age_ms(now_ms) <= self.cfg.takeover_after_ms
                    && r.hb_unix_ms > self.held_since_ms.unwrap_or(u64::MAX)
            })
            .map(|r| r.owner))
    }

    /// The chain shows no tokens left: close the position in `state.db`,
    /// with an `exits` row carrying the sale's proceeds when the sale is
    /// known (ours by its signature, else looked up on chain).
    fn close(
        &mut self,
        c: &rusqlite::Connection,
        p: &PositionRow,
        owner: Pubkey,
    ) -> Result<GuardAction, String> {
        let pd = self.pending.remove(&p.mint);
        let real = match &self.sell {
            Some(PaperSell { real: Some(r), .. }) => Some(r),
            _ => None,
        };
        // a sale of the mint on chain not booked yet
        let found = || -> Result<Option<([u8; 64], i64, u64)>, String> {
            match real
                .and_then(|r| r.find_sale.as_ref())
                .and_then(|f| f(&p.mint, p.opened_slot))
            {
                Some(x) if !exit_booked(c, &x.0)? => Ok(Some(x)),
                _ => Ok(None),
            }
        };
        let exit_row = |sig, reason: String, sol_delta, slot, try_no| ExitRow {
            sig,
            mint: p.mint,
            reason,
            venue: "curve".into(),
            tokens: p.tokens,
            sol_delta,
            slot,
            owner,
            try_no,
        };
        let (exit, spent, action) = match &pd {
            Some(d) => {
                // the proceeds of our sale, from its signature; when they
                // cannot be read (an earlier try may be the one that
                // landed), the sale is looked up on chain
                let mut sig = d.last_sig;
                let mut got = match real.and_then(|r| r.proceeds.as_ref()) {
                    Some(f) if sig != [0; 64] => f(&sig),
                    _ => None,
                };
                if got.is_none() {
                    if let Some((s, delta, slot)) = found()? {
                        sig = s;
                        got = Some((delta, slot));
                    }
                }
                let (sol_delta, slot) = got.unwrap_or((0, 0));
                (
                    Some(exit_row(
                        sig,
                        format!("guard_{}", d.reason),
                        sol_delta,
                        slot,
                        d.tries,
                    )),
                    d.spent,
                    GuardAction::Closed {
                        mint: p.mint,
                        tries: d.tries,
                    },
                )
            }
            // no sell of ours: someone else's (the hot box's) emptied the wallet
            None => match found()? {
                Some((sig, sol_delta, slot)) => (
                    Some(exit_row(sig, "guard_hot_sold".into(), sol_delta, slot, 0)),
                    0,
                    GuardAction::ClosedSaleFound {
                        mint: p.mint,
                        sig,
                        sol_delta,
                        slot,
                    },
                ),
                // already booked (e.g. by the hot box's shutdown)
                None if mint_has_exit(c, &p.mint)? => (
                    None,
                    0,
                    GuardAction::Closed {
                        mint: p.mint,
                        tries: 0,
                    },
                ),
                None => (None, 0, GuardAction::ClosedProceedsUnknown { mint: p.mint }),
            },
        };
        let row = PositionRow {
            tokens: 0,
            state: PosState::Closed,
            sell_spend: p.sell_spend.saturating_add(spent),
            closed_slot: exit
                .as_ref()
                .map(|e| e.slot)
                .filter(|s| *s > 0)
                .or(p.closed_slot),
            ..p.clone()
        };
        let w = rugsnipe_state::store::Write::Position(row);
        rugsnipe_state::store::apply(c, &w).map_err(|e| e.to_string())?;
        if let Some(e) = exit {
            let e = rugsnipe_state::store::Write::Exit(e);
            rugsnipe_state::store::apply(c, &e).map_err(|e| e.to_string())?;
        }
        clear_handoff(c, &p.mint)?;
        self.sold.insert(p.mint);
        Ok(action)
    }

    /// One iteration. `price(mint)` = current virtual SOL of the curve, if known.
    pub fn step(
        &mut self,
        c: &rusqlite::Connection,
        now_ms: u64,
        price: &dyn Fn(&Pubkey) -> Option<u64>,
    ) -> Result<Vec<GuardAction>, String> {
        let positions = load_open_positions(c).map_err(|e| e.to_string())?;
        let mut out = vec![];
        let handoffs = handoff_mints(c)?;
        for m in &handoffs {
            if !positions.iter().any(|p| p.mint == *m) {
                clear_handoff(c, m)?;
                out.push(GuardAction::HandoffCleared { mint: *m });
            }
        }
        if positions.is_empty() {
            if self.lease.is_held() {
                self.lease.release(c).map_err(|e| e.to_string())?;
                self.held_since_ms = None;
                out.push(GuardAction::Released);
            } else {
                out.push(GuardAction::Idle("no open positions"));
            }
            return Ok(out);
        }
        if self.lease.is_held() && !self.lease.renew(c, now_ms).map_err(|e| e.to_string())? {
            self.held_since_ms = None;
        }
        if !self.lease.is_held() {
            match self
                .lease
                .acquire(c, now_ms, self.cfg.takeover_after_ms)
                .map_err(|e| e.to_string())?
            {
                Acquire::Held {
                    epoch,
                    took_over_from,
                } => {
                    self.held_since_ms = Some(now_ms);
                    out.push(GuardAction::TookOver {
                        epoch,
                        from: took_over_from,
                    });
                }
                Acquire::Busy(row) => {
                    out.push(GuardAction::HotAlive {
                        age_ms: row.age_ms(now_ms),
                        owner: row.owner,
                    });
                    // the hot box is alive but handed these over: sell them
                    // without the lease
                    if positions.iter().any(|p| handoffs.contains(&p.mint)) {
                        self.sell_loop(c, now_ms, price, positions, Some(&handoffs), &handoffs, 0, &mut out)?;
                    }
                    return Ok(out);
                }
            }
        }
        // the hot box is back: hand the exits over unless a sell is in flight
        if self.pending.values().all(|p| p.gave_up) {
            if let Some(owner) = self.hot_back(c, now_ms)? {
                self.lease.release(c).map_err(|e| e.to_string())?;
                self.held_since_ms = None;
                out.push(GuardAction::ReleasedToHot { owner });
                return Ok(out);
            }
        }
        let held_ms = now_ms.saturating_sub(self.held_since_ms.unwrap_or(now_ms));
        self.sell_loop(c, now_ms, price, positions, None, &handoffs, held_ms, &mut out)?;
        Ok(out)
    }

    /// The sell rules over `positions` (`only`: just these mints, the
    /// hand-offs sold while the hot box holds the lease); a mint in
    /// `handoffs` is sold at once (reason `handoff`).
    #[allow(clippy::too_many_arguments)]
    fn sell_loop(
        &mut self,
        c: &rusqlite::Connection,
        now_ms: u64,
        price: &dyn Fn(&Pubkey) -> Option<u64>,
        positions: Vec<PositionRow>,
        only: Option<&HashSet<Pubkey>>,
        handoffs: &HashSet<Pubkey>,
        held_ms: u64,
        out: &mut Vec<GuardAction>,
    ) -> Result<(), String> {
        let real = !self.cfg.dry_run && self.sell.as_ref().is_some_and(|s| s.real.is_some());
        for p in positions {
            if self.sold.contains(&p.mint) || p.tokens == 0 {
                continue;
            }
            if only.is_some_and(|o| !o.contains(&p.mint)) {
                continue;
            }
            // real: the chain balance decides (size, and whether it is sold)
            let (bal, wallet) = match self.sell.as_ref() {
                Some(s) if real => (
                    s.real
                        .as_ref()
                        .and_then(|r| r.token_balance.as_ref())
                        .and_then(|f| f(&p.mint)),
                    s.signer.pubkey(),
                ),
                _ => (None, Pubkey::default()),
            };
            if real && bal == Some(0) {
                out.push(self.close(c, &p, wallet)?);
                continue;
            }
            let retry = match self.pending.get_mut(&p.mint) {
                Some(d) if d.gave_up => continue,
                Some(d) if now_ms.saturating_sub(d.last_ms) < self.cfg.retry_ms => continue,
                Some(d) if self.cfg.max_sell_spend > 0 && d.spent >= self.cfg.max_sell_spend => {
                    d.gave_up = true;
                    out.push(GuardAction::SellGaveUp {
                        mint: p.mint,
                        tries: d.tries,
                        spent: d.spent,
                    });
                    continue;
                }
                Some(d) => Some(d.reason),
                None => None,
            };
            let v = price(&p.mint);
            let dropped = match v {
                Some(v) if p.peak_v > 0 => {
                    let m =
                        (v as u128 * v as u128 * 10_000) / (p.peak_v as u128 * p.peak_v as u128);
                    m <= 10_000u128.saturating_sub(self.cfg.drop_from_peak_bps as u128)
                }
                _ => false,
            };
            let reason = if let Some(r) = retry {
                r
            } else if handoffs.contains(&p.mint) {
                "handoff"
            } else if dropped {
                "drop_from_peak"
            } else if held_ms >= self.cfg.grace_s * 1_000 {
                "grace"
            } else {
                continue;
            };
            let tokens = bal.unwrap_or(p.tokens);
            if self.cfg.dry_run || self.sell.is_none() {
                self.sold.insert(p.mint);
                out.push(GuardAction::WouldSell {
                    mint: p.mint,
                    tokens: p.tokens,
                    reason,
                });
                continue;
            }
            let s = self.sell.as_mut().expect("checked");
            let lifetime = match &s.real {
                Some(r) => match (r.blockhash)() {
                    Some(h) => h,
                    None => {
                        out.push(GuardAction::SellDeferred {
                            mint: p.mint,
                            why: "no recent blockhash",
                        });
                        continue;
                    }
                },
                None => [0x22; 32],
            };
            if s.real.is_none() {
                // paper: nothing to confirm
                self.sold.insert(p.mint);
            }
            let owner = s.signer.pubkey();
            let bc = pda::bonding_curve(&p.mint);
            let keys = MintKeys::curve(
                p.mint,
                bc,
                pda::associated_bonding_curve(&p.mint, &TOKEN_2022_PROGRAM_ID),
                pda::ata_spl(&bc, &WSOL_MINT),
                pda::ata_token_2022(&owner, &p.mint),
            );
            let market = Market {
                venue: Venue::Curve,
                v: v.unwrap_or(p.v_fill),
                t: 0,
                creator: Pubkey::default(),
                fees: FeeBps::DEFAULT,
            };
            let min_out = dump_min_out(&self.policy, &market, tokens);
            // the bid once, through the bid policy (never as both fee and tip)
            let plan = s.bid.plan_sell(
                None,
                self.policy.dump_tip_lamports,
                s.template.table.n_sigs.clamp(1, 2) as u8,
            );
            s.template.patch(&Patch {
                keys: &keys,
                lifetime: &lifetime,
                amount: tokens,
                limit: min_out,
                priority_lamports: plan.priority_lamports,
                tip_lamports: plan.tip_lamports,
                cu_limit: None,
            });
            s.template.sign(&s.signer, None);
            let var = Variant::from_template(
                &s.template,
                p.intent_id as u64,
                0xC0,
                VariantKind::Sell,
                plan.priority_lamports,
                plan.tip_lamports,
                Some(s.tip_account),
            );
            let _ = s.paper.send(&var);
            if let Some(r) = &s.real {
                let job = crate::msgs::FireJob {
                    tmpl: s.template.clone(),
                    intent: p.intent_id as u64,
                    idx: 0xC0,
                    prio: plan.priority_lamports,
                    tip: plan.tip_lamports,
                    tip_account: s.tip_account,
                    bid: self.policy.dump_tip_lamports,
                    real: true,
                    leader: crate::leader::UNKNOWN,
                };
                let targets = r.senders.real();
                let vs =
                    crate::sender::plan_variants(std::slice::from_ref(&job), targets, &s.signer);
                let sent = vs.len();
                let accepted = vs
                    .iter()
                    .filter(|(k, v)| targets[*k].send(v).is_ok())
                    .count();
                let d = self.pending.entry(p.mint).or_insert(Pending {
                    reason,
                    tries: 0,
                    last_ms: now_ms,
                    spent: 0,
                    last_sig: var.sig,
                    gave_up: false,
                });
                d.tries += 1;
                d.last_ms = now_ms;
                d.last_sig = vs.first().map(|(_, v)| v.sig).unwrap_or(var.sig);
                // the most a landed try can cost (a dropped one costs nothing)
                d.spent = d
                    .spent
                    .saturating_add(plan.priority_lamports + plan.tip_lamports);
                out.push(GuardAction::SentReal {
                    mint: p.mint,
                    tokens,
                    reason,
                    sent,
                    accepted,
                    try_no: d.tries,
                });
                continue;
            }
            out.push(GuardAction::Sold {
                mint: p.mint,
                tokens,
                reason,
                sig: var.sig,
            });
        }
        Ok(())
    }
}

/// An `exits` row with this signature exists.
fn exit_booked(c: &rusqlite::Connection, sig: &[u8; 64]) -> Result<bool, String> {
    c.query_row(
        "SELECT COUNT(*) FROM exits WHERE sig = ?1",
        [rugsnipe_state::rows::b58(sig)],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .map_err(|e| e.to_string())
}

/// An `exits` row of this mint exists.
fn mint_has_exit(c: &rusqlite::Connection, mint: &Pubkey) -> Result<bool, String> {
    c.query_row(
        "SELECT COUNT(*) FROM exits WHERE mint = ?1",
        [mint.to_string()],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .map_err(|e| e.to_string())
}
