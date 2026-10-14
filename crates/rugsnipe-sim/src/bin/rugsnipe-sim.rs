//! `rugsnipe-sim [--mint M] [--wallet W] [--nonce N] [--rpc URL] [--spend LAMPORTS] [--max-age S]`
//!
//! Picks the newest SOL-quoted curve (or `--mint`), builds the legacy and
//! v1 buy templates (with `--nonce`, also the nonce variants), simulates
//! each with loose limits (to measure) and the tight per-mint limits
//! (`65,000 + 1,500*S`, to check), and prints CU, reported and computed
//! loaded bytes, errors, and the fit of the measured CU to the formula.
//! Read-only: `simulateTransaction` with `sigVerify: false`, nothing is sent.

use pump_codec::Pubkey;
use rugsnipe_core::config::TxFormat;
use rugsnipe_sim::live::*;
use rugsnipe_sim::Case;
use rugsnipe_tx::rpc::RpcClient;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("usage: rugsnipe-sim [--mint M] [--wallet W] [--nonce N] [--rpc URL] [--spend LAMPORTS] [--max-age S]");
        return;
    }
    let rpc = RpcClient::new(
        &arg(&args, "--rpc").unwrap_or_else(|| "https://api.mainnet-beta.solana.com".into()),
    );
    let spend: u64 = arg(&args, "--spend")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000_000);
    let max_age: u64 = arg(&args, "--max-age")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let r = (|| -> Result<(), String> {
        let mut coin = match arg(&args, "--mint") {
            Some(m) => {
                let mint: Pubkey = m.parse().map_err(|_| "bad --mint")?;
                Coin {
                    mint,
                    bonding_curve: pump_codec::pda::bonding_curve(&mint),
                    token_program: pump_codec::constants::TOKEN_2022_PROGRAM_ID,
                    created_ms: 0,
                    symbol: String::new(),
                    virtual_sol: 0,
                    real_sol: 0,
                }
            }
            None => fresh_coin(&rpc, now_ms, max_age)?,
        };
        read_curve(&rpc, &mut coin)?;
        println!(
            "mint      {} ({}) age {:.1}s",
            coin.mint,
            coin.symbol,
            now_ms.saturating_sub(coin.created_ms) as f64 / 1000.0
        );
        println!(
            "curve     {}  virtual_sol {:.4} SOL  real_sol {:.4} SOL",
            coin.bonding_curve,
            coin.virtual_sol as f64 / 1e9,
            coin.real_sol as f64 / 1e9
        );
        let fallback: Pubkey = "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9"
            .parse()
            .unwrap();
        let (wallet, bal, uva_exists) = match arg(&args, "--wallet") {
            Some(w) => {
                let w: Pubkey = w.parse().map_err(|_| "bad --wallet")?;
                let uva =
                    account_data(&rpc, &pump_codec::pda::user_volume_accumulator(&w))?.is_some();
                (w, 0, uva)
            }
            None => pick_wallet(&rpc, &coin.bonding_curve, spend + 50_000_000, &fallback)?,
        };
        println!(
            "wallet    {wallet} ({:.3} SOL; user_volume_accumulator {})",
            bal as f64 / 1e9,
            if uva_exists {
                "exists"
            } else {
                "missing: the buy creates it"
            }
        );
        let nonce = match arg(&args, "--nonce") {
            Some(n) => {
                let acct: Pubkey = n.parse().map_err(|_| "bad --nonce")?;
                let (auth, value) = read_nonce(&rpc, &acct)?;
                println!(
                    "nonce     {acct} authority {auth} value {}",
                    bs58::encode(value).into_string()
                );
                Some((acct, auth, value))
            }
            None => None,
        };
        let mut cases = vec![];
        for f in [TxFormat::Legacy, TxFormat::V1] {
            for n in [false, true] {
                for tight in [false, true] {
                    cases.push(Case {
                        format: f,
                        nonce: n,
                        tight,
                    });
                }
            }
        }
        let rows = run_cases(&rpc, &coin, &wallet, nonce, &cases, spend)?;
        println!(
            "\n{:<22} {:>5} {:>9} {:>11} {:>9} {:>11} {:>11}  result",
            "template", "bytes", "cu_limit", "lads_limit", "cu_used", "loaded_rpc", "loaded_calc"
        );
        for (r, calc) in &rows {
            println!(
                "{:<22} {:>5} {:>9} {:>11} {:>9} {:>11} {:>11}  {}",
                r.label,
                r.size,
                r.cu_limit,
                r.lads_limit,
                r.units_consumed
                    .map(|v| v.to_string())
                    .unwrap_or("-".into()),
                r.loaded_reported
                    .map(|v| v.to_string())
                    .unwrap_or("-".into()),
                calc,
                r.err.clone().unwrap_or_else(|| "ok".into())
            );
            if r.err.is_some() {
                for l in &r.logs_tail {
                    println!("    | {l}");
                }
            }
        }
        let fits: Vec<(Pubkey, rugsnipe_sim::Fit)> = rows
            .iter()
            .filter_map(|(r, _)| rugsnipe_sim::fit(r).map(|f| (coin.mint, f)))
            .collect();
        println!(
            "\nper-mint CU (gap-5): limit = 65,000 + 1,500*S; model = 62,310 + 1,500*S + 1,185 (v1+nonce)"
        );
        print!("{}", rugsnipe_sim::fit_report(&fits));
        Ok(())
    })();
    if let Err(e) = r {
        eprintln!("rugsnipe-sim: {e}");
        std::process::exit(1);
    }
}
