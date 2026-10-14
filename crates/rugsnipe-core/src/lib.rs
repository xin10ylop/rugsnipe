//! # rugsnipe-core
//!
//! Shared, mostly pure building blocks of the hot box (docs/06 sections 5.3,
//! 6, 7.3, 13):
//!
//! | module | what |
//! |---|---|
//! | [`mode`] | `Mode` (paper is the default and the only mode that works without keys) |
//! | [`config`] | the TOML of docs/06 section 6 with serde, and `validate()` returning **every** refusal reason |
//! | [`bid`] | per-leader-class bid split, the failed-attempt cap and the cost of a failed attempt (pay on success) |
//! | [`armset`] | ArmSet JSON, sha256-in-filename and detached ed25519 signature verification |
//! | [`watchlist`] | ArcSwap snapshot (ArmSet) + dynamic entries (TTL, remove-if-no-launch, keep, last-transfer successor) |
//! | [`risk`] | `RiskGate`: caps, per-op consecutive-loss latch, kill latch, `decide()` on atomics |
//! | [`rslog`] | binary record log: compact header, `Rec` types, SPSC ring (rtrb) + writer thread, decoder |
//! | [`kill`] | kill-file poller (one `stat` per poll) |
//! | [`intent`] | `IntentId = xxh3(mint ‖ wallet)` |
//!
//! I/O is confined to [`rslog::RslogWriter`] (its own thread) and
//! [`kill::KillFile::poll`]; everything else takes bytes/values in and out.

pub mod armset;
pub mod bid;
pub mod config;
pub mod intent;
pub mod kill;
pub mod mode;
pub mod risk;
pub mod rslog;
pub mod watchlist;

pub use config::{Config, Refusal, ValidateEnv, Validation};
pub use intent::IntentId;
pub use mode::Mode;
pub use pump_codec::Pubkey;

/// Serde helpers for base58 pubkeys.
pub mod b58 {
    use pump_codec::Pubkey;
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(k: &Pubkey, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&k.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Pubkey, D::Error> {
        let s = String::deserialize(d)?;
        s.parse()
            .map_err(|_| D::Error::custom(format!("bad base58 pubkey {s:?}")))
    }

    /// `Vec<Pubkey>` as base58 strings.
    pub mod vec {
        use super::*;
        use serde::ser::SerializeSeq;
        pub fn serialize<S: Serializer>(v: &[Pubkey], s: S) -> Result<S::Ok, S::Error> {
            let mut q = s.serialize_seq(Some(v.len()))?;
            for k in v {
                q.serialize_element(&k.to_string())?;
            }
            q.end()
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Pubkey>, D::Error> {
            let v = Vec::<String>::deserialize(d)?;
            v.iter()
                .map(|s| {
                    s.parse()
                        .map_err(|_| D::Error::custom(format!("bad base58 pubkey {s:?}")))
                })
                .collect()
        }
    }
}
