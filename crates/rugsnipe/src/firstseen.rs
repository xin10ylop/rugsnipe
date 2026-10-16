//! The first-seen benchmark (docs/03 section 5, docs/08 section 9): which
//! feed delivered each create first and how far behind the others were.
//! The hot loop reports every feed's copy of a create (`ColdMsg::FeedSeen`,
//! before the first-wins check and the cross-feed dedup); the cold thread
//! keys them by signature. All receive times come from one monotonic clock
//! in one process, so the lags are exact, not clock-skewed. Run paper with
//! both `feeds.logsws` and `feeds.grpc` enabled and read `cold.first_seen`
//! in the summary (or the status line).

use rugsnipe_feed::event::{FeedSrc, SRC_COUNT};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Creates remembered for pairing (oldest dropped).
pub const MAX_TRACKED: usize = 50_000;
/// Lag samples kept per pair (for the quantiles).
pub const MAX_SAMPLES: usize = 100_000;

#[derive(Debug, Clone, Copy)]
struct Seen {
    first_src: FeedSrc,
    first_ns: u64,
    seen_mask: u8,
}

/// Lag quantiles of one feed behind another (ms).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct LagStats {
    pub n: u64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

/// The summary block (`cold.first_seen`).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct FirstSeenReport {
    /// Creates each feed delivered.
    pub seen_by: BTreeMap<String, u64>,
    /// Creates each feed delivered first.
    pub first_by: BTreeMap<String, u64>,
    /// Creates seen by at least two feeds.
    pub paired: u64,
    /// `"<late>_behind_<first>"` -> lag of the late feed's copy.
    pub lag: BTreeMap<String, LagStats>,
}

/// Pairs feed copies by signature (cold thread; allocates).
#[derive(Debug, Default)]
pub struct FirstSeen {
    by_sig: HashMap<u64, Seen>,
    order: VecDeque<u64>,
    seen_by: [u64; SRC_COUNT],
    first_by: [u64; SRC_COUNT],
    paired: u64,
    lags: HashMap<(FeedSrc, FeedSrc), Vec<f64>>,
    lag_n: HashMap<(FeedSrc, FeedSrc), u64>,
}

impl FirstSeen {
    pub fn new() -> Self {
        Self::default()
    }

    /// One feed's copy of a create.
    pub fn on_seen(&mut self, src: FeedSrc, sig_hash: u64, recv_ns: u64) {
        let bit = 1u8 << (src as u8);
        match self.by_sig.get_mut(&sig_hash) {
            None => {
                self.by_sig.insert(
                    sig_hash,
                    Seen {
                        first_src: src,
                        first_ns: recv_ns,
                        seen_mask: bit,
                    },
                );
                self.order.push_back(sig_hash);
                self.seen_by[src as usize] += 1;
                self.first_by[src as usize] += 1;
                if self.order.len() > MAX_TRACKED {
                    if let Some(old) = self.order.pop_front() {
                        self.by_sig.remove(&old);
                    }
                }
            }
            Some(s) => {
                if s.seen_mask & bit != 0 {
                    return; // the same feed again (a reconnect replay)
                }
                if s.seen_mask.count_ones() == 1 {
                    self.paired += 1;
                }
                s.seen_mask |= bit;
                self.seen_by[src as usize] += 1;
                let (first, lag_ns) = if recv_ns >= s.first_ns {
                    (s.first_src, recv_ns - s.first_ns)
                } else {
                    // the hot loop processed the earlier copy second (two
                    // feeds in one poll): re-attribute the first place
                    self.first_by[s.first_src as usize] -= 1;
                    self.first_by[src as usize] += 1;
                    let late = s.first_src;
                    let lag = s.first_ns - recv_ns;
                    s.first_src = src;
                    s.first_ns = recv_ns;
                    let k = (late, src);
                    *self.lag_n.entry(k).or_default() += 1;
                    let v = self.lags.entry(k).or_default();
                    if v.len() < MAX_SAMPLES {
                        v.push(lag as f64 / 1e6);
                    }
                    return;
                };
                let k = (src, first);
                *self.lag_n.entry(k).or_default() += 1;
                let v = self.lags.entry(k).or_default();
                if v.len() < MAX_SAMPLES {
                    v.push(lag_ns as f64 / 1e6);
                }
            }
        }
    }

    pub fn report(&self) -> FirstSeenReport {
        let mut r = FirstSeenReport {
            paired: self.paired,
            ..Default::default()
        };
        for s in FeedSrc::ALL {
            if self.seen_by[s as usize] > 0 {
                r.seen_by.insert(s.name().into(), self.seen_by[s as usize]);
                r.first_by
                    .insert(s.name().into(), self.first_by[s as usize]);
            }
        }
        for ((late, first), v) in &self.lags {
            let mut x = v.clone();
            x.sort_by(|a, b| a.total_cmp(b));
            let q = |p: f64| x[((x.len() - 1) as f64 * p).round() as usize];
            r.lag.insert(
                format!("{}_behind_{}", late.name(), first.name()),
                LagStats {
                    n: self.lag_n.get(&(*late, *first)).copied().unwrap_or(0),
                    mean_ms: x.iter().sum::<f64>() / x.len() as f64,
                    p50_ms: q(0.5),
                    p90_ms: q(0.9),
                    p99_ms: q(0.99),
                    max_ms: *x.last().unwrap_or(&0.0),
                },
            );
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_copies_and_measures_the_lag() {
        let mut f = FirstSeen::new();
        // 10 creates: gRPC first by (i+1)*100 ms, then the logs path
        for i in 0..10u64 {
            f.on_seen(FeedSrc::Grpc, i, 1_000_000_000 + i);
            f.on_seen(
                FeedSrc::LogsWs,
                i,
                1_000_000_000 + i + (i + 1) * 100_000_000,
            );
        }
        // one create only the logs path saw, one the logs path saw first
        f.on_seen(FeedSrc::LogsWs, 99, 5);
        f.on_seen(FeedSrc::LogsWs, 100, 1_000);
        f.on_seen(FeedSrc::Grpc, 100, 3_000_000);
        // a duplicate from the same feed is ignored
        f.on_seen(FeedSrc::Grpc, 3, 9_999_999_999);
        let r = f.report();
        assert_eq!(r.seen_by["grpc"], 11);
        assert_eq!(r.seen_by["logsws"], 12);
        assert_eq!(r.first_by["grpc"], 10);
        assert_eq!(r.first_by["logsws"], 2);
        assert_eq!(r.paired, 11);
        let l = &r.lag["logsws_behind_grpc"];
        assert_eq!(l.n, 10);
        assert!(
            (l.p50_ms - 600.0).abs() < 1e-6 || (l.p50_ms - 500.0).abs() < 1e-6,
            "{l:?}"
        );
        assert!((l.max_ms - 1000.0).abs() < 1e-6);
        assert!((l.mean_ms - 550.0).abs() < 1e-6);
        let g = &r.lag["grpc_behind_logsws"];
        assert_eq!(g.n, 1);
        assert!((g.p50_ms - 2.999).abs() < 1e-6);
        // an earlier copy processed second re-attributes the first place
        let mut f = FirstSeen::new();
        f.on_seen(FeedSrc::LogsWs, 7, 2_000_000);
        f.on_seen(FeedSrc::Grpc, 7, 1_000_000);
        let r = f.report();
        assert_eq!(r.first_by["grpc"], 1);
        assert_eq!(r.first_by["logsws"], 0);
        assert_eq!(r.lag["logsws_behind_grpc"].n, 1);
    }
}
