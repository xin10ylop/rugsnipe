//! `rugsnipe-ctl nonce create` against a mock JSON-RPC chain on 127.0.0.1
//! (the `serve_rpc` pattern of crates/rugsnipe/tests/common): existing
//! accounts are skipped, a missing one gets exactly one signed
//! create + initialize transaction, `--dry-run` sends nothing, a
//! group-readable keypair or a mistyped `--dry-run` is refused before any
//! network call, `--help` sends nothing, the first failure stops the run,
//! and `cred:NAME` URLs never print their key.

use base64::Engine as _;
use pump_codec::constants::{RECENT_BLOCKHASHES_SYSVAR_ID, RENT_SYSVAR_ID, SYSTEM_PROGRAM_ID};
use pump_codec::tx::TxView;
use pump_codec::Pubkey;
use rugsnipe_ctl::run;
use rugsnipe_tx::signer::HotSigner;
use rugsnipe_tx::sysix;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn tmpdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("rugsnipe-ctl-nonce-")
        .tempdir()
        .unwrap()
}

/// A solana-keygen style keypair file for `seed`, mode `mode`; returns the
/// path and the public key.
fn keypair_file(dir: &std::path::Path, seed: u8, mode: u32) -> (String, Pubkey) {
    let signer = HotSigner::from_seed(&Zeroizing::new([seed; 32]));
    let mut bytes = vec![seed; 32];
    bytes.extend_from_slice(signer.pubkey().as_ref());
    let text = format!(
        "[{}]",
        bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let p = dir.join("probe-keypair.json");
    std::fs::write(&p, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = mode;
    (p.display().to_string(), signer.pubkey())
}

fn nonce_data(authority: &Pubkey) -> Vec<u8> {
    let mut d = vec![];
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(authority.as_ref());
    d.extend_from_slice(&[7; 32]);
    d.extend_from_slice(&5_000u64.to_le_bytes());
    d
}

fn account_json(owner: &Pubkey, data: &[u8], lamports: u64) -> Value {
    json!({
        "lamports": lamports,
        "owner": owner.to_string(),
        "executable": false,
        "rentEpoch": 0,
        "space": data.len(),
        "data": [base64::engine::general_purpose::STANDARD.encode(data), "base64"],
    })
}

/// The mock chain: accounts by address, the raw transactions sent, and
/// knobs for failures.
#[derive(Default)]
struct Chain {
    accounts: HashMap<String, Value>,
    sent: Vec<Vec<u8>>,
    params: Vec<Value>,
    rent: u64,
    balance: u64,
    /// `sendTransaction` number `n` (0-based) is refused like a failed preflight.
    refuse_send: Option<usize>,
}

/// A JSON-RPC server on 127.0.0.1 (HTTP/1.1 keep-alive, one thread per
/// connection) over `chain`; returns the URL and the method log.
fn serve(chain: Arc<Mutex<Chain>>) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    let log = Arc::new(Mutex::new(vec![]));
    let log2 = log.clone();
    std::thread::spawn(move || {
        for st in l.incoming() {
            let Ok(st) = st else { continue };
            let (chain, log) = (chain.clone(), log2.clone());
            std::thread::spawn(move || {
                let mut w = st.try_clone().unwrap();
                let mut r = BufReader::new(st);
                loop {
                    let mut len = 0usize;
                    let mut first = true;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        if first && line.trim().is_empty() {
                            continue;
                        }
                        first = false;
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; len];
                    if r.read_exact(&mut body).is_err() {
                        return;
                    }
                    let req: Value = serde_json::from_slice(&body).unwrap_or_default();
                    let method = req["method"].as_str().unwrap_or("").to_string();
                    log.lock().unwrap().push(method.clone());
                    let resp = match handle(&chain, &method, &req["params"]) {
                        Ok(v) => json!({"jsonrpc": "2.0", "id": req["id"], "result": v}),
                        Err(e) => json!({"jsonrpc": "2.0", "id": req["id"],
                            "error": {"code": -32002, "message": e}}),
                    }
                    .to_string();
                    let out = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{resp}",
                        resp.len()
                    );
                    if w.write_all(out.as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (url, log)
}

fn handle(chain: &Mutex<Chain>, method: &str, p: &Value) -> Result<Value, String> {
    let mut c = chain.lock().unwrap();
    let ctx = json!({"slot": 400_000_000u64});
    match method {
        "getMinimumBalanceForRentExemption" => {
            assert_eq!(p[0], 80, "rent asked for 80 bytes");
            Ok(json!(c.rent))
        }
        "getBalance" => Ok(json!({"context": ctx, "value": c.balance})),
        "getAccountInfo" => {
            let k = p[0].as_str().unwrap();
            Ok(json!({"context": ctx, "value": c.accounts.get(k).cloned().unwrap_or(Value::Null)}))
        }
        "getLatestBlockhash" => Ok(json!({"context": ctx, "value": {
            "blockhash": bs58::encode([0x42u8; 32]).into_string(),
            "lastValidBlockHeight": 1_000}})),
        "sendTransaction" => {
            let n = c.sent.len();
            let raw = base64::engine::general_purpose::STANDARD
                .decode(p[0].as_str().unwrap())
                .unwrap();
            c.sent.push(raw.clone());
            c.params.push(p.clone());
            if c.refuse_send == Some(n) {
                return Err("Transaction simulation failed: Attempt to debit an account but found no record of a prior credit.".into());
            }
            // the transaction lands: the nonce account appears
            let tx = TxView::parse(&raw).unwrap();
            let keys = tx.static_account_keys();
            let (auth, addr) = (keys[0], keys[1]);
            let lamports = {
                let ix = tx.instructions().next().unwrap();
                let seed_len = u64::from_le_bytes(ix.data[36..44].try_into().unwrap()) as usize;
                u64::from_le_bytes(ix.data[44 + seed_len..52 + seed_len].try_into().unwrap())
            };
            c.accounts.insert(
                addr.to_string(),
                account_json(&SYSTEM_PROGRAM_ID, &nonce_data(&auth), lamports),
            );
            c.balance -= lamports + 5_000;
            Ok(json!(bs58::encode(&raw[1..65]).into_string()))
        }
        "getSignatureStatuses" => {
            let n = p[0].as_array().unwrap().len();
            Ok(json!({"context": ctx, "value": (0..n).map(|_| json!({
                "slot": 400_000_001u64, "confirmations": null, "err": null,
                "confirmationStatus": "confirmed"})).collect::<Vec<_>>()}))
        }
        m => Err(format!("unexpected method {m}")),
    }
}

fn chain(rent: u64, balance: u64) -> Arc<Mutex<Chain>> {
    Arc::new(Mutex::new(Chain {
        rent,
        balance,
        ..Default::default()
    }))
}

fn addrs(auth: &Pubkey, n: usize) -> Vec<Pubkey> {
    sysix::pool_addresses(auth, "buy", n)
}

#[test]
fn existing_accounts_are_skipped_and_nothing_is_sent() {
    let d = tmpdir();
    let (kp, auth) = keypair_file(d.path(), 11, 0o600);
    let c = chain(1_056_640, 1_000_000_000);
    for a in addrs(&auth, 2) {
        c.lock().unwrap().accounts.insert(
            a.to_string(),
            account_json(&SYSTEM_PROGRAM_ID, &nonce_data(&auth), 1_056_640),
        );
    }
    let (url, log) = serve(c.clone());
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &url,
        "--count",
        "2",
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert_eq!(out.text.matches(" exists ").count(), 2, "{}", out.text);
    assert!(
        out.text.contains("done: created 0, existed 2"),
        "{}",
        out.text
    );
    assert!(!log.lock().unwrap().iter().any(|m| m == "sendTransaction"));
    assert!(c.lock().unwrap().sent.is_empty());
}

#[test]
fn a_missing_account_gets_one_signed_create_and_initialize() {
    let d = tmpdir();
    let (kp, auth) = keypair_file(d.path(), 12, 0o600);
    // the RPC's rent is above the known 1,056,640: the larger is used
    let rent = 1_100_000;
    let c = chain(rent, 1_000_000_000);
    let a = addrs(&auth, 2);
    c.lock().unwrap().accounts.insert(
        a[0].to_string(),
        account_json(&SYSTEM_PROGRAM_ID, &nonce_data(&auth), 1_056_640),
    );
    let (url, log) = serve(c.clone());
    let args = s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &url,
        "--count",
        "2",
    ]);
    let out = run(&args);
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(
        out.text.contains("done: created 1, existed 1"),
        "{}",
        out.text
    );
    let created = out
        .text
        .lines()
        .find(|l| l.contains(" created "))
        .unwrap_or_else(|| panic!("{}", out.text));
    assert!(created.trim_start().starts_with("1  "), "{created}");
    assert!(created.contains(&a[1].to_string()), "{created}");
    assert!(created.trim_end().ends_with(&rent.to_string()), "{created}");
    {
        let ch = c.lock().unwrap();
        assert_eq!(ch.sent.len(), 1, "exactly one transaction");
        let p = &ch.params[0];
        assert_eq!(p[1]["skipPreflight"], false, "{p}");
        assert_eq!(p[1]["encoding"], "base64", "{p}");
        let raw = &ch.sent[0];
        let tx = TxView::parse(raw).unwrap();
        // one signer: the probe key, and the signature verifies
        assert_eq!(tx.num_signatures(), 1);
        let keys = tx.static_account_keys();
        assert_eq!(keys[0], auth);
        assert!(tx.is_signer(0) && tx.is_writable(0));
        assert_eq!(keys[1], a[1]);
        assert!(!tx.is_signer(1) && tx.is_writable(1));
        assert!(!(2..keys.len()).any(|i| tx.is_signer(i)), "no other signer");
        let sig: [u8; 64] = raw[1..65].try_into().unwrap();
        assert!(rugsnipe_tx::signer::verify(&auth, &raw[65..], &sig));
        assert!(
            created.contains(&bs58::encode(sig).into_string()),
            "{created}"
        );
        assert_eq!(tx.recent_blockhash(), &[0x42u8; 32]);
        // both instructions, System program
        let ixs: Vec<_> = tx.instructions().collect();
        assert_eq!(ixs.len(), 2);
        for ix in &ixs {
            assert_eq!(*tx.program_id(ix), SYSTEM_PROGRAM_ID);
        }
        // CreateAccountWithSeed: tag 3, base = probe, seed rs-buy-1, rent, 80 bytes, owner System
        let d0 = &ixs[0].data;
        assert_eq!(&d0[0..4], &3u32.to_le_bytes());
        assert_eq!(&d0[4..36], auth.as_ref());
        assert_eq!(u64::from_le_bytes(d0[36..44].try_into().unwrap()), 8);
        assert_eq!(&d0[44..52], b"rs-buy-1");
        assert_eq!(&d0[52..60], &rent.to_le_bytes());
        assert_eq!(&d0[60..68], &80u64.to_le_bytes());
        assert_eq!(&d0[68..100], SYSTEM_PROGRAM_ID.as_ref());
        assert_eq!(ixs[0].accounts, &[0, 1]);
        // InitializeNonceAccount: tag 6, authority = probe; nonce, RecentBlockhashes, Rent
        let d1 = &ixs[1].data;
        assert_eq!(&d1[0..4], &6u32.to_le_bytes());
        assert_eq!(&d1[4..36], auth.as_ref());
        let acc: Vec<Pubkey> = ixs[1].accounts.iter().map(|i| keys[*i as usize]).collect();
        assert_eq!(
            acc,
            vec![a[1], RECENT_BLOCKHASHES_SYSVAR_ID, RENT_SYSVAR_ID]
        );
    }
    // re-running is idempotent: both exist now, nothing more is sent
    let before = log.lock().unwrap().len();
    let again = run(&args);
    assert_eq!(again.code, 0, "{}", again.text);
    assert!(
        again.text.contains("done: created 0, existed 2"),
        "{}",
        again.text
    );
    assert_eq!(c.lock().unwrap().sent.len(), 1);
    assert!(!log.lock().unwrap()[before..]
        .iter()
        .any(|m| m == "sendTransaction"));
}

#[test]
fn dry_run_sends_nothing() {
    let d = tmpdir();
    let (kp, auth) = keypair_file(d.path(), 13, 0o400);
    let c = chain(1_056_640, 1_000_000_000);
    let (url, log) = serve(c.clone());
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &url,
        "--count",
        "3",
        "--dry-run",
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.starts_with("DRY RUN"), "{}", out.text);
    assert_eq!(
        out.text.matches(" would create ").count(),
        3,
        "{}",
        out.text
    );
    for a in addrs(&auth, 3) {
        assert!(out.text.contains(&a.to_string()), "{}", out.text);
    }
    assert!(out.text.contains("need 3184920 lamports"), "{}", out.text);
    let methods = log.lock().unwrap().clone();
    assert!(
        methods.iter().all(|m| [
            "getMinimumBalanceForRentExemption",
            "getAccountInfo",
            "getBalance"
        ]
        .contains(&m.as_str())),
        "{methods:?}"
    );
    assert!(c.lock().unwrap().sent.is_empty());
    // offline: the addresses only, no network
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--count",
        "1",
        "--start",
        "2",
        "--dry-run",
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(
        out.text.contains(&addrs(&auth, 3)[2].to_string()),
        "{}",
        out.text
    );
    assert!(out.text.contains("not checked"), "{}", out.text);
    // a real run needs --rpc-url
    assert_eq!(
        run(&s(&["nonce", "create", "--keypair", &kp])).code,
        2,
        "--rpc-url required"
    );
}

#[cfg(unix)]
#[test]
fn a_group_readable_keypair_is_refused_before_any_rpc_call() {
    let d = tmpdir();
    let (kp, _) = keypair_file(d.path(), 14, 0o640);
    let c = chain(1_056_640, 1_000_000_000);
    let (url, log) = serve(c.clone());
    for extra in [&[][..], &["--dry-run"][..]] {
        let mut a = s(&["nonce", "create", "--keypair", &kp, "--rpc-url", &url]);
        a.extend(s(extra));
        let out = run(&a);
        assert_ne!(out.code, 0, "{}", out.text);
        assert!(out.text.contains("group/world-accessible"), "{}", out.text);
        assert!(out.text.contains("chmod 400"), "{}", out.text);
    }
    assert!(log.lock().unwrap().is_empty(), "no RPC call");
    assert!(c.lock().unwrap().sent.is_empty());
}

#[test]
fn a_mistyped_dry_run_or_any_unknown_argument_is_refused_before_any_rpc_call() {
    let d = tmpdir();
    let (kp, _) = keypair_file(d.path(), 16, 0o600);
    let c = chain(1_056_640, 1_000_000_000);
    let (url, log) = serve(c.clone());
    let full = |extra: &[&str]| {
        let mut a = s(&[
            "nonce",
            "create",
            "--keypair",
            &kp,
            "--rpc-url",
            &url,
            "--count",
            "2",
        ]);
        a.extend(s(extra));
        a
    };
    for (extra, want) in [
        (&["--dryrun"][..], "did you mean --dry-run"),
        (&["--dry_run"][..], "did you mean --dry-run"),
        (&["--Dry-Run"][..], "did you mean --dry-run"),
        (&["-n"][..], "did you mean --dry-run"),
        (&["--dry-run=1"][..], "with no value"),
        (&["--count=32"][..], "with a space"),
        (&["--verbose"][..], "unknown argument \"--verbose\""),
        (&["32"][..], "a value without an option"),
        (&["--count", "3"][..], "--count given twice"),
        (&["--dry-run", "--dry-run"][..], "--dry-run given twice"),
        (&["--start"][..], "--start needs a value"),
        (&["--creds", "--dry-run"][..], "--creds needs a value"),
    ] {
        let out = run(&full(extra));
        assert_eq!(out.code, 2, "{extra:?}: {}", out.text);
        assert!(out.text.contains(want), "{extra:?}: {}", out.text);
        assert!(out.text.contains("nothing was read, signed or sent"));
        assert!(
            out.text.contains("--dry-run        check only"),
            "usage shown"
        );
        assert!(
            !out.text.contains("nonce create: authority"),
            "{}",
            out.text
        );
    }
    // the value option missing its value at the front, too
    let out = run(&s(&["nonce", "create", "--keypair", "--dry-run"]));
    assert_eq!(out.code, 2, "{}", out.text);
    assert!(out.text.contains("--keypair needs a value"), "{}", out.text);
    assert!(log.lock().unwrap().is_empty(), "no RPC call");
    assert!(c.lock().unwrap().sent.is_empty());
}

#[test]
fn help_prints_the_options_and_sends_nothing() {
    let d = tmpdir();
    let (kp, _) = keypair_file(d.path(), 17, 0o600);
    let c = chain(1_056_640, 1_000_000_000);
    let (url, log) = serve(c.clone());
    for a in [
        s(&["nonce", "create", "--help"]),
        s(&["nonce", "create", "-h"]),
        // added to the end of a full real command
        s(&[
            "nonce",
            "create",
            "--keypair",
            &kp,
            "--rpc-url",
            &url,
            "--count",
            "32",
            "--help",
        ]),
        // even next to a typo
        s(&["nonce", "create", "--keypair", &kp, "--dryrun", "-h"]),
    ] {
        let out = run(&a);
        assert_eq!(out.code, 0, "{a:?}: {}", out.text);
        assert!(
            out.text.starts_with("usage: rugsnipe-ctl nonce create"),
            "{}",
            out.text
        );
        for o in [
            "--keypair",
            "--rpc-url",
            "--count",
            "--start",
            "--dry-run",
            "--creds",
            "--confirm-s",
        ] {
            assert!(out.text.contains(o), "{o}: {}", out.text);
        }
        assert!(out.text.contains(
            "rugsnipe-ctl nonce create --keypair /etc/rugsnipe/keys/probe-keypair.json --rpc-url cred:RPC_URL --count 32 --dry-run"
        ));
    }
    // without --keypair: the reason and the options, exit 2
    let out = run(&s(&["nonce", "create"]));
    assert_eq!(out.code, 2, "{}", out.text);
    assert!(out.text.contains("--keypair FILE required"), "{}", out.text);
    assert!(out.text.contains("usage: rugsnipe-ctl nonce create"));
    assert!(log.lock().unwrap().is_empty(), "no RPC call");
    assert!(c.lock().unwrap().sent.is_empty());
}

#[test]
fn the_first_failure_stops_the_run() {
    let d = tmpdir();
    let (kp, auth) = keypair_file(d.path(), 15, 0o600);
    let c = chain(1_056_640, 1_000_000_000);
    c.lock().unwrap().refuse_send = Some(1);
    let (url, _log) = serve(c.clone());
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &url,
        "--count",
        "3",
    ]));
    assert_eq!(out.code, 1, "{}", out.text);
    assert!(out.text.contains(" created "), "{}", out.text);
    assert!(out.text.contains(" failed "), "{}", out.text);
    assert!(out.text.contains("STOPPED at #1"), "{}", out.text);
    assert!(out.text.contains("simulation failed"), "{}", out.text);
    // #2 was never attempted
    let ch = c.lock().unwrap();
    assert_eq!(ch.sent.len(), 2);
    assert!(!ch.accounts.contains_key(&addrs(&auth, 3)[2].to_string()));
}

#[test]
fn bad_accounts_and_too_little_sol_stop_before_sending() {
    let d = tmpdir();
    let (kp, auth) = keypair_file(d.path(), 16, 0o600);
    // someone sent SOL to #1: it can never be created with a seed
    let c = chain(1_056_640, 1_000_000_000);
    c.lock().unwrap().accounts.insert(
        addrs(&auth, 2)[1].to_string(),
        account_json(&SYSTEM_PROGRAM_ID, &[], 1_000),
    );
    let (url, _) = serve(c.clone());
    let args = s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &url,
        "--count",
        "2",
    ]);
    let out = run(&args);
    assert_eq!(out.code, 1, "{}", out.text);
    assert!(out.text.contains("BAD #1"), "{}", out.text);
    assert!(out.text.contains("STOPPED before sending"), "{}", out.text);
    assert!(c.lock().unwrap().sent.is_empty());
    // too little SOL for 2 accounts
    let c = chain(1_056_640, 1_500_000);
    let (url, _) = serve(c.clone());
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &url,
        "--count",
        "2",
    ]));
    assert_eq!(out.code, 1, "{}", out.text);
    assert!(out.text.contains("NOT ENOUGH"), "{}", out.text);
    assert!(c.lock().unwrap().sent.is_empty());
    // a hostile rent value is refused
    let c = chain(50_000_000, 1_000_000_000);
    let (url, _) = serve(c.clone());
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &url,
        "--count",
        "1",
    ]));
    assert_eq!(out.code, 1, "{}", out.text);
    assert!(out.text.contains("more than twice"), "{}", out.text);
    assert!(c.lock().unwrap().sent.is_empty());
}

#[cfg(unix)]
#[test]
fn cred_url_is_read_from_the_api_keys_file_and_never_printed() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmpdir();
    let (kp, _) = keypair_file(d.path(), 17, 0o600);
    let c = chain(1_056_640, 1_000_000_000);
    let (url, log) = serve(c.clone());
    let creds = d.path().join("keys");
    std::fs::create_dir(&creds).unwrap();
    let keyed = format!("{url}?api-key=s3cr3tvalue");
    let f = creds.join("api-keys.toml");
    std::fs::write(&f, format!("# rpc\nRPC_URL = \"{keyed}\"\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    let creds_s = creds.display().to_string();
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        "cred:RPC_URL",
        "--creds",
        &creds_s,
        "--count",
        "1",
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.contains("created 1"), "{}", out.text);
    assert!(
        !out.text.contains("s3cr3t") && !out.text.contains("api-key"),
        "{}",
        out.text
    );
    assert!(out.text.contains("(cred:RPC_URL)"), "{}", out.text);
    assert!(!log.lock().unwrap().is_empty());
    // a keyed plain URL on the command line is refused (ps, shell history)
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        &keyed,
        "--dry-run",
    ]));
    assert_eq!(out.code, 2, "{}", out.text);
    assert!(
        out.text.contains("cred:RPC_URL") && !out.text.contains("s3cr3t"),
        "{}",
        out.text
    );
    // a group-readable api-keys file is refused
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
    let out = run(&s(&[
        "nonce",
        "create",
        "--keypair",
        &kp,
        "--rpc-url",
        "cred:RPC_URL",
        "--creds",
        &creds_s,
        "--dry-run",
    ]));
    assert_eq!(out.code, 2, "{}", out.text);
    assert!(out.text.contains("group/world-accessible"), "{}", out.text);
}
