//! Safety rules: paper is the default and needs no keys; the paper sender
//! never opens a socket; real senders need a feature AND probe/live AND a
//! passing validation (signed gate_passed ArmSet, every cap...).

use pump_codec::Pubkey;
use rugsnipe_core::config::{ArmSetGate, Config, Refusal, ValidateEnv};
use rugsnipe_core::Mode;
use rugsnipe_tx::senders::paper::Paper;
use rugsnipe_tx::senders::*;
use std::collections::BTreeMap;

fn example() -> Config {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/rugsnipe.example.toml"
    );
    Config::from_toml_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn variant(i: u8) -> Variant {
    Variant::from_bytes(
        &[i; 300],
        [i; 64],
        7,
        i,
        VariantKind::Buy,
        1_000,
        1_000_000,
        Some(pump_codec::constants::JITO_TIP_ACCOUNTS[0]),
    )
}

/// Socket file descriptors of this process (Linux).
fn sockets() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .map(|d| {
            d.filter_map(|e| std::fs::read_link(e.ok()?.path()).ok())
                .filter(|l| l.to_string_lossy().starts_with("socket:"))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn paper_sender_never_opens_a_socket() {
    let before = sockets();
    let p = Paper::new(16);
    for i in 0..200u8 {
        assert_eq!(p.send(&variant(i)), SendResult::Recorded);
    }
    assert_eq!(p.total(), 200);
    assert_eq!(p.records().len(), 16, "bounded");
    assert!(!p.is_real());
    assert_eq!(sockets(), before);
    // and its source has no networking code
    let src = include_str!("../src/senders/paper.rs");
    for banned in [
        "std::net",
        "TcpStream",
        "UdpSocket",
        "ureq",
        "http::",
        "connect(",
    ] {
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect();
        assert!(!code.contains(banned), "paper.rs mentions {banned}");
    }
}

#[test]
fn paper_mode_builds_only_paper_even_with_real_senders_enabled() {
    let mut c = example();
    assert_eq!(c.mode.mode, Mode::Paper, "paper is the default");
    c.senders.helius.enabled = true;
    c.senders.jito.enabled = true;
    c.senders.rpcsend.enabled = true;
    let set = SenderSet::build(&c, &ValidateEnv::default(), &ApiKeys::default()).unwrap();
    assert!(set.real().is_empty());
    let before = sockets();
    let r = set.send_all(&variant(1));
    assert_eq!(r, vec![(SenderId::Paper, SendResult::Recorded)]);
    assert_eq!(sockets(), before);
}

fn good_env(c: &Config) -> ValidateEnv {
    let mut creds = BTreeMap::new();
    for n in [
        &c.secrets.hot_keypair,
        &c.secrets.probe_keypair,
        &c.secrets.api_keys,
    ] {
        creds.insert(n.clone(), Some(0o400));
    }
    ValidateEnv {
        clock_offset_ms: Some(0.5),
        post_exec_feed_healthy: true,
        armset: ArmSetGate::Verified { gate_passed: true },
        credential_modes: creds,
        live_cli_flag: true,
        arm_live_token: true,
    }
}

fn armable(mode: Mode) -> Config {
    let mut c = example();
    c.mode.mode = mode;
    c.tx.cu_limit = 72_150;
    c.tx.lads_bytes = 12_124_160;
    c.secrets.armset_pubkey = Pubkey::new_unique().to_string();
    c.secrets.cold_wallet = Pubkey::new_unique().to_string();
    c.senders.rpcsend.enabled = false;
    c
}

#[test]
fn real_modes_refused_without_signed_gate_or_caps() {
    for mode in [Mode::Probe, Mode::Live] {
        let c = armable(mode);
        // unsigned ArmSet
        let mut env = good_env(&c);
        env.armset = ArmSetGate::Unsigned;
        match SenderSet::build(&c, &env, &ApiKeys::default()) {
            Err(BuildError::Refused(r)) => {
                assert!(r.contains(&Refusal::ArmSet(ArmSetGate::Unsigned)))
            }
            other => panic!("{other:?}"),
        }
        // gate not passed
        let mut env = good_env(&c);
        env.armset = ArmSetGate::Verified { gate_passed: false };
        assert!(matches!(
            SenderSet::build(&c, &env, &ApiKeys::default()),
            Err(BuildError::Refused(_))
        ));
        // a cap missing
        let mut c2 = c.clone();
        c2.risk.max_daily_loss_lamports = 0;
        assert!(matches!(
            SenderSet::build(&c2, &good_env(&c2), &ApiKeys::default()),
            Err(BuildError::Refused(_))
        ));
        // everything set, no real sender enabled: builds, nothing real
        let set = SenderSet::build(&c, &good_env(&c), &ApiKeys::default()).unwrap();
        assert!(set.real().is_empty());
    }
}

#[test]
fn enabled_real_sender_needs_its_feature() {
    let mut c = armable(Mode::Probe);
    c.senders.helius.enabled = true;
    let r = SenderSet::build(&c, &good_env(&c), &ApiKeys::default());
    if cfg!(feature = "senders") {
        let set = r.unwrap();
        assert_eq!(set.real().len(), 1);
        assert_eq!(set.real()[0].id(), SenderId::Helius);
        assert_eq!(set.real()[0].min_tip(), 5_000, "swqos_only minimum");
    } else {
        assert_eq!(
            r.unwrap_err(),
            BuildError::FeatureMissing {
                sender: "helius",
                feature: "senders"
            }
        );
    }
    let mut c = armable(Mode::Live);
    c.senders.rpcsend.enabled = true;
    let r = SenderSet::build(&c, &good_env(&c), &ApiKeys::default());
    if cfg!(feature = "rpc") {
        assert_eq!(r.unwrap().real()[0].id(), SenderId::Rpc);
    } else {
        assert_eq!(
            r.unwrap_err(),
            BuildError::FeatureMissing {
                sender: "rpcsend",
                feature: "rpc"
            }
        );
    }
}

#[test]
fn tip_checks_happen_before_any_network() {
    let jito = pump_codec::constants::JITO_TIP_ACCOUNTS;
    let mut v = variant(1);
    assert_eq!(check_tip(&v, &jito, 1_000), Ok(()));
    v.tip_account = Some(Pubkey::new_unique());
    assert_eq!(check_tip(&v, &jito, 1_000), Err(SendError::WrongTipAccount));
    v.tip_account = None;
    assert_eq!(check_tip(&v, &jito, 1_000), Err(SendError::WrongTipAccount));
    let v = variant(2);
    assert_eq!(
        check_tip(&v, &jito, 2_000_000),
        Err(SendError::TipTooLow {
            min: 2_000_000,
            got: 1_000_000
        })
    );
    let big = Variant::from_bytes(&[0; 1_300], [0; 64], 1, 0, VariantKind::Buy, 0, 0, None);
    assert_eq!(check_tip(&big, &[], 0), Err(SendError::TooLarge));
}

#[test]
fn breaker_opens_and_recovers() {
    let b = Breaker::new(3, 1_000);
    assert!(b.allow(0));
    b.on_failure(10);
    b.on_failure(11);
    assert!(b.allow(12));
    b.on_failure(12);
    assert!(b.is_open(13) && b.is_open(1_011));
    assert!(b.allow(1_012), "half-open after the cooldown");
    b.on_success();
    assert_eq!(b.consecutive_failures(), 0);
}

#[test]
fn api_keys_parse_and_never_print() {
    let k = ApiKeys::parse(&zeroize::Zeroizing::new(
        "# keys\nNOZOMI_KEY = \"abc-secret\"\nJITO = \"x\"\n".into(),
    ))
    .unwrap();
    assert_eq!(k.get("NOZOMI_KEY"), Some("abc-secret"));
    assert!(!format!("{k:?}").contains("abc-secret"));
    assert!(ApiKeys::parse(&zeroize::Zeroizing::new("A = unquoted".into())).is_err());
}
