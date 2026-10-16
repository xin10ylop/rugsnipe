//! Every `rugsnipe-ctl` subcommand, through the library entry point.

use ed25519_dalek::{Signer, SigningKey};
use rugsnipe_ctl::run;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn example() -> String {
    format!(
        "{}/../../config/rugsnipe.example.toml",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// A per-test temporary directory, removed when dropped (no
/// `/tmp/rugsnipe-ctl-*` leftovers).
fn tmpdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("rugsnipe-ctl-test-")
        .tempdir()
        .unwrap()
}

#[test]
fn config_validate_paper_ok_live_refused() {
    let out = run(&s(&["config", "validate", &example()]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.contains("mode = paper") && out.text.ends_with("OK\n"));
    let live = std::fs::read_to_string(example())
        .unwrap()
        .replace("mode = \"paper\"", "mode = \"live\"");
    let d = tmpdir();
    let p = d.path().join("live.toml");
    std::fs::write(&p, live).unwrap();
    let out = run(&s(&[
        "config",
        "validate",
        p.to_str().unwrap(),
        "--creds",
        "/nonexistent",
    ]));
    assert_eq!(out.code, 1);
    for want in [
        "cu_limit = 0",
        "credential \"hot-keypair\" missing",
        "clock offset unknown",
        "no healthy post-execution feed",
        "ArmSet not usable",
        "--live",
        "NOT OK",
    ] {
        assert!(out.text.contains(want), "missing {want:?} in\n{}", out.text);
    }
}

#[test]
fn config_validate_refuses_exit_typos() {
    // the hot binary refuses unknown `[exit.default]` keys at startup; the
    // pre-flight must too (F3: `max_hold_slot` passed `config validate`)
    let lead = format!(
        "{}/../../config/paper-lead.example.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&lead).unwrap();
    assert_eq!(run(&s(&["config", "validate", &lead])).code, 0);
    let d = tmpdir();
    let p = d.path().join("typo.toml");
    std::fs::write(&p, text.replace("max_hold_slots", "max_hold_slot")).unwrap();
    let out = run(&s(&["config", "validate", p.to_str().unwrap()]));
    assert_eq!(out.code, 1, "{}", out.text);
    assert!(
        out.text
            .contains("REFUSED: exit: [exit.default]: unknown field `max_hold_slot`"),
        "{}",
        out.text
    );
    std::fs::write(&p, text.replace("[exit.default]", "[exit.lead]")).unwrap();
    let out = run(&s(&["config", "validate", p.to_str().unwrap()]));
    assert_ne!(out.code, 0, "{}", out.text);
    assert!(
        out.text.contains("unknown table [exit.lead]"),
        "{}",
        out.text
    );
}

#[test]
fn config_validate_refuses_a_missing_filter_model() {
    // rugsnipe-hot refuses to start without `entry.filter_model`; the
    // pre-flight of docs/08 step 5 must say so instead of "OK"
    let lead = format!(
        "{}/../../config/paper-lead.example.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&lead).unwrap();
    let d = tmpdir();
    let p = d.path().join("nomodel.toml");
    std::fs::write(
        &p,
        text.replace(
            "filter_model = \"data/reports/strategy/entry_filter_v1.json\"",
            "filter_model = \"/nonexistent/entry_filter_v1.json\"",
        ),
    )
    .unwrap();
    let out = run(&s(&["config", "validate", p.to_str().unwrap()]));
    assert_eq!(out.code, 1, "{}", out.text);
    assert!(
        out.text
            .contains("REFUSED: entry.filter_model: /nonexistent/entry_filter_v1.json"),
        "{}",
        out.text
    );
}

#[test]
fn template_dump_v1_and_legacy() {
    let mint = "8xWodG9Q37nvqeefeHKqxrgQMdRaVSQvSeiudxispump";
    let wallet = "DXatZCLiwrHKsREZ4z3ej8rCdFCuwRTUD24VWtBPFvVX";
    let out = run(&s(&[
        "template", "dump", "--mint", mint, "--wallet", wallet,
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.contains("V1, 805 bytes"), "{}", out.text);
    assert!(out.text.contains("mask 0xf"));
    // default CU: the hot path's per-mint limit over the 88,000 floor
    let bumps = pump_codec::pda::SnipeBumps::derive(
        &mint.parse().unwrap(),
        &wallet.parse().unwrap(),
        &pump_codec::constants::TOKEN_2022_PROGRAM_ID,
    );
    // the mint's S plus the wallet's UVA term W, as the hot path adds it
    let s_missing =
        bumps.missing() + rugsnipe_tx::cu::wallet_missing_bumps(&wallet.parse().unwrap());
    let want = (65_000 + 1_500 * s_missing).max(88_000);
    assert!(
        out.text
            .contains(&format!("cu_limit {want} (per-mint; S + W = {s_missing}")),
        "{}",
        out.text
    );
    assert!(
        out.text.contains(&format!("cu {:?}", Some(want))),
        "{}",
        out.text
    );
    // no floor: the bare formula; --cu pins a value
    let out = run(&s(&[
        "template",
        "dump",
        "--mint",
        mint,
        "--wallet",
        wallet,
        "--cu-floor",
        "0",
    ]));
    let bare = 65_000 + 1_500 * s_missing;
    assert!(
        out.text.contains(&format!("cu {:?}", Some(bare))),
        "{}",
        out.text
    );
    let out = run(&s(&[
        "template", "dump", "--mint", mint, "--wallet", wallet, "--cu", "123456",
    ]));
    assert!(out.text.contains("cu_limit 123456 (--cu"), "{}", out.text);
    assert!(out.text.contains("cu Some(123456)"), "{}", out.text);
    let out = run(&s(&[
        "template",
        "dump",
        "--mint",
        mint,
        "--wallet",
        wallet,
        "--format",
        "legacy",
        "--nonce",
        "6ytR8oynVvhs8iZSDa6cQfp52fgrXqgtyQyvtjFix6LY",
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.contains("Legacy"));
    assert!(
        run(&s(&[
            "template", "dump", "--mint", "nope", "--wallet", wallet
        ]))
        .code
            != 0
    );
}

#[test]
fn nonce_init_and_close_are_dry_runs() {
    let auth = "DXatZCLiwrHKsREZ4z3ej8rCdFCuwRTUD24VWtBPFvVX";
    let out = run(&s(&["nonce", "init", "--authority", auth, "--count", "2"]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.starts_with("DRY RUN"));
    assert!(out.text.contains("rent locked: 2113280 lamports"));
    assert_eq!(out.text.matches("base64 ").count(), 2);
    let out = run(&s(&[
        "nonce",
        "close",
        "--authority",
        auth,
        "--to",
        auth,
        "--count",
        "1",
        "--pool",
        "sell",
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.contains("rs-sell-0"));
    assert!(
        run(&s(&["nonce", "close", "--authority", auth])).code != 0,
        "--to required"
    );
}

#[test]
fn armset_verify_ok_and_tampered() {
    let sk = SigningKey::from_bytes(&[5u8; 32]);
    let pubkey = bs58::encode(sk.verifying_key().to_bytes()).into_string();
    let body = br#"{"ver":3,"created_at":1000,"regime":"slot200","gate_passed":false,"operators":[],"watch":[]}"#;
    let sha = rugsnipe_core::armset::sha256_hex(body);
    let d = tmpdir();
    let f = d.path().join(format!("armset-3-{sha}.json"));
    std::fs::write(&f, body).unwrap();
    std::fs::write(format!("{}.sig", f.display()), sk.sign(body).to_bytes()).unwrap();
    let out = run(&s(&[
        "armset",
        "verify",
        f.to_str().unwrap(),
        "--pubkey",
        &pubkey,
        "--now",
        "1500",
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    assert!(out.text.contains("gate_passed=false"));
    // tampered body
    std::fs::write(&f, br#"{"ver":3,"created_at":1000,"regime":"slot200","gate_passed":true,"operators":[],"watch":[]}"#).unwrap();
    let out = run(&s(&[
        "armset",
        "verify",
        f.to_str().unwrap(),
        "--pubkey",
        &pubkey,
        "--now",
        "1500",
    ]));
    assert_eq!(out.code, 1);
    assert!(out.text.contains("REJECTED"));
}

#[test]
fn usage_on_unknown() {
    let out = run(&s(&["frobnicate"]));
    assert_eq!(out.code, 2);
    assert!(out.text.contains("usage"));
}

#[test]
fn armset_keygen_sign_verify_roundtrip_and_keygen_refuses_git_tree() {
    let tmp = tmpdir();
    let dir = tmp.path().to_path_buf();
    let key = dir.join("dev.key");
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let out = rugsnipe_ctl::run(&args(&["armset", "keygen", "--out", key.to_str().unwrap()]));
    assert_eq!(out.code, 0, "{}", out.text);
    let pubkey = out
        .text
        .lines()
        .find_map(|l| l.strip_prefix("pubkey "))
        .unwrap()
        .to_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // never overwritten
    assert_ne!(
        rugsnipe_ctl::run(&args(&["armset", "keygen", "--out", key.to_str().unwrap()])).code,
        0
    );
    // refused inside this repository (outside keys/)
    let in_repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("should-not-exist.key");
    let r = rugsnipe_ctl::run(&args(&[
        "armset",
        "keygen",
        "--out",
        in_repo.to_str().unwrap(),
    ]));
    assert_ne!(r.code, 0);
    assert!(!in_repo.exists());
    // sign
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let json = dir.join("in.json");
    std::fs::write(
        &json,
        format!(r#"{{"ver":7,"created_at":{now},"regime":"paper","gate_passed":false,"operators":[],"watch":[],"funding_rules":[]}}"#),
    )
    .unwrap();
    let out = rugsnipe_ctl::run(&args(&[
        "armset",
        "sign",
        "--key",
        key.to_str().unwrap(),
        "--in",
        json.to_str().unwrap(),
        "--out-dir",
        dir.to_str().unwrap(),
    ]));
    assert_eq!(out.code, 0, "{}", out.text);
    let file = out.text.lines().next().unwrap().to_string();
    assert!(file.contains("armset-7-"));
    let v = rugsnipe_ctl::run(&args(&["armset", "verify", &file, "--pubkey", &pubkey]));
    assert_eq!(v.code, 0, "{}", v.text);
    // a group-readable key is refused
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
        let r = rugsnipe_ctl::run(&args(&[
            "armset",
            "sign",
            "--key",
            key.to_str().unwrap(),
            "--in",
            json.to_str().unwrap(),
        ]));
        assert_ne!(r.code, 0);
    }
    drop(tmp);
    assert!(!dir.exists(), "temporary directory removed");
}

#[cfg(not(feature = "rpc"))]
#[test]
fn sim_without_rpc_feature_explains() {
    let out = run(&s(&["sim", "--n", "1"]));
    assert_eq!(out.code, 2);
    assert!(out.text.contains("--features rpc"), "{}", out.text);
}

#[test]
fn kill_status_and_local_unkill() {
    let d = tmpdir();
    let db = d.path().join("state.db");
    let dbs = db.display().to_string();
    // no state.db: refused
    let out = run(&s(&["unkill", "--state", &dbs]));
    assert_eq!(out.code, 2, "{}", out.text);
    // a latch and a trigger as the cold thread writes them
    let c = rugsnipe_state::store::open_conn(&db).unwrap();
    let l = rugsnipe_state::killkv::Latch {
        reason: "unexplained_drop".into(),
        unix: 1_791_000_000,
        detail: "wallet fell 300000000 lamports".into(),
    };
    c.execute(
        "INSERT INTO kv(k, v) VALUES(?1, ?2), (?3, ?2)",
        rusqlite::params![
            rugsnipe_state::killkv::LATCH_KEY,
            l.to_json(),
            "kill_trigger:unexplained_drop"
        ],
    )
    .unwrap();
    drop(c);
    let out = run(&s(&["kill", "status", "--state", &dbs]));
    assert_eq!(out.code, 0);
    assert!(
        out.text.starts_with("KILLED: unexplained_drop"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("trigger unexplained_drop"),
        "{}",
        out.text
    );
    let out = run(&s(&["unkill", "--state", &dbs]));
    assert_eq!(out.code, 0);
    assert!(out.text.starts_with("unkilled"), "{}", out.text);
    let out = run(&s(&["kill", "status", "--state", &dbs]));
    assert!(out.text.starts_with("not killed"), "{}", out.text);
    assert!(
        out.text.contains("trigger unexplained_drop"),
        "history stays"
    );
    let out = run(&s(&["unkill", "--state", &dbs]));
    assert!(out.text.starts_with("not killed"));
}
