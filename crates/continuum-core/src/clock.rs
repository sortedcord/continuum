//! Time source abstraction. The core never reads the system clock directly.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(u64);

impl Timestamp {
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    #[must_use]
    pub fn saturating_add(self, duration: Duration) -> Self {
        Self(self.0.saturating_add(duration_millis(duration)))
    }

    /// Time elapsed since `earlier`, or zero if `earlier` is in the future.
    pub fn saturating_since(self, earlier: Self) -> Duration {
        Duration::from_millis(self.0.saturating_sub(earlier.0))
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// Wall-clock time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(duration_millis(since_epoch))
    }
}

/// A clock that only moves when told to. Clones share the same time.
#[derive(Debug, Clone, Default)]
pub struct ManualClock(Arc<AtomicU64>);

impl ManualClock {
    pub fn new(start: Timestamp) -> Self {
        Self(Arc::new(AtomicU64::new(start.0)))
    }

    pub fn advance(&self, duration: Duration) {
        self.0
            .fetch_add(duration_millis(duration), Ordering::SeqCst);
    }

    pub fn set(&self, now: Timestamp) {
        self.0.store(now.0, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_is_shared_between_clones() {
        let clock = ManualClock::new(Timestamp::from_millis(1_000));
        let other = clock.clone();
        clock.advance(Duration::from_secs(2));
        assert_eq!(other.now(), Timestamp::from_millis(3_000));
    }

    #[test]
    fn saturating_since_never_underflows() {
        let early = Timestamp::from_millis(10);
        let late = Timestamp::from_millis(25);
        assert_eq!(late.saturating_since(early), Duration::from_millis(15));
        assert_eq!(early.saturating_since(late), Duration::ZERO);
    }
}
