//! ArmSet: the signed list of armed operators and watched keys that Ohio
//! pushes to the hot box every 6 h (docs/06 section 5.3).
//!
//! * file `armset-<ver>-<sha256 hex>.json`, the sha256 of the exact file bytes;
//! * detached ed25519 signature `armset-<ver>-<sha>.json.sig` over the same
//!   bytes: 64 raw bytes, or the signature as base58 / hex text;
//! * JSON = P2's schema (`operators`, `watch`, `funding_rules`) plus
//!   `regime`, `min_hot_version`, `gate_passed`, `prereg_sha`,
//!   `shared_fee_payers`. Unknown fields are ignored (forward compatible).
//!
//! If the ArmSet is unsigned, stale or from the wrong regime the hot box runs
//! exits only; `gate_passed = false` keeps buys in paper.

use crate::config::ArmSetGate;
use ed25519_dalek::{Signature, VerifyingKey};
use pump_codec::Pubkey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Operator arming state (docs/06 section 9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpState {
    Candidate,
    Shadow,
    Probe,
    Armed,
    Suspended,
    Retired,
}

impl OpState {
    /// May fire real buys in probe mode.
    pub fn may_probe(self) -> bool {
        matches!(self, Self::Probe | Self::Armed)
    }
    /// May fire real buys in live mode.
    pub fn may_live(self) -> bool {
        matches!(self, Self::Armed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpEntry {
    pub op_id: u32,
    pub state: OpState,
    pub size_lamports: u64,
    pub v_max_lamports: u64,
    #[serde(default)]
    pub ev_cap_lamports: u64,
    #[serde(default)]
    pub ab_skip_p: f64,
    #[serde(default)]
    pub family: Option<String>,
    /// Per-operator exit overrides (interpreted by `rugsnipe-exit`).
    #[serde(default)]
    pub exit: Option<serde_json::Value>,
    #[serde(default)]
    pub stats: Option<serde_json::Value>,
}

/// How a watched key relates to its operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchKind {
    /// Create signer: live-fire key.
    Signer,
    /// Create fee payer: live-fire key.
    FeePayer,
    /// `creator` argument only: shadow (never live, docs/06 ruling 5).
    Creator,
    /// Funded by a watched funder (dynamic, section 9.6).
    Funded,
    /// Received >= 90% of a watched wallet's balance.
    Successor,
}

impl WatchKind {
    /// Keys that may trigger a real buy (signer / fee payer / funding-derived).
    pub fn is_live_key(self) -> bool {
        !matches!(self, Self::Creator)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatchJson {
    #[serde(with = "crate::b58")]
    pub key: Pubkey,
    pub kind: WatchKind,
    pub op_id: u32,
    #[serde(default)]
    pub rule_id: u32,
    #[serde(default)]
    pub expires_slot: Option<u64>,
    #[serde(default)]
    pub keep: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FundingRule {
    #[serde(with = "crate::b58")]
    pub funder: Pubkey,
    /// Inclusive lamport bands `[lo, hi]` (at most 5).
    pub bands: Vec<[u64; 2]>,
    #[serde(default = "yes")]
    pub fresh_only: bool,
    pub ttl_s: u64,
    pub remove_if_no_launch_s: u64,
    #[serde(default)]
    pub last_transfer: bool,
    pub op_id: u32,
    #[serde(default)]
    pub rule_id: u32,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmSet {
    pub ver: u64,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub code_sha: String,
    #[serde(default)]
    pub prereg_sha: String,
    pub created_at: u64,
    #[serde(default)]
    pub valid_until: Option<u64>,
    pub regime: String,
    #[serde(default)]
    pub min_hot_version: String,
    #[serde(default)]
    pub gate_passed: bool,
    #[serde(default, with = "crate::b58::vec")]
    pub shared_fee_payers: Vec<Pubkey>,
    #[serde(default, alias = "ops")]
    pub operators: Vec<OpEntry>,
    #[serde(default)]
    pub watch: Vec<WatchJson>,
    #[serde(default)]
    pub funding_rules: Vec<FundingRule>,
}

/// Why an ArmSet was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmSetError {
    Unsigned,
    BadSignatureEncoding,
    BadSignature,
    BadPubkey,
    ShaMismatch { want: String, got: String },
    Json(String),
    Stale { age_s: u64, max_s: u64 },
    WrongRegime { want: String, got: String },
    TooNewForHot { min_hot_version: String },
}

impl ArmSetError {
    /// Map to the validation gate.
    pub fn gate(&self) -> ArmSetGate {
        match self {
            Self::Unsigned => ArmSetGate::Unsigned,
            Self::BadSignatureEncoding | Self::BadSignature | Self::BadPubkey => {
                ArmSetGate::BadSignature
            }
            Self::ShaMismatch { .. } | Self::Json(_) => ArmSetGate::ShaMismatch,
            Self::Stale { .. } => ArmSetGate::Stale,
            Self::WrongRegime { .. } | Self::TooNewForHot { .. } => ArmSetGate::WrongRegime,
        }
    }
}

impl core::fmt::Display for ArmSetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ArmSetError {}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `(ver, sha)` from `armset-<ver>-<sha>.json` (any directory prefix).
pub fn parse_filename(name: &str) -> Option<(u64, String)> {
    let base = name.rsplit('/').next()?;
    let rest = base.strip_prefix("armset-")?.strip_suffix(".json")?;
    let (ver, sha) = rest.split_once('-')?;
    if sha.len() != 64 || !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((ver.parse().ok()?, sha.to_ascii_lowercase()))
}

/// Decode a detached signature file: 64 raw bytes, or base58 / hex text.
pub fn decode_signature(sig_file: &[u8]) -> Result<[u8; 64], ArmSetError> {
    if sig_file.len() == 64 {
        return Ok(sig_file.try_into().unwrap());
    }
    let text = core::str::from_utf8(sig_file)
        .map_err(|_| ArmSetError::BadSignatureEncoding)?
        .trim();
    if text.len() == 128 && text.bytes().all(|c| c.is_ascii_hexdigit()) {
        let mut out = [0u8; 64];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&text[2 * i..2 * i + 2], 16)
                .map_err(|_| ArmSetError::BadSignatureEncoding)?;
        }
        return Ok(out);
    }
    let v = bs58::decode(text)
        .into_vec()
        .map_err(|_| ArmSetError::BadSignatureEncoding)?;
    v.try_into().map_err(|_| ArmSetError::BadSignatureEncoding)
}

/// Context the hot box checks an ArmSet against.
#[derive(Debug, Clone)]
pub struct VerifyCtx<'a> {
    pub pubkey: &'a Pubkey,
    /// sha from the file name (`None` to skip that check).
    pub expected_sha: Option<&'a str>,
    pub now_unix: u64,
    pub max_age_s: u64,
    /// Current regime (`None` to skip).
    pub regime: Option<&'a str>,
    /// This binary's version, compared to `min_hot_version` (semver-ish).
    pub hot_version: &'a str,
}

/// A verified ArmSet with its sha.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedArmSet {
    pub armset: ArmSet,
    pub sha256: String,
}

impl VerifiedArmSet {
    pub fn gate(&self) -> ArmSetGate {
        ArmSetGate::Verified {
            gate_passed: self.armset.gate_passed,
        }
    }
}

fn version_tuple(v: &str) -> Vec<u64> {
    v.split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// Verify `bytes` (the exact file) against a detached signature and the
/// context. Order: signature, sha, JSON, freshness, regime, hot version.
pub fn verify(
    bytes: &[u8],
    sig_file: Option<&[u8]>,
    ctx: &VerifyCtx<'_>,
) -> Result<VerifiedArmSet, ArmSetError> {
    let sig_file = sig_file.ok_or(ArmSetError::Unsigned)?;
    let sig = Signature::from_bytes(&decode_signature(sig_file)?);
    let vk =
        VerifyingKey::from_bytes(&ctx.pubkey.to_bytes()).map_err(|_| ArmSetError::BadPubkey)?;
    vk.verify_strict(bytes, &sig)
        .map_err(|_| ArmSetError::BadSignature)?;
    let sha = sha256_hex(bytes);
    if let Some(want) = ctx.expected_sha {
        if !want.eq_ignore_ascii_case(&sha) {
            return Err(ArmSetError::ShaMismatch {
                want: want.to_string(),
                got: sha,
            });
        }
    }
    let armset: ArmSet =
        serde_json::from_slice(bytes).map_err(|e| ArmSetError::Json(e.to_string()))?;
    let age = ctx.now_unix.saturating_sub(armset.created_at);
    let expired = armset.valid_until.is_some_and(|u| ctx.now_unix > u);
    if age > ctx.max_age_s || expired {
        return Err(ArmSetError::Stale {
            age_s: age,
            max_s: ctx.max_age_s,
        });
    }
    if let Some(want) = ctx.regime {
        if armset.regime != want {
            return Err(ArmSetError::WrongRegime {
                want: want.to_string(),
                got: armset.regime.clone(),
            });
        }
    }
    if !armset.min_hot_version.is_empty()
        && version_tuple(&armset.min_hot_version) > version_tuple(ctx.hot_version)
    {
        return Err(ArmSetError::TooNewForHot {
            min_hot_version: armset.min_hot_version.clone(),
        });
    }
    Ok(VerifiedArmSet {
        armset,
        sha256: sha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn sample(gate: bool, created_at: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "ver": 57, "run_id": "pq-2026-10-21T06", "code_sha": "c0de", "prereg_sha": "beef",
            "created_at": created_at, "regime": "slot200", "min_hot_version": "0.1.0",
            "gate_passed": gate,
            "shared_fee_payers": ["11111111111111111111111111111111"],
            "operators": [{"op_id": 1234, "state": "armed", "size_lamports": 250_000_000u64,
                           "v_max_lamports": 41_000_000_000u64, "ab_skip_p": 0.2}],
            "watch": [{"key": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P", "kind": "signer", "op_id": 1234}],
            "funding_rules": [{"funder": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
               "bands": [[899_500_000u64, 900_500_000u64]], "ttl_s": 1800, "remove_if_no_launch_s": 3600,
               "last_transfer": true, "op_id": 1234}],
            "future_field": {"ignored": true}
        }))
        .unwrap()
    }

    fn ctx<'a>(pk: &'a Pubkey, sha: Option<&'a str>, now: u64) -> VerifyCtx<'a> {
        VerifyCtx {
            pubkey: pk,
            expected_sha: sha,
            now_unix: now,
            max_age_s: 21_600,
            regime: Some("slot200"),
            hot_version: "0.1.0",
        }
    }

    #[test]
    fn verify_good_and_every_failure() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let pk = Pubkey::new_from_array(sk.verifying_key().to_bytes());
        let bytes = sample(true, 1_000);
        let sig = sk.sign(&bytes).to_bytes();
        let sha = sha256_hex(&bytes);
        let v = verify(&bytes, Some(&sig), &ctx(&pk, Some(&sha), 2_000)).unwrap();
        assert!(v.armset.gate_passed);
        assert_eq!(v.armset.operators[0].state, OpState::Armed);
        assert_eq!(v.armset.watch[0].kind, WatchKind::Signer);
        assert_eq!(v.gate(), ArmSetGate::Verified { gate_passed: true });
        // signature as base58 and hex text
        let b58 = bs58::encode(sig).into_string();
        assert!(verify(&bytes, Some(b58.as_bytes()), &ctx(&pk, None, 2_000)).is_ok());
        let hex: String = sig.iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify(
            &bytes,
            Some(format!("{hex}\n").as_bytes()),
            &ctx(&pk, None, 2_000)
        )
        .is_ok());
        // unsigned
        assert_eq!(
            verify(&bytes, None, &ctx(&pk, None, 2_000)),
            Err(ArmSetError::Unsigned)
        );
        // tampered byte
        let mut t = bytes.clone();
        let i = t.iter().position(|&b| b == b'5').unwrap();
        t[i] = b'6';
        assert_eq!(
            verify(&t, Some(&sig), &ctx(&pk, None, 2_000)),
            Err(ArmSetError::BadSignature)
        );
        // wrong key
        let other = Pubkey::new_from_array(
            SigningKey::from_bytes(&[8u8; 32])
                .verifying_key()
                .to_bytes(),
        );
        assert_eq!(
            verify(&bytes, Some(&sig), &ctx(&other, None, 2_000)),
            Err(ArmSetError::BadSignature)
        );
        // sha in name mismatch
        assert!(matches!(
            verify(&bytes, Some(&sig), &ctx(&pk, Some(&"0".repeat(64)), 2_000)),
            Err(ArmSetError::ShaMismatch { .. })
        ));
        // stale
        assert!(matches!(
            verify(&bytes, Some(&sig), &ctx(&pk, None, 1_000 + 21_601)),
            Err(ArmSetError::Stale { .. })
        ));
        // wrong regime
        let mut c = ctx(&pk, None, 2_000);
        c.regime = Some("alpenglow");
        assert!(matches!(
            verify(&bytes, Some(&sig), &c),
            Err(ArmSetError::WrongRegime { .. })
        ));
        // hot too old
        let mut c = ctx(&pk, None, 2_000);
        c.hot_version = "0.0.9";
        assert!(matches!(
            verify(&bytes, Some(&sig), &c),
            Err(ArmSetError::TooNewForHot { .. })
        ));
        // garbage signature encodings
        assert_eq!(
            verify(&bytes, Some(b"not a signature"), &ctx(&pk, None, 2_000)),
            Err(ArmSetError::BadSignatureEncoding)
        );
    }

    #[test]
    fn gate_false_is_verified_but_not_passed() {
        let sk = SigningKey::from_bytes(&[9u8; 32]);
        let pk = Pubkey::new_from_array(sk.verifying_key().to_bytes());
        let bytes = sample(false, 1_000);
        let v = verify(
            &bytes,
            Some(&sk.sign(&bytes).to_bytes()),
            &ctx(&pk, None, 1_500),
        )
        .unwrap();
        assert_eq!(v.gate(), ArmSetGate::Verified { gate_passed: false });
    }

    #[test]
    fn filenames() {
        let sha = "a".repeat(64);
        assert_eq!(
            parse_filename(&format!("/var/lib/rugsnipe/armsets/armset-57-{sha}.json")),
            Some((57, sha.clone()))
        );
        assert_eq!(parse_filename("armset-57-abc.json"), None);
        assert_eq!(parse_filename(&format!("armset-x-{sha}.json")), None);
        assert_eq!(parse_filename(&format!("armset-1-{sha}.txt")), None);
    }
}
