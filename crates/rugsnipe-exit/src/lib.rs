//! # rugsnipe-exit
//!
//! The exit engine of `rugsnipe` (docs/06-architecture.md section 8): a
//! position state machine, the exit triggers, sell bounds, venue routing and
//! the sell retry schedule, all behind one **pure, deterministic** function:
//!
//! ```text
//! decide(&mut Position, &ExitEvent, Now) -> Vec<ExitAction>
//! ```
//!
//! No I/O, no clock reads, no floating point: every ratio is in basis points
//! and every amount in lamports or token base units, so the Python backtest
//! (`outcome.py`) can mirror it bit for bit. Conformance runs on JSONL tapes
//! ([`tape`]): 20 tapes built from real pump.fun trade histories
//! (`fixtures/tapes/`, `scripts/pumpapi_tapes.py`) and 12 seeded synthetic
//! ones for retries, escalation, transfers and migration
//! (`fixtures/tapes_synth/`), with checked-in expected outputs that
//! `scripts/exit_mirror.py`, an integer-only Python port of [`decide`] and the
//! harness, reproduces exactly.
//!
//! ## Position lifecycle
//!
//! `Pending -> Open -> Exiting <-> Open -> Closed`. A position opens **only**
//! on our own buy `TradeEvent` ([`Origin::Intent`]) or a non-zero chain
//! balance ([`Origin::Orphan`]), never on a send. Size always comes from the
//! chain ([`ExitEvent::Balance`] resyncs it). The [`Position`] fields mirror
//! the `positions` table of docs/06 section 5.1.
//!
//! ## Triggers (docs/06 section 8, defaults from section 6 `[exit.default]`)
//!
//! | # | trigger | rule | kind |
//! |---|---|---|---|
//! | 1 | BundleSell | a bundle wallet sells >= `bundle_sell_min` -> 100% | dump, immediate |
//! | 2 | Consolidation | transfers into one owner within 2 slots >= `consolidation_pct` of supply -> 100% | dump, immediate |
//! | 3 | BigBuy | one buy within `[lo, hi]` -> 100% | dump, immediate |
//! | 4 | TakeProfit | up to 5 `(mult, fraction)` levels, partial sells of the remainder | normal, slot close |
//! | 5 | Trailing | after `activate`, price <= peak x (1 - retrace) | normal, slot close |
//! | 6 | PhantomSL | after `after_ms`, mult <= 1 - bps | normal, slot close |
//! | 7 | NoActivity / MaxHold | no trade for 150 s / held 30 min | normal, slot close |
//! | 8 | Completion | venue -> PumpSwap; in-flight orders re-sent on the pool | venue |
//!
//! Lead-policy knobs (docs/04 sections 2 and 6; all off by default, so the
//! table above and the conformance tapes are unchanged): `tp_basis = "value"`
//! (take-profit on the realised value of the stake after fees and our own
//! impact), `fire_on = "trade"` (normal triggers also on the first trade that
//! shows them), `insider_sell` (whose sells are a BundleSell),
//! `phantom_sl = { after_ms, drop }` (stop measured from the post-bundle
//! price, marked without our own tokens) and `max_hold_slots` (hard exit at
//! landing slot + N).
//!
//! Price multiple: `mult = (v / v_fill)^2` from the curve's virtual SOL
//! ([`bounds::mult_bps`]). Bounds: normal `min_sol_output = quote_sell(tokens,
//! V_trigger) x (1 - tp_slip)`; dump `quote_sell(tokens, V = 30 SOL) x 0.98`
//! ([`bounds`]). Retries every 2 slots with the tip x2, up to `max_tries` and
//! the per-position sell-spend cap, then [`Alert::SellExhausted`].
//!
//! Normal triggers are evaluated once per slot, on the slot's closing state,
//! while dump triggers act on the event that shows them: a dump and a
//! take-profit in the same slot always produce the dump first and the
//! take-profit never. The first dump signal also emits
//! [`ExitAction::DumpSignal`] so the keeper burns an in-flight buy nonce
//! (docs/06 section 7.3), even before the position opens.
//!
//! ## Example
//!
//! ```
//! use rugsnipe_exit::*;
//! use pump_codec::{curve::FeeBps, Pubkey};
//! let (mint, me, dev) = (Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique());
//! let mut pos = Position::pending(mint, me, 1, ExitPolicy::default(),
//!     BundleSet::new(&[dev]), dev, FeeBps::DEFAULT, 1_000_000_000_000_000);
//! let fill = Trade { venue: Venue::Curve, mint, user: me, is_buy: true, sol_amount: 98_000_000,
//!     token_amount: 3_000_000_000_000, fee_lamports: 1_225_000, reserve_sol: 40_000_000_000,
//!     reserve_token: 804_750_000_000_000, slot: 100 };
//! let acts = decide(&mut pos, &ExitEvent::Trade(fill), Now { ms: 0, slot: 100 });
//! assert!(matches!(acts[0], ExitAction::Opened { .. }));
//! // the dev dumps 1 SOL worth: sell everything at floor bounds
//! let dump = Trade { user: dev, is_buy: false, sol_amount: 1_000_000_000, ..fill };
//! let acts = decide(&mut pos, &ExitEvent::Trade(dump), Now { ms: 400, slot: 101 });
//! assert!(matches!(acts[0], ExitAction::DumpSignal { reason: ExitReason::BundleSell, .. }));
//! assert!(matches!(acts[1], ExitAction::Sell(SellOrder { kind: SellKind::Dump, .. })));
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod bounds;
pub mod decide;
pub mod policy;
pub mod position;
pub mod tape;
pub mod types;

pub use decide::{decide, decide_into, unsent};
pub use policy::{
    ExitConfig, ExitOverrides, ExitPolicy, FireOn, InsiderSell, PhantomCfg, PhantomFrom,
    PolicyError, TpBasis, TpLadder, TpLevel,
};
pub use position::{BundleSet, PendingSell, Position, PositionState};
pub use types::{
    Alert, CloseCause, ExitAction, ExitEvent, ExitReason, Now, Origin, SellKind, SellOrder, Trade,
    TransferEvent, Venue,
};
