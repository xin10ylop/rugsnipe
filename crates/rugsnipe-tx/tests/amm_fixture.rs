//! PumpSwap canonical-pool `sell_v2` derivations against every real
//! `sell_v2` in the pump-codec fixtures (router CPIs on migrated pump coins).
//! All 17 accounts are derivable without RPC; the template itself is
//! SOL-quoted, and these pools happen to be PUMP-quoted, so the check runs
//! with each pool's own quote mint.

use base64::Engine as _;
use pump_codec::constants as c;
use pump_codec::instructions::{classify, AmmInstruction, AmmTradeKind, Classified};
use pump_codec::tx::TxView;
use pump_codec::{pda, Pubkey};
use rugsnipe_tx::template::{amm_mint_keys_for_quote, MintKeys};

#[test]
fn canonical_pool_sell_v2_accounts_match_mainnet() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../pump-codec/fixtures/tx");
    let mut checked = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(e.unwrap().path()).unwrap()).unwrap();
        let r = &v["result"];
        let raw = base64::engine::general_purpose::STANDARD.decode(r["transaction"][0].as_str().unwrap()).unwrap();
        let tx = TxView::parse(&raw).unwrap();
        let mut loaded: Vec<Pubkey> = vec![];
        if let Some(la) = r["meta"].get("loadedAddresses").filter(|x| !x.is_null()) {
            for k in ["writable", "readonly"] {
                for a in la[k].as_array().unwrap() {
                    loaded.push(a.as_str().unwrap().parse().unwrap());
                }
            }
        }
        let mut ixs: Vec<(Vec<u8>, Vec<u8>, usize)> = vec![];
        for ix in tx.instructions() {
            ixs.push((ix.accounts.to_vec(), ix.data.to_vec(), ix.program_id_index as usize));
        }
        for g in r["meta"]["innerInstructions"].as_array().into_iter().flatten() {
            for ix in g["instructions"].as_array().unwrap() {
                ixs.push((
                    ix["accounts"].as_array().unwrap().iter().map(|a| a.as_u64().unwrap() as u8).collect(),
                    bs58::decode(ix["data"].as_str().unwrap()).into_vec().unwrap(),
                    ix["programIdIndex"].as_u64().unwrap() as usize,
                ));
            }
        }
        for (idx, data, p) in ixs {
            let Some(prog) = tx.account_key(p, Some(&loaded)) else { continue };
            let accts = tx.indexed_accounts(&idx, Some(&loaded));
            let Ok(Classified::PumpSwap(AmmInstruction::Trade(t))) = classify(&prog, &data, &accts) else { continue };
            if t.kind != AmmTradeKind::SellV2 {
                continue;
            }
            let k: Vec<Pubkey> = idx.iter().map(|&i| tx.account_key(i as usize, Some(&loaded)).unwrap()).collect();
            let (user, mint, quote, base_tp, quote_tp) = (k[1], k[3], k[4], k[9], k[10]);
            eprintln!("sell_v2 base {mint} quote {quote} pool {}", k[0]);
            if !mint.to_string().ends_with("pump") {
                continue; // not a migrated pump coin (e.g. a USDC-based pool): no canonical PDA
            }
            let MintKeys([pool, m, user_base, pool_base, pool_quote]) = amm_mint_keys_for_quote(&mint, &quote, &user, &base_tp, &quote_tp);
            assert_eq!((k[0], k[3], k[5], k[7], k[8]), (pool, m, user_base, pool_base, pool_quote), "per-mint keys");
            assert_eq!(k[2], c::PUMP_SWAP_GLOBAL_CONFIG);
            assert_eq!(k[6], pda::associated_token_address(&user, &quote, &quote_tp));
            assert_eq!(k[11], c::SYSTEM_PROGRAM_ID);
            assert_eq!(k[12], pda::pump_swap_user_volume_accumulator(&user));
            assert_eq!(k[13], c::PUMP_SWAP_FEE_CONFIG);
            assert!(
                c::BUYBACK_FEE_RECIPIENTS.iter().any(|w| pda::associated_token_address(w, &quote, &quote_tp) == k[14]),
                "#14 = a listed buyback wallet's quote ATA"
            );
            assert_eq!((k[15], k[16]), (c::PUMP_SWAP_EVENT_AUTHORITY, c::PUMP_SWAP_PROGRAM_ID));
            checked += 1;
        }
    }
    assert!(checked >= 2, "only {checked} sell_v2 fixtures");
}
