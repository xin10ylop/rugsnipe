//! `rugsnipe-ctl` logic as a library (the binary is a thin wrapper, so every
//! subcommand is testable without spawning processes).
//!
//! ```text
//! rugsnipe-ctl config validate <file> [--creds DIR] [--clock-offset-ms X] [--post-exec-healthy]
//!                                     [--armset FILE [--sig FILE]] [--regime R] [--now UNIX] [--live]
//! rugsnipe-ctl template dump --mint M --wallet W [--format legacy|v1] [--nonce ACCT [--nonce-authority A]]
//!                            [--tip-account T] [--cu N] [--lads N] [--spend L] [--min-out N] [--prio L] [--tip L]
//! rugsnipe-ctl nonce init  --authority A [--count N] [--pool buy|sell] [--blockhash B58] [--lamports L]
//! rugsnipe-ctl nonce close --authority A --to T [--count N] [--pool buy|sell] [--blockhash B58] [--lamports L]
//! rugsnipe-ctl armset verify <file> --pubkey PK [--sig FILE] [--regime R] [--max-age S] [--now UNIX]
//! rugsnipe-ctl armset keygen --out FILE           (dev/paper signing key; refused inside a git tree except keys/)
//! rugsnipe-ctl armset sign --key FILE --in JSON [--out-dir DIR]   (writes armset-<ver>-<sha>.json + .sig)
//! ```
//!
//! Nothing here signs or sends: nonce init/close print **unsigned**
//! transactions (dry run) for review and offline signing.

use base64::Engine as _;
use pump_codec::constants as c;
use pump_codec::instructions::SolV3Keys;
use pump_codec::tx::TxView;
use pump_codec::Pubkey;
use rugsnipe_core::armset::{self, VerifyCtx};
use rugsnipe_core::config::{ArmSetGate, Config, TxFormat, ValidateEnv};
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
        ["nonce", "init", ..] => nonce_txs(&args[2..], true),
        ["nonce", "close", ..] => nonce_txs(&args[2..], false),
        ["armset", "verify", file, ..] => armset_verify(file, &args[3..]),
        ["armset", "keygen", ..] => armset_keygen(&args[2..]),
        ["armset", "sign", ..] => armset_sign(&args[2..]),
        _ => Err(USAGE.to_string()),
    };
    r.unwrap_or_else(err)
}

pub const USAGE: &str = "usage: rugsnipe-ctl <config validate FILE | template dump | nonce init | nonce close | armset verify FILE | armset keygen | armset sign> [options]\n(see `rugsnipe-ctl` crate docs for options)";

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

fn armset_gate(cfg: &Config, args: &[String]) -> Result<(ArmSetGate, String), String> {
    let Some(file) = opt(args, "--armset") else {
        return Ok((ArmSetGate::Missing, "no --armset given".into()));
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
                "ArmSet ver {} sha {} gate_passed={}",
                v.armset.ver, v.sha256, v.armset.gate_passed
            ),
        ),
        Err(e) => (e.gate(), format!("ArmSet rejected: {e}")),
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
    let (gate, note) = armset_gate(&cfg, args)?;
    env.armset = gate;
    let v = cfg.validate(&env);
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
    let p = CurveParams {
        format,
        user: wallet,
        nonce,
        cu_limit: num(args, "--cu", if nonce.is_some() { 72_150 } else { 72_000 })?,
        lads_bytes: num(args, "--lads", 12_124_160)?,
        tip_account: Some(pk_opt(args, "--tip-account")?.unwrap_or(c::JITO_TIP_ACCOUNTS[0])),
        buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[0],
        base_token_program: c::TOKEN_2022_PROGRAM_ID,
        partial_fill: Some(false),
    };
    let mut t = Template::buy(&p).map_err(|e| e.to_string())?;
    let keys = MintKeys::from_v3(&SolV3Keys::derive(
        mint,
        wallet,
        p.base_token_program,
        p.buyback_fee_recipient,
    ));
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
    });
    let mut s = String::new();
    let _ = writeln!(
        s,
        "buy template ({format:?}{}), unsigned",
        if nonce.is_some() { " + nonce" } else { "" }
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
                "OK: ArmSet ver {} sha256 {} regime {} gate_passed={} operators={} watch={} funding_rules={}{}\n",
                v.armset.ver,
                v.sha256,
                v.armset.regime,
                v.armset.gate_passed,
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
            "{}\n{sig_file}\nver {} sha256 {sha} signer {pk} operators {} watch {} funding_rules {} gate_passed={}\n",
            file.display(),
            set.ver,
            set.operators.len(),
            set.watch.len(),
            set.funding_rules.len(),
            set.gate_passed
        ),
        code: 0,
    })
}
