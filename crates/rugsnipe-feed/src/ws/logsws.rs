//! [`LogsWs`]: `logsSubscribe` over a Solana RPC websocket (free feed).
//!
//! * `mentions: [TSLvdd…]` (the pump mint authority, present in every create)
//!   at `processed` commitment -> [`Payload::Hit`] per create.
//! * Up to `max_watches` per-address subscriptions:
//!   - **funders** (`watch_funder`): each notification of a successful
//!     transaction queues a cold-path `getTransaction` (jsonParsed, confirmed,
//!     rate limited) whose system transfers by the funder become
//!     [`Payload::Funding`] events. The System program logs no amounts, so the
//!     logs alone cannot give them.
//!   - **mints** (`watch_mint`, e.g. held positions): every pump TradeEvent ->
//!     [`Payload::Trade`] for the exit engine.
//! * Reconnect with exponential backoff (jittered), client pings, a stale
//!   detector that forces a reconnect, raw-frame recording (bounded).
//!
//! Everything runs on one dedicated thread (current-thread tokio runtime);
//! events reach the hot loop through an `rtrb` ring ([`RingFeed`]).

use super::net::ws_connect;
use super::rpc::{get_transaction_body, RpcClient};
use crate::buf::FeedBuf;
use crate::clock::{mono_ns, unix_ns};
use crate::decode::{funding_from_get_transaction, parse_ws_message, FrameDecoder, WsMsg};
use crate::event::{FeedEvent, FeedSrc, FundingEvent, Payload};
use crate::health::FeedHealth;
use crate::record::{RecordConfig, Recorder};
use crate::ring::{ring, RingFeed, RingSink};
use crate::{Feed, FeedStatus};
use futures_util::{SinkExt, StreamExt};
use pump_codec::constants::MINT_AUTHORITY;
use pump_codec::Pubkey;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// LogsWs settings (`[feeds] logsws = { enabled, url, stale_ms }` plus knobs).
#[derive(Debug, Clone, PartialEq)]
pub struct LogsWsConfig {
    pub url: String,
    pub commitment: String,
    /// Subscribe to creates (`mentions: [mention]`).
    pub creates: bool,
    pub mention: Pubkey,
    pub stale_ms: u64,
    pub ping_ms: u64,
    pub connect_timeout_ms: u64,
    pub min_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub ring_capacity: usize,
    /// Maximum per-address subscriptions (funders + mints).
    pub max_watches: usize,
    pub watch_funders: Vec<Pubkey>,
    pub watch_mints: Vec<Pubkey>,
    /// HTTPS JSON-RPC endpoint for funding lookups (`None` disables them).
    pub rpc_url: Option<String>,
    pub rpc_rps: f64,
    pub rpc_queue: usize,
    /// Wait before `getTransaction` (processed -> confirmed).
    pub confirm_delay_ms: u64,
    /// Also emit the create transactions' TradeEvents as `Payload::Trade`.
    pub emit_create_trades: bool,
    pub record: Option<RecordConfig>,
}

impl Default for LogsWsConfig {
    fn default() -> Self {
        LogsWsConfig {
            url: "wss://api.mainnet-beta.solana.com".into(),
            commitment: "processed".into(),
            creates: true,
            mention: MINT_AUTHORITY,
            stale_ms: 10_000,
            ping_ms: 20_000,
            connect_timeout_ms: 20_000,
            min_backoff_ms: 1_000,
            max_backoff_ms: 60_000,
            ring_capacity: 8192,
            max_watches: 16,
            watch_funders: Vec::new(),
            watch_mints: Vec::new(),
            rpc_url: Some("https://api.mainnet-beta.solana.com".into()),
            rpc_rps: 2.0,
            rpc_queue: 256,
            confirm_delay_ms: 1_500,
            emit_create_trades: false,
            record: None,
        }
    }
}

/// Control messages to the websocket thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    WatchFunder(Pubkey),
    UnwatchFunder(Pubkey),
    WatchMint(Pubkey),
    UnwatchMint(Pubkey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SubKind {
    Creates,
    Funder(Pubkey),
    Mint(Pubkey),
}

/// The LogsWs adapter (a [`Feed`]).
#[derive(Debug)]
pub struct LogsWs {
    feed: RingFeed,
    ctl: mpsc::UnboundedSender<Control>,
    recorder: Option<Recorder>,
}

impl LogsWs {
    /// Start the websocket thread.
    pub fn spawn(cfg: LogsWsConfig) -> io::Result<LogsWs> {
        let (sink, mut feed, stop) = ring(FeedSrc::LogsWs, cfg.ring_capacity, cfg.stale_ms);
        let (ctl, ctl_rx) = mpsc::unbounded_channel();
        let recorder = match &cfg.record {
            Some(rc) => Some(Recorder::spawn(rc.clone())?.0),
            None => None,
        };
        let rec = recorder.clone();
        let h = std::thread::Builder::new()
            .name("feed-logsws".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(_) => return,
                };
                rt.block_on(run(cfg, sink, stop, ctl_rx, rec));
            })?;
        feed.set_thread(h);
        Ok(LogsWs {
            feed,
            ctl,
            recorder,
        })
    }

    /// Watch a funder (funding trigger). `false` if the thread is gone.
    pub fn watch_funder(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::WatchFunder(k)).is_ok()
    }

    pub fn unwatch_funder(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::UnwatchFunder(k)).is_ok()
    }

    /// Watch a mint's trades (held positions).
    pub fn watch_mint(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::WatchMint(k)).is_ok()
    }

    pub fn unwatch_mint(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::UnwatchMint(k)).is_ok()
    }

    /// A cloneable control handle (watch / unwatch from another thread after
    /// the adapter moved into a [`crate::FeedMux`]).
    pub fn handle(&self) -> LogsWsHandle {
        LogsWsHandle {
            ctl: self.ctl.clone(),
            health: self.feed.health_arc().clone(),
        }
    }

    pub fn health_arc(&self) -> &Arc<FeedHealth> {
        self.feed.health_arc()
    }

    pub fn recorder(&self) -> Option<&Recorder> {
        self.recorder.as_ref()
    }

    /// Stop the thread and wait for it.
    pub fn shutdown(&mut self) {
        self.feed.shutdown();
    }
}

impl Feed for LogsWs {
    #[inline]
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        self.feed.poll(out)
    }

    fn src(&self) -> FeedSrc {
        FeedSrc::LogsWs
    }

    fn health(&self) -> Option<&FeedHealth> {
        self.feed.health()
    }
}

/// Cloneable control handle of a [`LogsWs`] (cold threads use it to watch
/// held mints and funders; each call is one channel send).
#[derive(Debug, Clone)]
pub struct LogsWsHandle {
    ctl: mpsc::UnboundedSender<Control>,
    health: Arc<FeedHealth>,
}

impl LogsWsHandle {
    pub fn watch_funder(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::WatchFunder(k)).is_ok()
    }
    pub fn unwatch_funder(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::UnwatchFunder(k)).is_ok()
    }
    pub fn watch_mint(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::WatchMint(k)).is_ok()
    }
    pub fn unwatch_mint(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::UnwatchMint(k)).is_ok()
    }
    pub fn health(&self) -> &Arc<FeedHealth> {
        &self.health
    }
}

/// Client ping period: `min(ping_ms, stale_ms / 3)`, at least 500 ms.
fn ping_period(cfg: &LogsWsConfig) -> u64 {
    let p = if cfg.stale_ms > 0 {
        cfg.ping_ms.min(cfg.stale_ms / 3)
    } else {
        cfg.ping_ms
    };
    p.max(500)
}

fn sub_request(id: u64, mention: &Pubkey, commitment: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"logsSubscribe","params":[{{"mentions":["{mention}"]}},{{"commitment":"{commitment}"}}]}}"#
    )
}

fn unsub_request(id: u64, server_id: u64) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"logsUnsubscribe","params":[{server_id}]}}"#)
}

/// Jittered exponential backoff (xorshift on the clock; no rand dependency).
fn jitter(ms: u64) -> u64 {
    let mut x = mono_ns() | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let f = 800 + (x % 401); // 0.8 .. 1.2
    ms * f / 1000
}

struct FundingJob {
    sig: String,
    funder: Pubkey,
    due_ns: u64,
    tries: u8,
}

async fn sleep_or_stop(stop: &AtomicBool, ms: u64) {
    let mut left = ms;
    while left > 0 && !stop.load(Ordering::Acquire) {
        let d = left.min(200);
        tokio::time::sleep(Duration::from_millis(d)).await;
        left -= d;
    }
}

/// Cold-path worker: rate-limited `getTransaction` for funder notifications.
async fn funding_worker(
    url: String,
    rps: f64,
    mut jobs: mpsc::Receiver<FundingJob>,
    retry: mpsc::Sender<FundingJob>,
    results: mpsc::UnboundedSender<Vec<FundingEvent>>,
    health: Arc<FeedHealth>,
) {
    let Ok(mut client) = RpcClient::new(&url) else {
        return;
    };
    let gap_ns = (1e9 / rps.max(0.1)) as u64;
    let mut next_ok = 0u64;
    let mut id = 1u64;
    let mut found = Vec::new();
    while let Some(job) = jobs.recv().await {
        let now = mono_ns();
        let wait = job.due_ns.max(next_ok).saturating_sub(now);
        if wait > 0 {
            tokio::time::sleep(Duration::from_nanos(wait)).await;
        }
        next_ok = mono_ns() + gap_ns;
        FeedHealth::inc(&health.rpc_calls);
        id += 1;
        let resp = client.call(&get_transaction_body(id, &job.sig)).await;
        found.clear();
        let parsed = resp
            .map_err(|e| e.to_string())
            .and_then(|text| funding_from_get_transaction(&text, &job.funder, &mut found));
        match parsed {
            Ok(_) => {
                if !found.is_empty() {
                    let _ = results.send(found.clone());
                }
            }
            Err(e) => {
                if e.contains("not found") && job.tries < 3 {
                    let again = FundingJob {
                        due_ns: mono_ns() + 1_000_000_000,
                        tries: job.tries + 1,
                        ..job
                    };
                    if retry.try_send(again).is_err() {
                        FeedHealth::inc(&health.rpc_drops);
                    }
                } else {
                    FeedHealth::inc(&health.rpc_errors);
                }
            }
        }
    }
}

async fn run(
    cfg: LogsWsConfig,
    mut sink: RingSink,
    stop: Arc<AtomicBool>,
    mut ctl_rx: mpsc::UnboundedReceiver<Control>,
    recorder: Option<Recorder>,
) {
    let health = sink.health().clone();
    let mut funders: Vec<Pubkey> = cfg.watch_funders.clone();
    let mut mints: Vec<Pubkey> = cfg.watch_mints.clone();
    funders.truncate(cfg.max_watches);
    mints.truncate(cfg.max_watches.saturating_sub(funders.len()));

    let (fund_res_tx, mut fund_res_rx) = mpsc::unbounded_channel::<Vec<FundingEvent>>();
    let (job_tx, job_rx) = mpsc::channel::<FundingJob>(cfg.rpc_queue.max(1));
    if let Some(url) = cfg.rpc_url.clone() {
        tokio::spawn(funding_worker(
            url,
            cfg.rpc_rps,
            job_rx,
            job_tx.clone(),
            fund_res_tx,
            health.clone(),
        ));
    } else {
        drop(job_rx);
    }

    let mut dec = FrameDecoder::new();
    let mut backoff = cfg.min_backoff_ms;
    let mut next_id: u64 = 1;
    while !stop.load(Ordering::Acquire) {
        let t_conn = mono_ns();
        let mut ws = match ws_connect(&cfg.url, Duration::from_millis(cfg.connect_timeout_ms)).await
        {
            Ok(ws) => ws,
            Err(_) => {
                FeedHealth::inc(&health.reconnects);
                sleep_or_stop(&stop, jitter(backoff)).await;
                backoff = (backoff * 2).min(cfg.max_backoff_ms);
                continue;
            }
        };
        health.connected.store(true, Ordering::Relaxed);
        // a fresh connection is not stale until stale_ms passes without frames
        health.last_frame_ns.store(mono_ns(), Ordering::Relaxed);
        let mut pending: HashMap<u64, SubKind> = HashMap::new();
        let mut subs: HashMap<u64, SubKind> = HashMap::new();
        let mut want: Vec<SubKind> = Vec::new();
        if cfg.creates {
            want.push(SubKind::Creates);
        }
        want.extend(funders.iter().map(|k| SubKind::Funder(*k)));
        want.extend(mints.iter().map(|k| SubKind::Mint(*k)));
        let mut ok = true;
        for kind in want {
            let key = match kind {
                SubKind::Creates => cfg.mention,
                SubKind::Funder(k) | SubKind::Mint(k) => k,
            };
            let id = next_id;
            next_id += 1;
            pending.insert(id, kind);
            if ws
                .send(Message::Text(sub_request(id, &key, &cfg.commitment).into()))
                .await
                .is_err()
            {
                ok = false;
                break;
            }
        }
        // Ping at least 3x per stale window: a quiet but healthy socket answers with
        // pongs (which refresh `last_frame_ns`), so only a dead one trips the detector.
        // Measured: at ~23 creates/min, 11-12 s gaps without notifications are normal.
        let mut ping = tokio::time::interval(Duration::from_millis(ping_period(&cfg)));
        ping.tick().await;
        let mut check = tokio::time::interval(Duration::from_millis(250));
        while ok && !stop.load(Ordering::Acquire) {
            tokio::select! {
                msg = ws.next() => {
                    let text = match msg {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Binary(b))) => match String::from_utf8(b.to_vec()) {
                            Ok(s) => s.into(),
                            Err(_) => continue,
                        },
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(_)) => { health.last_frame_ns.store(mono_ns(), Ordering::Relaxed); continue; }
                    };
                    let recv = mono_ns();
                    health.frame();
                    if let Some(r) = &recorder {
                        r.record(FeedSrc::LogsWs, recv, unix_ns(), text.as_str());
                    }
                    match parse_ws_message(text.as_str()) {
                        Ok(WsMsg::Logs(n)) => {
                            let kind = subs.get(&n.subscription).copied().unwrap_or(SubKind::Creates);
                            match kind {
                                SubKind::Creates => {
                                    dec.logs_note(FeedSrc::LogsWs, recv, &n, cfg.emit_create_trades, &health, &mut |ev| { sink.push(ev); });
                                }
                                SubKind::Mint(m) => {
                                    // the watched mint's trades (and its create, if this is it)
                                    dec.logs_note(FeedSrc::LogsWs, recv, &n, true, &health, &mut |ev| {
                                        if !matches!(ev.payload, Payload::Trade(t) if t.mint != m) {
                                            sink.push(ev);
                                        }
                                    });
                                }
                                SubKind::Funder(f) => {
                                    if n.failed { FeedHealth::inc(&health.failed_tx); continue; }
                                    if job_tx.try_send(FundingJob {
                                        sig: n.signature.to_string(),
                                        funder: f,
                                        due_ns: recv + cfg.confirm_delay_ms * 1_000_000,
                                        tries: 0,
                                    }).is_err() {
                                        FeedHealth::inc(&health.rpc_drops);
                                    }
                                }
                            }
                        }
                        Ok(WsMsg::SubAck { id, subscription }) => {
                            if let Some(k) = pending.remove(&id) {
                                subs.insert(subscription, k);
                            }
                        }
                        Ok(WsMsg::Error { .. }) => FeedHealth::inc(&health.decode_errors),
                        Ok(WsMsg::Other) => {}
                        Err(_) => FeedHealth::inc(&health.decode_errors),
                    }
                }
                _ = ping.tick() => {
                    if ws.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
                }
                _ = check.tick() => {
                    if health.is_stale_at(mono_ns()) {
                        FeedHealth::inc(&health.stale_reconnects);
                        break;
                    }
                }
                Some(c) = ctl_rx.recv() => {
                    match apply_control(c, &mut funders, &mut mints, cfg.max_watches, &mut subs) {
                        Apply::Subscribe(kind, key) => {
                            let id = next_id;
                            next_id += 1;
                            pending.insert(id, kind);
                            if ws.send(Message::Text(sub_request(id, &key, &cfg.commitment).into())).await.is_err() { break; }
                        }
                        Apply::Unsubscribe(server) => {
                            let id = next_id;
                            next_id += 1;
                            if ws.send(Message::Text(unsub_request(id, server).into())).await.is_err() { break; }
                        }
                        Apply::Nothing => {}
                    }
                }
                Some(batch) = fund_res_rx.recv() => {
                    for f in batch {
                        health.event(f.slot);
                        sink.push(FeedEvent { src: FeedSrc::LogsWs, recv_ns: mono_ns(), slot: f.slot, bank_id: 0, payload: Payload::Funding(f) });
                    }
                }
            }
        }
        health.connected.store(false, Ordering::Relaxed);
        let _ = ws.close(None).await;
        if stop.load(Ordering::Acquire) {
            break;
        }
        FeedHealth::inc(&health.reconnects);
        if mono_ns().saturating_sub(t_conn) > 120_000_000_000 {
            backoff = cfg.min_backoff_ms;
        }
        sleep_or_stop(&stop, jitter(backoff)).await;
        backoff = (backoff * 2).min(cfg.max_backoff_ms);
    }
}

enum Apply {
    Subscribe(SubKind, Pubkey),
    Unsubscribe(u64),
    Nothing,
}

/// Update the watch lists for a control message; what to send, if anything.
fn apply_control(
    c: Control,
    funders: &mut Vec<Pubkey>,
    mints: &mut Vec<Pubkey>,
    max_watches: usize,
    subs: &mut HashMap<u64, SubKind>,
) -> Apply {
    let total = funders.len() + mints.len();
    let (kind, key, add) = match c {
        Control::WatchFunder(k) => (SubKind::Funder(k), k, true),
        Control::UnwatchFunder(k) => (SubKind::Funder(k), k, false),
        Control::WatchMint(k) => (SubKind::Mint(k), k, true),
        Control::UnwatchMint(k) => (SubKind::Mint(k), k, false),
    };
    let list = if matches!(kind, SubKind::Funder(_)) {
        funders
    } else {
        mints
    };
    if add {
        if list.contains(&key) || total >= max_watches {
            return Apply::Nothing;
        }
        list.push(key);
        Apply::Subscribe(kind, key)
    } else {
        list.retain(|x| *x != key);
        match subs.iter().find(|(_, k)| **k == kind).map(|(s, _)| *s) {
            Some(server) => {
                subs.remove(&server);
                Apply::Unsubscribe(server)
            }
            None => Apply::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests() {
        let r = sub_request(1, &MINT_AUTHORITY, "processed");
        assert_eq!(
            r,
            r#"{"jsonrpc":"2.0","id":1,"method":"logsSubscribe","params":[{"mentions":["TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"]},{"commitment":"processed"}]}"#
        );
        assert!(unsub_request(2, 99).contains("logsUnsubscribe"));
        let cfg = LogsWsConfig::default();
        assert_eq!(ping_period(&cfg), 3_333);
        assert_eq!(
            ping_period(&LogsWsConfig {
                stale_ms: 0,
                ..cfg.clone()
            }),
            20_000
        );
        for _ in 0..100 {
            let j = jitter(1000);
            assert!((800..=1200).contains(&j));
        }
    }

    #[test]
    fn watch_caps_and_unsubscribe() {
        let (mut f, mut m, mut subs) = (vec![], vec![], HashMap::new());
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        assert!(matches!(
            apply_control(Control::WatchFunder(a), &mut f, &mut m, 1, &mut subs),
            Apply::Subscribe(SubKind::Funder(_), _)
        ));
        assert!(matches!(
            apply_control(Control::WatchFunder(a), &mut f, &mut m, 1, &mut subs),
            Apply::Nothing
        ));
        assert!(
            matches!(
                apply_control(Control::WatchMint(b), &mut f, &mut m, 1, &mut subs),
                Apply::Nothing
            ),
            "cap of 1 reached"
        );
        subs.insert(77, SubKind::Funder(a));
        assert!(matches!(
            apply_control(Control::UnwatchFunder(a), &mut f, &mut m, 1, &mut subs),
            Apply::Unsubscribe(77)
        ));
        assert!(f.is_empty() && subs.is_empty());
    }
}
