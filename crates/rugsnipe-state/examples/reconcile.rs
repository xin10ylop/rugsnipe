//! Read-only recovery/reconcile against public RPC (feature `rpc`):
//! `cargo run -p rugsnipe-state --features rpc --example reconcile -- <owner> [nonce ...]`
//! Uses a throwaway state.db under the temp dir; prints the report.

use rugsnipe_state::chain::RpcChain;
use rugsnipe_state::recovery::{recover, RecoverCtx};
use rugsnipe_state::{Ledger, RpcBudget, StateStore};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let owner: pump_codec::Pubkey = args.first().expect("owner").parse().expect("pubkey");
    let nonces: Vec<(pump_codec::Pubkey, &'static str, u16)> = args[1..]
        .iter()
        .enumerate()
        .map(|(i, s)| (s.parse().expect("pubkey"), "buy", i as u16))
        .collect();
    let url = std::env::var("RPC_URL").unwrap_or("https://api.mainnet-beta.solana.com".into());
    let chain = RpcChain::new(&url, Arc::new(RpcBudget::new(2.0)));
    let dir = std::env::temp_dir().join(format!("rugsnipe-reconcile-{}", std::process::id()));
    let store = StateStore::open(dir.join("state.db")).expect("state.db");
    let ledger = Ledger::new(store.writer());
    let conn = store.reader().expect("reader");
    let rep = recover(
        &conn,
        &ledger,
        &RecoverCtx {
            owner: Some(owner),
            mode: "live",
            nonce_accounts: &nonces,
            chain: Some(&chain),
        },
    )
    .expect("recover");
    println!(
        "slot {:?}: {} orphan position(s) (non-zero token accounts), {} nonce row(s)",
        rep.slot,
        rep.orphans.len(),
        rep.nonces.len()
    );
    for o in rep.orphans.iter().take(10) {
        println!(
            "  orphan mint {} amount {} account {}",
            o.mint, o.amount, o.account
        );
    }
    for n in &rep.nonces {
        println!(
            "  nonce {} state {:?} value {} last_change_slot {:?}",
            n.pubkey,
            n.state,
            bs58::encode(n.value).into_string(),
            n.last_change_slot
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
