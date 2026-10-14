//! Config parsing (the checked-in example) and every refusal path.

use rugsnipe_core::config::{ArmSetGate, Config, Refusal, TxFormat, ValidateEnv};
use rugsnipe_core::Mode;
use std::collections::BTreeMap;

fn example() -> Config {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/rugsnipe.example.toml"
    );
    Config::from_toml_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn good_env(cfg: &Config) -> ValidateEnv {
    let mut creds = BTreeMap::new();
    creds.insert(cfg.secrets.hot_keypair.clone(), Some(0o400));
    creds.insert(cfg.secrets.probe_keypair.clone(), Some(0o600));
    creds.insert(cfg.secrets.api_keys.clone(), Some(0o400));
    ValidateEnv {
        clock_offset_ms: Some(0.4),
        post_exec_feed_healthy: true,
        armset: ArmSetGate::Verified { gate_passed: true },
        credential_modes: creds,
        live_cli_flag: true,
        arm_live_token: true,
    }
}

fn armable(mut c: Config, mode: Mode) -> Config {
    c.mode.mode = mode;
    c.tx.cu_limit = 72_150;
    c.tx.lads_bytes = 12_124_160;
    c.secrets.armset_pubkey = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".into();
    c.secrets.cold_wallet = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".into();
    c
}

#[test]
fn example_parses_and_paper_needs_nothing() {
    let c = example();
    assert_eq!(c.mode.mode, Mode::Paper);
    assert_eq!(c.box_.id, "fra1");
    assert_eq!(c.risk.max_snipe_lamports, 250_000_000);
    assert_eq!(c.tx.format, TxFormat::Legacy);
    assert_eq!(c.rpc_budget.sum(), 32);
    assert!(c.mode.is_frozen(459_700_000) && !c.mode.is_frozen(459_000_000));
    assert!(c.exit.contains_key("default"));
    assert!(
        !c.feeds.pumpportal.enabled,
        "PumpPortal adapter is default-off"
    );
    let v = c.validate(&ValidateEnv::default());
    assert!(v.ok(), "{:?}", v.refusals);
    // an empty file is a valid paper config too
    let empty = Config::from_toml_str("").unwrap();
    assert_eq!(empty.mode.mode, Mode::Paper);
    assert!(empty.validate(&ValidateEnv::default()).ok());
}

#[test]
fn unknown_keys_are_rejected() {
    let e = Config::from_toml_str("[risk]\nmax_snipe_lamport = 1\n").unwrap_err();
    assert!(e.contains("max_snipe_lamport"), "{e}");
    assert!(Config::from_toml_str("[mode]\nmode = \"yolo\"\n").is_err());
}

#[test]
fn probe_and_live_pass_with_everything_set() {
    for m in [Mode::Probe, Mode::Live] {
        let c = armable(example(), m);
        let v = c.validate(&good_env(&c));
        assert!(v.ok(), "{m}: {:?}", v.refusals);
    }
}

#[test]
fn every_refusal_is_reported_at_once() {
    let mut c = armable(example(), Mode::Live);
    c.risk.max_snipe_lamports = 0;
    c.risk.max_daily_loss_lamports = 0;
    c.tx.cu_limit = 0;
    c.tx.lads_bytes = 0;
    c.rpc_budget.collector = 20;
    c.secrets.cold_wallet = "<base58>".into();
    let mut env = good_env(&c);
    env.credential_modes
        .insert(c.secrets.hot_keypair.clone(), Some(0o644));
    env.clock_offset_ms = Some(7.0);
    env.post_exec_feed_healthy = false;
    env.armset = ArmSetGate::Verified { gate_passed: false };
    env.live_cli_flag = false;
    env.arm_live_token = false;
    let v = c.validate(&env);
    let r = &v.refusals;
    for want in [
        Refusal::CapMissing("max_snipe_lamports"),
        Refusal::CapMissing("max_daily_loss_lamports"),
        Refusal::CuLimitZero,
        Refusal::LadsZero,
        Refusal::CredentialPermissive {
            name: c.secrets.hot_keypair.clone(),
            mode: 0o644,
        },
        Refusal::ClockOffset { ms: 7.0, max: 5.0 },
        Refusal::NoPostExecFeed,
        Refusal::GateNotPassed,
        Refusal::RpcBudget { sum: 40 },
        Refusal::BadPubkey {
            field: "secrets.cold_wallet",
        },
        Refusal::LiveFlagMissing,
        Refusal::ArmLiveTokenMissing,
    ] {
        assert!(r.contains(&want), "missing {want:?} in {r:?}");
    }
    assert_eq!(r.len(), 12, "{r:?}");
    for x in r {
        assert!(!x.to_string().is_empty());
    }
}

#[test]
fn armset_states_and_missing_credentials() {
    let c = armable(example(), Mode::Probe);
    for g in [
        ArmSetGate::Missing,
        ArmSetGate::Unsigned,
        ArmSetGate::BadSignature,
        ArmSetGate::Stale,
        ArmSetGate::WrongRegime,
    ] {
        let mut env = good_env(&c);
        env.armset = g;
        assert_eq!(c.validate(&env).refusals, vec![Refusal::ArmSet(g)]);
    }
    let mut env = good_env(&c);
    env.credential_modes.clear();
    env.clock_offset_ms = None;
    let r = c.validate(&env).refusals;
    assert!(r.contains(&Refusal::CredentialMissing("probe-keypair".into())));
    assert!(r.contains(&Refusal::ClockUnknown));
    // probe does not need the live flag or the hot keypair
    assert!(!r.contains(&Refusal::LiveFlagMissing));
    assert!(!r.contains(&Refusal::CredentialMissing("hot-keypair".into())));
}

#[test]
fn invalid_in_every_mode() {
    let mut c = example();
    c.risk.kill_file = " ".into();
    c.tx.nonce_pool = 0;
    c.bid.split.bam = [0.6, 0.6];
    c.mode.max_age_slots = 0;
    let v = c.validate(&ValidateEnv::default());
    assert_eq!(v.refusals.len(), 4, "{:?}", v.refusals);
    assert!(v
        .refusals
        .iter()
        .all(|r| matches!(r, Refusal::Invalid { .. })));
}

#[test]
fn paper_with_real_senders_warns() {
    let mut c = example();
    c.senders.helius.enabled = true;
    c.feeds.pumpportal.enabled = true;
    let v = c.validate(&ValidateEnv::default());
    assert!(v.ok());
    assert_eq!(v.warnings.len(), 2, "{:?}", v.warnings);
}
