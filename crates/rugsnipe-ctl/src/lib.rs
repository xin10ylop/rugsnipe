//! `rugsnipe-ctl` logic as a library (the binary is a thin wrapper, so every
//! subcommand is testable without spawning processes).
//!
//! ```text
//! rugsnipe-ctl config validate <file> [--creds DIR] [--clock-offset-ms X] [--post-exec-healthy]
//!                                     [--armset FILE [--sig FILE]] [--regime R] [--now UNIX] [--live]
//! rugsnipe-ctl template dump --mint M --wallet W [--format legacy|v1] [--nonce ACCT [--nonce-authority A]]
//!                            [--tip-account T] [--cu N | --cu-floor F] [--lads N] [--spend L] [--min-out N] [--prio L] [--tip L]
//!                            (default CU = the hot path's per-mint 65,000 + 1,500*S over --cu-floor, default 88,000)
//! rugsnipe-ctl sim [--n 3] [--rpc URL] [--wallet W] [--nonce N] [--max-age S] [--spend L] [--legacy]
//!                  (feature `rpc`; read-only simulateTransaction, sigVerify false: measured vs formula CU per mint)
//! rugsnipe-ctl nonce init  --authority A [--count N] [--pool buy|sell] [--blockhash B58] [--lamports L]
//! rugsnipe-ctl nonce close --authority A --to T [--count N] [--pool buy|sell] [--blockhash B58] [--lamports L]
//! rugsnipe-ctl armset verify <file> --pubkey PK [--sig FILE] [--regime R] [--max-age S] [--now UNIX]
//! rugsnipe-ctl armset keygen --out FILE           (dev/paper signing key; refused inside a git tree except keys/)
//! rugsnipe-ctl armset sign --key FILE --in JSON [--out-dir DIR]   (writes armset-<ver>-<sha>.json + .sig)
//! rugsnipe-ctl kill status --state DB        (the persisted kill latch and every trigger row)
//! rugsnipe-ctl unkill --state DB             (local only: clears the persisted latch; restart the service after)
//! ```
//!
//! Nothing here signs or sends: nonce init/close print **unsigned**
//! transactions (dry run) for review and offline signing; `sim` only calls
//! `simulateTransaction` (read-only, `sigVerify: false`).

use base64::Engine as _;
use pump_codec::constants as c;
use pump_codec::instructions::SolV3Keys;
use pump_codec::tx::TxView;
use pump_codec::Pubkey;
use rugsnipe_core::armset::{self, VerifyCtx};
use rugsnipe_core::config::{ArmSetGate, Config, TxFormat, ValidateEnv};
use rugsnipe_tx::cu::{snipe_cu_limit, RECOMMENDED_CU_FLOOR};
use rugsnipe_tx::msg::{compile, Format, KeySpec};
use rugsnipe_tx::sysix;
use rugsnipe_tx::template::{CurveParams, MintKeys, NonceKeys, Patch, Template};
use std::fmt::Write as _;
use std::path::Path;

/// Output and exit code of a subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub text: String,
    pub code: i32,
}

fn err(msg: impl Into<String>) -> Out {
    Out {
        text: msg.into(),
        code: 2,
    }
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}
fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}
fn pk_opt(args: &[String], name: &str) -> Result<Option<Pubkey>, String> {
    opt(args, name)
        .map(|s| s.parse().map_err(|_| format!("{name}: bad pubkey {s:?}")))
        .transpose()
}
fn num<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match opt(args, name) {
        None => Ok(default),
        Some(s) => s
            .replace('_', "")
            .parse()
            .map_err(|_| format!("{name}: bad number {s:?}")),
    }
}
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Dispatch `args` (without the program name).
pub fn run(args: &[String]) -> Out {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = match a.as_slice() {
        ["config", "validate", file, ..] => config_validate(file, &args[3..]),
        ["template", "dump", ..] => template_dump(&args[2..]),
        ["sim", ..] => sim(&args[1..]),
        ["nonce", "init", ..] => nonce_txs(&args[2..], true),
        ["nonce", "close", ..] => nonce_txs(&args[2..], false),
        ["armset", "verify", file, ..] => armset_verify(file, &args[3..]),
        ["armset", "keygen", ..] => armset_keygen(&args[2..]),
        ["armset", "sign", ..] => armset_sign(&args[2..]),
        ["kill", "status", ..] => kill_status(&args[2..]),
        ["unkill", ..] => unkill(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    r.unwrap_or_else(err)
}

pub const USAGE: &str = "usage: rugsnipe-ctl <config validate FILE | template dump | sim | nonce init | nonce close | armset verify FILE | armset keygen | armset sign | kill status | unkill> [options]\n(see `rugsnipe-ctl` crate docs for options)";

fn state_path(args: &[String]) -> Result<&Path, String> {
    let p = opt(args, "--state").ok_or("--state <state.db> required")?;
    if !Path::new(p).exists() {
        return Err(format!("{p}: no such state.db"));
    }
    Ok(Path::new(p))
}

/// `kill status`: the persisted latch (docs/08 section 10) and every trigger row.
pub fn kill_status(args: &[String]) -> Result<Out, String> {
    let p = state_path(args)?;
    let c = rugsnipe_state::store::open_conn(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut t = String::new();
    match rugsnipe_state::killkv::latched(&c).map_err(|e| e.to_string())? {
        Some(l) => {
            let _ = writeln!(t, "KILLED: {} at {} ({})", l.reason, l.unix, l.detail);
        }
        None => {
            let _ = writeln!(t, "not killed");
        }
    }
    for (name, l) in rugsnipe_state::killkv::triggers(&c).map_err(|e| e.to_string())? {
        let _ = writeln!(
            t,
            "trigger {name}: {} at {} ({})",
            l.reason, l.unix, l.detail
        );
    }
    Ok(Out { text: t, code: 0 })
}

/// `unkill`: clear the persisted latch. Local and deliberate: the operator
/// runs it on the box (the hot process re-reads it at start; restart the
/// service after). The trigger rows stay as history.
pub fn unkill(args: &[String]) -> Result<Out, String> {
    let p = state_path(args)?;
    let c = rugsnipe_state::store::open_conn(p).map_err(|e| format!("{}: {e}", p.display()))?;
    match rugsnipe_state::killkv::unkill(&c, now_unix()).map_err(|e| e.to_string())? {
        Some(l) => Ok(Out {
            text: format!(
                "unkilled: cleared the latch {} ({}, at {}); restart rugsnipe-hot for buys to resume (remove the kill file first if it exists)\n",
                l.reason, l.detail, l.unix
            ),
            code: 0,
        }),
        None => Ok(Out {
            text: "not killed: nothing to clear\n".into(),
            code: 0,
        }),
    }
}

/// Mode bits of a credential file (`None` if missing).
fn mode_of(p: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .ok()
            .map(|m| m.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(p).ok().map(|_| 0o400)
    }
}

#[allow(clippy::type_complexity)]
fn armset_gate(
    cfg: &Config,
    args: &[String],
) -> Result<
    (
        ArmSetGate,
        String,
        Option<rugsnipe_core::config::ProbeStage>,
    ),
    String,
> {
    let Some(file) = opt(args, "--armset") else {
        return Ok((ArmSetGate::Missing, "no --armset given".into(), None));
    };
    let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let sig_path = opt(args, "--sig")
        .map(str::to_string)
        .unwrap_or(format!("{file}.sig"));
    let sig = std::fs::read(&sig_path).ok();
    let Ok(pubkey) = cfg.secrets.armset_pubkey.parse::<Pubkey>() else {
        return Ok((
            ArmSetGate::BadSignature,
            "secrets.armset_pubkey is not a pubkey".into(),
            None,
        ));
    };
    let expected = armset::parse_filename(file).map(|(_, s)| s);
    let ctx = VerifyCtx {
        pubkey: &pubkey,
        expected_sha: expected.as_deref(),
        now_unix: num(args, "--now", now_unix())?,
        max_age_s: cfg.risk.armset_max_age_s,
        regime: opt(args, "--regime"),
        hot_version: env!("CARGO_PKG_VERSION"),
    };
    Ok(match armset::verify(&bytes, sig.as_deref(), &ctx) {
        Ok(v) => (
            v.gate(),
            format!(
                "ArmSet ver {} sha {} gate_passed={} probe_allowed={} probe_stage={}",
                v.armset.ver,
                v.sha256,
                v.armset.gate_passed,
                v.armset.probe_allowed,
                v.armset.probe_stage.map_or("-", |s| s.as_str())
            ),
            v.armset.probe_stage,
        ),
        Err(e) => (e.gate(), format!("ArmSet rejected: {e}"), None),
    })
}

/// `config validate`: parse, stat credentials, verify the ArmSet, list every refusal.
pub fn config_validate(file: &str, args: &[String]) -> Result<Out, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let cfg = Config::from_toml_str(&text).map_err(|e| format!("{file}: {e}"))?;
    let creds_dir = opt(args, "--creds")
        .map(str::to_string)
        .or_else(|| std::env::var("CREDENTIALS_DIRECTORY").ok())
        .unwrap_or_else(|| "/etc/rugsnipe/keys".into());
    let mut env = ValidateEnv {
        clock_offset_ms: opt(args, "--clock-offset-ms").and_then(|s| s.parse().ok()),
        post_exec_feed_healthy: flag(args, "--post-exec-healthy"),
        live_cli_flag: flag(args, "--live"),
        arm_live_token: flag(args, "--arm-live-token"),
        ..Default::default()
    };
    for name in [
        &cfg.secrets.hot_keypair,
        &cfg.secrets.probe_keypair,
        &cfg.secrets.api_keys,
    ] {
        if !name.is_empty() {
            env.credential_modes
                .insert(name.clone(), mode_of(&Path::new(&creds_dir).join(name)));
        }
    }
    let (gate, note, stage) = armset_gate(&cfg, args)?;
    env.armset = gate;
    env.armset_probe_stage = stage;
    let mut v = cfg.validate(&env);
    // `[exit.default]` is passed raw to the exit engine, which refuses
    // unknown keys when the hot binary starts: refuse them here too
    if let Err(why) = exit_check(&cfg) {
        v.refusals
            .push(rugsnipe_core::config::Refusal::Invalid { field: "exit", why });
    }
    // rugsnipe-hot loads `entry.filter_model` at startup and refuses to
    // start without it: check the file here too (same path rule as
    // `rugsnipe::filter::resolve_path`: as given, else from the source tree)
    if let Err(why) = filter_model_check(&cfg.entry.filter_model) {
        v.refusals.push(rugsnipe_core::config::Refusal::Invalid {
            field: "entry.filter_model",
            why,
        });
    }
    let mut s = String::new();
    let _ = writeln!(s, "config {file}: mode = {}", v.mode);
    let _ = writeln!(s, "armset: {note}");
    for w in &v.warnings {
        let _ = writeln!(s, "warning: {w}");
    }
    for r in &v.refusals {
        let _ = writeln!(s, "REFUSED: {r}");
    }
    let _ = writeln!(s, "{}", if v.ok() { "OK" } else { "NOT OK" });
    Ok(Out {
        text: s,
        code: if v.ok() { 0 } else { 1 },
    })
}

/// `[exit.default]` as the hot binary reads it (`rugsnipe::setup::exit_policy`).
fn filter_model_check(p: &str) -> Result<(), String> {
    if p.is_empty() {
        return Ok(());
    }
    let given = Path::new(p);
    let path = if given.is_absolute() || given.exists() {
        given.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p)
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{p}: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{p}: not JSON: {e}"))?;
    if v.get("model").is_none() || v.get("features").is_none() {
        return Err(format!(
            "{p}: not an entry-filter export (no model/features)"
        ));
    }
    Ok(())
}

fn exit_check(cfg: &Config) -> Result<(), String> {
    let ec: rugsnipe_exit::ExitConfig = match cfg.exit.get("default") {
        Some(v) => v
            .clone()
            .try_into()
            .map_err(|e| format!("[exit.default]: {e}"))?,
        None => rugsnipe_exit::ExitConfig::default(),
    };
    let spend = match cfg.risk.max_sell_spend_lamports {
        0 => 20_000_000,
        n => n,
    };
    ec.to_policy(spend)
        .map(|_| ())
        .map_err(|e| format!("[exit.default]: {e:?}"))
}

fn describe(raw: &[u8]) -> String {
    let mut s = String::new();
    let Ok(tx) = TxView::parse(raw) else {
        return "(unparseable)".into();
    };
    let keys = tx.static_account_keys();
    let _ = writeln!(
        s,
        "version {:?}, {} bytes, {} signer(s), {} keys",
        tx.version(),
        raw.len(),
        tx.num_signatures(),
        keys.len()
    );
    if let Some(cfg) = tx.v1_config() {
        let _ = writeln!(
            s,
            "v1 config mask {:#x}: priority {:?} lamports, cu {:?}, lads {:?}",
            cfg.mask,
            cfg.priority_fee_lamports,
            cfg.compute_unit_limit,
            cfg.loaded_accounts_data_size_limit
        );
    }
    let _ = writeln!(
        s,
        "lifetime {}",
        bs58::encode(tx.recent_blockhash()).into_string()
    );
    for (i, k) in keys.iter().enumerate() {
        let _ = writeln!(
            s,
            "  key {i:>2} {}{} {k}",
            if tx.is_signer(i) { "s" } else { "-" },
            if tx.is_writable(i) { "w" } else { "r" }
        );
    }
    for (i, ix) in tx.instructions().enumerate() {
        let hex: String = ix
            .data
            .iter()
            .take(32)
            .map(|b| format!("{b:02x}"))
            .collect();
        let _ = writeln!(
            s,
            "  ix {i} program {} accounts {:?} data {hex}{}",
            tx.program_id(&ix),
            ix.accounts,
            if ix.data.len() > 32 { "…" } else { "" }
        );
    }
    s
}

/// `template dump`: build + patch a buy template for a mint and print it.
pub fn template_dump(args: &[String]) -> Result<Out, String> {
    let mint = pk_opt(args, "--mint")?.ok_or("--mint required")?;
    let wallet = pk_opt(args, "--wallet")?.ok_or("--wallet required")?;
    let format = match opt(args, "--format").unwrap_or("v1") {
        "legacy" => TxFormat::Legacy,
        "v1" => TxFormat::V1,
        f => return Err(format!("--format {f:?}: legacy|v1")),
    };
    let nonce = match pk_opt(args, "--nonce")? {
        Some(account) => Some(NonceKeys {
            account,
            authority: pk_opt(args, "--nonce-authority")?.unwrap_or(wallet),
        }),
        None => None,
    };
    let keys = MintKeys::from_v3(&SolV3Keys::derive(
        mint,
        wallet,
        c::TOKEN_2022_PROGRAM_ID,
        c::BUYBACK_FEE_RECIPIENTS[0],
    ));
    // the hot path's per-mint limit (gap-5) unless --cu pins one
    let floor: u32 = num(args, "--cu-floor", RECOMMENDED_CU_FLOOR)?;
    // the mint's four bumps plus the wallet's UVA term (as the hot path)
    let s_missing = keys
        .snipe_bumps(&wallet, &c::TOKEN_2022_PROGRAM_ID)
        .map(|b| b.missing() + rugsnipe_tx::cu::wallet_missing_bumps(&wallet))
        .ok_or("derived keys are not canonical PDAs (cannot happen)")?;
    let explicit_cu: Option<u32> = opt(args, "--cu")
        .map(|_| num(args, "--cu", 0))
        .transpose()?;
    let per_mint = snipe_cu_limit(s_missing, floor);
    let p = CurveParams {
        format,
        user: wallet,
        nonce,
        cu_limit: explicit_cu.unwrap_or(floor.max(1)),
        lads_bytes: num(args, "--lads", 12_124_160)?,
        tip_account: Some(pk_opt(args, "--tip-account")?.unwrap_or(c::JITO_TIP_ACCOUNTS[0])),
        buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[0],
        base_token_program: c::TOKEN_2022_PROGRAM_ID,
        partial_fill: Some(false),
    };
    let mut t = Template::buy(&p).map_err(|e| e.to_string())?;
    let lifetime: [u8; 32] = match opt(args, "--blockhash") {
        Some(b) => bs58::decode(b)
            .into_vec()
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or("--blockhash: bad base58 hash")?,
        None => [0u8; 32],
    };
    t.patch(&Patch {
        keys: &keys,
        lifetime: &lifetime,
        amount: num(args, "--spend", 10_000_000)?,
        limit: num(args, "--min-out", 1)?,
        priority_lamports: num(args, "--prio", 10_000)?,
        tip_lamports: num(args, "--tip", 1_000_000)?,
        cu_limit: explicit_cu.is_none().then_some(per_mint),
    });
    let mut s = String::new();
    let _ = writeln!(
        s,
        "buy template ({format:?}{}), unsigned",
        if nonce.is_some() { " + nonce" } else { "" }
    );
    let _ = writeln!(
        s,
        "cu_limit {} ({}; S + W = {s_missing} (W = wallet UVA term), per-mint 65,000 + 1,500*(S + W) = {}, floor {floor})",
        t.cu_limit(),
        if explicit_cu.is_some() {
            "--cu"
        } else {
            "per-mint"
        },
        snipe_cu_limit(s_missing, 0)
    );
    let _ = writeln!(s, "patch table: {:?}", t.table);
    s.push_str(&describe(t.bytes()));
    let _ = writeln!(
        s,
        "base64 {}",
        base64::engine::general_purpose::STANDARD.encode(t.bytes())
    );
    Ok(Out { text: s, code: 0 })
}

/// `sim`: measured vs formula CU of the per-mint limit on a few live mints
/// (read-only `simulateTransaction`, `sigVerify: false`; feature `rpc`).
/// Exit code 1 if a tight (per-mint limit) run fails or a measured cost is
/// above its limit.
#[cfg(feature = "rpc")]
pub fn sim(args: &[String]) -> Result<Out, String> {
    use rugsnipe_sim::live;
    let rpc = rugsnipe_tx::rpc::RpcClient::new(
        opt(args, "--rpc").unwrap_or("https://api.mainnet-beta.solana.com"),
    );
    let n: usize = num(args, "--n", 3)?;
    let spend: u64 = num(args, "--spend", 10_000_000)?;
    let max_age: u64 = num(args, "--max-age", 300)?;
    let now_ms = now_unix() * 1_000;
    let formats: Vec<TxFormat> = if flag(args, "--legacy") {
        vec![TxFormat::V1, TxFormat::Legacy]
    } else {
        vec![TxFormat::V1]
    };
    let wallet = match pk_opt(args, "--wallet")? {
        Some(w) => w,
        None => {
            let coin = live::fresh_coin(&rpc, now_ms, max_age)?;
            let fallback: Pubkey = "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9"
                .parse()
                .unwrap();
            live::pick_wallet(&rpc, &coin.bonding_curve, spend + 50_000_000, &fallback)?.0
        }
    };
    let nonce = match pk_opt(args, "--nonce")? {
        Some(acct) => {
            let (auth, value) = live::read_nonce(&rpc, &acct)?;
            Some((acct, auth, value))
        }
        None => None,
    };
    let (mints, skipped) =
        live::fit_live(&rpc, now_ms, max_age, n, &wallet, nonce, &formats, spend)?;
    Ok(sim_report(&wallet, &mints, &skipped))
}

/// `sim` without the `rpc` feature.
#[cfg(not(feature = "rpc"))]
pub fn sim(_args: &[String]) -> Result<Out, String> {
    Err("`rugsnipe-ctl sim` needs the `rpc` feature: cargo run -p rugsnipe-ctl --features rpc -- sim [--n 3] [--nonce N] [--wallet W] [--rpc URL]".into())
}

/// Render a `sim` run (also used by tests with synthetic rows).
#[cfg(feature = "rpc")]
fn sim_report(wallet: &Pubkey, mints: &[rugsnipe_sim::live::MintFit], skipped: &[String]) -> Out {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "ctl sim (read-only, sigVerify false): wallet {wallet}; S includes the wallet UVA term W; limit = 65,000 + 1,500*S, model = 62,310 + 1,500*S + 1,185 (v1+nonce; -150 without nonce, +450 legacy)"
    );
    let mut fits = vec![];
    let mut failures = vec![];
    for m in mints {
        let _ = writeln!(
            s,
            "mint {} ({}) S = {}",
            m.coin.mint,
            m.coin.symbol,
            m.rows.first().map(|(r, _)| r.s).unwrap_or(0)
        );
        for (r, _) in &m.rows {
            let _ = writeln!(
                s,
                "  {:<18} cu_limit {:>9} used {:>7} {}",
                r.label,
                r.cu_limit,
                r.units_consumed
                    .map(|v| v.to_string())
                    .unwrap_or("-".into()),
                r.err.clone().unwrap_or_else(|| "ok".into())
            );
        }
        fits.extend(m.fits.iter().map(|f| (m.coin.mint, f.clone())));
        failures.extend(
            m.tight_failures
                .iter()
                .map(|f| format!("{}: {f}", m.coin.mint)),
        );
    }
    for k in skipped {
        let _ = writeln!(s, "skipped {k}");
    }
    s.push_str(&rugsnipe_sim::fit_report(&fits));
    for f in &failures {
        let _ = writeln!(s, "TIGHT FAILED {f}");
    }
    let bad = !failures.is_empty() || fits.is_empty() || fits.iter().any(|(_, f)| f.headroom <= 0);
    Out {
        text: s,
        code: i32::from(bad),
    }
}

/// `nonce init|close`: print the unsigned transactions (dry run).
pub fn nonce_txs(args: &[String], init: bool) -> Result<Out, String> {
    let authority = pk_opt(args, "--authority")?.ok_or("--authority required")?;
    let count: usize = num(args, "--count", 32)?;
    let pool = opt(args, "--pool").unwrap_or("buy");
    if !["buy", "sell"].contains(&pool) {
        return Err("--pool buy|sell".into());
    }
    let blockhash: [u8; 32] = match opt(args, "--blockhash") {
        Some(b) => bs58::decode(b)
            .into_vec()
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or("--blockhash: bad base58 hash")?,
        None => [0u8; 32],
    };
    let lamports: u64 = num(args, "--lamports", sysix::NONCE_RENT_LAMPORTS)?;
    let to = if init {
        None
    } else {
        Some(pk_opt(args, "--to")?.ok_or("--to required for close")?)
    };
    let mut s = String::new();
    let _ = writeln!(
        s,
        "DRY RUN: {} {count} {pool} nonce account(s), authority {authority}; nothing is signed or sent.{}",
        if init { "create" } else { "close" },
        if blockhash == [0; 32] { " Lifetime is zero: pass --blockhash before signing." } else { "" }
    );
    let total = lamports as u128 * count as u128;
    let _ = writeln!(
        s,
        "{} {} lamports ({:.6} SOL)",
        if init { "rent locked:" } else { "withdrawn:" },
        total,
        total as f64 / 1e9
    );
    for i in 0..count {
        let (addr, ixs) = if init {
            let (a, ixs) = sysix::nonce_init_instructions(&authority, pool, i, lamports);
            (a, ixs.to_vec())
        } else {
            let a = sysix::create_with_seed(
                &authority,
                &sysix::nonce_seed(pool, i),
                &c::SYSTEM_PROGRAM_ID,
            );
            (
                a,
                vec![sysix::withdraw_nonce_account(
                    &a,
                    &authority,
                    &to.unwrap(),
                    lamports,
                )],
            )
        };
        let mut keys = vec![
            KeySpec::new(authority, true, true),
            KeySpec::new(addr, false, true),
        ];
        if let Some(t) = to {
            if t != authority {
                keys.push(KeySpec::new(t, false, true));
            }
        }
        keys.push(KeySpec::new(c::SYSTEM_PROGRAM_ID, false, false));
        keys.push(KeySpec::new(c::RECENT_BLOCKHASHES_SYSVAR_ID, false, false));
        keys.push(KeySpec::new(c::RENT_SYSVAR_ID, false, false));
        let (raw, _) =
            compile(&keys, &ixs, &blockhash, Format::Legacy).map_err(|e| e.to_string())?;
        let _ = writeln!(s, "\n#{i} {} -> {addr}", sysix::nonce_seed(pool, i));
        s.push_str(&describe(&raw));
        let _ = writeln!(
            s,
            "base64 {}",
            base64::engine::general_purpose::STANDARD.encode(&raw)
        );
    }
    Ok(Out { text: s, code: 0 })
}

/// `armset verify`.
pub fn armset_verify(file: &str, args: &[String]) -> Result<Out, String> {
    let pubkey = pk_opt(args, "--pubkey")?.ok_or("--pubkey required")?;
    let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let sig_path = opt(args, "--sig")
        .map(str::to_string)
        .unwrap_or(format!("{file}.sig"));
    let sig = std::fs::read(&sig_path).ok();
    let expected = armset::parse_filename(file).map(|(_, s)| s);
    let ctx = VerifyCtx {
        pubkey: &pubkey,
        expected_sha: expected.as_deref(),
        now_unix: num(args, "--now", now_unix())?,
        max_age_s: num(args, "--max-age", 21_600)?,
        regime: opt(args, "--regime"),
        hot_version: env!("CARGO_PKG_VERSION"),
    };
    Ok(match armset::verify(&bytes, sig.as_deref(), &ctx) {
        Ok(v) => Out {
            text: format!(
                "OK: ArmSet ver {} sha256 {} regime {} gate_passed={} probe_allowed={} operators={} watch={} funding_rules={}{}\n",
                v.armset.ver,
                v.sha256,
                v.armset.regime,
                v.armset.gate_passed,
                v.armset.probe_allowed,
                v.armset.operators.len(),
                v.armset.watch.len(),
                v.armset.funding_rules.len(),
                if expected.is_none() { " (file name carries no sha: not checked)" } else { "" }
            ),
            code: 0,
        },
        Err(e) => Out { text: format!("REJECTED: {e} (hot box would run exits only)\n"), code: 1 },
    })
}

/// `true` if `path` would land inside a git working tree outside a
/// gitignored `keys/` directory (key material never goes into git).
fn inside_git_tree(path: &Path) -> bool {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut in_keys = false;
    let mut dir = abs.parent();
    while let Some(d) = dir {
        if d.file_name().is_some_and(|n| n == "keys") {
            in_keys = true;
        }
        if d.join(".git").exists() {
            return !in_keys;
        }
        dir = d.parent();
    }
    false
}

/// `armset keygen --out FILE`: a fresh ed25519 key for signing paper/dev
/// ArmSets (Solana keypair JSON, mode 0600, never overwritten). The
/// production key lives on ohio1 only.
pub fn armset_keygen(args: &[String]) -> Result<Out, String> {
    use std::io::{Read, Write};
    let out = opt(args, "--out").ok_or("--out required")?;
    let path = Path::new(out);
    if inside_git_tree(path) {
        return Err(format!(
            "{out}: refusing to write key material inside a git working tree (use a path outside it, or keys/ which is gitignored)"
        ));
    }
    let mut seed = zeroize::Zeroizing::new([0u8; 32]);
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut seed[..]))
        .map_err(|e| format!("/dev/urandom: {e}"))?;
    let signer = rugsnipe_tx::signer::HotSigner::from_seed(&seed);
    let pk = signer.pubkey();
    let mut text = zeroize::Zeroizing::new(String::from("["));
    for (i, b) in seed.iter().chain(pk.as_ref().iter()).enumerate() {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&b.to_string());
    }
    text.push(']');
    let mut oo = std::fs::OpenOptions::new();
    oo.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        oo.mode(0o600);
    }
    let mut f = oo.open(path).map_err(|e| format!("{out}: {e}"))?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(Out {
        text: format!("wrote {out} (0600)\npubkey {pk}\n"),
        code: 0,
    })
}

/// `armset sign --key FILE --in JSON [--out-dir DIR]`: sign the exact JSON
/// bytes, write `armset-<ver>-<sha256>.json` and its detached `.sig`
/// (base58), then verify the result with the key's public half.
pub fn armset_sign(args: &[String]) -> Result<Out, String> {
    let key = opt(args, "--key").ok_or("--key required")?;
    let input = opt(args, "--in").ok_or("--in required")?;
    let dir = Path::new(opt(args, "--out-dir").unwrap_or("."));
    let signer = rugsnipe_tx::signer::HotSigner::load_file(Path::new(key))
        .map_err(|e| format!("{key}: {e}"))?;
    let bytes = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let set: armset::ArmSet =
        serde_json::from_slice(&bytes).map_err(|e| format!("{input}: not an ArmSet: {e}"))?;
    let sha = armset::sha256_hex(&bytes);
    let sig = signer.sign(&bytes);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("armset-{}-{sha}.json", set.ver));
    std::fs::write(&file, &bytes).map_err(|e| e.to_string())?;
    let sig_file = format!("{}.sig", file.display());
    std::fs::write(&sig_file, bs58::encode(sig).into_string()).map_err(|e| e.to_string())?;
    let pk = signer.pubkey();
    let check = armset::verify(
        &bytes,
        Some(bs58::encode(sig).into_string().as_bytes()),
        &VerifyCtx {
            pubkey: &pk,
            expected_sha: Some(&sha),
            now_unix: set.created_at,
            max_age_s: u64::MAX,
            regime: None,
            hot_version: "999.0.0",
        },
    );
    if let Err(e) = check {
        return Err(format!("signed ArmSet does not verify: {e}"));
    }
    Ok(Out {
        text: format!(
            "{}\n{sig_file}\nver {} sha256 {sha} signer {pk} operators {} watch {} funding_rules {} gate_passed={} probe_allowed={}\n",
            file.display(),
            set.ver,
            set.operators.len(),
            set.watch.len(),
            set.funding_rules.len(),
            set.gate_passed,
            set.probe_allowed
        ),
        code: 0,
    })
}
