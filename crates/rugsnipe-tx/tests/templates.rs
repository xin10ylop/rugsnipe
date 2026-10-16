//! PatchTable output == building the instruction set from scratch, for
//! 1,000 random mints (legacy and v1, with and without nonce, with and
//! without a second signer); == pump-rust-client 0.4.1 account lists for
//! 100 mints; byte-exact layout against solana-message's own legacy and v1
//! serializers; round trip through `pump_codec::tx::TxView`.

#![allow(deprecated)]

use pump_codec::constants as c;
use pump_codec::instructions::{
    classify, Classified, PumpInstruction, PumpTradeKind, SolV3Keys, TradeAmounts,
};
use pump_codec::tx::{TxVersion, TxView};
use pump_codec::{pda, Instruction, Pubkey};
use rugsnipe_core::config::TxFormat;
use rugsnipe_tx::msg::{compile, Format, KeySpec, V1Config};
use rugsnipe_tx::template::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn key(&mut self) -> Pubkey {
        let mut b = [0u8; 32];
        for ch in b.chunks_mut(8) {
            ch.copy_from_slice(&self.next().to_le_bytes());
        }
        Pubkey::new_from_array(b)
    }
}

/// (program, [(key, signer, writable)], data) per instruction, flags from the header.
type Semantic = Vec<(Pubkey, Vec<(Pubkey, bool, bool)>, Vec<u8>)>;

fn decompile(raw: &[u8]) -> Semantic {
    let tx = TxView::parse(raw).unwrap();
    let keys = tx.static_account_keys();
    tx.instructions()
        .map(|ix| {
            (
                *tx.program_id(&ix),
                ix.accounts
                    .iter()
                    .map(|&i| {
                        (
                            keys[i as usize],
                            tx.is_signer(i as usize),
                            tx.is_writable(i as usize),
                        )
                    })
                    .collect(),
                ix.data.to_vec(),
            )
        })
        .collect()
}

/// Reference: Solana's own compiler (sorted key order) serialized, then decompiled.
fn reference(
    ixs: &[Instruction],
    payer: &Pubkey,
    lifetime: [u8; 32],
    format: TxFormat,
    cu: u32,
    lads: u32,
    prio: u64,
) -> Semantic {
    let hash = solana_hash::Hash::new_from_array(lifetime);
    let msg = match format {
        TxFormat::Legacy => {
            let m = solana_message::legacy::Message::new_with_blockhash(ixs, Some(payer), &hash);
            let n = m.header.num_required_signatures as usize;
            let mut raw = vec![n as u8];
            raw.extend(std::iter::repeat_n(0u8, 64 * n));
            raw.extend(m.serialize());
            raw
        }
        TxFormat::V1 => {
            let cfg = solana_message::v1::TransactionConfig::empty()
                .with_priority_fee(prio)
                .with_compute_unit_limit(cu)
                .with_loaded_accounts_data_size_limit(lads);
            let m = solana_message::v1::Message::try_compile_with_config(payer, ixs, hash, cfg)
                .unwrap();
            let n = m.header.num_required_signatures as usize;
            let mut raw = m.serialize();
            raw.extend(std::iter::repeat_n(0u8, 64 * n));
            raw
        }
    };
    decompile(&msg)
}

fn params(rng: &mut Rng, format: TxFormat, nonce: bool, sep_authority: bool) -> CurveParams {
    let user = rng.key();
    CurveParams {
        format,
        user,
        nonce: nonce.then(|| NonceKeys {
            account: rng.key(),
            authority: if sep_authority { rng.key() } else { user },
        }),
        cu_limit: if nonce { 72_150 } else { 72_000 },
        lads_bytes: 12_124_160,
        tip_account: Some(c::JITO_TIP_ACCOUNTS[(rng.next() % 8) as usize]),
        buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[(rng.next() % 8) as usize],
        base_token_program: c::TOKEN_2022_PROGRAM_ID,
        partial_fill: Some(false),
    }
}

fn real_keys(mint: Pubkey, p: &CurveParams) -> MintKeys {
    MintKeys::from_v3(&SolV3Keys::derive(
        mint,
        p.user,
        p.base_token_program,
        p.buyback_fee_recipient,
    ))
}

#[test]
fn patched_buy_equals_scratch_build_for_1000_mints() {
    // Templates are compiled with the configured floor; every mint patches
    // its own CU limit (65,000 + 1,500*S over the floor, gap-5) and must
    // equal a from-scratch build whose CurveParams carry that limit. Floors
    // vary so both the formula and the clamp are exercised.
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let combos = [
        (TxFormat::Legacy, false, false, 65_000),
        (TxFormat::Legacy, true, false, 1),
        (TxFormat::Legacy, true, true, 88_000),
        (TxFormat::V1, false, false, 65_000),
        (TxFormat::V1, true, false, 70_000),
        (TxFormat::V1, true, true, 88_000),
    ];
    let mut templates: Vec<(CurveParams, Template)> = combos
        .iter()
        .map(|&(f, n, s, floor)| {
            let mut p = params(&mut rng, f, n, s);
            p.cu_limit = floor;
            (p, Template::buy(&p).unwrap())
        })
        .collect();
    let (mut above_floor, mut at_floor) = (0, 0);
    for i in 0..1_000 {
        let mint = rng.key();
        let (p, t) = &mut templates[i % combos.len()];
        let k = real_keys(mint, p);
        let floor = p.cu_limit;
        // per-mint limit from the bumps recovered from the keys (hot path) ==
        // from a from-scratch derivation of the four PDAs
        let w = rugsnipe_tx::cu::wallet_missing_bumps(&p.user);
        let s = pda::SnipeBumps::derive(&mint, &p.user, &p.base_token_program).missing() + w;
        let cu = k
            .snipe_cu_limit(&p.user, &p.base_token_program, w, floor)
            .unwrap();
        assert_eq!(cu, rugsnipe_tx::cu::snipe_cu_limit(s, floor));
        assert_eq!(cu, (65_000 + 1_500 * s).clamp(floor, 1_400_000));
        if cu > floor {
            above_floor += 1;
        } else {
            at_floor += 1;
        }
        let lifetime = {
            let mut b = [0u8; 32];
            b[..8].copy_from_slice(&rng.next().to_le_bytes());
            b[31] = 7;
            b
        };
        let (spend, min_out, prio, tip) = (
            rng.next() % 1_000_000_000 + 1,
            rng.next(),
            rng.next() % 50_000_000,
            rng.next() % 10_000_000,
        );
        t.patch(&Patch {
            keys: &k,
            lifetime: &lifetime,
            amount: spend,
            limit: min_out,
            priority_lamports: prio,
            tip_lamports: tip,
            cu_limit: Some(cu),
        });
        assert_eq!(t.cu_limit(), cu);
        let scratch = CurveParams { cu_limit: cu, ..*p };
        let ixs = buy_instructions(&scratch, &k, spend, min_out, prio, tip);
        let want = reference(&ixs, &p.user, lifetime, p.format, cu, p.lads_bytes, prio);
        let got = decompile(t.bytes());
        assert_eq!(got, want, "mint #{i} {mint} {:?} cu {cu}", p.format);
        // byte-exact too: a template compiled with this mint's limit
        let mut fresh = Template::buy(&scratch).unwrap();
        fresh.patch(&Patch {
            keys: &k,
            lifetime: &lifetime,
            amount: spend,
            limit: min_out,
            priority_lamports: prio,
            tip_lamports: tip,
            cu_limit: None,
        });
        assert_eq!(t.bytes(), fresh.bytes(), "mint #{i} {:?}", p.format);
        let tx = TxView::parse(t.bytes()).unwrap();
        assert_eq!(tx.recent_blockhash(), &lifetime);
        let cb = tx.compute_budget();
        assert_eq!(cb.compute_unit_limit, Some(cu));
        assert_eq!(cb.loaded_accounts_data_size_limit, Some(p.lads_bytes));
        match p.format {
            TxFormat::V1 => assert_eq!(cb.priority_fee_lamports, Some(prio)),
            // legacy: the price follows the per-mint limit; never above the total
            TxFormat::Legacy => {
                let price = cb.compute_unit_price_micro_lamports.unwrap();
                assert_eq!(price, legacy_price(prio, cu));
                assert!(price as u128 * cu as u128 / 1_000_000 <= prio as u128);
            }
        }
    }
    assert!(
        above_floor > 100 && at_floor > 100,
        "{above_floor} {at_floor}"
    );
}

#[test]
fn set_cu_limit_keeps_the_priority_total() {
    let mut rng = Rng(77);
    for f in [TxFormat::Legacy, TxFormat::V1] {
        let p = params(&mut rng, f, true, false);
        let mut t = Template::buy(&p).unwrap();
        let k = real_keys(rng.key(), &p);
        t.patch(&Patch {
            keys: &k,
            lifetime: &[2; 32],
            amount: 1,
            limit: 1,
            priority_lamports: 3_400_000,
            tip_lamports: 0,
            cu_limit: Some(71_000),
        });
        // a variant's bid change keeps the per-mint limit
        t.set_priority(1_000_000);
        let cb = TxView::parse(t.bytes()).unwrap().compute_budget();
        assert_eq!(cb.compute_unit_limit, Some(71_000));
        if f == TxFormat::Legacy {
            assert_eq!(
                cb.compute_unit_price_micro_lamports,
                Some(legacy_price(1_000_000, 71_000))
            );
        }
        // a limit change re-derives the legacy price from the same total
        t.set_cu_limit(89_000);
        let cb = TxView::parse(t.bytes()).unwrap().compute_budget();
        assert_eq!(cb.compute_unit_limit, Some(89_000));
        assert_eq!(t.priority_lamports(), 1_000_000);
        match f {
            TxFormat::Legacy => assert_eq!(
                cb.compute_unit_price_micro_lamports,
                Some(legacy_price(1_000_000, 89_000))
            ),
            TxFormat::V1 => assert_eq!(cb.priority_fee_lamports, Some(1_000_000)),
        }
        assert!(t.table.cu_limit.is_some());
    }
}

#[test]
fn patched_sells_equal_scratch_build() {
    let mut rng = Rng(42);
    for &(f, n, close) in &[
        (TxFormat::Legacy, false, true),
        (TxFormat::Legacy, true, false),
        (TxFormat::V1, false, true),
        (TxFormat::V1, true, true),
    ] {
        let p = params(&mut rng, f, n, false);
        let mut t = Template::sell(&p, close).unwrap();
        for _ in 0..50 {
            let k = real_keys(rng.key(), &p);
            let lt = [rng.next() as u8; 32];
            let (tok, min, prio, tip) = (rng.next(), rng.next(), rng.next() % 1_000_000, 1_000_000);
            t.patch(&Patch {
                keys: &k,
                lifetime: &lt,
                amount: tok,
                limit: min,
                priority_lamports: prio,
                tip_lamports: tip,
                cu_limit: None,
            });
            let want = reference(
                &sell_instructions(&p, &k, close, tok, min, prio, tip),
                &p.user,
                lt,
                f,
                p.cu_limit,
                p.lads_bytes,
                prio,
            );
            assert_eq!(decompile(t.bytes()), want);
        }
    }
    // PumpSwap canonical-pool sell
    for f in [TxFormat::Legacy, TxFormat::V1] {
        let user = rng.key();
        let ap = AmmSellParams {
            format: f,
            user,
            nonce: None,
            cu_limit: 120_000,
            lads_bytes: 16_000_000,
            tip_account: Some(c::JITO_TIP_ACCOUNTS[0]),
            buyback_wallet: c::BUYBACK_FEE_RECIPIENTS[3],
            base_token_program: c::TOKEN_2022_PROGRAM_ID,
        };
        let mut t = Template::amm_sell(&ap).unwrap();
        for _ in 0..20 {
            let k = amm_mint_keys(&rng.key(), &user, &c::TOKEN_2022_PROGRAM_ID);
            let lt = [9u8; 32];
            t.patch(&Patch {
                keys: &k,
                lifetime: &lt,
                amount: 5,
                limit: 6,
                priority_lamports: 7,
                tip_lamports: 8,
                cu_limit: None,
            });
            let want = reference(
                &amm_sell_instructions(&ap, &k, 5, 6, 7, 8),
                &user,
                lt,
                f,
                ap.cu_limit,
                ap.lads_bytes,
                7,
            );
            assert_eq!(decompile(t.bytes()), want);
        }
    }
}

fn p3<P: From<[u8; 32]>>(k: &Pubkey) -> P {
    P::from(k.to_bytes())
}

#[test]
fn buy_accounts_equal_pump_rust_client_for_100_mints() {
    let mut rng = Rng(7);
    let p = params(&mut rng, TxFormat::V1, true, false);
    let mut t = Template::buy(&p).unwrap();
    for _ in 0..100 {
        let mint = rng.key();
        let k = real_keys(mint, &p);
        t.patch(&Patch {
            keys: &k,
            lifetime: &[1; 32],
            amount: 10_000_000,
            limit: 1,
            priority_lamports: 0,
            tip_lamports: 1_000,
            cu_limit: None,
        });
        let sdk = pump_rust_client::PumpSdk.buy_exact_quote_in_v3_instruction(
            p3(&mint),
            p3(&c::WSOL_MINT),
            p3(&c::TOKEN_2022_PROGRAM_ID),
            p3(&c::SPL_TOKEN_PROGRAM_ID),
            p3(&p.user),
            p3(&p.buyback_fee_recipient),
            10_000_000,
            1,
        );
        let sdk_metas: Vec<([u8; 32], bool, bool)> = sdk
            .accounts
            .iter()
            .map(|m| (m.pubkey.to_bytes(), m.is_signer, m.is_writable))
            .collect();
        let sem = decompile(t.bytes());
        let (prog, metas, data) = sem
            .iter()
            .find(|(prog, _, _)| *prog == c::PUMP_PROGRAM_ID)
            .unwrap();
        assert_eq!(prog.to_bytes(), sdk.program_id.to_bytes());
        let ours: Vec<([u8; 32], bool, bool)> = metas
            .iter()
            .map(|(k, s, w)| (k.to_bytes(), *s, *w))
            .collect();
        assert_eq!(ours, sdk_metas, "mint {mint}");
        assert_eq!(&data[..24], &sdk.data[..24]);
    }
}

#[test]
fn layout_is_byte_exact_against_solana_message() {
    // Our fixed key order compiled by Solana's serializers must give our bytes.
    let mut rng = Rng(99);
    for (f, n) in [
        (TxFormat::Legacy, false),
        (TxFormat::Legacy, true),
        (TxFormat::V1, false),
        (TxFormat::V1, true),
    ] {
        let p = params(&mut rng, f, n, n);
        let mut t = Template::buy(&p).unwrap();
        let k = real_keys(rng.key(), &p);
        let lt = [0x5a; 32];
        t.patch(&Patch {
            keys: &k,
            lifetime: &lt,
            amount: 123,
            limit: 456,
            priority_lamports: 7_200,
            tip_lamports: 1_000_000,
            cu_limit: None,
        });
        let tx = TxView::parse(t.bytes()).unwrap();
        let keys = tx.static_account_keys().to_vec();
        let h = tx.header();
        let header = solana_message::MessageHeader {
            num_required_signatures: h.num_required_signatures,
            num_readonly_signed_accounts: h.num_readonly_signed_accounts,
            num_readonly_unsigned_accounts: h.num_readonly_unsigned_accounts,
        };
        let cis: Vec<solana_message::compiled_instruction::CompiledInstruction> = tx
            .instructions()
            .map(
                |ix| solana_message::compiled_instruction::CompiledInstruction {
                    program_id_index: ix.program_id_index,
                    accounts: ix.accounts.to_vec(),
                    data: ix.data.to_vec(),
                },
            )
            .collect();
        let hash = solana_hash::Hash::new_from_array(lt);
        let theirs = match f {
            TxFormat::Legacy => solana_message::legacy::Message {
                header,
                account_keys: keys,
                recent_blockhash: hash,
                instructions: cis,
            }
            .serialize(),
            TxFormat::V1 => {
                let cfg = solana_message::v1::TransactionConfig::empty()
                    .with_priority_fee(7_200)
                    .with_compute_unit_limit(p.cu_limit)
                    .with_loaded_accounts_data_size_limit(p.lads_bytes);
                solana_message::v1::Message::new(header, cfg, hash, keys, cis).serialize()
            }
        };
        assert_eq!(t.message(), theirs.as_slice(), "{f:?} nonce={n}");
        assert_eq!(
            tx.version(),
            if f == TxFormat::V1 {
                TxVersion::V1
            } else {
                TxVersion::Legacy
            }
        );
    }
}

#[test]
fn v1_sizes_match_research_and_stay_under_1232() {
    let mut rng = Rng(5);
    let mut p = params(&mut rng, TxFormat::V1, false, false);
    assert_eq!(
        Template::buy(&p).unwrap().len(),
        805,
        "v1 ATA create + buy + tip"
    );
    p.nonce = Some(NonceKeys {
        account: rng.key(),
        authority: p.user,
    });
    let t = Template::buy(&p).unwrap();
    assert_eq!(t.len(), 880, "v1 + nonce");
    assert_eq!(t.message()[0], 0x81);
    let tx = TxView::parse(t.bytes()).unwrap();
    assert_eq!(tx.v1_config().unwrap().mask, 0x0F);
    assert_eq!(
        tx.signatures_offset(),
        t.len() - 64,
        "v1 signatures at the tail"
    );
    assert_eq!(tx.static_account_keys().len(), 21);
    p.format = TxFormat::Legacy;
    let l = Template::buy(&p).unwrap();
    assert!(l.len() < rugsnipe_tx::MAX_TX_LEN, "{}", l.len());
}

#[test]
fn template_parses_as_buy_and_signs() {
    use rugsnipe_tx::signer::{verify, HotSigner};
    use zeroize::Zeroizing;
    let payer = HotSigner::from_seed(&Zeroizing::new([11u8; 32]));
    let auth = HotSigner::from_seed(&Zeroizing::new([12u8; 32]));
    let p = CurveParams {
        format: TxFormat::V1,
        user: payer.pubkey(),
        nonce: Some(NonceKeys {
            account: Pubkey::new_unique(),
            authority: auth.pubkey(),
        }),
        cu_limit: 72_150,
        lads_bytes: 12_124_160,
        tip_account: Some(c::JITO_TIP_ACCOUNTS[1]),
        buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[0],
        base_token_program: c::TOKEN_2022_PROGRAM_ID,
        partial_fill: None,
    };
    let mut t = Template::buy(&p).unwrap();
    let mint = Pubkey::new_unique();
    t.patch(&Patch {
        keys: &real_keys(mint, &p),
        lifetime: &[3; 32],
        amount: 50_000_000,
        limit: 9,
        priority_lamports: 1,
        tip_lamports: 2,
        cu_limit: None,
    });
    t.sign(&payer, Some(&auth));
    let tx = TxView::parse(t.bytes()).unwrap();
    assert_eq!(tx.num_signatures(), 2);
    assert!(verify(
        &payer.pubkey(),
        tx.message_bytes(),
        tx.signature(0).unwrap()
    ));
    assert!(verify(
        &auth.pubkey(),
        tx.message_bytes(),
        tx.signature(1).unwrap()
    ));
    let mut found = false;
    for ix in tx.instructions() {
        let a = tx.instruction_accounts(&ix, None);
        if let Classified::Pump(PumpInstruction::Trade(tr)) =
            classify(tx.program_id(&ix), ix.data, &a).unwrap()
        {
            assert_eq!(tr.kind, PumpTradeKind::BuyExactQuoteInV3);
            assert_eq!(
                tr.amounts,
                TradeAmounts::ExactQuoteIn {
                    spendable_quote_in: 50_000_000,
                    min_tokens_out: 9
                }
            );
            assert_eq!(tr.mint, Some(mint));
            assert_eq!(tr.bonding_curve, Some(pda::bonding_curve(&mint)));
            found = true;
        }
    }
    assert!(found);
    // re-patching clears stale signatures
    t.patch(&Patch {
        keys: &real_keys(Pubkey::new_unique(), &p),
        lifetime: &[4; 32],
        amount: 1,
        limit: 1,
        priority_lamports: 1,
        tip_lamports: 1,
        cu_limit: None,
    });
    assert_eq!(t.signature(), [0u8; 64]);
}

#[test]
fn missing_budget_and_spl_base_program() {
    let mut rng = Rng(3);
    let mut p = params(&mut rng, TxFormat::V1, false, false);
    p.cu_limit = 0;
    assert_eq!(Template::buy(&p).unwrap_err(), TemplateError::MissingBudget);
    // legacy-create (SPL Token) coin: base and quote token programs coincide
    let mut p = params(&mut rng, TxFormat::Legacy, true, false);
    p.base_token_program = c::SPL_TOKEN_PROGRAM_ID;
    let mut t = Template::buy(&p).unwrap();
    let k = real_keys(rng.key(), &p);
    t.patch(&Patch {
        keys: &k,
        lifetime: &[1; 32],
        amount: 1,
        limit: 1,
        priority_lamports: 100,
        tip_lamports: 1,
        cu_limit: None,
    });
    assert_eq!(
        decompile(t.bytes()),
        reference(
            &buy_instructions(&p, &k, 1, 1, 100, 1),
            &p.user,
            [1; 32],
            p.format,
            p.cu_limit,
            p.lads_bytes,
            100
        )
    );
}

#[test]
fn legacy_price_conversion() {
    assert_eq!(legacy_price(7_200, 72_000), 100_000);
    assert_eq!(legacy_price(72_000, 72_000), 1_000_000);
    assert_eq!(legacy_price(1, 0), 0);
    // never pays more than asked
    for (t, cu) in [(12_345u64, 72_150u32), (1, 3), (999_999, 61_513)] {
        assert!(legacy_price(t, cu) as u128 * cu as u128 / 1_000_000 <= t as u128);
    }
}

#[test]
fn compile_rejects_privilege_escalation_in_templates() {
    let a = Pubkey::new_unique();
    let ix = pump_codec::instructions::system_transfer(&a, &c::JITO_TIP_ACCOUNTS[0], 1);
    let keys = [
        KeySpec::new(a, true, true),
        KeySpec::new(c::JITO_TIP_ACCOUNTS[0], false, true),
        KeySpec::new(c::SYSTEM_PROGRAM_ID, false, false),
    ];
    let (raw, layout) = compile(
        &keys,
        &[ix],
        &[0; 32],
        Format::V1(V1Config {
            priority_fee_lamports: 1,
            compute_unit_limit: 450,
            loaded_accounts_data_size_limit: 32_768,
        }),
    )
    .unwrap();
    assert_eq!(layout.n_sigs, 1);
    assert_eq!(TxView::parse(&raw).unwrap().num_instructions(), 1);
}

/// Fresh random mints on every run (seed from the OS, printed for replay
/// with `RUGSNIPE_SEED=<seed>`): the patched template equals (a) the
/// from-scratch builder compiled by solana-message and (b) pump-rust-client
/// 0.4.1's own `buy_exact_quote_in_v3_instructions` (T22 ATA create + buy):
/// same programs, accounts, data and privileges, for 1,000 mints over every
/// format / nonce / authority combination. The only intended difference is
/// the `partial_fill` byte: the SDK always requests it, the template carries
/// the config value (docs/03 section 6: it never rescues a slippage failure).
#[test]
fn fresh_random_mints_patch_equals_scratch_and_pump_rust_client() {
    let seed = std::env::var("RUGSNIPE_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            let mut b = [0u8; 8];
            std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom").unwrap(), &mut b)
                .unwrap();
            u64::from_le_bytes(b)
        })
        | 1;
    eprintln!("RUGSNIPE_SEED={seed}");
    let mut rng = Rng(seed);
    let combos = [
        (TxFormat::Legacy, false, false),
        (TxFormat::Legacy, true, false),
        (TxFormat::Legacy, true, true),
        (TxFormat::V1, false, false),
        (TxFormat::V1, true, false),
        (TxFormat::V1, true, true),
    ];
    let mut templates: Vec<(CurveParams, Template)> = combos
        .iter()
        .map(|&(f, n, s)| {
            let mut p = params(&mut rng, f, n, s);
            p.partial_fill = Some(rng.next().is_multiple_of(2));
            (p, Template::buy(&p).unwrap())
        })
        .collect();
    for i in 0..1_000 {
        let mint = rng.key();
        let (p, t) = &mut templates[i % combos.len()];
        let k = real_keys(mint, p);
        let mut lifetime = [0u8; 32];
        for ch in lifetime.chunks_mut(8) {
            ch.copy_from_slice(&rng.next().to_le_bytes());
        }
        let spend = rng.next() % 10_000_000_000 + 1;
        let min_out = rng.next();
        let prio = rng.next() % 50_000_000;
        let tip = rng.next() % 10_000_000 + 1;
        t.patch(&Patch {
            keys: &k,
            lifetime: &lifetime,
            amount: spend,
            limit: min_out,
            priority_lamports: prio,
            tip_lamports: tip,
            cu_limit: None,
        });
        let got = decompile(t.bytes());
        // (a) from-scratch builder, compiled by Solana's own message code
        let want = reference(
            &buy_instructions(p, &k, spend, min_out, prio, tip),
            &p.user,
            lifetime,
            p.format,
            p.cu_limit,
            p.lads_bytes,
            prio,
        );
        assert_eq!(got, want, "seed {seed} mint #{i} {mint} {:?}", p.format);
        // (b) pump-rust-client: [ATA create (idempotent, T22), buy_exact_quote_in_v3]
        let sdk = pump_rust_client::PumpSdk.buy_exact_quote_in_v3_instructions(
            p3(&mint),
            p3(&c::WSOL_MINT),
            p3(&c::TOKEN_2022_PROGRAM_ID),
            p3(&c::SPL_TOKEN_PROGRAM_ID),
            p3(&p.user),
            p3(&p.buyback_fee_recipient),
            spend,
            min_out,
        );
        assert_eq!(sdk.len(), 2, "SOL-quoted: ATA create + buy");
        let ata_prog = c::ASSOCIATED_TOKEN_PROGRAM_ID.to_bytes();
        let ours: Vec<_> = got
            .iter()
            .filter(|(prog, _, _)| prog.to_bytes() == ata_prog || *prog == c::PUMP_PROGRAM_ID)
            .collect();
        assert_eq!(ours.len(), 2, "seed {seed} mint {mint}");
        for ((prog, metas, data), theirs) in ours.iter().zip(&sdk) {
            assert_eq!(prog.to_bytes(), theirs.program_id.to_bytes(), "seed {seed}");
            assert_eq!(metas.len(), theirs.accounts.len(), "seed {seed} {prog}");
            for ((k, s, w), m) in metas.iter().zip(&theirs.accounts) {
                assert_eq!(
                    k.to_bytes(),
                    m.pubkey.to_bytes(),
                    "seed {seed} mint {mint} {prog}"
                );
                if *prog == c::PUMP_PROGRAM_ID {
                    assert_eq!((*s, *w), (m.is_signer, m.is_writable), "seed {seed} {k}");
                } else {
                    // message-level privileges are the union over instructions
                    assert!(*s || !m.is_signer, "seed {seed} {k} must sign");
                    assert!(*w || !m.is_writable, "seed {seed} {k} must be writable");
                }
            }
            if *prog == c::PUMP_PROGRAM_ID {
                assert_eq!(data.len(), theirs.data.len(), "seed {seed}");
                assert_eq!(&data[..24], &theirs.data[..24], "seed {seed} mint {mint}");
                assert_eq!(theirs.data[24], 1, "the SDK always requests partial_fill");
                assert_eq!(
                    data[24],
                    p.partial_fill.unwrap() as u8,
                    "config partial_fill"
                );
            } else {
                assert_eq!(data, &theirs.data, "seed {seed} ATA create data");
            }
        }
    }
}

/// Index of the pump buy and of the tip transfer in a decompiled transaction.
fn buy_and_tip(sem: &Semantic, tip_account: &Pubkey) -> (usize, Option<usize>) {
    let buy = sem
        .iter()
        .position(|(prog, _, _)| *prog == c::PUMP_PROGRAM_ID)
        .expect("a pump instruction");
    let tip = sem.iter().position(|(prog, accs, data)| {
        *prog == c::SYSTEM_PROGRAM_ID
            && data.first() == Some(&2)
            && accs.get(1).is_some_and(|a| a.0 == *tip_account)
    });
    (buy, tip)
}

#[test]
fn tip_is_the_last_instruction_after_the_buy_or_sell() {
    // Pay on success (docs/03 section 6): the tip is a transfer inside the
    // same transaction, after the trade, so a failed (reverted) buy never pays
    // it. Every format, with and without nonce or a second signer.
    let mut rng = Rng(0x7195_0ff1);
    for f in [TxFormat::Legacy, TxFormat::V1] {
        for (nonce, sep) in [(false, false), (true, false), (true, true)] {
            let p = params(&mut rng, f, nonce, sep);
            let tip_acc = p.tip_account.unwrap();
            let k = real_keys(rng.key(), &p);
            let lifetime = [9u8; 32];
            let mut b = Template::buy(&p).unwrap();
            b.patch(&Patch {
                keys: &k,
                lifetime: &lifetime,
                amount: 500_000_000,
                limit: 1,
                priority_lamports: 10_000,
                tip_lamports: 3_400_000,
                cu_limit: None,
            });
            let mut s = Template::sell(&p, true).unwrap();
            s.patch(&Patch {
                keys: &k,
                lifetime: &lifetime,
                amount: 1_000,
                limit: 1,
                priority_lamports: 10_000,
                tip_lamports: 1_000_000,
                cu_limit: None,
            });
            for (t, what, tip) in [(&b, "buy", 3_400_000u64), (&s, "sell", 1_000_000)] {
                let sem = decompile(t.bytes());
                let (trade, tip_ix) = buy_and_tip(&sem, &tip_acc);
                let tip_ix = tip_ix.unwrap_or_else(|| panic!("{f:?} {what}: no tip"));
                assert!(
                    tip_ix > trade,
                    "{f:?} nonce {nonce} {what}: tip before the trade"
                );
                assert_eq!(tip_ix, sem.len() - 1, "{f:?} {what}: tip is not last");
                assert_eq!(sem[tip_ix].2[4..12], tip.to_le_bytes(), "{f:?} {what}");
                // the tip is the only System transfer: nothing else moves SOL to a sender
                let transfers = sem
                    .iter()
                    .filter(|(prog, _, d)| *prog == c::SYSTEM_PROGRAM_ID && d.first() == Some(&2))
                    .count();
                assert_eq!(transfers, 1, "{f:?} {what}");
            }
            // the direct-TPU variant (no tip account) ends with the buy
            let direct = CurveParams {
                tip_account: None,
                ..p
            };
            let mut d = Template::buy(&direct).unwrap();
            d.patch(&Patch {
                keys: &k,
                lifetime: &lifetime,
                amount: 500_000_000,
                limit: 1,
                priority_lamports: 10_000,
                tip_lamports: 0,
                cu_limit: None,
            });
            let sem = decompile(d.bytes());
            let (trade, tip_ix) = buy_and_tip(&sem, &tip_acc);
            assert_eq!((trade, tip_ix), (sem.len() - 1, None), "{f:?} direct");
            assert!(d.table.tip.is_none());
        }
    }
}

#[test]
fn bid_plans_keep_a_failed_attempt_within_the_cap() {
    // What a failed snipe pays is base fee x signatures + the header priority
    // fee (legacy: ceil(price x limit / 1e6)); the tip reverts. For every leader
    // class, format, signer count and bid, the patched template must stay
    // within bid.failed_attempt_lamports_max, and a tip class must carry the
    // bid in the tip with the priority fee at the floor.
    use rugsnipe_core::bid::{base_fee, BidPolicy, LeaderClass};
    use rugsnipe_core::config::BidCfg;
    let mut rng = Rng(0xcafe_f00d);
    for cap in [20_000u64, 1_000_000, 3_000_000] {
        let cfg = BidCfg {
            failed_attempt_lamports_max: cap,
            ..BidCfg::default()
        };
        let pol = BidPolicy::from_cfg(&cfg);
        for f in [TxFormat::Legacy, TxFormat::V1] {
            for (nonce, sep) in [(true, false), (true, true)] {
                let p = params(&mut rng, f, nonce, sep);
                let mut t = Template::buy(&p).unwrap();
                let n_sigs = t.table.n_sigs as u8;
                assert_eq!(n_sigs, if sep { 2 } else { 1 });
                for bid in [0u64, 10_000, 3_400_000, 25_000_000] {
                    for class in LeaderClass::ALL {
                        let plan = pol.plan(class, bid, n_sigs);
                        let k = real_keys(rng.key(), &p);
                        let cu = k
                            .snipe_cu_limit(&p.user, &p.base_token_program, 0, p.cu_limit)
                            .unwrap();
                        t.patch(&Patch {
                            keys: &k,
                            lifetime: &[3u8; 32],
                            amount: 500_000_000,
                            limit: 1,
                            priority_lamports: plan.priority_lamports,
                            tip_lamports: plan.tip_lamports,
                            cu_limit: Some(cu),
                        });
                        let cb = TxView::parse(t.bytes()).unwrap().compute_budget();
                        let charged = match f {
                            TxFormat::V1 => cb.priority_fee_lamports.unwrap(),
                            TxFormat::Legacy => {
                                let price = cb.compute_unit_price_micro_lamports.unwrap() as u128;
                                (price * cu as u128).div_ceil(1_000_000) as u64
                            }
                        };
                        let failed = base_fee(n_sigs) + charged;
                        assert!(
                            failed <= cap,
                            "{f:?} {class:?} bid {bid} sigs {n_sigs}: {failed} > {cap}"
                        );
                        // legacy rounds the price down: never more than the plan
                        assert!(failed <= plan.failed_cost(n_sigs));
                        if class.honours_tip() && cap >= 2 * 5_000 + cfg.min_priority_lamports {
                            assert_eq!(plan.priority_lamports, cfg.min_priority_lamports);
                            assert_eq!(plan.tip_lamports, bid);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn tip_account_patch_retargets_the_tip_and_keeps_the_message_exact() {
    // One template, any provider's tip account (each sender has its own):
    // the patched transaction equals a scratch build with that account, the
    // tip stays the last instruction and goes to the new account only.
    let helius = Pubkey::from_str_const("4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE");
    let mut rng = Rng(0x71b5);
    for f in [TxFormat::Legacy, TxFormat::V1] {
        let p = params(&mut rng, f, true, false);
        let k = real_keys(rng.key(), &p);
        let lifetime = [3u8; 32];
        let mut t = Template::buy(&p).unwrap();
        assert_eq!(t.tip_account(), p.tip_account);
        t.patch(&Patch {
            keys: &k,
            lifetime: &lifetime,
            amount: 20_000_000,
            limit: 7,
            priority_lamports: 10_000,
            tip_lamports: 1_000_000,
            cu_limit: None,
        });
        assert!(t.set_tip_account(&helius));
        assert_eq!(t.tip_account(), Some(helius));
        assert_eq!(t.tip_lamports(), Some(1_000_000));
        let sem = decompile(t.bytes());
        let (buy, tip) = buy_and_tip(&sem, &helius);
        assert_eq!(
            tip,
            Some(sem.len() - 1),
            "{f:?}: tip last, to the new account"
        );
        assert!(buy < sem.len() - 1);
        let (_, old) = buy_and_tip(&sem, &p.tip_account.unwrap());
        assert_eq!(old, None, "{f:?}: nothing goes to the template's account");
        let scratch = Template::buy(&CurveParams {
            tip_account: Some(helius),
            ..p
        })
        .unwrap();
        let mut s2 = scratch.clone();
        s2.patch(&Patch {
            keys: &k,
            lifetime: &lifetime,
            amount: 20_000_000,
            limit: 7,
            priority_lamports: 10_000,
            tip_lamports: 1_000_000,
            cu_limit: None,
        });
        assert_eq!(
            decompile(s2.bytes()),
            sem,
            "{f:?}: same instructions as a scratch build"
        );
    }
    // a template without a tip has nothing to retarget
    let p = CurveParams {
        tip_account: None,
        ..params(&mut rng, TxFormat::Legacy, true, false)
    };
    let mut t = Template::buy(&p).unwrap();
    assert!(!t.set_tip_account(&helius));
    assert_eq!(t.tip_account(), None);
}
