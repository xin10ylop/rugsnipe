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
