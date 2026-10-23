//! The leader schedule (docs/03 section 6): a recorded mainnet
//! `getEpochInfo` / `getLeaderSchedule` / `getClusterNodes` (epoch 1059,
//! 2026-10-16, trimmed to 4,000 slots: `fixtures/leader/README.md`) maps
//! every slot to its leader and client class exactly as an independent
//! reading of the JSON does; a gossip refresh re-classifies (a BAM node
//! falling back to Jito-Agave); and the hot loop bids per class of the
//! create slot's leader (tip on Jito-Agave, priority fee on BAM) and
//! records the class on the intent and on every send.

mod common;

use common::*;
use pump_codec::Pubkey;
use rugsnipe::leader::{parse_cluster_nodes, ClientClass, LeaderMap, Region, RegionMap};
use rugsnipe_feed::{FeedBuf, Payload};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fx(name: &str) -> Value {
    let p = format!("{}/fixtures/leader/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    v["result"].clone()
}

#[test]
fn recorded_schedule_maps_every_slot_to_its_leader_and_class() {
    let (ei, ls, cn) = (fx("epoch_info"), fx("leader_schedule"), fx("cluster_nodes"));
    let m = LeaderMap::from_rpc(&ei, &ls, &cn, 1).unwrap();
    assert_eq!(m.epoch, 1059);
    assert_eq!(m.first_slot, 457_488_000);
    assert_eq!(m.known_slots(), 4_000);
    assert_eq!(
        m.missing_nodes, 1,
        "one leader is not in the recorded gossip"
    );
    // an independent reading of the JSON
    let ids: HashMap<String, String> = cn
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            (
                n["pubkey"].as_str().unwrap().to_string(),
                n["clientId"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let mut by_slot: HashMap<u64, String> = HashMap::new();
    for (k, v) in ls.as_object().unwrap() {
        for s in v.as_array().unwrap() {
            by_slot.insert(s.as_u64().unwrap(), k.clone());
        }
    }
    let mut counts: HashMap<ClientClass, u64> = HashMap::new();
    for (idx, leader) in &by_slot {
        let slot = m.first_slot + idx;
        assert_eq!(
            m.leader_at(slot).unwrap().to_string(),
            *leader,
            "slot {slot}"
        );
        let want = ClientClass::from_client_id(ids.get(leader).map(String::as_str).unwrap_or(""));
        assert_eq!(m.class_at(slot), Some(want), "slot {slot}");
        *counts.entry(want).or_default() += 1;
    }
    // the shares of the trimmed window (Python cross-check when recorded)
    let got: HashMap<ClientClass, u64> = m
        .class_slots()
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .collect();
    assert_eq!(got, counts);
    assert_eq!(got[&ClientClass::Bam], 1_436);
    assert_eq!(got[&ClientClass::Harmonic], 1_152);
    assert_eq!(got[&ClientClass::JitoAgave], 1_064);
    assert_eq!(got[&ClientClass::Firedancer], 256);
    assert_eq!(got[&ClientClass::Other], 92);
    // slots of the window have an advertised tpuQuic; outside: unknown
    let some = m.first_slot + *by_slot.keys().min().unwrap();
    assert!(m.tpu_quic_at(some).is_some());
    assert_eq!(m.class_at(m.first_slot - 1), None);
    assert_eq!(
        m.class_at(m.first_slot + 431_999),
        None,
        "a slot outside the trimmed window"
    );
    assert_eq!(m.class_at(m.first_slot + 432_000), None, "the next epoch");
}

#[test]
fn a_gossip_refresh_reclassifies_a_bam_fallback() {
    let (ei, ls, cn) = (fx("epoch_info"), fx("leader_schedule"), fx("cluster_nodes"));
    let mut m = LeaderMap::from_rpc(&ei, &ls, &cn, 1).unwrap();
    let bam = (0..4_000u64)
        .map(|i| m.first_slot + 113_000 + i)
        .find(|s| m.class_at(*s) == Some(ClientClass::Bam))
        .unwrap();
    let who = m.leader_at(bam).unwrap().to_string();
    let mut nodes = cn.clone();
    for n in nodes.as_array_mut().unwrap() {
        if n["pubkey"] == who.as_str() {
            n["clientId"] = json!("JitoLabs");
        }
    }
    m.set_nodes(&parse_cluster_nodes(&nodes), 2);
    assert_eq!(m.class_at(bam), Some(ClientClass::JitoAgave));
    assert_eq!(m.nodes_unix, 2);
}

/// A two-leader schedule from slot 1,000: a Jito-Agave leader for slots
/// 1,000-1,001, a BAM leader for 1,002-1,003.
fn synthetic(jito: Pubkey, bam: Pubkey) -> LeaderMap {
    let ei = json!({"epoch": 1, "absoluteSlot": 1_000, "slotIndex": 0, "slotsInEpoch": 8});
    let ls = json!({jito.to_string(): [0, 1], bam.to_string(): [2, 3]});
    let cn = json!([
        {"pubkey": jito.to_string(), "clientId": "JitoLabs", "tpuQuic": "10.0.0.1:8009"},
        {"pubkey": bam.to_string(), "clientId": "AgaveBam", "tpuQuic": "10.0.0.2:8009"},
    ]);
    LeaderMap::from_rpc(&ei, &ls, &cn, 1).unwrap()
}

#[test]
fn the_hot_loop_bids_per_leader_class_and_records_it() {
    let dir = tmpdir("leader-bids");
    let dev = Pubkey::new_unique();
    let cfg = paper_cfg(&dir);
    let mut app = build(cfg, armset(&[dev], &[]), &dir, 300, false);
    app.leaders.store(Arc::new(synthetic(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    )));
    let (h1, _) = create(Pubkey::new_unique(), dev, 1_000, 1, 500_000_000);
    let (h2, _) = create(Pubkey::new_unique(), dev, 1_002, 2, 500_000_000);
    let mut feed = VecFeed::new(vec![
        ev(1_000, Payload::Hit(h1)),
        ev(1_002, Payload::Hit(h2)),
    ]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while app.hot.counters.intents < 2 {
        app.hot.step(&mut feed, &mut buf);
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "{:?}",
            app.hot.counters
        );
    }
    for _ in 0..50 {
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(2));
    }
    app.shutdown(vec![]);
    let c = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let class_of = |mint: &Pubkey| -> String {
        c.query_row(
            "SELECT leader_class FROM intents WHERE mint = ?1",
            [mint.to_string()],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(class_of(&h1.mint), "jito_agave");
    assert_eq!(class_of(&h2.mint), "bam");
    let sends = |mint: &Pubkey| -> Vec<(u8, i64, i64, String)> {
        let mut st = c
            .prepare(
                "SELECT s.variant, s.prio, s.tip, s.leader_class FROM sends s JOIN intents i ON i.intent_id = s.intent_id
                 WHERE i.mint = ?1 AND s.kind = 'buy' ORDER BY s.variant",
            )
            .unwrap();
        st.query_map([mint.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .map(|x| x.unwrap())
        .collect()
    };
    // both slots' leaders are of one class: one plan, one variant (the same
    // plan twice would be the same transaction)
    let j = sends(&h1.mint);
    assert_eq!(j.len(), 1, "{j:?}");
    for (_, prio, tip, class) in &j {
        // a tip-ordered leader: the bid rides the tip (pay on success), priority at the floor
        assert_eq!(class, "jito_agave");
        assert_eq!(*prio, 10_000, "{j:?}");
        assert!(*tip > 0, "{j:?}");
    }
    let b = sends(&h2.mint);
    assert_eq!(b.len(), 1, "{b:?}");
    for (_, prio, tip, class) in &b {
        // a priority-ordered leader: the bid is the (clamped) priority fee, no tip
        assert_eq!(class, "bam");
        assert_eq!(*tip, 0, "{b:?}");
        assert!(*prio > 10_000, "{b:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_leader_change_next_slot_gets_its_own_variant() {
    let dir = tmpdir("leader-bids-2");
    let dev = Pubkey::new_unique();
    let mut app = build(paper_cfg(&dir), armset(&[dev], &[]), &dir, 300, false);
    app.leaders.store(Arc::new(synthetic(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    )));
    // slot 1,001: Jito-Agave leader now, BAM leader next slot
    let (h, _) = create(Pubkey::new_unique(), dev, 1_001, 1, 500_000_000);
    let mut feed = VecFeed::new(vec![ev(1_001, Payload::Hit(h))]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    for _ in 0..100 {
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.hot.counters.intents, 1);
    app.shutdown(vec![]);
    let c = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let mut st = c
        .prepare("SELECT variant, prio, tip, leader_class FROM sends WHERE kind = 'buy' ORDER BY variant")
        .unwrap();
    let rows: Vec<(u8, i64, i64, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!((rows[0].0, rows[0].3.as_str()), (0, "jito_agave"));
    assert!(rows[0].2 > 0 && rows[0].1 == 10_000, "{rows:?}");
    assert_eq!((rows[1].0, rows[1].3.as_str()), (1, "bam"));
    assert!(rows[1].2 == 0 && rows[1].1 > 10_000, "{rows:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn regions_attach_per_leader_and_unknown_identities_stay_unknown() {
    let (jito, bam) = (Pubkey::new_unique(), Pubkey::new_unique());
    let mut m = synthetic(jito, bam);
    assert_eq!(m.region_at(1_000), Some(Region::Unknown));
    let mut map = RegionMap::new();
    map.insert(jito, Region::Na);
    map.insert(bam, Region::Eu);
    m.set_regions(&map);
    assert_eq!(m.region_at(1_000), Some(Region::Na));
    assert_eq!(m.region_at(1_001), Some(Region::Na));
    assert_eq!(m.region_at(1_002), Some(Region::Eu));
    assert_eq!(m.region_at(999), None, "before the epoch");
    // a refresh of the classes keeps the regions
    let cn = json!([{"pubkey": jito.to_string(), "clientId": "AgaveBam"}]);
    m.set_nodes(&parse_cluster_nodes(&cn), 2);
    assert_eq!(m.region_at(1_002), Some(Region::Eu));
    assert_eq!(Region::parse("asia"), Some(Region::Asia));
    assert_eq!(Region::parse("mars"), None);
    assert_eq!(rugsnipe::hot::leader_mask(&[]), None);
    let mask = rugsnipe::hot::leader_mask(&["eu".into()]).unwrap();
    assert!(mask[Region::Eu as usize] && mask[Region::Unknown as usize]);
    assert!(!mask[Region::Na as usize] && !mask[Region::Asia as usize]);
}

#[test]
fn the_hot_loop_skips_a_create_whose_slot_leader_is_out_of_region() {
    let dir = tmpdir("leader-region");
    let dev = Pubkey::new_unique();
    let mut cfg = paper_cfg(&dir);
    cfg.entry.leader_regions = vec!["eu".into()];
    let mut app = build(cfg, armset(&[dev], &[]), &dir, 300, false);
    let (na, eu) = (Pubkey::new_unique(), Pubkey::new_unique());
    let mut m = synthetic(na, eu);
    let mut map = RegionMap::new();
    map.insert(na, Region::Na);
    map.insert(eu, Region::Eu);
    m.set_regions(&map);
    app.leaders.store(Arc::new(m));
    // slot 1,000: the North American leader -> skipped; slot 1,002: Europe -> an intent
    let (h1, _) = create(Pubkey::new_unique(), dev, 1_000, 1, 500_000_000);
    let (h2, _) = create(Pubkey::new_unique(), dev, 1_002, 2, 500_000_000);
    let mut feed = VecFeed::new(vec![
        ev(1_000, Payload::Hit(h1)),
        ev(1_002, Payload::Hit(h2)),
    ]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while app.hot.counters.intents < 1 {
        app.hot.step(&mut feed, &mut buf);
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "{:?}",
            app.hot.counters
        );
    }
    for _ in 0..20 {
        app.hot.step(&mut feed, &mut buf);
    }
    assert_eq!(app.hot.counters.intents, 1, "{:?}", app.hot.counters);
    let skipped = app
        .stats
        .decision_counts()
        .into_iter()
        .find(|(k, _)| *k == "skip_far_leader")
        .map(|(_, n)| n);
    assert_eq!(skipped, Some(1));
    app.shutdown(vec![]);
    let c = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM intents WHERE mint = ?1",
            [h2.mint.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    let _ = std::fs::remove_dir_all(&dir);
}
