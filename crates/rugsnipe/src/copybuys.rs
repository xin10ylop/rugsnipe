//! The copied wallet's launch buys per create signer, kept across restarts
//! (`entry.copy_signer_min_buys`, docs/17 section 9). The hot loop counts
//! them live in a fixed table that starts empty, and the operator refresh
//! restarts the hot service every four hours: over Oct 21-25 a count started
//! empty at each of those restarts loses 127 of the 226 copies (+1.81 of
//! +4.22 SOL, `analysis/research/2026-10-25/five/copy/restart.py`). So the
//! hot loop hands each buy it counts to the cold thread
//! ([`crate::msgs::ColdMsg::HisBuy`]), which keeps them here and saves them
//! every minute and at shutdown (`copy_signers.json` in the state
//! directory); the next start seeds the hot loop's table from the file
//! ([`crate::hot::HotLoop::seed_his_signers`]).

use pump_codec::Pubkey;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// His buys kept per signer (the hot loop's table keeps as many).
pub const PER_SIGNER: usize = 8;

/// His last landed launch buys per create signer: their slots, oldest first.
#[derive(Debug, Clone, Default)]
pub struct SignerBuys {
    m: HashMap<Pubkey, Vec<u64>>,
    dirty: bool,
}

impl SignerBuys {
    /// His buy of `signer`'s launch in `slot`; the oldest of more than
    /// [`PER_SIGNER`] is dropped.
    pub fn note(&mut self, signer: Pubkey, slot: u64) {
        let v = self.m.entry(signer).or_default();
        v.push(slot);
        if v.len() > 1 && v[v.len() - 2] > slot {
            v.sort_unstable();
        }
        if v.len() > PER_SIGNER {
            v.remove(0);
        }
        self.dirty = true;
    }

    /// Drop the signers with no buy in the `window` slots up to his newest.
    pub fn prune(&mut self, window: u64) {
        let since = self.newest().saturating_sub(window);
        let n = self.m.len();
        self.m.retain(|_, v| v.last().is_some_and(|&s| s >= since));
        self.dirty |= self.m.len() != n;
    }

    /// His newest buy's slot (0: none).
    pub fn newest(&self) -> u64 {
        self.m
            .values()
            .filter_map(|v| v.last())
            .copied()
            .max()
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.m.len()
    }

    pub fn is_empty(&self) -> bool {
        self.m.is_empty()
    }

    /// Each signer and his buys of its launches (slots, oldest first).
    pub fn iter(&self) -> impl Iterator<Item = (&Pubkey, &[u64])> {
        self.m.iter().map(|(k, v)| (k, v.as_slice()))
    }

    /// `{"signer": [slot, ...]}`, oldest first.
    pub fn to_json(&self) -> String {
        let m: BTreeMap<String, &Vec<u64>> =
            self.m.iter().map(|(k, v)| (k.to_string(), v)).collect();
        serde_json::to_string(&m).unwrap_or_else(|_| "{}".into())
    }

    /// Load what [`SignerBuys::save`] wrote (a missing file is an empty
    /// start). Returns the signers loaded.
    pub fn load(&mut self, path: &Path) -> Result<usize, String> {
        let b = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let m: BTreeMap<String, Vec<u64>> =
            serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", path.display()))?;
        for (k, mut v) in m {
            let Ok(signer) = k.parse::<Pubkey>() else {
                continue;
            };
            v.sort_unstable();
            v.drain(..v.len().saturating_sub(PER_SIGNER));
            if !v.is_empty() {
                self.m.insert(signer, v);
            }
        }
        Ok(self.m.len())
    }

    /// Write the buys if any changed since the last save (temporary file,
    /// then rename).
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(b: u8) -> Pubkey {
        Pubkey::new_from_array([b; 32])
    }

    #[test]
    fn his_last_buys_per_signer_are_kept_oldest_first() {
        let mut b = SignerBuys::default();
        for s in 0..10 {
            b.note(k(1), 100 + s);
        }
        b.note(k(2), 50);
        b.note(k(1), 105);
        let v: Vec<u64> = b.iter().find(|(s, _)| **s == k(1)).unwrap().1.to_vec();
        assert_eq!(v.len(), PER_SIGNER);
        assert!(v.windows(2).all(|w| w[0] <= w[1]), "{v:?}");
        assert_eq!(*v.last().unwrap(), 109);
        assert_eq!(b.newest(), 109);
        // a signer he has not bought within the window of his newest buy goes
        b.prune(30);
        assert_eq!(b.len(), 1);
        assert!(b.iter().all(|(s, _)| *s == k(1)));
    }

    #[test]
    fn the_buys_survive_a_save_and_a_load() {
        let dir = std::env::temp_dir().join(format!("copybuys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("copy_signers.json");
        let mut b = SignerBuys::default();
        assert_eq!(b.load(&p).unwrap(), 0, "no file: an empty start");
        b.note(k(1), 7);
        b.note(k(1), 9);
        b.note(k(3), 8);
        b.save(&p).unwrap();
        let mut c = SignerBuys::default();
        assert_eq!(c.load(&p).unwrap(), 2);
        let mut got: Vec<(Pubkey, Vec<u64>)> = c.iter().map(|(s, v)| (*s, v.to_vec())).collect();
        got.sort();
        let mut want = vec![(k(1), vec![7, 9]), (k(3), vec![8])];
        want.sort();
        assert_eq!(got, want);
        // nothing new: no write
        std::fs::remove_file(&p).unwrap();
        c.save(&p).unwrap();
        assert!(!p.exists());
        // an unreadable file is an error, not an empty start
        std::fs::write(&p, "not json").unwrap();
        assert!(SignerBuys::default().load(&p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
