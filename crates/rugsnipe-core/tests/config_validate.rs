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

/// docs/06 section 6, item by item: starting from a configuration and an
/// environment that pass, each single defect alone refuses probe AND live
/// with exactly its own reason, and boundaries are where the spec puts them.
#[test]
fn each_section6_refusal_alone_refuses_probe_and_live() {
    for mode in [Mode::Probe, Mode::Live] {
        let base = armable(example(), mode);
        let env0 = good_env(&base);
        assert!(base.validate(&env0).ok(), "{mode}: baseline passes");
        let only = |c: &Config, env: &ValidateEnv, want: Vec<Refusal>, what: &str| {
            let v = c.validate(env);
            assert_eq!(v.refusals, want, "{mode}: {what}");
            // and paper still runs with the same defect
            let mut p = c.clone();
            p.mode.mode = Mode::Paper;
            assert!(p.validate(env).ok(), "{mode}: paper unaffected by {what}");
        };
        // (1) a cap is 0 ...
        for (name, _) in base.risk.caps() {
            let mut c = base.clone();
            let r = &mut c.risk;
            match name {
                "max_snipe_lamports" => r.max_snipe_lamports = 0,
                "max_probe_lamports" => r.max_probe_lamports = 0,
                "max_probes_per_day" => r.max_probes_per_day = 0,
                "max_open_positions" => r.max_open_positions = 0,
                "max_daily_loss_lamports" => r.max_daily_loss_lamports = 0,
                "max_consec_losses_per_op" => r.max_consec_losses_per_op = 0,
                "max_buys_per_op_per_day" => r.max_buys_per_op_per_day = 0,
                "max_buys_per_hour" => r.max_buys_per_hour = 0,
                "max_bid_lamports" => r.max_bid_lamports = 0,
                "max_sell_spend_lamports" => r.max_sell_spend_lamports = 0,
                "max_entry_vsol_lamports" => r.max_entry_vsol_lamports = 0,
                "max_hot_balance_lamports" => r.max_hot_balance_lamports = 0,
                "min_hot_balance_lamports" => r.min_hot_balance_lamports = 0,
                other => panic!("unlisted cap {other}"),
            }
            only(&c, &env0, vec![Refusal::CapMissing(name)], name);
        }
        // ... or missing from the file (the whole [risk] table absent)
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/rugsnipe.example.toml"
        ))
        .unwrap();
        let start = text.find("[risk]").unwrap();
        let end = start + text[start..].find("\n[feeds]").unwrap();
        let no_risk = format!("{}{}", &text[..start], &text[end..]);
        let c = armable(Config::from_toml_str(&no_risk).unwrap(), mode);
        let caps: Vec<Refusal> = c
            .risk
            .caps()
            .iter()
            .map(|(n, _)| Refusal::CapMissing(n))
            .collect();
        assert_eq!(caps.len(), 13);
        only(&c, &env0, caps, "no [risk] table");
        // (1b) cu_limit = 0
        let mut c = base.clone();
        c.tx.cu_limit = 0;
        only(&c, &env0, vec![Refusal::CuLimitZero], "cu_limit = 0");
        // (2) a key credential group- or world-readable (or missing)
        let key = match mode {
            Mode::Probe => base.secrets.probe_keypair.clone(),
            _ => base.secrets.hot_keypair.clone(),
        };
        for m in [0o440, 0o640, 0o404, 0o604, 0o644, 0o444, 0o410, 0o401] {
            let mut env = env0.clone();
            env.credential_modes.insert(key.clone(), Some(m));
            only(
                &base,
                &env,
                vec![Refusal::CredentialPermissive {
                    name: key.clone(),
                    mode: m,
                }],
                &format!("credential mode {m:o}"),
            );
        }
        for m in [0o400, 0o600, 0o100400] {
            let mut env = env0.clone();
            env.credential_modes.insert(key.clone(), Some(m));
            assert!(base.validate(&env).ok(), "{mode}: mode {m:o} is private");
        }
        let mut env = env0.clone();
        env.credential_modes.remove(&key);
        only(
            &base,
            &env,
            vec![Refusal::CredentialMissing(key.clone())],
            "credential missing",
        );
        // the api-keys credential counts once a keyed HTTP sender is enabled
        let mut c = base.clone();
        c.senders.nozomi.enabled = true;
        let mut env = env0.clone();
        env.credential_modes
            .insert(c.secrets.api_keys.clone(), Some(0o640));
        only(
            &c,
            &env,
            vec![Refusal::CredentialPermissive {
                name: c.secrets.api_keys.clone(),
                mode: 0o640,
            }],
            "api-keys group-readable",
        );
        // (3) clock offset > 5 ms (either sign), or unknown
        for (ms, ok) in [(5.0, true), (-5.0, true), (5.001, false), (-6.0, false)] {
            let mut env = env0.clone();
            env.clock_offset_ms = Some(ms);
            if ok {
                assert!(base.validate(&env).ok(), "{mode}: offset {ms}");
            } else {
                only(
                    &base,
                    &env,
                    vec![Refusal::ClockOffset { ms, max: 5.0 }],
                    &format!("offset {ms}"),
                );
            }
        }
        let mut env = env0.clone();
        env.clock_offset_ms = None;
        only(&base, &env, vec![Refusal::ClockUnknown], "clock unknown");
        // (4) no healthy post-exec feed
        let mut env = env0.clone();
        env.post_exec_feed_healthy = false;
        only(
            &base,
            &env,
            vec![Refusal::NoPostExecFeed],
            "no post-exec feed",
        );
        // (5) ArmSet unsigned (or otherwise unusable), or gate_passed = false
        for g in [
            ArmSetGate::Missing,
            ArmSetGate::Unsigned,
            ArmSetGate::BadSignature,
            ArmSetGate::ShaMismatch,
            ArmSetGate::Stale,
            ArmSetGate::WrongRegime,
        ] {
            let mut env = env0.clone();
            env.armset = g;
            only(&base, &env, vec![Refusal::ArmSet(g)], &format!("{g:?}"));
        }
        let mut env = env0.clone();
        env.armset = ArmSetGate::Verified { gate_passed: false };
        only(
            &base,
            &env,
            vec![Refusal::GateNotPassed],
            "gate_passed = false",
        );
        // (6) RPC budget sum > 32 (32 itself is allowed)
        let mut c = base.clone();
        assert_eq!(c.rpc_budget.sum(), 32);
        c.rpc_budget.backfill += 1;
        only(
            &c,
            &env0,
            vec![Refusal::RpcBudget { sum: 33 }],
            "rpc budget 33",
        );
    }
}
