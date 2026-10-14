//! The E1 lane against a mock JSON-RPC endpoint: getSignaturesForAddress ->
//! getBlock(signatures) -> getTransaction, the gap-3 bundle, the verdict,
//! the replay tape, the session sniper set and the RPC budget.

use base64::Engine as _;
use pump_codec::constants as c;
use pump_codec::curve::CurveState;
use pump_codec::Pubkey;
use rugsnipe::e1::{resolve, E1Cfg, E1Req};
use rugsnipe_core::config::EntryFilter;
use rugsnipe_core::rslog::ShadowOutcome;
use rugsnipe_state::RpcBudget;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const S: u64 = 900;

/// `Program data:` line of a (2024-layout) TradeEvent.
fn trade_log(
    mint: &Pubkey,
    user: &Pubkey,
    buy: bool,
    sol: u64,
    tok: u64,
    c: &CurveState,
) -> String {
    let mut d = c::event::TRADE.to_vec();
    d.extend_from_slice(mint.as_ref());
    d.extend_from_slice(&sol.to_le_bytes());
    d.extend_from_slice(&tok.to_le_bytes());
    d.push(buy as u8);
    d.extend_from_slice(user.as_ref());
    d.extend_from_slice(&0i64.to_le_bytes());
    d.extend_from_slice(&c.virtual_sol.to_le_bytes());
    d.extend_from_slice(&c.virtual_token.to_le_bytes());
    format!(
        "Program data: {}",
        base64::engine::general_purpose::STANDARD.encode(d)
    )
}

struct Chain {
    curve: CurveState,
    mint: Pubkey,
    txs: HashMap<String, Value>,
    sigs: Vec<Value>,
    block: Vec<String>,
    /// getTransaction(sig) answers HTTP 429 this many times first.
    flaky: Mutex<HashMap<String, u32>>,
}

impl Chain {
    fn new(mint: Pubkey) -> Self {
        Self {
            curve: CurveState::fresh(Pubkey::default()),
            mint,
            txs: HashMap::new(),
            sigs: vec![],
            block: (0..10).map(|i| format!("vote{i}")).collect(),
            flaky: Mutex::new(HashMap::new()),
        }
    }
    /// A transaction by `signer` with buys (`> 0`, lamports) / sells (`< 0`, tokens).
    fn tx(&mut self, sig: &str, slot: u64, signer: Pubkey, trades: &[i64], ok: bool) {
        let mut logs = vec![format!("Program {} invoke [1]", c::PUMP_PROGRAM_ID)];
        for &x in trades {
            if x > 0 {
                let tok = self.curve.tokens_for_sol(x as u64);
                self.curve.apply_buy(tok, x as u64);
                logs.push(trade_log(
                    &self.mint,
                    &signer,
                    true,
                    x as u64,
                    tok,
                    &self.curve,
                ));
            } else {
                let tok = (-x) as u64;
                let sol = self.curve.sell_sol_out(tok);
                self.curve.apply_sell(tok, sol);
                logs.push(trade_log(&self.mint, &signer, false, sol, tok, &self.curve));
            }
        }
        logs.push(format!("Program {} success", c::PUMP_PROGRAM_ID));
        self.txs.insert(
            sig.into(),
            json!({"slot": slot, "blockTime": 1_791_000_000,
                   "meta": {"err": if ok { Value::Null } else { json!({"InstructionError": [0, {"Custom": 6042}]}) },
                            "logMessages": logs},
                   "transaction": {"message": {"accountKeys": [
                        {"pubkey": signer.to_string(), "signer": true, "writable": true},
                        {"pubkey": self.mint.to_string(), "signer": sig.starts_with("create"), "writable": true}]}}}),
        );
        // getSignaturesForAddress lists newest first
        self.sigs.insert(
            0,
            json!({"signature": sig, "slot": slot,
                   "err": if ok { Value::Null } else { json!({"x": 1}) }}),
        );
        if slot == S {
            self.block.push(sig.into());
        }
    }
    fn filler(&mut self, n: usize) {
        for i in 0..n {
            self.block.push(format!("other{}-{i}", self.block.len()));
        }
    }
}

/// Serve JSON-RPC over HTTP/1.1 keep-alive; returns the URL and the call counter.
fn serve(chain: Chain) -> (String, Arc<AtomicU64>, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    let calls = Arc::new(AtomicU64::new(0));
    let methods = Arc::new(Mutex::new(vec![]));
    let chain = Arc::new(chain);
    let (c2, m2) = (calls.clone(), methods.clone());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let (chain, calls, methods) = (chain.clone(), c2.clone(), m2.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut w = s;
                loop {
                    let mut len = 0usize;
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if r.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        let l = line.trim_end();
                        if l.is_empty() {
                            break;
                        }
                        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap();
                        }
                    }
                    let mut body = vec![0u8; len];
                    r.read_exact(&mut body).unwrap();
                    let req: Value = serde_json::from_slice(&body).unwrap();
                    calls.fetch_add(1, Ordering::Relaxed);
                    let m = req["method"].as_str().unwrap().to_string();
                    methods.lock().unwrap().push(m.clone());
                    let sig = req["params"][0].as_str().unwrap_or("").to_string();
                    let limited = m == "getTransaction"
                        && chain.flaky.lock().unwrap().get_mut(&sig).is_some_and(|n| {
                            let hit = *n > 0;
                            *n = n.saturating_sub(1);
                            hit
                        });
                    if limited {
                        let out = r#"{"jsonrpc":"2.0","error":{"code": 429,"message":"Too many requests for a specific RPC call"}, "id": 1 }"#;
                        let head = format!(
                            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                            out.len()
                        );
                        if w.write_all(head.as_bytes()).is_err()
                            || w.write_all(out.as_bytes()).is_err()
                        {
                            return;
                        }
                        continue;
                    }
                    let result = match m.as_str() {
                        "getSignaturesForAddress" => json!(chain.sigs),
                        "getBlock" => json!({"signatures": chain.block}),
                        "getTransaction" => chain
                            .txs
                            .get(req["params"][0].as_str().unwrap())
                            .cloned()
                            .unwrap_or(Value::Null),
                        _ => Value::Null,
                    };
                    let out =
                        json!({"jsonrpc": "2.0", "id": req["id"], "result": result}).to_string();
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                        out.len()
                    );
                    if w.write_all(head.as_bytes()).is_err() || w.write_all(out.as_bytes()).is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    (url, calls, methods)
}

fn cfg(url: &str, filter: EntryFilter) -> E1Cfg {
    E1Cfg {
        rpc_url: url.into(),
        budget: Arc::new(RpcBudget::new(1_000.0)),
        delay_ms: 0,
        max_tx: 60,
        replay_slots: 12,
        late_block_frac: 0.10,
        entry_premium_bps: 110,
        max_premium_bps: 200,
        filter,
        retry_ms: 10,
        min_replay_slots: 7,
    }
}

fn req(mint: Pubkey, dev: Pubkey) -> E1Req {
    E1Req {
        intent: 1,
        mint,
        bc: Pubkey::new_unique(),
        create_sig: [1; 64],
        slot: S,
        creator: dev,
        user: dev,
        decided_unix_ms: 0,
    }
}

fn run(c: &E1Cfg, r: E1Req, snipers: Arc<Mutex<HashSet<Pubkey>>>) -> rugsnipe::e1::E1Res {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(resolve(c, r, 1, snipers))
}

#[test]
fn lane_reconstructs_the_creation_slot_and_the_replay_tape() {
    let (mint, dev, b1, sn, x) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let create = bs58::encode([1u8; 64]).into_string();
    let mut ch = Chain::new(mint);
    ch.tx(&create, S, dev, &[1_000_000_000], true); // index 10
    ch.tx("b1", S, b1, &[1_000_000_000], true); // 11: the bundle run
    let pb = (ch.curve.virtual_sol, ch.curve.virtual_token);
    ch.filler(5);
    ch.tx("lost", S, Pubkey::new_unique(), &[], false); // a failed race (slippage)
    ch.tx("snipe", S, sn, &[300_000_000], true); // 18: behind us
    ch.filler(100);
    ch.tx("later", S + 1, x, &[200_000_000], true);
    ch.tx("dump", S + 3, dev, &[-20_000_000_000_000], true);
    let (url, calls, methods) = serve(ch);
    let snipers = Arc::new(Mutex::new(HashSet::new()));
    let r = run(
        &cfg(&url, EntryFilter::MultiBundle),
        req(mint, dev),
        snipers.clone(),
    );
    assert_eq!(r.error, None);
    assert_eq!(r.outcome, ShadowOutcome::Filled);
    let s0 = r.slot0.clone().unwrap();
    assert_eq!((s0.create_index, s0.our_index), (10, 12));
    assert_eq!(s0.bundle_wallets, vec![b1]);
    assert!(s0.creator_set.contains(&dev));
    assert_eq!((s0.tp_ahead, s0.tp_behind), (0, 1));
    assert_eq!((s0.ref_v, s0.ref_t), pb);
    assert_eq!(r.s0_failed, 1);
    assert_eq!(r.premium_bps, 110);
    // the tape: the sniper behind us in slot 0, then slots s+1 and s+3
    let users: Vec<(u64, Pubkey)> = r.tape.iter().map(|t| (t.slot, t.user)).collect();
    assert_eq!(users, vec![(S, sn), (S + 1, x), (S + 3, dev)]);
    assert_eq!(r.horizon, S + 12);
    // 1 signature list + 1 block + 3 slot-0 txs + 2 later txs
    assert_eq!(calls.load(Ordering::Relaxed), 7);
    assert_eq!(r.calls, 7);
    let m = methods.lock().unwrap().clone();
    assert_eq!(m[0], "getSignaturesForAddress");
    assert_eq!(m[1], "getBlock");
    // the sniper joins the session set
    assert!(snipers.lock().unwrap().contains(&sn));
}

/// F3: a slot-0 transaction that cannot be read (rate limited past the
/// retries) used to be dropped silently; a dropped bundle buy breaks the
/// run early (here: the bundle shrinks to the create and the launch reads
/// as `class`). Every slot-0 transaction is read, or the intent is
/// unresolved; so is a creation slot above `max_tx`.
#[test]
fn unreadable_or_truncated_creation_slot_is_unresolved() {
    let (mint, dev, b1, b2) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let create = bs58::encode([1u8; 64]).into_string();
    let chain = || {
        let mut ch = Chain::new(mint);
        ch.tx(&create, S, dev, &[1_000_000_000], true); // 10
        ch.tx("b1", S, b1, &[1_000_000_000], true); // 11
        ch.filler(1);
        ch.tx("b2", S, b2, &[1_000_000_000], true); // 13
        ch.filler(200);
        for k in 1..=8 {
            ch.tx(
                &format!("l{k}"),
                S + k,
                Pubkey::new_unique(),
                &[50_000_000],
                true,
            );
        }
        ch
    };
    let none = Arc::new(Mutex::new(HashSet::new()));
    // control: all readable -> filled behind b2
    let (url, _, _) = serve(chain());
    let r = run(
        &cfg(&url, EntryFilter::MultiBundle),
        req(mint, dev),
        none.clone(),
    );
    assert_eq!(r.outcome, ShadowOutcome::Filled, "{r:?}");
    assert_eq!(r.slot0.as_ref().unwrap().our_index, 14);
    // b1 rate limited past the four retries
    let ch = chain();
    ch.flaky.lock().unwrap().insert("b1".into(), 100);
    let (url, _, _) = serve(ch);
    let r = run(
        &cfg(&url, EntryFilter::MultiBundle),
        req(mint, dev),
        none.clone(),
    );
    assert_eq!(r.outcome, ShadowOutcome::Unresolved, "{r:?}");
    assert!(r.error.as_deref().unwrap_or("").contains("b1"), "{r:?}");
    // more slot-0 transactions than max_tx
    let (url, _, _) = serve(chain());
    let mut c = cfg(&url, EntryFilter::MultiBundle);
    c.max_tx = 2;
    let r = run(&c, req(mint, dev), none);
    assert_eq!(r.outcome, ShadowOutcome::Unresolved, "{r:?}");
    assert!(r.error.as_deref().unwrap_or("").contains("max_tx"), "{r:?}");
}

#[test]
fn lane_verdicts_class_premium_and_unresolved() {
    let (mint, dev, b1, sn) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let create = bs58::encode([1u8; 64]).into_string();
    // only the create in slot 0: class after one call
    let mut ch = Chain::new(mint);
    ch.tx(&create, S, dev, &[2_000_000_000], true);
    ch.tx("later", S + 1, b1, &[200_000_000], true);
    let (url, calls, _) = serve(ch);
    let none = Arc::new(Mutex::new(HashSet::new()));
    let r = run(
        &cfg(&url, EntryFilter::MultiBundle),
        req(mint, dev),
        none.clone(),
    );
    assert_eq!((r.outcome, r.calls), (ShadowOutcome::Class, 1));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    // a create without its own buy is not a gap-3 tape: class
    let mut ch = Chain::new(mint);
    ch.tx(&create, S, dev, &[], true);
    ch.tx("b1", S, b1, &[500_000_000], true);
    ch.filler(200);
    let (url, _, _) = serve(ch);
    let r = run(
        &cfg(&url, EntryFilter::MultiBundle),
        req(mint, dev),
        none.clone(),
    );
    assert_eq!(r.outcome, ShadowOutcome::Class, "{r:?}");
    // a known sniper inside the run: ahead of us, its impact is the premium
    let mut ch = Chain::new(mint);
    ch.tx(&create, S, dev, &[1_000_000_000], true);
    ch.tx("b1", S, b1, &[500_000_000], true);
    ch.tx("snipe", S, sn, &[1_000_000_000], true);
    ch.filler(200);
    let (url, _, _) = serve(ch);
    let known = Arc::new(Mutex::new([sn].into_iter().collect::<HashSet<_>>()));
    let r = run(&cfg(&url, EntryFilter::MultiBundle), req(mint, dev), known);
    assert_eq!(r.outcome, ShadowOutcome::Premium, "{r:?}");
    let s0 = r.slot0.unwrap();
    assert_eq!(
        (s0.tp_ahead, s0.our_index, s0.bundle_wallets.len()),
        (1, 13, 1)
    );
    assert!(r.premium_bps > 200 && r.premium_bps == s0.measured_premium_bps);
    assert!(r.tape.is_empty(), "no tape for a failed attempt");
    // the create never shows up: unresolved after the retries
    let ch = Chain::new(mint);
    let (url, _, _) = serve(ch);
    let r = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(resolve(
            &cfg(&url, EntryFilter::MultiBundle),
            req(mint, dev),
            0,
            none,
        ));
    assert_eq!(r.outcome, ShadowOutcome::Unresolved);
    assert!(r.error.unwrap().contains("not in the curve's history"));
    assert_eq!(r.calls, 3, "one signature list per try");
}

#[test]
fn rate_limits_are_retried_and_a_cut_tape_is_unresolved() {
    let (mint, dev, b1, x) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let create = bs58::encode([1u8; 64]).into_string();
    let build = |later_429: u32| {
        let mut ch = Chain::new(mint);
        ch.tx(&create, S, dev, &[1_000_000_000], true);
        ch.tx("b1", S, b1, &[1_000_000_000], true);
        ch.filler(100);
        ch.tx("later", S + 2, x, &[200_000_000], true);
        ch.flaky.lock().unwrap().insert("b1".into(), 2);
        ch.flaky.lock().unwrap().insert("later".into(), later_429);
        ch
    };
    let none = || Arc::new(Mutex::new(HashSet::new()));
    // two 429s on a slot-0 transaction and one on the tape: retried, filled
    let (url, calls, _) = serve(build(1));
    let r = run(&cfg(&url, EntryFilter::MultiBundle), req(mint, dev), none());
    assert_eq!(r.outcome, ShadowOutcome::Filled, "{r:?}");
    assert_eq!(r.tape.len(), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 1 + 1 + 2 + 2 + 1 + 1);
    // the tape transaction never answers: the tape stops at s+1 < s+7
    let (url, _, _) = serve(build(100));
    let r = run(&cfg(&url, EntryFilter::MultiBundle), req(mint, dev), none());
    assert_eq!(r.outcome, ShadowOutcome::Unresolved);
    assert_eq!(r.horizon, S + 1);
    assert!(r.error.unwrap().contains("replay tape cut"));
    assert!(r.slot0.is_some(), "the creation slot is still recorded");
}
