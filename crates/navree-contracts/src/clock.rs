//! [`Clock`]: the source of "now", replaceable in tests.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use navree_datetime::{Span, Timestamp};

/// The current time. Resolve `Arc<dyn Clock>` instead of calling
/// [`Timestamp::now`] where tests need to control time.
pub trait Clock: Send + Sync + 'static {
    /// The current time.
    fn now(&self) -> Timestamp;
}

/// The real clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

mod private {
    pub trait Sealed {}
}

/// A length of time [`FixedClock::advance`] moves a clock by: a [`Span`] or a
/// [`Duration`].
pub trait ClockStep: private::Sealed {
    #[doc(hidden)]
    fn into_clock_span(self) -> Span;
}

impl private::Sealed for Span {}

impl ClockStep for Span {
    fn into_clock_span(self) -> Span {
        self
    }
}

impl private::Sealed for Duration {}

impl ClockStep for Duration {
    fn into_clock_span(self) -> Span {
        Span::try_from(self).unwrap_or(Span::MAX)
    }
}

/// A clock that stands still until [`set`](FixedClock::set) or
/// [`advance`](FixedClock::advance) moves it.
#[derive(Debug)]
pub struct FixedClock(Mutex<Timestamp>);

impl FixedClock {
    /// A clock showing `now`.
    pub fn new(now: Timestamp) -> Self {
        Self(Mutex::new(now))
    }

    /// Moves the clock to `now`.
    pub fn set(&self, now: Timestamp) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = now;
    }

    /// Moves the clock by `by` (saturating at [`Timestamp::MIN`] and
    /// [`Timestamp::MAX`]).
    pub fn advance(&self, by: impl ClockStep) {
        let mut now = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let delta = by.into_clock_span();
        *now = now.checked_add(delta).unwrap_or(if delta.is_negative() {
            Timestamp::MIN
        } else {
            Timestamp::MAX
        });
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
