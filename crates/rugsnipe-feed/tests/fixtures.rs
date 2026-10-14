//! Decoding on real data: live `logsSubscribe` frames recorded by
//! `examples/tail.rs`, the pump-codec transaction fixtures (their
//! `meta.logMessages` wrapped as notifications), PumpPortal relay lines and a
//! `getTransaction(jsonParsed)` funding transfer.

use base64::Engine as _;
use pump_codec::constants::{SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use pump_codec::events::{decode_event_payload, scan_program_data_logs, EventSource, PumpEvent};
use pump_codec::{pda, Pubkey};
use rugsnipe_feed::decode::{decode_logs, funding_from_get_transaction, parse_ws_message, WsMsg};
use rugsnipe_feed::replay::Replay;
use rugsnipe_feed::*;

fn fixture(path: &str) -> String {
    std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn codec_fixture(name: &str) -> serde_json::Value {
    serde_json::from_str(&fixture(&format!("../pump-codec/fixtures/tx/{name}.json"))).unwrap()
}

/// A logsNotification frame built from a pump-codec fixture's real logs.
fn frame_from_codec(name: &str, sub: u64) -> String {
    let v = codec_fixture(name);
    let r = &v["result"];
    let sig = v["query"]["params"][0].as_str().unwrap();
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "logsNotification",
        "params": {
            "result": {
                "context": {"slot": r["slot"]},
                "value": {"signature": sig, "err": r["meta"]["err"], "logs": r["meta"]["logMessages"]}
            },
            "subscription": sub
        }
    })
    .to_string()
}

/// Independent reference: the in-tx buys of `mint` from pump-codec directly.
fn reference_buys(logs: &[String], mint: &Pubkey) -> (u64, u64, u8) {
    let scan = scan_program_data_logs(logs).unwrap();
    let (mut sol, mut tok, mut n) = (0u64, 0u64, 0u8);
    for d in &scan.data {
        if EventSource::from_program(&d.program) != Some(EventSource::Pump) {
            continue;
        }
        if let Ok(PumpEvent::Trade(t)) = decode_event_payload(EventSource::Pump, &d.data) {
            if t.mint == *mint && t.is_buy {
                sol += t.sol_amount;
                tok += t.token_amount;
                n += 1;
            }
        }
    }
    (sol, tok, n)
}

fn hit_of(frame: &str) -> Option<CreateHitLite> {
    let WsMsg::Logs(n) = parse_ws_message(frame).unwrap() else {
        panic!("not a notification")
    };
    let mut tr = Vec::new();
    decode_logs(
        n.slot,
        Sig::from_b58(&n.signature).unwrap(),
        n.failed,
        &n.logs,
        &mut tr,
    )
    .hit
}

#[test]
fn live_recorded_frames_decode() {
    let text = fixture("fixtures/logs_frames.jsonl");
    let r = Replay::from_text(&text, false);
    assert_eq!(r.bad_lines, 0);
    let h = r.load_health().snapshot();
    let hits: Vec<CreateHitLite> = r
        .events()
        .iter()
        .filter_map(|e| match e.payload {
            Payload::Hit(h) => Some(h),
            _ => None,
        })
        .collect();
    assert!(hits.len() >= 10, "{} hits", hits.len());
    assert_eq!(h.creates as usize, hits.len());
    assert_eq!(h.decode_miss, 0);
    assert!(h.failed_tx >= 1, "fixture includes failed transactions");
    // cross-check every hit against pump-codec on the same logs
    let mut checked = 0;
    for line in text.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let fr = &v["frame"];
        if fr["method"] != "logsNotification" || !fr["params"]["result"]["value"]["err"].is_null() {
            continue;
        }
        let logs: Vec<String> = fr["params"]["result"]["value"]["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().to_string())
            .collect();
        let sig = fr["params"]["result"]["value"]["signature"]
            .as_str()
            .unwrap();
        let hit = hits
            .iter()
            .find(|h| h.sig.to_b58() == sig)
            .expect("every successful create frame yields a hit");
        let (sol, tok, n) = reference_buys(&logs, &hit.mint);
        assert_eq!(
            (hit.intx_sol_in, hit.intx_tokens_out, hit.intx_buys),
            (sol, tok, n),
            "{sig}"
        );
        assert_eq!(hit.bc, pda::bonding_curve(&hit.mint));
        assert!(hit.has(hit_flags::ABC_KNOWN));
        assert_eq!(
            hit.abc,
            pda::associated_bonding_curve(&hit.mint, &hit.token_program)
        );
        assert_eq!(
            hit.slot,
            fr["params"]["result"]["context"]["slot"].as_u64().unwrap()
        );
        assert!(hit.has(hit_flags::CREATOR_KNOWN));
        checked += 1;
    }
    assert_eq!(checked, hits.len());
}

#[test]
fn three_signer_create_has_truncated_logs() {
    // create_v2 + dev legacy buy + v3 buy by GGd2xV... (docs/06 section 0): the runtime
    // truncated this transaction's logs before the buys' TradeEvents, so the free
    // logs path sees the create but not the in-tx buys. The hit says so; the full
    // CreateHit needs wire bytes or inner instructions (getTransaction lane).
    let h = hit_of(&frame_from_codec("v1_create_v2_buy", 1)).unwrap();
    assert!(h.has(hit_flags::LOGS_TRUNCATED));
    assert_eq!(
        h.intx_buys, 0,
        "buys lost to truncation: intx_* are lower bounds"
    );
    assert_eq!(h.token_program, TOKEN_2022_PROGRAM_ID);
    assert!(h.is_sol_quote() && !h.mayhem);
}

#[test]
fn create_with_dev_buy_sums_match_pump_codec() {
    for name in [
        "fresh_v0alt_create_v2_buy_v3",
        "v0_create_v2_buy_v3",
        "fresh_v0alt_create_v2_bxsi",
    ] {
        let h = hit_of(&frame_from_codec(name, 1)).unwrap();
        let v = codec_fixture(name);
        let logs: Vec<String> = v["result"]["meta"]["logMessages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().into())
            .collect();
        let (sol, tok, n) = reference_buys(&logs, &h.mint);
        assert!(n >= 1, "{name}");
        assert_eq!(
            (h.intx_sol_in, h.intx_tokens_out, h.intx_buys),
            (sol, tok, n),
            "{name}"
        );
        assert!(!h.has(hit_flags::LOGS_TRUNCATED));
        assert!(h.v_after > 30_000_000_000, "{name}");
    }
}

#[test]
fn legacy_create_is_spl_token() {
    let h = hit_of(&frame_from_codec("legacy_create_spl", 1)).unwrap();
    assert!(h.has(hit_flags::LEGACY_CREATE) && h.has(hit_flags::TOKEN_PROGRAM_KNOWN));
    assert_eq!(h.token_program, SPL_TOKEN_PROGRAM_ID);
    assert_eq!(
        h.abc,
        pda::associated_bonding_curve(&h.mint, &SPL_TOKEN_PROGRAM_ID)
    );
}

#[test]
fn mayhem_non_sol_and_truncated_creates_are_flagged() {
    let m = hit_of(&frame_from_codec("fresh_v1_create_v2_mayhem_buy_v2", 1)).unwrap();
    assert!(m.mayhem);
    let q = hit_of(&frame_from_codec("fresh_v0_create_v2_quote_tail_no_buy", 1)).unwrap();
    assert!(!q.is_sol_quote(), "XsoCS1 quote");
    assert_eq!(q.intx_buys, 0);
    // logs truncated in this fixture: either the CreateEvent survived (flagged) or it is a miss
    let f = frame_from_codec("fresh_v1_create_v2_usdc_buy_v2", 1);
    let WsMsg::Logs(n) = parse_ws_message(&f).unwrap() else {
        panic!()
    };
    let mut tr = Vec::new();
    let o = decode_logs(n.slot, Sig::default(), n.failed, &n.logs, &mut tr);
    assert!(o.truncated);
    match o.hit {
        Some(h) => assert!(h.has(hit_flags::LOGS_TRUNCATED)),
        None => assert!(o.is_miss()),
    }
}

#[test]
fn create_that_completes_the_curve() {
    let h = hit_of(&frame_from_codec("create_buy_complete", 1)).unwrap();
    assert!(h.intx_buys >= 1);
    // all sellable tokens bought in the create tx
    assert_eq!(h.intx_tokens_out, 793_100_000_000_000);
}

#[test]
fn trades_without_create_are_not_hits() {
    let f = frame_from_codec("legacy_buy18", 1);
    let WsMsg::Logs(n) = parse_ws_message(&f).unwrap() else {
        panic!()
    };
    let mut tr = Vec::new();
    let o = decode_logs(n.slot, Sig::default(), n.failed, &n.logs, &mut tr);
    assert!(o.hit.is_none() && !o.create_attempt && !o.is_miss());
    assert_eq!(tr.len(), 1);
    assert!(tr[0].is_buy && tr[0].fee > 0);
    // replay with trades on: one Trade event
    let line = format!(r#"{{"recv_ns":5,"src":"logsws","frame":{f}}}"#);
    let r = Replay::from_text(&line, true);
    assert!(matches!(r.events()[0].payload, Payload::Trade(t) if t.ordinal == 0));
}

#[test]
fn wire_frames_go_to_the_arena() {
    let v = codec_fixture("v0_create_v2_buy_v3");
    let b64 = v["result"]["transaction"][0].as_str().unwrap();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    let line = format!(r#"{{"recv_ns":7,"src":"grpc","slot":456782396,"wire_b64":"{b64}"}}"#);
    let mut r = Replay::from_text(&line, false);
    let mut buf = FeedBuf::with_capacity(4, 4096);
    assert_eq!(r.poll(&mut buf), FeedStatus::Ready(1));
    let ev = buf.events()[0];
    assert_eq!((ev.src, ev.slot), (FeedSrc::Grpc, 456782396));
    let Payload::Wire(w) = ev.payload else {
        panic!()
    };
    assert_eq!(buf.wire(w), &raw[..]);
    // its dedup key is the first signature's hash
    let sig = Sig::from_b58(v["query"]["params"][0].as_str().unwrap()).unwrap();
    let k = ev
        .dedup_key(|w| event::wire_first_sig(buf.wire(w)).map(xxhash_rust::xxh3::xxh3_64))
        .unwrap();
    assert_eq!(k, sig.hash());
    let tx = pump_codec::tx::TxView::parse(buf.wire(w)).unwrap();
    assert_eq!(tx.signature(0).unwrap(), &sig.0);
    assert_eq!(r.poll(&mut buf), FeedStatus::Ended);
}

#[test]
fn relay_lines_and_cross_feed_dedup() {
    let text = fixture("fixtures/relay_pp.txt");
    let r = Replay::from_text(&text, false);
    let hits = r
        .events()
        .iter()
        .filter(|e| matches!(e.payload, Payload::Hit(_)))
        .count();
    assert_eq!(hits, 3, "two creates + one duplicate line");
    assert!(r.events().iter().all(|e| e.src == FeedSrc::Relay));
    let mut mux = FeedMux::new(vec![Box::new(r)]);
    let mut buf = FeedBuf::with_capacity(16, 0);
    assert_eq!(mux.poll(&mut buf), FeedStatus::Ready(3));
    let dups: Vec<_> = buf
        .events()
        .iter()
        .filter_map(|e| match e.payload {
            Payload::Dup(d) => Some(d),
            _ => None,
        })
        .collect();
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].first_src, FeedSrc::Relay);
    assert_eq!(dups[0].delay_ns, 2_000_000);
    let mayhem = buf
        .events()
        .iter()
        .filter(|e| matches!(e.payload, Payload::Hit(h) if h.mayhem))
        .count();
    assert_eq!(mayhem, 1);
}

#[test]
fn funding_transfer_from_get_transaction() {
    let v: serde_json::Value =
        serde_json::from_str(&fixture("fixtures/funding_get_transaction.json")).unwrap();
    let funder: Pubkey = v["funder"].as_str().unwrap().parse().unwrap();
    let result = v["result"].to_string();
    let mut out = Vec::new();
    let n = funding_from_get_transaction(&result, &funder, &mut out).unwrap();
    assert!(n >= 1);
    let f = out[0];
    assert_eq!(f.funder, funder);
    // the parsed instruction is the oracle
    let ix = v["result"]["transaction"]["message"]["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["program"] == "system" && i["parsed"]["info"]["source"] == funder.to_string())
        .unwrap();
    assert_eq!(
        f.lamports,
        ix["parsed"]["info"]["lamports"].as_u64().unwrap()
    );
    assert_eq!(
        f.recipient.to_string(),
        ix["parsed"]["info"]["destination"].as_str().unwrap()
    );
    assert_eq!(f.slot, v["result"]["slot"].as_u64().unwrap());
    assert_ne!(f.recipient_pre_lamports, u64::MAX);
    assert!(!f.sig.is_zero());
    // the same through a full JSON-RPC envelope; another funder finds nothing
    let env = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{result}}}"#);
    let mut out2 = Vec::new();
    funding_from_get_transaction(&env, &funder, &mut out2).unwrap();
    assert_eq!(out2, out);
    let mut none = Vec::new();
    assert_eq!(
        funding_from_get_transaction(&result, &Pubkey::new_unique(), &mut none).unwrap(),
        0
    );
    assert!(funding_from_get_transaction(
        r#"{"jsonrpc":"2.0","id":1,"result":null}"#,
        &funder,
        &mut none
    )
    .is_err());
}
