//! Clocks. `recv_ns` stamps are monotonic nanoseconds since the first call in
//! this process; wall time is only used for lag versus block time.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static ANCHOR: OnceLock<Instant> = OnceLock::new();

/// Monotonic nanoseconds since the process anchor (first call).
#[inline]
pub fn mono_ns() -> u64 {
    let a = ANCHOR.get_or_init(Instant::now);
    a.elapsed().as_nanos() as u64
}

/// Wall-clock nanoseconds since the Unix epoch.
#[inline]
pub fn unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn monotonic() {
        let a = super::mono_ns();
        let b = super::mono_ns();
        assert!(b >= a);
        assert!(super::unix_ns() > 1_700_000_000_000_000_000);
    }
}
