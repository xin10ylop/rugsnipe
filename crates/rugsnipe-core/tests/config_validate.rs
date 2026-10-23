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
        armset: ArmSetGate::Verified {
            gate_passed: true,
            probe_allowed: true,
        },
        credential_modes: creds,
        live_cli_flag: true,
        arm_live_token: true,
        armset_probe_stage: None,
    }
}

fn armable(mut c: Config, mode: Mode) -> Config {
    c.mode.mode = mode;
    c.tx.cu_limit = 72_150;
    c.tx.lads_bytes = 12_124_160;
    c.secrets.armset_pubkey = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".into();
    c.secrets.cold_wallet = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".into();
    // probe/live need the post-execution feed configured (docs/08 section 9)
    c.feeds.grpc.enabled = true;
    c.feeds.grpc.url = "https://api.rpcpool.com".into();
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
    // `[exit]` is raw for the exit engine: a misspelled table name would be
    // replaced by the default exits without a word, so it is refused (F3)
    let e = Config::from_toml_str("[exit.defualt]\nmax_hold_slots = 5\n").unwrap_err();
    assert!(e.contains("[exit.defualt]"), "{e}");
    assert!(Config::from_toml_str("[exit.default]\nmax_hold_slots = 5\n").is_ok());
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
    env.armset = ArmSetGate::Verified {
        gate_passed: false,
        probe_allowed: false,
    };
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
fn leader_regions_need_known_names_and_a_table() {
    let mut c = example();
    c.entry.leader_regions = vec!["eu".into(), "mars".into()];
    c.entry.leader_regions_file = String::new();
    let v = c.validate(&ValidateEnv::default());
    let s = format!("{:?}", v.refusals);
    assert!(
        s.contains("entry.leader_regions\"")
            || s.contains("entry.leader_regions,")
            || s.contains("leader_regions"),
        "{s}"
    );
    assert!(s.contains("leader_regions_file"), "{s}");
    c.entry.leader_regions = vec!["eu".into()];
    c.entry.leader_regions_file = "/var/lib/rugsnipe/data/leader_regions.json".into();
    assert!(c.validate(&ValidateEnv::default()).ok());
}

#[test]
fn invalid_in_every_mode() {
    let mut c = example();
    c.risk.kill_file = " ".into();
    c.tx.nonce_pool = 0;
    c.bid.split.bam = [0.6, 0.6];
    c.mode.max_age_slots = 0;
    c.bid.lamports = c.risk.max_bid_lamports.max(25_000_000) + 1; // above risk.max_bid_lamports
    let v = c.validate(&ValidateEnv::default());
    assert_eq!(v.refusals.len(), 5, "{:?}", v.refusals);
    assert!(
        format!("{:?}", v.refusals).contains("bid.lamports"),
        "{:?}",
        v.refusals
    );
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

#[test]
fn pumpportal_as_the_only_create_feed_warns() {
    // PumpPortal carries no v1 transactions (docs/03 section 5)
    let mut c = example();
    c.feeds.pumpportal.enabled = true;
    c.feeds.logsws.enabled = false;
    c.feeds.relay.enabled = false;
    let v = c.validate(&ValidateEnv::default());
    assert!(v.ok());
    assert!(
        v.warnings.iter().any(|w| w.contains("only create feed")),
        "{:?}",
        v.warnings
    );
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
        // probe needs probe_allowed (not the gate); live needs gate_passed
        let mut env = env0.clone();
        env.armset = ArmSetGate::Verified {
            gate_passed: false,
            probe_allowed: false,
        };
        only(
            &base,
            &env,
            vec![if mode == Mode::Probe {
                Refusal::ProbeNotAllowed
            } else {
                Refusal::GateNotPassed
            }],
            "ArmSet does not allow this mode",
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

#[test]
fn probe_needs_probe_allowed_not_the_gate_and_live_needs_the_gate() {
    let gate = |gate_passed, probe_allowed| ArmSetGate::Verified {
        gate_passed,
        probe_allowed,
    };
    // probe: probe_allowed is enough, gate_passed alone is not
    let c = armable(example(), Mode::Probe);
    let mut env = good_env(&c);
    env.armset = gate(false, true);
    assert!(c.validate(&env).ok(), "{:?}", c.validate(&env).refusals);
    env.armset = gate(true, false);
    assert_eq!(c.validate(&env).refusals, vec![Refusal::ProbeNotAllowed]);
    // probe still needs every cap and credential
    let mut c2 = c.clone();
    c2.risk.max_probe_lamports = 0;
    env.armset = gate(false, true);
    assert_eq!(
        c2.validate(&env).refusals,
        vec![Refusal::CapMissing("max_probe_lamports")]
    );
    env.credential_modes.remove(&c.secrets.probe_keypair);
    assert_eq!(
        c.validate(&env).refusals,
        vec![Refusal::CredentialMissing(c.secrets.probe_keypair.clone())]
    );
    // probe still needs a signed, fresh ArmSet
    let mut env = good_env(&c);
    env.armset = ArmSetGate::Stale;
    assert_eq!(
        c.validate(&env).refusals,
        vec![Refusal::ArmSet(ArmSetGate::Stale)]
    );
    // live: gate_passed is required, probe_allowed does not substitute
    let c = armable(example(), Mode::Live);
    let mut env = good_env(&c);
    env.armset = gate(false, true);
    assert_eq!(c.validate(&env).refusals, vec![Refusal::GateNotPassed]);
    env.armset = gate(true, false);
    assert!(c.validate(&env).ok());
}

#[test]
fn python_side_sections_parse_and_validate() {
    let toml = r#"
[rpc_budget]
hot = 4
guard = 4
keeper = 8
collector = 12
backfill = 4
per_method = 4
per_host = 10.5
[rpc_budget.methods]
getBlock = 2
getSignaturesForAddress = 3.5

[collector]
db_path = "/var/lib/rugsnipe/analytics.db"
box = "ohio1"
workers = 6
api_rps = 2
tape_delay_s = 2100
funding_hops = 2
cache_tx = true
budget_role = "collector"
rpc_endpoints = ["https://api.mainnet-beta.solana.com"]
logs_url = "wss://api.mainnet-beta.solana.com"
"#;
    let c = Config::from_toml_str(toml).unwrap();
    assert_eq!(c.rpc_budget.per_method, 4.0);
    assert_eq!(c.rpc_budget.per_host, 10.5);
    assert_eq!(c.rpc_budget.methods.get("getBlock"), Some(&2.0));
    assert_eq!(c.rpc_budget.sum(), 32, "only roles count toward the sum");
    assert_eq!(c.collector.box_.as_deref(), Some("ohio1"));
    assert_eq!(c.collector.workers, Some(6));
    assert_eq!(c.collector.api_rps, Some(2.0));
    assert_eq!(c.collector.cache_tx, Some(true));
    assert_eq!(c.collector.duration_s, None);
    assert!(c.validate(&ValidateEnv::default()).ok());
    // defaults when absent
    let d = Config::from_toml_str("").unwrap();
    assert_eq!(
        (d.rpc_budget.per_method, d.rpc_budget.per_host),
        (4.0, 10.0)
    );
    assert!(d.rpc_budget.methods.is_empty());
    assert_eq!(d.collector, Default::default());
    // typos are refused (Python would silently ignore them)
    assert!(Config::from_toml_str("[collector]\nworker = 3\n").is_err());
    assert!(Config::from_toml_str("[rpc_budget]\nper_hosts = 3\n").is_err());
    // validation
    let bad = |t: &str| -> Vec<String> {
        Config::from_toml_str(t)
            .unwrap()
            .validate(&ValidateEnv::default())
            .refusals
            .iter()
            .map(|r| r.to_string())
            .collect()
    };
    for (t, field) in [
        ("[rpc_budget]\nper_method = 0\n", "rpc_budget.per_method"),
        ("[rpc_budget]\nper_host = -1\n", "rpc_budget.per_host"),
        ("[rpc_budget.methods]\ngetBlock = 0\n", "rpc_budget.methods"),
        ("[collector]\nworkers = 0\n", "collector.workers"),
        ("[collector]\napi_rps = 0\n", "collector.api_rps"),
        ("[collector]\ntape_delay_s = -5\n", "collector.tape_delay_s"),
        (
            "[collector]\nbudget_role = \"hotbox\"\n",
            "collector.budget_role",
        ),
        (
            "[collector]\nrpc_endpoints = []\n",
            "collector.rpc_endpoints",
        ),
        (
            "[collector]\npp_url = \"https://pumpportal.fun\"\n",
            "collector.pp_url",
        ),
    ] {
        let r = bad(t);
        assert!(r.len() == 1 && r[0].starts_with(field), "{t:?}: {r:?}");
    }
    // the budget sum rule is unchanged: roles only, <= 32 for probe/live
    let mut c = armable(example(), Mode::Probe);
    c.rpc_budget.per_host = 1_000.0;
    c.rpc_budget.methods.insert("getBlock".into(), 500.0);
    assert!(c.validate(&good_env(&c)).ok());
    c.rpc_budget.hot += 1;
    assert_eq!(
        c.validate(&good_env(&c)).refusals,
        vec![Refusal::RpcBudget { sum: 33 }]
    );
}

#[test]
fn example_collector_and_budget_comments_are_valid_keys() {
    // every commented-out key of the examples' [collector] table (and the
    // commented `methods` line) is a real, valid key: uncomment and parse
    for f in [
        "rugsnipe.example.toml",
        "paper.example.toml",
        "paper-lead.example.toml",
    ] {
        let p = format!("{}/../../config/{f}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(p).unwrap();
        let mut out = String::new();
        let mut in_collector = false;
        for l in text.lines() {
            if l.starts_with('[') {
                in_collector = l.starts_with("[collector]");
            }
            let uncomment = (in_collector && l.starts_with("# ")) || l.starts_with("# methods = ");
            out.push_str(if uncomment { &l[2..] } else { l });
            out.push('\n');
        }
        let c = Config::from_toml_str(&out).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(c.collector.workers, Some(10), "{f}");
        assert_eq!(c.collector.tape_delay_s, Some(2_100.0), "{f}");
        assert_eq!(c.rpc_budget.methods.get("getBlock"), Some(&2.0), "{f}");
        assert!(c.validate(&ValidateEnv::default()).ok(), "{f}");
    }
}

#[test]
fn entry_and_paper_sections_default_parse_and_validate() {
    use rugsnipe_core::config::{EntryFilter, FillModel};
    // defaults (docs/04 sections 2, 6 and 7)
    let d = Config::default();
    assert_eq!(d.paper.fill_model, FillModel::E1);
    assert_eq!(d.paper.entry_premium, 0.011);
    assert_eq!(
        d.paper.failed_attempt_lamports, 0,
        "0 = modelled per leader class"
    );
    assert_eq!(d.bid.failed_attempt_lamports_max, 1_000_000);
    assert_eq!(d.bid.min_priority_lamports, 10_000);
    assert_eq!(d.paper.exit_latency_slots, 1);
    assert_eq!(d.entry.max_premium, 0.02);
    assert_eq!(d.entry.filter, EntryFilter::Any);
    // the lead example
    let p = format!(
        "{}/../../config/paper-lead.example.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let c = Config::from_toml_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    assert_eq!(c.entry.filter, EntryFilter::MultiBundle);
    assert_eq!(c.paper.fill_model, FillModel::E1);
    assert_eq!(c.risk.max_snipe_lamports, 500_000_000);
    assert!(c.validate(&ValidateEnv::default()).ok());
    let ex = c.exit["default"].as_table().unwrap();
    assert_eq!(ex["max_hold_slots"].as_integer(), Some(5));
    assert_eq!(ex["insider_sell"].as_str(), Some("bundle"));
    assert_eq!(ex["no_activity_ms"].as_integer(), Some(1_000));
    // e2 and the filter names parse; unknown values and keys are refused
    let c =
        Config::from_toml_str("[paper]\nfill_model = \"e2\"\n[entry]\nfilter = \"any\"\n").unwrap();
    assert_eq!(
        (c.paper.fill_model, c.entry.filter),
        (FillModel::E2, EntryFilter::Any)
    );
    assert!(Config::from_toml_str("[paper]\nfill_model = \"e3\"\n").is_err());
    assert!(Config::from_toml_str("[entry]\nmax_premuim = 0.02\n").is_err());
    // out-of-range values are refused in every mode
    let mut bad = example();
    bad.entry.max_premium = 1.5;
    bad.paper.entry_premium = -0.1;
    bad.paper.late_block_frac = 1.0;
    bad.paper.slot_ms = 0;
    bad.paper.replay_slots = 0;
    bad.paper.min_replay_slots = 7;
    let v = bad.validate(&ValidateEnv::default());
    let fields: Vec<&str> = v
        .refusals
        .iter()
        .filter_map(|r| match r {
            Refusal::Invalid { field, .. } => Some(*field),
            _ => None,
        })
        .collect();
    for f in [
        "entry.max_premium",
        "paper.entry_premium",
        "paper.late_block_frac",
        "paper.slot_ms",
        "paper.replay_slots",
        "paper.min_replay_slots",
    ] {
        assert!(fields.contains(&f), "{f}: {fields:?}");
    }
}

#[test]
fn failed_attempt_cap_and_pay_on_success_split_are_validated() {
    use rugsnipe_core::bid::{BidPolicy, LeaderClass};
    // defaults: tip classes bid with the tip, priority classes with the fee;
    // BAM is priority-ordered (docs/03 section 6: CU-price order in BAM rounds)
    let d = Config::default();
    assert_eq!(d.bid.split.jito_agave, [0.0, 1.0]);
    assert_eq!(d.bid.split.bam, [1.0, 0.0]);
    assert_eq!(d.bid.split.harmonic, [1.0, 0.0]);
    assert_eq!(d.bid.split.other, [1.0, 0.0]);
    // every example file carries the cap and the pay-on-success split
    for f in ["rugsnipe", "paper", "paper-lead"] {
        let p = format!(
            "{}/../../config/{f}.example.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let c = Config::from_toml_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(c.bid, d.bid, "{f}");
        let v = c.validate(&ValidateEnv::default());
        assert!(v.ok(), "{f}: {:?}", v.refusals);
        assert!(
            !v.warnings.iter().any(|w| w.starts_with("bid.")),
            "{f}: {:?}",
            v.warnings
        );
    }
    // below 2 x base fee + floor: no transaction could be sent within it
    let mut c = example();
    c.bid.failed_attempt_lamports_max = 2 * 5_000 + c.bid.min_priority_lamports - 1;
    let v = c.validate(&ValidateEnv::default());
    assert!(
        v.refusals
            .iter()
            .any(|r| matches!(r, Refusal::Invalid { field, .. }
            if *field == "bid.failed_attempt_lamports_max")),
        "{:?}",
        v.refusals
    );
    c.bid.failed_attempt_lamports_max = 0;
    assert!(!c.validate(&ValidateEnv::default()).ok());
    // exactly at the floor is accepted
    c.bid.failed_attempt_lamports_max = 2 * 5_000 + c.bid.min_priority_lamports;
    assert!(c.validate(&ValidateEnv::default()).ok());
    // a priority share on a tip-ordered class is paid on failure: warned
    let mut c = example();
    c.bid.split.jito_agave = [0.5, 0.5];
    let v = c.validate(&ValidateEnv::default());
    assert!(v.ok());
    assert!(
        v.warnings
            .iter()
            .any(|w| w.starts_with("bid.split.jito_agave")),
        "{:?}",
        v.warnings
    );
    // the TOML key parses and the cap binds the priority classes
    let c = Config::from_toml_str(
        "[bid]\nfailed_attempt_lamports_max = 505_000\n[risk]\nmax_bid_lamports = 25_000_000\n",
    )
    .unwrap();
    assert_eq!(c.bid.failed_attempt_lamports_max, 505_000);
    let pol = BidPolicy::from_cfg(&c.bid);
    let p = pol.plan(LeaderClass::Harmonic, c.risk.max_bid_lamports, 1);
    assert_eq!((p.priority_lamports, p.clamped), (500_000, true));
    // leader mix must be a distribution
    let mut c = example();
    c.paper.leader_mix.bam = 0.9;
    assert!(c
        .validate(&ValidateEnv::default())
        .refusals
        .iter()
        .any(|r| matches!(r, Refusal::Invalid { field, .. } if *field == "paper.leader_mix")));
    // unknown keys under [bid] are still refused
    assert!(Config::from_toml_str("[bid]\nfailed_attempt_max = 1\n").is_err());
}

#[test]
fn probe_stages_narrow_the_risk_caps_and_the_armset_must_agree() {
    use rugsnipe_core::config::{ProbeCfg, ProbeProfile, ProbeStage};
    // every example carries the two named stages of docs/04 section 7
    for f in ["rugsnipe", "paper", "paper-lead"] {
        let p = format!(
            "{}/../../config/{f}.example.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let c = Config::from_toml_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(c.probe, ProbeCfg::default(), "{f}");
        assert_eq!(c.probe.stage, ProbeStage::A, "{f}");
    }
    let a = ProbeProfile::stage_a();
    let b = ProbeProfile::stage_b();
    assert_eq!(
        (
            a.max_probe_lamports,
            a.max_probes_per_day,
            a.max_open_positions,
            a.max_daily_loss_lamports
        ),
        (20_000_000, 50, 1, 200_000_000)
    );
    assert_eq!(
        (
            b.max_probe_lamports,
            b.max_probes_per_day,
            b.max_open_positions,
            b.max_daily_loss_lamports
        ),
        (250_000_000, 300, 2, 1_000_000_000)
    );
    // paper and live: [risk] as written
    let c = armable(example(), Mode::Live);
    assert_eq!(c.effective_risk(), c.risk);
    // probe A: the stage narrows every one of its four caps
    let mut c = armable(example(), Mode::Probe);
    let r = c.effective_risk();
    assert_eq!(
        (
            r.max_probe_lamports,
            r.max_probes_per_day,
            r.max_open_positions,
            r.max_daily_loss_lamports
        ),
        (20_000_000, 50, 1, 200_000_000)
    );
    assert_eq!(r.max_snipe_lamports, c.risk.max_snipe_lamports);
    let v = c.validate(&good_env(&c));
    assert!(v.ok(), "{:?}", v.refusals);
    assert!(
        !v.warnings.iter().any(|w| w.contains("binds tighter")),
        "{:?}",
        v.warnings
    );
    // probe B
    c.probe.stage = ProbeStage::B;
    let r = c.effective_risk();
    assert_eq!(
        (
            r.max_probe_lamports,
            r.max_probes_per_day,
            r.max_open_positions,
            r.max_daily_loss_lamports
        ),
        (250_000_000, 300, 2, 1_000_000_000)
    );
    // a tighter [risk] cap still binds, and is reported
    c.risk.max_probe_lamports = 20_000_000;
    assert_eq!(c.effective_risk().max_probe_lamports, 20_000_000);
    let v = c.validate(&good_env(&c));
    assert!(
        v.warnings
            .iter()
            .any(|w| w.starts_with("risk.max_probe_lamports")),
        "{:?}",
        v.warnings
    );
    // the ArmSet's stage must match the config's (moving to B takes both):
    // stage b also needs an ArmSet that names it, stage a accepts none
    let refused_stage = |v: &rugsnipe_core::config::Validation| {
        v.refusals
            .iter()
            .any(|r| matches!(r, Refusal::Invalid { field, .. } if *field == "probe.stage"))
    };
    assert!(refused_stage(&c.validate(&good_env(&c))));
    let mut ca = c.clone();
    ca.probe.stage = ProbeStage::A;
    assert!(!refused_stage(&ca.validate(&good_env(&ca))));
    let mut env = good_env(&c);
    env.armset_probe_stage = Some(ProbeStage::A);
    let v = c.validate(&env);
    assert!(
        v.refusals
            .iter()
            .any(|r| matches!(r, Refusal::Invalid { field, .. } if *field == "probe.stage")),
        "{:?}",
        v.refusals
    );
    env.armset_probe_stage = Some(ProbeStage::B);
    assert!(c.validate(&env).ok());
    // a zero stage cap is refused in probe mode
    c.probe.b.max_open_positions = 0;
    assert!(!c.validate(&env).ok());
    // TOML: the keys parse, an unknown stage or key is refused
    let c = Config::from_toml_str(
        "[probe]\nstage = \"b\"\nkill = { min_landed = 100, min_attempts = 400, max_median_premium = 0.03, landing_attempts = 200 }\n",
    )
    .unwrap();
    assert_eq!(c.probe.stage, ProbeStage::B);
    assert!(Config::from_toml_str("[probe]\nstage = \"c\"\n").is_err());
    assert!(Config::from_toml_str("[probe]\nmax_probe_lamports = 1\n").is_err());
    // the kill rule is validated in every mode
    let mut c = example();
    c.probe.kill.max_median_premium = 0.0;
    assert!(!c.validate(&ValidateEnv::default()).ok());
}

#[test]
fn grpc_feed_keys_are_validated() {
    // the new spelling and the old one both parse
    let new = Config::from_toml_str(
        "[feeds]\ngrpc = { enabled = true, url = \"https://api.rpcpool.com\", x_token = \"GRPC_TOKEN\", stale_ms = 2000, bundle_run = false }\n",
    )
    .unwrap();
    assert!(new.feeds.grpc.enabled && !new.feeds.grpc.bundle_run);
    assert_eq!(new.feeds.grpc.x_token, "GRPC_TOKEN");
    let old = Config::from_toml_str(
        "[feeds]\ngrpc = { enabled = false, endpoint = \"https://x\", token = \"T\", stale_ms = 2000 }\n",
    )
    .unwrap();
    assert_eq!(
        (old.feeds.grpc.url.as_str(), old.feeds.grpc.x_token.as_str()),
        ("https://x", "T")
    );
    assert!(old.feeds.grpc.bundle_run, "default on");
    assert!(Config::from_toml_str("[feeds]\ngrpc = { urll = \"x\" }\n").is_err());
    // invalid values in every mode
    let mut c = example();
    c.feeds.grpc.enabled = true;
    c.feeds.grpc.url = "grpc.example:10000".into();
    c.feeds.grpc.stale_ms = 50;
    c.feeds.grpc.x_token = "abc=def".into();
    c.feeds.probe_live_require = "none".into();
    c.risk.send_error_window = 3;
    c.risk.max_disk_used_pct = 0.0;
    c.risk.kill_clock_offset_ms = -1.0;
    c.risk.max_slot_drift = 0;
    let v = c.validate(&ValidateEnv::default());
    let fields: Vec<String> = v
        .refusals
        .iter()
        .filter_map(|r| match r {
            Refusal::Invalid { field, .. } => Some(field.to_string()),
            _ => None,
        })
        .collect();
    for f in [
        "feeds.grpc.url",
        "feeds.grpc.stale_ms",
        "feeds.grpc.x_token",
        "feeds.probe_live_require",
        "risk.send_error_window",
        "risk.max_disk_used_pct",
        "risk.kill_clock_offset_ms",
        "risk.max_slot_drift",
    ] {
        assert!(fields.iter().any(|x| x == f), "{f} not refused: {fields:?}");
    }
    // plain http to a remote host: a warning (the token is unencrypted)
    let mut c = example();
    c.feeds.grpc.enabled = true;
    c.feeds.grpc.url = "http://grpc.example:10000".into();
    let v = c.validate(&ValidateEnv::default());
    assert!(v.ok(), "{:?}", v.refusals);
    assert!(v.warnings.iter().any(|w| w.contains("plain http")));
    // paper with the gRPC feed off is fine; probe without it is refused even
    // when something claims a healthy feed
    for m in [Mode::Probe, Mode::Live] {
        let mut c = armable(example(), m);
        c.feeds.grpc.enabled = false;
        let v = c.validate(&good_env(&c));
        assert!(
            v.refusals.contains(&Refusal::NoPostExecFeedConfigured),
            "{m}: {:?}",
            v.refusals
        );
        // a shred feed is not built into this binary: it does not count
        c.feeds.shreds.enabled = true;
        assert!(c
            .validate(&good_env(&c))
            .refusals
            .contains(&Refusal::NoPostExecFeedConfigured));
    }
    // Telegram commands need the bot token's name
    let mut c = example();
    c.obs.alert.telegram = String::new();
    c.obs.alert.commands = true;
    assert!(!c.validate(&ValidateEnv::default()).ok());
}

/// The operator-wave keys (docs/09): a zero premium is valid, the operator
/// table and the bundle-shape filter parse from TOML with their defaults,
/// and each refuses the combination it cannot serve.
#[test]
fn operator_wave_entry_keys_parse_and_validate() {
    let d = Config::default();
    assert_eq!(d.entry.operators_file, "");
    assert!(!d.entry.operators_only);
    assert!(d.entry.bundle_wallets.is_empty());
    let mut c = example();
    c.entry.max_premium = 0.0;
    assert!(
        c.validate(&ValidateEnv::default()).ok(),
        "max_premium = 0 is valid"
    );
    let p = armable(c.clone(), Mode::Probe);
    let v = p.validate(&good_env(&p));
    assert!(
        !format!("{:?}", v.refusals).contains("max_premium"),
        "{:?}",
        v.refusals
    );
    // operators_only needs the table; bundle_wallets needs the park
    c.entry.operators_only = true;
    c.entry.bundle_wallets = vec![3];
    c.entry.require_bundle = false;
    let s = format!("{:?}", c.validate(&ValidateEnv::default()).refusals);
    assert!(s.contains("entry.operators_only"), "{s}");
    assert!(s.contains("entry.bundle_wallets"), "{s}");
    c.entry.operators_file = "/var/lib/rugsnipe/data/operators/ahw-2026-10-23.json".into();
    c.entry.require_bundle = true;
    c.feeds.grpc.bundle_run = true;
    let v = c.validate(&ValidateEnv::default());
    assert!(v.ok(), "{:?}", v.refusals);
    // the TOML form
    let t = r#"
[entry]
max_premium = 0.0
require_bundle = true
operators_file = "/etc/rugsnipe/operators.json"
operators_only = true
bundle_wallets = [2, 3, 4]
[feeds]
grpc = { enabled = false, url = "", x_token = "", stale_ms = 4_000, bundle_run = true }
"#;
    let c = Config::from_toml_str(t).unwrap();
    assert_eq!(c.entry.max_premium, 0.0);
    assert_eq!(c.entry.operators_file, "/etc/rugsnipe/operators.json");
    assert!(c.entry.operators_only);
    assert_eq!(c.entry.bundle_wallets, vec![2, 3, 4]);
}
