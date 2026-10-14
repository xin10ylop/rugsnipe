//! rslog: schema sizes, golden file, decode round trip, SPSC writer thread.

use rugsnipe_core::rslog::*;
use rugsnipe_core::Pubkey;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sample_records() -> Vec<Rec> {
    let k = |b: u8| Pubkey::new_from_array([b; 32]);
    vec![
        Rec::FeedSeen {
            src: 1,
            slot: 456_789_000,
            recv_ns: 123,
            sig: [1; 64],
        },
        Rec::CreateHit(CreateHitRec {
            src: 1,
            slot: 456_789_000,
            sig: [2; 64],
            mint: k(3),
            user: k(4),
            fee_payer: k(5),
            creator: k(6),
            launcher: Pubkey::default(),
            intx_sol_in: 1_062_220_131,
            intx_tokens_out: 36_692_876_256_749,
            intx_buys: 2,
            n_signers: 3,
            flags: hit_flags::SOL_QUOTE | hit_flags::EXACT | hit_flags::TOKEN_2022,
        }),
        Rec::Matched {
            intent: 0xdead_beef,
            op_id: 1234,
            armset_ver: 57,
            via: 1,
            shadow: 0,
        },
        Rec::Decision {
            intent: 0xdead_beef,
            mint: k(3),
            mode: 0,
            code: 0x11,
            would_skip: 0,
            size: 100_000_000,
            v_max: 40_000_000_000,
            min_tokens: 1,
            nonce_idx: 7,
            decided_ns: 999,
        },
        Rec::Sent {
            intent: 0xdead_beef,
            variant: 0,
            sender: 0,
            prio: 7_200,
            tip: 1_000_000,
            sig: [9; 64],
            result: 1,
            latency_ns: 42,
        },
        Rec::PosEvent {
            intent: 0xdead_beef,
            kind: 1,
            slot: 456_789_001,
            v: 33_000_000_000,
            tokens: 1_000,
        },
        Rec::ExitFired {
            intent: 0xdead_beef,
            reason: 3,
            tokens: 1_000,
            min_out: 5,
            sig: [8; 64],
        },
        Rec::Risk {
            kind: 4,
            value: -600_000_000,
        },
        Rec::FeedHealth {
            src: 1,
            ok: 1,
            lag_ms: 180,
            decode_miss_ppm: 12,
        },
        Rec::Lease {
            name_hash: 0xabcd,
            owner: 2,
            epoch: 3,
            hb_unix_ms: 1_791_968_762_000,
        },
        Rec::Upgrade {
            program: pump_codec::constants::PUMP_PROGRAM_ID,
            slot: 456_494_698,
        },
        Rec::NonceEvt {
            idx: 7,
            from: 0,
            to: 1,
            slot: 456_789_000,
            value: [0x77; 32],
            intent: 0xdead_beef,
        },
        Rec::Shadow {
            intent: 0xdead_beef,
            mint: k(3),
            model: 1,
            outcome: ShadowOutcome::Filled as u8,
            create_slot: 456_789_000,
            create_index: 412,
            block_txs: 1_310,
            bundle_trades: 4,
            bundle_wallets: 3,
            our_index: 417,
            tp_ahead: 0,
            tp_behind: 2,
            s0_failed: 1,
            premium_bps: 110,
            post_bundle_v: 33_000_000_000,
            post_bundle_t: 975_454_545_454_545,
            post_bundle_mcap: 33_830_000_000,
        },
        Rec::FailedBuy {
            intent: 0xdead_beef,
            mint: k(3),
            reason: ShadowOutcome::Premium as u8,
            cost: 3_000_000,
            slot: 456_789_000,
            premium_bps: 340,
        },
    ]
}

#[test]
fn encoder_matches_schema_toml() {
    let schema: toml::Table =
        toml::from_str(&std::fs::read_to_string(dir().join("schema/rslog.toml")).unwrap()).unwrap();
    assert_eq!(
        schema["header"]["size"].as_integer().unwrap() as usize,
        HEADER_LEN
    );
    let kinds = schema["kind"].as_array().unwrap();
    assert_eq!(kinds.len(), KINDS.len());
    for rec in sample_records() {
        let k = kinds
            .iter()
            .find(|t| t["id"].as_integer().unwrap() as u16 == rec.kind())
            .unwrap();
        let mut b = vec![];
        rec.encode_payload(&mut b);
        assert_eq!(
            b.len() as i64,
            k["size"].as_integer().unwrap(),
            "{:?}",
            k["name"]
        );
        let name = KINDS.iter().find(|(i, _)| *i == rec.kind()).unwrap().1;
        assert_eq!(k["name"].as_str().unwrap(), name);
        // field sizes add up
        let sum: usize = k["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let t = f.as_str().unwrap().split(':').nth(1).unwrap();
                match t {
                    "u8" => 1,
                    "u16" => 2,
                    "u32" => 4,
                    "u64" | "i64" => 8,
                    s if s.starts_with('[') => s.trim_matches(['[', ']']).parse::<usize>().unwrap(),
                    s => panic!("type {s}"),
                }
            })
            .sum();
        assert_eq!(sum, b.len(), "{name}");
    }
}

#[test]
fn golden_file_round_trip() {
    let mut bytes = vec![];
    for (i, r) in sample_records().iter().enumerate() {
        r.encode_frame(
            1_000 + i as u64,
            1_791_968_762_000_000_000 + i as u64,
            0x0f1a,
            &mut bytes,
        );
    }
    let golden = dir().join("fixtures/golden.rslog");
    if std::env::var_os("RUGSNIPE_UPDATE_GOLDEN").is_some() || !golden.exists() {
        std::fs::write(&golden, &bytes).unwrap();
    }
    let want = std::fs::read(&golden).unwrap();
    assert_eq!(bytes, want, "encoder drifted from fixtures/golden.rslog");
    let (frames, end) = decode_frames(&want);
    assert_eq!(end, want.len());
    assert_eq!(
        frames.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        sample_records()
    );
    assert!(frames
        .iter()
        .all(|(h, _)| h.box_id == 0x0f1a && h.ver == VER));
    // truncated stream stops cleanly
    let (part, stop) = decode_frames(&want[..want.len() - 3]);
    assert_eq!(part.len(), frames.len() - 1);
    assert!(stop < want.len());
}

#[test]
fn shadow_outcomes_round_trip_and_classify() {
    for o in ShadowOutcome::ALL {
        assert_eq!(ShadowOutcome::from_u8(o as u8), Some(o));
    }
    assert_eq!(ShadowOutcome::from_u8(6), None);
    let failed: Vec<&str> = ShadowOutcome::ALL
        .iter()
        .filter(|o| o.is_failed_attempt())
        .map(|o| o.as_str())
        .collect();
    assert_eq!(failed, ["premium", "slippage", "not_in_slot0"]);
}

#[test]
fn writer_thread_drains_the_ring() {
    let d = std::env::temp_dir().join(format!("rslog-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (mut p, w) = RslogWriter::spawn(&d, "test", box_id("test"), 64).unwrap();
    let recs = sample_records();
    let mut pushed = 0;
    for i in 0..500 {
        if p.push(recs[i % recs.len()]) {
            pushed += 1;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
    let dropped = p.dropped();
    drop(p);
    let written = w.join().unwrap();
    assert_eq!(written, pushed);
    assert_eq!(pushed + dropped, 500);
    let mut all = vec![];
    for e in std::fs::read_dir(&d).unwrap() {
        all.extend(std::fs::read(e.unwrap().path()).unwrap());
    }
    let (frames, end) = decode_frames(&all);
    assert_eq!(end, all.len());
    assert_eq!(frames.len() as u64, written);
    assert!(frames.windows(2).all(|w| w[0].0.mono_ns <= w[1].0.mono_ns));
    let _ = std::fs::remove_dir_all(&d);
}
