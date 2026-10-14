//! `rugsnipe-guard` (skeleton, docs/06 section 8): watches the `exits`
//! lease in `state.db`; if the hot box stops renewing it while a position is
//! open, takes it over (compare-and-set, `epoch + 1`) and sells on a
//! `drop_from_peak_bps` drop or after `grace_s`. Paper only: `--dry-run`
//! (the default) reports what it would sell; `--paper-sell` patches, signs
//! (throwaway key) and records each sell with the paper sender. Prices come
//! from the curve account (`--rpc-url`, feature `rpc`), else only the grace
//! rule applies.
//!
//! ```text
//! rugsnipe-guard --config config/paper.example.toml --state data/raw/paper/state.db [--once]
//! ```

use rugsnipe::cli::{flag, num, opt};
use rugsnipe::guard::{Guard, GuardAction, GuardCfg, PaperSell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Current virtual SOL of a mint's curve, if known.
type PriceFn = Box<dyn Fn(&pump_codec::Pubkey) -> Option<u64>>;

const USAGE: &str =
    "usage: rugsnipe-guard --config <toml> --state <state.db> [--dry-run | --paper-sell]
    [--once] [--interval-ms N] [--duration-s N] [--rpc-url <url>]";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if flag(&args, "--help") || flag(&args, "-h") {
        println!("{USAGE}");
        return;
    }
    if let Err(e) = run(&args) {
        eprintln!("rugsnipe-guard: {e}");
        std::process::exit(2);
    }
}

#[cfg(feature = "rpc")]
fn price_source(args: &[String], cfg: &rugsnipe_core::Config) -> PriceFn {
    use rugsnipe_state::ChainRead;
    let Some(url) = opt(args, "--rpc-url") else {
        return Box::new(|_| None);
    };
    let budget = Arc::new(
        rugsnipe_state::RpcBudget::for_role(&cfg.rpc_budget, "guard")
            .unwrap_or(rugsnipe_state::RpcBudget::new(4.0)),
    );
    let chain = rugsnipe_state::chain::RpcChain::new(url, budget);
    Box::new(move |mint| {
        let bc = pump_codec::pda::bonding_curve(mint);
        let data = chain.accounts(&[bc]).ok()?.pop()??;
        pump_codec::curve::BondingCurve::decode(&data)
            .ok()
            .map(|b| b.virtual_quote_reserves)
    })
}

#[cfg(not(feature = "rpc"))]
fn price_source(args: &[String], _cfg: &rugsnipe_core::Config) -> PriceFn {
    if opt(args, "--rpc-url").is_some() {
        eprintln!("--rpc-url needs the `rpc` feature; prices unknown (grace rule only)");
    }
    Box::new(|_| None)
}

fn run(args: &[String]) -> Result<(), String> {
    let cfg = rugsnipe::setup::load_config(&PathBuf::from(
        opt(args, "--config").ok_or(format!("--config required\n{USAGE}"))?,
    ))?;
    if cfg.mode.mode.is_real() {
        return Err("the guard skeleton is paper only (no real sender is ever built here)".into());
    }
    let state = PathBuf::from(opt(args, "--state").ok_or("--state required")?);
    let conn = rugsnipe_state::store::open_conn(&state).map_err(|e| e.to_string())?;
    let paper_sell = flag(args, "--paper-sell");
    let sell = if paper_sell {
        let signer = Arc::new(rugsnipe::setup::throwaway_signer().map_err(|e| e.to_string())?);
        let pool = rugsnipe::setup::paper_nonce_pool(signer.pubkey(), 1);
        let t = rugsnipe::setup::templates(&cfg, signer.pubkey(), &pool)?;
        Some(PaperSell {
            template: t.sell_final,
            signer,
            paper: Arc::new(rugsnipe_tx::senders::paper::Paper::new(1_024)),
            tip_account: t.tip_account,
        })
    } else {
        None
    };
    let mut guard = Guard::new(
        GuardCfg {
            owner: format!("guard:{}:{}", cfg.box_.id, std::process::id()),
            takeover_after_ms: cfg.guard.takeover_after_ms,
            drop_from_peak_bps: cfg.guard.drop_from_peak_bps,
            grace_s: cfg.guard.grace_s,
            dry_run: !paper_sell,
        },
        sell,
    );
    let price = price_source(args, &cfg);
    let term = Arc::new(AtomicBool::new(false));
    for s in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(s, term.clone()).map_err(|e| e.to_string())?;
    }
    let once = flag(args, "--once");
    let every = Duration::from_millis(num(args, "--interval-ms", 500u64)?);
    let duration = num(args, "--duration-s", 0u64)?;
    let t0 = Instant::now();
    let mut last = String::new();
    loop {
        let actions = guard.step(&conn, rugsnipe::now_ms(), price.as_ref())?;
        let line = actions
            .iter()
            .map(|a| match a {
                GuardAction::Idle(w) => format!("idle: {w}"),
                GuardAction::HotAlive { owner, age_ms } => {
                    format!("exits lease held by {owner} (heartbeat {age_ms} ms ago)")
                }
                GuardAction::TookOver { epoch, from } => {
                    format!("TOOK OVER exits lease epoch {epoch} from {from:?}")
                }
                GuardAction::WouldSell {
                    mint,
                    tokens,
                    reason,
                } => format!("dry-run: would sell {tokens} of {mint} ({reason})"),
                GuardAction::Sold {
                    mint,
                    tokens,
                    reason,
                    sig,
                } => format!(
                    "paper sell {tokens} of {mint} ({reason}) sig {}",
                    bs58::encode(sig).into_string()
                ),
                GuardAction::Released => "released the exits lease (no open positions)".into(),
            })
            .collect::<Vec<_>>()
            .join("; ");
        if line != last || once {
            println!("{line}");
            last = line;
        }
        if once || term.load(Acquire) || (duration > 0 && t0.elapsed().as_secs() >= duration) {
            break;
        }
        std::thread::sleep(every);
    }
    if guard.holds_lease() {
        eprintln!("guard exiting while holding the exits lease");
    }
    Ok(())
}
