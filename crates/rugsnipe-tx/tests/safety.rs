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
        env.armset = ArmSetGate::Verified {
            gate_passed: false,
            probe_allowed: false,
        };
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
    c.senders.helius_swqos.enabled = true;
    let mut keys = ApiKeys::default();
    keys.insert("HELIUS_KEY", "k");
    let r = SenderSet::build(&c, &good_env(&c), &keys);
    if cfg!(feature = "senders") {
        let set = r.unwrap();
        assert_eq!(set.real().len(), 2);
        assert_eq!(set.real()[0].id(), SenderId::Helius);
        assert_eq!(set.real()[0].min_tip(), 1_000_000, "Sender minimum");
        assert_eq!(set.real()[1].id(), SenderId::HeliusSwqos);
        assert_eq!(set.real()[1].min_tip(), 5_000, "swqos_only minimum");
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

/// Every docs/06 section 6 refusal alone keeps `SenderSet::build` from
/// constructing anything real, even with a real sender enabled.
#[test]
fn each_section6_defect_alone_refuses_the_real_sender_set() {
    for mode in [Mode::Probe, Mode::Live] {
        let mut base = armable(mode);
        base.senders.jito.enabled = true;
        type Defect = Box<dyn Fn(&mut Config, &mut ValidateEnv)>;
        let defects: Vec<(&str, Defect)> = vec![
            ("cap 0", Box::new(|c, _| c.risk.max_open_positions = 0)),
            ("cu_limit 0", Box::new(|c, _| c.tx.cu_limit = 0)),
            (
                "group-readable key",
                Box::new(|c, e| {
                    for k in [&c.secrets.hot_keypair, &c.secrets.probe_keypair] {
                        e.credential_modes.insert(k.clone(), Some(0o640));
                    }
                }),
            ),
            (
                "world-readable key",
                Box::new(|c, e| {
                    for k in [&c.secrets.hot_keypair, &c.secrets.probe_keypair] {
                        e.credential_modes.insert(k.clone(), Some(0o604));
                    }
                }),
            ),
            ("clock 6 ms", Box::new(|_, e| e.clock_offset_ms = Some(6.0))),
            (
                "no post-exec feed",
                Box::new(|_, e| e.post_exec_feed_healthy = false),
            ),
            (
                "unsigned ArmSet",
                Box::new(|_, e| e.armset = ArmSetGate::Unsigned),
            ),
            (
                "gate_passed false",
                Box::new(|_, e| {
                    e.armset = ArmSetGate::Verified {
                        gate_passed: false,
                        probe_allowed: false,
                    }
                }),
            ),
            ("rpc budget 33", Box::new(|c, _| c.rpc_budget.hot += 1)),
        ];
        for (what, f) in defects {
            let mut c = base.clone();
            let mut env = good_env(&c);
            f(&mut c, &mut env);
            match SenderSet::build(&c, &env, &ApiKeys::default()) {
                Err(BuildError::Refused(r)) => assert_eq!(r.len(), 1, "{mode} {what}: {r:?}"),
                other => panic!("{mode} {what}: {other:?}"),
            }
        }
    }
}

/// `rpcsend.sells_only` (the public RPC path of docs/06 section 6) refuses
/// buy variants before any network; sells go out.
#[cfg(feature = "rpc")]
#[test]
fn rpcsend_sells_only_refuses_buys_before_the_network() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering::*};
    use std::sync::Arc;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicU64::new(0));
    let a = accepted.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            a.fetch_add(1, AcqRel);
            s.set_read_timeout(Some(std::time::Duration::from_millis(300)))
                .unwrap();
            let mut b = [0u8; 8192];
            let _ = s.read(&mut b);
            let body = r#"{"jsonrpc":"2.0","result":"sig","id":1}"#;
            let _ = s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    let mut c = armable(Mode::Live);
    c.senders.rpcsend.enabled = true;
    c.senders.rpcsend.url = format!("http://127.0.0.1:{port}");
    c.senders.rpcsend.sells_only = true;
    let set = SenderSet::build(&c, &good_env(&c), &ApiKeys::default()).unwrap();
    assert_eq!(set.real().len(), 1);
    let r = set.send_all(&variant(1));
    assert_eq!(
        r[1],
        (
            SenderId::Rpc,
            SendResult::Failed(SendError::NotForThisSender)
        )
    );
    assert_eq!(accepted.load(Acquire), 0, "a buy never reached the RPC");
    let sell = Variant::from_bytes(&[2; 300], [2; 64], 7, 0x81, VariantKind::Sell, 0, 0, None);
    let r = set.send_all(&sell);
    assert!(
        matches!(r[1], (SenderId::Rpc, SendResult::Accepted { .. })),
        "{r:?}"
    );
    assert_eq!(accepted.load(Acquire), 1, "the sell went out");
}

#[cfg(feature = "senders")]
#[test]
fn jito_regions_and_blockrazor_build_one_sender_each() {
    let mut c = armable(Mode::Probe);
    c.senders.jito.enabled = true;
    c.senders.jito.regions = vec!["frankfurt".into(), "amsterdam".into()];
    c.senders.blockrazor.enabled = true;
    c.senders.blockrazor.key = "BLOCKRAZOR_KEY".into();
    let mut keys = ApiKeys::default();
    keys.insert("BLOCKRAZOR_KEY", "b");
    let set = SenderSet::build(&c, &good_env(&c), &keys).unwrap();
    let labels: Vec<&str> = set.real().iter().map(|s| s.label()).collect();
    assert_eq!(
        labels,
        vec!["blockrazor", "jito_frankfurt", "jito_amsterdam"]
    );
    assert!(set
        .real()
        .iter()
        .filter(|s| s.id() == SenderId::Jito)
        .all(|s| s.buy_variants() == 1));
    // a dry set holds the same targets but can send through none of them
    let dry = SenderSet::build_dry(&c, &good_env(&c), &keys).unwrap();
    assert!(dry.real().is_empty() && dry.targets().len() == 3);
    let mut paper = c.clone();
    paper.mode.mode = Mode::Paper;
    assert!(SenderSet::build_dry(&paper, &good_env(&paper), &keys).is_err());
}
