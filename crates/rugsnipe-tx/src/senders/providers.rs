//! Same-building HTTP senders (feature `senders`). Endpoints, tip accounts
//! and minimum tips from docs/03 section 6 and `sol-trade-sdk`
//! `src/constants/swqos.rs` (2026-10-14).
//!
//! | provider | request | min tip (default) |
//! |---|---|---|
//! | Helius Sender `/fast` | JSON-RPC `sendTransaction` (`skipPreflight`, `maxRetries: 0`); `?swqos_only=true` | 0.001 SOL; 5,000 lamports with `swqos_only` |
//! | Nozomi | Batch Send `POST /api/sendBatch?c=KEY`, body `(u16 BE len ‖ tx)*`, octet-stream | 0.001 SOL |
//! | Astralane | `POST /irisb?api-key=KEY&method=sendTransaction`, raw tx bytes | 0.001 SOL (config) |
//! | Jito block engine (one sender per region, `frankfurt`, `amsterdam`) | `POST /api/v1/transactions?bundleOnly=true` JSON-RPC `sendTransaction` (a failed bundle-only transaction is not committed: costs nothing); `sendBundle` on `/api/v1/bundles` | 1,000 lamports; 1 req/s per IP per region (token bucket) |
//! | NextBlock | `POST /api/v2/submit`, `Authorization: KEY`, JSON `{"transaction":{"content":b64},"frontRunningProtection":false}` | 0.001 SOL |
//! | BlockRazor (docs.blockrazor.io, 2026-10-16) | `POST /sendTransaction`, header `apikey: KEY`, JSON `{"transaction":b64,"mode":"fast","revertProtection":false}`; keep-alive `POST /health` | 100,000 lamports (0.0001 SOL); 3 TPS |
//!
//! The Helius key goes in the query (`/fast?api-key=KEY`, 50 req/s per
//! region; keyless is 1 req/s per IP); the `swqos_only` leg is a second
//! sender on the same nonce.

use super::http::{json_rpc_send_body, HttpConn, HttpTarget};
use super::{
    check_tip, now_ms, ApiKeys, Breaker, SendError, SendResult, Sender, SenderId, TokenBucket,
    Variant,
};
use pump_codec::Pubkey;
use rugsnipe_core::config::HttpSenderCfg;
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

/// BlockRazor tip accounts (docs.blockrazor.io "Priority Fee & Tip",
/// 2026-10-16; `GET /getTipAccounts` serves the current list).
pub const BLOCKRAZOR_TIP_ACCOUNTS: [Pubkey; 15] = [
    pk("68Pwb4jS7eZATjDfhmTXgRJjCiZmw1L7Huy4HNpnxJ3o"),
    pk("4ABhJh5rZPjv63RBJBuyWzBK3g9gWMUQdTZP2kiW31V9"),
    pk("B2M4NG5eyZp5SBQrSdtemzk5TqVuaWGQnowGaCBt8GyM"),
    pk("5jA59cXMKQqZAVdtopv8q3yyw9SYfiE3vUCbt7p8MfVf"),
    pk("5YktoWygr1Bp9wiS1xtMtUki1PeYuuzuCF98tqwYxf61"),
    pk("295Avbam4qGShBYK7E9H5Ldew4B3WyJGmgmXfiWdeeyV"),
    pk("EDi4rSy2LZgKJX74mbLTFk4mxoTgT6F7HxxzG2HBAFyK"),
    pk("BnGKHAC386n4Qmv9xtpBVbRaUTKixjBe3oagkPFKtoy6"),
    pk("Dd7K2Fp7AtoN8xCghKDRmyqr5U169t48Tw5fEd3wT9mq"),
    pk("AP6qExwrbRgBAVaehg4b5xHENX815sMabtBzUzVB4v8S"),
    pk("8EpzWhsr7uyZkfVD7MTHoWqupM14xngzMeUcCkWbY1UG"),
    pk("56XRik27PddNnR7JkHKqd5pWdMHeqje7TCm1gsdmvnTP"),
    pk("2DJNtR1tT7vfub7sDJ6v9NGmrzGVFUer5FvjhEcWEpvv"),
    pk("3AveZTS3PJo5qKEqHJci5bjW8neufriirr9mJXfsv4iH"),
    pk("9DC798ezxU2f2i8ggQ4vsqrsNxdDWJWAFg4cT1xLDR1X"),
];

/// Minimum tips (lamports) below which a provider ignores or drops a
/// transaction; a configured `min_tip` can only raise these.
pub mod min_tips {
    pub const HELIUS: u64 = 1_000_000;
    pub const HELIUS_SWQOS: u64 = 5_000;
    /// Nozomi silently drops tips below 0.001 SOL.
    pub const NOZOMI: u64 = 1_000_000;
    pub const ASTRALANE: u64 = 1_000_000;
    pub const JITO: u64 = 1_000;
    pub const NEXTBLOCK: u64 = 1_000_000;
    pub const BLOCKRAZOR: u64 = 100_000;
}

/// Default endpoints (Frankfurt).
pub mod endpoints {
    pub const HELIUS: &str = "http://fra-sender.helius-rpc.com/fast";
    pub const NOZOMI: &str = "http://fra2.nozomi.temporal.xyz";
    pub const ASTRALANE: &str = "http://fr.gateway.astralane.io/irisb";
    pub const JITO: &str = "http://frankfurt.mainnet.block-engine.jito.wtf";
    pub const NEXTBLOCK: &str = "http://fra.nextblock.io";
    pub const BLOCKRAZOR: &str = "http://frankfurt.solana.blockrazor.xyz:443/sendTransaction";
}

/// `jito_<region>` as a static label (unknown regions: `jito`).
pub fn jito_label(region: &str) -> &'static str {
    match region {
        "frankfurt" => "jito_frankfurt",
        "amsterdam" => "jito_amsterdam",
        "london" => "jito_london",
        "dublin" => "jito_dublin",
        "ny" => "jito_ny",
        "slc" => "jito_slc",
        "tokyo" => "jito_tokyo",
        "singapore" => "jito_singapore",
        _ => "jito",
    }
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
    /// BlockRazor JSON (`mode = fast`).
    BlockRazor,
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
    /// Requests per second (`max_rps`; 0 = unlimited); the burst is the
    /// rate capped at 4, so both variants of an intent go out together on a
    /// path that allows 2 req/s or more, and Jito (1 req/s) takes one.
    bucket: TokenBucket,
    label: &'static str,
    buy_variants: u8,
}

impl core::fmt::Debug for HttpSender {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // never print headers or the path: they can hold API keys
        f.debug_struct("HttpSender")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("min_tip", &self.min_tip)
            .finish_non_exhaustive()
    }
}

impl HttpSender {
    /// Low-level constructor (tests, custom endpoints).
    pub fn new(
        id: SenderId,
        wire: Wire,
        target: HttpTarget,
        tips: &'static [Pubkey],
        min_tip: u64,
        headers: Vec<(String, String)>,
        max_rps: u32,
    ) -> Self {
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
            conn: Mutex::new((
                HttpConn::new(target, Duration::from_millis(1_500)),
                Vec::with_capacity(4_096),
            )),
            breaker: Breaker::new(5, 10_000),
            bucket: TokenBucket::new(max_rps as f64, max_rps.clamp(1, 4) as f64),
            label: id.name(),
            buy_variants: if id == SenderId::Jito { 1 } else { 2 },
        }
    }

    /// Ledger / summary name (`jito_frankfurt`, ...).
    pub fn with_label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }

    /// The request path (tests; may hold an API key: never print it).
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn breaker(&self) -> &Breaker {
        &self.breaker
    }

    /// Open the connection ahead of time.
    pub fn warm(&self) -> Result<(), SendError> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner()).0.warm()
    }

    /// Renew the idle connection ([`HttpConn::refresh`]); errors are the
    /// next send's problem (it reconnects itself).
    pub fn keepalive(&self) {
        let _ = self
            .conn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0
            .refresh();
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
        if !self.bucket.try_take() {
            return SendResult::Failed(SendError::RateLimited);
        }
        let mut g = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let (conn, body) = &mut *g;
        fill(body);
        let ct = match self.wire {
            Wire::NozomiBatch | Wire::Raw => "application/octet-stream",
            _ => "application/json",
        };
        let json_errors = matches!(self.wire, Wire::JsonRpc | Wire::BlockRazor);
        let hdrs: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let t0 = Instant::now();
        let r = conn.post(&self.path, ct, &hdrs, body);
        let latency_ns = t0.elapsed().as_nanos() as u64;
        match r {
            Ok(s) if (200..300).contains(&s) => {
                // JSON-RPC errors come back as 200 with an "error" member;
                // BlockRazor's success reply carries an empty one
                // (`{"signature":"...","error":""}`), which is not an error
                if json_errors && body_has_error(conn.last_body()) {
                    self.breaker.on_failure(now_ms());
                    let msg = String::from_utf8_lossy(conn.last_body())
                        .chars()
                        .take(200)
                        .collect();
                    return SendResult::Failed(SendError::Rpc(msg));
                }
                self.breaker.on_success();
                SendResult::Accepted { latency_ns }
            }
            Ok(s) => {
                self.breaker.on_failure(now_ms());
                SendResult::Failed(if s == 429 {
                    SendError::RateLimited
                } else {
                    SendError::Http(s)
                })
            }
            Err(e) => {
                self.breaker.on_failure(now_ms());
                SendResult::Failed(e)
            }
        }
    }
}

/// An `"error"` member whose value is not the empty string (`"error":""`
/// and `"error": ""` are BlockRazor's "no error").
fn body_has_error(body: &[u8]) -> bool {
    let key = b"\"error\"";
    let Some(i) = body.windows(key.len()).position(|w| w == key) else {
        return false;
    };
    let rest = &body[i + key.len()..];
    let rest = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or(&rest[0..0], |j| &rest[j..]);
    let Some(rest) = rest.strip_prefix(b":") else {
        return true;
    };
    let rest = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or(&rest[0..0], |j| &rest[j..]);
    !rest.starts_with(b"\"\"")
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

fn push_b64(b: &mut Vec<u8>, bytes: &[u8]) {
    use base64::Engine as _;
    let start = b.len();
    b.resize(
        start + base64::encoded_len(bytes.len(), true).unwrap_or(0),
        0,
    );
    let n = base64::engine::general_purpose::STANDARD
        .encode_slice(bytes, &mut b[start..])
        .unwrap_or(0);
    b.truncate(start + n);
}

impl Sender for HttpSender {
    fn id(&self) -> SenderId {
        self.id
    }
    fn label(&self) -> &'static str {
        self.label
    }
    fn buy_variants(&self) -> u8 {
        self.buy_variants
    }
    fn warm(&self) -> Result<(), SendError> {
        HttpSender::warm(self)
    }
    fn keepalive(&self) {
        HttpSender::keepalive(self)
    }
    fn send_batch(&self, vs: &[&Variant]) -> Vec<SendResult> {
        if self.wire == Wire::NozomiBatch && vs.len() > 1 {
            let r = self.send_many(vs);
            return vs.iter().map(|_| r.clone()).collect();
        }
        vs.iter().map(|v| self.send(v)).collect()
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
            Wire::JsonRpc => {
                self.request(|b| json_rpc_send_body(b, "sendTransaction", v.bytes(), opts))
            }
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
                b.clear();
                b.extend_from_slice(br#"{"transaction":{"content":""#);
                push_b64(b, v.bytes());
                b.extend_from_slice(br#""},"frontRunningProtection":false}"#);
            }),
            Wire::BlockRazor => self.request(|b| {
                b.clear();
                b.extend_from_slice(br#"{"transaction":""#);
                push_b64(b, v.bytes());
                b.extend_from_slice(br#"","mode":"fast","revertProtection":false}"#);
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
        let n = base64::engine::general_purpose::STANDARD
            .encode_slice(t, &mut out[start..])
            .unwrap_or(0);
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
        keys.get(&c.key)
            .map(str::to_string)
            .ok_or_else(|| format!("{what}: api key {:?} not in the api-keys credential", c.key))
    };
    let url = |default: &str| {
        if c.url.is_empty() {
            default.to_string()
        } else {
            c.url.clone()
        }
    };
    let s = match name {
        "helius" => {
            let mut t = HttpTarget::parse(&url(endpoints::HELIUS))?;
            if c.swqos_only {
                t = t.with_query("swqos_only=true");
            }
            if !c.key.is_empty() {
                t = t.with_query(&format!("api-key={}", key("helius")?));
            }
            let (id, floor) = if c.swqos_only {
                (SenderId::HeliusSwqos, min_tips::HELIUS_SWQOS)
            } else {
                (SenderId::Helius, min_tips::HELIUS)
            };
            // keyless: 1 req/s per IP; with a key 50 req/s per region
            let rps = match (c.max_rps, c.key.is_empty()) {
                (0, true) => 1,
                (0, false) => 50,
                (n, _) => n,
            };
            HttpSender::new(
                id,
                Wire::JsonRpc,
                t,
                &HELIUS_TIP_ACCOUNTS,
                c.min_tip.max(floor),
                vec![],
                rps,
            )
        }
        "nozomi" => {
            let t = HttpTarget::parse(&url(endpoints::NOZOMI))?;
            let t = t.with_path(&format!("/api/sendBatch?c={}", key("nozomi")?));
            HttpSender::new(
                SenderId::Nozomi,
                Wire::NozomiBatch,
                t,
                &NOZOMI_TIP_ACCOUNTS,
                c.min_tip.max(min_tips::NOZOMI),
                vec![],
                if c.max_rps == 0 { 5 } else { c.max_rps },
            )
        }
        "astralane" => {
            let t = HttpTarget::parse(&url(endpoints::ASTRALANE))?;
            let t = t.with_query(&format!(
                "api-key={}&method=sendTransaction",
                key("astralane")?
            ));
            // VIP tiers lower the minimum: a configured min_tip may go below
            // the free-tier 0.001 SOL, never below VIP3's 10,000 lamports
            let min = if c.min_tip > 0 {
                c.min_tip.max(10_000)
            } else {
                min_tips::ASTRALANE
            };
            HttpSender::new(
                SenderId::Astralane,
                Wire::Raw,
                t,
                &ASTRALANE_TIP_ACCOUNTS,
                min,
                vec![],
                if c.max_rps == 0 { 5 } else { c.max_rps },
            )
        }
        "jito" => {
            // an explicit url wins (tests, proxies); else the region's host
            let region = if c.region.is_empty() || c.region == "auto" {
                "frankfurt"
            } else {
                c.region.as_str()
            };
            if !region.chars().all(|ch| ch.is_ascii_lowercase()) {
                return Err(format!("jito region {region:?}"));
            }
            let base = if c.url.is_empty() {
                format!("http://{region}.mainnet.block-engine.jito.wtf")
            } else {
                c.url.clone()
            };
            let t = HttpTarget::parse(&base)?.with_path("/api/v1/transactions?bundleOnly=true");
            let headers = if c.key.is_empty() {
                vec![]
            } else {
                vec![("x-jito-auth".to_string(), key("jito")?)]
            };
            HttpSender::new(
                SenderId::Jito,
                Wire::JsonRpc,
                t,
                &pump_codec::constants::JITO_TIP_ACCOUNTS,
                c.min_tip.max(min_tips::JITO),
                headers,
                c.max_rps.max(1),
            )
            .with_label(jito_label(region))
        }
        "blockrazor" => {
            let t = HttpTarget::parse(&url(endpoints::BLOCKRAZOR))?;
            HttpSender::new(
                SenderId::BlockRazor,
                Wire::BlockRazor,
                t,
                &BLOCKRAZOR_TIP_ACCOUNTS,
                c.min_tip.max(min_tips::BLOCKRAZOR),
                vec![("apikey".to_string(), key("blockrazor")?)],
                if c.max_rps == 0 { 3 } else { c.max_rps },
            )
        }
        "nextblock" => {
            let t = HttpTarget::parse(&url(endpoints::NEXTBLOCK))?.with_path("/api/v2/submit");
            let min = c.min_tip.max(min_tips::NEXTBLOCK);
            HttpSender::new(
                SenderId::NextBlock,
                Wire::NextBlock,
                t,
                &NEXTBLOCK_TIP_ACCOUNTS,
                min,
                vec![("Authorization".to_string(), key("nextblock")?)],
                c.max_rps,
            )
        }
        other => return Err(format!("unknown sender {other}")),
    };
    Ok(Box::new(s))
}

#[cfg(test)]
mod body_error_tests {
    use super::body_has_error;

    #[test]
    fn empty_error_member_is_success() {
        assert!(!body_has_error(br#"{"signature":"56oj","error":""}"#));
        assert!(!body_has_error(br#"{"signature":"56oj", "error": ""}"#));
        assert!(body_has_error(
            br#"{"signature":"","error":"rpc error: code = Unknown desc = illegal transaction"}"#
        ));
        assert!(body_has_error(
            br#"{"jsonrpc":"2.0","error":{"code":-32002,"message":"x"},"id":1}"#
        ));
        assert!(!body_has_error(
            br#"{"jsonrpc":"2.0","result":"sig","id":1}"#
        ));
    }
}
