//! What the copied wallet did in one transaction: a buy of a pump coin
//! priced in another token (the trigger), or a sell.
//!
//! Its buy is valued in SOL from what it spent: wrapped SOL into its own
//! `multi_hop_swap`, native SOL into a Jupiter leg in the same transaction,
//! or a held token at its price ([`crate::oracle`]). Its sizes come in tiers
//! (about 0.9, 1.13 and 1.5 SOL, docs/20); the copy takes the 1.5 tier.

use crate::txlite::{Trade, TxLite};
use pump_codec::constants as c;
use pump_codec::multihop::Route;
use pump_codec::Pubkey;

/// Jupiter v6.
pub const JUPITER: Pubkey = Pubkey::from_str_const("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");

/// Its buy of a coin priced in another token.
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub sig: String,
    pub slot: u64,
    pub mint: Pubkey,
    /// The curve's quote mint.
    pub quote_mint: Pubkey,
    pub coin_program: Pubkey,
    pub quote_program: Pubkey,
    /// Coin tokens it got and quote it paid the curve.
    pub its_tokens: u64,
    pub its_quote_in: u64,
    /// The curve after its buy (the exits' reference).
    pub vt_post: u64,
    pub vq_post: u64,
    pub real_token_post: Option<u64>,
    /// What its route started from and how much (raw units).
    pub input_mint: Pubkey,
    pub input_amount: u64,
    /// Its buy in lamports (`None`: paid in a token without a price).
    pub sol_value: Option<u64>,
    /// Its own route when it used `multi_hop_swap` (input first).
    pub route: Option<Route>,
    /// It bought through Jupiter in the same transaction.
    pub via_jupiter: bool,
}

/// One thing it did.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    Buy(Box<Trigger>),
    /// A sell of `mint` (informational: our exits are our own).
    Sell {
        mint: Pubkey,
        tokens: u64,
    },
    /// A buy of a SOL-priced coin (not copied).
    SolBuy {
        mint: Pubkey,
    },
}

/// Reads `w`'s trades in `tx`. `price` gives lamports per raw unit of a
/// held token (the floats).
pub fn read(tx: &TxLite, w: &Pubkey, price: &dyn Fn(&Pubkey) -> Option<f64>) -> Vec<Seen> {
    if tx.err || tx.signer() != Some(*w) {
        return vec![];
    }
    let trades = tx.trades();
    let mhs = tx.multihops();
    let jup = tx.calls(&JUPITER);
    let mut out = vec![];
    for t in trades.iter().filter(|t| t.user == *w) {
        if !t.is_buy {
            out.push(Seen::Sell {
                mint: t.mint,
                tokens: t.token_amount,
            });
            continue;
        }
        if t.sol_quoted() {
            out.push(Seen::SolBuy { mint: t.mint });
            continue;
        }
        if let Some(tr) = trigger(tx, w, t, &mhs, jup, price) {
            out.push(Seen::Buy(Box::new(tr)));
        }
    }
    out
}

fn trigger(
    tx: &TxLite,
    w: &Pubkey,
    t: &Trade,
    mhs: &[pump_codec::multihop::ParsedMultiHop],
    jup: bool,
    price: &dyn Fn(&Pubkey) -> Option<f64>,
) -> Option<Trigger> {
    let quote_mint = t.quote_mint?;
    // its own multi_hop_swap ending in this coin
    let mh = mhs
        .iter()
        .find(|m| m.user == *w && m.hops.iter().any(|h| h.base_mint == t.mint));
    let (route, input_mint, input_amount) = match mh {
        Some(m) => {
            let input = tx
                .tokens
                .iter()
                .find(|b| tx.keys.get(b.index as usize) == Some(&m.user_in))
                .map(|b| b.mint)
                .or_else(|| infer_input(&m.hops))?;
            let r = Route::new(input, m.hops.clone()).ok()?;
            (Some(r), input, m.amount_in)
        }
        None => (None, quote_mint, t.quote_amount),
    };
    let spent_sol = tx.sol_spent(w) - tx.fee as i128;
    let sol_value = if c::is_sol_like_quote_mint(&input_mint) {
        Some(input_amount)
    } else if jup && spent_sol > 50_000_000 {
        // a Jupiter leg from SOL in the same transaction (it sends 5/3 SOL
        // and keeps about a tenth of the output): the share of what it
        // spent that the buy used (rent and tips in: within a few thousandths)
        let left = tx.token_delta(w, &input_mint).max(0) as f64;
        let used = input_amount as f64;
        Some((spent_sol as f64 * used / (used + left)) as u64)
    } else {
        price(&input_mint).map(|p| (p * input_amount as f64) as u64)
    };
    Some(Trigger {
        sig: tx.sig_b58(),
        slot: tx.slot,
        mint: t.mint,
        quote_mint,
        coin_program: tx.mint_program(&t.mint).unwrap_or(c::TOKEN_2022_PROGRAM_ID),
        quote_program: tx
            .mint_program(&quote_mint)
            .unwrap_or(c::TOKEN_2022_PROGRAM_ID),
        its_tokens: t.token_amount,
        its_quote_in: t.quote_amount,
        vt_post: t.vt,
        vq_post: t.vq,
        real_token_post: t.real_token,
        input_mint,
        input_amount,
        sol_value,
        route,
        via_jupiter: jup,
    })
}

/// The input of a chain of hops when no token balance names it: the first
/// hop's mint that the second does not trade (one hop: its quote, a buy).
fn infer_input(hops: &[pump_codec::multihop::Hop]) -> Option<Pubkey> {
    let h0 = hops.first()?;
    match hops.get(1) {
        Some(h1) if h1.has(&h0.base_mint) => Some(h0.quote_mint),
        Some(_) => Some(h0.base_mint),
        None => Some(h0.quote_mint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(name: &str) -> TxLite {
        let p = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        TxLite::from_rpc_json(&v).unwrap()
    }
    fn w4() -> Pubkey {
        "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg"
            .parse()
            .unwrap()
    }
    fn none(_: &Pubkey) -> Option<f64> {
        None
    }
    fn one(seen: Vec<Seen>) -> Trigger {
        let buys: Vec<Trigger> = seen
            .into_iter()
            .filter_map(|s| match s {
                Seen::Buy(t) => Some(*t),
                _ => None,
            })
            .collect();
        assert_eq!(buys.len(), 1, "{buys:?}");
        buys.into_iter().next().unwrap()
    }

    #[test]
    fn a_wsol_multihop_buy_is_valued_at_its_wsol_in() {
        let t = one(read(&fx("w4_buy_wsol_multihop"), &w4(), &none));
        let r = t.route.as_ref().expect("its route");
        assert_eq!(r.input, c::WSOL_MINT);
        assert_eq!(r.output(), t.mint);
        assert_eq!(r.hops.len(), 2);
        assert_eq!(t.sol_value, Some(t.input_amount));
        assert!(t.sol_value.unwrap() >= 1_450_000_000, "{t:?}");
        assert!(!c::is_sol_like_quote_mint(&t.quote_mint));
        assert!(t.vt_post > 0 && t.vq_post > 0 && t.its_tokens > 0);
    }

    #[test]
    fn a_wbtc_multihop_after_jupiter_is_valued_at_the_sol_it_spent() {
        let tx = fx("w4_buy_wbtc_multihop");
        let t = one(read(&tx, &w4(), &none));
        assert!(t.via_jupiter);
        assert_eq!(
            t.input_mint.to_string(),
            "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh"
        );
        let v = t.sol_value.expect("valued from its SOL");
        assert!((1_450_000_000..1_600_000_000).contains(&v), "{v}");
        assert_eq!(t.route.as_ref().unwrap().hops.len(), 2);
    }

    #[test]
    fn a_direct_curve_buy_after_jupiter_has_no_route_and_its_sol_value() {
        let t = one(read(&fx("w4_buy_jup_direct"), &w4(), &none));
        assert!(t.route.is_none());
        assert_eq!(t.input_mint, t.quote_mint);
        assert!(t.sol_value.unwrap() >= 1_450_000_000);
    }

    #[test]
    fn a_float_paid_buy_needs_a_price() {
        let tx = fx("w4_buy_float_direct");
        let t = one(read(&tx, &w4(), &none));
        assert_eq!(t.sol_value, None);
        let t = one(read(&tx, &w4(), &|_m: &Pubkey| Some(0.001)));
        assert_eq!(t.sol_value, Some((0.001 * t.input_amount as f64) as u64));
    }

    #[test]
    fn its_sell_reads_as_a_sell() {
        let s = read(&fx("w4_sell_wbtc_multihop"), &w4(), &none);
        assert!(matches!(s.as_slice(), [Seen::Sell { .. }]), "{s:?}");
        // another signer's transaction is not its
        let other: Pubkey = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
            .parse()
            .unwrap();
        assert!(read(&fx("w4_sell_wbtc_multihop"), &other, &none).is_empty());
    }
}
