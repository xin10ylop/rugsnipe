//! A held coin that graduates (its curve completes and the liquidity moves to
//! its canonical PumpSwap pool) is detected from the failed curve sell and
//! its pool sale is read from the landed transaction. The hot loop's side is
//! `sell_lifetime.rs::a_graduated_coin_sells_on_its_pumpswap_pool`.
//!
//! Fixture: a real direct PumpSwap sell of 2RR6sfbi (the coin that graduated
//! at our buy on 2026-10-27), `getTransaction` `jsonParsed` as the real lane
//! fetches it.

use pump_codec::Pubkey;
use rugsnipe::cold::BONDING_CURVE_COMPLETE;
use rugsnipe::real::{fail_reason, own_pool_sell, own_trade, pool_vaults, trade_of};
use serde_json::{json, Value};

const MINT: &str = "2RR6sfbiMbySH2b6Bwo4ZxUiByn29uhxfTSMY98Xpump";
const SELLER: &str = "7tq2UBBcsrY42fidD8fuvDQ1VeYXUEt9jwFoV4vmiKPn";

fn fixture() -> Value {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pool_sell_2RR6sfbi.json"
    );
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    v["result"].clone()
}

#[test]
fn bonding_curve_complete_has_its_own_fail_reason() {
    // the error of every one of the 405 failed sells of 2RR6sfbi
    let e = json!({"InstructionError": [4, {"Custom": 6005}]});
    assert_eq!(fail_reason(&e).0, BONDING_CURVE_COMPLETE);
    // a slippage failure is not a graduation
    let s = json!({"InstructionError": [3, {"Custom": 6002}]});
    assert_ne!(fail_reason(&s).0, BONDING_CURVE_COMPLETE);
}

#[test]
fn the_canonical_pool_vaults_are_the_ones_its_sells_use() {
    let (base, quote) = pool_vaults(&MINT.parse().unwrap());
    // accounts 7 and 8 of sell_v2 in our own mainnet-simulated sell of 2RR6sfbi
    assert_eq!(
        base.to_string(),
        "3PFDjHcRbL7Yeny2gvZMmL9vVxsbxFWdQsa9Zy2Wc5s8"
    );
    assert_eq!(
        quote.to_string(),
        "4ngZUGwcVexvukD2unBQem6bdkv79BEMRonWDujmbSjJ"
    );
}

#[test]
fn a_pool_sale_reads_from_the_transactions_balances() {
    let tx = fixture();
    let (mint, seller): (Pubkey, Pubkey) = (MINT.parse().unwrap(), SELLER.parse().unwrap());
    // no curve TradeEvent: the curve parser finds nothing
    assert!(own_trade(&tx, &mint, &seller, 0).is_none());
    let t = own_pool_sell(&tx, &mint, &seller, 0).expect("the pool sale");
    assert!(t.pool && !t.is_buy);
    assert_eq!(t.tokens, 2_396_194_659_118, "its token balance's drop");
    // net: its lamport gain 55,259,680 + the fee 23,284; gross: the wSOL vault's drop
    assert_eq!(t.sol, 58_115_486);
    assert_eq!(t.fee, 58_115_486 - 55_259_680 - 23_284);
    assert_eq!(
        (t.v, t.t),
        (3_234_224_607, 853_693_324_057_154),
        "the pool after the sale"
    );
    let x = trade_of(&t, mint, seller, 9);
    assert_eq!(x.venue, rugsnipe_exit::Venue::PumpSwap);
    // a tip paid in the same transaction counts as spent, not as a worse price
    let tipped = own_pool_sell(&tx, &mint, &seller, 1_000_000).unwrap();
    assert_eq!(tipped.sol, t.sol);
    assert_eq!(tipped.fee, t.fee - 1_000_000);
    // another wallet did not trade in it
    assert!(own_pool_sell(&tx, &mint, &Pubkey::new_unique(), 0).is_none());
}
