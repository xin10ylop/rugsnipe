//! `RpcSend` (feature `rpc`): JSON-RPC `sendTransaction` with
//! `skipPreflight: true, maxRetries: 0` (a stale variant must never be
//! re-broadcast by the RPC). Default for sells (`rpcsend.sells_only`);
//! blocking TLS, so run it on a sender thread, never the feed thread.

use super::{now_ms, Breaker, SendError, SendResult, Sender, SenderId, Variant, VariantKind};
use crate::rpc::RpcClient;
use base64::Engine as _;
use serde_json::json;
use std::time::Instant;

#[derive(Debug)]
pub struct RpcSend {
    client: RpcClient,
    breaker: Breaker,
    /// `rpcsend.sells_only`: buy variants are refused before the network.
    sells_only: bool,
}

impl RpcSend {
    pub fn new(url: &str, sells_only: bool) -> Result<Self, String> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!("bad rpc url {url:?}"));
        }
        Ok(Self {
            client: RpcClient::new(url),
            breaker: Breaker::new(5, 10_000),
            sells_only,
        })
    }
}

impl Sender for RpcSend {
    fn id(&self) -> SenderId {
        SenderId::Rpc
    }
    fn send(&self, v: &Variant) -> SendResult {
        if v.too_large() {
            return SendResult::Failed(SendError::TooLarge);
        }
        if self.sells_only && v.kind == VariantKind::Buy {
            return SendResult::Failed(SendError::NotForThisSender);
        }
        // a sell is never blocked by the breaker (one at a time as the
        // half-open probe of an open one) and its failure never opens it
        let sell = v.kind == VariantKind::Sell;
        let probe = if self.breaker.allow(now_ms()) {
            false
        } else if sell && self.breaker.try_probe() {
            true
        } else {
            return SendResult::Failed(SendError::BreakerOpen);
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(v.bytes());
        let t0 = Instant::now();
        let r = self.client.call(
            "sendTransaction",
            json!([b64, {"encoding": "base64", "skipPreflight": true, "maxRetries": 0}]),
        );
        if probe {
            self.breaker.end_probe();
        }
        match r {
            Ok(_) => {
                self.breaker.on_success();
                SendResult::Accepted {
                    latency_ns: t0.elapsed().as_nanos() as u64,
                }
            }
            Err(e) => {
                if !sell {
                    self.breaker.on_failure(now_ms());
                }
                SendResult::Failed(SendError::Rpc(e))
            }
        }
    }
    fn ready(&self, kind: VariantKind, now_ms: u64) -> bool {
        self.breaker.allows(kind, now_ms)
    }
    /// `sells_only`: no buy variant is planned for this path (it would only
    /// come back `NotForThisSender` and count as a send error).
    fn buy_variants(&self) -> u8 {
        if self.sells_only {
            0
        } else {
            2
        }
    }
}
