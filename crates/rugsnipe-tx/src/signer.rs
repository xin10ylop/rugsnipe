//! Hot-wallet signer. The keypair lives only in zeroizing memory
//! (`ed25519_dalek::SigningKey` zeroizes on drop; the file buffer and the
//! parsed 64-byte array are `Zeroizing`). `Debug` prints the public key
//! only; there is no `Clone`, `Display` or accessor for the secret.

use ed25519_dalek::{Signer as _, SigningKey};
use pump_codec::Pubkey;
use zeroize::Zeroizing;

/// Why a keypair was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// Not a JSON array of 64 bytes.
    Format,
    /// Bytes 32..64 are not the public key of bytes 0..32.
    PubkeyMismatch,
    /// The file is group- or world-accessible (mode shown in octal).
    Permissions(u32),
    Io(String),
}

impl core::fmt::Display for KeyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Format => write!(f, "keypair is not a JSON array of 64 bytes"),
            Self::PubkeyMismatch => write!(f, "keypair public half does not match its secret"),
            Self::Permissions(m) => write!(f, "keypair file mode {m:o} is group/world-accessible"),
            Self::Io(e) => write!(f, "keypair read failed: {e}"),
        }
    }
}
impl std::error::Error for KeyError {}

pub struct HotSigner {
    key: SigningKey,
    pubkey: Pubkey,
}

impl core::fmt::Debug for HotSigner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HotSigner")
            .field("pubkey", &self.pubkey.to_string())
            .finish_non_exhaustive()
    }
}

impl HotSigner {
    /// From the Solana CLI layout: 32-byte secret ‖ 32-byte public key.
    pub fn from_keypair_bytes(bytes: &Zeroizing<[u8; 64]>) -> Result<Self, KeyError> {
        let mut secret = Zeroizing::new([0u8; 32]);
        secret.copy_from_slice(&bytes[..32]);
        let key = SigningKey::from_bytes(&secret);
        let pubkey = Pubkey::new_from_array(key.verifying_key().to_bytes());
        if pubkey.as_ref() != &bytes[32..] {
            return Err(KeyError::PubkeyMismatch);
        }
        Ok(Self { key, pubkey })
    }

    /// From a 32-byte seed (tests, derived keys).
    pub fn from_seed(seed: &Zeroizing<[u8; 32]>) -> Self {
        let key = SigningKey::from_bytes(seed);
        let pubkey = Pubkey::new_from_array(key.verifying_key().to_bytes());
        Self { key, pubkey }
    }

    /// Parse a `[u8; 64]` JSON array (solana-keygen file content).
    pub fn from_keypair_json(text: &Zeroizing<String>) -> Result<Self, KeyError> {
        let t = text.trim();
        let inner = t
            .strip_prefix('[')
            .and_then(|r| r.strip_suffix(']'))
            .ok_or(KeyError::Format)?;
        let mut bytes = Zeroizing::new([0u8; 64]);
        let mut n = 0usize;
        for part in inner.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if n >= 64 {
                return Err(KeyError::Format);
            }
            bytes[n] = part.parse::<u8>().map_err(|_| KeyError::Format)?;
            n += 1;
        }
        if n != 64 {
            return Err(KeyError::Format);
        }
        Self::from_keypair_bytes(&bytes)
    }

    /// Load a keypair file (systemd credential). Refuses world-accessible files
    /// and group-writable or group-executable ones; group-read is accepted only
    /// when the file's group is the process's own effective group, which is how
    /// systemd hands credentials to a service (`LoadCredential=` files are
    /// mode 0440, owned by the service user and its primary group).
    pub fn load_file(path: &std::path::Path) -> Result<Self, KeyError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let m = std::fs::metadata(path).map_err(|e| KeyError::Io(e.to_string()))?;
            let mode = m.permissions().mode() & 0o7777;
            if !mode_private(mode, m.gid()) {
                return Err(KeyError::Permissions(mode));
            }
        }
        let raw = Zeroizing::new(std::fs::read(path).map_err(|e| KeyError::Io(e.to_string()))?);
        let text = Zeroizing::new(String::from_utf8(raw.to_vec()).map_err(|_| KeyError::Format)?);
        Self::from_keypair_json(&text)
    }

    pub fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    /// Ed25519 signature over `message`.
    #[inline]
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }
}

/// Verify a signature (tests, `ctl`).
pub fn verify(pubkey: &Pubkey, message: &[u8], sig: &[u8; 64]) -> bool {
    match ed25519_dalek::VerifyingKey::from_bytes(&pubkey.to_bytes()) {
        Ok(vk) => vk
            .verify_strict(message, &ed25519_dalek::Signature::from_bytes(sig))
            .is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_for(seed: [u8; 32]) -> Zeroizing<String> {
        let s = HotSigner::from_seed(&Zeroizing::new(seed));
        let mut all = seed.to_vec();
        all.extend_from_slice(s.pubkey().as_ref());
        Zeroizing::new(format!(
            "[{}]",
            all.iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ))
    }

    #[test]
    fn json_round_trip_sign_verify_and_debug_hides_secret() {
        let seed = [42u8; 32];
        let s = HotSigner::from_keypair_json(&json_for(seed)).unwrap();
        let sig = s.sign(b"hello");
        assert!(verify(&s.pubkey(), b"hello", &sig));
        assert!(!verify(&s.pubkey(), b"hellO", &sig));
        let dbg = format!("{s:?}");
        assert!(dbg.contains(&s.pubkey().to_string()));
        assert!(!dbg.contains("42, 42"), "{dbg}");
        assert!(!dbg.contains("[42"), "{dbg}");
    }

    #[test]
    fn rejects_bad_keypairs() {
        assert_eq!(
            HotSigner::from_keypair_json(&Zeroizing::new("[1,2,3]".into())).unwrap_err(),
            KeyError::Format
        );
        assert_eq!(
            HotSigner::from_keypair_json(&Zeroizing::new("nope".into())).unwrap_err(),
            KeyError::Format
        );
        let mut t = json_for([1u8; 32]).to_string();
        // corrupt the last public-key byte
        let cut = t.rfind(',').unwrap();
        t.replace_range(cut + 1..t.len() - 1, "0");
        assert!(matches!(
            HotSigner::from_keypair_json(&Zeroizing::new(t)),
            Err(KeyError::PubkeyMismatch) | Err(KeyError::Format)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn file_permissions_enforced() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("rugsnipe-key-{}.json", std::process::id()));
        std::fs::write(&p, json_for([3u8; 32]).as_bytes()).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            HotSigner::load_file(&p).unwrap_err(),
            KeyError::Permissions(0o644)
        );
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o400)).unwrap();
        assert!(HotSigner::load_file(&p).is_ok());
        let _ = std::fs::remove_file(&p);
    }
}

/// `true` when a credential's mode keeps it private: no world bits, no group
/// write/execute, and group-read only if `file_gid` is the process's own
/// effective group (systemd credentials are 0440 owned by the service's group).
#[cfg(unix)]
pub fn mode_private(mode: u32, file_gid: u32) -> bool {
    if mode & 0o007 != 0 || mode & 0o030 != 0 {
        return false;
    }
    if mode & 0o040 != 0 {
        // SAFETY: getegid has no preconditions and cannot fail.
        let egid = unsafe { libc::getegid() };
        // group root (gid 0) can read anything regardless: systemd >= 256 hands
        // credentials over as root:root 0440 plus an ACL for the service user
        return file_gid == egid || file_gid == 0;
    }
    true
}

#[cfg(not(unix))]
pub fn mode_private(_mode: u32, _file_gid: u32) -> bool {
    true
}

#[cfg(all(test, unix))]
mod mode_tests {
    use super::mode_private;

    #[test]
    fn systemd_credential_modes_are_private_only_for_our_own_group() {
        // SAFETY: getegid has no preconditions.
        let mine = unsafe { libc::getegid() };
        let other = mine.wrapping_add(12345).max(1);
        assert!(mode_private(0o400, other));
        assert!(mode_private(0o600, other));
        assert!(mode_private(0o440, mine), "systemd LoadCredential= files");
        assert!(!mode_private(0o440, other), "another group may read it");
        assert!(
            mode_private(0o440, 0),
            "root:root 0440 + ACL is systemd >= 256"
        );
        assert!(!mode_private(0o460, mine), "group write");
        assert!(!mode_private(0o444, mine), "world read");
        assert!(!mode_private(0o404, mine));
        assert!(!mode_private(0o450, mine), "group execute");
    }
}
