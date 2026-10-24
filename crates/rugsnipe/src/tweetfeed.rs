//! The tweet feed's copy check (`entry.tweet_feed`, docs/14 section 7).
//!
//! A copy reuses the first coin's tweet, so it reuses its words: on Oct 23-24
//! a coin whose ticker or name matched only posts an earlier coin had already
//! matched was a copy 60% of the time, and on the coins we won those made
//! -2.5% against +3.2% for the rest. The lane keeps the posts of the tracked
//! accounts (TweetStream) of the last `entry.tweet_feed_window_s`, each as a
//! match blob (its text, author, quoted text and quoted author: ASCII letters
//! and digits, lower case, fields split by a 0 byte), in an index the hot
//! loop reads without a lock ([`Posts`]: the lane swaps in a new list per
//! post). The hot loop calls [`classify`] at every tweet-host create (the
//! same coins the tweet check fetches), which also marks the posts it
//! matched as taken: no allocation, a substring search over a few hundred
//! posts.

use arc_swap::ArcSwap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Twitter's snowflake epoch (ms): a post id's top bits are its time.
const SNOWFLAKE_EPOCH_MS: u64 = 1_288_834_974_657;
/// Posts kept at most (the oldest go first).
pub const MAX_POSTS: usize = 4_096;
/// Shortest ticker key that may match (2-letter tickers match by chance).
pub const MIN_SYM: usize = 3;
/// Shortest name key that may match.
pub const MIN_NAME: usize = 4;

/// One post of a tracked account.
#[derive(Debug)]
pub struct Post {
    pub id: u64,
    /// Publish time (from the id).
    pub pub_ms: u64,
    /// When it reached this host.
    pub rx_ms: u64,
    pub blob: Box<[u8]>,
    /// A coin has matched it.
    pub used: AtomicBool,
}

/// The index the lane writes and the hot loop reads (oldest first).
pub type Posts = Arc<ArcSwap<Vec<Arc<Post>>>>;

pub fn new_posts() -> Posts {
    Arc::new(ArcSwap::from_pointee(Vec::new()))
}

/// Append the match key of `s` (ASCII letters and digits, lower case).
pub fn push_key(s: &str, out: &mut Vec<u8>) {
    out.extend(
        s.bytes()
            .filter(u8::is_ascii_alphanumeric)
            .map(|b| b.to_ascii_lowercase()),
    );
}

/// The feed's call on a coin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FeedCall {
    /// No post matched.
    None = 0,
    /// It matched a post no earlier coin had matched.
    First = 1,
    /// Every post it matched was taken by an earlier coin.
    Copy = 2,
}

impl FeedCall {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::First => "first",
            Self::Copy => "copy",
        }
    }
}

/// What [`classify`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classified {
    pub call: FeedCall,
    /// Posts matched.
    pub matched: u16,
    /// The newest matched post (0: none) and its age at `now_ms`.
    pub post: u64,
    pub post_age_ms: u64,
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(hay, needle).is_some()
}

/// The call on a coin with ticker key `sym` and name key `name` at `now_ms`:
/// the posts received by then and published within `window_ms` that contain
/// the ticker (3+) or the name (4+). Every matched post is marked taken.
pub fn classify(
    posts: &[Arc<Post>],
    sym: &[u8],
    name: &[u8],
    now_ms: u64,
    window_ms: u64,
) -> Classified {
    let mut out = Classified {
        call: FeedCall::None,
        matched: 0,
        post: 0,
        post_age_ms: 0,
    };
    let (use_sym, use_name) = (sym.len() >= MIN_SYM, name.len() >= MIN_NAME);
    if !use_sym && !use_name {
        return out;
    }
    let mut fresh = false;
    for p in posts.iter().rev() {
        if p.rx_ms > now_ms {
            continue;
        }
        if p.pub_ms.saturating_add(window_ms) < now_ms {
            break; // oldest first: the rest are older still
        }
        if (use_sym && contains(&p.blob, sym)) || (use_name && contains(&p.blob, name)) {
            if out.matched == 0 {
                out.post = p.id;
                out.post_age_ms = now_ms.saturating_sub(p.pub_ms);
            }
            out.matched = out.matched.saturating_add(1);
            fresh |= !p.used.swap(true, Relaxed);
        }
    }
    out.call = match (out.matched, fresh) {
        (0, _) => FeedCall::None,
        (_, true) => FeedCall::First,
        (_, false) => FeedCall::Copy,
    };
    out
}

/// A post from one TweetStream frame (`tweet/content`, or a `tweet/update`
/// that carries text), with its blob.
pub fn parse_frame(raw: &str, rx_ms: u64) -> Option<Post> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("v")?.as_u64()? != 1 || v.get("t")?.as_str()? != "tweet" {
        return None;
    }
    let op = v.get("op")?.as_str()?;
    let d = v.get("d")?;
    if !(op == "content" || (op == "update" && d.get("text").is_some())) {
        return None;
    }
    let id: u64 = d.get("tweetId")?.as_str()?.parse().ok()?;
    let who = |a: Option<&serde_json::Value>| {
        a.and_then(|a| a.get("handle").or_else(|| a.get("username")))
            .and_then(|h| h.as_str())
            .unwrap_or("")
            .to_string()
    };
    let text = |x: Option<&serde_json::Value>| x.and_then(|t| t.as_str()).unwrap_or("").to_string();
    let mut blob = Vec::with_capacity(256);
    push_key(&text(d.get("text")), &mut blob);
    blob.push(0);
    push_key(&who(d.get("author")), &mut blob);
    if let Some(r) = d.get("ref") {
        blob.push(0);
        push_key(&text(r.get("text")), &mut blob);
        blob.push(0);
        push_key(&who(r.get("author")), &mut blob);
    }
    Some(Post {
        id,
        pub_ms: (id >> 22).saturating_add(SNOWFLAKE_EPOCH_MS),
        rx_ms,
        blob: blob.into_boxed_slice(),
        used: AtomicBool::new(false),
    })
}

/// Add `p` to `posts` (a later frame of the same post replaces its blob and
/// keeps its receipt time and taken mark), dropping posts published more
/// than `window_ms` + 60 s before `now_ms` and the oldest past
/// [`MAX_POSTS`].
pub fn insert(posts: &Posts, p: Post, now_ms: u64, window_ms: u64) {
    let cur = posts.load();
    let keep_after = now_ms.saturating_sub(window_ms.saturating_add(60_000));
    let mut next: Vec<Arc<Post>> = cur
        .iter()
        .filter(|q| q.pub_ms >= keep_after && q.id != p.id)
        .cloned()
        .collect();
    let p = match cur.iter().find(|q| q.id == p.id) {
        Some(old) => Post {
            rx_ms: old.rx_ms,
            used: AtomicBool::new(old.used.load(Relaxed)),
            ..p
        },
        None => p,
    };
    // oldest first by publish time (frames can come out of order)
    let at = next.partition_point(|q| q.pub_ms <= p.pub_ms);
    next.insert(at, Arc::new(p));
    if next.len() > MAX_POSTS {
        let cut = next.len() - MAX_POSTS;
        next.drain(..cut);
    }
    posts.store(Arc::new(next));
}

/// Lane counters (the status line).
#[derive(Debug, Default)]
pub struct FeedStats {
    pub posts: AtomicU64,
    pub connects: AtomicU64,
    pub drops: AtomicU64,
    /// The last post's publish-to-receipt time (ms).
    pub last_lat_ms: AtomicU64,
    pub connected: AtomicBool,
}

/// The lane's thread.
pub struct FeedLane {
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl FeedLane {
    pub fn stop_and_join(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start the lane: TweetStream at `url` with `key`, posts into `posts`.
pub fn spawn(
    url: String,
    key: zeroize::Zeroizing<String>,
    window_ms: u64,
    posts: Posts,
    stats: Arc<FeedStats>,
) -> std::io::Result<FeedLane> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("tweet-feed".into())
        .spawn(move || {
            let st = stats.clone();
            rugsnipe_feed::ws::tweetstream::run_blocking(
                &url,
                &key,
                &stop2,
                &mut |s| match s {
                    rugsnipe_feed::ws::tweetstream::Status::Connected => {
                        st.connects.fetch_add(1, Relaxed);
                        st.connected.store(true, Relaxed);
                        eprintln!("tweet feed: connected {url}");
                    }
                    rugsnipe_feed::ws::tweetstream::Status::Closed(why) => {
                        st.drops.fetch_add(1, Relaxed);
                        st.connected.store(false, Relaxed);
                        eprintln!("tweet feed: {why}; reconnecting");
                    }
                },
                &mut |raw, rx_ms| {
                    if let Some(p) = parse_frame(raw, rx_ms) {
                        if raw.contains("\"content\"") {
                            stats.posts.fetch_add(1, Relaxed);
                            stats
                                .last_lat_ms
                                .store(rx_ms.saturating_sub(p.pub_ms), Relaxed);
                        }
                        insert(&posts, p, rx_ms, window_ms);
                    }
                },
            );
        })?;
    Ok(FeedLane {
        thread: Some(thread),
        stop,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: u64 = 2_113_944_610_368_082_408;

    fn frame(id: u64, text: &str, author: &str) -> String {
        format!(
            r#"{{"v":1,"t":"tweet","op":"content","ts":1,"d":{{"tweetId":"{id}","author":{{"handle":"{author}"}},"text":"{text}","ref":{{"type":"quote","author":{{"username":"Quoted"}},"text":"Big News"}}}}}}"#
        )
    }

    fn pub_ms(id: u64) -> u64 {
        (id >> 22) + SNOWFLAKE_EPOCH_MS
    }

    #[test]
    fn a_frame_becomes_a_post_with_its_match_blob() {
        let p = parse_frame(&frame(ID, "SOL 2 is LIVE! $SOL2", "DevvvOoooor"), 7).unwrap();
        assert_eq!((p.id, p.pub_ms, p.rx_ms), (ID, pub_ms(ID), 7));
        assert_eq!(&*p.blob, b"sol2islivesol2\0devvvooooor\0bignews\0quoted");
        assert!(parse_frame(r#"{"v":1,"t":"control","op":"auth_pong","d":{}}"#, 0).is_none());
        assert!(parse_frame(r#"{"v":1,"t":"tweet","op":"meta","d":{"tweetId":"1"}}"#, 0).is_none());
        assert!(parse_frame("not json", 0).is_none());
    }

    #[test]
    fn the_first_coin_takes_the_post_and_a_later_coin_on_it_is_a_copy() {
        let posts = new_posts();
        let t = pub_ms(ID);
        insert(
            &posts,
            parse_frame(&frame(ID, "Introducing Moon Dog", "ann"), t + 160).unwrap(),
            t + 160,
            1_800_000,
        );
        let snap = posts.load();
        // the ticker is in the post: first, and the post is taken
        let c = classify(&snap, b"moondog", b"moondog", t + 3_000, 1_800_000);
        assert_eq!(
            (c.call, c.matched, c.post, c.post_age_ms),
            (FeedCall::First, 1, ID, 3_000)
        );
        // the next coin on the same words: copy
        assert_eq!(
            classify(&snap, b"mdog", b"moondog", t + 40_000, 1_800_000).call,
            FeedCall::Copy
        );
        // a coin on other words: none; keys too short do not match
        assert_eq!(
            classify(&snap, b"cat", b"catcoin", t + 5_000, 1_800_000).call,
            FeedCall::None
        );
        assert_eq!(
            classify(&snap, b"mo", b"dog", t + 5_000, 1_800_000).call,
            FeedCall::None
        );
        // a post not yet received, or older than the window, does not match
        assert_eq!(
            classify(&snap, b"moondog", b"", t + 100, 1_800_000).call,
            FeedCall::None
        );
        assert_eq!(
            classify(&snap, b"moondog", b"", t + 1_900_000, 1_800_000).call,
            FeedCall::None
        );
    }

    #[test]
    fn an_update_keeps_the_taken_mark_and_old_posts_leave_the_index() {
        let posts = new_posts();
        let t = pub_ms(ID);
        insert(
            &posts,
            parse_frame(&frame(ID, "short", "a"), t + 100).unwrap(),
            t + 100,
            60_000,
        );
        assert_eq!(
            classify(&posts.load(), b"short", b"", t + 1_000, 60_000).call,
            FeedCall::First
        );
        // the full text arrives later as an update: same post, still taken
        let upd = format!(
            r#"{{"v":1,"t":"tweet","op":"update","d":{{"tweetId":"{ID}","text":"short but complete"}}}}"#
        );
        insert(&posts, parse_frame(&upd, t + 900).unwrap(), t + 900, 60_000);
        let snap = posts.load();
        assert_eq!(snap.len(), 1);
        assert_eq!((snap[0].rx_ms, snap[0].used.load(Relaxed)), (t + 100, true));
        assert_eq!(
            classify(&snap, b"complete", b"", t + 2_000, 60_000).call,
            FeedCall::Copy
        );
        // a post published 3 minutes later pushes the first out (window 60 s + 60 s)
        let later = ID + (180_000u64 << 22);
        insert(
            &posts,
            parse_frame(&frame(later, "next", "a"), t + 180_100).unwrap(),
            t + 180_100,
            60_000,
        );
        let snap = posts.load();
        assert_eq!(snap.iter().map(|p| p.id).collect::<Vec<_>>(), vec![later]);
    }
}
