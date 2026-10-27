//! What the long copy's detection and route planner make of the copied
//! wallet's transactions (RPC `json` encoding, one file each in DIR): every
//! buy of a coin priced in another token, its SOL value, and for the ones at
//! `min_sol` or more, the route or why there is none. PRICES (optional) is a
//! JSON map of mint -> lamports per raw unit for buys paid in a held token.
//!
//!     cargo run -p rugsnipe-long --example scan -- RPC_URL DIR [PRICES] [MIN_SOL]
use pump_codec::constants as c;
use pump_codec::Pubkey;
use rugsnipe_long::chain::Rpc;
use rugsnipe_long::detect::{self, Seen};
use rugsnipe_long::route;
use rugsnipe_long::txlite::TxLite;
use std::collections::HashMap;

const W4: &str = "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg";
const WBTC: &str = "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh";

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut rpc = Rpc::new(&a[0]);
    let prices: HashMap<String, f64> = a
        .get(2)
        .filter(|p| !p.is_empty())
        .map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap())
        .unwrap_or_default();
    let min_sol: f64 = a.get(3).map_or(1.45, |s| s.parse().unwrap());
    let w4: Pubkey = W4.parse().unwrap();
    let anchors = [c::WSOL_MINT, WBTC.parse().unwrap()];
    let mut files: Vec<_> = std::fs::read_dir(&a[1])
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let price = |m: &Pubkey| prices.get(&m.to_string()).copied();
    let mut bad = 0;
    let mut txs: Vec<TxLite> = files
        .iter()
        .filter_map(|p| {
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
            match TxLite::from_rpc_json(&v) {
                Ok(t) => Some(t),
                Err(e) => {
                    bad += 1;
                    println!("not parsed ({} v{}): {e}", p.display(), v["version"]);
                    None
                }
            }
        })
        .collect();
    println!("{} parsed, {bad} not", txs.len());
    txs.sort_by_key(|t| t.slot);
    let (mut sol_buys, mut sells, mut small, mut unpriced) = (0, 0, 0, 0);
    for tx in &txs {
        for s in detect::read(tx, &w4, &price) {
            match s {
                Seen::SolBuy { .. } => sol_buys += 1,
                Seen::Sell { .. } => sells += 1,
                Seen::Buy(t) => {
                    let Some(v) = t.sol_value else {
                        unpriced += 1;
                        println!(
                            "{} {} unpriced: paid in {}",
                            t.slot,
                            &t.mint.to_string()[..8],
                            &t.input_mint.to_string()[..8]
                        );
                        continue;
                    };
                    let sol = v as f64 / 1e9;
                    if sol < min_sol {
                        small += 1;
                        continue;
                    }
                    let how = match route::plan(&t, &anchors, &mut rpc) {
                        Ok(p) => format!("ROUTABLE {p:?}"),
                        Err(e) => format!("no route: {e:?}"),
                    };
                    println!(
                        "{} {} {:.3} SOL from {} quote {} jupiter {}: {}",
                        t.slot,
                        &t.mint.to_string()[..8],
                        sol,
                        &t.input_mint.to_string()[..8],
                        &t.quote_mint.to_string()[..8],
                        t.via_jupiter,
                        how.chars().take(160).collect::<String>()
                    );
                }
            }
        }
    }
    println!(
        "{} transactions: SOL-priced buys {sol_buys}, sells {sells}, other-quote buys under {min_sol} SOL {small}, unpriced {unpriced}",
        txs.len()
    );
}
