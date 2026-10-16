//! `rugsnipe-guard` (skeleton, docs/06 section 8): watches the `exits`
//! lease in `state.db`; if the hot box stops renewing it while a position is
//! open, takes it over (compare-and-set, `epoch + 1`) and sells on a
//! `drop_from_peak_bps` drop or after `grace_s`. `--dry-run` (the default)
//! reports what it would sell; `--paper-sell` patches, signs (throwaway key)
//! and records each sell with the paper sender. In probe mode `--real-sell`
//! (feature `real-senders`) loads the probe wallet and `api-keys` from
//! `$CREDENTIALS_DIRECTORY`, verifies `--armset`, and sends each dump sell
//! on a recent blockhash (`--rpc-url`) to every configured sender. Live is
//! refused. Prices come from the curve account (`--rpc-url`, feature `rpc`),
//! else only the grace rule applies.
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
    "usage: rugsnipe-guard --config <toml> --state <state.db> [--dry-run | --paper-sell | --real-sell]
    [--armset <armset-V-SHA.json>] [--once] [--interval-ms N] [--duration-s N] [--rpc-url <url>]";

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

/// `--real-sell` (probe): keys from systemd credentials, the ArmSet and the
/// sender gate verified, sells on a recent blockhash.
#[cfg(feature = "real-senders")]
fn real_sell_path(args: &[String], cfg: &rugsnipe_core::Config) -> Result<PaperSell, String> {
    let dir = rugsnipe::keys::credentials_dir().map_err(|r| r.to_string())?;
    let keys = rugsnipe::keys::load_real_keys(cfg, &dir).map_err(|rs| {
        rs.iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let (gate, stage) = match opt(args, "--armset") {
        Some(p) => {
            let r = rugsnipe::setup::load_armset_file(
                std::path::Path::new(p),
                cfg,
                rugsnipe::now_unix(),
                None,
            );
            (r.gate, r.armset.and_then(|a| a.probe_stage))
        }
        None => return Err("--real-sell needs --armset (the signed probe ArmSet)".into()),
    };
    let url = opt(args, "--rpc-url").ok_or("--real-sell needs --rpc-url (recent blockhashes)")?;
    let opts = rugsnipe::app::RealOpts {
        creds_dir: dir,
        clock_offset_ms: rugsnipe::app::chrony_offset_ms(),
        // the guard only sells: it needs no post-execution feed
        post_exec_feed_healthy: true,
        dry: false,
        rpc_url: url.to_string(),
        chain: None,
    };
    let env = rugsnipe::app::real_env(cfg, gate, stage, false, Some(&opts));
    let senders = Arc::new(
        rugsnipe_tx::senders::SenderSet::build(cfg, &env, &keys.api)
            .map_err(|e| format!("senders: {e:?}"))?,
    );
    let signer = Arc::new(keys.signer);
    let owner = signer.pubkey();
    let pool = rugsnipe_tx::nonce::NoncePool::new(
        owner,
        &rugsnipe_tx::sysix::pool_addresses(&owner, "buy", 1),
    );
    let t = rugsnipe::setup::templates(cfg, owner, &pool)?;
    let client = rugsnipe_tx::rpc::RpcClient::new(url);
    let blockhash = Arc::new(move || {
        let v = client
            .call(
                "getLatestBlockhash",
                serde_json::json!([{"commitment": "confirmed"}]),
            )
            .ok()?;
        let b = bs58::decode(v["value"]["blockhash"].as_str()?)
            .into_vec()
            .ok()?;
        <[u8; 32]>::try_from(b).ok()
    });
    eprintln!(
        "rugsnipe-guard: REAL sells from {owner} through {:?}",
        senders
    );
    Ok(PaperSell {
        template: t.sell_final,
        signer,
        paper: Arc::new(rugsnipe_tx::senders::paper::Paper::new(1_024)),
        tip_account: t.tip_account,
        bid: rugsnipe_core::bid::BidPolicy::from_cfg(&cfg.bid),
        real: Some(rugsnipe::guard::GuardReal { senders, blockhash }),
    })
}

#[cfg(not(feature = "real-senders"))]
fn real_sell_path(_args: &[String], _cfg: &rugsnipe_core::Config) -> Result<PaperSell, String> {
    Err("--real-sell needs a build with --features real-senders".into())
}

fn run(args: &[String]) -> Result<(), String> {
    let cfg = rugsnipe::setup::load_config(&PathBuf::from(
        opt(args, "--config").ok_or(format!("--config required\n{USAGE}"))?,
    ))?;
    let real_sell = flag(args, "--real-sell");
    match cfg.mode.mode {
        rugsnipe_core::Mode::Live => return Err("live is refused in this build".into()),
        rugsnipe_core::Mode::Probe if !real_sell && flag(args, "--paper-sell") => {
            return Err(
                "probe mode: --paper-sell would sell nothing real; use --dry-run or --real-sell"
                    .into(),
            )
        }
        rugsnipe_core::Mode::Paper if real_sell => {
            return Err("--real-sell needs mode = probe".into())
        }
        _ => {}
    }
    let state = PathBuf::from(opt(args, "--state").ok_or("--state required")?);
    let conn = rugsnipe_state::store::open_conn(&state).map_err(|e| e.to_string())?;
    let paper_sell = flag(args, "--paper-sell") || real_sell;
    let sell = if real_sell {
        Some(real_sell_path(args, &cfg)?)
    } else if paper_sell {
        let signer = Arc::new(rugsnipe::setup::throwaway_signer().map_err(|e| e.to_string())?);
        let pool = rugsnipe::setup::paper_nonce_pool(signer.pubkey(), 1);
        let t = rugsnipe::setup::templates(&cfg, signer.pubkey(), &pool)?;
        Some(PaperSell {
            template: t.sell_final,
            signer,
            paper: Arc::new(rugsnipe_tx::senders::paper::Paper::new(1_024)),
            tip_account: t.tip_account,
            bid: rugsnipe_core::bid::BidPolicy::from_cfg(&cfg.bid),
            real: None,
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
                GuardAction::SentReal {
                    mint,
                    tokens,
                    reason,
                    sent,
                    accepted,
                } => format!(
                    "REAL sell {tokens} of {mint} ({reason}): {accepted}/{sent} variants accepted"
                ),
                GuardAction::SellDeferred { mint, why } => {
                    format!("real sell of {mint} deferred: {why}")
                }
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
