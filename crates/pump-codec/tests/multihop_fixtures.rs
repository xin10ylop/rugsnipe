//! `multi_hop_swap` against H9kuqGKN's transactions of 2026-10-26: a buy of
//! GzMGFbVw (a curve quoted in the graduated coin C5xph2yL, whose PumpSwap
//! pool is quoted in WBTC) paid in WBTC, and its sell back to WBTC.
//! Buy 4xLvL3Xm9bDq (slot 461783589), sell 253UaGqNAp8H (slot 461792027).

use pump_codec::constants as c;
use pump_codec::multihop::{self, Hop, MultiHop, Route, Venue};
use pump_codec::pda;
use solana_pubkey::Pubkey;
use std::str::FromStr;

fn pk(s: &str) -> Pubkey {
    Pubkey::from_str(s).unwrap()
}

const BUY: [&str; 26] = [
    "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg",
    "9F87FQWKTyzgCG3DwSYdXHsEPpm58ZKv6WEH9JReJ729",
    "3bXBkFdmNQacteWPYp7eRT78jnNQ3Sr64a3miB8gaW43",
    "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw",
    "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx",
    "Grgw3sq3PHLMhbRPd7Kq36hnmLKyASGFuJYkPWTcEF3A",
    "51yYvkYFBpxCg5PYaGiSTgr4hkE8ooPHp18PrsKjgvTg",
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
    "11111111111111111111111111111111",
    "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR",
    "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
    "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf",
    "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt",
    "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1",
    "C5xph2yL6gMdxmuLGxJBkGpmuDFfyi5fqkBNhvL6pump",
    "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh",
    "3M9sVgL4u6ATKaiExf3XhQqNfMB6ijvzk3meMcQaen1w",
    "AhVBxmfr4s7W4ZcGnwp5eNo5i4Nv4tCE1YQrCcGDWgas",
    "3nQRB2Tsf7VFt3kv5eeD8zT9nB1Ad6DHTTmULC7mfnUd",
    "GzMGFbVwUVNACvZuZRvoctf7gZfUFwYToqrGj7Wcpump",
    "C5xph2yL6gMdxmuLGxJBkGpmuDFfyi5fqkBNhvL6pump",
    "4mPHibdxYZdQnToP91dPRGrCbYizDrzsLK1hVWPHMAoQ",
    "7xZoQraEQ48RrLpUnrrb6kgCFS5BEYznpgeZuSVzydPZ",
    "5EUPJptXDhzyMmCMsVidRe3pXnYdwX8wz5deUgkPMFkL",
];
/// Writable per account of the buy (from the transaction header).
const BUY_W: &str = "wwwrrwwrrrrrrrrrrrwwwrrwww";
const BUY_DATA: &str = "2b644913e9f66f944fe10200000000000100000000000000";
const SELL_DATA: &str = "2b644913e9f66f946ad9dcf54a2100000100000000000000";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn mints() -> (Pubkey, Pubkey, Pubkey) {
    (
        pk("3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh"), // WBTC (SPL)
        pk("C5xph2yL6gMdxmuLGxJBkGpmuDFfyi5fqkBNhvL6pump"), // the curve's quote (Token-2022)
        pk("GzMGFbVwUVNACvZuZRvoctf7gZfUFwYToqrGj7Wcpump"), // the coin (Token-2022)
    )
}

fn route() -> Route {
    let (wbtc, mid, coin) = mints();
    let pool = Hop::pool(
        &pda::canonical_pump_swap_pool(&mid, &wbtc),
        &mid,
        &c::TOKEN_2022_PROGRAM_ID,
        &wbtc,
        &c::SPL_TOKEN_PROGRAM_ID,
    );
    let curve = Hop::curve(
        &coin,
        &c::TOKEN_2022_PROGRAM_ID,
        &mid,
        &c::TOKEN_2022_PROGRAM_ID,
    );
    Route::new(wbtc, vec![pool, curve]).unwrap()
}

#[test]
fn hops_derive_the_real_markets_and_vaults() {
    let r = route();
    let got: Vec<String> = r
        .hops
        .iter()
        .flat_map(|h| h.accounts())
        .map(|k| k.to_string())
        .collect();
    assert_eq!(
        got,
        BUY[16..].iter().map(|s| s.to_string()).collect::<Vec<_>>()
    );
    assert_eq!(r.hops[0].venue, Venue::Pool);
    assert_eq!(r.hops[1].venue, Venue::Curve);
}

#[test]
fn buy_is_byte_equal_to_its_instruction() {
    let (wbtc, _, coin) = mints();
    let user = pk(BUY[0]);
    let r = route();
    let ix = multihop::multi_hop_swap(&MultiHop {
        user,
        user_in: pda::associated_token_address(&user, &wbtc, &c::SPL_TOKEN_PROGRAM_ID),
        user_out: pda::ata_token_2022(&user, &coin),
        route: &r,
        outer_mint: wbtc,
        outer_token_program: c::SPL_TOKEN_PROGRAM_ID,
        buyback_index: 0,
        amount_in: 188_751,
        min_amount_out: 1,
    });
    assert_eq!(ix.program_id, c::PUMP_SWAP_PROGRAM_ID);
    let got: Vec<String> = ix.accounts.iter().map(|m| m.pubkey.to_string()).collect();
    assert_eq!(got, BUY.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let w: String = ix
        .accounts
        .iter()
        .map(|m| if m.is_writable { 'w' } else { 'r' })
        .collect();
    assert_eq!(w, BUY_W);
    assert!(ix.accounts[0].is_signer && ix.accounts.iter().skip(1).all(|m| !m.is_signer));
    assert_eq!(hex(&ix.data), BUY_DATA);
}

#[test]
fn sell_is_the_reversed_route_to_the_same_accounts() {
    let (wbtc, _, coin) = mints();
    let user = pk(BUY[0]);
    let r = route().reversed();
    assert_eq!(r.input, coin);
    assert_eq!(r.output(), wbtc);
    let ix = multihop::multi_hop_swap(&MultiHop {
        user,
        user_in: pda::ata_token_2022(&user, &coin),
        user_out: pda::associated_token_address(&user, &wbtc, &c::SPL_TOKEN_PROGRAM_ID),
        route: &r,
        outer_mint: wbtc,
        outer_token_program: c::SPL_TOKEN_PROGRAM_ID,
        buyback_index: 0,
        amount_in: 36_605_836_188_010,
        min_amount_out: 1,
    });
    // the sell transaction lists in / out swapped and the hops curve first
    let mut want: Vec<&str> = BUY.to_vec();
    want.swap(1, 2);
    let hops: Vec<&str> = BUY[21..26]
        .iter()
        .chain(BUY[16..21].iter())
        .copied()
        .collect();
    want.splice(16.., hops);
    let got: Vec<String> = ix.accounts.iter().map(|m| m.pubkey.to_string()).collect();
    assert_eq!(got, want);
    assert_eq!(hex(&ix.data), SELL_DATA);
}

#[test]
fn parse_reads_the_hops_back() {
    let keys: Vec<Pubkey> = BUY.iter().map(|s| pk(s)).collect();
    let data: Vec<u8> = (0..BUY_DATA.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&BUY_DATA[i..i + 2], 16).unwrap())
        .collect();
    let p = multihop::parse_multi_hop_swap(&keys[..], &data).unwrap();
    assert_eq!(p.user, keys[0]);
    assert_eq!(p.amount_in, 188_751);
    assert_eq!(p.min_amount_out, 1);
    assert_eq!(p.hops, route().hops);
    let (wbtc, _, _) = mints();
    assert_eq!(Route::new(wbtc, p.hops).unwrap(), route());
    assert!(multihop::parse_multi_hop_swap(&keys[..20], &data).is_none());
}
