//! The leader schedule and the leader's client class per slot (docs/03
//! sections 6 and 6.1, docs/06 section 11 "leader table").
//!
//! The cold lane reads `getEpochInfo` + `getLeaderSchedule` once per epoch
//! and `getClusterNodes` every few minutes (the client id flips
//! `AgaveBam` -> `JitoLabs` when a BAM node falls back, and the advertised
//! `tpuQuic` moves with it), builds a [`LeaderMap`] and publishes it through
//! an `ArcSwap` ([`LeaderSnapshot`]). The hot loop reads the class of the
//! create slot and the next slot lock-free and allocation-free
//! ([`LeaderMap::class_at`]) to pick the bid variant of each (`BidPolicy`
//! has the per-class split), and every send and fill records the class.
//!
//! Classification (docs/03 section 6; `deploy/stake_geo_v2.py` `CLASS`;
//! `getClusterNodes.clientId` equals DoubleZero's `software_client` for
//! 99.84% of stake, research/speed-gap-1):
//!
//! | client id | class | bid class (`rugsnipe_core::bid::LeaderClass`) |
//! |---|---|---|
//! | `AgaveBam`, `FireBAM` | BAM | `bam` |
//! | `HarmonicAgave`, `HarmonicFiredancer` | Harmonic | `harmonic` |
//! | `JitoLabs` | Jito-Agave | `jito_agave` |
//! | `Firedancer`, `Frankendancer` | Firedancer | `other` |
//! | `Agave`, `Rakurai`, `Raiku`, unknown | other | `other` |

use arc_swap::ArcSwap;
use pump_codec::Pubkey;
use rugsnipe_core::bid::LeaderClass;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

/// Who runs the leader's block, as recorded per send and fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ClientClass {
    JitoAgave = 0,
    Bam = 1,
    Harmonic = 2,
    Firedancer = 3,
    Other = 4,
}

/// Code of an unknown leader in `Copy` messages.
pub const UNKNOWN: u8 = 0xff;

impl ClientClass {
    pub const ALL: [ClientClass; 5] = [
        ClientClass::JitoAgave,
        ClientClass::Bam,
        ClientClass::Harmonic,
        ClientClass::Firedancer,
        ClientClass::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::JitoAgave => "jito_agave",
            Self::Bam => "bam",
            Self::Harmonic => "harmonic",
            Self::Firedancer => "firedancer",
            Self::Other => "other",
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }

    /// Name of a message code (`None` for [`UNKNOWN`]).
    pub fn name_of(code: u8) -> Option<&'static str> {
        Self::from_u8(code).map(Self::as_str)
    }

    /// From a `getClusterNodes` `clientId`.
    pub fn from_client_id(id: &str) -> Self {
        let l = id.to_ascii_lowercase();
        if l.contains("bam") {
            Self::Bam
        } else if l.contains("harmonic") {
            Self::Harmonic
        } else if l.contains("jito") {
            Self::JitoAgave
        } else if l == "firedancer" || l == "frankendancer" {
            Self::Firedancer
        } else {
            Self::Other
        }
    }

    /// The bid class (`[bid] split`): Firedancer orders by priority fee.
    pub fn bid_class(self) -> LeaderClass {
        match self {
            Self::JitoAgave => LeaderClass::JitoAgave,
            Self::Bam => LeaderClass::Bam,
            Self::Harmonic => LeaderClass::Harmonic,
            Self::Firedancer | Self::Other => LeaderClass::Other,
        }
    }
}

/// One validator from `getClusterNodes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub pubkey: Pubkey,
    pub client_id: String,
    pub class: ClientClass,
    pub tpu_quic: Option<SocketAddr>,
    pub gossip: Option<SocketAddr>,
    pub version: Option<String>,
}

/// `getClusterNodes` result -> nodes by identity.
pub fn parse_cluster_nodes(result: &Value) -> HashMap<Pubkey, NodeInfo> {
    let mut out = HashMap::new();
    for n in result.as_array().into_iter().flatten() {
        let Some(pk) = n["pubkey"].as_str().and_then(|s| s.parse::<Pubkey>().ok()) else {
            continue;
        };
        let client_id = n["clientId"].as_str().unwrap_or("").to_string();
        out.insert(
            pk,
            NodeInfo {
                pubkey: pk,
                class: ClientClass::from_client_id(&client_id),
                client_id,
                tpu_quic: n["tpuQuic"].as_str().and_then(|s| s.parse().ok()),
                gossip: n["gossip"].as_str().and_then(|s| s.parse().ok()),
                version: n["version"].as_str().map(str::to_string),
            },
        );
    }
    out
}

/// The schedule of one epoch with each leader's class.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeaderMap {
    pub epoch: u64,
    /// Absolute slot of the epoch's first slot.
    pub first_slot: u64,
    /// Leader index per slot of the epoch (`u16::MAX` = unknown).
    slots: Vec<u16>,
    pub leaders: Vec<Pubkey>,
    /// Class per leader (same order as `leaders`).
    classes: Vec<ClientClass>,
    /// Advertised `tpuQuic` per leader.
    tpu_quic: Vec<Option<SocketAddr>>,
    /// When the classes were last refreshed from gossip (Unix seconds).
    pub nodes_unix: u64,
    /// Leaders not in gossip (class `other`).
    pub missing_nodes: u32,
}

impl LeaderMap {
    /// From `getEpochInfo`, `getLeaderSchedule` and `getClusterNodes` results.
    pub fn from_rpc(
        epoch_info: &Value,
        schedule: &Value,
        nodes: &Value,
        now_unix: u64,
    ) -> Result<LeaderMap, String> {
        let epoch = epoch_info["epoch"].as_u64().ok_or("epochInfo: no epoch")?;
        let abs = epoch_info["absoluteSlot"]
            .as_u64()
            .ok_or("epochInfo: no absoluteSlot")?;
        let idx = epoch_info["slotIndex"]
            .as_u64()
            .ok_or("epochInfo: no slotIndex")?;
        let n = epoch_info["slotsInEpoch"].as_u64().unwrap_or(432_000) as usize;
        let sched = schedule
            .as_object()
            .ok_or("leaderSchedule: not an object")?;
        let mut slots = vec![u16::MAX; n];
        let mut leaders = vec![];
        for (k, v) in sched {
            let Ok(pk) = k.parse::<Pubkey>() else {
                continue;
            };
            if leaders.len() >= u16::MAX as usize {
                break;
            }
            let li = leaders.len() as u16;
            leaders.push(pk);
            for s in v
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_u64())
            {
                if let Some(e) = slots.get_mut(s as usize) {
                    *e = li;
                }
            }
        }
        let mut m = LeaderMap {
            epoch,
            first_slot: abs - idx,
            slots,
            leaders,
            ..Default::default()
        };
        m.set_nodes(&parse_cluster_nodes(nodes), now_unix);
        Ok(m)
    }

    /// Re-classify from fresh gossip (the schedule stays).
    pub fn set_nodes(&mut self, nodes: &HashMap<Pubkey, NodeInfo>, now_unix: u64) {
        self.classes.clear();
        self.tpu_quic.clear();
        self.missing_nodes = 0;
        for l in &self.leaders {
            match nodes.get(l) {
                Some(n) => {
                    self.classes.push(n.class);
                    self.tpu_quic.push(n.tpu_quic);
                }
                None => {
                    self.missing_nodes += 1;
                    self.classes.push(ClientClass::Other);
                    self.tpu_quic.push(None);
                }
            }
        }
        self.nodes_unix = now_unix;
    }

    /// The map knows this slot's epoch.
    #[inline]
    pub fn covers(&self, slot: u64) -> bool {
        slot >= self.first_slot && slot < self.first_slot + self.slots.len() as u64
    }

    #[inline]
    fn leader_idx(&self, slot: u64) -> Option<usize> {
        if !self.covers(slot) {
            return None;
        }
        let i = self.slots[(slot - self.first_slot) as usize];
        (i != u16::MAX).then_some(i as usize)
    }

    /// The slot's leader identity.
    pub fn leader_at(&self, slot: u64) -> Option<Pubkey> {
        self.leader_idx(slot).map(|i| self.leaders[i])
    }

    /// The slot leader's class (hot path: two loads, no allocation).
    #[inline]
    pub fn class_at(&self, slot: u64) -> Option<ClientClass> {
        self.leader_idx(slot)
            .and_then(|i| self.classes.get(i).copied())
    }

    /// The slot leader's advertised `tpuQuic`.
    pub fn tpu_quic_at(&self, slot: u64) -> Option<SocketAddr> {
        self.leader_idx(slot)
            .and_then(|i| self.tpu_quic.get(i).copied().flatten())
    }

    /// Slots per class over the whole epoch (report).
    pub fn class_slots(&self) -> [(ClientClass, u64); 5] {
        let mut c = [0u64; 5];
        for &i in &self.slots {
            if i != u16::MAX {
                if let Some(k) = self.classes.get(i as usize) {
                    c[*k as usize] += 1;
                }
            }
        }
        ClientClass::ALL.map(|k| (k, c[k as usize]))
    }

    /// Slots of the epoch with a known leader.
    pub fn known_slots(&self) -> usize {
        self.slots.iter().filter(|i| **i != u16::MAX).count()
    }
}

/// The lock-free handle the hot loop and the cold thread share.
pub type LeaderSnapshot = Arc<ArcSwap<LeaderMap>>;

/// An empty snapshot (every slot unknown).
pub fn empty_snapshot() -> LeaderSnapshot {
    Arc::new(ArcSwap::from_pointee(LeaderMap::default()))
}

/// What the cold lane reports.
#[derive(Debug, Clone)]
pub enum LeaderEvent {
    /// A new map was published (epoch, known slots, leaders missing from gossip).
    Published {
        epoch: u64,
        known_slots: usize,
        missing: u32,
        classes: [(ClientClass, u64); 5],
    },
    Error(String),
    Calls(u32),
}

/// Settings of the refresh lane.
#[derive(Debug, Clone)]
pub struct LeaderCfg {
    pub rpc_url: String,
    pub budget: Arc<rugsnipe_state::RpcBudget>,
    /// `getClusterNodes` refresh period (docs/03: every few minutes; every
    /// 60 s around the Alpenglow cutover).
    pub nodes_every_s: u64,
    /// `getEpochInfo` period (a new epoch reloads the schedule).
    pub epoch_every_s: u64,
}

/// Refresh lane (cold runtime): publishes into `snap` until `stop`.
pub async fn run(
    cfg: LeaderCfg,
    snap: LeaderSnapshot,
    out: tokio::sync::mpsc::UnboundedSender<LeaderEvent>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    let mut rpc = match rugsnipe_feed::ws::RpcClient::new(&cfg.rpc_url) {
        Ok(r) => r,
        Err(e) => {
            let _ = out.send(LeaderEvent::Error(format!("leader lane rpc: {e}")));
            return;
        }
    };
    let mut calls = 0u32;
    let (mut last_nodes, mut last_epoch_check) = (0u64, 0u64);
    let mut have_epoch: Option<u64> = None;
    let mut nodes_v = Value::Null;
    while !stop.load(Ordering::Acquire) {
        let now = crate::now_unix();
        let mut publish = false;
        if have_epoch.is_none() || now.saturating_sub(last_epoch_check) >= cfg.epoch_every_s {
            last_epoch_check = now;
            match crate::real::call(
                &mut rpc,
                &cfg.budget,
                &mut calls,
                "getEpochInfo",
                serde_json::json!([{"commitment": "confirmed"}]),
            )
            .await
            {
                Ok(ei) => {
                    let epoch = ei["epoch"].as_u64();
                    if epoch.is_some() && epoch != have_epoch {
                        if nodes_v.is_null() || now.saturating_sub(last_nodes) >= cfg.nodes_every_s
                        {
                            match crate::real::call(
                                &mut rpc,
                                &cfg.budget,
                                &mut calls,
                                "getClusterNodes",
                                serde_json::json!([]),
                            )
                            .await
                            {
                                Ok(n) => {
                                    nodes_v = n;
                                    last_nodes = now;
                                }
                                Err(e) => {
                                    let _ = out
                                        .send(LeaderEvent::Error(format!("getClusterNodes: {e}")));
                                }
                            }
                        }
                        let abs = ei["absoluteSlot"].as_u64().unwrap_or(0);
                        match crate::real::call(
                            &mut rpc,
                            &cfg.budget,
                            &mut calls,
                            "getLeaderSchedule",
                            serde_json::json!([abs, {"commitment": "confirmed"}]),
                        )
                        .await
                        {
                            Ok(s) => match LeaderMap::from_rpc(&ei, &s, &nodes_v, now) {
                                Ok(m) => {
                                    snap.store(Arc::new(m));
                                    have_epoch = epoch;
                                    publish = true;
                                }
                                Err(e) => {
                                    let _ = out.send(LeaderEvent::Error(e));
                                }
                            },
                            Err(e) => {
                                let _ =
                                    out.send(LeaderEvent::Error(format!("getLeaderSchedule: {e}")));
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = out.send(LeaderEvent::Error(format!("getEpochInfo: {e}")));
                }
            }
        }
        if have_epoch.is_some() && now.saturating_sub(last_nodes) >= cfg.nodes_every_s {
            match crate::real::call(
                &mut rpc,
                &cfg.budget,
                &mut calls,
                "getClusterNodes",
                serde_json::json!([]),
            )
            .await
            {
                Ok(n) => {
                    last_nodes = now;
                    let mut m = (**snap.load()).clone();
                    m.set_nodes(&parse_cluster_nodes(&n), now);
                    snap.store(Arc::new(m));
                    nodes_v = n;
                    publish = true;
                }
                Err(e) => {
                    let _ = out.send(LeaderEvent::Error(format!("getClusterNodes: {e}")));
                }
            }
        }
        if publish {
            let m = snap.load();
            let _ = out.send(LeaderEvent::Published {
                epoch: m.epoch,
                known_slots: m.known_slots(),
                missing: m.missing_nodes,
                classes: m.class_slots(),
            });
        }
        if calls > 0 {
            let _ = out.send(LeaderEvent::Calls(calls));
            calls = 0;
        }
        for _ in 0..50 {
            if stop.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_ids_classify_like_stake_geo() {
        for (id, c) in [
            ("AgaveBam", ClientClass::Bam),
            ("FireBAM", ClientClass::Bam),
            ("HarmonicAgave", ClientClass::Harmonic),
            ("HarmonicFiredancer", ClientClass::Harmonic),
            ("JitoLabs", ClientClass::JitoAgave),
            ("Firedancer", ClientClass::Firedancer),
            ("Frankendancer", ClientClass::Firedancer),
            ("Agave", ClientClass::Other),
            ("Rakurai", ClientClass::Other),
            ("Unknown(42)", ClientClass::Other),
            ("", ClientClass::Other),
        ] {
            assert_eq!(ClientClass::from_client_id(id), c, "{id}");
        }
        // the bid class agrees with rugsnipe-core's mapping
        for id in [
            "AgaveBam",
            "FireBAM",
            "HarmonicAgave",
            "JitoLabs",
            "Firedancer",
            "Agave",
        ] {
            assert_eq!(
                ClientClass::from_client_id(id).bid_class(),
                LeaderClass::from_client_id(id),
                "{id}"
            );
        }
        for c in ClientClass::ALL {
            assert_eq!(ClientClass::from_u8(c as u8), Some(c));
        }
        assert_eq!(ClientClass::name_of(UNKNOWN), None);
    }
}
