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
//! * A **sell** rides a plain recent blockhash (no nonce), so copies with
//!   different signatures could all land (a partial take-profit selling
//!   twice, the tip paid twice). A sell is therefore signed **once**
//!   ([`plan_variants`]): one message, one signature, the tip to the first
//!   tip-taking sender's account; every other sender gets the identical
//!   bytes if that message passes its tip check (same tip set, e.g. both
//!   Helius legs, every Jito region; tipless paths such as `rpcsend`) and
//!   is skipped otherwise. One sell job can land at most once.
//! * Each real sender has its own worker thread (warm connection, breaker,
//!   token bucket), so one slow provider never delays the others; a Nozomi
//!   worker sends both variants of an intent in one batch request.
//! * Per-sender send-error ratios ([`crate::triggers::SenderErrWindows`]):
//!   a sender whose own last `risk.send_error_window` sends failed above
//!   `risk.send_error_kill_ratio` is quarantined for `senders.quarantine_min`
//!   (no variants go to it); the cold thread latches `send_errors` only when
//!   every sender is degraded.
//! * Idle: the thread sleeps 200 µs between empty polls, or yields without
//!   sleeping when the hot binary runs with `--busy-poll` ([`set_busy_poll`]).
//! * In a `--probe-dry` run [`SenderSet::real`] is empty: the per-sender
//!   variants are signed exactly as in a real run and recorded, nothing
//!   leaves the box.

use crate::msgs::{ColdMsg, FireJob};
use crate::stats::HotStats;
use crate::triggers::SenderErrWindows;
use rugsnipe_core::rslog::{Rec, RslogProducer};
use rugsnipe_feed::clock::mono_ns;
use rugsnipe_tx::senders::{
    tip_plan, SendError, SendResult, Sender, SenderId, SenderSet, Variant, VariantKind,
};
use rugsnipe_tx::signer::HotSigner;
use std::sync::atomic::{AtomicBool, Ordering::Acquire, Ordering::Relaxed};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// `--busy-poll`: the sender thread yields instead of sleeping when idle.
static BUSY_POLL: AtomicBool = AtomicBool::new(false);

/// Switch the sender thread's idle wait between a 200 µs sleep (off) and a
/// yield (on, `--busy-poll`: the hot role runs on pinned, isolated cores).
/// Read on every idle iteration, so it may be set after the thread started.
pub fn set_busy_poll(on: bool) {
    BUSY_POLL.store(on, Relaxed);
}

/// The idle wait of the sender thread.
fn idle_wait() {
    if BUSY_POLL.load(Relaxed) {
        std::thread::yield_now();
    } else {
        std::thread::sleep(Duration::from_micros(200));
    }
}

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
/// sender: `(target index, variant)`. Buys: tip account and tip per sender
/// ([`tip_plan`]), re-picked until every `(account, tip, priority)` of the
/// intent is unique, so every variant has its own signature (all ride one
/// durable nonce). Sells: one signed message per job ([`plan_sell`]).
pub fn plan_variants(
    jobs: &[FireJob],
    targets: &[Box<dyn Sender>],
    signer: &HotSigner,
) -> Vec<(usize, Variant)> {
    let mut out = vec![];
    let mut used: Vec<([u8; 32], u64, u64)> = vec![];
    for job in jobs {
        let kind = kind_of(job.idx);
        if kind == VariantKind::Sell {
            out.extend(plan_sell(job, targets, signer));
            continue;
        }
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

/// A sell job's copies: **one** signed message for every target that
/// accepts it, so the fan-out can land at most once (a sell has no durable
/// nonce). The tip goes to the first tip-taking target's account
/// ([`tip_plan`], raised to its minimum); a target whose tip check refuses
/// that message (another provider's tip set) is skipped, a tipless path
/// takes it as is. With no tip-taking target the template's tip account
/// and the plan's tip stay.
pub fn plan_sell(
    job: &FireJob,
    targets: &[Box<dyn Sender>],
    signer: &HotSigner,
) -> Vec<(usize, Variant)> {
    let mut tmpl = job.tmpl.clone();
    tmpl.set_priority(job.prio);
    let anchor = targets
        .iter()
        .enumerate()
        .find_map(|(k, s)| tip_plan(s.as_ref(), k, job.idx, job.intent, job.tip));
    let (acct, tip) = anchor.unwrap_or((job.tip_account, job.tip));
    tmpl.set_tip_account(&acct);
    tmpl.set_tip(tip);
    tmpl.sign(signer, None);
    let v = Variant::from_template(
        &tmpl,
        job.intent,
        job.idx,
        VariantKind::Sell,
        job.prio,
        tip,
        Some(acct),
    );
    targets
        .iter()
        .enumerate()
        .filter(|(_, s)| rugsnipe_tx::senders::check_tip(&v, s.tip_accounts(), s.min_tip()).is_ok())
        .map(|(k, _)| (k, v.clone()))
        .collect()
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
    leader: u8,
}

/// Work for one sender's worker: the variants of one intent.
struct Batch {
    variants: Vec<Variant>,
    bid: u64,
    t0: u64,
    leader: u8,
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
                    leader: b.leader,
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
            // the text of a failure, once a minute per sender (the ledger only
            // keeps the short code), so a dead sender can be diagnosed from
            // the journal; keys never appear in it (response bodies, OS errors)
            let last_err_log = std::cell::RefCell::new([0u64; SenderId::COUNT]);
            let record = |d: Done, sent: &mut u64, rslog: &mut RslogProducer| {
                *sent += 1;
                if let SendResult::Failed(e) = &d.result {
                    let detail = match e {
                        SendError::Io(m) | SendError::Rpc(m) => Some(m.as_str()),
                        SendError::Http(_) => Some(""),
                        _ => None,
                    };
                    if let Some(detail) = detail {
                        let mut last = last_err_log.borrow_mut();
                        let t = crate::now_ms();
                        if t.saturating_sub(last[d.id as usize]) >= 60_000 {
                            last[d.id as usize] = t;
                            let http = match e {
                                SendError::Http(s) => format!(" http {s}"),
                                _ => String::new(),
                            };
                            eprintln!(
                                "sender: {} {} {}{http}: {}",
                                d.label,
                                kind_name(d.variant.kind),
                                err_name(&d.result),
                                detail.replace(['\n', '\r'], " ")
                            );
                        }
                    }
                }
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
                    leader: d.leader,
                });
            };
            let mut errs = SenderErrWindows::new(
                senders.send_error_window,
                senders.send_error_ratio,
                senders.quarantine_ms,
            );
            // a real sender's result into its own window; a crossing quarantines it
            let note = |d: &Done, errs: &mut SenderErrWindows| {
                if d.id == SenderId::Paper
                    || matches!(d.result, SendResult::Failed(SendError::NotForThisSender))
                {
                    return;
                }
                let v = errs.on_send(d.label, d.result.is_ok(), crate::now_ms());
                if let Some(q) = v.quarantined {
                    eprintln!("sender: {q}");
                }
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
                            leader: job.leader,
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
                    let leader = real[0].leader;
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
                                    leader,
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
                    let now_ms = crate::now_ms();
                    for (k, vs) in per.into_iter().enumerate() {
                        if vs.is_empty() || errs.is_quarantined(senders.real()[k].label(), now_ms) {
                            continue;
                        }
                        match &workers[k] {
                            Some((tx, _)) => {
                                let _ = tx.send(Batch {
                                    variants: vs,
                                    bid,
                                    t0,
                                    leader,
                                });
                            }
                            None => {
                                let s = &senders.real()[k];
                                for v in vs {
                                    let result = SendResult::Failed(SendError::Io(
                                        "worker not running".into(),
                                    ));
                                    let d = Done {
                                        intent: v.intent,
                                        variant: v,
                                        id: s.id(),
                                        label: s.label(),
                                        bid,
                                        result,
                                        t0,
                                        leader,
                                    };
                                    note(&d, &mut errs);
                                    record(d, &mut sent, &mut rslog);
                                }
                            }
                        }
                    }
                }
                let mut got = 0;
                while let Ok(d) = done_rx.try_recv() {
                    got += 1;
                    note(&d, &mut errs);
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
                    idle_wait();
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
    fn a_sell_intent_never_yields_two_distinct_signatures() {
        static A: std::sync::LazyLock<Vec<Pubkey>> =
            std::sync::LazyLock::new(|| (1..=8).map(|i| Pubkey::new_from_array([i; 32])).collect());
        static B: std::sync::LazyLock<Vec<Pubkey>> = std::sync::LazyLock::new(|| {
            (40..48).map(|i| Pubkey::new_from_array([i; 32])).collect()
        });
        let jito = &pump_codec::constants::JITO_TIP_ACCOUNTS;
        let signer = crate::setup::throwaway_signer().unwrap();
        let mut cfg = Config::default();
        cfg.tx.cu_limit = 65_000;
        cfg.tx.lads_bytes = 12_124_160;
        let pool = crate::setup::paper_nonce_pool(signer.pubkey(), 2);
        let t = crate::setup::templates(&cfg, signer.pubkey(), &pool).unwrap();
        let fake = |id, label, tips: &'static [Pubkey], min| -> Box<dyn Sender> {
            Box::new(Fake {
                id,
                label,
                tips,
                min,
                variants: 2,
            })
        };
        let none: &'static [Pubkey] = &[];
        // every fan-out shape: tipless first, provider sets, Jito regions
        let shapes: Vec<Vec<Box<dyn Sender>>> = vec![
            vec![
                fake(SenderId::Rpc, "rpcsend", none, 0),
                fake(SenderId::Helius, "helius", &A, 1_000_000),
                fake(SenderId::HeliusSwqos, "helius_swqos", &A, 5_000),
                fake(SenderId::Nozomi, "nozomi", &B, 1_000_000),
                fake(SenderId::Jito, "jito_frankfurt", jito, 1_000),
                fake(SenderId::Jito, "jito_amsterdam", jito, 1_000),
            ],
            vec![
                fake(SenderId::Jito, "jito_frankfurt", jito, 1_000),
                fake(SenderId::Jito, "jito_amsterdam", jito, 1_000),
                fake(SenderId::Nozomi, "nozomi", &B, 1_000_000),
                fake(SenderId::Rpc, "rpcsend", none, 0),
            ],
            vec![fake(SenderId::Rpc, "rpcsend", none, 0)],
        ];
        for (n, targets) in shapes.iter().enumerate() {
            for (tmpl, idx) in [(&t.sell_partial, 0x80u8), (&t.sell_final, 0x83)] {
                for tip in [0u64, 2_000_000] {
                    let job = FireJob {
                        tmpl: tmpl.clone(),
                        intent: 99,
                        idx,
                        prio: 50_000,
                        tip,
                        tip_account: t.tip_account,
                        bid: tip,
                        real: true,
                        leader: crate::leader::UNKNOWN,
                    };
                    let vs = plan_variants(&[job], targets, &signer);
                    assert!(!vs.is_empty(), "shape {n}: the sell goes somewhere");
                    let sigs: std::collections::HashSet<[u8; 64]> =
                        vs.iter().map(|(_, v)| v.sig).collect();
                    assert_eq!(sigs.len(), 1, "shape {n}: one sell, one signature");
                    let bytes: std::collections::HashSet<Vec<u8>> =
                        vs.iter().map(|(_, v)| v.bytes().to_vec()).collect();
                    assert_eq!(bytes.len(), 1, "shape {n}: identical bytes everywhere");
                    for (k, v) in &vs {
                        let s = &targets[*k];
                        assert!(
                            rugsnipe_tx::senders::check_tip(v, s.tip_accounts(), s.min_tip())
                                .is_ok(),
                            "shape {n}: {} accepts the message",
                            s.label()
                        );
                    }
                }
            }
        }
        // the Jito-first shape: both regions and the tipless path share it
        let job = FireJob {
            tmpl: t.sell_final.clone(),
            intent: 5,
            idx: 0x80,
            prio: 50_000,
            tip: 10_000,
            tip_account: t.tip_account,
            bid: 10_000,
            real: true,
            leader: crate::leader::UNKNOWN,
        };
        let ks: Vec<usize> = plan_variants(&[job], &shapes[1], &signer)
            .iter()
            .map(|(k, _)| *k)
            .collect();
        assert_eq!(ks, vec![0, 1, 3]);
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
            leader: crate::leader::UNKNOWN,
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
        // a sell is one signed message: the first tip-taking path's tip set
        // (both Helius legs share it), never a second signature
        let sell = FireJob {
            tmpl: t.sell_final.clone(),
            idx: 0x81,
            ..job(0, 10_000, 1_000_000)
        };
        let sv = plan_variants(&[sell], &targets, &signer);
        let ks: Vec<usize> = sv.iter().map(|(k, _)| *k).collect();
        assert_eq!(ks, vec![0, 1], "helius + helius_swqos take the same bytes");
        let sigs: std::collections::HashSet<[u8; 64]> = sv.iter().map(|(_, v)| v.sig).collect();
        assert_eq!(sigs.len(), 1);
    }
}
