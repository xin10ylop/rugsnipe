//! Telegram alerts (optional; docs/06 section 13 "Critical alerts direct
//! from the hot box"). Credentials `[obs] alert.telegram` (bot token) and
//! `alert.chat` (chat id) are names in the `api-keys` credential (or files
//! of those names); without both nothing is sent.
//!
//! The cold thread queues messages ([`Alerts::send`]: a channel push, never
//! a wait); a task on the cold runtime posts them (`sendMessage`, JSON over
//! HTTPS, the same client as the cold JSON-RPC lanes). The hot thread never
//! touches it. Sent: start, every kill (with the trigger and its detail),
//! an hourly summary; optionally `/kill` from the configured chat is polled
//! (`getUpdates`) and latches the kill (unkill stays local). The bot token
//! is never logged; errors name the HTTP failure only.

use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

/// Messages waiting at most (older ones are dropped, counted).
pub const QUEUE: usize = 64;

/// Counters (summary).
#[derive(Debug, Default)]
pub struct AlertStats {
    pub queued: AtomicU64,
    pub sent: AtomicU64,
    pub failed: AtomicU64,
    pub dropped: AtomicU64,
    pub kill_commands: AtomicU64,
}

/// The cold thread's handle.
#[derive(Debug, Clone)]
pub struct Alerts {
    tx: Option<mpsc::Sender<String>>,
    pub stats: Arc<AlertStats>,
    /// Set by the poller when an authorised `/kill` arrives.
    pub kill_requested: Arc<AtomicBool>,
}

impl Alerts {
    /// No Telegram configured: every send is a no-op.
    pub fn disabled() -> Self {
        Self {
            tx: None,
            stats: Arc::default(),
            kill_requested: Arc::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.tx.is_some()
    }

    /// Queue a message (never blocks; a full queue drops it).
    pub fn send(&self, text: impl Into<String>) {
        let Some(tx) = &self.tx else { return };
        self.stats.queued.fetch_add(1, Ordering::Relaxed);
        if tx.try_send(text.into()).is_err() {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Start the sender (and the `/kill` poller) on the current tokio
    /// runtime. `api_base` is `https://api.telegram.org` (tests: a local server).
    pub fn start(
        api_base: &str,
        token: Zeroizing<String>,
        chat: Zeroizing<String>,
        poll_commands: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel(QUEUE);
        let me = Self {
            tx: Some(tx),
            stats: Arc::default(),
            kill_requested: Arc::default(),
        };
        let base = format!("{}/bot{}", api_base.trim_end_matches('/'), token.as_str());
        tokio::spawn(sender(
            Zeroizing::new(format!("{base}/sendMessage")),
            chat.clone(),
            rx,
            me.stats.clone(),
        ));
        if poll_commands {
            tokio::spawn(poller(
                Zeroizing::new(format!("{base}/getUpdates")),
                chat,
                me.kill_requested.clone(),
                me.stats.clone(),
            ));
        }
        me
    }
}

/// The `sendMessage` body.
pub fn message_body(chat: &str, text: &str) -> String {
    serde_json::json!({"chat_id": chat, "text": text, "disable_web_page_preview": true}).to_string()
}

async fn sender(
    url: Zeroizing<String>,
    chat: Zeroizing<String>,
    mut rx: mpsc::Receiver<String>,
    stats: Arc<AlertStats>,
) {
    let Ok(mut c) = rugsnipe_feed::ws::RpcClient::new(&url) else {
        return;
    };
    while let Some(text) = rx.recv().await {
        let body = message_body(&chat, &text);
        let ok = matches!(
            c.call(&body).await.ok().and_then(|r| serde_json::from_str::<Value>(&r).ok()),
            Some(v) if v["ok"].as_bool() == Some(true)
        );
        let k = if ok { &stats.sent } else { &stats.failed };
        k.fetch_add(1, Ordering::Relaxed);
    }
}

/// `/kill` from the configured chat in a `getUpdates` result; returns the
/// next offset.
pub fn kill_in_updates(v: &Value, chat: &str, offset: i64) -> (bool, i64) {
    let mut kill = false;
    let mut next = offset;
    for u in v["result"].as_array().into_iter().flatten() {
        let id = u["update_id"].as_i64().unwrap_or(0);
        next = next.max(id + 1);
        let m = &u["message"];
        let from_chat = m["chat"]["id"].as_i64().map(|i| i.to_string());
        let text = m["text"].as_str().unwrap_or("").trim();
        if from_chat.as_deref() == Some(chat) && (text == "/kill" || text.starts_with("/kill@")) {
            kill = true;
        }
    }
    (kill, next)
}

async fn poller(
    url: Zeroizing<String>,
    chat: Zeroizing<String>,
    kill: Arc<AtomicBool>,
    stats: Arc<AlertStats>,
) {
    let Ok(mut c) = rugsnipe_feed::ws::RpcClient::new(&url) else {
        return;
    };
    let mut offset = 0i64;
    loop {
        let body =
            serde_json::json!({"offset": offset, "timeout": 0, "allowed_updates": ["message"]})
                .to_string();
        if let Some(v) = c
            .call(&body)
            .await
            .ok()
            .and_then(|r| serde_json::from_str::<Value>(&r).ok())
        {
            let (k, next) = kill_in_updates(&v, &chat, offset);
            offset = next;
            if k {
                stats.kill_commands.fetch_add(1, Ordering::Relaxed);
                kill.store(true, Ordering::Release);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_and_commands() {
        let b: Value = serde_json::from_str(&message_body("-100", "KILL send_errors")).unwrap();
        assert_eq!(b["chat_id"], "-100");
        assert_eq!(b["text"], "KILL send_errors");
        let v = serde_json::json!({"ok": true, "result": [
            {"update_id": 10, "message": {"chat": {"id": 555}, "text": "/kill"}},
            {"update_id": 11, "message": {"chat": {"id": -100}, "text": "hello"}},
        ]});
        assert_eq!(
            kill_in_updates(&v, "-100", 0),
            (false, 12),
            "/kill from another chat is ignored"
        );
        let v = serde_json::json!({"ok": true, "result": [
            {"update_id": 12, "message": {"chat": {"id": -100}, "text": "/kill@rugbot"}},
        ]});
        assert_eq!(kill_in_updates(&v, "-100", 12), (true, 13));
        let d = Alerts::disabled();
        d.send("x");
        assert!(!d.enabled());
        assert_eq!(d.stats.queued.load(Ordering::Relaxed), 0);
    }
}
