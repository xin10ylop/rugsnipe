//! Second-pass adversarial verification (2026-10-14).
//!
//! * TxView: every strict prefix of every fixture is rejected; a differential
//!   fuzz against `agave-transaction-view` 6.1 (agave-sanitized => we parse
//!   and agree field by field; we parse => agave parses, except for the
//!   documented leniencies); random garbage never panics.
//! * Curve math: exact values at the extremes (buy that empties the curve,
//!   sell everything back, `u64::MAX` inputs) and the invariant that no trade
//!   ever lowers `k = V * T`.
//! * Fixtures of every kind: every create links to its instruction (top-level
//!   or CPI), every SOL create starts from the fresh reserves (mayhem too),
//!   and every on-chain trade instruction's accounts equal our builders'.

mod common;

use agave_transaction_view::sanitize::SanitizeConfig;
use agave_transaction_view::transaction_version::TransactionVersion as AV;
use agave_transaction_view::transaction_view::TransactionView as AgaveView;
use common::*;
use pump_codec::constants as c;
use pump_codec::curve::{self, CurveState, FeeBps};
use pump_codec::events::{events_from_inner_instructions, PumpEvent, TradeEvent};
use pump_codec::instructions::{
    self as ixs, classify, AccountLookup, Classified, LegacyTradeKeys, PumpInstruction, PumpTrade,
    PumpTradeKind, SolV3Keys, TradeLayout,
};
use pump_codec::tx::{TxVersion, TxView};
use pump_codec::{pda, Pubkey};

// ------------------------------------------------------------- helpers
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn iters(default: usize) -> usize {
    std::env::var("PUMP_CODEC_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Agave's limits opened up so that only structural rules remain.
const PERMISSIVE: SanitizeConfig = SanitizeConfig {
    min_requested_heap_size: 0,
    max_requested_heap_size: u32::MAX,
    max_instructions: usize::MAX,
    max_accounts_per_instruction: usize::MAX,
};

/// Field-by-field equality of our view and agave's.
fn assert_same(ctx: &str, t: &TxView<'_>, a: &AgaveView<false, &[u8]>) {
    let v = match a.version() {
        AV::Legacy => TxVersion::Legacy,
        AV::V0 => TxVersion::V0,
        AV::V1 => TxVersion::V1,
    };
    assert_eq!(t.version(), v, "{ctx}");
    let h = t.header();
    assert_eq!(h.num_required_signatures, a.num_required_signatures(), "{ctx}");
    assert_eq!(
        h.num_readonly_signed_accounts,
        a.num_readonly_signed_static_accounts(),
        "{ctx}"
    );
    assert_eq!(
        h.num_readonly_unsigned_accounts,
        a.num_readonly_unsigned_static_accounts(),
        "{ctx}"
    );
    assert_eq!(t.num_signatures(), a.num_signatures() as usize, "{ctx}");
    for (i, s) in a.signatures().iter().enumerate() {
        assert_eq!(t.signature(i).unwrap().as_slice(), s.as_ref(), "{ctx}");
    }
    assert_eq!(t.static_account_keys(), a.static_account_keys(), "{ctx}");
    assert_eq!(t.recent_blockhash().as_slice(), a.recent_blockhash().as_ref());
    assert_eq!(t.message_bytes(), a.message_data(), "{ctx}");
    let ours: Vec<_> = t
        .instructions()
        .map(|i| (i.program_id_index, i.accounts, i.data))
        .collect();
    let theirs: Vec<_> = a
        .instructions_iter()
        .map(|i| (i.program_id_index, i.accounts, i.data))
        .collect();
    assert_eq!(ours, theirs, "{ctx}");
    let ol: Vec<_> = t
        .address_table_lookups()
        .map(|l| (*l.account_key, l.writable_indexes, l.readonly_indexes))
        .collect();
    let tl: Vec<_> = a
        .address_table_lookup_iter()
        .map(|l| (*l.account_key, l.writable_indexes, l.readonly_indexes))
        .collect();
    assert_eq!(ol, tl, "{ctx}");
    // agave's `total_num_accounts` / `num_requested_write_locks` (sanitized
    // views only), recomputed from its raw header and lookup frames
    let n_static = a.num_static_account_keys() as usize;
    let (lw, lr) = (
        a.total_writable_lookup_accounts() as usize,
        a.total_readonly_lookup_accounts() as usize,
    );
    assert_eq!(t.total_account_count(), n_static + lw + lr, "{ctx}");
    let writable = (0..t.total_account_count())
        .filter(|&i| t.is_writable(i))
        .count();
    let agave_writable = n_static
        - a.num_readonly_signed_static_accounts() as usize
        - a.num_readonly_unsigned_static_accounts() as usize
        + lw;
    assert_eq!(writable, agave_writable, "{ctx}");
    match (t.v1_config(), a.transaction_config()) {
        (Some(c), Some(x)) => {
            assert_eq!(c.mask, x.mask(), "{ctx}");
            assert_eq!(c.priority_fee_lamports, x.priority_fee_lamports());
            assert_eq!(c.compute_unit_limit, x.compute_unit_limit());
            assert_eq!(
                c.loaded_accounts_data_size_limit,
                x.loaded_accounts_data_size_limit()
            );
            assert_eq!(c.heap_size, x.requested_heap_size());
        }
        (None, None) => {}
        (o, _) => panic!("{ctx}: config presence differs ({o:?})"),
    }
}

/// The differential property on one buffer. Returns whether we parsed it.
fn differential(ctx: &str, raw: &[u8]) -> bool {
    let ours = TxView::parse(raw);
    let unsanitized = AgaveView::try_new_unsanitized(raw);
    let sanitized_ok = AgaveView::try_new_sanitized(raw, &PERMISSIVE).is_ok();
    if sanitized_ok {
        assert!(
            ours.is_ok(),
            "{ctx}: agave accepts (sanitized) but TxView rejects: {:?}",
            ours.err()
        );
    }
    match (&ours, &unsanitized) {
        (Ok(t), Ok(a)) => assert_same(ctx, t, a),
        (Ok(t), Err(_)) => {
            // Documented leniencies: unknown v1 config bits (forward
            // compatible) and >12 legacy/v0 signatures (agave's packet cap).
            let unknown_bits = t.v1_config().is_some_and(|c| c.mask & !0b1_1111 != 0);
            let many_sigs = t.version() != TxVersion::V1 && t.num_signatures() > 12;
            assert!(
                unknown_bits || many_sigs,
                "{ctx}: TxView accepts but agave rejects"
            );
        }
        _ => {}
    }
    ours.is_ok()
}

// ------------------------------------------------------- tx: prefixes
#[test]
fn every_strict_prefix_and_any_suffix_is_rejected() {
    let mut cuts = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        for cut in 0..fx.raw.len() {
            assert!(
                TxView::parse(&fx.raw[..cut]).is_err(),
                "{name}: prefix of {cut}/{} bytes parsed",
                fx.raw.len()
            );
            cuts += 1;
        }
        for extra in [&[0u8][..], &[0x81], &[0; 64], &fx.raw[..1]] {
            let mut m = fx.raw.clone();
            m.extend_from_slice(extra);
            assert!(TxView::parse(&m).is_err(), "{name}: trailing bytes accepted");
        }
    }
    assert!(cuts > 40_000, "{cuts}");
}

#[test]
fn version_dispatch_matches_agave() {
    // 0x81 is v1; any other byte with the MSB set is an unknown versioned
    // envelope (a legacy/v0 tx would need >= 128 signatures).
    let fx = TxFixture::load("v1_create_v2_buy");
    for first in [0x80u8, 0x82, 0xff] {
        let mut m = fx.raw.clone();
        m[0] = first;
        assert!(TxView::parse(&m).is_err(), "first byte {first:#x}");
        assert!(AgaveView::try_new_unsanitized(m.as_slice()).is_err());
    }
    // A buffer that would decode as a legacy tx with 128 signatures.
    let mut big = vec![0x80u8, 0x01];
    big.extend(std::iter::repeat_n(7u8, 128 * 64 + 400));
    assert!(TxView::parse(&big).is_err());
}

// ------------------------------------------- tx: differential fuzzing
#[test]
fn differential_fuzz_against_agave_transaction_view() {
    let mut rng = Rng(0xD1FF_E2E7_1A15_0001);
    let (mut parsed, mut total) = (0usize, 0usize);
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        assert!(differential(&name, &fx.raw), "{name}: fixture must parse");
        let v1 = fx.raw[0] == 0x81;
        for k in 0..iters(400) {
            let mut m = fx.raw.clone();
            match k % 6 {
                // random byte flips anywhere
                0 => {
                    for _ in 0..1 + rng.below(3) {
                        let i = rng.below(m.len());
                        m[i] = rng.next() as u8;
                    }
                }
                // structural fields: v1 header/mask/counts/ix headers, or the
                // legacy signature count / header / first compact counts
                1 | 2 => {
                    let hot: &[usize] = if v1 {
                        &[1, 2, 3, 4, 5, 6, 7, 40, 41]
                    } else {
                        &[0, 65, 66, 67, 68, 69, 70]
                    };
                    let i = if v1 && k % 6 == 2 {
                        // inside the instruction header table
                        let n_addr = m[41] as usize;
                        let slots = u32::from_le_bytes([m[4], m[5], m[6], m[7]]).count_ones()
                            as usize;
                        (42 + 32 * n_addr + 4 * slots + rng.below(4 * m[40] as usize + 1))
                            .min(m.len() - 1)
                    } else {
                        hot[rng.below(hot.len())].min(m.len() - 1)
                    };
                    m[i] = match rng.below(4) {
                        0 => m[i].wrapping_add(1),
                        1 => m[i].wrapping_sub(1),
                        2 => 0,
                        _ => rng.next() as u8,
                    };
                }
                // insert or delete one byte
                3 => {
                    let i = rng.below(m.len());
                    m.insert(i, rng.next() as u8);
                }
                4 => {
                    let i = rng.below(m.len());
                    m.remove(i);
                }
                // flip the priority-fee / unknown mask bits (v1) or truncate
                _ => {
                    if v1 {
                        m[4] ^= 1 << rng.below(8);
                    } else {
                        m.truncate(rng.below(m.len()));
                    }
                }
            }
            total += 1;
            if differential(&format!("{name} mutation {k}"), &m) {
                parsed += 1;
            }
        }
    }
    // the fuzz must exercise both outcomes
    assert!(parsed > total / 20 && parsed < total, "{parsed}/{total}");
}

#[test]
fn random_garbage_never_panics_and_agrees_with_agave() {
    let mut rng = Rng(0x6A2B_A6E0_0000_0001);
    for i in 0..iters(400) * 50 {
        let len = rng.below(1600);
        let mut b: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if let Some(f) = b.first_mut() {
            *f = [0x81, 0x01, 0x02, 0x80, *f][i % 5];
        }
        if b.len() > 41 && b[0] == 0x81 {
            // plausible v1 prefix: small counts, known mask bits
            b[1] = 1 + (b[1] % 3);
            b[2] = 0;
            b[3] %= 3;
            b[4] &= 0b1_1111;
            b[5] = 0;
            b[6] = 0;
            b[7] = 0;
            b[40] %= 4;
            b[41] %= 8;
        }
        let _ = differential("garbage", &b);
        if let Ok(t) = TxView::parse(&b) {
            let _ = t.compute_budget();
            for ix in t.instructions() {
                let a = t.instruction_accounts(&ix, None);
                let _ = classify(t.program_id(&ix), ix.data, &a);
            }
        }
    }
}

// ------------------------------------------------------ curve extremes
#[test]
fn curve_extremes_are_exact_and_overflow_free() {
    let creator = Pubkey::new_from_array([3; 32]);
    let fresh = CurveState::fresh(creator);
    let fees = FeeBps::DEFAULT;

    // Buy that empties the curve: any token amount caps at the remaining supply.
    let q = fresh.quote_buy_exact_tokens(u64::MAX, fees).unwrap();
    assert_eq!(q.tokens, curve::INITIAL_REAL_TOKEN_RESERVES);
    assert_eq!(q.sol_cost, curve::COMPLETION_REAL_SOL_FROM_FRESH);
    assert_eq!((q.protocol_fee, q.creator_fee), (807_550_912, 255_016_078));
    assert_eq!(q.total, 86_067_926_047);
    assert_eq!(
        fresh.quote_buy_exact_tokens(curve::INITIAL_REAL_TOKEN_RESERVES, fees),
        Some(q)
    );
    let mut done = fresh;
    done.apply_buy(q.tokens, q.sol_cost);
    assert!(done.complete);
    assert_eq!(done.real_token, 0);
    assert_eq!(done.virtual_sol, 115_005_359_057);
    assert_eq!(done.virtual_token, curve::COMPLETE_VIRTUAL_TOKEN_RESERVES);
    // nothing more to buy on a sold-out curve
    assert_eq!(done.quote_buy_exact_tokens(1, fees), None);
    assert_eq!(done.quote_buy_exact_sol_in(u64::MAX, fees), None);
    assert_eq!(done.tokens_for_sol(u64::MAX), 0);

    // Sell everything back: 1 lamport of rounding dust stays in the curve.
    let s = done.quote_sell(q.tokens, fees).unwrap();
    assert_eq!(s.sol_out, 85_005_359_056);
    assert!(s.sol_out <= done.real_sol);
    assert_eq!((s.protocol_fee, s.creator_fee), (807_550_912, 255_016_078));
    assert_eq!(s.net, 83_942_792_066);
    let mut back = done;
    back.apply_sell(q.tokens, s.sol_out);
    assert_eq!(back.real_sol, 1);
    assert_eq!(back.real_token, curve::INITIAL_REAL_TOKEN_RESERVES);

    // Exact-SOL-in: the largest budget that still fits the remaining supply
    // (binary search done offline with big integers), and one lamport more.
    let q = fresh.quote_buy_exact_sol_in(86_067_926_047, fees).unwrap();
    assert_eq!(
        (q.sol_cost, q.protocol_fee, q.creator_fee, q.tokens),
        (85_005_359_057, 807_550_912, 255_016_078, 793_099_999_998_038)
    );
    assert_eq!(q.total, 86_067_926_047);
    assert_eq!(fresh.quote_buy_exact_sol_in(86_067_926_048, fees), None);
    assert_eq!(fresh.quote_buy_exact_sol_in(u64::MAX, fees), None);
    assert_eq!(
        fresh.tokens_for_sol(u64::MAX),
        curve::INITIAL_REAL_TOKEN_RESERVES
    );

    // u64::MAX everywhere: no panic (debug build checks overflow), sane results.
    let max = CurveState {
        virtual_sol: u64::MAX,
        virtual_token: u64::MAX,
        real_sol: u64::MAX,
        real_token: u64::MAX,
        complete: false,
        creator,
    };
    assert_eq!(max.buy_cost(u64::MAX - 1), None); // cost overflows u64
    assert_eq!(max.buy_cost(u64::MAX), None); // n >= T
    assert_eq!(max.sell_sol_out(u64::MAX), u64::MAX / 2);
    assert_eq!(max.tokens_for_sol(u64::MAX), u64::MAX / 2);
    let fat = FeeBps {
        protocol: u64::MAX,
        creator: u64::MAX,
    };
    assert_eq!(max.quote_buy_exact_sol_in(u64::MAX, fat), None);
    assert_eq!(max.quote_sell(u64::MAX, fat), None); // fees exceed the output
    assert_eq!(curve::fee_ceil(u64::MAX, u64::MAX), u64::MAX);
    assert_eq!(curve::buyback_fee(u64::MAX, u64::MAX), u64::MAX);
    assert_eq!(curve::exact_in_split(u64::MAX, fat), None);
    assert!(max.quote_buy_exact_tokens(u64::MAX, fat).is_none());
    let q = max.quote_buy_exact_tokens(1, FeeBps::ZERO).unwrap();
    assert_eq!(q.sol_cost, 2); // floor(1*V/(T-1)) + 1 with V = T
    // zero reserves
    let zero = CurveState {
        virtual_sol: 0,
        virtual_token: 0,
        real_sol: 0,
        real_token: 0,
        complete: false,
        creator,
    };
    assert_eq!(zero.tokens_for_sol(0), 0);
    assert_eq!(zero.sell_sol_out(0), 0);
    assert_eq!(zero.buy_cost(1), None);
    assert_eq!(zero.market_cap_lamports(curve::TOKEN_TOTAL_SUPPLY), 0);
    assert_eq!(zero.quote_buy_exact_sol_in(1_000, fees), None);
}

#[test]
fn no_trade_ever_lowers_k() {
    // Every rounding goes the curve's way: (V', T') after any buy or sell
    // satisfies V' * T' >= V * T. Reserves < 2^62 keep the products in u128.
    let mut rng = Rng(0x00C0_FFEE_0000_0001);
    let fees = FeeBps::DEFAULT;
    for _ in 0..iters(400) * 50 {
        let mut s = CurveState {
            virtual_sol: 1 + (rng.next() >> (2 + rng.below(40))),
            virtual_token: 1 + (rng.next() >> (2 + rng.below(30))),
            real_sol: u64::MAX >> 2,
            real_token: 0,
            complete: false,
            creator: Pubkey::new_from_array([1; 32]),
        };
        s.real_token = s.virtual_token / 2;
        for _ in 0..8 {
            let k = s.virtual_sol as u128 * s.virtual_token as u128;
            let x = rng.next() >> rng.below(64);
            let before = s;
            match rng.below(3) {
                0 => {
                    if let Some(q) = s.quote_buy_exact_tokens(x, fees) {
                        s.apply_buy(q.tokens, q.sol_cost);
                        assert_eq!(q.total, q.sol_cost + q.protocol_fee + q.creator_fee);
                    }
                }
                1 => {
                    if let Some(q) = s.quote_buy_exact_sol_in(x, fees) {
                        assert!(q.total <= x && q.tokens <= before.real_token);
                        s.apply_buy(q.tokens, q.sol_cost);
                    }
                }
                _ => {
                    let n = x % (before.virtual_token / 4 + 1);
                    if let Some(q) = s.quote_sell(n, fees) {
                        assert!(q.sol_out < before.virtual_sol || n == 0);
                        s.apply_sell(n, q.sol_out);
                    }
                }
            }
            let k2 = s.virtual_sol as u128 * s.virtual_token as u128;
            assert!(k2 >= k, "k fell: {before:?} -> {s:?}");
        }
    }
}

// --------------------------------------------- fixtures of every kind
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Summary of a classified instruction ("pump.<ix>/<n accounts>", ...).
fn summary(cl: Classified<'_>) -> Option<String> {
    use pump_codec::instructions::AmmInstruction as A;
    Some(match cl {
        Classified::Pump(PumpInstruction::Create(c)) => {
            format!("pump.create:{:?}/{}", c.kind, c.num_accounts)
        }
        Classified::Pump(PumpInstruction::Trade(t)) => {
            format!("pump.{}/{}", t.kind.name(), t.num_accounts)
        }
        Classified::Pump(PumpInstruction::Other { discriminator }) => {
            format!("pump.other:{}", hex(&discriminator))
        }
        Classified::PumpSwap(A::Trade(t)) => format!("amm.{:?}/{}", t.kind, t.num_accounts),
        Classified::PumpSwap(A::CreatePool(_)) => "amm.create_pool".into(),
        Classified::PumpSwap(A::Other { discriminator }) => {
            format!("amm.other:{}", hex(&discriminator))
        }
        Classified::NotPump => return None,
    })
}

#[test]
fn fresh_fixtures_classify_like_an_independent_decoder() {
    // Expectations from a separate Python wire decoder (discriminator name +
    // account count, top-level then CPI order) used to pick the fixtures.
    let expected: &[(&str, &[&str], &[&str])] = &[
        ("fresh_amm_inner_bexq_v2", &[], &["amm.BuyExactQuoteInV2/17"]),
        ("fresh_amm_inner_sell_v2", &[], &["amm.SellV2/17"]),
        ("fresh_amm_sell_then_buy", &["amm.Sell/23", "amm.Buy/25"], &[]),
        ("fresh_legacy_inner_buy18", &[], &["pump.buy/18"]),
        (
            "fresh_v0_create_v2_quote_tail_no_buy",
            &["pump.create:V2/20"],
            &[],
        ),
        ("fresh_v0_sell16", &["pump.sell/16"], &[]),
        (
            "fresh_v0alt2_inner_bexq_v3",
            &[],
            &["pump.buy_exact_quote_in_v3/17"],
        ),
        ("fresh_v0alt3_inner_sell_v3", &[], &["pump.sell_v3/17"]),
        ("fresh_v0alt4_inner_buy_v3", &[], &["pump.buy_v3/17"]),
        (
            "fresh_v0alt_create_v2_buy18",
            &["pump.create:V2/16", "pump.buy/18"],
            &[],
        ),
        (
            "fresh_v0alt_create_v2_buy_v3",
            &["pump.create:V2/16", "pump.buy_v3/17"],
            &[],
        ),
        (
            "fresh_v0alt_create_v2_bxsi",
            &["pump.create:V2/16", "pump.buy_exact_sol_in/18"],
            &[],
        ),
        (
            "fresh_v0alt_create_v2_inner_buy",
            &["pump.create:V2/16"],
            &["pump.buy/18"],
        ),
        (
            "fresh_v0alt_create_v2_tail3_extend_bxsi",
            &[
                "pump.create:V2/19",
                "pump.other:ea66c2cb96483ee5",
                "pump.buy_exact_sol_in/18",
            ],
            &[],
        ),
        (
            "fresh_v0alt_distribute_creator_fees",
            &["amm.other:8b348655e4e56cf1", "pump.other:a572670079cef751"],
            &[],
        ),
        ("fresh_v0alt_inner_sell_v2", &[], &["pump.sell_v2/26"]),
        (
            "fresh_v1_big_inner_buy_v3_uva",
            &[],
            &[
                "pump.other:5e06ca73ff60e8b7",
                "pump.buy_v3/17",
                "pump.other:f945a4da9667548a",
            ],
        ),
        (
            "fresh_v1_cpi_create_v2_bexq_v2",
            &[],
            &["pump.create:V2/19", "pump.buy_exact_quote_in_v2/27"],
        ),
        (
            "fresh_v1_create_v2_mayhem_buy_v2",
            &["pump.create:V2/16", "pump.buy_v2/27"],
            &[],
        ),
        (
            "fresh_v1_create_v2_pumpquote_buy_v2",
            &["pump.create:V2/20", "pump.buy_v2/27"],
            &[],
        ),
        (
            "fresh_v1_create_v2_usdc_buy_v2",
            &["pump.create:V2/19", "pump.buy_v2/27"],
            &[],
        ),
        (
            "fresh_v1_inner_bexq_v2",
            &[],
            &["pump.buy_exact_quote_in_v2/27"],
        ),
        ("fresh_v1_inner_sell_v3", &[], &["pump.sell_v3/17"]),
        (
            "fresh_v1_sell_v3_then_amm_sell_v2",
            &[],
            &["pump.sell_v3/17", "amm.SellV2/17"],
        ),
        (
            "legacy_create_spl_b",
            &[
                "pump.create:Legacy/14",
                "pump.other:ea66c2cb96483ee5",
                "pump.buy/18",
            ],
            &[],
        ),
    ];
    for (name, want_top, want_inner) in expected {
        let fx = TxFixture::load(name);
        let tx = fx.view();
        let keys = fx.full_keys();
        let top: Vec<String> = tx
            .instructions()
            .filter_map(|ix| {
                let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
                summary(classify(tx.program_id(&ix), ix.data, &a).unwrap())
            })
            .collect();
        let inner: Vec<String> = fx
            .inner
            .iter()
            .filter(|(_, _, _, d)| !d.starts_with(&c::EVENT_IX_TAG))
            .filter_map(|(_, p, accts, data)| {
                let ak: Vec<Pubkey> = accts.iter().map(|&k| keys[k as usize]).collect();
                summary(classify(&keys[*p as usize], data, ak.as_slice()).unwrap())
            })
            .collect();
        assert_eq!(&top, want_top, "{name}: top-level");
        assert_eq!(&inner, want_inner, "{name}: CPI");
    }
}

#[test]
fn fresh_fixture_events_in_order() {
    let expected: &[(&str, &[&str])] = &[
        ("fresh_amm_inner_bexq_v2", &["BuyEvent"]),
        ("fresh_amm_inner_sell_v2", &["SellEvent"]),
        ("fresh_amm_sell_then_buy", &["SellEvent", "BuyEvent"]),
        ("fresh_legacy_inner_buy18", &["TradeEvent"]),
        ("fresh_v0_create_v2_quote_tail_no_buy", &["CreateEvent"]),
        ("fresh_v0_sell16", &["TradeEvent"]),
        ("fresh_v0alt2_inner_bexq_v3", &["TradeEvent"]),
        ("fresh_v0alt3_inner_sell_v3", &["TradeEvent"]),
        ("fresh_v0alt4_inner_buy_v3", &["TradeEvent"]),
        ("fresh_v0alt_create_v2_buy18", &["CreateEvent", "TradeEvent"]),
        ("fresh_v0alt_create_v2_buy_v3", &["CreateEvent", "TradeEvent"]),
        ("fresh_v0alt_create_v2_bxsi", &["CreateEvent", "TradeEvent"]),
        ("fresh_v0alt_create_v2_inner_buy", &["CreateEvent", "TradeEvent"]),
        (
            "fresh_v0alt_create_v2_tail3_extend_bxsi",
            &["CreateEvent", "TradeEvent"],
        ),
        ("fresh_v0alt_distribute_creator_fees", &[]),
        ("fresh_v0alt_inner_sell_v2", &["TradeEvent"]),
        ("fresh_v1_big_inner_buy_v3_uva", &["TradeEvent"]),
        ("fresh_v1_cpi_create_v2_bexq_v2", &["CreateEvent", "TradeEvent"]),
        (
            "fresh_v1_create_v2_mayhem_buy_v2",
            &["CreateEvent", "TradeEvent"],
        ),
        (
            "fresh_v1_create_v2_pumpquote_buy_v2",
            &["CreateEvent", "TradeEvent"],
        ),
        ("fresh_v1_create_v2_usdc_buy_v2", &["CreateEvent", "TradeEvent"]),
        ("fresh_v1_inner_bexq_v2", &["TradeEvent"]),
        ("fresh_v1_inner_sell_v3", &["TradeEvent"]),
        ("fresh_v1_sell_v3_then_amm_sell_v2", &["TradeEvent", "SellEvent"]),
        ("legacy_create_spl_b", &["CreateEvent", "TradeEvent"]),
    ];
    for (name, want) in expected {
        let fx = TxFixture::load(name);
        let keys = fx.full_keys();
        let got: Vec<&str> = events_from_inner_instructions(&keys, fx.inner_instructions(), true)
            .unwrap()
            .iter()
            .filter(|r| !matches!(r.event, PumpEvent::Other { .. }))
            .map(|r| r.event.name())
            .collect();
        assert_eq!(&got, want, "{name}");
    }
}

/// The pump instruction that emitted a CreateEvent: the top-level ix at
/// `outer`, or the first pump create among its CPIs.
fn create_ix<'f>(
    fx: &'f TxFixture,
    outer: u8,
) -> (pump_codec::instructions::PumpCreate<'f>, Vec<Pubkey>) {
    let tx = fx.view();
    let keys = fx.full_keys();
    let ix = tx.instruction(outer as usize).unwrap();
    let full: Vec<Pubkey> = ix.accounts.iter().map(|&i| keys[i as usize]).collect();
    if let Classified::Pump(PumpInstruction::Create(c)) =
        classify(tx.program_id(&ix), ix.data, full.as_slice()).unwrap()
    {
        return (c, full);
    }
    for (o, p, accts, data) in &fx.inner {
        if *o != outer {
            continue;
        }
        let ak: Vec<Pubkey> = accts.iter().map(|&k| keys[k as usize]).collect();
        if let Classified::Pump(PumpInstruction::Create(c)) =
            classify(&keys[*p as usize], data, ak.as_slice()).unwrap()
        {
            return (c, ak);
        }
    }
    panic!("{}: no create under outer ix {outer}", fx.name)
}

#[test]
fn every_create_links_to_its_instruction_and_keys() {
    use pump_codec::instructions::{CreateKind, UserKeys};
    let mut n = 0;
    let mut cpi = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let keys = fx.full_keys();
        let evs = events_from_inner_instructions(&keys, fx.inner_instructions(), true).unwrap();
        for r in &evs {
            let PumpEvent::Create(ce) = &r.event else {
                continue;
            };
            n += 1;
            let (cr, accts) = create_ix(&fx, r.outer_index);
            let top_is_pump = *fx.view().program_id(
                &fx.view().instruction(r.outer_index as usize).unwrap(),
            ) == c::PUMP_PROGRAM_ID;
            if !top_is_pump {
                cpi += 1;
            }
            assert_eq!(
                (cr.args.name, cr.args.symbol, cr.args.uri),
                (ce.name, ce.symbol, ce.uri),
                "{name}"
            );
            assert_eq!(cr.args.creator, ce.creator, "{name}");
            assert_eq!(cr.mint, Some(ce.mint), "{name}");
            assert_eq!(cr.bonding_curve, Some(ce.bonding_curve), "{name}");
            assert_eq!(cr.user, Some(ce.user), "{name}");
            assert_eq!(cr.token_program, ce.token_program, "{name}");
            assert_eq!(pda::bonding_curve(&ce.mint), ce.bonding_curve);
            let tp = ce.token_program.unwrap();
            assert_eq!(
                cr.associated_bonding_curve,
                Some(pda::associated_token_address(&ce.bonding_curve, &ce.mint, &tp)),
                "{name}"
            );
            let sol = ce.quote_mint.map(|q| c::is_sol_like_quote_mint(&q));
            match cr.kind {
                CreateKind::V2 => {
                    assert_eq!(tp, c::TOKEN_2022_PROGRAM_ID, "{name}");
                    assert_eq!(Some(cr.args.is_mayhem_mode), ce.is_mayhem_mode, "{name}");
                    assert_eq!(cr.is_sol_quote, sol, "{name}: SOL classification");
                    assert!(matches!(cr.quote_tail_len, 0 | 3 | 4), "{name}");
                    if cr.quote_tail_len > 0 {
                        // the tail's quote mint is the event's (wSOL for a SOL coin
                        // created with an explicit tail, where the event says 1111...)
                        let q = cr.quote_mint.unwrap();
                        assert!(
                            Some(q) == ce.quote_mint
                                || (q == c::WSOL_MINT && ce.quote_mint == Some(Pubkey::default())),
                            "{name}: tail quote {q} vs event {:?}",
                            ce.quote_mint
                        );
                    }
                    if cr.quote_tail_len == 4 {
                        assert_eq!(accts.account(19), Some(pda::quote_control()), "{name}");
                    }
                }
                CreateKind::Legacy => {
                    assert_eq!(tp, c::SPL_TOKEN_PROGRAM_ID, "{name}");
                    assert_eq!(cr.num_accounts, 14, "{name}");
                    assert_eq!(accts.account(6), Some(pda::metadata(&ce.mint)), "{name}");
                    assert_eq!(sol, Some(true), "{name}");
                }
            }
            // Hot path: keys from the create instruction == full derivation.
            let user = Pubkey::new_from_array([0xab; 32]);
            let bb = c::BUYBACK_FEE_RECIPIENTS[1];
            match SolV3Keys::from_create(&cr, &UserKeys::derive(user), bb) {
                Some(k) => {
                    assert_eq!(sol, Some(true), "{name}");
                    assert_eq!(k, SolV3Keys::derive(ce.mint, user, tp, bb), "{name}");
                }
                None => assert_eq!(sol, Some(false), "{name}: SOL coin without v3 keys"),
            }
        }
    }
    assert!(n >= 20, "{n} creates");
    assert!(cpi >= 1, "a create made through a launcher CPI");
}

#[test]
fn every_sol_create_starts_fresh_and_trades_chain_mayhem_included() {
    let mut creates = 0;
    let mut mayhem = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let keys = fx.full_keys();
        let evs = events_from_inner_instructions(&keys, fx.inner_instructions(), true).unwrap();
        let Some(ce) = evs.iter().find_map(|r| match &r.event {
            PumpEvent::Create(c) => Some(c.clone()),
            _ => None,
        }) else {
            continue;
        };
        if ce.quote_mint.is_some_and(|q| !c::is_sol_like_quote_mint(&q)) {
            continue;
        }
        creates += 1;
        assert_eq!(
            (
                ce.virtual_sol_reserves,
                ce.virtual_token_reserves,
                ce.real_token_reserves,
                ce.token_total_supply
            ),
            (
                curve::INITIAL_VIRTUAL_SOL_RESERVES,
                curve::INITIAL_VIRTUAL_TOKEN_RESERVES,
                curve::INITIAL_REAL_TOKEN_RESERVES,
                curve::TOKEN_TOTAL_SUPPLY
            ),
            "{name}"
        );
        let mut state = CurveState::fresh(ce.creator);
        for t in evs.iter().filter_map(|r| match &r.event {
            PumpEvent::Trade(t) if t.mint == ce.mint => Some(t),
            _ => None,
        }) {
            if t.mayhem_mode == Some(true) {
                mayhem += 1;
            }
            assert_eq!(t.pre_state().unwrap(), state, "{name}: chain");
            state = t.post_state().unwrap();
        }
    }
    assert!(creates >= 15, "{creates}");
    assert!(mayhem >= 2, "mayhem create+buy fixtures: {mayhem}");
}

/// (trade instruction, its full account keys, TradeEvent) of a fixture, in
/// execution order (each top-level ix, then its CPIs).
fn trades<'f>(fx: &'f TxFixture) -> Vec<(PumpTrade, Vec<Pubkey>, Vec<u8>, TradeEvent<'f>)> {
    let keys = fx.full_keys();
    let tx = fx.view();
    let mut out = Vec::new();
    for (i, ix) in tx.instructions().enumerate() {
        let full: Vec<Pubkey> = ix.accounts.iter().map(|&k| keys[k as usize]).collect();
        if let Classified::Pump(PumpInstruction::Trade(t)) =
            classify(tx.program_id(&ix), ix.data, full.as_slice()).unwrap()
        {
            out.push((t, full, ix.accounts.to_vec()));
        }
        for (o, p, accts, data) in &fx.inner {
            if *o as usize != i {
                continue;
            }
            let ak: Vec<Pubkey> = accts.iter().map(|&k| keys[k as usize]).collect();
            if let Classified::Pump(PumpInstruction::Trade(t)) =
                classify(&keys[*p as usize], data, ak.as_slice()).unwrap()
            {
                out.push((t, ak, accts.clone()));
            }
        }
    }
    let evs: Vec<TradeEvent<'f>> =
        events_from_inner_instructions(&keys, fx.inner_instructions(), true)
            .unwrap()
            .into_iter()
            .filter_map(|r| match r.event {
                PumpEvent::Trade(t) => Some(t),
                _ => None,
            })
            .collect();
    assert_eq!(out.len(), evs.len(), "{}", fx.name);
    out.into_iter()
        .zip(evs)
        .map(|((t, k, idx), e)| (t, k, idx, e))
        .collect()
}

#[test]
fn every_onchain_trade_matches_our_builders_account_for_account() {
    let (mut v3, mut legacy, mut non_ata) = (0, 0, 0);
    let mut seen = std::collections::BTreeSet::new();
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let tx = fx.view();
        for (t, onchain, idx, ev) in trades(&fx) {
            let sol = ev.quote_mint == Some(Pubkey::default());
            let mint = t.mint.unwrap();
            let user = t.user.unwrap();
            let metas = match t.kind.layout() {
                TradeLayout::V3 if sol => {
                    let k = SolV3Keys::derive(
                        mint,
                        user,
                        t.base_token_program.unwrap(),
                        t.buyback_fee_recipient.unwrap(),
                    );
                    assert!(c::BUYBACK_FEE_RECIPIENTS.contains(&k.buyback_fee_recipient));
                    v3 += 1;
                    match t.kind {
                        PumpTradeKind::BuyV3 => ixs::buy_v3(&k, 1, 1, None).accounts,
                        PumpTradeKind::SellV3 => ixs::sell_v3(&k, 1, 1).accounts,
                        _ => ixs::buy_exact_quote_in_v3(&k, 1, 1, None).accounts,
                    }
                }
                TradeLayout::LegacyBuy | TradeLayout::LegacySell => {
                    let k = LegacyTradeKeys::derive(
                        mint,
                        user,
                        ev.creator.unwrap(),
                        t.base_token_program.unwrap(),
                        t.fee_recipient.unwrap(),
                        t.buyback_fee_recipient.unwrap(),
                    );
                    legacy += 1;
                    let mut m = if t.kind == PumpTradeKind::Sell {
                        // 17 accounts = cashback coin (user_volume_accumulator first)
                        let cashback = onchain.len() >= 17
                            && onchain[14] == pda::user_volume_accumulator(&user);
                        k.sell_metas(cashback)
                    } else {
                        k.buy_metas()
                    };
                    // the program accepts any user token account; compare the rest
                    if onchain[5] != k.associated_user {
                        non_ata += 1;
                        m[5].pubkey = onchain[5];
                    }
                    m
                }
                _ => continue, // v2 / token-quoted v3: no builder (parse only)
            };
            seen.insert(t.kind.name());
            let ours: Vec<Pubkey> = metas.iter().map(|m| m.pubkey).collect();
            // senders may append extra accounts (e.g. legacy_buy19)
            assert_eq!(
                &onchain[..ours.len()],
                ours.as_slice(),
                "{name}: {} accounts",
                t.kind.name()
            );
            // The tx-level privileges cover every account we mark writable or
            // signer (a CPI can only narrow privileges).
            for (m, &i) in metas.iter().zip(&idx) {
                if m.is_writable {
                    assert!(tx.is_writable(i as usize), "{name}: {} not writable", m.pubkey);
                }
                if m.is_signer {
                    assert!(tx.is_signer(i as usize), "{name}: {} not a signer", m.pubkey);
                }
            }
        }
    }
    for k in ["buy_v3", "sell_v3", "buy_exact_quote_in_v3", "buy", "buy_exact_sol_in", "sell"] {
        assert!(seen.contains(k), "no on-chain {k} checked: {seen:?}");
    }
    assert!(v3 >= 8 && legacy >= 12, "v3 {v3}, legacy {legacy}");
    assert!(non_ata < legacy, "non-ATA user accounts {non_ata}/{legacy}");
}

#[test]
fn whitelisted_mayhem_agent_trades_fee_free() {
    // Global.whitelist_pda == the mayhem sol-vault: its trades carry 0 bps.
    let v = read_json("accounts/global.json");
    use base64::Engine as _;
    let d = base64::engine::general_purpose::STANDARD
        .decode(v["result"]["value"]["data"][0].as_str().unwrap())
        .unwrap();
    let g = curve::Global::decode(&d).unwrap();
    assert_eq!(g.whitelist_pda, c::MAYHEM_SOL_VAULT);
    let fx = TxFixture::load("fresh_v0alt_inner_sell_v2");
    let (t, _, _, ev) = trades(&fx).pop().unwrap();
    assert_eq!(t.kind, PumpTradeKind::SellV2);
    assert_eq!(ev.user, g.whitelist_pda);
    assert_eq!(ev.mayhem_mode, Some(true));
    assert_eq!(ev.quote_mint, Some(c::USDC_MINT));
    assert_eq!(
        (ev.fee_basis_points, ev.creator_fee_basis_points, ev.fee, ev.creator_fee),
        (Some(0), Some(0), Some(0), Some(0))
    );
}
