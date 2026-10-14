mod common;
use base64::Engine as _;
use common::*;
use pump_codec::events::{events_from_inner_instructions, PumpEvent};
#[test]
fn dump() {
    let v = read_json("accounts/global.json");
    let d = base64::engine::general_purpose::STANDARD.decode(v["result"]["value"]["data"][0].as_str().unwrap()).unwrap();
    let g = pump_codec::curve::Global::decode(&d).unwrap();
    println!("whitelist_pda={} reserved={} auth={}", g.whitelist_pda, g.reserved_fee_recipient, g.authority);
    for name in ["fresh_v0alt_inner_sell_v2", "fresh_v1_create_v2_usdc_buy_v2", "fresh_v1_inner_bexq_v2", "fresh_v1_big_inner_buy_v3_uva"] {
        let fx = TxFixture::load(name);
        let keys = fx.full_keys();
        for r in events_from_inner_instructions(&keys, fx.inner_instructions(), true).unwrap() {
            if let PumpEvent::Trade(t) = &r.event {
                println!("{name} user={} creator={:?} fee_payer={} mint={}", t.user, t.creator, fx.view().fee_payer(), t.mint);
            }
        }
    }
}
