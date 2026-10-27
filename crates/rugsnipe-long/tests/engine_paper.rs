//! The engine in paper mode on wallet 4's real wSOL multi-hop buy: the
//! trigger, the paper fill once the landing slot is over, and each exit.

use rugsnipe_long::chain::Rpc;
use rugsnipe_long::config::{LongCfg, Mode};
use rugsnipe_long::engine::Engine;
use rugsnipe_long::feed::Src;
use rugsnipe_long::oracle::{now_ms, Oracle};
use rugsnipe_long::pos::{Reason, State};
use rugsnipe_long::txlite::TxLite;

fn fx(name: &str) -> TxLite {
    let p = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    TxLite::from_rpc_json(&v).unwrap()
}

fn engine(tag: &str) -> Engine {
    let dir = std::env::temp_dir().join(format!("long-engine-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = LongCfg {
        mode: Mode::Log,
        state_dir: dir.to_string_lossy().into(),
        ..Default::default()
    };
    // a copied route from wrapped SOL needs no chain read
    Engine::new(
        cfg,
        Default::default(),
        None,
        Rpc::new("http://127.0.0.1:9"),
        Oracle::default(),
        None,
        None,
    )
    .unwrap()
}

/// Move position 0's curve to `px` times its reference price (constant
/// product: both reserves move, as a trade event reports them).
fn price_to(e: &mut Engine, px: f64) {
    let p = &mut e.book.positions[0];
    let k = p.vt_ref as f64 * p.vq_ref as f64;
    p.vt = (p.vt_ref as f64 / px.sqrt()) as u64;
    p.vq = (k / p.vt as f64) as u64;
}

fn opened(tag: &str) -> Engine {
    let mut e = engine(tag);
    let tx = fx("w4_buy_wsol_multihop");
    e.on_tx(Src::Copied, &tx);
    assert_eq!(e.book.positions.len(), 1, "a paper position for its buy");
    let p = &e.book.positions[0];
    assert!(p.paper && p.state == State::Buying && p.how == "copied");
    // the landing slot is not over: no fill yet
    e.on_slot(tx.slot + 1);
    e.on_tick(now_ms());
    assert_eq!(e.book.positions[0].state, State::Buying);
    e.on_slot(tx.slot + 2);
    e.on_tick(now_ms());
    let p = &e.book.positions[0];
    assert_eq!(p.state, State::Open);
    assert!(p.tokens > 0);
    e
}

#[test]
fn take_profit_closes_with_a_gain() {
    let mut e = opened("tp");
    price_to(&mut e, 2.1);
    e.on_tick(now_ms());
    let p = &e.book.positions[0];
    assert_eq!(p.state, State::Closed);
    assert_eq!(p.reason, Some(Reason::Tp));
    // 2.1x its price, less our own impact both ways and the fees
    assert!(
        p.net_lamports > 700_000_000 && p.net_lamports < 1_100_000_000,
        "{}",
        p.net_lamports
    );
    // the closed record went to closed.jsonl and the book file
    let dir = std::path::PathBuf::from(&e.cfg.state_dir);
    assert!(std::fs::read_to_string(dir.join("closed.jsonl"))
        .unwrap()
        .contains("\"tp\""));
    assert!(std::fs::read_to_string(dir.join("positions.json"))
        .unwrap()
        .contains("closed"));
}

#[test]
fn stop_closes_with_a_loss_and_time_after_the_hold() {
    let mut e = opened("stop");
    price_to(&mut e, 0.75);
    e.on_tick(now_ms());
    let p = &e.book.positions[0];
    assert_eq!(p.reason, Some(Reason::Stop));
    assert!(
        p.net_lamports < -200_000_000 && p.net_lamports > -400_000_000,
        "{}",
        p.net_lamports
    );

    let mut e = opened("time");
    let seen = e.book.positions[0].seen_ms;
    e.on_tick(seen + 1_789_000);
    assert_eq!(e.book.positions[0].state, State::Open);
    e.on_tick(seen + 1_790_000);
    assert_eq!(e.book.positions[0].reason, Some(Reason::Time));
}

#[test]
fn its_second_buy_and_max_open_and_kill_are_skipped() {
    let mut e = opened("skips");
    // the same transaction again: its second buy of the coin, not a second position
    e.on_tx(Src::Copied, &fx("w4_buy_wsol_multihop"));
    assert_eq!(e.book.positions.len(), 1);
    // KILL: no new buys
    let mut e = engine("kill");
    std::fs::create_dir_all(&e.cfg.state_dir).unwrap();
    std::fs::write(std::path::Path::new(&e.cfg.state_dir).join("KILL"), b"").unwrap();
    e.on_tx(Src::Copied, &fx("w4_buy_wsol_multihop"));
    assert!(e.book.positions.is_empty());
    // max_open reached
    let mut e = engine("max");
    e.cfg.max_open = 1;
    e.on_tx(Src::Copied, &fx("w4_buy_wsol_multihop"));
    e.on_tx(Src::Copied, &fx("w4_buy_wbtc_multihop"));
    assert_eq!(e.book.positions.len(), 1);
}

#[test]
fn a_restart_keeps_the_open_position() {
    let e = opened("restart");
    let dir = e.cfg.state_dir.clone();
    drop(e);
    let cfg = LongCfg {
        mode: Mode::Log,
        state_dir: dir,
        ..Default::default()
    };
    let e2 = Engine::new(
        cfg,
        Default::default(),
        None,
        Rpc::new("http://127.0.0.1:9"),
        Oracle::default(),
        None,
        None,
    )
    .unwrap();
    assert_eq!(e2.book.positions.len(), 1);
    assert_eq!(e2.book.positions[0].state, State::Open);
    assert!(
        e2.book.positions[0].buy_route().is_some(),
        "the stored route reads back"
    );
}

/// Our own transaction on position 0 (the engine's wallet is the default
/// key): signature `sig`, landed failed or not, fee, and the coin balance
/// change.
fn ours(e: &Engine, sig: u8, err: bool, fee: u64, coin_pre: u64, coin_post: u64) -> TxLite {
    let me = pump_codec::Pubkey::default();
    let mint = e.book.positions[0].mint_pk();
    TxLite {
        sig: vec![sig; 64],
        slot: 1,
        err,
        fee,
        keys: vec![me],
        outer: vec![],
        inner: vec![],
        pre_lamports: vec![1_000_000_000],
        post_lamports: vec![1_000_000_000 - fee],
        tokens: vec![rugsnipe_long::txlite::TokBal {
            index: 1,
            mint,
            owner: me,
            program: pump_codec::constants::TOKEN_2022_PROGRAM_ID,
            pre: coin_pre,
            post: coin_post,
        }],
    }
}

fn b58(sig: u8) -> String {
    bs58::encode(vec![sig; 64]).into_string()
}

/// 2026-10-27, the hot bot's 2RR6sfbi: a sell that kept failing was re-sent
/// 405 times. Here failed sells go again at once three times (the exit
/// matters and the floor drops each time), then wait 5 s, 30 s and 2 min, and
/// past `max_sell_fail_sol` of failed fees every 10 min; never given up.
#[test]
fn a_sell_that_keeps_failing_backs_off_instead_of_spending() {
    let mut e = opened("sellfail");
    e.cfg.mode = Mode::On;
    e.cfg.max_sell_fail_sol = 0.001;
    {
        let p = &mut e.book.positions[0];
        p.paper = false;
        p.tokens = 5_000;
    }
    price_to(&mut e, 2.5);
    let t0 = now_ms();
    for k in 1..=3u8 {
        let p = &mut e.book.positions[0];
        p.state = State::Selling;
        p.sell_sigs.push(b58(k));
        let tx = ours(&e, k, true, 300_005, 5_000, 5_000);
        e.on_tx(Src::Ours, &tx);
        let p = &e.book.positions[0];
        assert_eq!((p.state, p.sell_tries), (State::Open, k as u32));
    }
    let p = &e.book.positions[0];
    assert_eq!(p.costs, 3 * 300_005);
    let wait = p.sell_next_ms.saturating_sub(t0);
    assert!(
        (4_000..=6_000).contains(&wait),
        "the 4th try waits 5 s: {wait}"
    );
    // within the wait nothing is sent (an attempt here would fail to send and count a try)
    e.on_tick(t0 + 1_000);
    assert_eq!(e.book.positions[0].sell_tries, 3);
    // past it the exit is tried again (no wallet here: it cannot be sent, and backs off)
    e.on_tick(t0 + 6_000);
    let p = &e.book.positions[0];
    assert_eq!(p.sell_tries, 4);
    assert!(p.sell_next_ms > t0 + 6_000);
    // one more failure crosses the 0.001 SOL cap: every 10 min from now
    let p = &mut e.book.positions[0];
    p.state = State::Selling;
    p.sell_sigs.push(b58(9));
    let tx = ours(&e, 9, true, 300_005, 5_000, 5_000);
    e.on_tx(Src::Ours, &tx);
    let p = &e.book.positions[0];
    assert!(p.costs >= 1_000_000);
    assert!(p.sell_next_ms >= now_ms() + 590_000, "10 min after the cap");
    assert!(p.live(), "never given up: the coins are still ours to sell");
}

/// A buy not seen within 90 s is given up as dropped; if it lands after all,
/// the position reopens on the tokens it bought instead of leaving them in
/// the wallet unmanaged.
#[test]
fn a_dropped_buy_that_lands_late_reopens() {
    let mut e = opened("latebuy");
    e.cfg.mode = Mode::On;
    {
        let p = &mut e.book.positions[0];
        p.paper = false;
        p.tokens = 0;
        p.state = State::Failed;
        p.dropped = true;
        p.closed_ms = now_ms();
        p.buy_sig = Some(b58(42));
    }
    let tx = ours(&e, 42, false, 1_005_000, 0, 7_777);
    e.on_tx(Src::Ours, &tx);
    let p = &e.book.positions[0];
    assert_eq!(p.state, State::Open);
    assert_eq!(p.tokens, 7_777);
    assert!(!p.dropped && p.live());
    // a buy that landed and failed stays failed (its signature cannot land twice)
    let mut e = opened("failedbuy");
    e.cfg.mode = Mode::On;
    {
        let p = &mut e.book.positions[0];
        p.paper = false;
        p.tokens = 0;
        p.state = State::Failed;
        p.dropped = false;
        p.buy_sig = Some(b58(43));
    }
    let tx = ours(&e, 43, false, 1_005_000, 0, 7_777);
    e.on_tx(Src::Ours, &tx);
    assert_eq!(e.book.positions[0].state, State::Failed);
}

/// 2026-10-27, CkFH2smc: the wallet sold after 14 minutes and the paper copy
/// held to 30 (-18.4%). The backtest's price path ends at its sale, so its
/// copies sell right behind it; the engine does the same.
#[test]
fn its_early_sell_takes_us_out_behind_it() {
    use rugsnipe_long::detect::{self, Seen};
    let mut e = opened("itssell");
    let sell = fx("w4_sell_wbtc_multihop");
    let w4: pump_codec::Pubkey = e.cfg.copy_wallet.parse().unwrap();
    let mint = match detect::read(&sell, &w4, &|_| None).as_slice() {
        [Seen::Sell { mint, .. }] => *mint,
        s => panic!("{s:?}"),
    };
    // our open position is on the coin it sells
    e.book.positions[0].mint = mint.to_string();
    e.on_tx(Src::Copied, &sell);
    assert_eq!(e.book.positions[0].reason, Some(Reason::ItsSell));
    e.on_tick(now_ms());
    let p = &e.book.positions[0];
    assert_eq!((p.state, p.reason), (State::Closed, Some(Reason::ItsSell)));
    let dir = std::path::PathBuf::from(&e.cfg.state_dir);
    assert!(std::fs::read_to_string(dir.join("closed.jsonl"))
        .unwrap()
        .contains("\"its_sell\""));
    // a sale of a coin we no longer hold changes nothing
    e.on_tx(Src::Copied, &sell);
    assert_eq!(e.book.positions[0].state, State::Closed);
}
