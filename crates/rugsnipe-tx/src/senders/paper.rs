//! Paper sender: records the would-send variant and never touches the
//! network. This module deliberately imports nothing from `std::net`, `std::io`
//! or any HTTP/TLS crate; `tests/safety.rs` also checks that no socket file
//! descriptor appears while it runs.

use super::{SendResult, Sender, SenderId, Variant};
use std::collections::VecDeque;
use std::sync::Mutex;

/// What the paper sender kept.
#[derive(Debug, Clone)]
pub struct PaperRecord {
    pub variant: Variant,
    pub recorded_wall_ms: u64,
}

/// Bounded in-memory recorder (oldest records drop first).
#[derive(Debug)]
pub struct Paper {
    cap: usize,
    log: Mutex<VecDeque<PaperRecord>>,
    total: std::sync::atomic::AtomicU64,
}

impl Paper {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            log: Mutex::new(VecDeque::with_capacity(cap.min(4_096))),
            total: Default::default(),
        }
    }
    /// Records still held.
    pub fn records(&self) -> Vec<PaperRecord> {
        self.log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
    /// Variants recorded since start.
    pub fn total(&self) -> u64 {
        self.total.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Sender for Paper {
    fn id(&self) -> SenderId {
        SenderId::Paper
    }
    fn is_real(&self) -> bool {
        false
    }
    fn send(&self, v: &Variant) -> SendResult {
        let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        if log.len() == self.cap {
            log.pop_front();
        }
        log.push_back(PaperRecord {
            variant: v.clone(),
            recorded_wall_ms: super::now_ms(),
        });
        self.total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SendResult::Recorded
    }
}
