//! `rugsnipe-guard` (docs/06 section 8 "Guard"), skeleton: it watches the
//! `exits` lease in `state.db`. While the hot box's exit loop renews it the
//! guard idles; if the heartbeat is older than `takeover_after_ms` **and** a
//! position is open, it takes the lease over by compare-and-set (`epoch + 1`)
//! and sells on a >= `drop_from_peak_bps` drop from the peak price (price ∝
//! V², so `(v / peak_v)^2`), or after `grace_s` of holding the lease.
//!
//! `dry_run` (the default) only reports what it would sell; otherwise each
//! sell is patched into the final-sell template (dump floor bounds, the
//! dump bid split once by `BidPolicy::plan_sell`), signed and recorded by the
//! paper sender. In probe mode with [`GuardReal`] (`rugsnipe-guard
//! --real-sell`) the same sell goes on a recent blockhash to every real
//! sender, re-targeted per sender like the hot loop's
//! ([`crate::sender::plan_variants`]: the provider's tip account and
//! minimum tip), signed with the probe key.

use pump_codec::constants::{TOKEN_2022_PROGRAM_ID, WSOL_MINT};
use pump_codec::curve::FeeBps;
use pump_codec::{pda, Pubkey};
use rugsnipe_exit::bounds::{dump_min_out, Market};
use rugsnipe_exit::{ExitPolicy, Venue};
use rugsnipe_state::lease::{Acquire, Lease};
use rugsnipe_state::recovery::load_open_positions;
use rugsnipe_tx::senders::paper::Paper;
use rugsnipe_tx::senders::{Sender as _, Variant, VariantKind};
use rugsnipe_tx::signer::HotSigner;
use rugsnipe_tx::template::{MintKeys, Patch, Template};
use std::collections::HashSet;
use std::sync::Arc;

/// Guard settings (`[guard]` plus the process identity).
#[derive(Debug, Clone)]
pub struct GuardCfg {
    pub owner: String,
    pub takeover_after_ms: u64,
    pub drop_from_peak_bps: u64,
    pub grace_s: u64,
    pub dry_run: bool,
}

/// What one guard step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardAction {
    /// Nothing to guard.
    Idle(&'static str),
    /// The hot box holds a fresh lease.
    HotAlive {
        owner: String,
        age_ms: u64,
    },
    TookOver {
        epoch: u64,
        from: Option<String>,
    },
    WouldSell {
        mint: Pubkey,
        tokens: u64,
        reason: &'static str,
    },
    Sold {
        mint: Pubkey,
        tokens: u64,
        reason: &'static str,
        sig: [u8; 64],
    },
    /// Real sell variants handed to the senders (`accepted` of `sent`).
    SentReal {
        mint: Pubkey,
        tokens: u64,
        reason: &'static str,
        sent: usize,
        accepted: usize,
    },
    /// A real sell could not be built (no recent blockhash): retried next step.
    SellDeferred {
        mint: Pubkey,
        why: &'static str,
    },
    Released,
}

/// The paper sell path of a non-dry-run guard.
pub struct PaperSell {
    pub template: Template,
    pub signer: Arc<HotSigner>,
    pub paper: Arc<Paper>,
    pub tip_account: Pubkey,
    /// `[bid]`: the dump bid is split by leader class like every sell
    /// (the guard knows no leader: planned as priority-ordered).
    pub bid: rugsnipe_core::bid::BidPolicy,
    /// Probe: send for real (else paper only).
    pub real: Option<GuardReal>,
}

/// The real sell path of the guard (probe): the gated sender set and a
/// source of recent blockhashes.
pub struct GuardReal {
    pub senders: Arc<rugsnipe_tx::senders::SenderSet>,
    pub blockhash: Arc<dyn Fn() -> Option<[u8; 32]> + Send + Sync>,
}

impl core::fmt::Debug for GuardReal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GuardReal")
            .field("senders", &self.senders)
            .finish_non_exhaustive()
    }
}

impl core::fmt::Debug for PaperSell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaperSell")
            .field("signer", &self.signer)
            .finish_non_exhaustive()
    }
}

/// The guard state machine.
#[derive(Debug)]
pub struct Guard {
    cfg: GuardCfg,
    lease: Lease,
    held_since_ms: Option<u64>,
    sell: Option<PaperSell>,
    sold: HashSet<Pubkey>,
    policy: ExitPolicy,
}

impl Guard {
    pub fn new(cfg: GuardCfg, sell: Option<PaperSell>) -> Self {
        let lease = Lease::new("exits", &cfg.owner);
        Self {
            cfg,
            lease,
            held_since_ms: None,
            sell,
            sold: HashSet::new(),
            policy: ExitPolicy::default(),
        }
    }

    pub fn holds_lease(&self) -> bool {
        self.lease.is_held()
    }

    /// One iteration. `price(mint)` = current virtual SOL of the curve, if known.
    pub fn step(
        &mut self,
        c: &rusqlite::Connection,
        now_ms: u64,
        price: &dyn Fn(&Pubkey) -> Option<u64>,
    ) -> Result<Vec<GuardAction>, String> {
        let positions = load_open_positions(c).map_err(|e| e.to_string())?;
        let mut out = vec![];
        if positions.is_empty() {
            if self.lease.is_held() {
                self.lease.release(c).map_err(|e| e.to_string())?;
                self.held_since_ms = None;
                out.push(GuardAction::Released);
            } else {
                out.push(GuardAction::Idle("no open positions"));
            }
            return Ok(out);
        }
        if self.lease.is_held() && !self.lease.renew(c, now_ms).map_err(|e| e.to_string())? {
            self.held_since_ms = None;
        }
        if !self.lease.is_held() {
            match self
                .lease
                .acquire(c, now_ms, self.cfg.takeover_after_ms)
                .map_err(|e| e.to_string())?
            {
                Acquire::Held {
                    epoch,
                    took_over_from,
                } => {
                    self.held_since_ms = Some(now_ms);
                    out.push(GuardAction::TookOver {
                        epoch,
                        from: took_over_from,
                    });
                }
                Acquire::Busy(row) => {
                    out.push(GuardAction::HotAlive {
                        age_ms: row.age_ms(now_ms),
                        owner: row.owner,
                    });
                    return Ok(out);
                }
            }
        }
        let held_ms = now_ms.saturating_sub(self.held_since_ms.unwrap_or(now_ms));
        for p in positions {
            if self.sold.contains(&p.mint) || p.tokens == 0 {
                continue;
            }
            let v = price(&p.mint);
            let dropped = match v {
                Some(v) if p.peak_v > 0 => {
                    let m =
                        (v as u128 * v as u128 * 10_000) / (p.peak_v as u128 * p.peak_v as u128);
                    m <= 10_000u128.saturating_sub(self.cfg.drop_from_peak_bps as u128)
                }
                _ => false,
            };
            let reason = if dropped {
                "drop_from_peak"
            } else if held_ms >= self.cfg.grace_s * 1_000 {
                "grace"
            } else {
                continue;
            };
            if self.cfg.dry_run || self.sell.is_none() {
                self.sold.insert(p.mint);
                out.push(GuardAction::WouldSell {
                    mint: p.mint,
                    tokens: p.tokens,
                    reason,
                });
                continue;
            }
            let s = self.sell.as_mut().expect("checked");
            let lifetime = match &s.real {
                Some(r) => match (r.blockhash)() {
                    Some(h) => h,
                    None => {
                        out.push(GuardAction::SellDeferred {
                            mint: p.mint,
                            why: "no recent blockhash",
                        });
                        continue;
                    }
                },
                None => [0x22; 32],
            };
            self.sold.insert(p.mint);
            let owner = s.signer.pubkey();
            let bc = pda::bonding_curve(&p.mint);
            let keys = MintKeys::curve(
                p.mint,
                bc,
                pda::associated_bonding_curve(&p.mint, &TOKEN_2022_PROGRAM_ID),
                pda::ata_spl(&bc, &WSOL_MINT),
                pda::ata_token_2022(&owner, &p.mint),
            );
            let market = Market {
                venue: Venue::Curve,
                v: v.unwrap_or(p.v_fill),
                t: 0,
                creator: Pubkey::default(),
                fees: FeeBps::DEFAULT,
            };
            let min_out = dump_min_out(&self.policy, &market, p.tokens);
            // the bid once, through the bid policy (never as both fee and tip)
            let plan = s.bid.plan_sell(
                None,
                self.policy.dump_tip_lamports,
                s.template.table.n_sigs.clamp(1, 2) as u8,
            );
            s.template.patch(&Patch {
                keys: &keys,
                lifetime: &lifetime,
                amount: p.tokens,
                limit: min_out,
                priority_lamports: plan.priority_lamports,
                tip_lamports: plan.tip_lamports,
                cu_limit: None,
            });
            s.template.sign(&s.signer, None);
            let var = Variant::from_template(
                &s.template,
                p.intent_id as u64,
                0xC0,
                VariantKind::Sell,
                plan.priority_lamports,
                plan.tip_lamports,
                Some(s.tip_account),
            );
            let _ = s.paper.send(&var);
            if let Some(r) = &s.real {
                let job = crate::msgs::FireJob {
                    tmpl: s.template.clone(),
                    intent: p.intent_id as u64,
                    idx: 0xC0,
                    prio: plan.priority_lamports,
                    tip: plan.tip_lamports,
                    tip_account: s.tip_account,
                    bid: self.policy.dump_tip_lamports,
                    real: true,
                };
                let targets = r.senders.real();
                let vs =
                    crate::sender::plan_variants(std::slice::from_ref(&job), targets, &s.signer);
                let sent = vs.len();
                let accepted = vs
                    .iter()
                    .filter(|(k, v)| targets[*k].send(v).is_ok())
                    .count();
                out.push(GuardAction::SentReal {
                    mint: p.mint,
                    tokens: p.tokens,
                    reason,
                    sent,
                    accepted,
                });
                continue;
            }
            out.push(GuardAction::Sold {
                mint: p.mint,
                tokens: p.tokens,
                reason,
                sig: var.sig,
            });
        }
        Ok(out)
    }
}
