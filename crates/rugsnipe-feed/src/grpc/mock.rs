//! A local Yellowstone gRPC server for tests (feature `grpc-mock`): it
//! speaks the real `geyser.Geyser/Subscribe` protocol (tonic, plain h2c on
//! 127.0.0.1), checks the `x-token`, records every `SubscribeRequest` the
//! client sends (filters, pings, resubscribes) and streams the updates a
//! test pushes ([`MockGeyser::push`]): recorded mainnet transactions
//! ([`fixture_update`] turns a pump-codec `getTransaction` fixture into a
//! `SubscribeUpdateTransaction`), slots, pings. [`MockGeyser::drop_stream`]
//! ends the current stream; a silent server lets the client's stale
//! detector fire. No network beyond loopback.

use super::convert::wire_to_tx;
use base64::Engine as _;
use futures_util::StreamExt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use yellowstone_grpc_proto::prelude::geyser_server::{Geyser, GeyserServer};
use yellowstone_grpc_proto::prelude::*;
use yellowstone_grpc_proto::tonic::{self, Request, Response, Status, Streaming};

type UpdateStream =
    Pin<Box<dyn futures_util::Stream<Item = Result<SubscribeUpdate, Status>> + Send + 'static>>;

#[derive(Default)]
struct Inner {
    requests: Vec<SubscribeRequest>,
    sender: Option<mpsc::Sender<Result<SubscribeUpdate, Status>>>,
    deshred_requests: Vec<SubscribeDeshredRequest>,
    dsender: Option<mpsc::Sender<Result<SubscribeUpdateDeshred, Status>>>,
    connections: u64,
    refused: u64,
}

/// The mock server state shared with the test.
#[derive(Clone)]
pub struct MockGeyser {
    token: Option<String>,
    inner: Arc<Mutex<Inner>>,
}

impl std::fmt::Debug for MockGeyser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockGeyser").finish_non_exhaustive()
    }
}

impl MockGeyser {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every request received so far (the initial subscription, pings,
    /// resubscribes), across connections.
    pub fn requests(&self) -> Vec<SubscribeRequest> {
        self.lock().requests.clone()
    }

    /// Subscribe calls accepted / refused (bad token).
    pub fn connections(&self) -> u64 {
        self.lock().connections
    }

    pub fn refused(&self) -> u64 {
        self.lock().refused
    }

    /// A client stream is open.
    pub fn connected(&self) -> bool {
        self.lock().sender.as_ref().is_some_and(|s| !s.is_closed())
    }

    /// Send an update to the open stream; `false` when none is open.
    pub fn push(&self, u: SubscribeUpdate) -> bool {
        let tx = self.lock().sender.clone();
        match tx {
            Some(tx) => tx.try_send(Ok(u)).is_ok(),
            None => false,
        }
    }

    /// Send a deshred update to the connected `SubscribeDeshred` client.
    pub fn push_deshred(&self, u: SubscribeUpdateDeshred) -> bool {
        let tx = self.lock().dsender.clone();
        match tx {
            Some(tx) => tx.try_send(Ok(u)).is_ok(),
            None => false,
        }
    }

    /// Every `SubscribeDeshred` request received (the first is the subscription).
    pub fn deshred_requests(&self) -> Vec<SubscribeDeshredRequest> {
        self.lock().deshred_requests.clone()
    }

    pub fn deshred_connected(&self) -> bool {
        self.lock().dsender.is_some()
    }

    /// End the current stream (the client sees it close and reconnects).
    pub fn drop_stream(&self) {
        self.lock().sender = None;
    }
}

#[tonic::async_trait]
impl Geyser for MockGeyser {
    type SubscribeStream = UpdateStream;
    type SubscribeDeshredStream =
        Pin<Box<dyn futures_util::Stream<Item = Result<SubscribeUpdateDeshred, Status>> + Send>>;
    type SubscribeGossipStream =
        Pin<Box<dyn futures_util::Stream<Item = Result<SubscribeUpdateGossip, Status>> + Send>>;

    async fn subscribe(
        &self,
        request: Request<Streaming<SubscribeRequest>>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let tok = request
            .metadata()
            .get("x-token")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if self.token.is_some() && tok != self.token {
            self.lock().refused += 1;
            return Err(Status::unauthenticated("bad x-token"));
        }
        let (tx, rx) = mpsc::channel(4_096);
        {
            let mut g = self.lock();
            g.connections += 1;
            g.sender = Some(tx);
        }
        let me = self.clone();
        let mut reqs = request.into_inner();
        tokio::spawn(async move {
            while let Some(Ok(r)) = reqs.next().await {
                let ping = r.ping;
                me.lock().requests.push(r);
                if let Some(p) = ping {
                    let pong = SubscribeUpdate {
                        update_oneof: Some(subscribe_update::UpdateOneof::Pong(
                            SubscribeUpdatePong { id: p.id },
                        )),
                        ..Default::default()
                    };
                    me.push(pong);
                }
            }
        });
        let s =
            futures_util::stream::unfold(
                rx,
                |mut rx| async move { rx.recv().await.map(|m| (m, rx)) },
            );
        Ok(Response::new(Box::pin(s)))
    }

    async fn subscribe_deshred(
        &self,
        request: Request<Streaming<SubscribeDeshredRequest>>,
    ) -> Result<Response<Self::SubscribeDeshredStream>, Status> {
        let tok = request
            .metadata()
            .get("x-token")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if self.token.is_some() && tok != self.token {
            self.lock().refused += 1;
            return Err(Status::unauthenticated("bad x-token"));
        }
        let (tx, rx) = mpsc::channel(4_096);
        {
            let mut g = self.lock();
            g.connections += 1;
            g.dsender = Some(tx);
        }
        let me = self.clone();
        let mut reqs = request.into_inner();
        tokio::spawn(async move {
            while let Some(Ok(r)) = reqs.next().await {
                let ping = r.ping;
                me.lock().deshred_requests.push(r);
                if let Some(p) = ping {
                    let pong = SubscribeUpdateDeshred {
                        update_oneof: Some(subscribe_update_deshred::UpdateOneof::Pong(
                            SubscribeUpdatePong { id: p.id },
                        )),
                        ..Default::default()
                    };
                    me.push_deshred(pong);
                }
            }
        });
        let s =
            futures_util::stream::unfold(
                rx,
                |mut rx| async move { rx.recv().await.map(|m| (m, rx)) },
            );
        Ok(Response::new(Box::pin(s)))
    }

    async fn subscribe_gossip(
        &self,
        _r: Request<SubscribeGossipRequest>,
    ) -> Result<Response<Self::SubscribeGossipStream>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn subscribe_replay_info(
        &self,
        _r: Request<SubscribeReplayInfoRequest>,
    ) -> Result<Response<SubscribeReplayInfoResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn ping(&self, r: Request<PingRequest>) -> Result<Response<PongResponse>, Status> {
        Ok(Response::new(PongResponse {
            count: r.into_inner().count,
        }))
    }

    async fn get_latest_blockhash(
        &self,
        _r: Request<GetLatestBlockhashRequest>,
    ) -> Result<Response<GetLatestBlockhashResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn get_block_height(
        &self,
        _r: Request<GetBlockHeightRequest>,
    ) -> Result<Response<GetBlockHeightResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn get_slot(
        &self,
        _r: Request<GetSlotRequest>,
    ) -> Result<Response<GetSlotResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn is_blockhash_valid(
        &self,
        _r: Request<IsBlockhashValidRequest>,
    ) -> Result<Response<IsBlockhashValidResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn get_version(
        &self,
        _r: Request<GetVersionRequest>,
    ) -> Result<Response<GetVersionResponse>, Status> {
        Ok(Response::new(GetVersionResponse {
            version: "mock".into(),
        }))
    }
}

/// A running mock server (stopped on drop).
pub struct MockServer {
    pub addr: SocketAddr,
    pub geyser: MockGeyser,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for MockServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockServer")
            .field("addr", &self.addr)
            .finish()
    }
}

impl MockServer {
    /// Serve on 127.0.0.1 (a free port) on its own thread; `token`: the
    /// `x-token` the server requires (`None`: any).
    pub fn start(token: Option<&str>) -> MockServer {
        let geyser = MockGeyser {
            token: token.map(str::to_string),
            inner: Arc::new(Mutex::new(Inner::default())),
        };
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let g = geyser.clone();
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("mock runtime");
            rt.block_on(async move {
                let inc =
                    tonic::transport::server::TcpIncoming::bind("127.0.0.1:0".parse().unwrap())
                        .expect("bind");
                addr_tx.send(inc.local_addr().expect("addr")).unwrap();
                let _ = tonic::transport::Server::builder()
                    .add_service(GeyserServer::new(g))
                    .serve_with_incoming_shutdown(inc, async {
                        let _ = stop_rx.await;
                    })
                    .await;
            });
        });
        let addr = addr_rx.recv().expect("mock server address");
        MockServer {
            addr,
            geyser,
            stop: Some(stop_tx),
            thread: Some(thread),
        }
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        self.geyser.drop_stream();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn b58_32(s: &str) -> Vec<u8> {
    bs58::decode(s).into_vec().unwrap_or_default()
}

/// A `SubscribeUpdate` (transaction) built from a pump-codec fixture
/// (`getTransaction`, base64 encoding): the wire bytes become the protobuf
/// transaction, the RPC meta its status meta (fee, error, logs, inner
/// instructions, loaded addresses). `filters` are the filter names the
/// server says matched.
pub fn fixture_update(
    fixture: &serde_json::Value,
    slot: u64,
    index: u64,
    filters: &[&str],
) -> Result<SubscribeUpdate, String> {
    let r = &fixture["result"];
    let b64 = r["transaction"][0]
        .as_str()
        .ok_or("no base64 transaction")?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    let tx = wire_to_tx(&raw)?;
    let m = &r["meta"];
    let inner = m["innerInstructions"]
        .as_array()
        .map(|groups| {
            groups
                .iter()
                .map(|g| InnerInstructions {
                    index: g["index"].as_u64().unwrap_or(0) as u32,
                    instructions: g["instructions"]
                        .as_array()
                        .map(|ixs| {
                            ixs.iter()
                                .map(|ix| InnerInstruction {
                                    program_id_index: ix["programIdIndex"].as_u64().unwrap_or(0)
                                        as u32,
                                    accounts: ix["accounts"]
                                        .as_array()
                                        .map(|a| {
                                            a.iter()
                                                .map(|x| x.as_u64().unwrap_or(0) as u8)
                                                .collect()
                                        })
                                        .unwrap_or_default(),
                                    data: bs58::decode(ix["data"].as_str().unwrap_or(""))
                                        .into_vec()
                                        .unwrap_or_default(),
                                    stack_height: ix["stackHeight"].as_u64().map(|h| h as u32),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let keys = |k: &str| -> Vec<Vec<u8>> {
        m["loadedAddresses"][k]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).map(b58_32).collect())
            .unwrap_or_default()
    };
    let logs: Vec<String> = m["logMessages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let err = (!m["err"].is_null()).then(|| TransactionError {
        err: m["err"].to_string().into_bytes(),
    });
    let meta = TransactionStatusMeta {
        err,
        fee: m["fee"].as_u64().unwrap_or(0),
        inner_instructions: inner,
        inner_instructions_none: m["innerInstructions"].is_null(),
        log_messages: logs,
        log_messages_none: m["logMessages"].is_null(),
        loaded_writable_addresses: keys("writable"),
        loaded_readonly_addresses: keys("readonly"),
        ..Default::default()
    };
    let signature = tx.signatures.first().cloned().unwrap_or_default();
    Ok(SubscribeUpdate {
        filters: filters.iter().map(|f| f.to_string()).collect(),
        update_oneof: Some(subscribe_update::UpdateOneof::Transaction(
            SubscribeUpdateTransaction {
                transaction: Some(SubscribeUpdateTransactionInfo {
                    signature,
                    is_vote: false,
                    transaction: Some(tx),
                    meta: Some(meta),
                    index,
                }),
                slot,
                bank_id: 0,
            },
        )),
        created_at: None,
    })
}

/// A slot update.
pub fn slot_update(slot: u64) -> SubscribeUpdate {
    SubscribeUpdate {
        filters: vec![super::F_SLOTS.to_string()],
        update_oneof: Some(subscribe_update::UpdateOneof::Slot(SubscribeUpdateSlot {
            slot,
            parent: slot.checked_sub(1),
            status: SlotStatus::SlotProcessed as i32,
            dead_error: None,
            bank_id: None,
        })),
        created_at: None,
    }
}

/// A deshred (pre-execution) copy of a recorded transaction: the raw
/// transaction and the loaded addresses from its meta, no execution meta.
pub fn fixture_deshred_update(
    fixture: &serde_json::Value,
    slot: u64,
) -> Result<SubscribeUpdateDeshred, String> {
    let r = &fixture["result"];
    let b64 = r["transaction"][0]
        .as_str()
        .ok_or("no base64 transaction")?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    let tx = wire_to_tx(&raw)?;
    let sig = tx.signatures.first().cloned().unwrap_or_default();
    let m = &r["meta"];
    let addrs = |k: &str| -> Vec<Vec<u8>> {
        m["loadedAddresses"][k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| bs58::decode(s).into_vec().ok())
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(SubscribeUpdateDeshred {
        filters: vec![super::F_DESHRED.to_string()],
        update_oneof: Some(subscribe_update_deshred::UpdateOneof::DeshredTransaction(
            SubscribeUpdateDeshredTransaction {
                slot,
                transaction: Some(SubscribeUpdateDeshredTransactionInfo {
                    signature: sig,
                    is_vote: false,
                    transaction: Some(tx),
                    loaded_writable_addresses: addrs("writable"),
                    loaded_readonly_addresses: addrs("readonly"),
                    completed_data_set_starting_shred_index: 0,
                    completed_data_set_ending_shred_index_exclusive: 0,
                }),
            },
        )),
        created_at: None,
    })
}

/// A deshred slot update.
pub fn deshred_slot_update(slot: u64) -> SubscribeUpdateDeshred {
    SubscribeUpdateDeshred {
        filters: vec![super::F_SLOTS.to_string()],
        update_oneof: Some(subscribe_update_deshred::UpdateOneof::Slot(
            SubscribeUpdateSlot {
                slot,
                parent: slot.checked_sub(1),
                status: SlotStatus::SlotProcessed as i32,
                dead_error: None,
                bank_id: None,
            },
        )),
        created_at: None,
    }
}

/// A server ping.
pub fn ping_update() -> SubscribeUpdate {
    SubscribeUpdate {
        filters: vec![],
        update_oneof: Some(subscribe_update::UpdateOneof::Ping(SubscribeUpdatePing {})),
        created_at: None,
    }
}
