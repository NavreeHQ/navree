//! [`Backoff`]: the delays between the attempts of one request.

use std::time::Duration;

/// The delay before each retry of one request: the first delay, then twice
/// the one before up to a cap, each with a random share of itself added
/// (jitter, so clients that failed together do not retry together), for at
/// most the number of retries it was built for.
///
/// The first delay is used as given, even above the cap; every later one is
/// at most the cap before jitter, and jitter can add up to as much again.
pub(crate) struct Backoff {
    /// The delay before the next retry without jitter; `None` before the
    /// first.
    current: Option<Duration>,
    first: Duration,
    cap: Duration,
    left: u32,
    /// A number in `[0, 1)`: the share of a delay added as jitter.
    jitter: fn() -> f64,
}

impl Backoff {
    /// `retries` delays starting at `first` and doubling up to `cap`, with
    /// random jitter.
    pub(crate) fn new(first: Duration, cap: Duration, retries: u32) -> Self {
        Self::with_jitter(first, cap, retries, rand::random::<f64>)
    }

    /// Like [`new`](Self::new), with `jitter` drawing the shares.
    fn with_jitter(first: Duration, cap: Duration, retries: u32, jitter: fn() -> f64) -> Self {
        Self {
            current: None,
            first,
            cap,
            left: retries,
            jitter,
        }
    }
}

impl Iterator for Backoff {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        self.left = self.left.checked_sub(1)?;
        let delay = match self.current {
            None => self.first,
            Some(previous) if previous < self.cap => previous.saturating_mul(2).min(self.cap),
            Some(_) => self.cap,
        };
        self.current = Some(delay);
        let share = (self.jitter)().clamp(0.0, 1.0);
        let extra = Duration::try_from_secs_f64(delay.as_secs_f64() * share).unwrap_or_default();
        Some(delay.saturating_add(extra))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn delays(first: Duration, cap: Duration, retries: u32, jitter: fn() -> f64) -> Vec<Duration> {
        Backoff::with_jitter(first, cap, retries, jitter).collect()
    }

    #[test]
    fn delays_double_up_to_the_cap() {
        assert_eq!(
            delays(ms(200), ms(30_000), 10, || 0.0),
            [
                ms(200),
                ms(400),
                ms(800),
                ms(1_600),
                ms(3_200),
                ms(6_400),
                ms(12_800),
                ms(25_600),
                ms(30_000),
                ms(30_000),
            ]
        );
    }

    #[test]
    fn jitter_adds_up_to_a_whole_delay() {
        assert_eq!(
            delays(ms(200), ms(30_000), 3, || 0.5),
            [ms(300), ms(600), ms(1_200)]
        );
        let nearly = delays(ms(200), ms(30_000), 3, || 0.999_999);
        assert!(nearly[0] > ms(399) && nearly[0] < ms(400), "{nearly:?}");
        assert!(nearly[2] > ms(1_599) && nearly[2] < ms(1_600), "{nearly:?}");
        // Jitter is on top of the cap: the largest delay is twice the cap.
        let last = delays(ms(200), ms(1_000), 6, || 0.999_999)[5];
        assert!(last > ms(1_999) && last < ms(2_000), "{last:?}");
    }

    #[test]
    fn a_first_delay_above_the_cap_is_used_as_given() {
        assert_eq!(
            delays(ms(40_000), ms(30_000), 3, || 0.0),
            [ms(40_000), ms(30_000), ms(30_000)]
        );
    }

    #[test]
    fn the_number_of_delays_is_the_number_of_retries() {
        assert!(delays(ms(200), ms(30_000), 0, || 0.0).is_empty());
        assert_eq!(delays(ms(200), ms(30_000), 1, || 0.0), [ms(200)]);
        let mut backoff = Backoff::with_jitter(ms(1), ms(2), 2, || 0.0);
        assert!(backoff.next().is_some());
        assert!(backoff.next().is_some());
        assert_eq!(backoff.next(), None);
        assert_eq!(backoff.next(), None, "and it stays finished");
    }

    #[test]
    fn a_zero_first_delay_stays_zero() {
        assert_eq!(
            delays(Duration::ZERO, ms(30_000), 3, || 0.7),
            [Duration::ZERO; 3]
        );
    }

    #[test]
    fn huge_delays_saturate_instead_of_overflowing() {
        let first = Duration::MAX / 2 + ms(1);
        let delays = delays(first, Duration::MAX, 3, || 0.999);
        assert!(delays[0] >= first, "{delays:?}");
        assert_eq!(delays[1..], [Duration::MAX; 2]);
        let u64_ms = Duration::from_millis(u64::MAX);
        let delays = Backoff::with_jitter(u64_ms, Duration::MAX, 4, || 0.5).collect::<Vec<_>>();
        assert_eq!(delays.len(), 4);
    }

    #[test]
    fn a_jitter_outside_zero_to_one_is_held_to_it() {
        assert_eq!(delays(ms(100), ms(1_000), 1, || -3.0), [ms(100)]);
        assert_eq!(delays(ms(100), ms(1_000), 1, || 7.0), [ms(200)]);
        assert_eq!(delays(ms(100), ms(1_000), 1, || f64::NAN), [ms(100)]);
    }

    #[test]
    fn random_jitter_stays_between_one_and_two_delays() {
        for _ in 0..200 {
            let drawn: Vec<Duration> = Backoff::new(ms(200), ms(30_000), 4).collect();
            for (delay, base) in drawn.iter().zip([200, 400, 800, 1_600]) {
                assert!(*delay >= ms(base) && *delay < ms(2 * base), "{drawn:?}");
            }
        }
    }
}
