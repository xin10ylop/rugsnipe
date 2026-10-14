//! TxView against real legacy / v0 / v1 transactions, with two independent
//! oracles: the RPC's own `encoding: "json"` decoding and
//! `agave-transaction-view` 6.1. Signatures are verified with ed25519 over
//! `message_bytes()`, which proves the v1 tail layout.

mod common;

use agave_transaction_view::transaction_view::TransactionView as AgaveView;
use common::*;
use pump_codec::tx::{TxVersion, TxView};
use pump_codec::Pubkey;

fn b58(b: &[u8]) -> String {
    bs58::encode(b).into_string()
}

#[test]
fn every_fixture_round_trips_against_rpc_json() {
    let mut seen = std::collections::HashMap::new();
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let tx = fx.view();
        *seen.entry(tx.version()).or_insert(0) += 1;
        let j = fx.json();
        let t = &j["transaction"];
        let m = &t["message"];

        // version
        let want = match &j["version"] {
            serde_json::Value::String(s) if s == "legacy" => TxVersion::Legacy,
            v if v.as_u64() == Some(0) => TxVersion::V0,
            v if v.as_u64() == Some(1) => TxVersion::V1,
            v => panic!("{name}: version {v}"),
        };
        assert_eq!(tx.version(), want, "{name}");
        assert_eq!(fx.result["version"], j["version"], "{name}");

        // signatures (first = tx id)
        let sigs: Vec<String> = (0..tx.num_signatures())
            .map(|i| b58(tx.signature(i).unwrap()))
            .collect();
        let jsigs: Vec<String> = t["signatures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        assert_eq!(sigs, jsigs, "{name}");
        assert_eq!(tx.transaction_id().unwrap(), fx.signature, "{name}");

        // header
        let h = tx.header();
        assert_eq!(
            h.num_required_signatures as u64,
            m["header"]["numRequiredSignatures"].as_u64().unwrap()
        );
        assert_eq!(
            h.num_readonly_signed_accounts as u64,
            m["header"]["numReadonlySignedAccounts"].as_u64().unwrap()
        );
        assert_eq!(
            h.num_readonly_unsigned_accounts as u64,
            m["header"]["numReadonlyUnsignedAccounts"].as_u64().unwrap()
        );

        // static keys, blockhash
        let keys: Vec<String> = tx
            .static_account_keys()
            .iter()
            .map(|k| k.to_string())
            .collect();
        let jkeys: Vec<String> = m["accountKeys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys, jkeys, "{name}");
        assert_eq!(
            b58(tx.recent_blockhash()),
            m["recentBlockhash"].as_str().unwrap(),
            "{name}"
        );

        // instructions
        let jix = m["instructions"].as_array().unwrap();
        assert_eq!(tx.num_instructions(), jix.len(), "{name}");
        for (ix, ji) in tx.instructions().zip(jix) {
            assert_eq!(
                ix.program_id_index as u64,
                ji["programIdIndex"].as_u64().unwrap(),
                "{name}"
            );
            let ja: Vec<u8> = ji["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(ix.accounts, ja.as_slice(), "{name}");
            assert_eq!(b58(ix.data), ji["data"].as_str().unwrap(), "{name}");
        }

        // v0 lookups
        let jl = m
            .get("addressTableLookups")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let lookups: Vec<_> = tx.address_table_lookups().collect();
        assert_eq!(lookups.len(), jl.len(), "{name}");
        for (l, jl) in lookups.iter().zip(&jl) {
            assert_eq!(
                l.account_key.to_string(),
                jl["accountKey"].as_str().unwrap()
            );
            let w: Vec<u8> = jl["writableIndexes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_u64().unwrap() as u8)
                .collect();
            let r: Vec<u8> = jl["readonlyIndexes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(l.writable_indexes, w.as_slice());
            assert_eq!(l.readonly_indexes, r.as_slice());
        }
        let (nw, nr) = tx.num_lookup_accounts();
        assert_eq!(nw + nr, fx.loaded.len(), "{name}: loaded address count");

        // v1 config
        if tx.version() == TxVersion::V1 {
            let cfg = tx.v1_config().unwrap();
            let jc = &m["transactionConfig"];
            assert_eq!(
                cfg.compute_unit_limit.map(u64::from),
                jc["computeUnitLimit"].as_u64(),
                "{name}"
            );
            assert_eq!(
                cfg.priority_fee_lamports,
                jc["priorityFee"].as_u64(),
                "{name}"
            );
            assert_eq!(
                cfg.loaded_accounts_data_size_limit.map(u64::from),
                jc["loadedAccountsDataSizeLimit"].as_u64(),
                "{name}"
            );
            assert_eq!(
                cfg.heap_size.map(u64::from),
                jc["heapSize"].as_u64(),
                "{name}"
            );
        } else {
            assert!(tx.v1_config().is_none());
        }
    }
    // The fixture set covers all three formats.
    assert!(
        seen.get(&TxVersion::Legacy).copied().unwrap_or(0) >= 1,
        "{seen:?}"
    );
    assert!(
        seen.get(&TxVersion::V0).copied().unwrap_or(0) >= 1,
        "{seen:?}"
    );
    assert!(
        seen.get(&TxVersion::V1).copied().unwrap_or(0) >= 1,
        "{seen:?}"
    );
}

#[test]
fn every_fixture_matches_agave_transaction_view() {
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let tx = fx.view();
        let a = AgaveView::try_new_unsanitized(fx.raw.as_slice())
            .unwrap_or_else(|e| panic!("{name}: agave {e:?}"));
        let av = match a.version() {
            agave_transaction_view::transaction_version::TransactionVersion::Legacy => {
                TxVersion::Legacy
            }
            agave_transaction_view::transaction_version::TransactionVersion::V0 => TxVersion::V0,
            agave_transaction_view::transaction_version::TransactionVersion::V1 => TxVersion::V1,
        };
        assert_eq!(tx.version(), av, "{name}");
        assert_eq!(tx.num_signatures(), a.num_signatures() as usize);
        for (i, s) in a.signatures().iter().enumerate() {
            assert_eq!(
                tx.signature(i).unwrap().as_slice(),
                s.as_ref(),
                "{name} sig {i}"
            );
        }
        assert_eq!(tx.static_account_keys(), a.static_account_keys(), "{name}");
        assert_eq!(
            tx.recent_blockhash().as_slice(),
            a.recent_blockhash().as_ref(),
            "{name}"
        );
        assert_eq!(
            tx.message_bytes(),
            a.message_data(),
            "{name}: signed message bytes"
        );
        let ours: Vec<_> = tx
            .instructions()
            .map(|i| (i.program_id_index, i.accounts.to_vec(), i.data.to_vec()))
            .collect();
        let theirs: Vec<_> = a
            .instructions_iter()
            .map(|i| (i.program_id_index, i.accounts.to_vec(), i.data.to_vec()))
            .collect();
        assert_eq!(ours, theirs, "{name}");
        let ol: Vec<_> = tx
            .address_table_lookups()
            .map(|l| {
                (
                    *l.account_key,
                    l.writable_indexes.to_vec(),
                    l.readonly_indexes.to_vec(),
                )
            })
            .collect();
        let tl: Vec<_> = a
            .address_table_lookup_iter()
            .map(|l| {
                (
                    *l.account_key,
                    l.writable_indexes.to_vec(),
                    l.readonly_indexes.to_vec(),
                )
            })
            .collect();
        assert_eq!(ol, tl, "{name}");
        match (tx.v1_config(), a.transaction_config()) {
            (Some(c), Some(t)) => {
                assert_eq!(c.mask, t.mask());
                assert_eq!(c.priority_fee_lamports, t.priority_fee_lamports());
                assert_eq!(c.compute_unit_limit, t.compute_unit_limit());
                assert_eq!(
                    c.loaded_accounts_data_size_limit,
                    t.loaded_accounts_data_size_limit()
                );
                assert_eq!(c.heap_size, t.requested_heap_size());
            }
            (None, None) => {}
            (o, t) => panic!("{name}: config mismatch {o:?} vs {:?}", t.map(|t| t.mask())),
        }
    }
}

#[test]
fn every_signature_verifies_over_message_bytes() {
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let tx = fx.view();
        let msg = tx.message_bytes();
        for (i, key) in tx.signer_keys().iter().enumerate() {
            let sig = solana_signature::Signature::from(*tx.signature(i).unwrap());
            assert!(
                sig.verify(key.as_ref(), msg),
                "{name}: signature {i} does not verify"
            );
        }
    }
}

#[test]
fn v1_signatures_are_at_the_tail_and_signer_matches_rpc() {
    let mut n = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let tx = fx.view();
        if tx.version() != TxVersion::V1 {
            continue;
        }
        n += 1;
        assert_eq!(fx.raw[0], 0x81, "{name}");
        let ns = tx.num_signatures();
        assert_eq!(tx.signatures_offset(), fx.raw.len() - 64 * ns, "{name}");
        assert_eq!(tx.message_bytes().len(), fx.raw.len() - 64 * ns);
        assert_eq!(tx.signatures_bytes(), &fx.raw[fx.raw.len() - 64 * ns..]);
        let j = fx.json();
        let k0 = j["transaction"]["message"]["accountKeys"][0]
            .as_str()
            .unwrap();
        assert_eq!(tx.fee_payer().to_string(), k0, "{name}");
        assert_eq!(tx.signer_keys()[0].to_string(), k0);
        // v1 never carries lookups.
        assert_eq!(tx.address_table_lookups().count(), 0);
    }
    assert!(n >= 3, "expected several v1 fixtures, got {n}");
}

#[test]
fn v0_with_lookups_resolves_through_loaded_addresses() {
    // worked_dev_buy_v2 is v0 and loads writable + readonly addresses from an ALT.
    let fx = TxFixture::load("worked_dev_buy_v2");
    let tx = fx.view();
    assert_eq!(tx.version(), TxVersion::V0);
    let (nw, nr) = tx.num_lookup_accounts();
    assert!(nw > 0 && nr > 0);
    assert_eq!(
        tx.total_account_count(),
        tx.static_account_keys().len() + fx.loaded.len()
    );
    // writable flags follow the header + lookup sections
    let ns = tx.static_account_keys().len();
    assert!(tx.is_writable(ns) && !tx.is_writable(ns + nw));
    assert!(tx.is_writable(0) && tx.is_signer(0));
}

#[test]
fn compute_budget_requests() {
    // legacy: ComputeBudget instructions
    let fx = TxFixture::load("worked_buy");
    let cb = fx.view().compute_budget();
    assert!(cb.compute_unit_limit.is_some() && cb.compute_unit_price_micro_lamports.is_some());
    assert!(cb.priority_fee_lamports.is_none());
    assert!(cb.priority_fee_lamports_total().unwrap() > 0);
    // v1: TransactionConfigMask
    let fx = TxFixture::load("v1_create_v2_buy");
    let cb = fx.view().compute_budget();
    assert_eq!(cb.compute_unit_limit, Some(437_423));
    assert_eq!(cb.priority_fee_lamports, Some(1_800_000));
    assert_eq!(cb.loaded_accounts_data_size_limit, Some(24_313_856));
    assert_eq!(cb.heap_size, None);
}

#[test]
fn rejects_malformed_input() {
    let fx = TxFixture::load("v1_create_v2_buy");
    // truncation anywhere is an error, never a panic
    for cut in [0usize, 1, 5, 41, 100, fx.raw.len() - 1] {
        assert!(TxView::parse(&fx.raw[..cut]).is_err(), "cut {cut}");
    }
    let mut extra = fx.raw.clone();
    extra.push(0);
    assert!(TxView::parse(&extra).is_err());
    // one priority-fee bit without the other is invalid (SIMD-0385)
    let mut bad = fx.raw.clone();
    bad[4] = (bad[4] & !0b11) | 0b01;
    assert!(TxView::parse(&bad).is_err());
    let legacy = TxFixture::load("legacy_buy18");
    for cut in [1usize, 64, 65, 70, legacy.raw.len() - 1] {
        assert!(TxView::parse(&legacy.raw[..cut]).is_err(), "cut {cut}");
    }
    // fuzz-ish: flipping bytes never panics
    for i in 0..fx.raw.len().min(400) {
        let mut m = fx.raw.clone();
        m[i] ^= 0xff;
        let _ = TxView::parse(&m).map(|t| t.instructions().count());
    }
}

#[test]
fn synthetic_v1_snipe_compiled_by_solana_message() {
    // Our builders -> solana-message 4.6 v1 compiler -> wire bytes -> TxView.
    use pump_codec::constants as c;
    use pump_codec::instructions::{
        self as ixs, classify, Classified, PumpInstruction, PumpTradeKind, SolV3Keys, TradeAmounts,
    };
    use solana_message::v1::{Message as V1Message, TransactionConfig};
    use solana_signer::Signer;
    let kp = solana_keypair::Keypair::new_from_array([7u8; 32]);
    let payer: Pubkey = kp.pubkey();
    let mint = pk("CzBigdLBY9C261pL2fgJ2icAeZcuZFDnivk8FB1mxzCX");
    let keys = SolV3Keys::derive(
        mint,
        payer,
        c::TOKEN_2022_PROGRAM_ID,
        c::BUYBACK_FEE_RECIPIENTS[3],
    );
    let [ata, buy] = ixs::buy_v3_with_ata(&keys, 1_000_000, 10_000_000, Some(true));
    let tip = ixs::jito_tip(&payer, 0, 15_000_000);
    let cfg = TransactionConfig::empty()
        .with_priority_fee(123_456)
        .with_compute_unit_limit(80_000)
        .with_loaded_accounts_data_size_limit(65_536)
        .with_heap_size(64 * 1024);
    let msg = V1Message::try_compile_with_config(
        &payer,
        &[ata, buy, tip],
        solana_hash::Hash::new_from_array([9u8; 32]),
        cfg,
    )
    .unwrap();
    let mut raw = msg.serialize();
    let sig = kp.sign_message(&raw);
    raw.extend_from_slice(sig.as_ref());
    let tx = TxView::parse(&raw).unwrap();
    assert_eq!(tx.version(), TxVersion::V1);
    let cfg = tx.v1_config().unwrap();
    assert_eq!(cfg.priority_fee_lamports, Some(123_456));
    assert_eq!(cfg.compute_unit_limit, Some(80_000));
    assert_eq!(cfg.loaded_accounts_data_size_limit, Some(65_536));
    assert_eq!(cfg.heap_size, Some(65_536));
    assert_eq!(cfg.mask, 0b11111);
    assert_eq!(*tx.fee_payer(), payer);
    assert_eq!(tx.num_instructions(), 3);
    let ix = tx.instruction(1).unwrap();
    let accts = tx.instruction_accounts(&ix, None);
    match classify(tx.program_id(&ix), ix.data, &accts).unwrap() {
        Classified::Pump(PumpInstruction::Trade(t)) => {
            assert_eq!(t.kind, PumpTradeKind::BuyV3);
            assert_eq!(
                t.amounts,
                TradeAmounts::ExactTokensOut {
                    amount: 1_000_000,
                    max_quote_cost: 10_000_000
                }
            );
            assert_eq!(t.mint, Some(mint));
            assert_eq!(t.user, Some(payer));
        }
        o => panic!("{o:?}"),
    }
    assert!(solana_signature::Signature::from(*tx.signature(0).unwrap())
        .verify(payer.as_ref(), tx.message_bytes()));
    // agave agrees, and the tx fits the legacy 1232-byte packet without an ALT.
    let a = AgaveView::try_new_unsanitized(raw.as_slice()).unwrap();
    assert_eq!(
        a.transaction_config().unwrap().compute_unit_limit(),
        Some(80_000)
    );
    assert!(raw.len() < 1232, "{} bytes", raw.len());
}
