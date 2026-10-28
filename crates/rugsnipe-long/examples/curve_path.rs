//! A coin's curve, trade by trade, from its transactions (RPC `json`
//! encoding, one file each in DIR): slot, time, signer, side, amounts, the
//! post-trade virtual reserves and the price against the state the first
//! buy by WALLET left (the long copy's reference: `curve_price_ratio`).
//! Checks a paper position's fill and exits against the chain.
//!
//!     cargo run -p rugsnipe-long --example curve_path -- DIR MINT WALLET
use pump_codec::multihop::curve_price_ratio;
use pump_codec::Pubkey;
use rugsnipe_long::txlite::TxLite;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mint: Pubkey = a[1].parse().expect("MINT");
    let w: Pubkey = a[2].parse().expect("WALLET");
    let mut txs: Vec<(TxLite, i64)> = std::fs::read_dir(&a[0])
        .unwrap()
        .filter_map(|e| {
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(e.ok()?.path()).ok()?).ok()?;
            let bt = v["blockTime"].as_i64().unwrap_or(0);
            Some((TxLite::from_rpc_json(&v).ok()?, bt))
        })
        .collect();
    txs.sort_by_key(|(t, _)| t.slot);
    let mut vt_ref = None;
    for (tx, bt) in &txs {
        if tx.err {
            continue;
        }
        for t in tx.trades().iter().filter(|t| t.mint == mint) {
            if vt_ref.is_none() && t.user == w && t.is_buy {
                vt_ref = Some(t.vt);
            }
            let px = vt_ref.map_or(0.0, |r| curve_price_ratio(r, t.vt, 0));
            println!(
                "{} {:02}:{:02}:{:02} {} {} {:>4} tok {:>16} quote {:>14} vt {:>18} vq {:>16} {:>6.3}x",
                tx.slot,
                bt.rem_euclid(86_400) / 3600,
                bt.rem_euclid(3600) / 60,
                bt.rem_euclid(60),
                &t.user.to_string()[..8],
                if t.user == w { "IT " } else { "   " },
                if t.is_buy { "buy" } else { "sell" },
                t.token_amount,
                t.quote_amount,
                t.vt,
                t.vq,
                px
            );
        }
    }
}
