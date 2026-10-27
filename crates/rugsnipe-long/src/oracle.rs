//! Prices of the floats in SOL (Jupiter Price API v3, every 20 s on a
//! thread of its own): lamports per raw unit of a mint. Used to value its
//! buys paid from a held token, to size our stake in a float, and to
//! report the floats' value. A price older than [`STALE_MS`] is not used.

use pump_codec::constants as c;
use pump_codec::Pubkey;
use rugsnipe_tx::rpc::HttpGet;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const API: &str = "https://lite-api.jup.ag/price/v3";
pub const STALE_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Px {
    /// Lamports per raw unit.
    lamports: f64,
    at_ms: u64,
}

/// Shared prices.
#[derive(Debug, Clone, Default)]
pub struct Oracle {
    inner: Arc<Mutex<HashMap<Pubkey, Px>>>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Oracle {
    /// Lamports per raw unit of `mint` (wrapped SOL: 1).
    pub fn lamports_per_unit(&self, mint: &Pubkey) -> Option<f64> {
        if *mint == c::WSOL_MINT {
            return Some(1.0);
        }
        let g = self.inner.lock().ok()?;
        let p = g.get(mint)?;
        (now_ms().saturating_sub(p.at_ms) < STALE_MS).then_some(p.lamports)
    }

    /// Set a price (tests, or a fill's own rate).
    pub fn set(&self, mint: Pubkey, lamports_per_unit: f64) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(
                mint,
                Px {
                    lamports: lamports_per_unit,
                    at_ms: now_ms(),
                },
            );
        }
    }

    /// One refresh from the API body (`{mint: {usdPrice, decimals}}`).
    pub fn apply(&self, body: &serde_json::Value) -> usize {
        let sol = body[c::WSOL_MINT.to_string()]["usdPrice"]
            .as_f64()
            .filter(|p| *p > 0.0);
        let Some(sol) = sol else { return 0 };
        let mut n = 0;
        if let (Some(obj), Ok(mut g)) = (body.as_object(), self.inner.lock()) {
            for (k, v) in obj {
                let (Ok(m), Some(usd), Some(dec)) = (
                    k.parse::<Pubkey>(),
                    v["usdPrice"].as_f64(),
                    v["decimals"].as_u64(),
                ) else {
                    continue;
                };
                if m == c::WSOL_MINT || usd <= 0.0 {
                    continue;
                }
                let lamports = usd / 10f64.powi(dec as i32) / sol * 1e9;
                g.insert(
                    m,
                    Px {
                        lamports,
                        at_ms: now_ms(),
                    },
                );
                n += 1;
            }
        }
        n
    }

    /// Refresh `mints` every `every` on a thread of its own.
    pub fn spawn(&self, api: &str, mints: Vec<Pubkey>, every: Duration) {
        let api = api.to_string();
        let me = self.clone();
        std::thread::Builder::new()
            .name("long-oracle".into())
            .spawn(move || {
                let http = HttpGet::new(Duration::from_secs(10), 1 << 20);
                let mut ids: Vec<String> = mints.iter().map(|m| m.to_string()).collect();
                ids.push(c::WSOL_MINT.to_string());
                let url = format!("{api}?ids={}", ids.join(","));
                loop {
                    match http.get(&url) {
                        Ok((200, body)) => {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                                me.apply(&v);
                            }
                        }
                        Ok((s, _)) => {
                            eprintln!("long oracle: HTTP {s} (retry in {} s)", every.as_secs())
                        }
                        Err(e) => eprintln!("long oracle: {e}"),
                    }
                    std::thread::sleep(every);
                }
            })
            .expect("spawn the oracle thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_in_lamports_per_raw_unit() {
        let o = Oracle::default();
        let wbtc: Pubkey = "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh"
            .parse()
            .unwrap();
        let body = serde_json::json!({
            "So11111111111111111111111111111111111111112": {"usdPrice": 200.0, "decimals": 9},
            "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh": {"usdPrice": 100000.0, "decimals": 8}
        });
        assert_eq!(o.apply(&body), 1);
        // 1 WBTC = 500 SOL: one sat (1e-8) = 5e-6 SOL = 5000 lamports
        let p = o.lamports_per_unit(&wbtc).unwrap();
        assert!((p - 5000.0).abs() < 1e-6, "{p}");
        assert_eq!(o.lamports_per_unit(&c::WSOL_MINT), Some(1.0));
        assert_eq!(o.lamports_per_unit(&Pubkey::new_from_array([1; 32])), None);
    }
}
