//! Small helpers for the demo routes and jobs: random durations and
//! chances, without a random number generator of their own.

use uuid::Uuid;

/// A random number from `low` to `high`, both included.
pub fn between(low: u64, high: u64) -> u64 {
    if high <= low {
        return low;
    }
    let span = u128::from(high - low) + 1;
    let random = Uuid::new_v4().as_u128() & u128::from(u64::MAX);
    low + u64::try_from(random % span).unwrap_or(0)
}

/// Whether a one-in-`n` chance came up.
pub fn one_in(n: u64) -> bool {
    between(0, n.saturating_sub(1)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_stay_inside_their_range() {
        for _ in 0..200 {
            let n = between(1_300, 2_200);
            assert!((1_300..=2_200).contains(&n));
        }
        assert_eq!(between(5, 5), 5);
        assert_eq!(between(9, 3), 9);
    }

    #[test]
    fn a_one_in_one_chance_always_comes_up() {
        assert!(one_in(1));
    }
}
