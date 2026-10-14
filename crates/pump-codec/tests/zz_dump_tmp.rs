mod common;
use common::*;
use pump_codec::events::{events_from_inner_instructions, PumpEvent};
#[test]
fn dump() {
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let keys = fx.full_keys();
        for r in events_from_inner_instructions(&keys, fx.inner_instructions(), true).unwrap() {
            match &r.event {
                PumpEvent::Trade(t) => println!("{name} | TRADE {} buy={} sol={} tok={} q={:?} qa={:?} fee={:?}/{:?} cr={:?}/{:?} bb={:?}/{:?} mayhem={:?} fr={:?} cash={:?}/{:?} hr={:?}/{:?} vq={:?} rq={:?} vs={} vt={}",
                    t.ix_name.unwrap_or("?"), t.is_buy, t.sol_amount, t.token_amount, t.quote_mint.map(|q| q.to_string()[..6].to_string()), t.quote_amount,
                    t.fee_basis_points, t.fee, t.creator_fee_basis_points, t.creator_fee, t.buyback_fee_basis_points, t.buyback_fee, t.mayhem_mode,
                    t.fee_recipient.map(|q| q.to_string()[..6].to_string()), t.cashback_fee_basis_points, t.cashback, t.holder_rewards_bps, t.holder_rewards,
                    t.virtual_quote_reserves, t.real_quote_reserves, t.virtual_sol_reserves, t.virtual_token_reserves),
                e => println!("{name} | {} {:?}", e.name(), r.source),
            }
        }
    }
}
