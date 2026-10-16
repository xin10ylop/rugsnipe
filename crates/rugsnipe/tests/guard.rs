//! rugsnipe-guard skeleton: idles while the hot box renews the exits lease,
//! takes over a stale lease when a position is open, sells (dry run or paper).

mod common;

use common::tmpdir;
use pump_codec::Pubkey;
use rugsnipe::guard::{Guard, GuardAction, GuardCfg, PaperSell};
use rugsnipe_state::lease::Lease;
use rugsnipe_state::rows::*;
use rugsnipe_state::{Ledger, StateStore};
use std::sync::Arc;

fn position(mint: Pubkey, peak: u64) -> PositionRow {
    PositionRow {
        mint,
        intent_id: 5,
        opened_slot: 1,
        tokens: 1_000_000_000_000,
        cost: 100_000_000,
        v_fill: 32_000_000_000,
        peak_v: peak,
        bundle_json: "[]".into(),
        policy_json: "{}".into(),
        origin: Origin::Intent,
        state: PosState::Open,
        sell_spend: 0,
        closed_slot: None,
    }
}

fn cfg(dry_run: bool, grace_s: u64) -> GuardCfg {
    GuardCfg {
        owner: "guard:test".into(),
        takeover_after_ms: 5_000,
        drop_from_peak_bps: 3_000,
        grace_s,
        dry_run,
    }
}

#[test]
fn guard_idles_while_hot_renews_then_takes_over_and_sells() {
    let dir = tmpdir("guard");
    let s = StateStore::open(dir.join("state.db")).unwrap();
    let l = Ledger::new(s.writer());
    let c = s.reader().unwrap();
    let mut g = Guard::new(cfg(true, 60), None);
    let none = |_: &Pubkey| None;
    assert_eq!(
        g.step(&c, 1_000, &none).unwrap(),
        vec![GuardAction::Idle("no open positions")]
    );
    let (m1, m2) = (Pubkey::new_unique(), Pubkey::new_unique());
    l.position(position(m1, 40_000_000_000));
    l.position(position(m2, 40_000_000_000));
    l.flush();
    // the hot box holds a fresh lease
    let hc = s.reader().unwrap();
    let mut hot = Lease::new("exits", "hot:test");
    hot.acquire(&hc, 10_000, 5_000).unwrap();
    assert!(matches!(
        g.step(&c, 12_000, &none).unwrap()[..],
        [GuardAction::HotAlive { .. }]
    ));
    // hot stops renewing: > 5 s later the guard takes over
    let price = move |m: &Pubkey| {
        if *m == m1 {
            Some(33_000_000_000)
        } else {
            Some(39_000_000_000)
        }
    };
    let acts = g.step(&c, 15_500, &price).unwrap();
    assert!(
        matches!(&acts[0], GuardAction::TookOver { epoch: 2, from: Some(f) } if f == "hot:test"),
        "{acts:?}"
    );
    // m1 dropped (33/40)^2 = 68% of peak (>= 30% drop) -> sells now; m2 waits for grace
    assert!(
        matches!(&acts[1], GuardAction::WouldSell { mint, reason: "drop_from_peak", .. } if *mint == m1),
        "{acts:?}"
    );
    assert_eq!(acts.len(), 2);
    assert!(
        !hot.renew(&hc, 15_600).unwrap(),
        "the hot box lost the lease"
    );
    let acts = g.step(&c, 15_500 + 60_000, &price).unwrap();
    assert!(
        matches!(&acts[..], [GuardAction::WouldSell { mint, reason: "grace", .. }] if *mint == m2),
        "{acts:?}"
    );
    // positions closed -> lease released
    let mut closed = position(m1, 0);
    closed.state = PosState::Closed;
    l.position(closed.clone());
    closed.mint = m2;
    l.position(closed);
    l.flush();
    assert_eq!(
        g.step(&c, 80_000, &price).unwrap(),
        vec![GuardAction::Released]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn guard_paper_sell_signs_with_the_paper_sender_only() {
    let dir = tmpdir("guard-paper");
    let s = StateStore::open(dir.join("state.db")).unwrap();
    let l = Ledger::new(s.writer());
    let c = s.reader().unwrap();
    let m = Pubkey::new_unique();
    l.position(position(m, 40_000_000_000));
    l.flush();
    let signer = Arc::new(rugsnipe::setup::throwaway_signer().unwrap());
    let cfgt = rugsnipe_core::config::Config::default();
    let pool = rugsnipe::setup::paper_nonce_pool(signer.pubkey(), 1);
    let t = rugsnipe::setup::templates(&cfgt, signer.pubkey(), &pool).unwrap();
    let paper = Arc::new(rugsnipe_tx::senders::paper::Paper::new(16));
    let mut g = Guard::new(
        cfg(false, 0),
        Some(PaperSell {
            template: t.sell_final,
            signer: signer.clone(),
            paper: paper.clone(),
            tip_account: t.tip_account,
            bid: rugsnipe_core::bid::BidPolicy::from_cfg(&cfgt.bid),
            real: None,
        }),
    );
    let acts = g.step(&c, 1_000, &|_| None).unwrap();
    assert!(matches!(
        &acts[0],
        GuardAction::TookOver {
            epoch: 1,
            from: None
        }
    ));
    let GuardAction::Sold {
        sig,
        reason: "grace",
        ..
    } = acts[1]
    else {
        panic!("{acts:?}")
    };
    let rec = paper.records();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].variant.sig, sig);
    // the dump bid is paid once, through the bid policy: the leader is
    // unknown, so it is the priority fee and no tip (docs/03 section 6.1)
    let v = &rec[0].variant;
    assert_eq!(v.tip_lamports, 0, "{v:?}");
    assert!(
        v.priority_lamports >= cfgt.bid.min_priority_lamports,
        "{v:?}"
    );
    assert!(rugsnipe_tx::signer::verify(
        &signer.pubkey(),
        &rec[0].variant.bytes()[65..],
        &sig
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Probe: the guard's dump path through the real senders (a mock Helius):
/// one variant per sender on a recent blockhash, the provider's tip account
/// and minimum tip, signed by the wallet; deferred while no blockhash is known.
#[cfg(feature = "real-senders")]
#[test]
fn guard_real_sell_goes_through_the_real_senders_on_a_recent_blockhash() {
    use std::sync::Mutex;
    let dir = tmpdir("guard-real");
    let s = StateStore::open(dir.join("state.db")).unwrap();
    let l = Ledger::new(s.writer());
    let c = s.reader().unwrap();
    let m = Pubkey::new_unique();
    l.position(position(m, 40_000_000_000));
    l.flush();
    let got: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let g2 = got.clone();
    let (url, _) = common::serve_rpc(move |_m, p| {
        use base64::Engine as _;
        g2.lock().unwrap().push(
            base64::engine::general_purpose::STANDARD
                .decode(p[0].as_str().unwrap())
                .unwrap(),
        );
        Ok(serde_json::json!("ok"))
    });
    let mut cfgt = rugsnipe_core::config::Config::default();
    rugsnipe::setup::paper_caps(&mut cfgt.risk);
    cfgt.mode.mode = rugsnipe_core::Mode::Probe;
    cfgt.tx.cu_limit = 88_000;
    cfgt.tx.lads_bytes = 12_124_160;
    cfgt.secrets.probe_keypair = "probe-keypair".into();
    cfgt.secrets.armset_pubkey = Pubkey::new_unique().to_string();
    cfgt.secrets.cold_wallet = Pubkey::new_unique().to_string();
    cfgt.senders.rpcsend.enabled = false;
    cfgt.senders.helius.enabled = true;
    cfgt.senders.helius.url = format!("{url}fast");
    cfgt.senders.helius.max_rps = 10;
    let mut env = rugsnipe_core::config::ValidateEnv {
        clock_offset_ms: Some(0.1),
        post_exec_feed_healthy: true,
        armset: rugsnipe_core::config::ArmSetGate::Verified {
            gate_passed: false,
            probe_allowed: true,
        },
        ..Default::default()
    };
    env.credential_modes
        .insert("probe-keypair".into(), Some(0o400));
    let senders =
        Arc::new(rugsnipe_tx::senders::SenderSet::build(&cfgt, &env, &Default::default()).unwrap());
    let signer = Arc::new(rugsnipe::setup::throwaway_signer().unwrap());
    let pool = rugsnipe::setup::paper_nonce_pool(signer.pubkey(), 1);
    let t = rugsnipe::setup::templates(&cfgt, signer.pubkey(), &pool).unwrap();
    let hash: Arc<Mutex<Option<[u8; 32]>>> = Arc::default();
    let h2 = hash.clone();
    let mut g = Guard::new(
        cfg(false, 0),
        Some(PaperSell {
            template: t.sell_final,
            signer: signer.clone(),
            paper: Arc::new(rugsnipe_tx::senders::paper::Paper::new(16)),
            tip_account: t.tip_account,
            bid: rugsnipe_core::bid::BidPolicy::from_cfg(&cfgt.bid),
            real: Some(rugsnipe::guard::GuardReal {
                senders,
                blockhash: Arc::new(move || *h2.lock().unwrap()),
            }),
        }),
    );
    let acts = g.step(&c, 1_000, &|_| None).unwrap();
    assert!(
        matches!(&acts[1], GuardAction::SellDeferred { mint, .. } if *mint == m),
        "{acts:?}"
    );
    assert!(got.lock().unwrap().is_empty());
    *hash.lock().unwrap() = Some([7; 32]);
    let acts = g.step(&c, 2_000, &|_| None).unwrap();
    assert!(
        matches!(
            &acts[..],
            [GuardAction::SentReal {
                sent: 1,
                accepted: 1,
                ..
            }]
        ),
        "{acts:?}"
    );
    let txs = got.lock().unwrap().clone();
    assert_eq!(txs.len(), 1);
    let view = pump_codec::tx::TxView::parse(&txs[0]).unwrap();
    assert_eq!(view.recent_blockhash(), &[7u8; 32]);
    assert!(view
        .static_account_keys()
        .iter()
        .any(|k| rugsnipe_tx::senders::providers::HELIUS_TIP_ACCOUNTS.contains(k)));
    assert!(rugsnipe_tx::signer::verify(
        &signer.pubkey(),
        &txs[0][65..],
        txs[0][1..65].try_into().unwrap()
    ));
    let _ = std::fs::remove_dir_all(&dir);
}
