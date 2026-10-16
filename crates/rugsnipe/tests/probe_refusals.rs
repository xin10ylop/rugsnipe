//! The refusal matrix of the real `rugsnipe-hot` binary in probe (and live)
//! mode: every case exits 2 with the reason, before any state file, feed or
//! sender exists. Keys come only from `$CREDENTIALS_DIRECTORY`.
//!
//! | case | reason printed |
//! |---|---|
//! | no `$CREDENTIALS_DIRECTORY` | names the variable |
//! | probe keypair missing | `credential "probe-keypair" missing` |
//! | probe keypair group/world readable | `group/world-accessible (mode 644)` |
//! | `api-keys` lacks a sender's key | the key name, never a value |
//! | ArmSet without `probe_allowed` | `probe_allowed = false` |
//! | ArmSet stage b, config stage a | `names probe stage b, the config a` |
//! | live | `live is refused in this build` |
//! | `--probe-dry` in paper | `--probe-dry needs mode = probe` |

mod common;

use common::*;
use rugsnipe_core::config::ProbeStage;
use std::path::{Path, PathBuf};

fn probe_toml(dir: &Path, signer_pubkey: &str) -> String {
    format!(
        r#"
[box]
id = "probe-test"
[mode]
mode = "probe"
[secrets]
probe_keypair = "probe-keypair"
hot_keypair = "hot-keypair"
api_keys = "api-keys"
armset_pubkey = "{signer_pubkey}"
cold_wallet = "{cold}"
[risk]
max_snipe_lamports = 250_000_000
max_probe_lamports = 250_000_000
max_probes_per_day = 300
max_open_positions = 2
max_daily_loss_lamports = 1_000_000_000
max_consec_losses_per_op = 20
max_buys_per_op_per_day = 300
max_buys_per_hour = 40
max_bid_lamports = 25_000_000
max_sell_spend_lamports = 20_000_000
max_entry_vsol_lamports = 40_000_000_000
max_hot_balance_lamports = 5_000_000_000
min_hot_balance_lamports = 100_000_000
kill_file = "{kill}"
[feeds]
logsws = {{ enabled = false }}
pumpportal = {{ enabled = false }}
[senders]
rpcsend = {{ enabled = false }}
helius = {{ enabled = true, url = "http://127.0.0.1:9/fast", key = "HELIUS_KEY" }}
blockrazor = {{ enabled = true, url = "http://127.0.0.1:9/sendTransaction", key = "BLOCKRAZOR_KEY" }}
[tx]
cu_limit = 88_000
lads_bytes = 12_124_160
[probe]
stage = "a"
[obs]
metrics_listen = []
rslog_dir = "{rslog}"
"#,
        cold = pump_codec::Pubkey::new_unique(),
        kill = dir.join("KILL").display(),
        rslog = dir.join("rslog").display(),
    )
}

struct Case {
    dir: PathBuf,
    cfg: PathBuf,
    armset: PathBuf,
    creds: PathBuf,
}

fn case(name: &str, probe_allowed: bool, stage: Option<ProbeStage>) -> Case {
    let dir = tmpdir(&format!("probe-refuse-{name}"));
    let signing = rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([77; 32]));
    let cfg = dir.join("probe.toml");
    std::fs::write(&cfg, probe_toml(&dir, &signing.pubkey().to_string())).unwrap();
    let mut set = armset(&[pump_codec::Pubkey::new_unique()], &[]);
    set.probe_allowed = probe_allowed;
    set.probe_stage = stage;
    set.operators[0].size_lamports = 20_000_000;
    let armset = signed_armset(&dir, &set, &signing);
    let creds = dir.join("creds");
    std::fs::create_dir_all(&creds).unwrap();
    write_mode(&creds, "probe-keypair", &keypair_json(3), 0o400);
    write_mode(
        &creds,
        "api-keys",
        "HELIUS_KEY = \"hk-VALUE-1\"\nBLOCKRAZOR_KEY = \"br-VALUE-2\"\n",
        0o400,
    );
    Case {
        dir,
        cfg,
        armset,
        creds,
    }
}

fn run(c: &Case, extra: &[&str], creds: Option<&Path>) -> (i32, String) {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_rugsnipe-hot"));
    cmd.arg("--config")
        .arg(&c.cfg)
        .arg("--armset")
        .arg(&c.armset)
        .arg("--state")
        .arg(c.dir.join("state.db"))
        .arg("--no-metrics")
        .args(extra)
        .env_remove("CREDENTIALS_DIRECTORY");
    if let Some(d) = creds {
        cmd.env("CREDENTIALS_DIRECTORY", d);
    }
    let out = cmd.output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        !c.dir.join("state.db").exists(),
        "refused before any state was opened:\n{err}"
    );
    assert!(
        !err.contains("VALUE-"),
        "no key material in the output:\n{err}"
    );
    (out.status.code().unwrap_or(-1), err)
}

#[test]
fn every_refusal_is_reported_before_anything_starts() {
    // no $CREDENTIALS_DIRECTORY
    let c = case("nocreds", true, Some(ProbeStage::A));
    let (code, err) = run(&c, &[], None);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("CREDENTIALS_DIRECTORY"), "{err}");
    // the gates this box cannot pass are listed too (no chronyc / gRPC here)
    assert!(err.contains("post-execution"), "{err}");

    // probe keypair missing
    std::fs::remove_file(c.creds.join("probe-keypair")).unwrap();
    let (code, err) = run(&c, &[], Some(&c.creds));
    assert_eq!(code, 2);
    assert!(
        err.contains(r#"credential "probe-keypair" missing"#),
        "{err}"
    );

    // group/world readable keypair
    write_mode(&c.creds, "probe-keypair", &keypair_json(3), 0o644);
    let (code, err) = run(&c, &[], Some(&c.creds));
    assert_eq!(code, 2);
    assert!(
        err.contains(r#"credential "probe-keypair" is group/world-accessible (mode 644)"#),
        "{err}"
    );

    // api-keys without a key an enabled sender names
    write_mode(&c.creds, "probe-keypair", &keypair_json(3), 0o400);
    write_mode(&c.creds, "api-keys", "HELIUS_KEY = \"hk-VALUE-1\"\n", 0o400);
    let (code, err) = run(&c, &[], Some(&c.creds));
    assert_eq!(code, 2);
    assert!(err.contains("BLOCKRAZOR_KEY"), "{err}");
    let _ = std::fs::remove_dir_all(&c.dir);

    // ArmSet without probe_allowed
    let c = case("noprobe", false, Some(ProbeStage::A));
    let (code, err) = run(&c, &[], Some(&c.creds));
    assert_eq!(code, 2);
    assert!(err.contains("probe_allowed = false"), "{err}");
    let _ = std::fs::remove_dir_all(&c.dir);

    // stage mismatch: the ArmSet names b, the config a
    let c = case("stage", true, Some(ProbeStage::B));
    let (code, err) = run(&c, &[], Some(&c.creds));
    assert_eq!(code, 2);
    assert!(
        err.contains("the ArmSet names probe stage b, the config a"),
        "{err}"
    );

    // live: refused whatever else holds
    write_mode(&c.creds, "hot-keypair", &keypair_json(4), 0o400);
    let (code, err) = run(&c, &["--mode", "live", "--live"], Some(&c.creds));
    assert_eq!(code, 2);
    assert!(err.contains("live is refused in this build"), "{err}");

    // --probe-dry is a probe-mode flag
    let (code, err) = run(&c, &["--mode", "paper", "--probe-dry"], Some(&c.creds));
    assert_eq!(code, 2);
    assert!(err.contains("--probe-dry needs mode = probe"), "{err}");
    let _ = std::fs::remove_dir_all(&c.dir);
}
