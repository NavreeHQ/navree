//! Properties of percentage rollouts.

use navree_flags::{Rollout, bucket};
use proptest::prelude::*;
use uuid::Uuid;

proptest! {
    /// Raising the percentage never turns a user or workspace off.
    #[test]
    fn raising_the_percentage_never_turns_a_scope_off(
        id in any::<u128>(),
        flag in "[a-z][a-z0-9-]{0,30}",
        low in 0_u8..=100,
        raise in 0_u8..=100,
    ) {
        let id = Uuid::from_u128(id);
        let high = low.saturating_add(raise).min(100);
        if Rollout::percent(low).includes_named(&flag, id) {
            prop_assert!(Rollout::percent(high).includes_named(&flag, id));
        }
        if Rollout::percent(low).of_workspaces().includes_named(&flag, id) {
            prop_assert!(Rollout::percent(high).of_workspaces().includes_named(&flag, id));
        }
    }

    /// Buckets are within 0..100 and the edges are exact.
    #[test]
    fn buckets_are_percent_slots(id in any::<u128>(), flag in "[a-z][a-z0-9-]{0,30}") {
        let id = Uuid::from_u128(id);
        let slot = bucket(&flag, id);
        prop_assert!(slot < 100);
        prop_assert!(!Rollout::percent(slot).includes_named(&flag, id));
        prop_assert!(Rollout::percent(slot + 1).includes_named(&flag, id));
        prop_assert!(Rollout::percent(100).includes_named(&flag, id));
        prop_assert!(!Rollout::percent(0).includes_named(&flag, id));
    }
}

#[test]
fn percentages_roughly_match_their_share() {
    let ids: Vec<Uuid> = (0..10_000_u128)
        .map(|n| Uuid::from_u128(n * 7919))
        .collect();
    for percent in [10_u8, 25, 50, 90] {
        let inside = ids
            .iter()
            .filter(|id| Rollout::percent(percent).includes_named("new-pipeline", **id))
            .count();
        let expected = ids.len() * usize::from(percent) / 100;
        let tolerance = ids.len() / 50; // 2 points
        assert!(
            inside.abs_diff(expected) <= tolerance,
            "{percent}%: {inside} of {} inside, expected about {expected}",
            ids.len()
        );
    }
}
