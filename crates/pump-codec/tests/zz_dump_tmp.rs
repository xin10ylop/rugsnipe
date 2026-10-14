mod common;
use common::*;
use pump_codec::events::{events_from_inner_instructions, PumpEvent};
use pump_codec::instructions::*;
use pump_codec::{pda, Pubkey, constants as c};
#[test]
fn dump() {
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let keys = fx.full_keys();
        let tx = fx.view();
        let evs = events_from_inner_instructions(&keys, fx.inner_instructions(), true).unwrap();
        let mut line = String::new();
        for ix in tx.instructions() {
            let full: Vec<Pubkey> = ix.accounts.iter().map(|&k| keys[k as usize]).collect();
            if let Ok(Classified::Pump(PumpInstruction::Create(cr))) = classify(tx.program_id(&ix), ix.data, full.as_slice()) {
                line += &format!(" ARG.creator={} ARG.user#={:?} signer0={}", cr.args.creator, cr.user, tx.fee_payer());
            }
        }
        for r in &evs {
            match &r.event {
                PumpEvent::Create(ce) => line += &format!(" CE.creator={} CE.user={}", ce.creator, ce.user),
                PumpEvent::Trade(t) => line += &format!(" TE.creator={:?} TE.user={}", t.creator, t.user),
                _ => {}
            }
        }
        if line.contains("CE.") { println!("{name}:{line}"); }
    }
    // the non-ATA v3 account
    let fx = TxFixture::load("v1_buy_exact_quote_in_v3");
    let user: Pubkey = "8Z2tbjpLjRrrCMNKtu69wg81LkFg4U9iv5MHmbw7EJwG".parse().unwrap();
    let mint: Pubkey = "AvUytJGmJsto4GStZMyfmp6GjD982xrwd2M2goE6pump".parse().unwrap();
    println!("ata t22={} spl={} onchain=6zymZFxV9wCj7c3Zy4fC8VKDDjb145NdAAcog7M2eUzk", pda::ata_token_2022(&user, &mint), pda::ata_spl(&user, &mint));
    let _ = (c::PUMP_PROGRAM_ID, fx.name);
}
