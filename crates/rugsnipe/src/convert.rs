//! Conversions between the feed's heap-free records and the codec / exit
//! engine types (all `Copy`, no allocation).

use pump_codec::constants::{SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, WSOL_MINT};
use pump_codec::create_hit::{CreateHit, HitSource, KeyList};
use pump_codec::curve::CurveState;
use pump_codec::instructions::CreateKind;
use pump_codec::Pubkey;
use rugsnipe_exit::{Trade, Venue};
use rugsnipe_feed::{hit_flags as hf, CreateHitLite, TradeLite};

/// A logs-path `CreateHit` from the feed's `CreateHitLite` (docs/06
/// section 7.1 "free mode"): the signer is the CreateEvent's `user`; fee
/// payer, other signers and token owners are unknown (paper only).
pub fn hit_from_lite(l: &CreateHitLite) -> CreateHit {
    let mut signers = KeyList::default();
    signers.push(l.user);
    let token_prog = if l.has(hf::TOKEN_PROGRAM_KNOWN) {
        l.token_program
    } else if l.has(hf::LEGACY_CREATE) {
        SPL_TOKEN_PROGRAM_ID
    } else {
        Pubkey::default()
    };
    let quote = l.has(hf::QUOTE_KNOWN).then(|| {
        if l.quote_mint == Pubkey::default() {
            WSOL_MINT
        } else {
            l.quote_mint
        }
    });
    let sol = quote.is_some_and(|q| q == WSOL_MINT);
    // Curve after the in-tx buys: exact for SOL, non-mayhem coins whose logs
    // were not truncated (fresh curve + the summed TradeEvents).
    let curve_after = (sol && !l.mayhem && !l.has(hf::LOGS_TRUNCATED)).then(|| {
        let mut c = CurveState::fresh(l.creator);
        if l.intx_buys > 0 {
            c.apply_buy(l.intx_tokens_out, l.intx_sol_in);
        }
        c
    });
    CreateHit {
        source: HitSource::Logs,
        slot: Some(l.slot),
        sig: l.sig.0,
        version: None,
        kind: if l.has(hf::LEGACY_CREATE) {
            Some(CreateKind::Legacy)
        } else if token_prog == TOKEN_2022_PROGRAM_ID {
            Some(CreateKind::V2)
        } else {
            None
        },
        mint: l.mint,
        bc: l.bc,
        abc: if l.has(hf::ABC_KNOWN) {
            l.abc
        } else {
            Pubkey::default()
        },
        aqbc_hint: None,
        user: l.user,
        fee_payer: None,
        creator_arg: l.creator,
        holder_reward: false,
        creator_fee_bps: 0,
        signers,
        quote,
        token_prog,
        mayhem: l.mayhem,
        cashback: l.has(hf::CASHBACK),
        intx_sol_in: l.intx_sol_in,
        intx_tokens_out: l.intx_tokens_out,
        intx_buys: l.intx_buys,
        intx_buyers: KeyList::default(),
        intx_exact: !l.has(hf::LOGS_TRUNCATED),
        curve_after,
        launcher: Default::default(),
        logs_truncated: l.has(hf::LOGS_TRUNCATED),
        tx_shape: Default::default(),
    }
}

/// Virtual SOL after the create's in-tx buys (`V0`, docs/06 section 7.1 step 8).
pub fn v0_of(hit: &CreateHit) -> (u64, u64) {
    match hit.curve_after {
        Some(c) => (c.virtual_sol, c.virtual_token),
        None => {
            let mut c = CurveState::fresh(hit.creator_arg);
            c.apply_buy(hit.intx_tokens_out, hit.intx_sol_in);
            (c.virtual_sol, c.virtual_token)
        }
    }
}

/// A feed trade as an exit-engine trade (bonding curve venue).
pub fn trade_from_lite(t: &TradeLite) -> Trade {
    Trade {
        venue: Venue::Curve,
        mint: t.mint,
        user: t.user,
        is_buy: t.is_buy,
        sol_amount: t.sol_amount,
        token_amount: t.token_amount,
        fee_lamports: t.fee,
        reserve_sol: t.virtual_sol_reserves,
        reserve_token: t.virtual_token_reserves,
        slot: t.slot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rugsnipe_feed::Sig;

    #[test]
    fn lite_to_hit() {
        let user = Pubkey::new_unique();
        let l = CreateHitLite {
            mint: Pubkey::new_unique(),
            bc: Pubkey::new_unique(),
            abc: Pubkey::new_unique(),
            user,
            creator: Pubkey::new_unique(),
            token_program: TOKEN_2022_PROGRAM_ID,
            quote_mint: Pubkey::default(),
            mayhem: false,
            slot: 7,
            sig: Sig([5; 64]),
            intx_sol_in: 1_000_000_000,
            intx_tokens_out: 34_612_903_225_806,
            intx_buys: 1,
            v_after: 31_000_000_000,
            flags: hf::TOKEN_PROGRAM_KNOWN | hf::QUOTE_KNOWN | hf::CREATOR_KNOWN | hf::ABC_KNOWN,
        };
        let h = hit_from_lite(&l);
        assert!(h.is_sol_quote());
        assert_eq!(h.signers.as_slice(), &[user]);
        assert_eq!(h.fee_payer, None);
        assert_eq!(h.source, HitSource::Logs);
        assert_eq!(h.kind, Some(CreateKind::V2));
        assert_eq!(h.abc, l.abc);
        let (v, t) = v0_of(&h);
        assert_eq!(v, 31_000_000_000);
        assert!(t < pump_codec::curve::INITIAL_VIRTUAL_TOKEN_RESERVES);
        // truncated logs: no exact curve, totals are lower bounds
        let h2 = hit_from_lite(&CreateHitLite {
            flags: l.flags | hf::LOGS_TRUNCATED,
            ..l
        });
        assert!(h2.curve_after.is_none() && h2.logs_truncated && !h2.intx_exact);
        // unknown quote is not SOL
        let h3 = hit_from_lite(&CreateHitLite {
            flags: hf::TOKEN_PROGRAM_KNOWN,
            ..l
        });
        assert!(!h3.is_sol_quote());
    }
}
