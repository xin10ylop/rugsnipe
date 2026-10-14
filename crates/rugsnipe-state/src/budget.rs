//! `RpcBudget`: per-IP, per-method token bucket (docs/06 sections 6
//! `[rpc_budget]`, ruling 21). Every process role on one public-RPC IP gets
//! `rate` requests/s **per method** (the public mainnet endpoint limits per
//! method and IP); the roles' sum must stay <= 32 (checked by
//! `Config::validate`). Burst = one second of tokens. Cold path only.

use rugsnipe_core::config::RpcBudgetCfg;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

/// Token buckets keyed by JSON-RPC method.
#[derive(Debug)]
pub struct RpcBudget {
    rate: f64,
    burst: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RpcBudget {
    /// `rate` requests per second per method (0 = every request refused).
    pub fn new(rate: f64) -> Self {
        Self {
            rate: rate.max(0.0),
            burst: rate.max(1.0),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// The budget of `role` (`hot`, `guard`, `keeper`, `collector`, `backfill`).
    pub fn for_role(cfg: &RpcBudgetCfg, role: &str) -> Option<Self> {
        let r = match role {
            "hot" => cfg.hot,
            "guard" => cfg.guard,
            "keeper" => cfg.keeper,
            "collector" => cfg.collector,
            "backfill" => cfg.backfill,
            _ => return None,
        };
        Some(Self::new(r as f64))
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    fn refill(&self, b: &mut Bucket, now: Instant) {
        let dt = now.saturating_duration_since(b.last).as_secs_f64();
        b.tokens = (b.tokens + dt * self.rate).min(self.burst);
        b.last = now;
    }

    /// Take one token for `method` at `now` if available.
    pub fn try_acquire_at(&self, method: &str, now: Instant) -> bool {
        if self.rate <= 0.0 {
            return false;
        }
        let mut m = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let b = m.entry(method.to_string()).or_insert(Bucket {
            tokens: self.burst,
            last: now,
        });
        self.refill(b, now);
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    pub fn try_acquire(&self, method: &str) -> bool {
        self.try_acquire_at(method, Instant::now())
    }

    /// How long until `method` has a token (zero if one is available now).
    pub fn wait_time(&self, method: &str) -> Duration {
        if self.rate <= 0.0 {
            return Duration::from_secs(3600);
        }
        let now = Instant::now();
        let mut m = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let Some(b) = m.get_mut(method) else {
            return Duration::ZERO;
        };
        self.refill(b, now);
        if b.tokens >= 1.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64((1.0 - b.tokens) / self.rate)
        }
    }

    /// Block the calling (cold) thread until a token is available, then take it.
    pub fn acquire_blocking(&self, method: &str) {
        loop {
            if self.try_acquire(method) {
                return;
            }
            std::thread::sleep(self.wait_time(method).max(Duration::from_millis(5)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_method_rate_and_burst() {
        let b = RpcBudget::new(4.0);
        let t0 = Instant::now();
        let n = (0..10).filter(|_| b.try_acquire_at("getSlot", t0)).count();
        assert_eq!(n, 4, "burst = one second of tokens");
        assert!(
            b.try_acquire_at("getTransaction", t0),
            "methods are independent"
        );
        assert!(!b.try_acquire_at("getSlot", t0 + Duration::from_millis(200)));
        assert!(b.try_acquire_at("getSlot", t0 + Duration::from_millis(260)));
        // over 10 s at most 4/s + burst
        let mut got = 0;
        for i in 0..1000 {
            if b.try_acquire_at("x", t0 + Duration::from_millis(i * 10)) {
                got += 1;
            }
        }
        assert!((40..=45).contains(&got), "{got}");
    }

    #[test]
    fn roles_from_config() {
        let cfg = RpcBudgetCfg::default();
        assert_eq!(RpcBudget::for_role(&cfg, "hot").unwrap().rate(), 4.0);
        assert_eq!(RpcBudget::for_role(&cfg, "keeper").unwrap().rate(), 8.0);
        assert!(RpcBudget::for_role(&cfg, "nope").is_none());
        assert!(!RpcBudget::new(0.0).try_acquire("a"));
    }
}
