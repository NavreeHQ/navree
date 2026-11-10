//! The cache's clock: Tokio's, so `tokio::time::pause` and `advance` drive
//! expiry in tests, counted in nanoseconds since the cache was built.

use std::time::Duration;

use tokio::time::Instant;

/// A deadline that never comes (also the "no TTI" value: an idle limit of
/// `NEVER` nanoseconds is never reached).
pub(crate) const NEVER: u64 = u64::MAX;

/// Nanoseconds since the cache was built, read from Tokio's clock.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Clock {
    origin: Instant,
}

impl Clock {
    pub(crate) fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// The time now, always below [`NEVER`].
    pub(crate) fn now(&self) -> u64 {
        let nanos = Instant::now()
            .saturating_duration_since(self.origin)
            .as_nanos();
        u64::try_from(nanos).map_or(NEVER - 1, |n| n.min(NEVER - 1))
    }
}

/// A duration in nanoseconds, saturating.
pub(crate) fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(NEVER)
}

/// The deadline `ttl` after `now` (`None`: never).
pub(crate) fn deadline_after(now: u64, ttl: Option<Duration>) -> u64 {
    ttl.map_or(NEVER, |ttl| now.saturating_add(nanos(ttl)))
}
