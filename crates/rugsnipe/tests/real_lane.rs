//! The real fill lane (probe) against a mock JSON-RPC endpoint, on recorded
//! transaction shapes: a landed buy with our own TradeEvent (rank in its
//! slot, premium over the post-bundle price), a landed-but-failed buy (cost
//! = `meta.fee`, error 6042 = slippage), a dropped send (the nonce moved on,
//! none of our signatures landed: cost 0), a landed sell (our sell
//! TradeEvent), a held mint's balance, the wallet and the blockhash.

mod common;

use common::*;
use pump_codec::curve::CurveState;
use pump_codec::{pda, Pubkey};
use rugsnipe::real::{self, LaneCfg, LaneCmd, LaneEvent, Pending, SentVar};
use rugsnipe_state::RpcBudget;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const S: u64 = 1_000;

fn sig(n: u8) -> [u8; 64] {
    [n; 64]
}
fn b58(s: &[u8]) -> String {
    bs58::encode(s).into_string()
}

struct Fixture {
    owner: Pubkey,
    mint: Pubkey,
    dev: Pubkey,
    txs: HashMap<String, Value>,
    /// getSignaturesForAddress (newest first)
    sigs: Vec<Value>,
    block: Vec<String>,
    statuses: HashMap<String, Value>,
    accounts: HashMap<String, Vec<u8>>,
    expect_premium: u32,
    our_tokens: u64,
}

fn fixture() -> Fixture {
    let owner = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (dev, bw, x, y) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let mut c = CurveState::fresh(dev);
    let mut txs = HashMap::new();
    let mut sigs = vec![];
    let mut block: Vec<String> = (0..30).map(|i| format!("vote{i}")).collect();
    let buy = |c: &mut CurveState, user: Pubkey, sol: u64| -> (String, u64) {
        let tok = c.tokens_for_sol(sol);
        c.apply_buy(tok, sol);
        (trade_log_line(&mint, &user, true, sol, tok, c), tok)
    };
    let mut add = |name: String, ix: usize, json: Value, ok: bool| {
        block[ix] = name.clone();
        sigs.insert(
            0,
            json!({"signature": name, "slot": S, "err": if ok { Value::Null } else { json!({"x": 1}) }}),
        );
        txs.insert(name, json);
    };
    // create (dev buys 1 SOL) at 5, a bundle wallet at 6, a sniper at 12
    let (l, _) = buy(&mut c, dev, 1_000_000_000);
    add(
        b58(&sig(1)),
        5,
        tx_json(S, &[dev, mint], &[l], Value::Null, 5_000),
        true,
    );
    let (l, _) = buy(&mut c, bw, 500_000_000);
    add(
        b58(&sig(2)),
        6,
        tx_json(S, &[bw], &[l], Value::Null, 5_000),
        true,
    );
    let ref_t = c.virtual_token;
    let (l, _) = buy(&mut c, x, 300_000_000);
    add(
        b58(&sig(3)),
        12,
        tx_json(S, &[x], &[l], Value::Null, 5_000),
        true,
    );
    // ours at 14 (0.02 SOL)
    let t_pre = c.virtual_token;
    let (l, our_tokens) = buy(&mut c, owner, 20_000_000);
    add(
        b58(&sig(10)),
        14,
        tx_json(S, &[owner], &[l], Value::Null, 15_000),
        true,
    );
    // our second attempt failed at 16 (slippage), someone behind at 20
    add(
        b58(&sig(11)),
        16,
        tx_json(
            S,
            &[owner],
            &[],
            json!({"InstructionError": [3, {"Custom": 6042}]}),
            1_005_000,
        ),
        false,
    );
    let (l, _) = buy(&mut c, y, 100_000_000);
    add(
        b58(&sig(4)),
        20,
        tx_json(S, &[y], &[l], Value::Null, 5_000),
        true,
    );
    // our sell of the position, in a later slot
    let out = c.sell_sol_out(our_tokens);
    c.apply_sell(our_tokens, out);
    let sell = trade_log_line(&mint, &owner, false, out, our_tokens, &c);
    txs.insert(
        b58(&sig(20)),
        tx_json(S + 3, &[owner], &[sell], Value::Null, 15_000),
    );
    let mut statuses = HashMap::new();
    statuses.insert(
        b58(&sig(10)),
        json!({"slot": S, "confirmations": 1, "err": null, "confirmationStatus": "confirmed"}),
    );
    statuses.insert(
        b58(&sig(11)),
        json!({"slot": S, "confirmations": 1, "err": {"InstructionError": [3, {"Custom": 6042}]},
               "confirmationStatus": "confirmed"}),
    );
    statuses.insert(
        b58(&sig(20)),
        json!({"slot": S + 3, "confirmations": 1, "err": null, "confirmationStatus": "confirmed"}),
    );
    Fixture {
        owner,
        mint,
        dev,
        txs,
        sigs,
        block,
        statuses,
        accounts: HashMap::new(),
        expect_premium: real::premium_bps(ref_t, t_pre).unwrap(),
        our_tokens,
    }
}

fn serve(f: Arc<Mutex<Fixture>>) -> (String, Arc<Mutex<Vec<String>>>) {
    serve_rpc(move |method, params| {
        let f = f.lock().unwrap();
        Ok(match method {
            "getSignatureStatuses" => {
                let list: Vec<Value> = params[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| {
                        f.statuses
                            .get(s.as_str().unwrap())
                            .cloned()
                            .unwrap_or(Value::Null)
                    })
                    .collect();
                json!({"context": {"slot": S + 5}, "value": list})
            }
            "getTransaction" => f
                .txs
                .get(params[0].as_str().unwrap())
                .cloned()
                .unwrap_or(Value::Null),
            "getSignaturesForAddress" => Value::Array(f.sigs.clone()),
            "getBlock" => {
                assert_eq!(params[1]["transactionDetails"], "signatures");
                json!({"signatures": f.block})
            }
            "getMultipleAccounts" => {
                use base64::Engine as _;
                let v: Vec<Value> = params[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| match f.accounts.get(k.as_str().unwrap()) {
                        Some(d) => json!({"data": [base64::engine::general_purpose::STANDARD.encode(d), "base64"],
                                          "lamports": 1, "owner": "11111111111111111111111111111111"}),
                        None => Value::Null,
                    })
                    .collect();
                json!({"context": {"slot": S + 6}, "value": v})
            }
            "getBalance" => json!({"context": {"slot": S}, "value": 1_234_000_000u64}),
            "getLatestBlockhash" => {
                json!({"context": {"slot": S + 2}, "value": {"blockhash": b58(&[0x42; 32]), "lastValidBlockHeight": 1}})
            }
            other => return Err(format!("unexpected {other}")),
        })
    })
}

#[test]
fn landed_failed_and_dropped_attempts_sells_and_balances() {
    let fx = fixture();
    let (owner, mint, dev, our_tokens, premium) =
        (fx.owner, fx.mint, fx.dev, fx.our_tokens, fx.expect_premium);
    let bc = pda::bonding_curve(&mint);
    let nonce = |i: u8| Pubkey::new_from_array([0x90 + i; 32]);
    let f = Arc::new(Mutex::new(fx));
    {
        // nonces 1 and 2 still hold the values our variants carry; nonce 3 moved on
        let mut g = f.lock().unwrap();
        g.accounts
            .insert(nonce(1).to_string(), nonce_account_data(&owner, [1; 32]));
        g.accounts
            .insert(nonce(2).to_string(), nonce_account_data(&owner, [2; 32]));
        g.accounts
            .insert(nonce(3).to_string(), nonce_account_data(&owner, [0x33; 32]));
        let mut ata = vec![0u8; 165];
        ata[64..72].copy_from_slice(&777u64.to_le_bytes());
        g.accounts
            .insert(pda::ata_token_2022(&owner, &mint).to_string(), ata);
    }
    let (url, log) = serve(f.clone());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
    let (etx, mut erx) = tokio::sync::mpsc::unbounded_channel();
    let cfg = LaneCfg {
        rpc_url: url,
        budget: Arc::new(RpcBudget::new(200.0)),
        owner,
        dry: true,
    };
    let pending = |intent: u64, n: u8| Pending {
        intent,
        mint,
        bc,
        create_sig: sig(1),
        slot: S,
        creator: dev,
        user: dev,
        nonce_idx: n as u16,
        nonce_account: nonce(n),
        nonce_value: [n; 32],
        spend: 20_000_000,
    };
    let var = |s: u8, variant: u8, label: &'static str| SentVar {
        sig: sig(s),
        variant,
        label,
        prio: 10_000,
        tip: 1_000_000,
    };
    for (intent, n, s, v, label) in [
        (1u64, 1u8, 10u8, 0u8, "helius"),
        (2, 2, 11, 1, "nozomi"),
        (3, 3, 12, 0, "jito_frankfurt"),
    ] {
        ctx.send(LaneCmd::Track(pending(intent, n))).unwrap();
        ctx.send(LaneCmd::Sent {
            intent,
            var: var(s, v, label),
        })
        .unwrap();
    }
    ctx.send(LaneCmd::SellSent {
        mint,
        sig: sig(20),
        slot: S + 2,
        tip: 3_400_000,
    })
    .unwrap();
    ctx.send(LaneCmd::Held { intent: 1, mint }).unwrap();
    ctx.send(LaneCmd::Slot(S + 2)).unwrap();
    let snipers = Arc::new(Mutex::new(HashSet::new()));
    let evs = rt.block_on(async move {
        let task = tokio::spawn(real::run(cfg, crx, etx, snipers));
        let mut evs = vec![];
        let t0 = Instant::now();
        let done = |evs: &Vec<LaneEvent>| {
            let n = |f: &dyn Fn(&LaneEvent) -> bool| evs.iter().filter(|e| f(e)).count();
            n(&|e| matches!(e, LaneEvent::Landed(_))) == 2
                && n(&|e| matches!(e, LaneEvent::Dropped { .. })) == 1
                && n(&|e| matches!(e, LaneEvent::OwnTrade { .. })) == 1
                && n(&|e| matches!(e, LaneEvent::Balance { .. })) >= 1
                && n(&|e| matches!(e, LaneEvent::Wallet { .. })) >= 1
        };
        while !done(&evs) {
            assert!(t0.elapsed() < Duration::from_secs(20), "{evs:#?}");
            if let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(100), erx.recv()).await
            {
                evs.push(e);
            }
        }
        drop(ctx);
        let _ = task.await;
        evs
    });
    // 1. landed and succeeded: our TradeEvent, rank and premium
    let landed: Vec<&real::Landed> = evs
        .iter()
        .filter_map(|e| match e {
            LaneEvent::Landed(l) => Some(l),
            _ => None,
        })
        .collect();
    let ok = landed.iter().find(|l| l.intent == 1).unwrap();
    assert_eq!(ok.fail, None);
    assert_eq!(ok.fee, 15_000);
    let t = ok.trade.expect("our TradeEvent");
    assert!(t.is_buy && t.sol == 20_000_000 && t.tokens == our_tokens);
    assert_eq!(ok.var.unwrap().label, "helius");
    assert_eq!(ok.rank.tx_index, Some(14));
    assert_eq!(
        (ok.rank.tp_ahead, ok.rank.tp_behind),
        (1, 1),
        "{:?}",
        ok.rank
    );
    assert_eq!(ok.rank.rival_sol_ahead, 300_000_000);
    assert_eq!(ok.rank.premium_bps, Some(premium), "{:?}", ok.rank);
    assert!(premium > 0);
    assert_eq!(ok.rank.info.create_index, Some(5));
    assert_eq!(ok.rank.info.bundle_wallets, 1);
    assert_eq!(
        ok.rank.info.s0_failed, 1,
        "our failed attempt is a slot-0 failure"
    );
    assert!(!ok.late);
    // 2. landed and failed: meta.fee is the cost, 6042 is slippage
    let bad = landed.iter().find(|l| l.intent == 2).unwrap();
    assert_eq!(bad.fail.as_ref().unwrap().0, "slippage");
    assert_eq!(bad.fee, 1_005_000);
    assert_eq!(bad.trade, None);
    assert_eq!(bad.rank.tx_index, Some(16));
    assert_eq!(bad.var.unwrap().variant, 1);
    // 3. dropped: the nonce moved on and none of ours landed
    assert!(evs.iter().any(|e| matches!(e,
        LaneEvent::Dropped { intent: 3, primary: Some(v), burned: false } if v.label == "jito_frankfurt")));
    assert!(evs
        .iter()
        .any(|e| matches!(e, LaneEvent::Nonce { idx: 3, value, .. } if *value == [0x33; 32])));
    assert!(!evs
        .iter()
        .any(|e| matches!(e, LaneEvent::Nonce { idx: 1 | 2, .. })));
    // 4. the sell landed: our sell TradeEvent; the held mint's balance
    assert!(evs.iter().any(|e| matches!(e,
        LaneEvent::OwnTrade { trade, mint: m, .. } if !trade.is_buy && trade.tokens == our_tokens && *m == mint)));
    assert!(evs
        .iter()
        .any(|e| matches!(e, LaneEvent::Balance { tokens: 777, .. })));
    assert!(evs.iter().any(|e| matches!(
        e,
        LaneEvent::Wallet {
            lamports: 1_234_000_000
        }
    )));
    assert!(evs.iter().any(|e| matches!(e, LaneEvent::Blockhash { hash, slot } if *hash == [0x42; 32] && *slot == S + 2)));
    // read-only: no transaction was ever sent
    assert!(!log.lock().unwrap().iter().any(|m| m == "sendTransaction"));
}

#[test]
fn a_burn_is_signed_on_the_nonce_value_and_sent_unless_dry() {
    let sent = Arc::new(Mutex::new(vec![]));
    let s2 = sent.clone();
    let (url, _log) = serve_rpc(move |method, params| match method {
        "sendTransaction" => {
            s2.lock()
                .unwrap()
                .push(params[0].as_str().unwrap().to_string());
            Ok(json!("sig"))
        }
        "getBalance" => Ok(json!({"value": 1})),
        "getLatestBlockhash" => {
            Ok(json!({"context": {"slot": 1}, "value": {"blockhash": b58(&[1; 32])}}))
        }
        _ => Ok(Value::Null),
    });
    let signer = Arc::new(rugsnipe_tx::signer::HotSigner::from_seed(
        &zeroize::Zeroizing::new([9; 32]),
    ));
    for dry in [true, false] {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (ctx, crx) = tokio::sync::mpsc::unbounded_channel();
        let (etx, mut erx) = tokio::sync::mpsc::unbounded_channel();
        let cfg = LaneCfg {
            rpc_url: url.clone(),
            budget: Arc::new(RpcBudget::new(100.0)),
            owner: signer.pubkey(),
            dry,
        };
        let account = rugsnipe_tx::sysix::pool_addresses(&signer.pubkey(), "buy", 1)[0];
        ctx.send(LaneCmd::Burn {
            idx: 0,
            account,
            value: [7; 32],
            signer: signer.clone(),
        })
        .unwrap();
        let ev = rt.block_on(async move {
            let task = tokio::spawn(real::run(cfg, crx, etx, Arc::default()));
            let t0 = Instant::now();
            loop {
                assert!(t0.elapsed() < Duration::from_secs(5));
                if let Ok(Some(e @ LaneEvent::BurnSent { .. })) =
                    tokio::time::timeout(Duration::from_millis(100), erx.recv()).await
                {
                    drop(ctx);
                    let _ = task.await;
                    return e;
                }
            }
        });
        let LaneEvent::BurnSent { idx, sig, ok, err } = ev else {
            unreachable!()
        };
        assert_eq!(idx, 0);
        assert_eq!(ok, !dry, "{err}");
        let (want, want_sig) = rugsnipe_tx::sysix::burn_tx(&signer, &account, &[7; 32]).unwrap();
        assert_eq!(sig, want_sig);
        let got = sent.lock().unwrap().clone();
        if dry {
            assert!(got.is_empty(), "a dry run never sends");
        } else {
            use base64::Engine as _;
            assert_eq!(got.len(), 1);
            assert_eq!(
                base64::engine::general_purpose::STANDARD
                    .decode(&got[0])
                    .unwrap(),
                want
            );
        }
    }
}
