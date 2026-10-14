//! `IntentId = xxh3_64(mint ‖ wallet)`: the same id on every box and after
//! every restart, so the ledger's `UNIQUE(mint, mode)` and the rslog join.

use pump_codec::Pubkey;
use xxhash_rust::xxh3::xxh3_64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IntentId(pub u64);

impl IntentId {
    pub fn new(mint: &Pubkey, wallet: &Pubkey) -> Self {
        let mut b = [0u8; 64];
        b[..32].copy_from_slice(mint.as_ref());
        b[32..].copy_from_slice(wallet.as_ref());
        Self(xxh3_64(&b))
    }
    /// As stored in SQLite (`INTEGER`, signed 64-bit).
    pub fn as_i64(self) -> i64 {
        self.0 as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_and_order_sensitive() {
        let (m, w) = (Pubkey::new_unique(), Pubkey::new_unique());
        assert_eq!(IntentId::new(&m, &w), IntentId::new(&m, &w));
        assert_ne!(IntentId::new(&m, &w), IntentId::new(&w, &m));
        // pinned vector: xxh3_64 of 64 zero bytes
        let z = Pubkey::default();
        assert_eq!(IntentId::new(&z, &z).0, xxh3_64(&[0u8; 64]));
    }
}
