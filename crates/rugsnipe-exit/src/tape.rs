//! Conformance tapes: a JSONL format for market histories and the
//! deterministic replay harness shared with the Python mirror (`outcome.py`).
//!
//! # Tape format (version 1)
//!
//! Line 1 is the header:
//!
//! ```json
//! {"kind":"header","version":1,"name":"...","mint":"<b58>","owner":"<b58>","creator":"<b58>",
//!  "bundle":["<b58>",...],"supply":1000000000000000,"fees":{"protocol":95,"creator":30},
//!  "policy":{...ExitPolicy, integer form...},"source":"..."}
//! ```
//!
//! Every following line is one event, in chain order:
//!
//! | kind | fields |
//! |---|---|
//! | `trade` | `slot, tx_index, ts_ms, sig, user, is_buy, sol_amount, token_amount, fee, virtual_sol_reserves, virtual_token_reserves, own, venue?` (post-trade reserves, a pump `TradeEvent`; `own: true` marks our fill, `venue` defaults to `curve`) |
//! | `transfer` | `slot, ts_ms, from_owner, to_owner, amount` |
//! | `balance` | `slot, ts_ms, tokens` |
//! | `complete` | `slot, ts_ms` |
//! | `pool` | `slot, ts_ms, reserve_sol, reserve_token` |
//! | `tick` | `slot, ts_ms` |
//!
//! # Replay harness (normative; the Python mirror implements the same steps)
//!
//! 1. `pos = Position::pending(mint, owner, intent_id = 1, policy, bundle, creator, fees, supply)`.
//! 2. Before each tape event at slot `s`, a `Tick` is run for every skipped slot
//!    `last+1 .. s-1` at `ms = min(last_ms + (k - last) * 400, event.ts_ms)`.
//!    `now = {ms: max(previous ms, event ms), slot: s}`.
//! 3. **Simulated fills.** At each step, before the event, every sell attempt
//!    emitted at an earlier slot gets exactly one chance to fill, in `order_id`
//!    order, at the current reserves of the position: it fills iff the balance
//!    covers it and the quote is `>= min_sol_output`; a fill is fed back as our
//!    own sell `Trade` (gross `sol_amount`, `fee`, post-sale reserves) and its
//!    actions are recorded. An attempt that does not fill expires (the engine
//!    retries on its schedule).
//! 4. The event is decided and every action recorded as
//!    `{"step","slot","ms","action":{"type":...,<ExitAction fields>}}` (`step`
//!    counts decisions, simulated fills included).
//! 5. After the last event, ticks continue slot by slot (400 ms each) for up to
//!    20,000 slots or until the position closes. The last line is
//!    `{"final":{...}}`.
//!
//! Our buy is a price taker: later tape events keep their observed reserves.

use crate::bounds::Market;
use crate::decide::decide;
use crate::policy::ExitPolicy;
use crate::position::{BundleSet, Position, PositionState};
use crate::types::{b58, ExitAction, ExitEvent, Now, SellOrder, Trade, TransferEvent, Venue};
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::Pubkey;
use serde::{Deserialize, Serialize};

/// Observer called after every decision with the position and the actions.
pub type Observer = Box<dyn FnMut(&Position, &[ExitAction])>;

/// Slot duration used to time synthesized ticks.
pub const SLOT_MS: u64 = 400;
/// Ticks run after the last tape event.
pub const TAIL_SLOTS: u64 = 20_000;

/// Curve fee rates in the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapeFees {
    pub protocol: u64,
    pub creator: u64,
}

/// Tape header (first line).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapeHeader {
    pub kind: String,
    pub version: u32,
    pub name: String,
    #[serde(with = "b58")]
    pub mint: Pubkey,
    #[serde(with = "b58")]
    pub owner: Pubkey,
    #[serde(with = "b58")]
    pub creator: Pubkey,
    pub bundle: BundleSet,
    pub supply: u64,
    pub fees: TapeFees,
    pub policy: ExitPolicy,
    #[serde(default)]
    pub source: String,
}

fn curve() -> Venue {
    Venue::Curve
}

/// One tape event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TapeEvent {
    Trade {
        slot: u64,
        #[serde(default)]
        tx_index: u32,
        ts_ms: u64,
        #[serde(default)]
        sig: String,
        #[serde(with = "b58")]
        user: Pubkey,
        is_buy: bool,
        sol_amount: u64,
        token_amount: u64,
        #[serde(default)]
        fee: u64,
        virtual_sol_reserves: u64,
        virtual_token_reserves: u64,
        #[serde(default)]
        own: bool,
        #[serde(default = "curve")]
        venue: Venue,
    },
    Transfer {
        slot: u64,
        ts_ms: u64,
        #[serde(with = "b58")]
        from_owner: Pubkey,
        #[serde(with = "b58")]
        to_owner: Pubkey,
        amount: u64,
    },
    Balance {
        slot: u64,
        ts_ms: u64,
        tokens: u64,
    },
    Complete {
        slot: u64,
        ts_ms: u64,
    },
    Pool {
        slot: u64,
        ts_ms: u64,
        reserve_sol: u64,
        reserve_token: u64,
    },
    Tick {
        slot: u64,
        ts_ms: u64,
    },
}

impl TapeEvent {
    pub fn slot(&self) -> u64 {
        match self {
            TapeEvent::Trade { slot, .. }
            | TapeEvent::Transfer { slot, .. }
            | TapeEvent::Balance { slot, .. }
            | TapeEvent::Complete { slot, .. }
            | TapeEvent::Pool { slot, .. }
            | TapeEvent::Tick { slot, .. } => *slot,
        }
    }

    pub fn ts_ms(&self) -> u64 {
        match self {
            TapeEvent::Trade { ts_ms, .. }
            | TapeEvent::Transfer { ts_ms, .. }
            | TapeEvent::Balance { ts_ms, .. }
            | TapeEvent::Complete { ts_ms, .. }
            | TapeEvent::Pool { ts_ms, .. }
            | TapeEvent::Tick { ts_ms, .. } => *ts_ms,
        }
    }

    /// The engine event for `mint`.
    pub fn to_exit_event(&self, mint: Pubkey) -> ExitEvent {
        match *self {
            TapeEvent::Trade {
                slot,
                user,
                is_buy,
                sol_amount,
                token_amount,
                fee,
                virtual_sol_reserves,
                virtual_token_reserves,
                venue,
                ..
            } => ExitEvent::Trade(Trade {
                venue,
                mint,
                user,
                is_buy,
                sol_amount,
                token_amount,
                fee_lamports: fee,
                reserve_sol: virtual_sol_reserves,
                reserve_token: virtual_token_reserves,
                slot,
            }),
            TapeEvent::Transfer {
                slot,
                from_owner,
                to_owner,
                amount,
                ..
            } => ExitEvent::Transfer(TransferEvent {
                mint,
                from_owner,
                to_owner,
                amount,
                slot,
            }),
            TapeEvent::Balance { slot, tokens, .. } => ExitEvent::Balance { tokens, slot },
            TapeEvent::Complete { slot, .. } => ExitEvent::Complete { slot },
            TapeEvent::Pool {
                slot,
                reserve_sol,
                reserve_token,
                ..
            } => ExitEvent::PoolState {
                reserve_sol,
                reserve_token,
                slot,
            },
            TapeEvent::Tick { .. } => ExitEvent::Tick,
        }
    }
}

/// A parsed tape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tape {
    pub header: TapeHeader,
    pub events: Vec<TapeEvent>,
}

impl Tape {
    /// Parse JSONL text.
    pub fn parse(text: &str) -> Result<Tape, String> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let first = lines.next().ok_or("empty tape")?;
        let header: TapeHeader = serde_json::from_str(first).map_err(|e| format!("header: {e}"))?;
        if header.kind != "header" || header.version != 1 {
            return Err("not a version 1 tape header".into());
        }
        let mut events = Vec::new();
        for (i, l) in lines.enumerate() {
            events.push(serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 2))?);
        }
        Ok(Tape { header, events })
    }

    /// Serialize back to JSONL.
    pub fn to_jsonl(&self) -> String {
        let mut s = serde_json::to_string(&self.header).unwrap_or_default();
        s.push('\n');
        for e in &self.events {
            s.push_str(&serde_json::to_string(e).unwrap_or_default());
            s.push('\n');
        }
        s
    }

    /// The pending position the harness starts from.
    pub fn position(&self) -> Position {
        let h = &self.header;
        Position::pending(
            h.mint,
            h.owner,
            1,
            h.policy,
            h.bundle,
            h.creator,
            FeeBps {
                protocol: h.fees.protocol,
                creator: h.fees.creator,
            },
            h.supply,
        )
    }
}

/// One recorded decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub step: u64,
    pub slot: u64,
    pub ms: u64,
    pub action: ExitAction,
}

/// End state of a replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub state: PositionState,
    pub tokens: u64,
    pub cost: u64,
    pub proceeds: u64,
    pub realized_lamports: i64,
    pub sell_spend: u64,
    pub closed_slot: Option<u64>,
    pub steps: u64,
    pub fills: u64,
}

/// Replay result.
#[derive(Debug, Clone)]
pub struct ReplayOut {
    pub records: Vec<Record>,
    pub summary: Summary,
    pub position: Position,
}

impl ReplayOut {
    /// The expected-output JSONL (records, then `{"final":...}`).
    pub fn to_jsonl(&self) -> String {
        let mut s = String::new();
        for r in &self.records {
            s.push_str(&serde_json::to_string(r).unwrap_or_default());
            s.push('\n');
        }
        s.push_str(
            &serde_json::to_string(&serde_json::json!({ "final": self.summary }))
                .unwrap_or_default(),
        );
        s.push('\n');
        s
    }
}

struct Harness {
    pos: Position,
    step: u64,
    fills: u64,
    records: Vec<Record>,
    /// Attempts waiting for their one fill chance: (emitted slot, order).
    attempts: Vec<(u64, SellOrder)>,
    /// Optional observer of every (position, actions) pair, e.g. invariant checks.
    observer: Option<Observer>,
}

impl Harness {
    fn run(&mut self, ev: ExitEvent, now: Now) {
        self.try_fills(now);
        self.apply(ev, now);
    }

    fn apply(&mut self, ev: ExitEvent, now: Now) {
        let actions = decide(&mut self.pos, &ev, now);
        self.step += 1;
        if let Some(obs) = self.observer.as_mut() {
            obs(&self.pos, &actions);
        }
        for a in actions {
            match a {
                ExitAction::Sell(o) => {
                    self.attempts.retain(|(_, x)| x.order_id != o.order_id);
                    self.attempts.push((now.slot, o));
                }
                ExitAction::Closed { .. } => self.attempts.clear(),
                _ => {}
            }
            self.records.push(Record {
                step: self.step,
                slot: now.slot,
                ms: now.ms,
                action: a,
            });
        }
    }

    fn try_fills(&mut self, now: Now) {
        let mut due: Vec<SellOrder> = Vec::new();
        self.attempts.retain(|(s, o)| {
            if *s < now.slot {
                due.push(*o);
                false
            } else {
                true
            }
        });
        due.sort_by_key(|o| o.order_id);
        for o in due {
            if !self.pos.state.is_active() || self.pos.tokens < o.tokens || o.tokens == 0 {
                continue;
            }
            let p = &self.pos;
            let (gross, net, post_v, post_t) = match p.venue {
                Venue::Curve => {
                    let s = CurveState {
                        virtual_sol: p.cur_v,
                        virtual_token: p.cur_t,
                        real_sol: 0,
                        real_token: 0,
                        complete: false,
                        creator: p.creator,
                    };
                    match s.quote_sell(o.tokens, p.fees()) {
                        Some(q) => (
                            q.sol_out,
                            q.net,
                            p.cur_v.saturating_sub(q.sol_out),
                            p.cur_t.saturating_add(o.tokens),
                        ),
                        None => continue,
                    }
                }
                Venue::PumpSwap => {
                    let m: Market = p.market();
                    let net = m.sell_net(o.tokens, p.policy.amm_fee_bps);
                    let gross = crate::bounds::amm_sell_net(o.tokens, p.cur_v, p.cur_t, 0);
                    (
                        gross,
                        net,
                        p.cur_v.saturating_sub(gross),
                        p.cur_t.saturating_add(o.tokens),
                    )
                }
            };
            if net < o.min_sol_output {
                continue;
            }
            self.fills += 1;
            let fill = ExitEvent::Trade(Trade {
                venue: p.venue,
                mint: p.mint,
                user: p.owner,
                is_buy: false,
                sol_amount: gross,
                token_amount: o.tokens,
                fee_lamports: gross - net,
                reserve_sol: post_v,
                reserve_token: post_t,
                slot: now.slot,
            });
            self.apply(fill, now);
        }
    }
}

/// Replay a tape through [`decide`] with the harness rules above.
pub fn replay(tape: &Tape) -> ReplayOut {
    replay_observed(tape, None)
}

/// [`replay`] calling `observer` after every decision (used by property tests).
pub fn replay_observed(tape: &Tape, observer: Option<Observer>) -> ReplayOut {
    let mut h = Harness {
        pos: tape.position(),
        step: 0,
        fills: 0,
        records: Vec::new(),
        attempts: Vec::new(),
        observer,
    };
    let mint = tape.header.mint;
    let mut last: Option<(u64, u64)> = None; // (slot, ms)
    let mut ms_floor = 0u64;
    for ev in &tape.events {
        let (s, ms) = (ev.slot(), ev.ts_ms());
        if let Some((ls, lms)) = last {
            let mut k = ls + 1;
            while k < s && h.pos.state != PositionState::Closed {
                let tms = (lms + (k - ls) * SLOT_MS).min(ms).max(ms_floor);
                ms_floor = tms;
                h.run(ExitEvent::Tick, Now { ms: tms, slot: k });
                k += 1;
            }
        }
        if h.pos.state == PositionState::Closed {
            break;
        }
        let now = Now {
            ms: ms.max(ms_floor),
            slot: s,
        };
        ms_floor = now.ms;
        h.run(ev.to_exit_event(mint), now);
        last = Some((s.max(last.map_or(0, |l| l.0)), now.ms));
    }
    if let Some((ls, lms)) = last {
        let mut k = 1;
        while k <= TAIL_SLOTS && h.pos.state != PositionState::Closed {
            h.run(
                ExitEvent::Tick,
                Now {
                    ms: lms + k * SLOT_MS,
                    slot: ls + k,
                },
            );
            k += 1;
        }
    }
    let p = h.pos;
    ReplayOut {
        summary: Summary {
            state: p.state,
            tokens: p.tokens,
            cost: p.cost,
            proceeds: p.proceeds,
            realized_lamports: p.realized_lamports() as i64,
            sell_spend: p.sell_spend,
            closed_slot: p.closed_slot,
            steps: h.step,
            fills: h.fills,
        },
        records: h.records,
        position: p,
    }
}
