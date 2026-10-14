//! `RpcSend` (feature `rpc`): JSON-RPC `sendTransaction` with
//! `skipPreflight: true, maxRetries: 0` (a stale variant must never be
//! re-broadcast by the RPC). Default for sells (`rpcsend.sells_only`);
//! blocking TLS, so run it on a sender thread, never the feed thread.

use super::{now_ms, Breaker, SendError, SendResult, Sender, SenderId, Variant};
use crate::rpc::RpcClient;
use base64::Engine as _;
use serde_json::json;
use std::time::Instant;

#[derive(Debug)]
pub struct RpcSend {
    client: RpcClient,
    breaker: Breaker,
}

impl RpcSend {
    pub fn new(url: &str) -> Result<Self, String> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!("bad rpc url {url:?}"));
        }
        Ok(Self {
            client: RpcClient::new(url),
            breaker: Breaker::new(5, 10_000),
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
        if !self.breaker.allow(now_ms()) {
            return SendResult::Failed(SendError::BreakerOpen);
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(v.bytes());
        let t0 = Instant::now();
        let r = self.client.call(
            "sendTransaction",
            json!([b64, {"encoding": "base64", "skipPreflight": true, "maxRetries": 0}]),
        );
        match r {
            Ok(_) => {
                self.breaker.on_success();
                SendResult::Accepted {
                    latency_ns: t0.elapsed().as_nanos() as u64,
                }
            }
            Err(e) => {
                self.breaker.on_failure(now_ms());
                SendResult::Failed(SendError::Rpc(e))
            }
        }
    }
}
