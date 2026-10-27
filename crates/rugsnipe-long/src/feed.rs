//! The Yellowstone stream (processed commitment) on a thread of its own:
//! the copied wallet's landed transactions, every transaction of ours
//! (failed ones too: a failed buy is a cost), every landed transaction on a
//! held mint (the exits), and slots. The held set changes at runtime
//! ([`FeedHandle::watch`]); the subscription is re-sent on the same stream.

use crate::txlite::TxLite;
use futures_util::{SinkExt, StreamExt};
use pump_codec::Pubkey;
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient};
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions, SubscribeRequestPing,
};

/// Which filter a transaction came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Src {
    Copied,
    Ours,
    Mint,
}

#[derive(Debug)]
pub enum FeedMsg {
    Tx(Src, Box<TxLite>),
    Slot(u64),
    /// Connected (true) or the session ended (false).
    Up(bool),
}

#[derive(Debug, Clone)]
pub struct FeedCfg {
    pub url: String,
    pub x_token: Option<String>,
    pub copied: Pubkey,
    /// Our wallet (`None` in paper without one: nothing of ours to see).
    pub ours: Option<Pubkey>,
}

/// Changes the held set.
#[derive(Debug, Clone)]
pub struct FeedHandle {
    tx: tokio::sync::mpsc::UnboundedSender<BTreeSet<Pubkey>>,
    pub stop: Arc<AtomicBool>,
}

impl FeedHandle {
    /// Watch exactly `mints` from now on.
    pub fn watch(&self, mints: BTreeSet<Pubkey>) {
        let _ = self.tx.send(mints);
    }
}

fn request(cfg: &FeedCfg, mints: &BTreeSet<Pubkey>) -> SubscribeRequest {
    let f = |include: Vec<String>, failed: bool| SubscribeRequestFilterTransactions {
        vote: Some(false),
        failed: Some(failed),
        account_include: include,
        ..Default::default()
    };
    let mut transactions = HashMap::new();
    transactions.insert("copied".to_string(), f(vec![cfg.copied.to_string()], false));
    if let Some(o) = cfg.ours {
        transactions.insert("ours".to_string(), f(vec![o.to_string()], true));
    }
    if !mints.is_empty() {
        transactions.insert(
            "mints".to_string(),
            f(mints.iter().map(|m| m.to_string()).collect(), false),
        );
    }
    let mut slots = HashMap::new();
    slots.insert("slots".to_string(), SubscribeRequestFilterSlots::default());
    SubscribeRequest {
        transactions,
        slots,
        commitment: Some(CommitmentLevel::Processed as i32),
        ..Default::default()
    }
}

/// Start the feed thread.
pub fn spawn(cfg: FeedCfg, out: mpsc::Sender<FeedMsg>) -> FeedHandle {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BTreeSet<Pubkey>>();
    let stop = Arc::new(AtomicBool::new(false));
    let st = stop.clone();
    std::thread::Builder::new()
        .name("long-feed".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async move {
                let mut mints = BTreeSet::new();
                let mut backoff = 500u64;
                while !st.load(Ordering::Acquire) {
                    while let Ok(m) = rx.try_recv() {
                        mints = m;
                    }
                    let t0 = std::time::Instant::now();
                    let r = session(&cfg, &out, &mut rx, &mut mints, &st).await;
                    let _ = out.send(FeedMsg::Up(false));
                    if st.load(Ordering::Acquire) {
                        break;
                    }
                    if t0.elapsed() > Duration::from_secs(60) {
                        backoff = 500;
                    }
                    eprintln!(
                        "long feed: session ended: {} (reconnect in {backoff} ms)",
                        r.err().unwrap_or_else(|| "closed".into())
                    );
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    backoff = (backoff * 2).min(15_000);
                }
            });
        })
        .expect("spawn the feed thread");
    FeedHandle { tx, stop }
}

async fn session(
    cfg: &FeedCfg,
    out: &mpsc::Sender<FeedMsg>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<BTreeSet<Pubkey>>,
    mints: &mut BTreeSet<Pubkey>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut b = GeyserGrpcClient::build_from_shared(cfg.url.clone())
        .map_err(|e| e.to_string())?
        .x_token(cfg.x_token.clone())
        .map_err(|e| e.to_string())?
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(24 * 3600))
        .tcp_nodelay(true)
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(5))
        .keep_alive_while_idle(true)
        .max_decoding_message_size(64 << 20);
    if cfg.url.starts_with("https://") {
        b = b
            .tls_config(ClientTlsConfig::new().with_native_roots())
            .map_err(|e| e.to_string())?;
    }
    let mut client = tokio::time::timeout(Duration::from_secs(10), b.connect())
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = tokio::time::timeout(
        Duration::from_secs(10),
        client.subscribe_with_request(Some(request(cfg, mints))),
    )
    .await
    .map_err(|_| "subscribe timeout".to_string())?
    .map_err(|e| e.to_string())?;
    let _ = out.send(FeedMsg::Up(true));
    let mut ping = tokio::time::interval(Duration::from_secs(10));
    let mut last = std::time::Instant::now();
    let mut id = 1i32;
    while !stop.load(Ordering::Acquire) {
        tokio::select! {
            m = stream.next() => match m {
                Some(Ok(u)) => {
                    last = std::time::Instant::now();
                    match u.update_oneof {
                        Some(UpdateOneof::Transaction(t)) => {
                            let src = if u.filters.iter().any(|f| f == "ours") {
                                Src::Ours
                            } else if u.filters.iter().any(|f| f == "copied") {
                                Src::Copied
                            } else {
                                Src::Mint
                            };
                            if let Some(x) = TxLite::from_grpc(&t) {
                                if out.send(FeedMsg::Tx(src, Box::new(x))).is_err() {
                                    return Ok(());
                                }
                            }
                        }
                        Some(UpdateOneof::Slot(s)) => {
                            let _ = out.send(FeedMsg::Slot(s.slot));
                        }
                        Some(UpdateOneof::Ping(_)) => {
                            id = id.wrapping_add(1);
                            sink.send(SubscribeRequest { ping: Some(SubscribeRequestPing { id }), ..Default::default() })
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        _ => {}
                    }
                }
                Some(Err(e)) => return Err(e.to_string()),
                None => return Err("stream ended".into()),
            },
            _ = ping.tick() => {
                if last.elapsed() > Duration::from_secs(30) {
                    return Err("stale: nothing for 30 s".into());
                }
                id = id.wrapping_add(1);
                sink.send(SubscribeRequest { ping: Some(SubscribeRequestPing { id }), ..Default::default() })
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Some(m) = rx.recv() => {
                let mut m = m;
                while let Ok(n) = rx.try_recv() {
                    m = n;
                }
                if m != *mints {
                    *mints = m;
                    sink.send(request(cfg, mints)).await.map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}
