//! Our buy and sell transactions (legacy, no lookup table: a 2-hop route
//! with the wrap, the ATA, the budget and the tip fits, as wallet 4's own do).
//!
//! * **Buy**: compute budget; when paying wrapped SOL, create our wSOL ATA,
//!   transfer the stake in, `SyncNative`; create the coin ATA;
//!   `multi_hop_swap`; close the wSOL ATA (the wallet stays in native SOL);
//!   the tip.
//! * **Sell**: compute budget; the wSOL ATA when ending in wrapped SOL;
//!   `multi_hop_swap` of every token we hold back along the route; close
//!   the wSOL ATA and the emptied coin ATA (its rent comes back); the tip.

use pump_codec::constants as c;
use pump_codec::instructions as pi;
use pump_codec::multihop::{self, MultiHop, Route};
use pump_codec::{pda, Instruction, Pubkey};
use rugsnipe_tx::msg::{compile_signed_legacy, CompileError};

/// Budget and tip of one transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fees {
    pub cu_limit: u32,
    /// Micro-lamports per compute unit.
    pub cu_price: u64,
    /// Tip account and lamports (`None`: no tip, e.g. plain RPC).
    pub tip: Option<(Pubkey, u64)>,
}

fn budget(f: &Fees) -> Vec<Instruction> {
    vec![
        pi::set_compute_unit_limit(f.cu_limit),
        pi::set_compute_unit_price(f.cu_price),
    ]
}

fn tip(user: &Pubkey, f: &Fees) -> Option<Instruction> {
    f.tip
        .filter(|(_, l)| *l > 0)
        .map(|(to, l)| pi::system_transfer(user, &to, l))
}

/// The buy: `amount_in` of `route.input` (lamports when wrapped SOL) for at
/// least `min_out` coin tokens.
pub fn buy_ixs(
    user: &Pubkey,
    route: &Route,
    anchor_program: &Pubkey,
    coin_program: &Pubkey,
    amount_in: u64,
    min_out: u64,
    fees: &Fees,
    buyback_index: usize,
) -> Vec<Instruction> {
    let anchor = route.input;
    let coin = route.output();
    let anchor_ata = pda::associated_token_address(user, &anchor, anchor_program);
    let coin_ata = pda::associated_token_address(user, &coin, coin_program);
    let wsol = anchor == c::WSOL_MINT;
    let mut v = budget(fees);
    if wsol {
        v.push(pi::create_associated_token_account_idempotent(
            user,
            user,
            &c::WSOL_MINT,
            &c::SPL_TOKEN_PROGRAM_ID,
        ));
        v.push(pi::system_transfer(user, &anchor_ata, amount_in));
        v.push(pi::spl_sync_native(&anchor_ata));
    }
    v.push(pi::create_associated_token_account_idempotent(
        user,
        user,
        &coin,
        coin_program,
    ));
    v.push(multihop::multi_hop_swap(&MultiHop {
        user: *user,
        user_in: anchor_ata,
        user_out: coin_ata,
        route,
        outer_mint: anchor,
        outer_token_program: *anchor_program,
        buyback_index,
        amount_in,
        min_amount_out: min_out,
    }));
    if wsol {
        v.push(pi::spl_close_account(&anchor_ata, user, user));
    }
    v.extend(tip(user, fees));
    v
}

/// The sell: `tokens` coin tokens back along `route` (the buy route
/// reversed: coin first) for at least `min_out` of the anchor. With
/// `close_coin`, the coin ATA is closed after (sell every token we hold).
pub fn sell_ixs(
    user: &Pubkey,
    route: &Route,
    anchor_program: &Pubkey,
    coin_program: &Pubkey,
    tokens: u64,
    min_out: u64,
    close_coin: bool,
    fees: &Fees,
    buyback_index: usize,
) -> Vec<Instruction> {
    let coin = route.input;
    let anchor = route.output();
    let anchor_ata = pda::associated_token_address(user, &anchor, anchor_program);
    let coin_ata = pda::associated_token_address(user, &coin, coin_program);
    let wsol = anchor == c::WSOL_MINT;
    let mut v = budget(fees);
    if wsol {
        v.push(pi::create_associated_token_account_idempotent(
            user,
            user,
            &c::WSOL_MINT,
            &c::SPL_TOKEN_PROGRAM_ID,
        ));
    }
    v.push(multihop::multi_hop_swap(&MultiHop {
        user: *user,
        user_in: coin_ata,
        user_out: anchor_ata,
        route,
        outer_mint: anchor,
        outer_token_program: *anchor_program,
        buyback_index,
        amount_in: tokens,
        min_amount_out: min_out,
    }));
    if wsol {
        v.push(pi::spl_close_account(&anchor_ata, user, user));
    }
    if close_coin {
        v.push(pi::token_close_account(coin_program, &coin_ata, user, user));
    }
    v.extend(tip(user, fees));
    v
}

/// Compile and sign (legacy). `sign` signs the message bytes.
pub fn sign(
    user: &Pubkey,
    ixs: &[Instruction],
    blockhash: &[u8; 32],
    sign: impl FnOnce(&[u8]) -> [u8; 64],
) -> Result<(Vec<u8>, [u8; 64]), CompileError> {
    compile_signed_legacy(user, ixs, blockhash, sign)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pump_codec::multihop::Hop;

    fn k(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn two_hops_from_wsol() -> Route {
        let (q, coin) = (k(5), k(7));
        let pool = Hop::pool(
            &k(9),
            &q,
            &c::TOKEN_2022_PROGRAM_ID,
            &c::WSOL_MINT,
            &c::SPL_TOKEN_PROGRAM_ID,
        );
        let curve = Hop::curve(
            &coin,
            &c::TOKEN_2022_PROGRAM_ID,
            &q,
            &c::TOKEN_2022_PROGRAM_ID,
        );
        Route::new(c::WSOL_MINT, vec![pool, curve]).unwrap()
    }

    #[test]
    fn a_two_hop_buy_and_sell_from_wsol_fit_one_transaction() {
        let user = k(1);
        let r = two_hops_from_wsol();
        let fees = Fees {
            cu_limit: 300_000,
            cu_price: 2_000_000,
            tip: Some((k(2), 1_000_000)),
        };
        let b = buy_ixs(
            &user,
            &r,
            &c::SPL_TOKEN_PROGRAM_ID,
            &c::TOKEN_2022_PROGRAM_ID,
            1_000_000_000,
            1,
            &fees,
            0,
        );
        let (bytes, _) = sign(&user, &b, &[0; 32], |_| [7; 64]).unwrap();
        assert!(bytes.len() <= rugsnipe_tx::MAX_TX_LEN, "{}", bytes.len());
        let s = sell_ixs(
            &user,
            &r.reversed(),
            &c::SPL_TOKEN_PROGRAM_ID,
            &c::TOKEN_2022_PROGRAM_ID,
            5,
            1,
            true,
            &fees,
            0,
        );
        let (bytes, sig) = sign(&user, &s, &[0; 32], |_| [7; 64]).unwrap();
        assert!(bytes.len() <= rugsnipe_tx::MAX_TX_LEN, "{}", bytes.len());
        assert_eq!(sig, [7; 64]);
        assert_eq!(&bytes[1..65], &[7; 64]);
    }

    #[test]
    fn the_buy_wraps_exactly_the_stake_and_unwraps_after() {
        let user = k(1);
        let r = two_hops_from_wsol();
        let fees = Fees {
            cu_limit: 1,
            cu_price: 1,
            tip: None,
        };
        let b = buy_ixs(
            &user,
            &r,
            &c::SPL_TOKEN_PROGRAM_ID,
            &c::TOKEN_2022_PROGRAM_ID,
            123,
            4,
            &fees,
            3,
        );
        assert_eq!(b.len(), 2 + 3 + 1 + 1 + 1);
        let wsol_ata =
            pda::associated_token_address(&user, &c::WSOL_MINT, &c::SPL_TOKEN_PROGRAM_ID);
        assert_eq!(b[3].accounts[1].pubkey, wsol_ata);
        assert_eq!(&b[3].data[4..12], &123u64.to_le_bytes());
        let mh = &b[6];
        assert_eq!(mh.program_id, c::PUMP_SWAP_PROGRAM_ID);
        assert_eq!(mh.accounts[1].pubkey, wsol_ata);
        assert_eq!(
            mh.accounts[6].pubkey,
            multihop::buyback_fee_account(3, &c::WSOL_MINT, &c::SPL_TOKEN_PROGRAM_ID)
        );
        assert_eq!(b[7].program_id, c::SPL_TOKEN_PROGRAM_ID);
        assert_eq!(b[7].data, vec![9]);
    }
}
