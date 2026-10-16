//! Sender thread: takes [`FireJob`]s from the hot loop (secondary variants,
//! and every variant in a real mode), re-prices, re-signs and hands them to
//! the gated [`SenderSet`], then records `Sent` in its own rslog stream and
//! in the ledger.
//!
//! * A job not marked `real` (paper mode, or a decision forced to paper in a
//!   real mode) goes to the paper recorder only.
//! * A `real` job is recorded once in the paper recorder (the bid plan as
//!   the hot loop signed it) and then **re-targeted per real sender**
//!   ([`plan_variants`]): the provider's tip account (rotated, unique inside
//!   the intent), `tip = max(plan tip, provider minimum)`, the plan's
//!   priority fee, signed again with the real key; every variant rides the
//!   intent's one durable nonce, so at most one executes. Jito takes only
//!   the pay-on-success primary of a buy (1 req/s per IP and region).
//! * Each real sender has its own worker thread (warm connection, breaker,
//!   token bucket), so one slow provider never delays the others; a Nozomi
//!   worker sends both variants of an intent in one batch request.
//! * In a `--probe-dry` run [`SenderSet::real`] is empty: the per-sender
//!   variants are signed exactly as in a real run and recorded, nothing
//!   leaves the box.

use crate::msgs::{ColdMsg, FireJob};
use crate::stats::HotStats;
use rugsnipe_core::rslog::{Rec, RslogProducer};
use rugsnipe_feed::clock::mono_ns;
use rugsnipe_tx::senders::{
    tip_plan, SendError, SendResult, Sender, SenderId, SenderSet, Variant, VariantKind,
};
use rugsnipe_tx::signer::HotSigner;
use std::sync::atomic::{AtomicBool, Ordering::Acquire};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Kind byte of a variant index (`0x80` = sell).
pub fn kind_of(idx: u8) -> VariantKind {
    if idx & 0x80 != 0 {
        VariantKind::Sell
    } else {
        VariantKind::Buy
    }
}

/// `buy` / `sell` / `burn`.
pub fn kind_name(k: VariantKind) -> &'static str {
    match k {
        VariantKind::Buy => "buy",
        VariantKind::Sell => "sell",
        VariantKind::Burn => "burn",
    }
}

/// A short, key-free name of a send failure (ledger `sends.err`).
pub fn err_name(r: &SendResult) -> &'static str {
    match r {
        SendResult::Recorded | SendResult::Accepted { .. } => "",
        SendResult::Failed(e) => match e {
            SendError::BreakerOpen => "breaker_open",
            SendError::RateLimited => "rate_limited",
            SendError::TooLarge => "too_large",
            SendError::TipTooLow { .. } => "tip_too_low",
            SendError::WrongTipAccount => "wrong_tip_account",
            SendError::Http(_) => "http",
            SendError::Io(_) => "io",
            SendError::Rpc(_) => "rpc_error",
            SendError::NotForThisSender => "not_for_this_sender",
        },
    }
}

/// The variants of `jobs` (one intent's jobs, in order) for each target
/// sender: `(target index, variant)`. Tip account and tip per sender
/// ([`tip_plan`]), re-picked until every `(account, tip, priority)` of the
/// intent is unique, so every variant has its own signature.
pub fn plan_variants(
    jobs: &[FireJob],
    targets: &[Box<dyn Sender>],
    signer: &HotSigner,
) -> Vec<(usize, Variant)> {
    let mut out = vec![];
    let mut used: Vec<([u8; 32], u64, u64)> = vec![];
    for job in jobs {
        let kind = kind_of(job.idx);
        for (k, s) in targets.iter().enumerate() {
            if kind == VariantKind::Buy && (job.idx & 0x7f) >= s.buy_variants() {
                continue;
            }
            let mut tmpl = job.tmpl.clone();
            tmpl.set_priority(job.prio);
            // the provider's rotated tip account and floor (tip_plan); paths
            // without tip accounts (rpcsend) keep the template's Jito set
            let (accounts, tip, start): (&[pump_codec::Pubkey], u64, usize) =
                match tip_plan(s.as_ref(), k, job.idx, job.intent, job.tip) {
                    Some((a, tip)) => {
                        let accts = s.tip_accounts();
                        (accts, tip, accts.iter().position(|x| *x == a).unwrap_or(0))
                    }
                    None => {
                        let accts = &pump_codec::constants::JITO_TIP_ACCOUNTS[..];
                        let i = job.intent as usize + 3 * k + (job.idx & 0x7f) as usize;
                        (accts, job.tip, i % accts.len())
                    }
                };
            let mut acct = accounts[start % accounts.len()];
            for step in 0..accounts.len() {
                acct = accounts[(start + step) % accounts.len()];
                if !used.contains(&(acct.to_bytes(), tip, job.prio)) {
                    break;
                }
            }
            used.push((acct.to_bytes(), tip, job.prio));
            tmpl.set_tip_account(&acct);
            tmpl.set_tip(tip);
            tmpl.sign(signer, None);
            out.push((
                k,
                Variant::from_template(&tmpl, job.intent, job.idx, kind, job.prio, tip, Some(acct)),
            ));
        }
    }
    out
}

/// One send result on its way back to the main sender thread.
struct Done {
    intent: u64,
    variant: Variant,
    id: SenderId,
    label: &'static str,
    bid: u64,
    result: SendResult,
    t0: u64,
}

/// Work for one sender's worker: the variants of one intent.
struct Batch {
    variants: Vec<Variant>,
    bid: u64,
    t0: u64,
}

fn spawn_worker(
    s: &dyn Sender,
    k: usize,
    senders: Arc<SenderSet>,
    done: mpsc::Sender<Done>,
) -> std::io::Result<(mpsc::Sender<Batch>, JoinHandle<()>)> {
    let (tx, rx) = mpsc::channel::<Batch>();
    let name = format!("send-{}", s.label());
    let h = std::thread::Builder::new().name(name).spawn(move || {
        let s = &senders.real()[k];
        let _ = s.warm();
        while let Ok(b) = rx.recv() {
            let refs: Vec<&Variant> = b.variants.iter().collect();
            let results = s.send_batch(&refs);
            for (v, result) in b.variants.iter().zip(results) {
                let _ = done.send(Done {
                    intent: v.intent,
                    variant: v.clone(),
                    id: s.id(),
                    label: s.label(),
                    bid: b.bid,
                    result,
                    t0: b.t0,
                });
            }
        }
    })?;
    Ok((tx, h))
}

/// Start the sender thread. It exits when the ring is empty and either
/// `stop` is set or the hot loop dropped its producer; it returns the number
/// of `(sender, result)` pairs it produced (paper records included).
pub fn spawn(
    mut rx: rtrb::Consumer<FireJob>,
    senders: Arc<SenderSet>,
    signer: Arc<HotSigner>,
    mut rslog: RslogProducer,
    cold: mpsc::Sender<ColdMsg>,
    stats: Arc<HotStats>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<JoinHandle<u64>> {
    std::thread::Builder::new()
        .name("sender".into())
        .spawn(move || {
            let (done_tx, done_rx) = mpsc::channel::<Done>();
            let mut workers = vec![];
            for (k, s) in senders.real().iter().enumerate() {
                match spawn_worker(s.as_ref(), k, senders.clone(), done_tx.clone()) {
                    Ok(w) => workers.push(Some(w)),
                    Err(e) => {
                        eprintln!("sender: worker for {} failed to start: {e}", s.label());
                        workers.push(None);
                    }
                }
            }
            drop(done_tx);
            let mut sent = 0u64;
            let record = |d: Done, sent: &mut u64, rslog: &mut RslogProducer| {
                *sent += 1;
                stats.sends[d.id as usize].inc();
                let now = mono_ns();
                rslog.push(Rec::Sent {
                    intent: d.intent,
                    variant: d.variant.idx,
                    sender: d.id as u8,
                    prio: d.variant.priority_lamports,
                    tip: d.variant.tip_lamports,
                    sig: d.variant.sig,
                    result: d.result.code(),
                    latency_ns: now.saturating_sub(d.t0),
                });
                let http = match &d.result {
                    SendResult::Failed(SendError::Http(s)) => *s,
                    _ => 0,
                };
                let _ = cold.send(ColdMsg::Sent {
                    intent: d.intent,
                    sig: d.variant.sig,
                    variant: d.variant.idx,
                    sender: d.id as u8,
                    label: d.label,
                    prio: d.variant.priority_lamports,
                    tip: d.variant.tip_lamports,
                    bid: d.bid,
                    sent_ns: now,
                    sent_unix_ms: crate::now_ms(),
                    result: d.result.code(),
                    err: err_name(&d.result),
                    http,
                    kind: d.variant.kind as u8,
                });
            };
            let mut pending: Vec<FireJob> = vec![];
            loop {
                while let Ok(job) = rx.pop() {
                    pending.push(job);
                }
                let n = pending.len();
                // one intent's jobs together (the hot loop pushes them back to back)
                while !pending.is_empty() {
                    let intent = pending[0].intent;
                    let end = pending
                        .iter()
                        .position(|j| j.intent != intent)
                        .unwrap_or(pending.len());
                    let jobs: Vec<FireJob> = pending.drain(..end).collect();
                    let t0 = mono_ns();
                    for job in &jobs {
                        // the plan as the hot loop priced it, signed: the paper record
                        let mut tmpl = job.tmpl.clone();
                        tmpl.set_priority(job.prio);
                        tmpl.set_tip(job.tip);
                        tmpl.sign(&signer, None);
                        let v = Variant::from_template(
                            &tmpl,
                            job.intent,
                            job.idx,
                            kind_of(job.idx),
                            job.prio,
                            job.tip,
                            Some(job.tip_account),
                        );
                        let result = senders.paper.send(&v);
                        let d = Done {
                            intent: job.intent,
                            variant: v,
                            id: SenderId::Paper,
                            label: SenderId::Paper.name(),
                            bid: job.bid,
                            result,
                            t0,
                        };
                        if job.real && senders.mode.is_real() && !senders.dry {
                            // real jobs: the paper copy is counted and logged, the
                            // ledger keeps the real sends only
                            sent += 1;
                            stats.sends[SenderId::Paper as usize].inc();
                        } else if !job.real || !senders.mode.is_real() {
                            record(d, &mut sent, &mut rslog);
                        }
                    }
                    let real: Vec<FireJob> = jobs.into_iter().filter(|j| j.real).collect();
                    if real.is_empty() || !senders.mode.is_real() {
                        continue;
                    }
                    let bid = real[0].bid;
                    let planned = plan_variants(&real, senders.targets(), &signer);
                    if senders.dry {
                        // --probe-dry: every per-sender variant is signed and recorded
                        for (k, v) in planned {
                            let s = &senders.targets()[k];
                            let result = senders.paper.send(&v);
                            record(
                                Done {
                                    intent: v.intent,
                                    variant: v,
                                    id: SenderId::Paper,
                                    label: s.label(),
                                    bid,
                                    result,
                                    t0,
                                },
                                &mut sent,
                                &mut rslog,
                            );
                        }
                        continue;
                    }
                    let mut per: Vec<Vec<Variant>> = vec![vec![]; workers.len()];
                    for (k, v) in planned {
                        per[k].push(v);
                    }
                    for (k, vs) in per.into_iter().enumerate() {
                        if vs.is_empty() {
                            continue;
                        }
                        match &workers[k] {
                            Some((tx, _)) => {
                                let _ = tx.send(Batch {
                                    variants: vs,
                                    bid,
                                    t0,
                                });
                            }
                            None => {
                                let s = &senders.real()[k];
                                for v in vs {
                                    let result = SendResult::Failed(SendError::Io(
                                        "worker not running".into(),
                                    ));
                                    record(
                                        Done {
                                            intent: v.intent,
                                            variant: v,
                                            id: s.id(),
                                            label: s.label(),
                                            bid,
                                            result,
                                            t0,
                                        },
                                        &mut sent,
                                        &mut rslog,
                                    );
                                }
                            }
                        }
                    }
                }
                let mut got = 0;
                while let Ok(d) = done_rx.try_recv() {
                    got += 1;
                    record(d, &mut sent, &mut rslog);
                }
                if n == 0 && got == 0 {
                    if stop.load(Acquire) || rx.is_abandoned() {
                        // finish what the workers hold, then stop
                        let handles: Vec<JoinHandle<()>> = workers
                            .drain(..)
                            .flatten()
                            .map(|(tx, h)| {
                                drop(tx);
                                h
                            })
                            .collect();
                        for h in handles {
                            let _ = h.join();
                        }
                        while let Ok(d) = done_rx.try_recv() {
                            record(d, &mut sent, &mut rslog);
                        }
                        return sent;
                    }
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pump_codec::Pubkey;
    use rugsnipe_core::config::Config;
    use rugsnipe_tx::senders::SenderId;

    /// A tip-taking path with its own accounts (no network).
    #[derive(Debug)]
    struct Fake {
        id: SenderId,
        label: &'static str,
        tips: &'static [Pubkey],
        min: u64,
        variants: u8,
    }
    impl Sender for Fake {
        fn id(&self) -> SenderId {
            self.id
        }
        fn send(&self, _v: &Variant) -> SendResult {
            SendResult::Recorded
        }
        fn tip_accounts(&self) -> &[Pubkey] {
            self.tips
        }
        fn min_tip(&self) -> u64 {
            self.min
        }
        fn label(&self) -> &'static str {
            self.label
        }
        fn buy_variants(&self) -> u8 {
            self.variants
        }
    }

    #[test]
    fn one_unique_signed_variant_per_sender_with_its_tip_account_and_floor() {
        // provider tip sets (the real ones need feature `senders`)
        static HELIUS_TIP_ACCOUNTS: std::sync::LazyLock<Vec<Pubkey>> =
            std::sync::LazyLock::new(|| {
                (1..=10).map(|i| Pubkey::new_from_array([i; 32])).collect()
            });
        static NOZOMI_TIP_ACCOUNTS: std::sync::LazyLock<Vec<Pubkey>> =
            std::sync::LazyLock::new(|| {
                (20..37).map(|i| Pubkey::new_from_array([i; 32])).collect()
            });
        let jito = &pump_codec::constants::JITO_TIP_ACCOUNTS;
        let signer = crate::setup::throwaway_signer().unwrap();
        let owner = signer.pubkey();
        let mut cfg = Config::default();
        cfg.tx.cu_limit = 88_000;
        cfg.tx.lads_bytes = 12_124_160;
        let pool = crate::setup::paper_nonce_pool(owner, 2);
        let t = crate::setup::templates(&cfg, owner, &pool).unwrap();
        let job = |idx, prio, tip| FireJob {
            tmpl: t.buys[0].clone(),
            intent: 77,
            idx,
            prio,
            tip,
            tip_account: t.tip_account,
            bid: 3_400_000,
            real: true,
        };
        // primary (pay on success: tip = bid) and the priority-ordered secondary (tip 0)
        let jobs = [job(0, 10_000, 3_400_000), job(1, 990_000, 0)];
        let targets: Vec<Box<dyn Sender>> = vec![
            Box::new(Fake {
                id: SenderId::Helius,
                label: "helius",
                tips: &HELIUS_TIP_ACCOUNTS,
                min: 1_000_000,
                variants: 2,
            }),
            Box::new(Fake {
                id: SenderId::HeliusSwqos,
                label: "helius_swqos",
                tips: &HELIUS_TIP_ACCOUNTS,
                min: 5_000,
                variants: 2,
            }),
            Box::new(Fake {
                id: SenderId::Nozomi,
                label: "nozomi",
                tips: &NOZOMI_TIP_ACCOUNTS,
                min: 1_000_000,
                variants: 2,
            }),
            Box::new(Fake {
                id: SenderId::Jito,
                label: "jito_frankfurt",
                tips: jito,
                min: 1_000,
                variants: 1,
            }),
            Box::new(Fake {
                id: SenderId::Jito,
                label: "jito_amsterdam",
                tips: jito,
                min: 1_000,
                variants: 1,
            }),
        ];
        let vs = plan_variants(&jobs, &targets, &signer);
        // helius x2, swqos x2, nozomi x2, each Jito region the primary only
        assert_eq!(vs.len(), 8);
        let sigs: std::collections::HashSet<[u8; 64]> = vs.iter().map(|(_, v)| v.sig).collect();
        assert_eq!(sigs.len(), 8, "every variant has its own signature");
        for (k, v) in &vs {
            let s = &targets[*k];
            assert!(
                s.tip_accounts().contains(&v.tip_account.unwrap()),
                "{}",
                s.label()
            );
            assert!(v.tip_lamports >= s.min_tip(), "{}: tip floor", s.label());
            assert!(rugsnipe_tx::senders::check_tip(v, s.tip_accounts(), s.min_tip()).is_ok());
            assert!(rugsnipe_tx::signer::verify(
                &owner,
                &v.bytes()[65..],
                &v.sig
            ));
            match v.idx {
                0 => assert_eq!((v.priority_lamports, v.tip_lamports), (10_000, 3_400_000)),
                _ => {
                    assert_eq!(v.priority_lamports, 990_000);
                    assert_eq!(
                        v.tip_lamports,
                        s.min_tip(),
                        "a zero tip is raised to the floor"
                    );
                }
            }
            if s.id() == SenderId::Jito {
                assert_eq!(v.idx, 0, "Jito: the pay-on-success primary only");
            }
        }
        // the template's tip account is where the variant's tip goes
        let (_, v) = &vs[0];
        let view = pump_codec::tx::TxView::parse(v.bytes()).unwrap();
        assert!(view.static_account_keys().contains(&v.tip_account.unwrap()));
        assert!(!view.static_account_keys().contains(&t.tip_account));
        // two Jito regions never share an account inside one intent
        let jito_accts: Vec<Pubkey> = vs
            .iter()
            .filter(|(k, _)| targets[*k].id() == SenderId::Jito)
            .map(|(_, v)| v.tip_account.unwrap())
            .collect();
        assert_ne!(jito_accts[0], jito_accts[1]);
        // sells go to every path
        let sell = FireJob {
            tmpl: t.sell_final.clone(),
            idx: 0x81,
            ..job(0, 10_000, 1_000_000)
        };
        assert_eq!(plan_variants(&[sell], &targets, &signer).len(), 5);
    }
}
