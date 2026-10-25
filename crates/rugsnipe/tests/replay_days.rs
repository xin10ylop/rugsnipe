//! Chain days through the real hot loop (ignored; run on demand): every
//! launch the export holds, as the gRPC feed delivers it (the create's wire
//! bytes and post-execution trailer, then the create slot's trades with
//! their block indexes), into the same `HotLoop` the box runs, built from the
//! box's own TOML. The copied wallet's transactions drive a real
//! `FollowTracker` (as the cold thread does), and the operator table is
//! swapped at each of its 4-hourly refreshes (a restart, as on the box).
//! What it records is the bot's own decision per launch (rslog `Decision`)
//! and whether it fired.
//!
//! ```text
//! REPLAY_DIR=<dir> REPLAY_OUT=<file> cargo test --release -p rugsnipe \
//!     --test replay_days -- --ignored --nocapture
//! ```
//!
//! `<dir>/config.toml`: the box's config. `<dir>/events.jsonl`, sorted by
//! slot then block index, one of:
//! `{"t":"table","slot":S,"file":"tables/<unix>.json"}`,
//! `{"t":"create","slot":S,"index":I,"mint":M,"tx":<getTransaction result, base64>}`,
//! `{"t":"trade","slot":S,"index":I,"mint":M,"user":U,"buy":b,"sol":n,"tok":n,"fee":n,"vs":n,"vt":n,"sig":s,"ord":n,"is_create":b,"run_end":b}`,
//! `{"t":"attempt","slot":S,"index":I,"mint":M}` (the copied wallet's
//! transaction on the mint, landed or failed),
//! `{"t":"ltrade","slot":S,"index":I,"mint":M,"user":U,"buy":b,"sol":n,"tok":n,"fee":n,"vs":n,"vt":n}`
//! (a trade of create + 1..3: the operator-dump tracker's input, as the cold
//! thread gets the trades of the launches it does not hold).
//!
//! Optional: `<dir>/leaders/<epoch>.json` (`{"epoch","first_slot","schedule"}`,
//! the `getLeaderSchedule` result) and `<dir>/leader_regions.json` give the
//! hot loop its slot leaders and their regions (`entry.leader_regions`), as
//! the cold lane builds them.
//!
//! Left out on purpose: sends (no sender is built; a fire is the fire job),
//! fills (none come back, so the count caps are lifted: they are applied to
//! the output afterwards), the tweet lane (no verdict comes, as for most live
//! creates) and the risk gate's daily loss.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::curve::FeeBps;
use pump_codec::Pubkey;
use rugsnipe::dumpers::{DumpTracker, DumperSnapshot};
use rugsnipe::follow::{Fees, FollowTracker};
use rugsnipe::hot::{HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::leader::{LeaderMap, LeaderSnapshot};
use rugsnipe::msgs::FireJob;
use rugsnipe::operators::OperatorTable;
use rugsnipe::setup::{self, ArmState};
use rugsnipe::stats::decision_label;
use rugsnipe_core::armset::{ArmSet, OpEntry, OpState};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, Rec, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::IntentId;
use rugsnipe_feed::bundle::{BundleRun, CloseReason, RunTrade};
use rugsnipe_feed::{
    Feed, FeedBuf, FeedEvent, FeedSrc, FeedStatus, FiredSet, Payload, Sig, TradeLite,
};
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const A: &str = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2";
const US: &str = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff";

enum Item {
    Wire(u64, Vec<u8>, usize),
    Ev(FeedEvent),
}

/// Events pushed in by the driver, in order; idle when empty.
struct QFeed {
    q: VecDeque<Item>,
    slot: u64,
}

impl Feed for QFeed {
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        let Some(it) = self.q.front() else {
            return FeedStatus::Idle;
        };
        let ok = match it {
            Item::Wire(slot, b, m) => out.push_wire_parts(
                FeedSrc::Grpc,
                rugsnipe_feed::clock::mono_ns(),
                *slot,
                0,
                [b, &[]],
                *m,
            ),
            Item::Ev(e) => out.push(*e),
        };
        if ok {
            self.q.pop_front();
            FeedStatus::Ready(1)
        } else {
            FeedStatus::Full
        }
    }
    fn src(&self) -> FeedSrc {
        FeedSrc::Grpc
    }
    fn slot(&self) -> u64 {
        self.slot
    }
}

fn pk(s: &str) -> Pubkey {
    s.parse().unwrap_or_default()
}

fn sig_of(s: &str) -> Sig {
    let mut out = [0u8; 64];
    if let Ok(v) = bs58::decode(s).into_vec() {
        let n = v.len().min(64);
        out[..n].copy_from_slice(&v[..n]);
    }
    Sig(out)
}

/// The box's TOML, made safe to run here: no sender, no key, no kill file of
/// a real box, and the count caps lifted (no fill ever comes back).
fn box_cfg(dir: &Path, path: &Path) -> rugsnipe_core::config::Config {
    let mut c = setup::load_config(path).unwrap();
    let tmp = tmpdir("replay-days");
    c.risk.kill_file = tmp.join("KILL").display().to_string();
    c.risk.max_open_positions = 1_000_000;
    c.risk.max_buys_per_hour = 1_000_000;
    c.risk.max_probes_per_day = 1_000_000_000;
    c.risk.max_buys_per_op_per_day = 1_000_000_000;
    c.risk.max_daily_loss_lamports = 1_000_000_000_000_000;
    // the table itself is set per refresh (`HotCfg::operators`); the start
    // check only wants the key present
    if c.entry.operators_only && c.entry.operators_file.is_empty() {
        c.entry.operators_file = dir.join("operators.json").display().to_string();
    }
    c.risk.max_consec_losses_per_op = 1_000_000_000;
    c.senders = Default::default();
    c.senders.rpcsend.enabled = false;
    c.secrets.hot_keypair = "hot-keypair".into();
    c.secrets.probe_keypair = "probe-keypair".into();
    c.secrets.api_keys = "api-keys".into();
    c.secrets.armset_pubkey = Pubkey::new_unique().to_string();
    c.secrets.cold_wallet = Pubkey::new_unique().to_string();
    c.feeds.grpc.enabled = true;
    c.feeds.grpc.url = "https://grpc.invalid".into();
    c.feeds.grpc.bundle_run = true;
    c.obs.rslog_dir = dir.join("rslog").display().to_string();
    c
}

/// What the box's start checks see when everything is in place.
fn box_env(c: &rugsnipe_core::config::Config) -> rugsnipe_core::config::ValidateEnv {
    let mut creds = BTreeMap::new();
    for n in [
        &c.secrets.hot_keypair,
        &c.secrets.probe_keypair,
        &c.secrets.api_keys,
    ] {
        creds.insert(n.clone(), Some(0o400));
    }
    rugsnipe_core::config::ValidateEnv {
        clock_offset_ms: Some(0.5),
        post_exec_feed_healthy: true,
        armset: rugsnipe_core::config::ArmSetGate::Verified {
            gate_passed: true,
            probe_allowed: true,
        },
        credential_modes: creds,
        live_cli_flag: true,
        arm_live_token: true,
        armset_probe_stage: Some(c.probe.stage),
    }
}

/// The box's ArmSet: one class operator matching every create.
fn class_armset(c: &rugsnipe_core::config::Config) -> ArmSet {
    let mut set = armset(&[], &[]);
    set.operators = vec![OpEntry {
        op_id: 1,
        state: if c.mode.mode == rugsnipe_core::Mode::Probe {
            OpState::Probe
        } else {
            OpState::Armed
        },
        size_lamports: 500_000_000,
        v_max_lamports: 115_000_000_000,
        ev_cap_lamports: 0,
        ab_skip_p: 0.0,
        family: None,
        exit: None,
        stats: None,
    }];
    set.match_all = Some(1);
    set.gate_passed = true;
    set.probe_allowed = true;
    set.probe_stage = Some(c.probe.stage);
    set
}

struct Box_ {
    hot: HotLoop,
    fire_rx: rtrb::Consumer<FireJob>,
    cold_rx: rtrb::Consumer<rugsnipe::msgs::ColdMsg>,
    fills_tx: rtrb::Producer<rugsnipe::msgs::HotIn>,
    writer: Option<rugsnipe_core::rslog::RslogWriter>,
}

#[allow(clippy::too_many_arguments)]
fn build_box(
    cfg: &rugsnipe_core::config::Config,
    table: Option<OperatorTable>,
    wallet: Arc<rugsnipe_tx::signer::HotSigner>,
    shared: Arc<Shared>,
    leaders: LeaderSnapshot,
    dumpers: DumperSnapshot,
    rslog_dir: &Path,
    n: usize,
) -> Box_ {
    let owner = wallet.pubkey();
    let senders = Arc::new(SenderSet::build(cfg, &box_env(cfg), &ApiKeys::default()).unwrap());
    // a nonce per fire: nothing burns them here (no keeper, no fill)
    let nonces = Arc::new(setup::paper_nonce_pool(owner, 1 << 17));
    let templates = setup::templates(cfg, owner, &nonces).unwrap();
    let set = class_armset(cfg);
    let watch = Arc::new(Watchlist::new());
    watch.load_armset(&set);
    let arm = Arc::new(ArcSwap::from_pointee(ArmState::from_armset(&set, true, "")));
    let risk = Arc::new(RiskGate::new(cfg.risk.clone()));
    risk.set_hot_balance(1_000_000_000_000);
    let (rslog, w) = RslogWriter::spawn(rslog_dir, &format!("r{n}"), box_id("r"), 1 << 16).unwrap();
    let (cold_tx, cold_rx) = rtrb::RingBuffer::new(1 << 16);
    let (fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
    let (fire_tx, fire_rx) = rtrb::RingBuffer::<FireJob>::new(1 << 12);
    let mut reg = prometheus_client::registry::Registry::default();
    let stats = Arc::new(rugsnipe::stats::HotStats::new(&mut reg));
    let mut hc = HotCfg::from_config(cfg, setup::exit_policy(cfg).unwrap());
    hc.operators = table.map(Arc::new);
    let hot = HotLoop::new(HotParts {
        cfg: hc,
        watch,
        arm,
        risk,
        fired: Arc::new(FiredSet::with_capacity(1 << 16)),
        nonces,
        signer: wallet,
        templates,
        senders,
        rslog,
        cold: cold_tx,
        fills_in: fills_rx,
        fire: fire_tx,
        stats,
        shared,
        leaders,
        dumpers,
    });
    Box_ {
        hot,
        fire_rx,
        cold_rx,
        fills_tx,
        writer: Some(w),
    }
}

/// The epoch's leader map from `<dir>/leaders/<epoch>.json`, with regions.
fn leader_map(dir: &Path, epoch: u64, regions: &rugsnipe::leader::RegionMap) -> Option<LeaderMap> {
    let text = std::fs::read_to_string(dir.join(format!("leaders/{epoch}.json"))).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let first = v["first_slot"].as_u64()?;
    let info = serde_json::json!({
        "epoch": epoch,
        "absoluteSlot": first,
        "slotIndex": 0,
        "slotsInEpoch": 432_000,
    });
    let mut m = LeaderMap::from_rpc(&info, &v["schedule"], &serde_json::json!([]), 0).ok()?;
    m.set_regions(regions);
    Some(m)
}

/// One launch's create-slot trades as the cold thread's tracker sees them.
#[derive(Default)]
struct Run {
    slot: u64,
    index: u32,
    dev_fees: Option<bool>,
    cset: Vec<Pubkey>,
    trades: Vec<rugsnipe::filter::S0Trade>,
}

#[test]
#[ignore]
fn replay_days() {
    let dir = PathBuf::from(std::env::var("REPLAY_DIR").expect("REPLAY_DIR"));
    let out_path = PathBuf::from(std::env::var("REPLAY_OUT").expect("REPLAY_OUT"));
    let work = tmpdir("replay-days-run");
    let cfg = box_cfg(&work, &dir.join("config.toml"));
    let wallet = Arc::new(setup::throwaway_signer().unwrap());
    let owner = wallet.pubkey();
    let shared = Arc::new(Shared::default());
    let mut tracker = FollowTracker::new(
        shared.follow.clone(),
        cfg.entry.follow_min_attempts,
        cfg.entry.follow_min_rate,
        cfg.entry.follow_hot_min_value,
        vec![pk(A), pk(US), owner],
    );
    let rslog_dir = work.join("rslog");
    // the cold lane's leader map (entry.leader_regions) and dump tracker
    // (entry.dumper_skip), shared by every restart
    let leaders = rugsnipe::leader::empty_snapshot();
    let regions = {
        let p = dir.join("leader_regions.json");
        let p = if p.exists() {
            p
        } else {
            PathBuf::from(&cfg.entry.leader_regions_file)
        };
        rugsnipe::leader::load_regions(&p).unwrap_or_default()
    };
    let mut epoch = u64::MAX;
    let dumps = rugsnipe::dumpers::snapshot_from(vec![]);
    let mut dump = DumpTracker::new(dumps.clone());
    let mut n_box = 0usize;
    let mut bx = build_box(
        &cfg,
        None,
        wallet.clone(),
        shared.clone(),
        leaders.clone(),
        dumps.clone(),
        &rslog_dir,
        n_box,
    );
    let mut writers = vec![];
    let mut feed = QFeed {
        q: VecDeque::new(),
        slot: 0,
    };
    let mut buf = FeedBuf::with_capacity(64, 1 << 20);
    let mut fired: HashMap<u64, u32> = HashMap::new();
    let mut mints: BTreeMap<u64, (Pubkey, u64)> = BTreeMap::new();
    let mut runs: HashMap<Pubkey, Run> = HashMap::new();
    let mut attempts: Vec<(u64, Pubkey)> = vec![];
    let mut cur_slot = 0u64;
    let mut n_ev = 0u64;
    let drain =
        |bx: &mut Box_, feed: &mut QFeed, buf: &mut FeedBuf, fired: &mut HashMap<u64, u32>| {
            loop {
                let st = bx.hot.step(feed, buf);
                // the cold thread's side: consumed (its trackers run here)
                while bx.cold_rx.pop().is_ok() {}
                while let Ok(j) = bx.fire_rx.pop() {
                    *fired.entry(j.intent).or_default() += 1;
                    // what the cold thread reports when the buy did not land: the
                    // position slot and the risk reservation come back (the fill
                    // itself is not modeled here)
                    let _ = bx.fills_tx.push(rugsnipe::msgs::HotIn::RealFailed {
                        intent: j.intent,
                        cost: 0,
                        dropped: true,
                    });
                }
                if matches!(st, FeedStatus::Idle | FeedStatus::Ended) && feed.q.is_empty() {
                    break;
                }
            }
        };
    let f = std::fs::File::open(dir.join("events.jsonl")).unwrap();
    for line in std::io::BufReader::new(f).lines() {
        let line = line.unwrap();
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let slot = v["slot"].as_u64().unwrap_or(0);
        if slot > cur_slot {
            // the cold thread's view up to the slot before: the runs of
            // closed create slots and the copied wallet's transactions
            let done: Vec<Pubkey> = runs
                .iter()
                .filter(|(_, r)| r.slot < slot)
                .map(|(m, _)| *m)
                .collect();
            for m in done {
                let r = runs.remove(&m).unwrap();
                let ft: Vec<_> = r
                    .trades
                    .iter()
                    .filter(|x| !tracker.excluded().contains(&x.user))
                    .copied()
                    .collect();
                if let Some(fb) = rugsnipe::filter::bundle_features(&ft, &r.cset, None) {
                    let post = ft
                        .iter()
                        .rev()
                        .find(|x| x.is_buy && fb.wallets.contains(&x.user))
                        .map(|x| (x.v, x.t));
                    tracker.on_run(m, r.slot, Fees::of(r.dev_fees), &fb.wallets, post);
                }
                tracker.prune(r.slot);
                // the dump tracker's run: every trade of the create slot
                let mut trades: Vec<RunTrade> = r
                    .trades
                    .iter()
                    .map(|x| RunTrade {
                        index: x.index,
                        is_create: x.is_create,
                        user: x.user,
                        is_buy: x.is_buy,
                        sol: x.sol,
                        tok: x.tok,
                        v: x.v,
                        t: x.t,
                    })
                    .collect();
                trades.sort_by_key(|t| (t.index, !t.is_create));
                dump.on_run(&BundleRun {
                    mint: m,
                    create_sig: Sig::default(),
                    slot: r.slot,
                    create_index: Some(r.index),
                    dev_fees: r.dev_fees,
                    trades,
                    closed: CloseReason::SlotEnded,
                    create_ns: 0,
                    run_closed_ns: 0,
                    closed_ns: 0,
                    out_of_order: 0,
                });
            }
            dump.prune(slot);
            if slot / 432_000 != epoch {
                epoch = slot / 432_000;
                if let Some(m) = leader_map(&dir, epoch, &regions) {
                    leaders.store(Arc::new(m));
                }
            }
            let mut rest = vec![];
            for (s, m) in attempts.drain(..) {
                if s < slot {
                    tracker.on_attempt(m, s);
                } else {
                    rest.push((s, m));
                }
            }
            attempts = rest;
            tracker.publish();
            cur_slot = slot;
            feed.slot = slot;
            drain(&mut bx, &mut feed, &mut buf, &mut fired);
        }
        n_ev += 1;
        match v["t"].as_str().unwrap_or("") {
            "table" => {
                let p = dir.join(v["file"].as_str().unwrap());
                let table = OperatorTable::load(&p).unwrap();
                drain(&mut bx, &mut feed, &mut buf, &mut fired);
                n_box += 1;
                let mut old = std::mem::replace(
                    &mut bx,
                    build_box(
                        &cfg,
                        Some(table),
                        wallet.clone(),
                        shared.clone(),
                        leaders.clone(),
                        dumps.clone(),
                        &rslog_dir,
                        n_box,
                    ),
                );
                while let Ok(j) = old.fire_rx.pop() {
                    *fired.entry(j.intent).or_default() += 1;
                }
                writers.push(old.writer.take());
                drop(old);
            }
            "create" => {
                let mint = pk(v["mint"].as_str().unwrap());
                let index = v["index"].as_u64().unwrap_or(0);
                let fx = serde_json::json!({ "result": v["tx"] });
                let Ok(up) =
                    rugsnipe_feed::grpc::mock::fixture_update(&fx, slot, index, &["creates"])
                else {
                    continue;
                };
                use yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof;
                let Some(UpdateOneof::Transaction(t)) = up.update_oneof else {
                    continue;
                };
                let info = t.transaction.unwrap();
                let Ok(wire) =
                    rugsnipe_feed::grpc::convert::tx_to_wire(info.transaction.as_ref().unwrap())
                else {
                    continue;
                };
                let mut meta = vec![];
                rugsnipe_feed::wiremeta::encode(
                    &rugsnipe_feed::grpc::convert::meta_in(info.meta.as_ref(), Some(index as u32)),
                    &mut meta,
                );
                // the decoded create, for the tracker's run (fee routing,
                // creator set) and the output
                let hit = {
                    use rugsnipe_feed::wiremeta::{WireMeta, EMPTY_INNER, MAX_INNER};
                    let m = WireMeta::parse(&meta).unwrap();
                    let mut loaded = vec![Pubkey::default(); 256];
                    let mut inner = [EMPTY_INNER; MAX_INNER];
                    let (nl, ni) = (m.loaded_into(&mut loaded), m.inner_into(&mut inner));
                    pump_codec::tx::TxView::parse(&wire).ok().and_then(|view| {
                        pump_codec::create_hit(
                            &view,
                            Some(&loaded[..nl]),
                            &inner[..ni],
                            FeeBps::DEFAULT,
                        )
                        .ok()
                        .flatten()
                    })
                };
                if let Some(h) = &hit {
                    let mut cset = vec![h.creator_arg, h.user];
                    if let Some(fp) = h.fee_payer {
                        cset.push(fp);
                    }
                    runs.insert(
                        mint,
                        Run {
                            slot,
                            index: index as u32,
                            dev_fees: Some(h.dev_fees()),
                            cset,
                            trades: vec![],
                        },
                    );
                }
                let intent = IntentId::new(&mint, &owner).0;
                mints.insert(intent, (mint, slot));
                let n = meta.len();
                let mut b = wire;
                b.extend_from_slice(&meta);
                feed.q.push_back(Item::Wire(slot, b, n));
                drain(&mut bx, &mut feed, &mut buf, &mut fired);
            }
            "trade" => {
                let mint = pk(v["mint"].as_str().unwrap());
                let t = TradeLite {
                    mint,
                    user: pk(v["user"].as_str().unwrap()),
                    is_buy: v["buy"].as_bool().unwrap_or(false),
                    sol_amount: v["sol"].as_u64().unwrap_or(0),
                    token_amount: v["tok"].as_u64().unwrap_or(0),
                    fee: v["fee"].as_u64().unwrap_or(0),
                    virtual_sol_reserves: v["vs"].as_u64().unwrap_or(0),
                    virtual_token_reserves: v["vt"].as_u64().unwrap_or(0),
                    slot,
                    sig: sig_of(v["sig"].as_str().unwrap_or("")),
                    ordinal: v["ord"].as_u64().unwrap_or(0) as u8,
                    index: v["index"].as_u64().unwrap_or(u32::MAX as u64) as u32,
                };
                if let Some(r) = runs.get_mut(&mint) {
                    if slot == r.slot {
                        r.trades.push(rugsnipe::filter::S0Trade {
                            index: t.index,
                            is_create: v["is_create"].as_bool().unwrap_or(false),
                            user: t.user,
                            is_buy: t.is_buy,
                            sol: t.sol_amount,
                            tok: t.token_amount,
                            v: t.virtual_sol_reserves,
                            t: t.virtual_token_reserves,
                        });
                    }
                }
                feed.q.push_back(Item::Ev(FeedEvent {
                    src: FeedSrc::Grpc,
                    recv_ns: rugsnipe_feed::clock::mono_ns(),
                    slot,
                    bank_id: 0,
                    payload: Payload::Trade(t),
                }));
                drain(&mut bx, &mut feed, &mut buf, &mut fired);
                if v["run_end"].as_bool().unwrap_or(false) {
                    // the bundle wait (entry.bundle_wait_ms) runs in real time
                    std::thread::sleep(std::time::Duration::from_micros(
                        cfg.entry
                            .bundle_wait_ms
                            .saturating_mul(1_000)
                            .max(cfg.entry.bundle_wait_us)
                            + 500,
                    ));
                    drain(&mut bx, &mut feed, &mut buf, &mut fired);
                }
            }
            "attempt" => {
                attempts.push((slot, pk(v["mint"].as_str().unwrap())));
            }
            "ltrade" => {
                // the cold thread gets the trades of mints the hot loop does
                // not hold: a launch we fired on is ours
                let mint = pk(v["mint"].as_str().unwrap());
                if fired.contains_key(&IntentId::new(&mint, &owner).0) {
                    continue;
                }
                let _ = dump.on_trade(
                    &rugsnipe_exit::Trade {
                        venue: rugsnipe_exit::Venue::Curve,
                        mint,
                        user: pk(v["user"].as_str().unwrap()),
                        is_buy: v["buy"].as_bool().unwrap_or(false),
                        sol_amount: v["sol"].as_u64().unwrap_or(0),
                        token_amount: v["tok"].as_u64().unwrap_or(0),
                        fee_lamports: v["fee"].as_u64().unwrap_or(0),
                        reserve_sol: v["vs"].as_u64().unwrap_or(0),
                        reserve_token: v["vt"].as_u64().unwrap_or(0),
                        slot,
                    },
                    0,
                );
            }
            _ => {}
        }
        if n_ev.is_multiple_of(20_000) {
            eprintln!(
                "replay: {n_ev} events, slot {cur_slot}, fired {}",
                fired.len()
            );
        }
    }
    // the last slot closes
    feed.slot = cur_slot + 2;
    drain(&mut bx, &mut feed, &mut buf, &mut fired);
    while let Ok(j) = bx.fire_rx.pop() {
        *fired.entry(j.intent).or_default() += 1;
    }
    writers.push(bx.writer.take());
    drop(bx);
    for w in writers.into_iter().flatten() {
        w.join().unwrap();
    }
    let mut dec: HashMap<u64, Vec<u8>> = HashMap::new();
    for r in rslog_records(&rslog_dir) {
        if let Rec::Decision { intent, code, .. } = r {
            dec.entry(intent).or_default().push(code);
        }
    }
    let mut out = std::fs::File::create(&out_path).unwrap();
    let mut n_fired = 0;
    for (intent, (mint, slot)) in &mints {
        let codes = dec.get(intent).cloned().unwrap_or_default();
        let fire = fired.get(intent).copied().unwrap_or(0) > 0;
        n_fired += fire as u32;
        let labels: Vec<&str> = codes.iter().map(|c| decision_label(*c)).collect();
        writeln!(
            out,
            "{}",
            serde_json::json!({"mint": mint.to_string(), "slot": slot, "fired": fire, "decisions": labels})
        )
        .unwrap();
    }
    eprintln!(
        "replay: {} creates, {} fired, {} events, follow records {} followed {}, dumpers {} ({} launches tracked)",
        mints.len(),
        n_fired,
        n_ev,
        tracker.records(),
        tracker.len(),
        dump.len(),
        dump.launches
    );
    let _ = std::fs::remove_dir_all(&work);
}
