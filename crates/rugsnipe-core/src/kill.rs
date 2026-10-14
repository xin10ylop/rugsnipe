//! Kill file: if the configured path exists, buys stop (the latch stays set
//! after the file is removed; only a local unkill clears it). One `stat`
//! per poll; call it from the cold thread every ~100 ms and before each
//! decision batch.

use crate::risk::{KillReason, RiskGate};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct KillFile {
    path: PathBuf,
}

impl KillFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
    /// `true` if the kill file exists now.
    pub fn present(&self) -> bool {
        std::fs::symlink_metadata(&self.path).is_ok()
    }
    /// Latch `gate` if the file exists; returns whether the gate is killed.
    pub fn poll(&self, gate: &RiskGate) -> bool {
        if self.present() {
            gate.kill(KillReason::KillFile);
        }
        gate.is_killed()
    }
}
