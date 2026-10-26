//! [`Clock`]: the source of "now", replaceable in tests.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};

/// The current time. Resolve `Arc<dyn Clock>` instead of calling `Utc::now()`
/// where tests need to control time.
pub trait Clock: Send + Sync + 'static {
    /// The current time.
    fn now(&self) -> DateTime<Utc>;
}

/// The real clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that stands still until [`set`](FixedClock::set) or
/// [`advance`](FixedClock::advance) moves it.
#[derive(Debug)]
pub struct FixedClock(Mutex<DateTime<Utc>>);

impl FixedClock {
    /// A clock showing `now`.
    pub fn new(now: DateTime<Utc>) -> Self {
        Self(Mutex::new(now))
    }

    /// Moves the clock to `now`.
    pub fn set(&self, now: DateTime<Utc>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = now;
    }

    /// Moves the clock forward by `by` (saturating at the latest representable time).
    pub fn advance(&self, by: Duration) {
        let mut now = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let delta = TimeDelta::from_std(by).unwrap_or(TimeDelta::MAX);
        *now = now
            .checked_add_signed(delta)
            .unwrap_or(DateTime::<Utc>::MAX_UTC);
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
