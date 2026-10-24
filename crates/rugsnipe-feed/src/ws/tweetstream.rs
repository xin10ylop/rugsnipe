//! The TweetStream websocket client (`entry.tweet_feed`, docs/14 section 7):
//! connect with the key as a subprotocol (`tweetstream.v1`,
//! `tweetstream.auth.token.<key>`), send `auth_ping`, hand every text frame
//! and its receipt time (unix ms, taken before parsing) to the caller, and
//! reconnect with jittered backoff (1 s doubling to 30 s; 30 s up resets it)
//! until `stop`. The server pings every 30 s; tungstenite answers. The key is
//! never logged: handshake errors carry the HTTP status only.

use super::net;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

/// What the client tells its caller besides frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Connected,
    /// The connection ended (or never opened): why, without the key.
    Closed(String),
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

async fn sleep_or_stop(stop: &AtomicBool, ms: u64) {
    let mut left = ms;
    while left > 0 && !stop.load(Ordering::Acquire) {
        let d = left.min(200);
        tokio::time::sleep(Duration::from_millis(d)).await;
        left -= d;
    }
}

/// Run the client on the calling thread (its own current-thread runtime)
/// until `stop` is set.
pub fn run_blocking(
    url: &str,
    key: &str,
    stop: &AtomicBool,
    on_status: &mut dyn FnMut(Status),
    on_frame: &mut dyn FnMut(&str, u64),
) {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        on_status(Status::Closed("no runtime".into()));
        return;
    };
    rt.block_on(run(url, key, stop, on_status, on_frame));
}

async fn run(
    url: &str,
    key: &str,
    stop: &AtomicBool,
    on_status: &mut dyn FnMut(Status),
    on_frame: &mut dyn FnMut(&str, u64),
) {
    let auth = format!("tweetstream.auth.token.{key}");
    let protocols = ["tweetstream.v1", auth.as_str()];
    let mut backoff = 1_000u64;
    while !stop.load(Ordering::Acquire) {
        let opened = Instant::now();
        let why = match net::ws_connect_protocols(url, &protocols, Duration::from_secs(10)).await {
            Err(e) => e.to_string(),
            Ok(mut ws) => {
                on_status(Status::Connected);
                let mut why = "closed by the server".to_string();
                if ws
                    .send(Message::Text(r#"{"op":"auth_ping"}"#.into()))
                    .await
                    .is_err()
                {
                    why = "auth_ping not sent".into();
                } else {
                    while !stop.load(Ordering::Acquire) {
                        let next =
                            tokio::time::timeout(Duration::from_millis(500), ws.next()).await;
                        let msg = match next {
                            Err(_) => continue, // nothing for 500 ms: look at stop
                            Ok(m) => m,
                        };
                        match msg {
                            Some(Ok(Message::Text(t))) => on_frame(t.as_str(), now_ms()),
                            Some(Ok(Message::Close(_))) | None => break,
                            Some(Ok(_)) => {} // ping / pong / binary
                            Some(Err(e)) => {
                                why = format!("read: {e}");
                                break;
                            }
                        }
                    }
                }
                why
            }
        };
        if stop.load(Ordering::Acquire) {
            break;
        }
        on_status(Status::Closed(why.replace(key, "<key>")));
        if opened.elapsed() >= Duration::from_secs(30) {
            backoff = 1_000;
        }
        let jit = 800 + (now_ms() % 401); // 0.8 .. 1.2
        sleep_or_stop(stop, backoff * jit / 1000).await;
        backoff = (backoff * 2).min(30_000);
    }
}
