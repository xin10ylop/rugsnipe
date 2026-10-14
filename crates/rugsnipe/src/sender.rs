//! Sender thread: takes [`FireJob`]s from the hot loop (secondary variants,
//! and every variant in a real mode), re-prices, re-signs and hands each to
//! the gated [`SenderSet`] (paper only in paper mode), then records `Sent`
//! in its own rslog stream and in the ledger.

use crate::msgs::{ColdMsg, FireJob};
use crate::stats::HotStats;
use rugsnipe_core::rslog::{Rec, RslogProducer};
use rugsnipe_feed::clock::mono_ns;
use rugsnipe_tx::senders::{SenderSet, Variant, VariantKind};
use rugsnipe_tx::signer::HotSigner;
use std::sync::atomic::{AtomicBool, Ordering::Acquire};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Start the sender thread. It exits when the ring is empty and either
/// `stop` is set or the hot loop dropped its producer.
pub fn spawn(
    mut rx: rtrb::Consumer<FireJob>,
    senders: Arc<SenderSet>,
    signer: Arc<HotSigner>,
    mut rslog: RslogProducer,
    cold: Sender<ColdMsg>,
    stats: Arc<HotStats>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<JoinHandle<u64>> {
    std::thread::Builder::new()
        .name("sender".into())
        .spawn(move || {
            let mut sent = 0u64;
            loop {
                let mut n = 0;
                while let Ok(mut job) = rx.pop() {
                    n += 1;
                    let t0 = mono_ns();
                    job.tmpl.set_priority(job.prio);
                    job.tmpl.set_tip(job.tip);
                    job.tmpl.sign(&signer, None);
                    let kind = if job.idx & 0x80 != 0 {
                        VariantKind::Sell
                    } else {
                        VariantKind::Buy
                    };
                    let v = Variant::from_template(
                        &job.tmpl,
                        job.intent,
                        job.idx,
                        kind,
                        job.prio,
                        job.tip,
                        Some(job.tip_account),
                    );
                    for (id, res) in senders.send_all(&v) {
                        sent += 1;
                        stats.sends[id as usize].inc();
                        let now = mono_ns();
                        rslog.push(Rec::Sent {
                            intent: job.intent,
                            variant: job.idx,
                            sender: id as u8,
                            prio: job.prio,
                            tip: job.tip,
                            sig: v.sig,
                            result: res.code(),
                            latency_ns: now.saturating_sub(t0),
                        });
                        let _ = cold.send(ColdMsg::Sent {
                            intent: job.intent,
                            sig: v.sig,
                            variant: job.idx,
                            sender: id as u8,
                            prio: job.prio,
                            tip: job.tip,
                            sent_ns: now,
                            result: res.code(),
                        });
                    }
                }
                if n == 0 {
                    if stop.load(Acquire) || rx.is_abandoned() {
                        return sent;
                    }
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
        })
}
