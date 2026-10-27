//! Sending: the hot unit's configured HTTP senders (Nozomi, Helius, Jito
//! ...) built from its `[senders]` tables, plus `sendTransaction` on our
//! RPC. Each sender wants its own tip account, so each gets its own signed
//! copy; a buy goes to one sender only (config), a sell to several (a
//! second copy finds no tokens and fails for its fee).

use pump_codec::Pubkey;
use rugsnipe_core::config::Config;
use rugsnipe_tx::senders::{providers, ApiKeys, SendResult, Sender, Variant, VariantKind};

pub struct Out {
    senders: Vec<(String, Box<dyn Sender>)>,
}

/// One signed copy: what it was for and how it went.
#[derive(Debug, Clone)]
pub struct Sent {
    pub label: String,
    pub sig: String,
    pub ok: bool,
    pub why: String,
}

impl Out {
    /// The enabled HTTP senders of the hot config, named in `want`
    /// (empty = all enabled).
    pub fn build(hot: &Config, keys: &ApiKeys, want: &[String]) -> Result<Self, String> {
        let mut senders = vec![];
        for (name, c) in hot.senders.http() {
            if !c.enabled || (!want.is_empty() && !want.iter().any(|w| w == name)) {
                continue;
            }
            let mut c = c.clone();
            c.key = hot.senders.key_of(name, &c).to_string();
            let kind = if name == "helius_swqos" {
                c.swqos_only = true;
                "helius"
            } else {
                name
            };
            // a bundle path drops a failing transaction and wants the bid as
            // its priority fee: not what these sends are shaped for
            if name == "nozomi_bundle" {
                continue;
            }
            let s = providers::build(kind, &c, keys).map_err(|e| format!("sender {name}: {e}"))?;
            senders.push((name.to_string(), s));
        }
        for w in want {
            if !senders.iter().any(|(n, _)| n == w) {
                return Err(format!("sender {w}: not enabled in the hot config"));
            }
        }
        Ok(Self { senders })
    }

    pub fn labels(&self) -> Vec<&str> {
        self.senders.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// The tip each sender needs: its first tip account and its minimum,
    /// raised to `tip` (`None` for a sender without tips).
    pub fn tips(&self, tip: u64) -> Vec<Option<(Pubkey, u64)>> {
        self.senders
            .iter()
            .map(|(_, s)| s.tip_accounts().first().map(|a| (*a, tip.max(s.min_tip()))))
            .collect()
    }

    /// Send `txs[i]` (bytes, signature) through sender `i`, in parallel.
    pub fn send_each(
        &self,
        kind: VariantKind,
        txs: &[(Vec<u8>, [u8; 64])],
        tips: &[Option<(Pubkey, u64)>],
    ) -> Vec<Sent> {
        std::thread::scope(|sc| {
            let hs: Vec<_> = self
                .senders
                .iter()
                .zip(txs)
                .zip(tips)
                .map(|(((name, s), (bytes, sig)), tip)| {
                    sc.spawn(move || {
                        let v = Variant::from_bytes(
                            bytes,
                            *sig,
                            0,
                            0,
                            kind,
                            0,
                            tip.map_or(0, |t| t.1),
                            tip.map(|t| t.0),
                        );
                        let r = s.send(&v);
                        Sent {
                            label: name.clone(),
                            sig: v.signature_b58(),
                            ok: r.is_ok(),
                            why: match r {
                                SendResult::Failed(e) => format!("{e:?}"),
                                _ => String::new(),
                            },
                        }
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        })
    }
}
