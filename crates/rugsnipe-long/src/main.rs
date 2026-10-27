//! `rugsnipe-long`: see the crate docs (docs/20).
//!
//! ```text
//! rugsnipe-long run [--config /etc/rugsnipe/long.toml]
//!     The service: mode log (paper) or on (real), from long.toml.
//! rugsnipe-long status [--config ...] [--tz +0200] [--last 15] [--chain]
//!     Positions now, today's result (paper and real), the last closed;
//!     `--chain`: the wallet's SOL and floats.
//! rugsnipe-long float buy MINT SOL | float sell MINT all|AMOUNT [--config ...]
//!     Buy or sell a float through Jupiter from the long wallet.
//! rugsnipe-long keygen --out /etc/rugsnipe/keys/long-wallet.json
//!     A new wallet for the long copy (JSON keypair, 0400; prints the address).
//! rugsnipe-long simulate --rpc URL --tx SIG [--user PUBKEY] [--sol 0.02] [--float MINT]...
//!     Its buy SIG copied with our route from the user's wallet, simulated on
//!     the chain now: the buy alone, then buy + sell back in one transaction.
//! ```

use pump_codec::constants as c;
use pump_codec::{pda, Pubkey};
use rugsnipe_long::chain::{token_amount, Rpc};
use rugsnipe_long::config::{LongCfg, Mode};
use rugsnipe_long::detect::{self, Seen};
use rugsnipe_long::engine::Engine;
use rugsnipe_long::oracle::{now_ms, Oracle};
use rugsnipe_long::pos::{Book, State};
use rugsnipe_long::send::Out;
use rugsnipe_long::txb::{self, Fees};
use rugsnipe_long::{feed, keys, route, txlite::TxLite};
use std::time::Duration;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter()
        .position(|a| a == k)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn args_all(args: &[String], k: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == k)
        .map(|w| w[1].clone())
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("simulate") => simulate(&args[1..]),
        Some("run") => run(&args[1..]),
        Some("status") => status(&args[1..]),
        Some("float") => float(&args[1..]),
        Some("keygen") => keygen(&args[1..]),
        _ => Err("usage: rugsnipe-long run|status|float buy MINT SOL|float sell MINT all|AMOUNT|simulate ... (see --help in the source docs)".into()),
    };
    if let Err(e) = r {
        eprintln!("rugsnipe-long: {e}");
        std::process::exit(2);
    }
}

fn simulate(args: &[String]) -> Result<(), String> {
    let url = arg(args, "--rpc").ok_or("--rpc URL")?;
    let sig = arg(args, "--tx").ok_or("--tx SIG")?;
    let w4: Pubkey = arg(args, "--copy")
        .unwrap_or("H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg".into())
        .parse()
        .map_err(|_| "--copy")?;
    let user: Pubkey = arg(args, "--user")
        .unwrap_or("4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff".into())
        .parse()
        .map_err(|_| "--user")?;
    let sol: f64 = arg(args, "--sol")
        .unwrap_or("0.02".into())
        .parse()
        .map_err(|_| "--sol")?;
    let mut anchors = vec![c::WSOL_MINT];
    for f in args_all(args, "--float") {
        anchors.push(f.parse().map_err(|_| "--float MINT")?);
    }
    let mut rpc = Rpc::new(&url);
    let tx = TxLite::from_rpc_json(&rpc.transaction(&sig)?)?;
    let seen = detect::read(&tx, &w4, &|_| None);
    let t = seen
        .into_iter()
        .find_map(|s| match s {
            Seen::Buy(t) => Some(*t),
            _ => None,
        })
        .ok_or(
            "no buy of a coin priced in another token by the copied wallet in that transaction",
        )?;
    println!(
        "its buy: {} quote {} input {} amount {} valued {:?} SOL, route {}",
        t.mint,
        t.quote_mint,
        t.input_mint,
        t.input_amount,
        t.sol_value.map(|v| v as f64 / 1e9),
        t.route
            .as_ref()
            .map_or("direct curve".into(), |r| format!("{} hops", r.hops.len()))
    );
    let plan = route::plan(&t, &anchors, &mut rpc).map_err(|e| e.to_string())?;
    println!(
        "our route ({}): {:?}",
        plan.how,
        plan.route
            .mints()
            .iter()
            .map(|m| m.to_string()[..8].to_string())
            .collect::<Vec<_>>()
    );
    let amount_in = if plan.anchor == c::WSOL_MINT {
        (sol * 1e9) as u64
    } else {
        match (t.sol_value, plan.anchor == t.input_mint) {
            (Some(v), true) => (t.input_amount as f64 * sol * 1e9 / v as f64) as u64,
            _ => {
                return Err(
                    "a float anchor needs its own buy valued in SOL with the same input".into(),
                )
            }
        }
    };
    let fees = Fees {
        cu_limit: 400_000,
        cu_price: 1,
        tip: None,
    };
    let coin_ata = pda::associated_token_address(&user, &t.mint, &t.coin_program);
    let buy = txb::buy_ixs(
        &user,
        &plan.route,
        &plan.anchor_program,
        &t.coin_program,
        amount_in,
        1,
        &fees,
        0,
    );
    let (bytes, _) = txb::sign(&user, &buy, &[0; 32], |_| [0; 64]).map_err(|e| e.to_string())?;
    println!("buy transaction: {} bytes", bytes.len());
    let v = rpc.simulate(&bytes, &[coin_ata])?;
    let tokens = v["accounts"][0]["data"][0]
        .as_str()
        .map(|s| {
            token_amount(
                &base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
                    .unwrap_or_default(),
            )
        })
        .unwrap_or(0);
    println!(
        "buy: err {} units {} tokens {}",
        v["err"], v["unitsConsumed"], tokens
    );
    if !v["err"].is_null() || tokens == 0 {
        for l in v["logs"]
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            println!("   {}", l.as_str().unwrap_or(""));
        }
        return Err("the buy does not simulate".into());
    }
    // buy then sell 98% back in one transaction (one budget)
    let sell = txb::sell_ixs(
        &user,
        &plan.route.reversed(),
        &plan.anchor_program,
        &t.coin_program,
        tokens * 98 / 100,
        1,
        false,
        &fees,
        0,
    );
    let mut both = buy.clone();
    both.extend(sell.into_iter().skip(2));
    let watch = if plan.anchor == c::WSOL_MINT {
        user
    } else {
        pda::associated_token_address(&user, &plan.anchor, &plan.anchor_program)
    };
    match txb::sign(&user, &both, &[0; 32], |_| [0; 64]) {
        Err(e) => println!("round trip: does not fit one transaction ({e}); the buy simulated"),
        Ok((bytes, _)) => {
            let before = if plan.anchor == c::WSOL_MINT {
                rpc.balance(&user)?
            } else {
                rpc.token_balance(&user, &plan.anchor, &plan.anchor_program)?
            };
            let v = rpc.simulate(&bytes, &[watch])?;
            let after = if plan.anchor == c::WSOL_MINT {
                v["accounts"][0]["lamports"].as_u64().unwrap_or(0)
            } else {
                v["accounts"][0]["data"][0]
                    .as_str()
                    .map(|s| {
                        token_amount(
                            &base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
                                .unwrap_or_default(),
                        )
                    })
                    .unwrap_or(0)
            };
            println!(
                "round trip ({} bytes): err {} units {} anchor {} -> {} ({:+.2}% of the stake)",
                bytes.len(),
                v["err"],
                v["unitsConsumed"],
                before,
                after,
                (after as f64 - before as f64) / amount_in as f64 * 100.0
            );
            if !v["err"].is_null() {
                for l in v["logs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .rev()
                    .take(12)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                {
                    println!("   {}", l.as_str().unwrap_or(""));
                }
            }
        }
    }
    Ok(())
}

fn load_cfg(args: &[String]) -> Result<LongCfg, String> {
    let p = arg(args, "--config").unwrap_or("/etc/rugsnipe/long.toml".into());
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
    LongCfg::from_toml_str(&text)
}

fn load_hot(cfg: &LongCfg) -> Result<rugsnipe_core::config::Config, String> {
    let text =
        std::fs::read_to_string(&cfg.hot_config).map_err(|e| format!("{}: {e}", cfg.hot_config))?;
    rugsnipe_core::config::Config::from_toml_str(&text)
        .map_err(|e| format!("{}: {e}", cfg.hot_config))
}

fn rpc_of(cfg: &LongCfg, k: &rugsnipe_tx::senders::ApiKeys) -> Result<Rpc, String> {
    if cfg.rpc_url.is_empty() {
        return Err("long.toml: rpc_url is empty (a URL or cred:NAME)".into());
    }
    Ok(Rpc::new(&keys::resolve_url(k, &cfg.rpc_url)?))
}

fn run(args: &[String]) -> Result<(), String> {
    let cfg = load_cfg(args)?;
    let hot = load_hot(&cfg)?;
    let k = keys::api_keys(&cfg.api_keys)?;
    let rpc = rpc_of(&cfg, &k)?;
    // the wallet: required for real trading; paper uses it (if present) to
    // see our own transactions, which it never sends
    let signer = match keys::wallet(&cfg.wallet_file) {
        Ok(s) => Some(s),
        Err(e) if cfg.mode == Mode::On => {
            return Err(format!("mode on needs the long wallet: {e}"))
        }
        Err(e) => {
            eprintln!("long: no wallet ({e}): paper only");
            None
        }
    };
    let me = signer.as_ref().map(|s| s.pubkey()).unwrap_or_default();
    let (out_buy, out_sell) = if cfg.mode == Mode::On {
        (
            Some(Out::build(&hot, &k, &cfg.buy_senders)?),
            Some(Out::build(&hot, &k, &cfg.sell_senders)?),
        )
    } else {
        (None, None)
    };
    if !hot.feeds.grpc.enabled || hot.feeds.grpc.url.is_empty() {
        return Err(format!("{}: [feeds] grpc is not enabled", cfg.hot_config));
    }
    let url = keys::resolve_url(&k, &hot.feeds.grpc.url)?;
    let x_token = if hot.feeds.grpc.x_token.is_empty() {
        None
    } else {
        Some(
            k.get(&hot.feeds.grpc.x_token)
                .ok_or_else(|| {
                    format!(
                        "feeds.grpc.x_token {:?} not in the api-keys file",
                        hot.feeds.grpc.x_token
                    )
                })?
                .to_string(),
        )
    };
    println!(
        "long start: mode {:?} wallet {} copy {} min_sol {} stake {} SOL tp {}x stop {}x hold {}s max_open {} floats {:?} buy senders {:?} sell senders {:?} feed {}",
        cfg.mode,
        me,
        cfg.copy_wallet,
        cfg.min_sol,
        cfg.stake_sol,
        cfg.take_profit,
        cfg.stop,
        cfg.max_hold_s,
        cfg.max_open,
        cfg.floats,
        out_buy.as_ref().map(|o| o.labels()).unwrap_or_default(),
        out_sell.as_ref().map(|o| o.labels()).unwrap_or_default(),
        keys::redact(&url)
    );
    let oracle = Oracle::default();
    oracle.spawn(&cfg.price_api, cfg.float_pks(), Duration::from_secs(20));
    let copy = cfg.copy_pk();
    let mut eng = Engine::new(cfg, me, signer, rpc, oracle, out_buy, out_sell)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let h = feed::spawn(
        feed::FeedCfg {
            url,
            x_token,
            copied: copy,
            ours: (me != Pubkey::default()).then_some(me),
        },
        tx,
    );
    eng.set_feed(h);
    eng.run(rx);
    Ok(())
}

/// `+0200` (from `date +%z`) as seconds.
fn tz_offset(args: &[String]) -> i64 {
    let z = arg(args, "--tz").unwrap_or_default();
    let (sign, d) = match z.as_bytes().first() {
        Some(b'-') => (-1, &z[1..]),
        Some(b'+') => (1, &z[1..]),
        _ => (1, z.as_str()),
    };
    let h: i64 = d.get(..2).and_then(|x| x.parse().ok()).unwrap_or(0);
    let m: i64 = d.get(2..4).and_then(|x| x.parse().ok()).unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

fn hhmm(ms: u64, off: i64) -> String {
    let s = (ms / 1000) as i64 + off;
    let d = s.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", d / 3600, d % 3600 / 60, d % 60)
}

fn status(args: &[String]) -> Result<(), String> {
    let cfg = load_cfg(args)?;
    let off = tz_offset(args);
    let last: usize = arg(args, "--last")
        .and_then(|x| x.parse().ok())
        .unwrap_or(15);
    let dir = std::path::Path::new(&cfg.state_dir);
    let book = Book::load(&dir.join("positions.json"))?;
    let now = now_ms();
    let wallet = keys::wallet(&cfg.wallet_file).ok().map(|w| w.pubkey());
    println!(
        "== long copy (rugsnipe-long, docs/20): mode {:?}, wallet {}, copying {} buys of {}+ SOL, {} SOL each, tp {}x stop {}x {}s, max_open {}{}",
        cfg.mode,
        wallet.map_or("none (paper only)".into(), |w| w.to_string()),
        &cfg.copy_wallet[..8],
        cfg.min_sol,
        cfg.stake_sol,
        cfg.take_profit,
        cfg.stop,
        cfg.max_hold_s,
        cfg.max_open,
        if dir.join("KILL").exists() { "  KILL: no new buys" } else { "" }
    );
    if args.iter().any(|a| a == "--chain") {
        if let (Some(w), Ok(k)) = (wallet, keys::api_keys(&cfg.api_keys)) {
            if let Ok(mut rpc) = rpc_of(&cfg, &k) {
                let b = rpc
                    .balance(&w)
                    .map(|b| format!("{:.4}", b as f64 / 1e9))
                    .unwrap_or("?".into());
                let mut fl = vec![];
                for f in cfg.float_pks() {
                    let prog =
                        route::Chain::mint_program(&mut rpc, &f).unwrap_or(c::SPL_TOKEN_PROGRAM_ID);
                    let a = rpc.token_balance(&w, &f, &prog).unwrap_or(0);
                    let d = rpc.decimals(&f).unwrap_or(0);
                    fl.push(format!(
                        "{} {:.8}",
                        &f.to_string()[..8],
                        a as f64 / 10f64.powi(d as i32)
                    ));
                }
                println!(
                    "   wallet {b} SOL  floats: {}",
                    if fl.is_empty() {
                        "none".into()
                    } else {
                        fl.join(", ")
                    }
                );
            }
        }
    }
    let day = now / 86_400_000;
    for paper in [true, false] {
        let c: Vec<_> = book
            .positions
            .iter()
            .filter(|p| {
                p.paper == paper && p.state == State::Closed && p.closed_ms / 86_400_000 == day
            })
            .collect();
        if c.is_empty() && paper != (cfg.mode != Mode::On) {
            continue;
        }
        println!(
            "   today (the UTC day) {}: {} closed, {} won, net {:+.4} SOL; {} open; {} buys failed",
            if paper { "paper" } else { "REAL" },
            c.len(),
            c.iter().filter(|p| p.net_lamports > 0).count(),
            c.iter().map(|p| p.net_lamports).sum::<i64>() as f64 / 1e9,
            book.positions
                .iter()
                .filter(|p| p.paper == paper && p.live())
                .count(),
            book.positions
                .iter()
                .filter(|p| p.paper == paper
                    && p.state == State::Failed
                    && p.closed_ms / 86_400_000 == day)
                .count()
        );
    }
    let open: Vec<_> = book.positions.iter().filter(|p| p.live()).collect();
    if !open.is_empty() {
        println!("   open now:");
        println!(
            "     {:>4}  {:5}  {:8}  {:8}  {:>7}  {:11}  {:8}  {:>6}  {:>6}  {:>6}",
            "#", "side", "bought", "coin", "its SOL", "route", "state", "price", "peak", "held"
        );
        for p in open {
            println!(
                "     {:>4}  {:5}  {:8}  {:8}  {:>7.3}  {:11}  {:8}  {:>5.2}x  {:>5.2}x  {:>5}s",
                p.id,
                if p.paper { "paper" } else { "REAL" },
                hhmm(p.seen_ms, off),
                &p.mint[..8],
                p.its_lamports as f64 / 1e9,
                p.how,
                format!("{:?}", p.state).to_lowercase(),
                p.price(),
                p.peak,
                now.saturating_sub(p.seen_ms) / 1000
            );
        }
    }
    // the closed ones, newest first (closed.jsonl keeps them all)
    let text = std::fs::read_to_string(dir.join("closed.jsonl")).unwrap_or_default();
    let mut closed: Vec<rugsnipe_long::pos::Position> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    closed.sort_by_key(|p| std::cmp::Reverse(p.closed_ms));
    if !closed.is_empty() {
        println!("   last {} closed, newest first:", last.min(closed.len()));
        println!(
            "     {:>4}  {:5}  {:8}  {:8}  {:>7}  {:11}  {:11}  {:>6}  {:>6}  {:>6}  {:>8}  {:>7}",
            "#",
            "side",
            "bought",
            "coin",
            "its SOL",
            "route",
            "exit",
            "peak",
            "held",
            "",
            "net SOL",
            "pct"
        );
        for p in closed.iter().take(last) {
            println!(
                "     {:>4}  {:5}  {:8}  {:8}  {:>7.3}  {:11}  {:11}  {:>5.2}x  {:>5}s  {:>6}  {:>+8.4}  {:>+6.1}%",
                p.id,
                if p.paper { "paper" } else { "REAL" },
                hhmm(p.seen_ms, off),
                &p.mint[..8],
                p.its_lamports as f64 / 1e9,
                p.how,
                p.reason.map_or("?", |r| r.name()),
                p.peak,
                p.closed_ms.saturating_sub(p.seen_ms) / 1000,
                "",
                p.net_lamports as f64 / 1e9,
                p.net_lamports as f64 / p.stake_lamports.max(1) as f64 * 100.0
            );
        }
        let all: i64 = closed
            .iter()
            .filter(|p| !p.paper)
            .map(|p| p.net_lamports)
            .sum();
        let pap: i64 = closed
            .iter()
            .filter(|p| p.paper)
            .map(|p| p.net_lamports)
            .sum();
        println!(
            "   all closed: real {} ({:+.4} SOL), paper {} ({:+.4} SOL)",
            closed.iter().filter(|p| !p.paper).count(),
            all as f64 / 1e9,
            closed.iter().filter(|p| p.paper).count(),
            pap as f64 / 1e9
        );
    }
    Ok(())
}

/// Jupiter swap API v1 (lite).
const JUP: &str = "https://lite-api.jup.ag/swap/v1";

fn float(args: &[String]) -> Result<(), String> {
    let cfg = load_cfg(args)?;
    let k = keys::api_keys(&cfg.api_keys)?;
    let rpc = rpc_of(&cfg, &k)?;
    let signer = keys::wallet(&cfg.wallet_file)?;
    let me = signer.pubkey();
    let (side, mint) = (
        args.first().map(String::as_str),
        args.get(1).ok_or("float buy|sell MINT ...")?,
    );
    let mint: Pubkey = mint.parse().map_err(|_| "MINT")?;
    let mut chain = rpc;
    let prog = route::Chain::mint_program(&mut chain, &mint).ok_or("unknown mint")?;
    let (input, output, amount) = match side {
        Some("buy") => {
            let s: f64 = args
                .get(2)
                .ok_or("float buy MINT SOL")?
                .parse()
                .map_err(|_| "SOL")?;
            (c::WSOL_MINT, mint, (s * 1e9) as u64)
        }
        Some("sell") => {
            let held = chain.token_balance(&me, &mint, &prog)?;
            let a = match args.get(2).map(String::as_str) {
                Some("all") | None => held,
                Some(x) => x
                    .parse::<u64>()
                    .map_err(|_| "AMOUNT (raw units) or all")?
                    .min(held),
            };
            (mint, c::WSOL_MINT, a)
        }
        _ => return Err("float buy MINT SOL | float sell MINT all|AMOUNT".into()),
    };
    if amount == 0 {
        return Err("nothing to swap".into());
    }
    let q = chain.rpc.get_json(&format!(
        "{JUP}/quote?inputMint={input}&outputMint={output}&amount={amount}&slippageBps=50"
    ))?;
    println!(
        "quote: {} {} -> {} {} (impact {}%)",
        q["inAmount"],
        &input.to_string()[..8],
        q["outAmount"],
        &output.to_string()[..8],
        q["priceImpactPct"]
    );
    let body = serde_json::json!({
        "quoteResponse": q,
        "userPublicKey": me.to_string(),
        "wrapAndUnwrapSol": true,
        "dynamicComputeUnitLimit": true,
        "prioritizationFeeLamports": {"priorityLevelWithMaxLamports": {"maxLamports": 2_000_000, "priorityLevel": "high"}}
    });
    let (st, text) = chain.rpc.post_raw(
        &format!("{JUP}/swap"),
        "application/json",
        body.to_string().as_bytes(),
    )?;
    if st != 200 {
        return Err(format!(
            "jupiter swap: HTTP {st}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut tx = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        v["swapTransaction"].as_str().ok_or("no swapTransaction")?,
    )
    .map_err(|e| e.to_string())?;
    // [n_sigs (compact, 1 here)][sigs][message]: sign the message
    if tx.first() != Some(&1) {
        return Err("expected one signer".into());
    }
    let sig = signer.sign(&tx[65..]);
    tx[1..65].copy_from_slice(&sig);
    let s = chain.send(&tx)?;
    println!("sent {s}");
    for _ in 0..30 {
        std::thread::sleep(Duration::from_secs(2));
        if let Ok(st) = chain.statuses(std::slice::from_ref(&s)) {
            match st.first() {
                Some(Some(Ok(slot))) => {
                    println!(
                        "landed in slot {slot}; {} balance now {}",
                        &mint.to_string()[..8],
                        chain.token_balance(&me, &mint, &prog)?
                    );
                    return Ok(());
                }
                Some(Some(Err(e))) => return Err(format!("failed: {e}")),
                _ => {}
            }
        }
    }
    Err("not landed after 60 s (check the signature)".into())
}

fn keygen(args: &[String]) -> Result<(), String> {
    use std::io::Read as _;
    let out = arg(args, "--out").ok_or("keygen --out PATH")?;
    if std::path::Path::new(&out).exists() {
        return Err(format!("{out} exists: not overwritten"));
    }
    let mut seed = zeroize::Zeroizing::new([0u8; 32]);
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut seed[..]))
        .map_err(|e| format!("/dev/urandom: {e}"))?;
    let signer = rugsnipe_tx::signer::HotSigner::from_seed(&seed);
    let pubkey = signer.pubkey();
    let mut bytes = zeroize::Zeroizing::new(Vec::with_capacity(64));
    bytes.extend_from_slice(&seed[..]);
    bytes.extend_from_slice(pubkey.as_ref());
    let json = zeroize::Zeroizing::new(format!(
        "[{}]",
        bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",")
    ));
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o400)
            .open(&out)
            .map_err(|e| format!("{out}: {e}"))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("{out}: {e}"))?;
    }
    // the file loads back to the same address
    let back = rugsnipe_tx::signer::HotSigner::load_file(std::path::Path::new(&out))
        .map_err(|e| format!("{out}: {e}"))?;
    if back.pubkey() != pubkey {
        return Err("the written keypair does not load back".into());
    }
    println!("{pubkey}");
    eprintln!(
        "wrote {out} (0400). Fund this address with SOL; keep a copy of the file somewhere safe."
    );
    Ok(())
}
