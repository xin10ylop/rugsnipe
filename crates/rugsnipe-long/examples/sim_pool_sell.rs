//! The long copy's sell of a graduated coin, simulated on mainnet from a
//! wallet that holds it: the engine's own builder (`txb::sell_ixs`) on the
//! coin's canonical PumpSwap pool, as `send_sell` builds it once the curve has
//! completed. `QUOTE` is the coin's quote (WBTC, a graduated pump coin...);
//! `THEN` optionally a second pool hop from the quote to the anchor (the
//! anchor's pool with the quote, e.g. wSOL or WBTC), as a 2-hop route walks
//! back.
//!
//!     cargo run -p rugsnipe-long --example sim_pool_sell -- RPC_URL HOLDER COIN QUOTE TOKENS [THEN_ANCHOR]
use pump_codec::constants as c;
use pump_codec::multihop::{Hop, Route};
use pump_codec::{pda, Pubkey};
use rugsnipe_long::chain::Rpc;
use rugsnipe_long::route::Chain;
use rugsnipe_long::txb::{self, Fees};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let pk = |s: &str| s.parse::<Pubkey>().map_err(|_| format!("bad key {s}"));
    let mut rpc = Rpc::new(&a[0]);
    let (holder, coin, quote) = (pk(&a[1])?, pk(&a[2])?, pk(&a[3])?);
    let tokens: u64 = a[4].parse().map_err(|_| "TOKENS")?;
    let coin_prog = rpc.mint_program(&coin).ok_or("coin program")?;
    let quote_prog = rpc.mint_program(&quote).ok_or("quote program")?;
    let pool = pda::canonical_pump_swap_pool(&coin, &quote);
    println!("coin pool {pool}: exists {}", rpc.pool_exists(&pool));
    let mut hops = vec![Hop::pool(&pool, &coin, &coin_prog, &quote, &quote_prog)];
    let mut anchor = quote;
    if let Some(then) = a.get(5) {
        // the quote's own pool against the anchor (quote is its base)
        let anc = pk(then)?;
        let p2 = pda::canonical_pump_swap_pool(&quote, &anc);
        let anc_prog = rpc.mint_program(&anc).ok_or("anchor program")?;
        println!("quote pool {p2}: exists {}", rpc.pool_exists(&p2));
        hops.push(Hop::pool(&p2, &quote, &quote_prog, &anc, &anc_prog));
        anchor = anc;
    }
    let route = Route::new(coin, hops).map_err(|e| format!("{e:?}"))?;
    let anchor_prog = rpc.mint_program(&anchor).ok_or("anchor program")?;
    let fees = Fees {
        cu_limit: 400_000,
        cu_price: 1,
        tip: None,
    };
    let ixs = txb::sell_ixs(
        &holder,
        &route,
        &anchor_prog,
        &coin_prog,
        tokens,
        1,
        false,
        &fees,
        0,
    );
    let (bytes, _) = txb::sign(&holder, &ixs, &[0; 32], |_| [0; 64]).map_err(|e| e.to_string())?;
    let watch = if anchor == c::WSOL_MINT {
        holder
    } else {
        pda::associated_token_address(&holder, &anchor, &anchor_prog)
    };
    let before = if anchor == c::WSOL_MINT {
        rpc.balance(&holder)?
    } else {
        rpc.token_balance(&holder, &anchor, &anchor_prog)
            .unwrap_or(0)
    };
    let v = rpc.simulate(&bytes, &[watch])?;
    println!(
        "pool sell of {tokens} {}: {} bytes, err {}, units {}",
        &coin.to_string()[..8],
        bytes.len(),
        v["err"],
        v["unitsConsumed"]
    );
    if v["err"].is_null() {
        let acc = &v["accounts"][0];
        let after = if anchor == c::WSOL_MINT {
            acc["lamports"].as_u64().unwrap_or(0)
        } else {
            acc["data"][0]
                .as_str()
                .and_then(|s| {
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok()
                })
                .filter(|d| d.len() >= 72)
                .map_or(0, |d| u64::from_le_bytes(d[64..72].try_into().unwrap()))
        };
        println!(
            "   {} received: {} raw units (before {before}, after {after})",
            &anchor.to_string()[..8],
            after as i128 - before as i128
        );
    } else {
        for l in v["logs"].as_array().into_iter().flatten().rev().take(10) {
            println!("   {}", l.as_str().unwrap_or(""));
        }
    }
    Ok(())
}
