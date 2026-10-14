//! The hot poll loop is allocation-free after warm-up: a counting global
//! allocator, armed per thread, sees zero allocations while polling a
//! FeedMux over a Replay and a RingFeed (whose producer runs on another thread).

use rugsnipe_feed::event::{CreateHitLite, FeedEvent, FeedSrc, Payload};
use rugsnipe_feed::replay::Replay;
use rugsnipe_feed::ring::ring;
use rugsnipe_feed::{Feed, FeedBuf, FeedMux, FiredSet};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ARMED.with(|a| a.get()) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if ARMED.with(|a| a.get()) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn armed<T>(f: impl FnOnce() -> T) -> (T, u64) {
    let before = ALLOCS.load(Ordering::Relaxed);
    ARMED.with(|a| a.set(true));
    let r = f();
    ARMED.with(|a| a.set(false));
    (r, ALLOCS.load(Ordering::Relaxed) - before)
}

#[test]
fn counter_counts() {
    let (v, n) = armed(|| vec![1u8; 10]);
    assert_eq!((v.len(), n), (10, 1));
}

#[test]
fn poll_loop_does_not_allocate() {
    let text = std::fs::read_to_string(format!(
        "{}/fixtures/logs_frames.jsonl",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let replay = Replay::from_text(&text, true);
    let n_replay = replay.events().len();
    assert!(n_replay > 10);

    let (mut sink, ringfeed, _stop) = ring(FeedSrc::LogsWs, 1024, 0);
    let producer = std::thread::spawn(move || {
        for i in 0..5000u64 {
            let mut h = CreateHitLite::default();
            h.sig.0[..8].copy_from_slice(&i.to_le_bytes());
            h.mint = pump_codec::Pubkey::new_from_array([(i % 251) as u8; 32]);
            let ev = FeedEvent {
                src: FeedSrc::LogsWs,
                recv_ns: i,
                slot: i,
                bank_id: 0,
                payload: Payload::Hit(h),
            };
            while !sink.push(ev) {
                std::thread::yield_now();
            }
        }
    });

    let mut mux = FeedMux::new(vec![Box::new(replay), Box::new(ringfeed)]);
    let fired = FiredSet::default();
    let mut buf = FeedBuf::with_capacity(256, 1 << 16);
    // warm-up: one poll touches every lazily initialized path (clock anchor, ...)
    let _ = mux.poll(&mut buf);
    let warm = buf.len();
    buf.clear();

    let ((seen, firsts), allocs) = armed(|| {
        let mut seen = 0usize;
        let mut firsts = 0usize;
        let t0 = std::time::Instant::now();
        while warm + seen < 5000 + n_replay && t0.elapsed().as_secs() < 20 {
            mux.poll(&mut buf);
            for ev in buf.events() {
                seen += 1;
                if let Some(m) = ev.mint() {
                    if fired.first(&m, 0, ev.src) == rugsnipe_feed::Fired::First {
                        firsts += 1;
                    }
                }
            }
            buf.clear();
        }
        (seen, firsts)
    });
    producer.join().unwrap();
    assert_eq!(warm + seen, 5000 + n_replay, "events lost");
    assert!(firsts > 0);
    assert_eq!(allocs, 0, "the hot poll loop allocated {allocs} times");
}
