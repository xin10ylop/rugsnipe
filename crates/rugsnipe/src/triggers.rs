//! Kill triggers (docs/06 section 13, docs/08 section 10). Each trigger is a
//! pure evaluator here, fed by the cold thread; when one fires the cold
//! thread latches the risk gate (buys stop, exits continue), writes a
//! `kv.kill_trigger:<name>` row with the time, the reason and the detail,
//! persists the latch (`kv.kill_latch`, survives a restart until a local
//! `rugsnipe-ctl unkill`) and alerts.
//!
//! | trigger | rule | input |
//! |---|---|---|
//! | `send_errors` | more than `risk.send_error_kill_ratio` (20%) of the last `risk.send_error_window` (50) real sends failed | every real send's result |
//! | `unexplained_drop` | the wallet fell by more than our known spends of the last [`SPEND_WINDOW_MS`] (stakes, bids, fees, rent, sells, burns: upper bounds) plus `risk.unexplained_drop_lamports` between two reads | the real lane's `getBalance` |
//! | `program_upgrade` | pump's programdata last-deployed slot or upgrade authority changed (vs `kv.pump_programdata`, so an upgrade while down counts too) | `getAccountInfo` 45-byte data slice, every 60 s |
//! | `disk` | the state partition is more than `risk.max_disk_used_pct` (90%) used | `statvfs` on the state.db directory |
//! | `clock` | `chronyc -c tracking` offset above `risk.kill_clock_offset_ms` (50 ms) | chrony, every 60 s (unknown: counted, not a kill; start is refused instead) |
//! | `drift` | the gRPC feed's slot trails `getSlot` (processed) by more than `risk.max_slot_drift` (8) | the feed's slot vs RPC, every 10 s |
//! | `feed_stale` | the post-execution feed is stale or disconnected for more than its `stale_ms` | `FeedHealth` |
//! | `daily_loss` | closed P&L + failed-attempt costs + open positions valued at the current curve (sell quote net of fees) below `-max_daily_loss_lamports` | risk gate + position marks |
//!
//! All of them latch only in probe/live; paper evaluates what is cheap and
//! reports `would_kill` in the summary.

use pump_codec::Pubkey;
use std::collections::VecDeque;

/// Known spends stay in the allowance this long (a send lands within a few
/// slots; a burned nonce gives up after `tx.burn_after_slots`).
pub const SPEND_WINDOW_MS: u64 = 120_000;

/// Trigger names (kv rows `kill_trigger:<name>`).
pub mod names {
    pub const SEND_ERRORS: &str = "send_errors";
    pub const UNEXPLAINED_DROP: &str = "unexplained_drop";
    pub const PROGRAM_UPGRADE: &str = "program_upgrade";
    pub const DISK: &str = "disk";
    pub const CLOCK: &str = "clock";
    pub const DRIFT: &str = "drift";
    pub const FEED_STALE: &str = "feed_stale";
    pub const DAILY_LOSS: &str = "daily_loss";
}

/// The send-error ratio over the last `n` real sends.
#[derive(Debug, Clone)]
pub struct SendErrWindow {
    ring: VecDeque<bool>,
    n: usize,
    max_ratio: f64,
}

impl SendErrWindow {
    pub fn new(n: usize, max_ratio: f64) -> Self {
        Self {
            ring: VecDeque::with_capacity(n.max(1)),
            n: n.max(1),
            max_ratio,
        }
    }

    /// Failures among the sends held.
    pub fn errors(&self) -> usize {
        self.ring.iter().filter(|ok| !**ok).count()
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// One send's result; `Some(detail)` when the window is full and the
    /// failure ratio is above the limit.
    pub fn on_send(&mut self, ok: bool) -> Option<String> {
        if self.ring.len() == self.n {
            self.ring.pop_front();
        }
        self.ring.push_back(ok);
        let e = self.errors();
        (self.ring.len() == self.n && e as f64 / self.n as f64 > self.max_ratio).then(|| {
            format!(
                "{e} of the last {} sends failed ({:.0}% > {:.0}%)",
                self.n,
                100.0 * e as f64 / self.n as f64,
                100.0 * self.max_ratio
            )
        })
    }
}

/// Wallet reads against our known spends.
#[derive(Debug, Clone)]
pub struct BalanceWatch {
    last: Option<u64>,
    spends: VecDeque<(u64, u64)>,
    tolerance: u64,
}

impl BalanceWatch {
    pub fn new(tolerance: u64) -> Self {
        Self {
            last: None,
            spends: VecDeque::new(),
            tolerance,
        }
    }

    /// A known outflow (an upper bound of what it can cost) at `now_ms`.
    pub fn on_spend(&mut self, lamports: u64, now_ms: u64) {
        self.spends.push_back((now_ms, lamports));
    }

    /// Spends still in the allowance.
    pub fn allowance(&self, now_ms: u64) -> u64 {
        self.spends
            .iter()
            .filter(|(t, _)| now_ms.saturating_sub(*t) <= SPEND_WINDOW_MS)
            .map(|(_, l)| *l)
            .sum()
    }

    /// A balance read; `Some(detail)` on an unexplained drop.
    pub fn on_balance(&mut self, lamports: u64, now_ms: u64) -> Option<String> {
        while self
            .spends
            .front()
            .is_some_and(|(t, _)| now_ms.saturating_sub(*t) > SPEND_WINDOW_MS)
        {
            self.spends.pop_front();
        }
        let prev = self.last.replace(lamports)?;
        let drop = prev.saturating_sub(lamports);
        let allow = self.allowance(now_ms);
        (drop > allow.saturating_add(self.tolerance)).then(|| {
            format!(
                "wallet fell {drop} lamports ({prev} -> {lamports}); known spends {allow} + tolerance {}",
                self.tolerance
            )
        })
    }
}

/// The 45-byte head of an upgradeable program's programdata account:
/// `u32 variant (3) | u64 last-deployed slot | option<authority>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramData {
    pub slot: u64,
    pub authority: Option<Pubkey>,
}

impl ProgramData {
    pub const SLICE: usize = 45;

    pub fn parse(b: &[u8]) -> Option<ProgramData> {
        if b.len() < 13 || u32::from_le_bytes(b[0..4].try_into().ok()?) != 3 {
            return None;
        }
        let slot = u64::from_le_bytes(b[4..12].try_into().ok()?);
        let authority = match b[12] {
            1 => Some(Pubkey::new_from_array(b.get(13..45)?.try_into().ok()?)),
            _ => None,
        };
        Some(ProgramData { slot, authority })
    }

    pub fn to_kv(&self) -> String {
        format!(
            "{}:{}",
            self.slot,
            self.authority.map(|a| a.to_string()).unwrap_or_default()
        )
    }

    pub fn from_kv(s: &str) -> Option<ProgramData> {
        let (slot, auth) = s.split_once(':')?;
        Some(ProgramData {
            slot: slot.parse().ok()?,
            authority: if auth.is_empty() {
                None
            } else {
                Some(auth.parse().ok()?)
            },
        })
    }
}

/// `Some(detail)` when the program was redeployed or its authority moved.
pub fn upgrade_changed(baseline: &ProgramData, now: &ProgramData) -> Option<String> {
    (baseline != now).then(|| {
        format!(
            "pump programdata changed: deployed slot {} -> {}, authority {:?} -> {:?}",
            baseline.slot,
            now.slot,
            baseline.authority.map(|a| a.to_string()),
            now.authority.map(|a| a.to_string())
        )
    })
}

/// Used share (percent) of the filesystem holding `path`.
pub fn disk_used_pct(path: &std::path::Path) -> std::io::Result<f64> {
    use std::os::unix::ffi::OsStrExt;
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    };
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path, `st` a writable statvfs.
    let r = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(used_pct(
        st.f_blocks as u64,
        st.f_bfree as u64,
        st.f_bavail as u64,
    ))
}

/// `df`'s Use%: used / (used + available to users).
pub fn used_pct(blocks: u64, free: u64, avail: u64) -> f64 {
    let used = blocks.saturating_sub(free);
    let denom = used + avail;
    if denom == 0 {
        0.0
    } else {
        100.0 * used as f64 / denom as f64
    }
}

pub fn disk_exceeded(pct: f64, max: f64) -> Option<String> {
    (pct > max).then(|| format!("state partition {pct:.1}% used > {max:.0}%"))
}

/// `Some(detail)` when the measured offset is above `max` (unknown: `None`).
pub fn clock_exceeded(offset_ms: Option<f64>, max: f64) -> Option<String> {
    let ms = offset_ms?;
    (ms.abs() > max).then(|| format!("clock offset {ms:.1} ms > {max:.0} ms (chronyc tracking)"))
}

/// `Some(detail)` when the feed's slot trails the RPC slot by more than `max`.
pub fn drift_exceeded(feed_slot: u64, rpc_slot: u64, max: u64) -> Option<String> {
    if feed_slot == 0 || rpc_slot == 0 {
        return None;
    }
    let d = rpc_slot.saturating_sub(feed_slot);
    (d > max).then(|| format!("gRPC slot {feed_slot} trails getSlot {rpc_slot} by {d} > {max}"))
}

/// The post-execution feed is unusable: disconnected, or connected without
/// a data message for more than `stale_ms`.
pub fn feed_stale(connected: bool, since_frame_ms: u64, stale_ms: u64) -> Option<String> {
    // a reconnect inside the window is not a kill: only `stale_ms` without
    // a data message is (`u64::MAX`: never received one)
    if since_frame_ms <= stale_ms {
        return None;
    }
    let ago = if since_frame_ms == u64::MAX {
        "ever".to_string()
    } else {
        format!("{since_frame_ms} ms")
    };
    Some(if connected {
        format!("post-execution feed stale: no message for {ago} > {stale_ms} ms")
    } else {
        format!("post-execution feed disconnected, no message for {ago} > {stale_ms} ms")
    })
}

/// One open position for the mark-to-market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub tokens: u64,
    /// Lamports paid for the tokens.
    pub cost: u64,
    /// Net lamports from partial sells so far.
    pub proceeds: u64,
    /// Net lamports the tokens would fetch now (sell quote at the current curve).
    pub value: u64,
}

/// Today's P&L marked to market: closed P&L and failed costs (the risk
/// gate's `realized_today`) plus every open position at its sell quote.
pub fn mtm_today(realized_today: i64, marks: &[Mark]) -> i64 {
    marks.iter().fold(realized_today, |acc, m| {
        acc.saturating_add(m.value as i64 + m.proceeds as i64 - m.cost as i64)
    })
}

pub fn daily_loss_exceeded(mtm: i64, cap: u64) -> Option<String> {
    (cap > 0 && mtm <= -(cap as i64))
        .then(|| format!("daily P&L marked to market {mtm} lamports <= -{cap}"))
}

/// The value of a held position from its snapshot (curve sell quote net of
/// fees; a PumpSwap position at its pool reserves with the same fee rate).
pub fn position_mark(p: &rugsnipe_exit::Position) -> Mark {
    use rugsnipe_exit::bounds::{amm_sell_net, curve_sell_net};
    let fees = pump_codec::curve::FeeBps {
        protocol: p.fee_protocol_bps,
        creator: p.fee_creator_bps,
    };
    let value = match p.venue {
        rugsnipe_exit::Venue::Curve => curve_sell_net(p.tokens, p.cur_v, p.cur_t, p.creator, fees),
        rugsnipe_exit::Venue::PumpSwap => amm_sell_net(
            p.tokens,
            p.cur_v,
            p.cur_t,
            (p.fee_protocol_bps + p.fee_creator_bps).min(10_000) as u32,
        ),
    };
    Mark {
        tokens: p.tokens,
        cost: p.cost,
        proceeds: p.proceeds,
        value,
    }
}

/// `BPFLoaderUpgradeab1e11111111111111111111111`.
pub const BPF_LOADER_UPGRADEABLE: Pubkey =
    Pubkey::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");

/// pump's programdata account (`B5MvUw…`, docs/06 section 0).
pub fn pump_programdata() -> Pubkey {
    Pubkey::find_program_address(
        &[pump_codec::constants::PUMP_PROGRAM_ID.as_ref()],
        &BPF_LOADER_UPGRADEABLE,
    )
    .0
}

/// What the chain checks lane reports.
#[derive(Debug, Clone)]
pub enum CheckEvent {
    /// `getSlot` at processed commitment.
    RpcSlot(u64),
    /// pump's programdata head.
    ProgramData(ProgramData),
    Error(String),
    Calls(u32),
}

/// The chain reads behind `drift` and `program_upgrade` (cold runtime,
/// inside the keeper's RPC budget): `getSlot` every `slot_every_ms`,
/// pump's programdata head (45-byte `dataSlice`) every `upgrade_every_ms`.
pub async fn chain_checks(
    rpc_url: String,
    budget: std::sync::Arc<rugsnipe_state::RpcBudget>,
    slot_every_ms: u64,
    upgrade_every_ms: u64,
    out: tokio::sync::mpsc::UnboundedSender<CheckEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    let mut rpc = match rugsnipe_feed::ws::RpcClient::new(&rpc_url) {
        Ok(r) => r,
        Err(e) => {
            let _ = out.send(CheckEvent::Error(format!("checks rpc: {e}")));
            return;
        }
    };
    let pd = pump_programdata().to_string();
    let (mut last_slot, mut last_pd) = (0u64, 0u64);
    let mut calls = 0u32;
    while !stop.load(Ordering::Acquire) {
        let now = crate::now_ms();
        if now.saturating_sub(last_slot) >= slot_every_ms {
            last_slot = now;
            match crate::real::call(
                &mut rpc,
                &budget,
                &mut calls,
                "getSlot",
                serde_json::json!([{"commitment": "processed"}]),
            )
            .await
            {
                Ok(v) => {
                    if let Some(s) = v.as_u64() {
                        let _ = out.send(CheckEvent::RpcSlot(s));
                    }
                }
                Err(e) => {
                    let _ = out.send(CheckEvent::Error(format!("getSlot: {e}")));
                }
            }
        }
        if now.saturating_sub(last_pd) >= upgrade_every_ms {
            last_pd = now;
            let params = serde_json::json!([pd, {"encoding": "base64", "commitment": "confirmed",
                "dataSlice": {"offset": 0, "length": ProgramData::SLICE}}]);
            match crate::real::call(&mut rpc, &budget, &mut calls, "getAccountInfo", params).await {
                Ok(v) => {
                    use base64::Engine as _;
                    let head = v["value"]["data"][0]
                        .as_str()
                        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
                        .and_then(|b| ProgramData::parse(&b));
                    match head {
                        Some(h) => {
                            let _ = out.send(CheckEvent::ProgramData(h));
                        }
                        None => {
                            let _ =
                                out.send(CheckEvent::Error("pump programdata unreadable".into()));
                        }
                    }
                }
                Err(e) => {
                    let _ = out.send(CheckEvent::Error(format!("programdata: {e}")));
                }
            }
        }
        if calls > 0 {
            let _ = out.send(CheckEvent::Calls(calls));
            calls = 0;
        }
        for _ in 0..10 {
            if stop.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

/// `chronyc -c tracking` offset (ms), `None` if chrony is missing, not
/// synchronised or not readable as this user.
pub fn chrony_offset_ms() -> Option<f64> {
    crate::app::chrony_offset_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pump_programdata_is_the_known_account() {
        assert!(
            pump_programdata().to_string().starts_with("B5MvUw"),
            "{}",
            pump_programdata()
        );
    }

    #[test]
    fn send_errors_over_the_last_50() {
        let mut w = SendErrWindow::new(50, 0.20);
        // 10 failures in 50: exactly 20% is not above it
        for i in 0..50 {
            assert!(w.on_send(i % 5 != 4).is_none(), "{i}");
        }
        assert_eq!(w.errors(), 10);
        // the 11th failure in the window trips it
        let d = w.on_send(false).expect("11/50 > 20%");
        assert!(d.contains("11 of the last 50"), "{d}");
        // a short window never fires, however bad
        let mut s = SendErrWindow::new(50, 0.20);
        for _ in 0..49 {
            assert!(s.on_send(false).is_none());
        }
        assert!(s.on_send(false).is_some());
    }

    #[test]
    fn balance_drops_are_explained_by_known_spends_only() {
        let mut b = BalanceWatch::new(50_000_000);
        assert!(
            b.on_balance(2_000_000_000, 0).is_none(),
            "first read: baseline"
        );
        // a 0.02 SOL buy with its bid, fees and rent: explained
        b.on_spend(20_000_000 + 5_000_000 + 1_513_840 + 20_000, 1_000);
        assert!(b.on_balance(1_975_000_000, 2_000).is_none());
        // a rise (a sell, a deposit) is never a kill
        assert!(b.on_balance(2_100_000_000, 3_000).is_none());
        // 0.3 SOL gone with nothing sent: unexplained
        let d = b.on_balance(1_800_000_000, 4_000).expect("unexplained");
        assert!(d.contains("300000000"), "{d}");
        // the allowance expires after the window
        let mut b = BalanceWatch::new(0);
        b.on_balance(1_000_000_000, 0);
        b.on_spend(100_000_000, 0);
        assert_eq!(b.allowance(SPEND_WINDOW_MS + 1), 0);
        assert!(b.on_balance(900_000_000, SPEND_WINDOW_MS + 1).is_some());
    }

    #[test]
    fn programdata_change_is_an_upgrade() {
        let auth = Pubkey::new_from_array([7; 32]);
        let mut b = vec![3, 0, 0, 0];
        b.extend_from_slice(&456_494_698u64.to_le_bytes());
        b.push(1);
        b.extend_from_slice(auth.as_ref());
        let p = ProgramData::parse(&b).unwrap();
        assert_eq!(p.slot, 456_494_698);
        assert_eq!(p.authority, Some(auth));
        assert_eq!(ProgramData::from_kv(&p.to_kv()), Some(p));
        assert!(upgrade_changed(&p, &p).is_none());
        let redeployed = ProgramData {
            slot: 457_000_000,
            ..p
        };
        assert!(upgrade_changed(&p, &redeployed)
            .unwrap()
            .contains("457000000"));
        let frozen = ProgramData {
            authority: None,
            ..p
        };
        assert!(upgrade_changed(&p, &frozen).is_some(), "authority change");
        assert_eq!(ProgramData::from_kv(&frozen.to_kv()), Some(frozen));
        assert!(ProgramData::parse(&[2, 0, 0, 0]).is_none());
    }

    #[test]
    fn disk_clock_drift_stale_and_loss() {
        assert!((used_pct(1000, 50, 0) - 100.0).abs() < 1e-9);
        assert!((used_pct(1000, 100, 50) - 94.736_842).abs() < 1e-3);
        assert!(disk_exceeded(91.0, 90.0).is_some());
        assert!(disk_exceeded(90.0, 90.0).is_none());
        let real = disk_used_pct(&std::env::temp_dir()).unwrap();
        assert!((0.0..=100.0).contains(&real));
        assert!(clock_exceeded(Some(51.0), 50.0).is_some());
        assert!(clock_exceeded(Some(-60.0), 50.0).is_some());
        assert!(clock_exceeded(Some(3.0), 50.0).is_none());
        assert!(clock_exceeded(None, 50.0).is_none());
        assert!(drift_exceeded(100, 109, 8).is_some());
        assert!(drift_exceeded(100, 108, 8).is_none());
        assert!(
            drift_exceeded(110, 100, 8).is_none(),
            "the feed ahead is fine"
        );
        assert!(drift_exceeded(0, 100, 8).is_none(), "no feed slot yet");
        assert!(feed_stale(false, 2_500, 2_000)
            .unwrap()
            .contains("disconnected"));
        assert!(
            feed_stale(false, 500, 2_000).is_none(),
            "a reconnect inside the window"
        );
        assert!(feed_stale(true, 2_500, 2_000).is_some());
        assert!(feed_stale(true, u64::MAX, 2_000).is_some());
        assert!(feed_stale(true, 1_000, 2_000).is_none());
        let m = [
            Mark {
                tokens: 1,
                cost: 20_000_000,
                proceeds: 0,
                value: 5_000_000,
            },
            Mark {
                tokens: 1,
                cost: 20_000_000,
                proceeds: 10_000_000,
                value: 15_000_000,
            },
        ];
        // closed -150M today, open positions -15M and +5M
        let mtm = mtm_today(-150_000_000, &m);
        assert_eq!(mtm, -160_000_000);
        assert!(daily_loss_exceeded(mtm, 200_000_000).is_none());
        assert!(daily_loss_exceeded(mtm, 160_000_000).is_some());
        assert!(daily_loss_exceeded(-1, 0).is_none(), "no cap: no trigger");
    }
}
