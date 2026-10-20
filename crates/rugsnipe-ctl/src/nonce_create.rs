//! `rugsnipe-ctl nonce create`: create the buy nonce pool on chain, signed
//! with the probe keypair and sent through JSON-RPC, so the operator never
//! needs the Agave CLI (docs/research/readiness-2026-10-20.md M2).
//!
//! ```text
//! rugsnipe-ctl nonce create --keypair FILE --rpc-url URL|cred:NAME [--count 32] [--start 0]
//!                           [--dry-run] [--creds DIR] [--confirm-s 30]
//! ```
//!
//! The addresses are the hot binary's: `createWithSeed(probe, "rs-buy-<i>",
//! System)` ([`sysix::pool_addresses`], the pool `rugsnipe::app` fires
//! from). For each `i` in `start..start+count`: `getAccountInfo`; an
//! initialized nonce account of the probe key (System-owned, 80 bytes) is
//! `exists` and skipped; a missing one gets one transaction
//! ([`sysix::nonce_create_tx`]: `CreateAccountWithSeed` + `InitializeNonceAccount`,
//! the probe key as payer, base and authority) sent with `skipPreflight:
//! false` and confirmed by polling `getSignatureStatuses`. The first failure
//! stops the run (exit 1); re-running is idempotent (a created account is
//! `exists` next time, and a second create of the same address cannot
//! succeed on chain). Anything else at an address (foreign authority,
//! wrong owner or size, SOL sent to it) stops the run before anything is
//! sent: the hot binary quarantines such a slot.
//!
//! Arguments are checked first ([`check_args`]): anything but the options
//! above, a repeated option or a value option without a value exits 2
//! before the keypair is read, so a mistyped `--dry-run` never sends;
//! `--help` / `-h` prints [`USAGE`] and exits 0.
//!
//! The keypair is loaded by [`HotSigner::load_file`] (the hot binary's
//! loader), which refuses a group- or world-accessible file. `--rpc-url
//! cred:NAME` is read like the hot binary's `cred:` URLs: a `NAME = "value"`
//! line of the api-keys credential, else a credential file `NAME`, in
//! `--creds DIR` (default `$CREDENTIALS_DIRECTORY`, else
//! `/etc/rugsnipe/keys`, where the api-keys file is `api-keys.toml`). Only
//! `scheme://host` of the URL is ever printed.

use super::{flag, num, opt, Out};
use base64::Engine as _;
use pump_codec::constants::SYSTEM_PROGRAM_ID;
use pump_codec::Pubkey;
use rugsnipe_tx::rpc::RpcClient;
use rugsnipe_tx::senders::ApiKeys;
use rugsnipe_tx::signer::{HotSigner, KeyError};
use rugsnipe_tx::sysix;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// The pool the hot binary fires from (`sysix::pool_addresses(owner, "buy", n)`).
pub const POOL: &str = "buy";
/// Fee of a one-signature transaction without a priority fee.
pub const TX_FEE_LAMPORTS: u64 = 5_000;
/// Largest rent accepted from the RPC (twice the known 80-byte rent): more
/// means a broken or hostile endpoint, and 32 accounts would lock it all.
pub const MAX_RENT_LAMPORTS: u64 = 2 * sysix::NONCE_RENT_LAMPORTS;
/// Highest index accepted (`--start + --count`).
pub const MAX_INDEX: usize = 1_024;
const POLL: Duration = Duration::from_millis(400);

/// What an address holds on chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnChain {
    /// No account: create it.
    Missing,
    /// An initialized nonce account of the probe key holding `lamports`.
    Nonce { lamports: u64 },
    /// Not checked (`--dry-run` without `--rpc-url`).
    Unchecked,
    /// Something else: never create over it, escalate.
    Bad(String),
}

/// `getAccountInfo`'s `value` (base64 encoding) for a nonce of `authority`.
pub fn classify(value: &Value, authority: &Pubkey) -> OnChain {
    if value.is_null() {
        return OnChain::Missing;
    }
    let lamports = value["lamports"].as_u64().unwrap_or(0);
    let owner = value["owner"].as_str().unwrap_or("");
    if owner != SYSTEM_PROGRAM_ID.to_string() {
        return OnChain::Bad(format!(
            "owned by {owner}, not the System program: not a nonce account"
        ));
    }
    let Some(data) = value["data"][0]
        .as_str()
        .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
    else {
        return OnChain::Bad("account data is not base64".into());
    };
    if data.is_empty() {
        return OnChain::Bad(format!(
            "holds {lamports} lamports but no data (someone sent SOL to this address): createAccountWithSeed cannot create it"
        ));
    }
    if data.len() as u64 != sysix::NONCE_ACCOUNT_LEN {
        return OnChain::Bad(format!(
            "data is {} bytes, not {}",
            data.len(),
            sysix::NONCE_ACCOUNT_LEN
        ));
    }
    match rugsnipe_tx::nonce::parse_nonce_account(&data) {
        None => OnChain::Bad("80 bytes but not an initialized nonce account".into()),
        Some((auth, _, _)) if auth != *authority => OnChain::Bad(format!(
            "nonce authority is {auth}, not the probe key {authority}"
        )),
        Some(_) => OnChain::Nonce { lamports },
    }
}

/// `(index, address)` of `start..start+count` in the buy pool of `authority`.
pub fn pool_rows(authority: &Pubkey, start: usize, count: usize) -> Vec<(usize, Pubkey)> {
    let all = sysix::pool_addresses(authority, POOL, start + count);
    (start..start + count)
        .zip(all[start..].iter().copied())
        .collect()
}

// ------------------------------------------------------------------ URLs

/// `scheme://host[:port]` of a URL: user info, path and query (an API key
/// can sit in any of them) are dropped.
pub fn host_only(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "<url>".into();
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let auth = &rest[..end];
    let host = auth.rsplit_once('@').map(|(_, h)| h).unwrap_or(auth);
    format!("{scheme}://{host}")
}

/// `s` with the resolved `url` and every other `http(s)://` URL in it
/// reduced to [`host_only`] (client errors can echo the URL).
fn redact(s: &str, url: &str) -> String {
    let s = if url.is_empty() {
        s.to_string()
    } else {
        s.replace(url, &host_only(url))
    };
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    loop {
        let next = ["https://", "http://"]
            .iter()
            .filter_map(|p| rest.find(p))
            .min();
        let Some(i) = next else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '>' | ','))
            .unwrap_or(tail.len());
        out.push_str(&host_only(&tail[..end]));
        rest = &tail[end..];
    }
    out
}

/// A usable RPC URL: `https://`, or `http://` to this machine (tests).
fn check_url(url: &str) -> Result<(), String> {
    let host = host_only(url);
    let hostport = host.split_once("://").map_or("", |(_, h)| h);
    let name = match hostport.rsplit_once(':') {
        Some((h, port)) if port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => hostport,
    };
    if url.starts_with("https://") && !name.is_empty() {
        return Ok(());
    }
    if url.starts_with("http://") && matches!(name, "127.0.0.1" | "localhost" | "[::1]") {
        return Ok(());
    }
    Err(format!(
        "--rpc-url {host}: must be an https:// URL (or cred:NAME holding one)"
    ))
}

/// Mode bits of a credential file; `None` if missing or systemd's `ABSENT`
/// placeholder (as `rugsnipe::keys::mode_of`).
fn cred_mode(p: &Path) -> Option<u32> {
    let m = std::fs::metadata(p).ok()?;
    if m.len() <= 16 && std::fs::read(p).is_ok_and(|b| b.trim_ascii() == b"ABSENT") {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(m.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        Some(0o400)
    }
}

fn read_cred(p: &Path) -> Result<Option<Zeroizing<String>>, String> {
    match cred_mode(p) {
        None => Ok(None),
        Some(m) if m & 0o077 != 0 => Err(format!(
            "credential {} is mode {m:o} (group/world-accessible): refused. Fix: sudo chmod 400 {}",
            p.display(),
            p.display()
        )),
        Some(_) => {
            let raw =
                Zeroizing::new(std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?);
            String::from_utf8(raw.to_vec())
                .map(|t| Some(Zeroizing::new(t)))
                .map_err(|_| format!("credential {} is not UTF-8", p.display()))
        }
    }
}

/// The value of credential `name` in `dir`, looked up like the hot
/// binary's `named_secret`: a `NAME = "value"` line of the api-keys
/// credential (`api-keys` under systemd, `api-keys.toml` in
/// /etc/rugsnipe/keys), else a one-line credential file `NAME`.
pub fn named_secret(dir: &Path, name: &str) -> Result<Option<Zeroizing<String>>, String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(format!(
            "cred:{name}: a credential name is letters, digits, '_' and '-' (cred:RPC_URL)"
        ));
    }
    for file in ["api-keys", "api-keys.toml"] {
        let p = dir.join(file);
        if let Some(text) = read_cred(&p)? {
            // the parser's message names a line number, never a value
            let k = ApiKeys::parse(&text).map_err(|why| format!("{}: {why}", p.display()))?;
            if let Some(v) = k.get(name).filter(|v| !v.is_empty()) {
                return Ok(Some(Zeroizing::new(v.to_string())));
            }
        }
    }
    Ok(read_cred(&dir.join(name))?.and_then(|t| {
        let v = t.trim();
        (!v.is_empty()).then(|| Zeroizing::new(v.to_string()))
    }))
}

/// `--rpc-url` resolved: a plain URL as given (refused with a query string:
/// a key on the command line shows in `ps` and shell history), or the
/// value of `cred:NAME`.
pub fn resolve_rpc_url(arg: &str, creds: &Path) -> Result<Zeroizing<String>, String> {
    let url = match arg.strip_prefix("cred:") {
        Some(name) => named_secret(creds, name.trim())?.ok_or_else(|| {
            format!(
                "--rpc-url {arg}: {:?} is neither a line of {}/api-keys.toml nor a file {}/{}",
                name.trim(),
                creds.display(),
                creds.display(),
                name.trim()
            )
        })?,
        None => {
            let l = arg.to_ascii_lowercase();
            if arg.contains('?')
                || l.contains("api-key=")
                || l.contains("api_key=")
                || l.contains("token=")
            {
                return Err(format!(
                    "--rpc-url {}: carries a query string (an API key?) on the command line: put it in /etc/rugsnipe/keys/api-keys.toml as RPC_URL = \"...\" and pass --rpc-url cred:RPC_URL",
                    host_only(arg)
                ));
            }
            Zeroizing::new(arg.to_string())
        }
    };
    check_url(url.trim())?;
    Ok(Zeroizing::new(url.trim().to_string()))
}

fn creds_dir(args: &[String]) -> PathBuf {
    opt(args, "--creds")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/etc/rugsnipe/keys"))
}

// ------------------------------------------------------------------ RPC

struct Rpc {
    client: RpcClient,
    url: Zeroizing<String>,
}

impl Rpc {
    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.client
            .call(method, params)
            .map_err(|e| redact(&e, &self.url).chars().take(400).collect())
    }

    /// Reads are retried (3 tries, 0.5 s apart): a 429 or a dropped
    /// connection should not stop a run.
    fn read(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut last = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(500));
            }
            match self.call(method, params.clone()) {
                Ok(v) => return Ok(v),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn account(&self, key: &Pubkey, authority: &Pubkey) -> Result<OnChain, String> {
        let r = self.read(
            "getAccountInfo",
            json!([key.to_string(), {"encoding": "base64", "commitment": "confirmed"}]),
        )?;
        Ok(classify(&r["value"], authority))
    }

    fn blockhash(&self) -> Result<[u8; 32], String> {
        let r = self.read("getLatestBlockhash", json!([{"commitment": "confirmed"}]))?;
        r["value"]["blockhash"]
            .as_str()
            .and_then(|b| bs58::decode(b).into_vec().ok())
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| format!("getLatestBlockhash: no blockhash in {r}"))
    }

    /// `Ok(None)`: not confirmed yet; `Ok(Some(err))`: landed (`err` = the
    /// transaction error, `None` for success).
    fn status(&self, sig: &str) -> Result<Option<Option<Value>>, String> {
        let r = self.read(
            "getSignatureStatuses",
            json!([[sig], {"searchTransactionHistory": false}]),
        )?;
        let e = &r["value"][0];
        if e.is_null() {
            return Ok(None);
        }
        let st = e["confirmationStatus"].as_str().unwrap_or("");
        if st != "confirmed" && st != "finalized" {
            return Ok(None);
        }
        Ok(Some(e.get("err").filter(|x| !x.is_null()).cloned()))
    }
}

// ------------------------------------------------------------------ the command

fn row(idx: usize, addr: &Pubkey, result: &str, sig: &str, lamports: &str) -> String {
    format!(
        "{idx:>4}  {:<44}  {result:<12}  {sig:<88}  {lamports}\n",
        addr.to_string()
    )
}

/// Write a progress line now (the operator watches a run of up to a minute).
fn say(live: &mut dyn Write, s: &str) {
    let _ = live.write_all(s.as_bytes());
    let _ = live.flush();
}

fn key_error(path: &str, e: &KeyError) -> String {
    match e {
        KeyError::Permissions(_) => format!(
            "{path}: {e}: refused (other users could read the wallet). Fix: sudo chown root:root {path} && sudo chmod 400 {path}"
        ),
        _ => format!("{path}: {e}"),
    }
}

/// The balance line: what the wallet holds against what `missing` creates need.
fn funds_line(balance: Option<u64>, missing: u64, rent: u64) -> String {
    let needed = missing * (rent + TX_FEE_LAMPORTS);
    let Some(b) = balance else {
        return format!(
            "not checked against the chain (no --rpc-url); each account locks {rent} lamports of rent plus a {TX_FEE_LAMPORTS} lamport fee"
        );
    };
    let short = if b < needed {
        format!(
            ": NOT ENOUGH, send at least {} more lamports first",
            needed - b
        )
    } else {
        String::new()
    };
    format!(
        "wallet balance {b} lamports ({:.6} SOL); {missing} to create need {needed} lamports ({missing} x (rent {rent} + fee {TX_FEE_LAMPORTS})){short}",
        b as f64 / 1e9
    )
}

/// Why the run stopped at index `i` (exit 1).
fn stopped(i: usize, addr: &Pubkey, why: &str, sig: &str, created: u64, existed: u64) -> Out {
    let sig = if sig.is_empty() {
        String::new()
    } else {
        format!(" (signature {sig})")
    };
    Out {
        text: format!(
            "STOPPED at #{i} {addr} ({}): {why}\ncreated {created}, existed {existed}; nothing after #{i} was sent. Fix the cause and re-run the same command: accounts that exist are skipped{sig}\n",
            sysix::nonce_seed(POOL, i)
        ),
        code: 1,
    }
}

/// `rugsnipe-ctl nonce create --help`.
pub const USAGE: &str = "usage: rugsnipe-ctl nonce create --keypair FILE --rpc-url URL|cred:NAME [--count 32] [--start 0] [--dry-run] [--creds DIR] [--confirm-s 30]
  Creates the buy nonce accounts rs-buy-<start> .. rs-buy-<start+count-1> of the --keypair wallet (the
  addresses the hot binary uses), signed with --keypair and sent through --rpc-url. Accounts that already
  exist are skipped; the first failure stops the run (exit 1); re-running the same command is safe.
  options (each one at most once; anything else is refused and nothing is sent):
    --keypair FILE   the probe wallet, mode 400 or 600: /etc/rugsnipe/keys/probe-keypair.json
    --rpc-url URL    cred:RPC_URL (the RPC_URL line of /etc/rugsnipe/keys/api-keys.toml), or a plain https:// URL without a key
    --count N        how many accounts (default 32)
    --start I        first index (default 0)
    --dry-run        check only: nothing is signed or sent (without --rpc-url it prints the addresses only)
    --creds DIR      where cred:NAME is looked up (default $CREDENTIALS_DIRECTORY, else /etc/rugsnipe/keys)
    --confirm-s S    seconds to wait for each confirmation (default 30)
    --help, -h       this text
  check first, then run the same command without --dry-run:
    sudo /opt/rugsnipe/bin/rugsnipe-ctl nonce create --keypair /etc/rugsnipe/keys/probe-keypair.json --rpc-url cred:RPC_URL --count 32 --dry-run
    sudo /opt/rugsnipe/bin/rugsnipe-ctl nonce create --keypair /etc/rugsnipe/keys/probe-keypair.json --rpc-url cred:RPC_URL --count 32
";

/// Options that take a value (`--name VALUE`).
const VALUE_OPTS: [&str; 6] = [
    "--keypair",
    "--rpc-url",
    "--count",
    "--start",
    "--confirm-s",
    "--creds",
];

/// Refuse every argument but the known options, each at most once and each
/// value option with a value: this command signs and sends, so a mistyped
/// `--dry-run` (`--dryrun`, `--dry_run`, `--dry-run=1`, `-n`) must stop the
/// run, not be ignored. Checked before the keypair is read.
pub fn check_args(args: &[String]) -> Result<(), String> {
    let refuse = |why: String| {
        Err(format!(
            "nonce create: {why}\nnothing was read, signed or sent. The options are:\n{USAGE}"
        ))
    };
    let mut seen: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let known = VALUE_OPTS.contains(&a) || a == "--dry-run";
        if !known {
            let squashed: String = a
                .to_ascii_lowercase()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect();
            let hint = if a.split_once('=').is_some_and(|(k, _)| k == "--dry-run") {
                ": write --dry-run alone, with no value".to_string()
            } else if let Some(k) = a
                .split_once('=')
                .map(|(k, _)| k)
                .filter(|k| VALUE_OPTS.contains(k))
            {
                format!(": write {k} VALUE with a space, not {k}=VALUE")
            } else if squashed.contains("dry") || a == "-n" {
                ": did you mean --dry-run ?".to_string()
            } else if a.starts_with('-') {
                String::new()
            } else {
                " (a value without an option before it)".to_string()
            };
            return refuse(format!("unknown argument {a:?}{hint}"));
        }
        if seen.contains(&a) {
            return refuse(format!("{a} given twice"));
        }
        seen.push(a);
        if VALUE_OPTS.contains(&a) {
            match args.get(i + 1) {
                Some(v) if !v.starts_with("--") && !v.is_empty() => i += 2,
                _ => return refuse(format!("{a} needs a value")),
            }
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// `nonce create`: rows go to `live` as they happen; the returned [`Out`]
/// carries the summary (exit 0) or why the run stopped (exit 1).
pub fn nonce_create(args: &[String], live: &mut dyn Write) -> Result<Out, String> {
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "--help" | "-h" | "help"))
    {
        return Ok(Out {
            text: USAGE.to_string(),
            code: 0,
        });
    }
    check_args(args)?;
    let keypair = opt(args, "--keypair").ok_or_else(|| {
        format!(
            "--keypair FILE required (the probe wallet: --keypair /etc/rugsnipe/keys/probe-keypair.json)\n{USAGE}"
        )
    })?;
    let count: usize = num(args, "--count", 32)?;
    let start: usize = num(args, "--start", 0)?;
    let confirm_s: u64 = num(args, "--confirm-s", 30)?;
    let dry = flag(args, "--dry-run");
    if count == 0 || start.saturating_add(count) > MAX_INDEX {
        return Err(format!(
            "--start {start} --count {count}: need 1 <= count and start + count <= {MAX_INDEX}"
        ));
    }
    let rpc_arg = opt(args, "--rpc-url");
    if rpc_arg.is_none() && !dry {
        return Err(
            "--rpc-url required: cred:RPC_URL (a line of /etc/rugsnipe/keys/api-keys.toml) or a plain https:// URL".into(),
        );
    }
    // every local check before the first network call
    let rpc = match rpc_arg {
        Some(a) => {
            let url = resolve_rpc_url(a, &creds_dir(args))?;
            Some((
                Rpc {
                    client: RpcClient::new(&url),
                    url: url.clone(),
                },
                match a.strip_prefix("cred:") {
                    Some(_) => format!("{} ({a})", host_only(&url)),
                    None => host_only(&url),
                },
            ))
        }
        None => None,
    };
    let signer = HotSigner::load_file(Path::new(keypair)).map_err(|e| key_error(keypair, &e))?;
    let authority = signer.pubkey();
    let rows = pool_rows(&authority, start, count);
    say(
        live,
        &format!(
            "{}nonce create: authority {authority} (keypair {keypair}), accounts {} .. {}, rpc {}\n",
            if dry { "DRY RUN (nothing is signed or sent): " } else { "" },
            sysix::nonce_seed(POOL, start),
            sysix::nonce_seed(POOL, start + count - 1),
            rpc.as_ref().map_or("none (addresses only)", |(_, l)| l.as_str()),
        ),
    );
    // --- rent: the larger of the known value and the RPC's
    let rent = match &rpc {
        Some((r, _)) => {
            let live_rent = r
                .read(
                    "getMinimumBalanceForRentExemption",
                    json!([sysix::NONCE_ACCOUNT_LEN]),
                )
                .map_err(|e| format!("getMinimumBalanceForRentExemption: {e}"))?
                .as_u64()
                .ok_or("getMinimumBalanceForRentExemption: not a number")?;
            if live_rent > MAX_RENT_LAMPORTS {
                return Ok(Out {
                    text: format!(
                        "STOPPED before sending: the RPC says rent for 80 bytes is {live_rent} lamports, more than twice the known {}: check the RPC\n",
                        sysix::NONCE_RENT_LAMPORTS
                    ),
                    code: 1,
                });
            }
            say(
                live,
                &format!(
                    "rent: {} lamports per account (known {}, RPC getMinimumBalanceForRentExemption(80) = {live_rent}; the larger is used)\n",
                    live_rent.max(sysix::NONCE_RENT_LAMPORTS),
                    sysix::NONCE_RENT_LAMPORTS
                ),
            );
            live_rent.max(sysix::NONCE_RENT_LAMPORTS)
        }
        None => sysix::NONCE_RENT_LAMPORTS,
    };
    // --- scan every address first: a bad account or too little SOL stops
    // the run before anything is sent
    let mut found = Vec::with_capacity(rows.len());
    for (i, addr) in &rows {
        let st = match &rpc {
            Some((r, _)) => match r.account(addr, &authority) {
                Ok(st) => st,
                Err(e) => {
                    return Ok(Out {
                        text: format!(
                            "STOPPED before sending: getAccountInfo #{i} {addr}: {e}\nre-run the same command once the RPC answers\n"
                        ),
                        code: 1,
                    })
                }
            },
            None => OnChain::Unchecked,
        };
        found.push((*i, *addr, st));
    }
    let missing = found
        .iter()
        .filter(|(_, _, s)| *s == OnChain::Missing)
        .count() as u64;
    let bad: Vec<_> = found
        .iter()
        .filter_map(|(i, a, s)| match s {
            OnChain::Bad(why) => Some((*i, *a, why.clone())),
            _ => None,
        })
        .collect();
    let needed = missing * (rent + TX_FEE_LAMPORTS);
    let balance = match &rpc {
        Some((r, _)) => Some(
            r.read(
                "getBalance",
                json!([authority.to_string(), {"commitment": "confirmed"}]),
            )
            .map_err(|e| format!("getBalance: {e}"))?["value"]
                .as_u64()
                .ok_or("getBalance: not a number")?,
        ),
        None => None,
    };
    let short = balance.is_some_and(|b| b < needed);
    let header = format!(
        "{:>4}  {:<44}  {:<12}  {:<88}  lamports\n",
        "idx", "address", "result", "signature"
    );
    let funds = funds_line(balance, missing, rent);
    if dry || !bad.is_empty() || short {
        let mut t = header.clone();
        for (i, a, s) in &found {
            t.push_str(&match s {
                OnChain::Missing => row(*i, a, "would create", "-", &rent.to_string()),
                OnChain::Nonce { lamports } => row(*i, a, "exists", "-", &lamports.to_string()),
                OnChain::Unchecked => row(*i, a, "not checked", "-", "-"),
                OnChain::Bad(_) => row(*i, a, "BAD", "-", "-"),
            });
        }
        say(live, &t);
        let mut msg = String::new();
        for (i, a, why) in &bad {
            msg.push_str(&format!(
                "BAD #{i} {a} ({}): {why}\n",
                sysix::nonce_seed(POOL, *i)
            ));
        }
        msg.push_str(&funds);
        msg.push('\n');
        if dry {
            let ok = bad.is_empty() && !short;
            msg.push_str(&format!(
                "DRY RUN: nothing was signed or sent. {}\n",
                if ok {
                    "Run the same command without --dry-run to create them."
                } else {
                    "A real run would stop: fix the lines above first."
                }
            ));
            return Ok(Out {
                text: msg,
                code: i32::from(!ok),
            });
        }
        msg.push_str(
            "STOPPED before sending anything. A BAD address is quarantined by the hot binary: escalate to an engineer. Too little SOL: fund the wallet and re-run.\n",
        );
        return Ok(Out { text: msg, code: 1 });
    }
    say(live, &format!("{funds}\n{header}"));
    // --- create, in index order; the first failure stops the run
    let (r, _) = rpc.as_ref().expect("rpc is set unless --dry-run");
    let (mut created, mut existed) = (0u64, 0u64);
    for (i, addr, st) in &found {
        if let OnChain::Nonce { lamports } = st {
            existed += 1;
            say(live, &row(*i, addr, "exists", "-", &lamports.to_string()));
            continue;
        }
        let stop = |why: String, sig: &str| stopped(*i, addr, &why, sig, created, existed);
        let hash = match r.blockhash() {
            Ok(h) => h,
            Err(e) => {
                say(live, &row(*i, addr, "failed", "-", "-"));
                return Ok(stop(e, ""));
            }
        };
        let (tx, sig, a) = sysix::nonce_create_tx(&signer, POOL, *i, rent, &hash)
            .map_err(|e| format!("#{i}: cannot build the transaction: {e}"))?;
        debug_assert_eq!(a, *addr);
        let sig58 = bs58::encode(sig).into_string();
        let sent = r.call(
            "sendTransaction",
            json!([
                base64::engine::general_purpose::STANDARD.encode(&tx),
                {"encoding": "base64", "skipPreflight": false, "preflightCommitment": "confirmed"}
            ]),
        );
        if let Err(e) = sent {
            say(live, &row(*i, addr, "failed", "-", "-"));
            return Ok(stop(
                format!("sendTransaction: {e} (a refused preflight sends nothing; after a timeout it may still land: wait a minute, then re-run)"),
                &sig58,
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(confirm_s);
        let outcome = loop {
            match r.status(&sig58) {
                Ok(Some(None)) => break Ok(()),
                Ok(Some(Some(err))) => break Err(format!("landed but failed: {err}")),
                Ok(None) | Err(_) if Instant::now() < deadline => std::thread::sleep(POLL),
                Ok(None) => {
                    // the status may lag the account: look once more
                    break match r.account(addr, &authority) {
                        Ok(OnChain::Nonce { .. }) => Ok(()),
                        _ => Err(format!(
                            "not confirmed within {confirm_s} s (it may still land: wait a minute, then re-run; a second create of the same address cannot succeed)"
                        )),
                    };
                }
                Err(e) => break Err(format!("getSignatureStatuses: {e}")),
            }
        };
        match outcome {
            Ok(()) => {
                created += 1;
                say(live, &row(*i, addr, "created", &sig58, &rent.to_string()));
            }
            Err(why) => {
                say(live, &row(*i, addr, "failed", &sig58, "-"));
                return Ok(stop(why, &sig58));
            }
        }
    }
    let locked = created as u128 * rent as u128;
    Ok(Out {
        text: format!(
            "done: created {created}, existed {existed}, failed 0 ({} .. {}); rent locked by this run {locked} lamports ({:.6} SOL), fees {} lamports\nnext: the --probe-dry of docs/08 section 9 step 5 must print `dry nonce burn ... simulateTransaction err=null`\n",
            sysix::nonce_seed(POOL, start),
            sysix::nonce_seed(POOL, start + count - 1),
            locked as f64 / 1e9,
            created * TX_FEE_LAMPORTS
        ),
        code: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_addresses_are_the_hot_binarys() {
        let auth: Pubkey = "DXatZCLiwrHKsREZ4z3ej8rCdFCuwRTUD24VWtBPFvVX"
            .parse()
            .unwrap();
        let rows = pool_rows(&auth, 0, 3);
        // the pool the hot binary seeds (rugsnipe::app: pool_addresses(owner, "buy", n))
        let hot = sysix::pool_addresses(&auth, "buy", 3);
        assert_eq!(rows.iter().map(|r| r.1).collect::<Vec<_>>(), hot);
        for (i, a) in &rows {
            assert_eq!(
                *a,
                sysix::create_with_seed(&auth, &format!("rs-buy-{i}"), &SYSTEM_PROGRAM_ID)
            );
        }
        // pinned: sha256(base || "rs-buy-i" || System) computed outside Rust
        let want = [
            "H2wRXvy1K3MsSdwhgGCdLh28AxiE1hF6BAEgwJnGz6vH",
            "E53eQ2ipwxH6EB9S3mKjhTzScYzDsFUyLhrVdwLqKoQ1",
            "51xC8GkVSdCyaGbuGRp5bjt58GCAfed8LAGtGDKCBZUw",
        ];
        for ((i, a), w) in rows.iter().zip(want) {
            assert_eq!(a.to_string(), w, "#{i}");
        }
        // --start keeps the index
        assert_eq!(pool_rows(&auth, 2, 1), vec![(2, hot[2])]);
    }

    fn acct(owner: &Pubkey, data: &[u8], lamports: u64) -> Value {
        json!({
            "lamports": lamports,
            "owner": owner.to_string(),
            "data": [base64::engine::general_purpose::STANDARD.encode(data), "base64"],
        })
    }

    #[test]
    fn classify_accounts() {
        let auth = Pubkey::new_from_array([4; 32]);
        let other = Pubkey::new_from_array([5; 32]);
        let mut nonce = vec![];
        nonce.extend_from_slice(&1u32.to_le_bytes());
        nonce.extend_from_slice(&1u32.to_le_bytes());
        nonce.extend_from_slice(auth.as_ref());
        nonce.extend_from_slice(&[9; 32]);
        nonce.extend_from_slice(&5_000u64.to_le_bytes());
        assert_eq!(classify(&Value::Null, &auth), OnChain::Missing);
        assert_eq!(
            classify(&acct(&SYSTEM_PROGRAM_ID, &nonce, 1_056_640), &auth),
            OnChain::Nonce {
                lamports: 1_056_640
            }
        );
        let bad = |v: Value, a: &Pubkey, want: &str| match classify(&v, a) {
            OnChain::Bad(why) => assert!(why.contains(want), "{why}"),
            o => panic!("{o:?}"),
        };
        bad(
            acct(&SYSTEM_PROGRAM_ID, &nonce, 1),
            &other,
            "nonce authority is",
        );
        bad(acct(&other, &nonce, 1), &auth, "not the System program");
        bad(acct(&SYSTEM_PROGRAM_ID, &[], 5), &auth, "someone sent SOL");
        bad(acct(&SYSTEM_PROGRAM_ID, &nonce[..40], 5), &auth, "40 bytes");
        let mut uninit = nonce.clone();
        uninit[4] = 0;
        bad(
            acct(&SYSTEM_PROGRAM_ID, &uninit, 5),
            &auth,
            "not an initialized",
        );
    }

    #[test]
    fn the_documented_commands_pass_the_argument_check() {
        let v = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        for l in USAGE.lines().filter(|l| l.contains("sudo ")) {
            let a = v(l.split_once(" nonce create ").unwrap().1);
            assert_eq!(check_args(&a), Ok(()), "{l}");
        }
        let all = v("--keypair k --rpc-url cred:RPC_URL --count 32 --start 0 --dry-run --creds /d --confirm-s 30");
        assert_eq!(check_args(&all), Ok(()));
        assert_eq!(check_args(&[]), Ok(()));
        assert!(
            check_args(&v("--start -1")).is_ok(),
            "the number check says why"
        );
        for bad in [
            "--dryrun",
            "--dry-run=yes",
            "-n",
            "x",
            "--keypair",
            "--count 1 --count 2",
        ] {
            assert!(check_args(&v(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn urls_print_the_host_only() {
        assert_eq!(
            host_only("https://mainnet.helius-rpc.com/?api-key=abc"),
            "https://mainnet.helius-rpc.com"
        );
        assert_eq!(
            host_only("https://u:p@x.quiknode.pro/TOKEN/"),
            "https://x.quiknode.pro"
        );
        assert_eq!(host_only("http://127.0.0.1:8899/"), "http://127.0.0.1:8899");
        let url = "https://x.example/path/TOKEN?api-key=abc";
        let e = redact(
            &format!("io error talking to {url}: refused (see {url})"),
            url,
        );
        assert!(!e.contains("TOKEN") && !e.contains("abc"), "{e}");
        assert!(
            e.contains("https://x.example:") || e.contains("https://x.example "),
            "{e}"
        );
        assert_eq!(
            redact("GET http://h.example/q?k=1 failed", ""),
            "GET http://h.example failed"
        );
        assert!(check_url("https://api.mainnet-beta.solana.com").is_ok());
        assert!(check_url("http://127.0.0.1:9/").is_ok());
        assert!(check_url("http://rpc.example.com/").is_err());
        assert!(check_url("ftp://x").is_err());
        assert!(
            resolve_rpc_url("https://h.example/?api-key=abc", Path::new("/nonexistent"))
                .unwrap_err()
                .contains("cred:RPC_URL")
        );
        assert!(resolve_rpc_url("cred:../etc", Path::new("/nonexistent")).is_err());
        assert!(resolve_rpc_url("cred:RPC_URL", Path::new("/nonexistent"))
            .unwrap_err()
            .contains("neither a line"));
    }
}
