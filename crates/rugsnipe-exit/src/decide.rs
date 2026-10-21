//! The pure decision function.
//!
//! Order of work for one event (all integer math, no I/O, no clock reads):
//!
//! 1. **Slot close.** If the event's slot is past `cur_slot`, the normal
//!    triggers (take-profit, trailing, phantom SL, no-activity, max-hold) are
//!    evaluated once on the *closing* state of `cur_slot`. Dump triggers act
//!    immediately on the event that shows them. Hence a dump and a
//!    take-profit that fire in the same slot always yield the dump first and
//!    the take-profit never (the position is already exiting at dump bounds).
//! 2. **Retries.** Orders whose last attempt is `every_slots` old are re-sent
//!    with the tip multiplied by `tip_mult`, until `max_tries` or the per-position
//!    sell-spend cap, then a critical alert. Normal orders escalate to dump
//!    bounds after `escalate_after_tries` attempts or on a dump signal.
//! 3. **The event.** Fills (our own trades, balances) move the state machine;
//!    other trades and transfers feed the dump detectors and the price.
//!
//! Invariant (property-tested): the tokens of all orders in flight never
//! exceed the chain balance, and every new order fits in the uncommitted balance.

use crate::bounds::{dump_min_out, normal_min_out, pool_from_curve, Market};
use crate::policy::{FireOn, InsiderSell, PhantomFrom, TpBasis, BPS};
use crate::position::{PendingSell, Position, PositionState, XferRec, XFER_RING};
use crate::types::{
    Alert, CloseCause, ExitAction, ExitEvent, ExitReason, Now, Origin, SellKind, SellOrder, Trade,
    TransferEvent, Venue,
};

/// Attempts after which a still-failing sell raises the "sell failing" alert.
pub const FAILING_ALERT_AFTER: u32 = 5;

/// Decide what to do about `ev` at `now`. Mutates only `pos`; returns the
/// actions in the order they must be executed.
pub fn decide(pos: &mut Position, ev: &ExitEvent, now: Now) -> Vec<ExitAction> {
    let mut out = Vec::new();
    decide_into(pos, ev, now, &mut out);
    out
}

/// [`decide`] appending into a caller-owned buffer (no allocation once it has capacity).
pub fn decide_into(pos: &mut Position, ev: &ExitEvent, now: Now, out: &mut Vec<ExitAction>) {
    if pos.state == PositionState::Closed {
        return;
    }
    match ev {
        ExitEvent::Trade(t) if t.mint != pos.mint => return,
        ExitEvent::Transfer(x) if x.mint != pos.mint => return,
        _ => {}
    }
    let slot = now.slot.max(ev.slot().unwrap_or(0));

    // 1. slot close
    if slot > pos.cur_slot {
        if pos.state.is_active() {
            eval_normal(pos, slot, now, out);
        }
        pos.cur_slot = slot;
    }

    // 2. retries due
    for i in 0..pos.pending.len() {
        let Some(p) = pos.pending[i] else { continue };
        if !p.exhausted && slot >= p.sent_slot.saturating_add(pos.policy.retry_every_slots) {
            reissue(pos, i, slot, None, out);
        }
    }

    // 3. the event
    match *ev {
        ExitEvent::Trade(t) => on_trade(pos, &t, slot, now, out),
        ExitEvent::Balance { tokens, .. } => on_balance(pos, tokens, slot, now, out),
        ExitEvent::Transfer(x) => on_transfer(pos, &x, slot, now, out),
        ExitEvent::Complete { .. } => on_complete(pos, slot, out),
        ExitEvent::PoolState {
            reserve_sol,
            reserve_token,
            ..
        } => {
            if pos.venue == Venue::PumpSwap {
                pos.cur_v = reserve_sol;
                pos.cur_t = reserve_token;
                pos.pool_known = true;
                update_peak(pos);
            }
        }
        ExitEvent::SellFailed { order_id } => {
            if let Some(i) = pos
                .pending
                .iter()
                .position(|p| p.is_some_and(|p| p.order.order_id == order_id && !p.exhausted))
            {
                reissue(pos, i, slot, None, out);
            }
        }
        ExitEvent::Tick => {}
    }
}

// ------------------------------------------------------------------ events
fn on_trade(pos: &mut Position, t: &Trade, slot: u64, now: Now, out: &mut Vec<ExitAction>) {
    if t.venue == pos.venue {
        pos.cur_v = t.reserve_sol;
        pos.cur_t = t.reserve_token;
        if t.venue == Venue::PumpSwap {
            pos.pool_known = true;
        }
    }
    let own = t.user == pos.owner;
    if own {
        if t.is_buy {
            on_own_buy(pos, t, slot, now, out);
        } else {
            on_own_sell(pos, t, slot, now, out);
        }
        return;
    }
    pos.last_trade_ms = pos.last_trade_ms.max(now.ms);
    if pos.state.is_active() && pos.v_fill == 0 {
        // orphan opened before any price was seen
        pos.v_fill = pos.cur_v;
        pos.t_fill = pos.cur_t;
    }
    update_peak(pos);
    let p = pos.policy;
    if !t.is_buy
        && p.insider_sell != InsiderSell::Off
        && t.sol_amount >= p.bundle_sell_min_lamports
        && pos.bundle.contains(&t.user)
    {
        dump(pos, ExitReason::BundleSell, slot, now, out);
    } else if t.is_buy
        && p.big_buy_lo_lamports > 0
        && t.sol_amount >= p.big_buy_lo_lamports
        && (p.big_buy_hi_lamports == 0 || t.sol_amount <= p.big_buy_hi_lamports)
    {
        dump(pos, ExitReason::BigBuy, slot, now, out);
    } else if p.fire_on == FireOn::Trade && t.venue == pos.venue && pos.state.is_active() {
        // the first event that shows a normal trigger sends the exit (this
        // trade's slot is the trigger and the send slot)
        eval_normal(pos, slot, now, out);
    }
}

fn on_own_buy(pos: &mut Position, t: &Trade, slot: u64, now: Now, out: &mut Vec<ExitAction>) {
    let paid = t.sol_amount.saturating_add(t.fee_lamports);
    if pos.state == PositionState::Pending {
        pos.state = PositionState::Open;
        pos.origin = Some(Origin::Intent);
        pos.tokens = t.token_amount;
        pos.cost = paid;
        pos.v_fill = t.reserve_sol;
        pos.t_fill = t.reserve_token;
        open_common(pos, slot, now);
        out.push(ExitAction::Opened {
            origin: Origin::Intent,
            tokens: pos.tokens,
            v_fill: pos.v_fill,
        });
    } else {
        // a second variant / late fill on an open position
        pos.tokens = pos.tokens.saturating_add(t.token_amount);
        pos.cost = pos.cost.saturating_add(paid);
    }
    if pos.dumped.is_some() {
        ensure_dump_orders(pos, slot, out);
    }
}

fn on_own_sell(pos: &mut Position, t: &Trade, slot: u64, now: Now, out: &mut Vec<ExitAction>) {
    pos.tokens = pos.tokens.saturating_sub(t.token_amount);
    pos.proceeds = pos
        .proceeds
        .saturating_add(t.sol_amount.saturating_sub(t.fee_lamports));
    // match the fill to an order: same size first, else the oldest
    let idx = pos
        .pending
        .iter()
        .position(|p| p.is_some_and(|p| p.order.tokens == t.token_amount))
        .or_else(|| pos.pending.iter().position(|p| p.is_some()));
    if let Some(i) = idx {
        pos.last_fill_reason = pos.pending[i].map(|p| p.order.reason);
        pos.pending[i] = None;
    }
    if pos.tokens == 0 && pos.state.is_active() {
        let cause = match pos.last_fill_reason {
            Some(r) => CloseCause::Sold(r),
            None => CloseCause::BalanceZero,
        };
        close(pos, cause, slot, out);
        return;
    }
    clip_pending(pos);
    after_fill(pos, slot, now, out);
}

fn on_balance(pos: &mut Position, tokens: u64, slot: u64, now: Now, out: &mut Vec<ExitAction>) {
    match pos.state {
        PositionState::Pending => {
            if tokens == 0 {
                return;
            }
            pos.state = PositionState::Open;
            pos.origin = Some(Origin::Orphan);
            pos.tokens = tokens;
            pos.v_fill = pos.cur_v;
            pos.t_fill = pos.cur_t;
            open_common(pos, slot, now);
            out.push(ExitAction::Opened {
                origin: Origin::Orphan,
                tokens,
                v_fill: pos.v_fill,
            });
            if pos.dumped.is_some() {
                ensure_dump_orders(pos, slot, out);
            }
        }
        PositionState::Open | PositionState::Exiting => {
            if tokens == 0 {
                close(pos, CloseCause::BalanceZero, slot, out);
                return;
            }
            pos.tokens = tokens;
            clip_pending(pos);
            if pos.dumped.is_some() {
                ensure_dump_orders(pos, slot, out);
            } else if !pos.has_pending() {
                pos.state = PositionState::Open;
            }
        }
        PositionState::Closed => {}
    }
}

fn on_transfer(
    pos: &mut Position,
    x: &TransferEvent,
    slot: u64,
    now: Now,
    out: &mut Vec<ExitAction>,
) {
    let h = pos.xfer_head as usize % XFER_RING;
    pos.xfers[h] = XferRec {
        to_owner: x.to_owner,
        amount: x.amount,
        slot: x.slot,
    };
    pos.xfer_head = ((h + 1) % XFER_RING) as u8;
    let p = pos.policy;
    if p.consolidation_pct == 0 || x.to_owner == pos.owner {
        return;
    }
    let window = p.consolidation_window_slots.max(1);
    let sum: u128 = pos
        .xfers
        .iter()
        .filter(|r| r.amount > 0 && r.to_owner == x.to_owner)
        .filter(|r| r.slot <= x.slot && x.slot - r.slot < window)
        .map(|r| r.amount as u128)
        .sum();
    if sum * 100 >= p.consolidation_pct as u128 * pos.supply as u128 {
        dump(pos, ExitReason::Consolidation, slot, now, out);
    }
}

fn on_complete(pos: &mut Position, slot: u64, out: &mut Vec<ExitAction>) {
    if pos.venue == Venue::PumpSwap {
        return;
    }
    pos.venue = Venue::PumpSwap;
    let (q, b) = pool_from_curve(pos.cur_v);
    pos.cur_v = q;
    pos.cur_t = b;
    pos.pool_known = false;
    out.push(ExitAction::VenueChanged {
        venue: Venue::PumpSwap,
    });
    // curve orders in flight can no longer execute: re-send them on the pool now
    for i in 0..pos.pending.len() {
        if pos.pending[i].is_some_and(|p| !p.exhausted) {
            reissue(pos, i, slot, None, out);
        }
    }
}

// ---------------------------------------------------------------- triggers
fn open_common(pos: &mut Position, slot: u64, now: Now) {
    // the curve just before our buy (`t_fill + tokens`): the reference of
    // `phantom_sl.from = "entry"`
    pos.t_ref = if pos.venue == Venue::Curve && pos.t_fill > 0 {
        pos.t_fill.saturating_add(pos.tokens)
    } else {
        0
    };
    pos.opened_slot = slot;
    pos.opened_ms = now.ms;
    pos.last_trade_ms = now.ms;
    pos.peak_v = pos.v_fill;
    pos.peak_mult_bps = BPS;
    pos.cur_slot = pos.cur_slot.max(slot);
}

fn update_peak(pos: &mut Position) {
    if !pos.state.is_active() || pos.v_fill == 0 {
        return;
    }
    if pos.venue == Venue::Curve {
        pos.peak_v = pos.peak_v.max(pos.cur_v);
    }
    let m = pos.mult_bps();
    pos.peak_mult_bps = pos.peak_mult_bps.max(m);
    let a = u64::from(pos.policy.trail_activate_bps);
    if a > 0 && pos.peak_mult_bps >= a {
        pos.trail_armed = true;
    }
}

/// Normal triggers on the closing state of `pos.cur_slot`.
fn eval_normal(pos: &mut Position, send_slot: u64, now: Now, out: &mut Vec<ExitAction>) {
    if pos.state != PositionState::Open
        || pos.dumped.is_some()
        || pos.has_pending()
        || pos.tokens == 0
    {
        return;
    }
    let p = pos.policy;
    let held = now.ms.saturating_sub(pos.opened_ms);
    let idle = now.ms.saturating_sub(pos.last_trade_ms);
    if pos.v_fill == 0 {
        // An orphan adopted before any trade on the mint was seen has no
        // price, so none of the price triggers can fire. A dead mint never
        // trades again, so the balance would sit in its slot forever. The
        // timers still apply: sell the whole balance blind (the bound is
        // computed on the empty market, i.e. zero), no price needed.
        let blind = if p.no_activity_ms > 0 && idle >= p.no_activity_ms {
            Some(ExitReason::NoActivity)
        } else if (p.max_hold_ms > 0 && held >= p.max_hold_ms)
            || (p.max_hold_slots > 0
                && send_slot >= pos.opened_slot.saturating_add(p.max_hold_slots))
        {
            Some(ExitReason::MaxHold)
        } else {
            None
        };
        if let Some(reason) = blind {
            let tokens = pos.available_tokens();
            let trigger_slot = pos.cur_slot;
            new_order(
                pos,
                reason,
                SellKind::Normal,
                tokens,
                trigger_slot,
                send_slot,
                out,
            );
        }
        return;
    }
    let m = pos.mult_bps();
    // phantom stop mark: our fill price, or the post-bundle price with the
    // curve marked without our own tokens (`from = "entry"`)
    let phantom_m = match p.phantom_from {
        PhantomFrom::Fill => Some(m),
        PhantomFrom::Entry => pos.ex_us_mult_bps(),
    };
    let full = if pos.trail_armed
        && (m as u128) * (BPS as u128)
            <= (pos.peak_mult_bps as u128) * (BPS - u64::from(p.trail_retrace_bps)) as u128
    {
        Some(ExitReason::Trailing)
    } else if p.phantom_bps > 0
        && held >= p.phantom_after_ms
        && phantom_m.is_some_and(|pm| pm <= BPS - u64::from(p.phantom_bps))
    {
        Some(ExitReason::PhantomSl)
    } else if p.no_activity_ms > 0 && idle >= p.no_activity_ms {
        Some(ExitReason::NoActivity)
    } else if (p.max_hold_ms > 0 && held >= p.max_hold_ms)
        || (p.max_hold_slots > 0 && send_slot >= pos.opened_slot.saturating_add(p.max_hold_slots))
    {
        Some(ExitReason::MaxHold)
    } else {
        None
    };
    let trigger_slot = pos.cur_slot;
    if let Some(reason) = full {
        let tokens = pos.available_tokens();
        new_order(
            pos,
            reason,
            SellKind::Normal,
            tokens,
            trigger_slot,
            send_slot,
            out,
        );
        return;
    }
    // take-profit ladder: every newly crossed level sells its fraction of the remainder
    let tp_m = match p.tp_basis {
        TpBasis::Price => m,
        TpBasis::Value => {
            if p.tp.is_empty() {
                return;
            }
            pos.value_bps().unwrap_or(0)
        }
    };
    let mut keep = BPS;
    let mut top: Option<usize> = None;
    for (i, l) in p.tp.levels().iter().enumerate() {
        if pos.tp_done & (1 << i) == 0 && tp_m >= u64::from(l.mult_bps) {
            keep = keep * (BPS - u64::from(l.frac_bps)) / BPS;
            pos.tp_done |= 1 << i;
            top = Some(i);
        }
    }
    let Some(top) = top else { return };
    let avail = pos.available_tokens();
    let mut sell = ((avail as u128) * (BPS - keep) as u128 / BPS as u128) as u64;
    if keep == 0 || avail.saturating_sub(sell) < p.dust_tokens {
        sell = avail;
    }
    if sell > 0 {
        new_order(
            pos,
            ExitReason::TakeProfit(top as u8),
            SellKind::Normal,
            sell,
            trigger_slot,
            send_slot,
            out,
        );
    }
}

fn dump(pos: &mut Position, reason: ExitReason, slot: u64, _now: Now, out: &mut Vec<ExitAction>) {
    if !pos.dump_signalled {
        pos.dump_signalled = true;
        out.push(ExitAction::DumpSignal { reason, slot });
    }
    if pos.dumped.is_none() {
        pos.dumped = Some(reason);
    }
    if pos.state.is_active() {
        ensure_dump_orders(pos, slot, out);
    }
}

/// While dumped: every order in flight runs at dump bounds and the rest of the
/// balance is on an order too (if a slot is free).
fn ensure_dump_orders(pos: &mut Position, slot: u64, out: &mut Vec<ExitAction>) {
    let Some(reason) = pos.dumped else { return };
    if !pos.state.is_active() {
        return;
    }
    pos.state = PositionState::Exiting;
    for i in 0..pos.pending.len() {
        if let Some(p) = pos.pending[i] {
            if p.order.kind == SellKind::Normal && !p.exhausted {
                reissue(pos, i, slot, Some(SellKind::Dump), out);
            }
        }
    }
    let avail = pos.available_tokens();
    if avail > 0 {
        new_order(pos, reason, SellKind::Dump, avail, slot, slot, out);
    }
}

fn after_fill(pos: &mut Position, slot: u64, _now: Now, out: &mut Vec<ExitAction>) {
    if pos.dumped.is_some() {
        ensure_dump_orders(pos, slot, out);
    } else if !pos.has_pending() && pos.state == PositionState::Exiting {
        pos.state = PositionState::Open;
    }
}

fn close(pos: &mut Position, cause: CloseCause, slot: u64, out: &mut Vec<ExitAction>) {
    pos.state = PositionState::Closed;
    pos.closed_slot = Some(slot);
    pos.pending = [None; crate::position::MAX_PENDING];
    out.push(ExitAction::Closed { cause, slot });
}

/// Keep `Σ in-flight <= balance` after the balance shrank (newest orders shrink first).
fn clip_pending(pos: &mut Position) {
    let mut over = pos.inflight_tokens().saturating_sub(pos.tokens);
    for i in (0..pos.pending.len()).rev() {
        if over == 0 {
            break;
        }
        if let Some(mut p) = pos.pending[i] {
            let cut = over.min(p.order.tokens);
            p.order.tokens -= cut;
            over -= cut;
            pos.pending[i] = if p.order.tokens == 0 { None } else { Some(p) };
        }
    }
}

// ------------------------------------------------------------------ orders
fn remaining_spend(pos: &Position) -> u64 {
    pos.policy
        .max_sell_spend_lamports
        .saturating_sub(pos.sell_spend)
}

fn bound(pos: &Position, kind: SellKind, tokens: u64, v_trig: u64, t_trig: u64) -> u64 {
    match kind {
        SellKind::Dump => dump_min_out(&pos.policy, &pos.market(), tokens),
        SellKind::Normal => {
            let m = Market {
                v: v_trig,
                t: t_trig,
                ..pos.market()
            };
            normal_min_out(&pos.policy, &m, tokens)
        }
    }
}

/// Place a first attempt. `tokens` must fit in the uncommitted balance.
fn new_order(
    pos: &mut Position,
    reason: ExitReason,
    kind: SellKind,
    tokens: u64,
    trigger_slot: u64,
    send_slot: u64,
    out: &mut Vec<ExitAction>,
) {
    let tokens = tokens.min(pos.available_tokens());
    if tokens == 0 {
        return;
    }
    let Some(slot_idx) = pos.pending.iter().position(|p| p.is_none()) else {
        return;
    };
    let base_tip = match kind {
        SellKind::Dump => pos.policy.dump_tip_lamports,
        SellKind::Normal => pos.policy.sell_tip_lamports,
    };
    let tip = base_tip.min(remaining_spend(pos));
    pos.sell_spend = pos.sell_spend.saturating_add(tip);
    let close_account = tokens == pos.tokens && !pos.has_pending();
    let order = SellOrder {
        order_id: pos.next_order_id,
        reason,
        kind,
        venue: pos.venue,
        tokens,
        min_sol_output: bound(pos, kind, tokens, pos.cur_v, pos.cur_t),
        tip_lamports: tip,
        try_no: 1,
        trigger_slot,
        v_trigger: pos.cur_v,
        close_account,
    };
    pos.next_order_id = pos.next_order_id.wrapping_add(1);
    pos.pending[slot_idx] = Some(PendingSell {
        order,
        t_trigger: pos.cur_t,
        sent_slot: send_slot,
        exhausted: false,
    });
    pos.state = PositionState::Exiting;
    out.push(ExitAction::Sell(order));
}

/// Re-send the order in `pending[i]` (retry, escalation, venue change).
fn reissue(
    pos: &mut Position,
    i: usize,
    slot: u64,
    force: Option<SellKind>,
    out: &mut Vec<ExitAction>,
) {
    let Some(mut p) = pos.pending[i] else { return };
    let pol = pos.policy;
    let spend_left = remaining_spend(pos);
    if p.order.try_no >= pol.retry_max_tries || spend_left == 0 {
        p.exhausted = true;
        pos.pending[i] = Some(p);
        out.push(ExitAction::Alert(Alert::SellExhausted {
            order_id: p.order.order_id,
            tries: p.order.try_no,
        }));
        return;
    }
    let mut kind = force.unwrap_or(p.order.kind);
    if pos.dumped.is_some() || p.order.try_no >= pol.escalate_after_tries {
        kind = SellKind::Dump;
    }
    let tip = ((p.order.tip_lamports.max(1) as u128) * pol.retry_tip_mult_bps as u128 / BPS as u128)
        .min(spend_left as u128) as u64;
    pos.sell_spend = pos.sell_spend.saturating_add(tip);
    // never more than the balance minus the other orders
    let others = pos.inflight_tokens().saturating_sub(p.order.tokens);
    let tokens = p.order.tokens.min(pos.tokens.saturating_sub(others));
    if tokens == 0 {
        pos.pending[i] = None;
        return;
    }
    p.order.tokens = tokens;
    p.order.kind = kind;
    p.order.venue = pos.venue;
    p.order.try_no += 1;
    p.order.tip_lamports = tip;
    p.order.min_sol_output = if pos.venue == Venue::PumpSwap && kind == SellKind::Normal {
        // the trigger state was a curve state: quote on the pool now
        normal_min_out(&pos.policy, &pos.market(), tokens)
    } else {
        bound(pos, kind, tokens, p.order.v_trigger, p.t_trigger)
    };
    p.sent_slot = slot;
    pos.pending[i] = Some(p);
    out.push(ExitAction::Sell(p.order));
    if p.order.try_no == FAILING_ALERT_AFTER + 1 {
        out.push(ExitAction::Alert(Alert::SellFailing {
            order_id: p.order.order_id,
            tries: FAILING_ALERT_AFTER,
        }));
    }
}
