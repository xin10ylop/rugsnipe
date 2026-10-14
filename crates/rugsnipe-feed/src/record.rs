//! Bounded recorder of raw frames for later replay.
//!
//! Lines are JSONL: `{"recv_ns":<mono>,"recv_unix_ns":<wall>,"src":"logsws","frame":<raw JSON>}`
//! (a frame that is not JSON is stored as `"frame_text":"..."`). Files rotate at
//! `max_file_bytes` and only the newest `max_files` are kept, so disk use is
//! bounded by `max_file_bytes * max_files`. Writing happens on a dedicated
//! thread behind a bounded channel; when it is full, frames are dropped and
//! counted (the feed thread never blocks on disk).

use crate::event::FeedSrc;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Recorder settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordConfig {
    pub dir: PathBuf,
    /// File name prefix (`<prefix>-<unix_s>-<seq>.jsonl`).
    pub prefix: String,
    pub max_file_bytes: u64,
    pub max_files: usize,
    /// Channel depth (frames).
    pub queue: usize,
}

impl RecordConfig {
    /// 64 MiB x 8 files under `dir`.
    pub fn new(dir: impl Into<PathBuf>, prefix: &str) -> RecordConfig {
        RecordConfig {
            dir: dir.into(),
            prefix: prefix.to_string(),
            max_file_bytes: 64 << 20,
            max_files: 8,
            queue: 8192,
        }
    }
}

struct Line {
    src: FeedSrc,
    recv_ns: u64,
    recv_unix_ns: u64,
    frame: String,
}

/// Counters of a [`Recorder`].
#[derive(Debug, Default)]
pub struct RecordStats {
    pub written: AtomicU64,
    pub dropped: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
}

/// Handle to the recorder thread. Cloning shares the thread.
#[derive(Debug, Clone)]
pub struct Recorder {
    tx: SyncSender<Line>,
    stats: Arc<RecordStats>,
}

impl std::fmt::Debug for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line({}, {} bytes)", self.src.name(), self.frame.len())
    }
}

impl Recorder {
    /// Start the writer thread (creates `dir`).
    pub fn spawn(cfg: RecordConfig) -> io::Result<(Recorder, JoinHandle<()>)> {
        fs::create_dir_all(&cfg.dir)?;
        let (tx, rx) = sync_channel::<Line>(cfg.queue.max(1));
        let stats = Arc::new(RecordStats::default());
        let st = stats.clone();
        let h = std::thread::Builder::new()
            .name("feed-recorder".into())
            .spawn(move || {
                let mut w = Writer::new(cfg);
                for line in rx {
                    match w.write(&line) {
                        Ok(n) => {
                            st.written.fetch_add(1, Relaxed);
                            st.bytes.fetch_add(n as u64, Relaxed);
                        }
                        Err(_) => {
                            st.errors.fetch_add(1, Relaxed);
                        }
                    }
                }
                let _ = w.flush();
            })?;
        Ok((Recorder { tx, stats }, h))
    }

    /// Queue one frame (never blocks; drops and counts when the queue is full).
    pub fn record(&self, src: FeedSrc, recv_ns: u64, recv_unix_ns: u64, frame: &str) {
        let line = Line {
            src,
            recv_ns,
            recv_unix_ns,
            frame: frame.to_string(),
        };
        match self.tx.try_send(line) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.stats.dropped.fetch_add(1, Relaxed);
            }
        }
    }

    pub fn stats(&self) -> &RecordStats {
        &self.stats
    }
}

struct Writer {
    cfg: RecordConfig,
    cur: Option<(BufWriter<File>, u64)>,
    seq: u64,
    start_s: u64,
}

impl Writer {
    fn new(cfg: RecordConfig) -> Writer {
        Writer {
            cfg,
            cur: None,
            seq: 0,
            start_s: crate::clock::unix_ns() / 1_000_000_000,
        }
    }

    fn open(&mut self) -> io::Result<()> {
        let name = format!("{}-{}-{:04}.jsonl", self.cfg.prefix, self.start_s, self.seq);
        self.seq += 1;
        let f = File::create(self.cfg.dir.join(name))?;
        self.cur = Some((BufWriter::new(f), 0));
        prune(&self.cfg.dir, &self.cfg.prefix, self.cfg.max_files)?;
        Ok(())
    }

    fn write(&mut self, l: &Line) -> io::Result<usize> {
        let need_new = match &self.cur {
            None => true,
            Some((_, n)) => *n >= self.cfg.max_file_bytes,
        };
        if need_new {
            if let Some((mut w, _)) = self.cur.take() {
                w.flush()?;
            }
            self.open()?;
        }
        let body = if serde_json::from_str::<serde::de::IgnoredAny>(&l.frame).is_ok() {
            format!("\"frame\":{}", l.frame.trim())
        } else {
            format!(
                "\"frame_text\":{}",
                serde_json::to_string(&l.frame).unwrap_or_else(|_| "\"\"".into())
            )
        };
        let text = format!(
            "{{\"recv_ns\":{},\"recv_unix_ns\":{},\"src\":\"{}\",{}}}\n",
            l.recv_ns,
            l.recv_unix_ns,
            l.src.name(),
            body
        );
        let (w, n) = self.cur.as_mut().expect("open");
        w.write_all(text.as_bytes())?;
        *n += text.len() as u64;
        Ok(text.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some((w, _)) = self.cur.as_mut() {
            w.flush()?;
        }
        Ok(())
    }
}

/// Delete the oldest `<prefix>-*.jsonl` files beyond `keep`.
fn prune(dir: &Path, prefix: &str, keep: usize) -> io::Result<()> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{prefix}-")) && n.ends_with(".jsonl"))
        })
        .collect();
    files.sort();
    while files.len() > keep.max(1) {
        let _ = fs::remove_file(files.remove(0));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_and_bounds_disk() {
        let dir = std::env::temp_dir().join(format!("rugsnipe-rec-{}", crate::clock::unix_ns()));
        let cfg = RecordConfig {
            dir: dir.clone(),
            prefix: "t".into(),
            max_file_bytes: 300,
            max_files: 3,
            queue: 1000,
        };
        let (r, h) = Recorder::spawn(cfg).unwrap();
        for i in 0..50 {
            r.record(FeedSrc::LogsWs, i, i, r#"{"a":1}"#);
        }
        r.record(FeedSrc::LogsWs, 99, 99, "not json");
        drop(r);
        h.join().unwrap();
        let files: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(files.len(), 3);
        let mut all = String::new();
        let mut paths: Vec<_> = files.into_iter().map(|f| f.unwrap().path()).collect();
        paths.sort();
        for p in &paths {
            all.push_str(&fs::read_to_string(p).unwrap());
        }
        let last = all.lines().last().unwrap();
        assert!(last.contains("\"frame_text\":\"not json\""), "{last}");
        for l in all.lines() {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            assert_eq!(v["src"], "logsws");
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
