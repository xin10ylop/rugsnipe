mod common;
use common::*;
use pump_codec::instructions::*;
use pump_codec::Pubkey;
fn s(c: Classified<'_>) -> Option<String> {
    Some(match c {
        Classified::Pump(PumpInstruction::Create(c)) => format!("pump.create:{:?}/{} tail={} sol={:?} q={:?}", c.kind, c.num_accounts, c.quote_tail_len, c.is_sol_quote, c.quote_mint.map(|q| q.to_string()[..6].to_string())),
        Classified::Pump(PumpInstruction::Trade(t)) => format!("pump.{}/{} bb={:?} pf={:?} tv={:?}", t.kind.name(), t.num_accounts, t.buyback_fee_recipient.map(|q| q.to_string()[..6].to_string()), t.partial_fill, t.track_volume),
        Classified::Pump(PumpInstruction::Other { discriminator }) => format!("pump.other:{}", discriminator.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        Classified::PumpSwap(AmmInstruction::Trade(t)) => format!("amm.{:?}/{}", t.kind, t.num_accounts),
        Classified::PumpSwap(AmmInstruction::CreatePool(_)) => "amm.create_pool".into(),
        Classified::PumpSwap(AmmInstruction::Other { discriminator }) => format!("amm.other:{}", discriminator.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        Classified::NotPump => return None,
    })
}
#[test]
fn dump() {
    for name in tx_names() {
        if !name.starts_with("fresh") && name != "legacy_create_spl_b" { continue; }
        let fx = TxFixture::load(&name);
        let tx = fx.view();
        let keys = fx.full_keys();
        let mut top = vec![]; let mut inner = vec![];
        for ix in tx.instructions() {
            let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
            if let Some(x) = s(classify(tx.program_id(&ix), ix.data, &a).unwrap()) { top.push(x); }
        }
        for (_o, p, accts, data) in &fx.inner {
            let ak: Vec<Pubkey> = accts.iter().map(|&k| keys[k as usize]).collect();
            if data.starts_with(&pump_codec::constants::EVENT_IX_TAG) { continue; }
            if let Some(x) = s(classify(&keys[*p as usize], data, ak.as_slice()).unwrap()) { inner.push(x); }
        }
        println!("{name} v={:?} len={} | top={top:?} | inner={inner:?}", tx.version(), fx.raw.len());
    }
}
