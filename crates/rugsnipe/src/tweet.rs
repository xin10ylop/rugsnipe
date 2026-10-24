//! The tweet check (`entry.tweet_gate`, docs/13-tweet-coins.md): a coin a
//! launch tool deploys on a tweet names that tweet in its metadata file
//! (`"twitter": "https://x.com/<author>/status/<id>"`). On Oct 23-24 the
//! first coin on a fresh tweet paid and the later copies lost, so the bot
//! buys only the first coin on a tweet under `entry.tweet_max_age_s`.
//!
//! The hot loop hands every create it may want (a `entry.tweet_hosts` uri,
//! a dev buy of at least `entry.tweet_min_dev_sol`) to this lane through an
//! allocation-free ring and parks the launch; the lane fetches the metadata
//! off the hot thread (keep-alive connections kept warm), ranks the coin
//! among the coins seen on the same tweet (in create order, not fetch
//! order) and answers through a second ring. The coins seen per tweet are
//! saved next to state.db every minute and at shutdown, so a restart (the
//! deployer table's refresh restarts the bot) does not take the copies of
//! a tweet seen before it for first coins.

use pump_codec::create_hit::URI_MAX;
use pump_codec::Pubkey;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Twitter's snowflake epoch (ms): a tweet id's top bits are its time.
const SNOWFLAKE_EPOCH_MS: u64 = 1_288_834_974_657;
/// The dispatcher's idle poll (the fetch itself takes 10-150 ms).
const POLL: Duration = Duration::from_micros(500);
/// How often the warm thread touches each host (Cloudflare closes idle
/// keep-alive connections after a few minutes).
const WARM_EVERY: Duration = Duration::from_secs(20);
/// Tweets are forgotten this long after their own time.
const KEEP_MS: u64 = 3 * 3_600_000;
/// Coins remembered per tweet (more are counted as copies).
const PER_TWEET: usize = 32;
/// Largest metadata body read.
pub const MAX_BODY: u64 = 64 * 1024;

/// A create the hot loop hands to the lane (`Copy`: the request ring holds
/// these, no heap on the create path).
#[derive(Clone, Copy)]
pub struct TweetReq {
    pub mint: Pubkey,
    pub slot: u64,
    /// The create's block position (`u32::MAX`: unknown, after any known
    /// one of its slot).
    pub index: u32,
    pub uri: [u8; URI_MAX],
    pub uri_len: u8,
    /// Wall clock (ms) when the hot loop saw the create: the tweet's age
    /// is taken at it.
    pub seen_ms: u64,
    /// The hot loop's monotonic clock (ns) at the push: the verdict's delay.
    pub push_ns: u64,
}

impl TweetReq {
    pub fn uri(&self) -> &str {
        core::str::from_utf8(&self.uri[..self.uri_len as usize]).unwrap_or("")
    }
}

/// What the lane found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TweetKind {
    /// The first coin on its tweet (in create order, among the creates the
    /// lane was asked about) and the tweet is fresh: the gate passes.
    First = 0,
    /// An earlier coin links the same tweet.
    Copy = 1,
    /// The tweet is older than `entry.tweet_max_age_s`.
    Stale = 2,
    /// The metadata names no tweet (no `twitter` status link).
    NoTweet = 3,
    /// The fetch failed (network, timeout, HTTP status, not JSON).
    Error = 4,
}

impl TweetKind {
    pub const COUNT: usize = 5;
    pub const ALL: [TweetKind; 5] = [
        Self::First,
        Self::Copy,
        Self::Stale,
        Self::NoTweet,
        Self::Error,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Copy => "copy",
            Self::Stale => "stale",
            Self::NoTweet => "no_tweet",
            Self::Error => "error",
        }
    }
}

/// The lane's answer for one create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweetVerdict {
    pub mint: Pubkey,
    pub slot: u64,
    pub kind: TweetKind,
    /// The tweet's id (0: none).
    pub sid: u64,
    /// The coin's place among the coins on its tweet (1 = first; 0: no tweet).
    pub rank: u16,
    /// Seconds from the tweet to the create (negative: clock skew).
    pub age_s: i64,
    /// The request's push time (hot loop's monotonic ns).
    pub push_ns: u64,
    /// Time in the fetch itself (us).
    pub fetch_us: u32,
    /// Why an [`TweetKind::Error`] failed ([`FetchFail::None`] otherwise).
    pub fail: FetchFail,
    /// The answer's HTTP status (0: no answer).
    pub http: u16,
}

/// Why a fetch gave no verdict (journaled with the error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FetchFail {
    None = 0,
    /// No answer within `entry.tweet_timeout_ms`.
    Timeout = 1,
    /// Connect, TLS or read error.
    Network = 2,
    /// A non-2xx status.
    Status = 3,
    /// A 2xx body that is not JSON.
    Body = 4,
}

impl FetchFail {
    pub fn of(fetched: &Result<(u16, String), String>, kind: TweetKind) -> (Self, u16) {
        let http = fetched.as_ref().map(|(s, _)| *s).unwrap_or(0);
        let fail = match fetched {
            _ if kind != TweetKind::Error => Self::None,
            Err(e) if e.to_ascii_lowercase().contains("timeout") || e.contains("timed out") => {
                Self::Timeout
            }
            Err(_) => Self::Network,
            Ok((s, _)) if !(200..300).contains(s) => Self::Status,
            Ok(_) => Self::Body,
        };
        (fail, http)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Timeout => "timeout",
            Self::Network => "network",
            Self::Status => "http",
            Self::Body => "not json",
        }
    }
}

/// An error verdict's reason for the journal line: ` (timeout)`,
/// ` (http 404)`; nothing for other verdicts (formats without allocating).
pub struct FailNote(pub FetchFail, pub u16);

impl std::fmt::Display for FailNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            FetchFail::None => Ok(()),
            FetchFail::Status => write!(f, " (http {})", self.1),
            k => write!(f, " ({})", k.as_str()),
        }
    }
}

/// The tweet id of a status link (`.../status/<digits>`), if any.
pub fn status_id(link: &str) -> Option<u64> {
    let i = link.find("/status/")? + "/status/".len();
    let digits: &str = &link[i..];
    let n = digits.bytes().take_while(u8::is_ascii_digit).count();
    digits[..n].parse().ok().filter(|&v| v > 0)
}

/// A tweet id's own time (unix ms).
pub fn tweet_ms(sid: u64) -> u64 {
    (sid >> 22).saturating_add(SNOWFLAKE_EPOCH_MS)
}

/// The tweet a metadata body links (its `twitter` field), if any.
pub fn tweet_of(body: &str) -> Result<Option<u64>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    Ok(v.get("twitter")
        .and_then(|t| t.as_str())
        .and_then(status_id))
}

/// The coins seen per tweet, in create order (the lane's memory).
#[derive(Debug, Default)]
pub struct TweetBook {
    by_tweet: HashMap<u64, Vec<(u64, u32, Pubkey)>>,
}

impl TweetBook {
    /// Record `mint` (created at `slot`, block position `index`) on tweet
    /// `sid` and return its rank: 1 + the coins created before it. The same
    /// mint again keeps its place. A coin whose fetch finished first but
    /// that was created later loses rank 1 to the earlier one when that one
    /// arrives (both may have passed by then: the order is only known once
    /// both are in).
    pub fn rank(&mut self, sid: u64, slot: u64, index: u32, mint: Pubkey) -> u16 {
        let v = self.by_tweet.entry(sid).or_default();
        if !v.iter().any(|x| x.2 == mint) && v.len() < PER_TWEET {
            v.push((slot, index, mint));
            v.sort_unstable_by_key(|x| (x.0, x.1));
        }
        let at = (slot, index);
        let earlier = v.iter().filter(|x| (x.0, x.1) < at).count();
        // a coin not kept (the tweet is full) is a late copy
        let kept = v.iter().any(|x| x.2 == mint);
        (if kept { earlier + 1 } else { PER_TWEET + 1 }).min(u16::MAX as usize) as u16
    }

    /// Forget tweets older than `keep_ms` at `now_ms` (by the tweet's time).
    pub fn prune(&mut self, now_ms: u64, keep_ms: u64) {
        self.by_tweet
            .retain(|sid, _| tweet_ms(*sid).saturating_add(keep_ms) >= now_ms);
    }

    pub fn len(&self) -> usize {
        self.by_tweet.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_tweet.is_empty()
    }

    /// One line per coin: `sid slot index mint`.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(self.by_tweet.len() * 96);
        for (sid, v) in &self.by_tweet {
            for (slot, index, mint) in v {
                out.push_str(&format!("{sid} {slot} {index} {mint}\n"));
            }
        }
        out
    }

    /// The book [`Self::to_text`] wrote (bad lines are skipped).
    pub fn from_text(text: &str) -> Self {
        let mut b = Self::default();
        for l in text.lines() {
            let mut f = l.split_ascii_whitespace();
            let (Some(sid), Some(slot), Some(index), Some(mint)) = (
                f.next().and_then(|x| x.parse::<u64>().ok()),
                f.next().and_then(|x| x.parse::<u64>().ok()),
                f.next().and_then(|x| x.parse::<u32>().ok()),
                f.next().and_then(|x| x.parse::<Pubkey>().ok()),
            ) else {
                continue;
            };
            b.rank(sid, slot, index, mint);
        }
        b
    }

    /// Write the book to `path` (through a temporary file and a rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }

    /// The book saved at `path`, its old tweets dropped (empty if there is
    /// none).
    pub fn load(path: &Path, now_ms: u64) -> Self {
        let mut b = std::fs::read_to_string(path)
            .map(|t| Self::from_text(&t))
            .unwrap_or_default();
        b.prune(now_ms, KEEP_MS);
        b
    }

    /// Coins remembered.
    pub fn coins(&self) -> usize {
        self.by_tweet.values().map(Vec::len).sum()
    }
}

/// The verdict on one fetched body (`fetched`: `(status, body)` or the
/// fetch error), ranking the coin in `book`.
pub fn judge(
    fetched: &Result<(u16, String), String>,
    req: &TweetReq,
    book: &mut TweetBook,
    max_age_s: u64,
) -> (TweetKind, u64, u16, i64) {
    let body = match fetched {
        Ok((s, b)) if (200..300).contains(s) => b,
        _ => return (TweetKind::Error, 0, 0, 0),
    };
    let sid = match tweet_of(body) {
        Ok(Some(s)) => s,
        Ok(None) => return (TweetKind::NoTweet, 0, 0, 0),
        Err(_) => return (TweetKind::Error, 0, 0, 0),
    };
    let age_s = (req.seen_ms as i64 - tweet_ms(sid) as i64) / 1_000;
    let rank = book.rank(sid, req.slot, req.index, req.mint);
    let kind = if rank > 1 {
        TweetKind::Copy
    } else if age_s > max_age_s as i64 {
        TweetKind::Stale
    } else {
        TweetKind::First
    };
    (kind, sid, rank, age_s)
}

/// A GET: `(status, body)` or an error (the real one is
/// `rugsnipe_tx::rpc::HttpGet`; tests pass their own).
pub type FetchFn = Arc<dyn Fn(&str) -> Result<(u16, String), String> + Send + Sync>;

/// The lane's settings (`entry.tweet_*`).
#[derive(Debug, Clone)]
pub struct TweetLaneCfg {
    pub workers: usize,
    pub max_age_s: u64,
    /// Urls the warm thread GETs every [`WARM_EVERY`] (one per tweet host),
    /// so a create's fetch finds a connection with TLS done.
    pub warm: Vec<String>,
    /// Where the book is saved and loaded from (none: kept in memory only).
    pub book_path: Option<PathBuf>,
}

/// The lane's threads (they end once the hot loop drops its request ring).
pub struct TweetLane {
    threads: Vec<JoinHandle<()>>,
}

impl TweetLane {
    pub fn join(self) {
        for t in self.threads {
            let _ = t.join();
        }
    }
}

/// Start the lane: a dispatcher draining `requests`, `cfg.workers` fetch
/// threads answering into `verdicts`, and the warm thread.
pub fn spawn(
    cfg: TweetLaneCfg,
    fetch: FetchFn,
    mut requests: rtrb::Consumer<TweetReq>,
    verdicts: rtrb::Producer<TweetVerdict>,
) -> std::io::Result<TweetLane> {
    let book = cfg
        .book_path
        .as_deref()
        .map(|p| TweetBook::load(p, crate::now_ms()))
        .unwrap_or_default();
    if let Some(p) = &cfg.book_path {
        eprintln!(
            "tweet check: {} coins on {} tweets of the last {} h loaded from {}",
            book.coins(),
            book.len(),
            KEEP_MS / 3_600_000,
            p.display()
        );
    }
    let book = Arc::new(Mutex::new(book));
    let out = Arc::new(Mutex::new(verdicts));
    let stop = Arc::new(AtomicBool::new(false));
    let (job_tx, job_rx) = mpsc::channel::<TweetReq>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let mut threads = vec![];
    for w in 0..cfg.workers.max(1) {
        let (book, out, job_rx, fetch) = (book.clone(), out.clone(), job_rx.clone(), fetch.clone());
        let max_age_s = cfg.max_age_s;
        threads.push(
            std::thread::Builder::new()
                .name(format!("tweet-w{w}"))
                .spawn(move || loop {
                    let job = { job_rx.lock().unwrap_or_else(|e| e.into_inner()).recv() };
                    let Ok(req) = job else { return };
                    let t = Instant::now();
                    let fetched = fetch(req.uri());
                    let fetch_us = t.elapsed().as_micros().min(u32::MAX as u128) as u32;
                    let (kind, sid, rank, age_s) = {
                        let mut b = book.lock().unwrap_or_else(|e| e.into_inner());
                        judge(&fetched, &req, &mut b, max_age_s)
                    };
                    let (fail, http) = FetchFail::of(&fetched, kind);
                    let v = TweetVerdict {
                        mint: req.mint,
                        slot: req.slot,
                        kind,
                        sid,
                        rank,
                        age_s,
                        push_ns: req.push_ns,
                        fetch_us,
                        fail,
                        http,
                    };
                    // a full answer ring drops the verdict: the park then
                    // times out as tweet_late (the ring holds 1024)
                    let _ = out.lock().unwrap_or_else(|e| e.into_inner()).push(v);
                })?,
        );
    }
    {
        let (book, stop, book_path) = (book.clone(), stop.clone(), cfg.book_path.clone());
        let save = move |b: &TweetBook| {
            if let Some(p) = &book_path {
                if let Err(e) = b.save(p) {
                    eprintln!("tweet check: saving {}: {e}", p.display());
                }
            }
        };
        threads.push(
            std::thread::Builder::new()
                .name("tweet-lane".into())
                .spawn(move || {
                    let mut last_prune = Instant::now();
                    loop {
                        let mut any = false;
                        while let Ok(r) = requests.pop() {
                            any = true;
                            if job_tx.send(r).is_err() {
                                break;
                            }
                        }
                        if requests.is_abandoned() && requests.is_empty() {
                            stop.store(true, Relaxed);
                            save(&book.lock().unwrap_or_else(|e| e.into_inner()));
                            return; // drops job_tx: the workers end
                        }
                        if last_prune.elapsed() > Duration::from_secs(60) {
                            last_prune = Instant::now();
                            let mut b = book.lock().unwrap_or_else(|e| e.into_inner());
                            b.prune(crate::now_ms(), KEEP_MS);
                            save(&b);
                        }
                        if !any {
                            std::thread::sleep(POLL);
                        }
                    }
                })?,
        );
    }
    if !cfg.warm.is_empty() {
        let (warm, stop) = (cfg.warm.clone(), stop.clone());
        threads.push(
            std::thread::Builder::new()
                .name("tweet-warm".into())
                .spawn(move || {
                    let mut next = Instant::now();
                    while !stop.load(Relaxed) {
                        if Instant::now() >= next {
                            next = Instant::now() + WARM_EVERY;
                            for u in &warm {
                                let _ = fetch(u);
                            }
                        }
                        std::thread::sleep(Duration::from_millis(250));
                    }
                })?,
        );
    }
    Ok(TweetLane { threads })
}

/// The warm url of a tweet host: its root over https (any answer keeps the
/// pooled connection; the body is ignored).
pub fn warm_url(host: &str) -> String {
    format!("https://{}/", host.trim_start_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(mint: Pubkey, slot: u64, index: u32, seen_ms: u64) -> TweetReq {
        let uri = "https://metadata.j7tracker.io/metadata/qxtvmbCFDM.json";
        let mut r = TweetReq {
            mint,
            slot,
            index,
            uri: [0; URI_MAX],
            uri_len: uri.len() as u8,
            seen_ms,
            push_ns: 0,
        };
        r.uri[..uri.len()].copy_from_slice(uri.as_bytes());
        r
    }

    const SID: u64 = 2_113_944_610_368_082_408;

    fn body(sid: u64) -> Result<(u16, String), String> {
        Ok((
            200,
            format!(
                r#"{{"name":"Solana 2","symbol":"SOL2","twitter":"https://x.com/HimothyyDeploys/status/{sid}","website":""}}"#
            ),
        ))
    }

    #[test]
    fn status_ids_and_tweet_times_follow_the_snowflake() {
        assert_eq!(
            status_id("https://x.com/a/status/2113944610368082408"),
            Some(SID)
        );
        assert_eq!(
            status_id("https://x.com/i/web/status/2113944610368082408?s=20"),
            Some(SID)
        );
        assert_eq!(status_id("https://x.com/HimothyyDeploys"), None);
        assert_eq!(status_id("https://x.com/a/status/"), None);
        // the research's ((id >> 22) + 1288834974657) / 1000
        assert_eq!(
            tweet_ms(SID) / 1000,
            ((SID >> 22) + 1_288_834_974_657) / 1000
        );
        assert_eq!(tweet_of(&body(SID).unwrap().1), Ok(Some(SID)));
        assert_eq!(tweet_of(r#"{"name":"x","twitter":""}"#), Ok(None));
        assert!(tweet_of("<html>").is_err());
    }

    #[test]
    fn the_first_coin_on_a_fresh_tweet_passes_and_copies_rank_behind_it() {
        let mut b = TweetBook::default();
        let t = tweet_ms(SID);
        let (a, c, d) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        // 4 s after the tweet: first
        let (k, sid, rank, age) = judge(&body(SID), &req(a, 100, 50, t + 4_000), &mut b, 3600);
        assert_eq!((k, sid, rank, age), (TweetKind::First, SID, 1, 4));
        // the same mint again keeps rank 1 (a second feed's copy)
        assert_eq!(
            judge(&body(SID), &req(a, 100, 50, t + 4_000), &mut b, 3600).0,
            TweetKind::First
        );
        // a later coin on the same tweet is a copy
        let (k, _, rank, _) = judge(&body(SID), &req(c, 103, 7, t + 6_000), &mut b, 3600);
        assert_eq!((k, rank), (TweetKind::Copy, 2));
        // a coin created before `a` but fetched after it takes rank 1
        let (k, _, rank, _) = judge(&body(SID), &req(d, 100, 10, t + 4_000), &mut b, 3600);
        assert_eq!((k, rank), (TweetKind::First, 1));
        assert_eq!(b.rank(SID, 100, 50, a), 2, "a now ranks behind d");
    }

    #[test]
    fn stale_tweets_missing_links_and_failed_fetches_do_not_pass() {
        let mut b = TweetBook::default();
        let t = tweet_ms(SID);
        let m = Pubkey::new_unique();
        assert_eq!(
            judge(&body(SID), &req(m, 1, 1, t + 3_601_000), &mut b, 3600).0,
            TweetKind::Stale
        );
        let none = Ok((200, r#"{"name":"x","symbol":"X"}"#.to_string()));
        assert_eq!(
            judge(&none, &req(Pubkey::new_unique(), 1, 2, t), &mut b, 3600).0,
            TweetKind::NoTweet
        );
        let e404 = Ok((404, "not found".to_string()));
        assert_eq!(
            judge(&e404, &req(Pubkey::new_unique(), 1, 3, t), &mut b, 3600).0,
            TweetKind::Error
        );
        let err: Result<(u16, String), String> = Err("timeout".into());
        assert_eq!(
            judge(&err, &req(Pubkey::new_unique(), 1, 4, t), &mut b, 3600).0,
            TweetKind::Error
        );
        // a skewed clock (the create seen before the tweet's own time) is fresh
        let (k, _, _, age) = judge(
            &body(SID + (5 << 22)),
            &req(Pubkey::new_unique(), 1, 5, t),
            &mut b,
            3600,
        );
        assert_eq!(k, TweetKind::First);
        assert!(age <= 0);
    }

    #[test]
    fn the_book_survives_a_restart_and_drops_old_tweets_on_load() {
        let dir = std::env::temp_dir().join(format!("tweet_book_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tweet_book.txt");
        let (a, c) = (Pubkey::new_unique(), Pubkey::new_unique());
        let old_sid = SID - ((4 * 3_600_000u64) << 22); // 4 h older than SID
        let mut b = TweetBook::default();
        b.rank(SID, 100, 50, a);
        b.rank(old_sid, 1, 1, c);
        b.save(&path).unwrap();
        assert_eq!(TweetBook::from_text(&b.to_text()).coins(), 2);
        // a bad line is skipped
        std::fs::write(&path, format!("{}garbage line\n", b.to_text())).unwrap();
        let mut back = TweetBook::load(&path, tweet_ms(SID) + 60_000);
        assert_eq!(
            (back.len(), back.coins()),
            (1, 1),
            "the 4 h old tweet is dropped"
        );
        // a coin after the restart on the same tweet is a copy of the one before
        assert_eq!(back.rank(SID, 103, 7, Pubkey::new_unique()), 2);
        assert_eq!(back.rank(SID, 100, 50, a), 1);
        // no file: an empty book
        assert!(TweetBook::load(&dir.join("none.txt"), 0).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_fetch_says_why() {
        let k = TweetKind::Error;
        let t: Result<(u16, String), String> = Err("timeout: global".into());
        assert_eq!(FetchFail::of(&t, k), (FetchFail::Timeout, 0));
        let n: Result<(u16, String), String> = Err("io: Connection reset by peer".into());
        assert_eq!(FetchFail::of(&n, k), (FetchFail::Network, 0));
        let h: Result<(u16, String), String> = Ok((404, "Not Found".into()));
        assert_eq!(FetchFail::of(&h, k), (FetchFail::Status, 404));
        let j: Result<(u16, String), String> = Ok((200, "<html>".into()));
        assert_eq!(FetchFail::of(&j, k), (FetchFail::Body, 200));
        assert_eq!(
            FetchFail::of(&body(SID), TweetKind::First),
            (FetchFail::None, 200)
        );
        assert_eq!(FailNote(FetchFail::Status, 404).to_string(), " (http 404)");
        assert_eq!(FailNote(FetchFail::Timeout, 0).to_string(), " (timeout)");
        assert_eq!(FailNote(FetchFail::None, 200).to_string(), "");
    }

    #[test]
    fn the_book_forgets_old_tweets() {
        let mut b = TweetBook::default();
        b.rank(SID, 1, 1, Pubkey::new_unique());
        b.prune(tweet_ms(SID) + KEEP_MS - 1, KEEP_MS);
        assert_eq!(b.len(), 1);
        b.prune(tweet_ms(SID) + KEEP_MS + 1, KEEP_MS);
        assert!(b.is_empty());
    }

    #[test]
    fn the_lane_answers_through_its_ring_and_ends_with_the_hot_loop() {
        let (mut req_tx, req_rx) = rtrb::RingBuffer::<TweetReq>::new(16);
        let (out_tx, mut out_rx) = rtrb::RingBuffer::<TweetVerdict>::new(16);
        let fetch: FetchFn = Arc::new(|u: &str| {
            if u.ends_with("qxtvmbCFDM.json") {
                body(SID)
            } else {
                Err("unreachable".into())
            }
        });
        let lane = spawn(
            TweetLaneCfg {
                workers: 2,
                max_age_s: 3600,
                warm: vec![],
                book_path: None,
            },
            fetch,
            req_rx,
            out_tx,
        )
        .unwrap();
        let m = Pubkey::new_unique();
        req_tx
            .push(req(m, 7, 3, tweet_ms(SID) + 2_000))
            .ok()
            .unwrap();
        let t = Instant::now();
        let v = loop {
            if let Ok(v) = out_rx.pop() {
                break v;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "no verdict");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            (v.mint, v.slot, v.kind, v.sid, v.rank),
            (m, 7, TweetKind::First, SID, 1)
        );
        drop(req_tx);
        lane.join();
    }
}
