//! Real key loading for probe mode (docs/06 section 13 "Secrets", docs/08
//! sections 4 and 13): the probe wallet and the sender API keys, from the
//! systemd credentials directory only (`$CREDENTIALS_DIRECTORY`, filled by
//! `LoadCredential=`), never from a path on the command line.
//!
//! | credential (`[secrets]` name) | what | used in |
//! |---|---|---|
//! | `probe_keypair` (`probe-keypair`) | the probe wallet (solana-keygen JSON) | stages A and B |
//! | `hot_keypair` (`hot-keypair`) | the live wallet | live: refused in this build |
//! | `api_keys` (`api-keys`) | `NAME = "value"` lines: `HELIUS_KEY`, `NOZOMI_KEY`, `ASTRALANE_KEY`, `BLOCKRAZOR_KEY`, `NEXTBLOCK_KEY` (optional) | the senders whose `key` names them |
//!
//! Every refusal is a [`Refusal`] (the same types `Config::validate`
//! reports): a missing file, a group- or world-accessible one, a malformed
//! keypair, an API key name a sender needs but the file lacks, or `live`.
//! Key material only ever lives in zeroizing buffers ([`HotSigner`],
//! [`ApiKeys`]); no error or debug output contains it.

use rugsnipe_core::config::{Config, Refusal};
use rugsnipe_core::Mode;
use rugsnipe_tx::senders::ApiKeys;
use rugsnipe_tx::signer::{HotSigner, KeyError};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// The environment variable systemd sets for `LoadCredential=`.
pub const CREDENTIALS_ENV: &str = "CREDENTIALS_DIRECTORY";

/// The probe wallet and the sender API keys.
pub struct RealKeys {
    pub signer: HotSigner,
    pub api: ApiKeys,
}

impl core::fmt::Debug for RealKeys {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RealKeys")
            .field("signer", &self.signer)
            .field("api", &self.api)
            .finish()
    }
}

/// `$CREDENTIALS_DIRECTORY`, or a refusal naming it.
pub fn credentials_dir() -> Result<PathBuf, Refusal> {
    match std::env::var_os(CREDENTIALS_ENV) {
        Some(d) if !d.is_empty() => Ok(PathBuf::from(d)),
        _ => Err(Refusal::Invalid {
            field: "secrets",
            why: format!(
                "${CREDENTIALS_ENV} is not set: real keys are loaded only from systemd credentials (LoadCredential=)"
            ),
        }),
    }
}

/// The units' fallback for a missing credential (`SetCredential=<name>:ABSENT`):
/// systemd hands the literal `ABSENT` to the process, which means "no key".
pub const ABSENT: &str = "ABSENT";

/// The file is systemd's `ABSENT` placeholder.
pub fn is_absent(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.len() <= 16)
        && std::fs::read(p).is_ok_and(|b| b.trim_ascii() == ABSENT.as_bytes())
}

/// Mode bits of a credential (`None` = missing, unreadable metadata, or
/// systemd's `ABSENT` placeholder).
pub fn mode_of(p: &Path) -> Option<u32> {
    if is_absent(p) {
        return None;
    }
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

/// `credential name -> mode` for every `[secrets]` credential named in the
/// config (the `ValidateEnv::credential_modes` input).
pub fn credential_modes(cfg: &Config, dir: &Path) -> BTreeMap<String, Option<u32>> {
    let mut m = BTreeMap::new();
    for name in [
        &cfg.secrets.hot_keypair,
        &cfg.secrets.probe_keypair,
        &cfg.secrets.api_keys,
    ] {
        if !name.is_empty() {
            m.insert(name.clone(), mode_of(&dir.join(name)));
        }
    }
    m
}

fn check_file(name: &str, dir: &Path) -> Result<PathBuf, Refusal> {
    if name.is_empty() {
        return Err(Refusal::CredentialMissing("<unnamed>".into()));
    }
    let p = dir.join(name);
    match mode_of(&p) {
        None => Err(Refusal::CredentialMissing(name.to_string())),
        Some(m) if m & 0o077 != 0 => Err(Refusal::CredentialPermissive {
            name: name.to_string(),
            mode: m,
        }),
        Some(_) => Ok(p),
    }
}

/// The API key names the enabled senders need (`key` of each enabled HTTP
/// sender; `helius_swqos` falls back to `helius.key`).
pub fn needed_api_keys(cfg: &Config) -> Vec<String> {
    let s = &cfg.senders;
    let mut v: Vec<String> = vec![];
    for (name, c) in s.http() {
        if !c.enabled {
            continue;
        }
        let key = if name == "helius_swqos" && c.key.is_empty() {
            s.helius.key.clone()
        } else {
            c.key.clone()
        };
        if !key.is_empty() && !v.contains(&key) {
            v.push(key);
        }
    }
    v
}

/// Load the probe wallet and the API keys from `dir` for `cfg`'s mode.
/// Probe stages A and B use the probe wallet; live is refused here (this
/// build has no live path). Every problem found is returned.
pub fn load_real_keys(cfg: &Config, dir: &Path) -> Result<RealKeys, Vec<Refusal>> {
    let mut r = vec![];
    match cfg.mode.mode {
        Mode::Probe => {}
        Mode::Live => {
            return Err(vec![Refusal::Invalid {
                field: "mode",
                why: "live is refused in this build (probe only; no hot-keypair is loaded)".into(),
            }])
        }
        Mode::Paper => {
            return Err(vec![Refusal::Invalid {
                field: "mode",
                why: "paper mode loads no keys (throwaway wallet)".into(),
            }])
        }
    }
    let signer = match check_file(&cfg.secrets.probe_keypair, dir) {
        Ok(p) => match HotSigner::load_file(&p) {
            Ok(s) => Some(s),
            Err(e) => {
                r.push(key_refusal(&cfg.secrets.probe_keypair, &e));
                None
            }
        },
        Err(e) => {
            r.push(e);
            None
        }
    };
    let needed = needed_api_keys(cfg);
    let api = if needed.is_empty() && cfg.secrets.api_keys.is_empty() {
        Some(ApiKeys::default())
    } else if needed.is_empty() {
        // named but unused: load it if present (a broken file is still refused)
        match check_file(&cfg.secrets.api_keys, dir) {
            Ok(p) => read_api_keys(&p, &cfg.secrets.api_keys)
                .map_err(|e| r.push(e))
                .ok(),
            Err(Refusal::CredentialMissing(_)) => Some(ApiKeys::default()),
            Err(e) => {
                r.push(e);
                None
            }
        }
    } else {
        match check_file(&cfg.secrets.api_keys, dir) {
            Ok(p) => match read_api_keys(&p, &cfg.secrets.api_keys) {
                Ok(k) => {
                    for n in &needed {
                        if k.get(n).is_none_or(str::is_empty) {
                            r.push(Refusal::Invalid {
                                field: "secrets.api_keys",
                                why: format!(
                                    "API key {n:?} (named by an enabled sender) is not in credential {:?}",
                                    cfg.secrets.api_keys
                                ),
                            });
                        }
                    }
                    Some(k)
                }
                Err(e) => {
                    r.push(e);
                    None
                }
            },
            Err(e) => {
                r.push(e);
                None
            }
        }
    };
    match (signer, api, r.is_empty()) {
        (Some(signer), Some(api), true) => Ok(RealKeys { signer, api }),
        _ => Err(r),
    }
}

/// A named secret that is not a sender key: the gRPC `x-token`
/// (`[feeds] grpc.x_token`) and the Telegram bot token / chat id
/// (`[obs] alert.telegram` / `alert.chat`). Looked up as a `NAME = "value"`
/// line of the `api-keys` credential first, then as a credential file of
/// that name (one line). Works in every mode (paper may run the gRPC feed:
/// a feed, not a sender) and never touches a wallet. `Ok(None)`: not found;
/// a group/world-accessible file is refused.
pub fn named_secret(
    cfg: &Config,
    dir: &Path,
    name: &str,
) -> Result<Option<Zeroizing<String>>, Refusal> {
    if name.is_empty() {
        return Ok(None);
    }
    if !cfg.secrets.api_keys.is_empty() {
        match check_file(&cfg.secrets.api_keys, dir) {
            Ok(p) => {
                let k = read_api_keys(&p, &cfg.secrets.api_keys)?;
                if let Some(v) = k.get(name).filter(|v| !v.is_empty()) {
                    return Ok(Some(Zeroizing::new(v.to_string())));
                }
            }
            Err(Refusal::CredentialMissing(_)) => {}
            Err(e) => return Err(e),
        }
    }
    match check_file(name, dir) {
        Ok(p) => {
            let raw = Zeroizing::new(
                std::fs::read(&p).map_err(|_| Refusal::CredentialMissing(name.into()))?,
            );
            let text =
                Zeroizing::new(
                    String::from_utf8(raw.to_vec()).map_err(|_| Refusal::Invalid {
                        field: "secrets",
                        why: format!("credential {name:?} is not UTF-8"),
                    })?,
                );
            let v = text.trim();
            Ok((!v.is_empty()).then(|| Zeroizing::new(v.to_string())))
        }
        Err(Refusal::CredentialMissing(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

fn key_refusal(name: &str, e: &KeyError) -> Refusal {
    match e {
        KeyError::Permissions(m) => Refusal::CredentialPermissive {
            name: name.to_string(),
            mode: *m,
        },
        KeyError::Io(_) => Refusal::CredentialMissing(name.to_string()),
        // Format / mismatch: the message names the problem, never the bytes
        other => Refusal::Invalid {
            field: "secrets.probe_keypair",
            why: format!("credential {name:?}: {other}"),
        },
    }
}

fn read_api_keys(p: &Path, name: &str) -> Result<ApiKeys, Refusal> {
    let raw =
        Zeroizing::new(std::fs::read(p).map_err(|_| Refusal::CredentialMissing(name.into()))?);
    let text = Zeroizing::new(
        String::from_utf8(raw.to_vec()).map_err(|_| Refusal::Invalid {
            field: "secrets.api_keys",
            why: format!("credential {name:?} is not UTF-8"),
        })?,
    );
    ApiKeys::parse(&text).map_err(|why| Refusal::Invalid {
        field: "secrets.api_keys",
        // the parser's message names a line number, never a value
        why: format!("credential {name:?}: {why}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn keypair_json(seed: u8) -> String {
        let s = HotSigner::from_seed(&Zeroizing::new([seed; 32]));
        let mut all = vec![seed; 32];
        all.extend_from_slice(s.pubkey().as_ref());
        format!(
            "[{}]",
            all.iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rugsnipe-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(d: &Path, name: &str, text: &str, mode: u32) {
        let p = d.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn probe_cfg() -> Config {
        let mut c = Config::default();
        c.mode.mode = Mode::Probe;
        c.secrets.probe_keypair = "probe-keypair".into();
        c.secrets.hot_keypair = "hot-keypair".into();
        c.secrets.api_keys = "api-keys".into();
        c.senders.nozomi.enabled = true;
        c.senders.nozomi.key = "NOZOMI_KEY".into();
        c.senders.blockrazor.enabled = true;
        c.senders.blockrazor.key = "BLOCKRAZOR_KEY".into();
        c
    }

    #[test]
    fn loads_the_probe_wallet_and_api_keys_and_never_prints_them() {
        let d = dir("ok");
        write(&d, "probe-keypair", &keypair_json(7), 0o400);
        write(
            &d,
            "api-keys",
            "# sender keys\nNOZOMI_KEY = \"noz-secret\"\nBLOCKRAZOR_KEY = \"br-secret\"\n",
            0o400,
        );
        let k = load_real_keys(&probe_cfg(), &d).unwrap();
        let want = HotSigner::from_seed(&Zeroizing::new([7; 32])).pubkey();
        assert_eq!(k.signer.pubkey(), want);
        assert_eq!(k.api.get("NOZOMI_KEY"), Some("noz-secret"));
        let dbg = format!("{k:?}");
        assert!(!dbg.contains("secret") && !dbg.contains("[7,"), "{dbg}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refusal_matrix() {
        let cfg = probe_cfg();
        // missing everything
        let d = dir("missing");
        let e = load_real_keys(&cfg, &d).unwrap_err();
        assert!(
            e.contains(&Refusal::CredentialMissing("probe-keypair".into())),
            "{e:?}"
        );
        assert!(
            e.contains(&Refusal::CredentialMissing("api-keys".into())),
            "{e:?}"
        );
        // group/world readable
        write(&d, "probe-keypair", &keypair_json(1), 0o644);
        write(
            &d,
            "api-keys",
            "NOZOMI_KEY = \"a\"\nBLOCKRAZOR_KEY = \"b\"\n",
            0o440,
        );
        let e = load_real_keys(&cfg, &d).unwrap_err();
        assert!(
            e.contains(&Refusal::CredentialPermissive {
                name: "probe-keypair".into(),
                mode: 0o644
            }),
            "{e:?}"
        );
        assert!(
            e.contains(&Refusal::CredentialPermissive {
                name: "api-keys".into(),
                mode: 0o440
            }),
            "{e:?}"
        );
        // malformed keypair, a needed API key missing
        write(&d, "probe-keypair", "[1,2,3]", 0o400);
        write(&d, "api-keys", "NOZOMI_KEY = \"a\"\n", 0o400);
        let e = load_real_keys(&cfg, &d).unwrap_err();
        let text = e
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("not a JSON array"), "{text}");
        assert!(text.contains("BLOCKRAZOR_KEY"), "{text}");
        // unquoted value: refused without echoing it
        write(&d, "probe-keypair", &keypair_json(1), 0o400);
        write(&d, "api-keys", "NOZOMI_KEY = sekrit\n", 0o400);
        let e = load_real_keys(&cfg, &d).unwrap_err();
        let text = format!("{e:?}");
        assert!(
            text.contains("line 1") && !text.contains("sekrit"),
            "{text}"
        );
        // live is refused even with every file present
        write(&d, "hot-keypair", &keypair_json(2), 0o400);
        let mut live = cfg.clone();
        live.mode.mode = Mode::Live;
        let e = load_real_keys(&live, &d).unwrap_err();
        assert!(e[0].to_string().contains("live is refused"), "{e:?}");
        // systemd's placeholder for a missing credential is a missing credential
        write(&d, "probe-keypair", "ABSENT", 0o400);
        let e = load_real_keys(&cfg, &d).unwrap_err();
        assert!(
            e.contains(&Refusal::CredentialMissing("probe-keypair".into())),
            "{e:?}"
        );
        assert_eq!(credential_modes(&cfg, &d)["probe-keypair"], None);
        write(&d, "probe-keypair", &keypair_json(1), 0o400);
        // no sender needs a key and no api-keys file: fine
        let mut keyless = cfg.clone();
        keyless.senders.nozomi.enabled = false;
        keyless.senders.blockrazor.enabled = false;
        std::fs::remove_file(d.join("api-keys")).unwrap();
        assert!(load_real_keys(&keyless, &d).is_ok());
        // modes as Config::validate sees them
        let m = credential_modes(&cfg, &d);
        assert_eq!(m["probe-keypair"], Some(0o400));
        assert_eq!(m["api-keys"], None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn named_secrets_come_from_api_keys_or_their_own_file() {
        let d = dir("named");
        let cfg = probe_cfg();
        write(
            &d,
            "api-keys",
            "HELIUS_KEY = \"h\"\nGRPC_TOKEN = \"tok-1\"\n",
            0o400,
        );
        assert_eq!(
            named_secret(&cfg, &d, "GRPC_TOKEN")
                .unwrap()
                .as_deref()
                .map(|s| s.as_str()),
            Some("tok-1")
        );
        assert!(named_secret(&cfg, &d, "TG_TOKEN").unwrap().is_none());
        write(&d, "TG_CHAT", "-100123\n", 0o400);
        assert_eq!(
            named_secret(&cfg, &d, "TG_CHAT")
                .unwrap()
                .as_deref()
                .map(|s| s.as_str()),
            Some("-100123")
        );
        write(&d, "TG_TOKEN", "bot:x", 0o644);
        assert!(matches!(
            named_secret(&cfg, &d, "TG_TOKEN"),
            Err(Refusal::CredentialPermissive { .. })
        ));
        assert!(named_secret(&cfg, &d, "").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn needed_keys_follow_enabled_senders() {
        let mut c = probe_cfg();
        c.senders.helius.key = "HELIUS_KEY".into();
        c.senders.helius_swqos.enabled = true; // shares helius.key
        assert_eq!(
            needed_api_keys(&c),
            vec![
                "HELIUS_KEY".to_string(),
                "NOZOMI_KEY".into(),
                "BLOCKRAZOR_KEY".into()
            ]
        );
    }
}
