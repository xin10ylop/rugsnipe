//! Same-building HTTP senders (feature `senders`). Endpoints, tip accounts
//! and minimum tips from docs/03 section 6 and `sol-trade-sdk`
//! `src/constants/swqos.rs` (2026-10-14).
//!
//! | provider | request | min tip (default) |
//! |---|---|---|
//! | Helius Sender `/fast` | JSON-RPC `sendTransaction` (`skipPreflight`, `maxRetries: 0`); `?swqos_only=true` | 0.001 SOL; 5,000 lamports with `swqos_only` |
//! | Nozomi | Batch Send `POST /api/sendBatch?c=KEY`, body `(u16 BE len ‖ tx)*`, octet-stream | 0.001 SOL |
//! | Astralane | `POST /irisb?api-key=KEY&method=sendTransaction`, raw tx bytes | 0.001 SOL (config) |
//! | Jito block engine | `POST /api/v1/transactions?bundleOnly=true` JSON-RPC `sendTransaction`; `sendBundle` on `/api/v1/bundles` | 1,000 lamports; 1 req/s per IP |
//! | NextBlock | `POST /api/v2/submit`, `Authorization: KEY`, JSON `{"transaction":{"content":b64},"frontRunningProtection":false}` | 0.001 SOL |

use super::http::{json_rpc_send_body, HttpConn, HttpTarget};
use super::{check_tip, now_ms, ApiKeys, Breaker, SendError, SendResult, Sender, SenderId, Variant};
use pump_codec::Pubkey;
use rugsnipe_core::config::HttpSenderCfg;
use std::sync::atomic::{AtomicU64, Ordering::*};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const fn pk(s: &str) -> Pubkey {
    Pubkey::from_str_const(s)
}

/// Helius Sender tip accounts.
pub const HELIUS_TIP_ACCOUNTS: [Pubkey; 10] = [
    pk("4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE"),
    pk("D2L6yPZ2FmmmTKPgzaMKdhu6EWZcTpLy1Vhx8uvZe7NZ"),
    pk("9bnz4RShgq1hAnLnZbP8kbgBg1kEmcJBYQq3gQbmnSta"),
    pk("5VY91ws6B2hMmBFRsXkoAAdsPHBJwRfBht4DXox3xkwn"),
    pk("2nyhqdwKcJZR2vcqCyrYsaPVdAnFoJjiksCXJ7hfEYgD"),
    pk("2q5pghRs6arqVjRvT5gfgWfWcHWmw1ZuCzphgd5KfWGJ"),
    pk("wyvPkWjVZz1M8fHQnMMCDTQDbkManefNNhweYk5WkcF"),
    pk("3KCKozbAaF75qEU33jtzozcJ29yJuaLJTy2jFdzUY8bT"),
    pk("4vieeGHPYPG2MmyPRcYjdiDmmhN3ww7hsFNap8pVN3Ey"),
    pk("4TQLFNWK8AovT1gFvda5jfw2oJeRMKEmw7aH6MGBJ3or"),
];

/// Nozomi (Temporal) tip accounts.
pub const NOZOMI_TIP_ACCOUNTS: [Pubkey; 17] = [
    pk("TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq"),
    pk("noz3jAjPiHuBPqiSPkkugaJDkJscPuRhYnSpbi8UvC4"),
    pk("noz3str9KXfpKknefHji8L1mPgimezaiUyCHYMDv1GE"),
    pk("noz6uoYCDijhu1V7cutCpwxNiSovEwLdRHPwmgCGDNo"),
    pk("noz9EPNcT7WH6Sou3sr3GGjHQYVkN3DNirpbvDkv9YJ"),
    pk("nozc5yT15LazbLTFVZzoNZCwjh3yUtW86LoUyqsBu4L"),
    pk("nozFrhfnNGoyqwVuwPAW4aaGqempx4PU6g6D9CJMv7Z"),
    pk("nozievPk7HyK1Rqy1MPJwVQ7qQg2QoJGyP71oeDwbsu"),
    pk("noznbgwYnBLDHu8wcQVCEw6kDrXkPdKkydGJGNXGvL7"),
    pk("nozNVWs5N8mgzuD3qigrCG2UoKxZttxzZ85pvAQVrbP"),
    pk("nozpEGbwx4BcGp6pvEdAh1JoC2CQGZdU6HbNP1v2p6P"),
    pk("nozrhjhkCr3zXT3BiT4WCodYCUFeQvcdUkM7MqhKqge"),
    pk("nozrwQtWhEdrA6W8dkbt9gnUaMs52PdAv5byipnadq3"),
    pk("nozUacTVWub3cL4mJmGCYjKZTnE9RbdY5AP46iQgbPJ"),
    pk("nozWCyTPppJjRuw2fpzDhhWbW355fzosWSzrrMYB1Qk"),
    pk("nozWNju6dY353eMkMqURqwQEoM3SFgEKC6psLCSfUne"),
    pk("nozxNBgWohjR75vdspfxR5H9ceC7XXH99xpxhVGt3Bb"),
];

/// Astralane tip accounts.
pub const ASTRALANE_TIP_ACCOUNTS: [Pubkey; 14] = [
    pk("astrazznxsGUhWShqgNtAdfrzP2G83DzcWVJDxwV9bF"),
    pk("astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm"),
    pk("astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk"),
    pk("astraRVUuTHjpwEVvNBeQEgwYx9w9CFyfxjYoobCZhL"),
    pk("astraEJ2fEj8Xmy6KLG7B3VfbKfsHXhHrNdCQx7iGJK"),
    pk("astraubkDw81n4LuutzSQ8uzHCv4BhPVhfvTcYv8SKC"),
    pk("astraZW5GLFefxNPAatceHhYjfA1ciq9gvfEg2S47xk"),
    pk("astrawVNP4xDBKT7rAdxrLYiTSTdqtUr63fSMduivXK"),
    pk("AstrA1ejL4UeXC2SBP4cpeEmtcFPZVLxx3XGKXyCW6to"),
    pk("AsTra79FET4aCKWspPqeSFvjJNyp96SvAnrmyAxqg5b7"),
    pk("AstrABAu8CBTyuPXpV4eSCJ5fePEPnxN8NqBaPKQ9fHR"),
    pk("AsTRADtvb6tTmrsqULQ9Wji9PigDMjhfEMza6zkynEvV"),
    pk("AsTRAEoyMofR3vUPpf9k68Gsfb6ymTZttEtsAbv8Bk4d"),
    pk("AStrAJv2RN2hKCHxwUMtqmSxgdcNZbihCwc1mCSnG83W"),
];

/// NextBlock tip accounts.
pub const NEXTBLOCK_TIP_ACCOUNTS: [Pubkey; 8] = [
    pk("NextbLoCkVtMGcV47JzewQdvBpLqT9TxQFozQkN98pE"),
    pk("NexTbLoCkWykbLuB1NkjXgFWkX9oAtcoagQegygXXA2"),
    pk("NeXTBLoCKs9F1y5PJS9CKrFNNLU1keHW71rfh7KgA1X"),
    pk("NexTBLockJYZ7QD7p2byrUa6df8ndV2WSd8GkbWqfbb"),
    pk("neXtBLock1LeC67jYd1QdAa32kbVeubsfPNTJC1V5At"),
    pk("nEXTBLockYgngeRmRrjDV31mGSekVPqZoMGhQEZtPVG"),
    pk("NEXTbLoCkB51HpLBLojQfpyVAMorm3zzKg7w9NFdqid"),
    pk("nextBLoCkPMgmG8ZgJtABeScP35qLa2AMCNKntAP7Xc"),
];

/// Default endpoints (Frankfurt).
pub mod endpoints {
    pub const HELIUS: &str = "http://fra-sender.helius-rpc.com/fast";
    pub const NOZOMI: &str = "http://fra2.nozomi.temporal.xyz";
    pub const ASTRALANE: &str = "http://fr.gateway.astralane.io/irisb";
    pub const JITO: &str = "http://frankfurt.mainnet.block-engine.jito.wtf";
    pub const NEXTBLOCK: &str = "http://fra.nextblock.io";
}

/// How the request body is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// JSON-RPC `sendTransaction` (Helius, Jito).
    JsonRpc,
    /// Nozomi batch: `u16 BE len ‖ tx`.
    NozomiBatch,
    /// Raw bytes (Astralane `irisb`).
    Raw,
    /// NextBlock JSON.
    NextBlock,
}

/// One HTTP provider.
pub struct HttpSender {
    id: SenderId,
    wire: Wire,
    tips: &'static [Pubkey],
    min_tip: u64,
    path: String,
    headers: Vec<(String, String)>,
    opts: &'static str,
    conn: Mutex<(HttpConn, Vec<u8>)>,
    breaker: Breaker,
    /// Minimum ms between requests (0 = unlimited).
    min_interval_ms: u64,
    last_ms: AtomicU64,
}

impl core::fmt::Debug for HttpSender {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // never print headers or the path: they can hold API keys
        f.debug_struct("HttpSender").field("id", &self.id).field("min_tip", &self.min_tip).finish_non_exhaustive()
    }
}

impl HttpSender {
    /// Low-level constructor (tests, custom endpoints).
    pub fn new(id: SenderId, wire: Wire, target: HttpTarget, tips: &'static [Pubkey], min_tip: u64, headers: Vec<(String, String)>, max_rps: u32) -> Self {
        let opts = match id {
            SenderId::Jito => r#"{"encoding":"base64"}"#,
            _ => r#"{"encoding":"base64","skipPreflight":true,"maxRetries":0}"#,
        };
        Self {
            id,
            wire,
            tips,
            min_tip,
            path: target.path.clone(),
            headers,
            opts,
            conn: Mutex::new((HttpConn::new(target, Duration::from_millis(1_500)), Vec::with_capacity(4_096))),
            breaker: Breaker::new(5, 10_000),
            min_interval_ms: if max_rps == 0 { 0 } else { 1_000 / max_rps as u64 },
            last_ms: AtomicU64::new(0),
        }
    }

    pub fn breaker(&self) -> &Breaker {
        &self.breaker
    }

    /// Open the connection ahead of time.
    pub fn warm(&self) -> Result<(), SendError> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner()).0.warm()
    }

    fn rate_ok(&self, now: u64) -> bool {
        if self.min_interval_ms == 0 {
            return true;
        }
        self.last_ms
            .fetch_update(AcqRel, Acquire, |last| (now >= last + self.min_interval_ms).then_some(now))
            .is_ok()
    }

    /// Send several transactions in one request where the wire allows it
    /// (Nozomi batch; others send them one by one).
    pub fn send_many(&self, vs: &[&Variant]) -> SendResult {
        for v in vs {
            if let Err(e) = check_tip(v, self.tips, self.min_tip) {
                return SendResult::Failed(e);
            }
        }
        if self.wire != Wire::NozomiBatch {
            let mut last = SendResult::Recorded;
            for v in vs {
                last = self.send(v);
                if !last.is_ok() {
                    return last;
                }
            }
            return last;
        }
        self.request(|body| {
            body.clear();
            for v in vs {
                body.extend_from_slice(&(v.bytes().len() as u16).to_be_bytes());
                body.extend_from_slice(v.bytes());
            }
        })
    }

    fn request(&self, fill: impl FnOnce(&mut Vec<u8>)) -> SendResult {
        let now = now_ms();
        if !self.breaker.allow(now) {
            return SendResult::Failed(SendError::BreakerOpen);
        }
        if !self.rate_ok(now) {
            return SendResult::Failed(SendError::RateLimited);
        }
        let mut g = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let (conn, body) = &mut *g;
        fill(body);
        let ct = match self.wire {
            Wire::NozomiBatch | Wire::Raw => "application/octet-stream",
            _ => "application/json",
        };
        let hdrs: Vec<(&str, &str)> = self.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let t0 = Instant::now();
        let r = conn.post(&self.path, ct, &hdrs, body);
        let latency_ns = t0.elapsed().as_nanos() as u64;
        match r {
            Ok(s) if (200..300).contains(&s) => {
                // JSON-RPC errors come back as 200 with an "error" member
                if matches!(self.wire, Wire::JsonRpc) && contains(conn.last_body(), b"\"error\"") {
                    self.breaker.on_failure(now_ms());
                    let msg = String::from_utf8_lossy(conn.last_body()).chars().take(200).collect();
                    return SendResult::Failed(SendError::Rpc(msg));
                }
                self.breaker.on_success();
                SendResult::Accepted { latency_ns }
            }
            Ok(s) => {
                self.breaker.on_failure(now_ms());
                SendResult::Failed(if s == 429 { SendError::RateLimited } else { SendError::Http(s) })
            }
            Err(e) => {
                self.breaker.on_failure(now_ms());
                SendResult::Failed(e)
            }
        }
    }
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

impl Sender for HttpSender {
    fn id(&self) -> SenderId {
        self.id
    }
    fn tip_accounts(&self) -> &[Pubkey] {
        self.tips
    }
    fn min_tip(&self) -> u64 {
        self.min_tip
    }
    fn send(&self, v: &Variant) -> SendResult {
        if let Err(e) = check_tip(v, self.tips, self.min_tip) {
            return SendResult::Failed(e);
        }
        let opts = self.opts;
        match self.wire {
            Wire::JsonRpc => self.request(|b| json_rpc_send_body(b, "sendTransaction", v.bytes(), opts)),
            Wire::NozomiBatch => self.request(|b| {
                b.clear();
                b.extend_from_slice(&(v.bytes().len() as u16).to_be_bytes());
                b.extend_from_slice(v.bytes());
            }),
            Wire::Raw => self.request(|b| {
                b.clear();
                b.extend_from_slice(v.bytes());
            }),
            Wire::NextBlock => self.request(|b| {
                use base64::Engine as _;
                b.clear();
                b.extend_from_slice(br#"{"transaction":{"content":""#);
                let start = b.len();
                b.resize(start + base64::encoded_len(v.bytes().len(), true).unwrap_or(0), 0);
                let n = base64::engine::general_purpose::STANDARD.encode_slice(v.bytes(), &mut b[start..]).unwrap_or(0);
                b.truncate(start + n);
                b.extend_from_slice(br#""},"frontRunningProtection":false}"#);
            }),
        }
    }
}

/// Jito `sendBundle` (up to 5 transactions, all-or-nothing, revert-protected).
pub fn jito_bundle_body(out: &mut Vec<u8>, txs: &[&[u8]]) {
    use base64::Engine as _;
    out.clear();
    out.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"sendBundle","params":[["#);
    for (i, t) in txs.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.push(b'"');
        let start = out.len();
        out.resize(start + base64::encoded_len(t.len(), true).unwrap_or(0), 0);
        let n = base64::engine::general_purpose::STANDARD.encode_slice(t, &mut out[start..]).unwrap_or(0);
        out.truncate(start + n);
        out.push(b'"');
    }
    out.extend_from_slice(br#"],{"encoding":"base64"}]}"#);
}

/// Build a configured provider (`name` = config table name).
pub fn build(name: &str, c: &HttpSenderCfg, keys: &ApiKeys) -> Result<Box<dyn Sender>, String> {
    let key = |what: &str| -> Result<String, String> {
        if c.key.is_empty() {
            return Err(format!("{what}: `key` (credential name) missing"));
        }
        keys.get(&c.key).map(str::to_string).ok_or_else(|| format!("{what}: api key {:?} not in the api-keys credential", c.key))
    };
    let url = |default: &str| if c.url.is_empty() { default.to_string() } else { c.url.clone() };
    let s = match name {
        "helius" => {
            let mut t = HttpTarget::parse(&url(endpoints::HELIUS))?;
            if c.swqos_only {
                t = t.with_query("swqos_only=true");
            }
            if !c.key.is_empty() {
                t = t.with_query(&format!("api-key={}", key("helius")?));
            }
            let min = if c.min_tip > 0 { c.min_tip } else if c.swqos_only { 5_000 } else { 1_000_000 };
            HttpSender::new(SenderId::Helius, Wire::JsonRpc, t, &HELIUS_TIP_ACCOUNTS, min, vec![], c.max_rps)
        }
        "nozomi" => {
            let t = HttpTarget::parse(&url(endpoints::NOZOMI))?;
            let t = t.with_path(&format!("/api/sendBatch?c={}", key("nozomi")?));
            HttpSender::new(SenderId::Nozomi, Wire::NozomiBatch, t, &NOZOMI_TIP_ACCOUNTS, c.min_tip.max(1_000_000), vec![], c.max_rps)
        }
        "astralane" => {
            let t = HttpTarget::parse(&url(endpoints::ASTRALANE))?;
            let t = t.with_query(&format!("api-key={}&method=sendTransaction", key("astralane")?));
            let min = if c.min_tip > 0 { c.min_tip } else { 1_000_000 };
            HttpSender::new(SenderId::Astralane, Wire::Raw, t, &ASTRALANE_TIP_ACCOUNTS, min, vec![], c.max_rps)
        }
        "jito" => {
            let base = if c.region.is_empty() || c.region == "auto" {
                url(endpoints::JITO)
            } else {
                format!("http://{}.mainnet.block-engine.jito.wtf", c.region)
            };
            let t = HttpTarget::parse(&base)?.with_path("/api/v1/transactions?bundleOnly=true");
            let headers = if c.key.is_empty() { vec![] } else { vec![("x-jito-auth".to_string(), key("jito")?)] };
            let min = if c.min_tip > 0 { c.min_tip } else { 1_000 };
            HttpSender::new(SenderId::Jito, Wire::JsonRpc, t, &pump_codec::constants::JITO_TIP_ACCOUNTS, min, headers, c.max_rps.max(1))
        }
        "nextblock" => {
            let t = HttpTarget::parse(&url(endpoints::NEXTBLOCK))?.with_path("/api/v2/submit");
            let min = if c.min_tip > 0 { c.min_tip } else { 1_000_000 };
            HttpSender::new(SenderId::NextBlock, Wire::NextBlock, t, &NEXTBLOCK_TIP_ACCOUNTS, min, vec![("Authorization".to_string(), key("nextblock")?)], c.max_rps)
        }
        other => return Err(format!("unknown sender {other}")),
    };
    Ok(Box::new(s))
}
