//! TTL, TTI, the per-entry TTL and the sweep, on Tokio's paused clock.

use std::time::Duration;

use navree_bounded::{Cache, Compute, Computed};
use tokio::time::advance;

const SECOND: Duration = Duration::from_secs(1);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn ttl_cache(ttl: Duration) -> Cache<String, u32> {
    Cache::builder(100).shards(1).time_to_live(ttl).build()
}

fn tti_cache(tti: Duration) -> Cache<String, u32> {
    Cache::builder(100).shards(1).time_to_idle(tti).build()
}

#[tokio::test(start_paused = true)]
async fn an_entry_lives_for_exactly_its_ttl() {
    let cache = ttl_cache(60 * SECOND);
    cache.insert("a".into(), 1);
    advance(60 * SECOND - ms(1)).await;
    assert_eq!(cache.get("a"), Some(1), "one millisecond before the deadline");
    advance(ms(1)).await;
    assert_eq!(cache.get("a"), None, "at the deadline");
}

#[tokio::test(start_paused = true)]
async fn reading_does_not_extend_a_ttl() {
    let cache = ttl_cache(10 * SECOND);
    cache.insert("a".into(), 1);
    for _ in 0..9 {
        advance(SECOND).await;
        assert_eq!(cache.get("a"), Some(1));
    }
    advance(SECOND).await;
    assert_eq!(cache.get("a"), None);
}

#[tokio::test(start_paused = true)]
async fn replacing_restarts_the_ttl() {
    let cache = ttl_cache(10 * SECOND);
    cache.insert("a".into(), 1);
    advance(8 * SECOND).await;
    cache.insert("a".into(), 2);
    advance(8 * SECOND).await;
    assert_eq!(cache.get("a"), Some(2), "16s after the first insert, 8s after the second");
    advance(2 * SECOND).await;
    assert_eq!(cache.get("a"), None);
}

#[tokio::test(start_paused = true)]
async fn an_idle_entry_expires_and_a_read_restarts_the_clock() {
    let cache = tti_cache(10 * SECOND);
    cache.insert("a".into(), 1);
    cache.insert("b".into(), 2);
    for _ in 0..5 {
        advance(9 * SECOND).await;
        assert_eq!(cache.get("a"), Some(1), "a is read every 9s");
    }
    assert_eq!(cache.get("b"), None, "b was idle for 45s");
    advance(10 * SECOND).await;
    assert_eq!(cache.get("a"), None, "a was idle for 10s");
}

#[tokio::test(start_paused = true)]
async fn contains_key_is_not_a_use() {
    let cache = tti_cache(10 * SECOND);
    cache.insert("a".into(), 1);
    advance(6 * SECOND).await;
    assert!(cache.contains_key("a"));
    advance(4 * SECOND).await;
    assert!(!cache.contains_key("a"), "contains_key at 6s did not restart the idle clock");
}

#[tokio::test(start_paused = true)]
async fn the_sooner_of_ttl_and_tti_wins() {
    let cache: Cache<String, u32> = Cache::builder(10)
        .shards(1)
        .time_to_live(30 * SECOND)
        .time_to_idle(10 * SECOND)
        .build();
    cache.insert("busy".into(), 1);
    cache.insert("idle".into(), 2);
    for _ in 0..3 {
        advance(8 * SECOND).await;
        assert_eq!(cache.get("busy"), Some(1));
    }
    // 32s in: "busy" was read at 24s (so it is not idle), but its TTL ran out
    // at 30s; "idle" went after 10s.
    advance(8 * SECOND).await;
    assert_eq!(cache.get("busy"), None, "the TTL ends a busy entry");
    assert_eq!(cache.get("idle"), None, "the TTI ended the idle one");
}

#[tokio::test(start_paused = true)]
async fn an_entry_ttl_overrides_the_default() {
    let cache = ttl_cache(10 * SECOND);
    cache.insert_with_ttl("short".into(), 1, Some(2 * SECOND));
    cache.insert_with_ttl("long".into(), 2, Some(60 * SECOND));
    cache.insert_with_ttl("forever".into(), 3, None);
    cache.insert("default".into(), 4);
    advance(2 * SECOND).await;
    assert_eq!(cache.get("short"), None);
    advance(8 * SECOND).await;
    assert_eq!(cache.get("default"), None);
    assert_eq!(cache.get("long"), Some(2));
    advance(3_600 * SECOND).await;
    assert_eq!(cache.get("long"), None);
    assert_eq!(cache.get("forever"), Some(3), "None never expires, whatever the default");
}

#[tokio::test(start_paused = true)]
async fn a_zero_ttl_stores_nothing_and_clears_the_old_entry() {
    let cache = ttl_cache(10 * SECOND);
    cache.insert("a".into(), 1);
    cache.insert_with_ttl("a".into(), 2, Some(Duration::ZERO));
    assert_eq!(cache.get("a"), None);
    assert!(cache.is_empty(), "nothing was stored");
}

#[tokio::test(start_paused = true)]
async fn a_huge_ttl_means_never() {
    let cache = ttl_cache(10 * SECOND);
    cache.insert_with_ttl("a".into(), 1, Some(Duration::MAX));
    advance(3_600 * SECOND).await;
    assert_eq!(cache.get("a"), Some(1));
}

#[tokio::test(start_paused = true)]
async fn compute_update_keeps_the_deadline_and_store_sets_a_new_one() {
    let cache = ttl_cache(10 * SECOND);
    cache.insert("a".into(), 1);
    advance(6 * SECOND).await;
    let outcome = cache.compute("a", |n| Compute::Update(n.copied().unwrap_or(0) + 1));
    assert_eq!(outcome, Computed::Replaced);
    advance(4 * SECOND).await;
    assert_eq!(cache.get("a"), None, "Update kept the original deadline");

    cache.insert("b".into(), 1);
    advance(6 * SECOND).await;
    cache.compute("b", |_| Compute::Store(2, Some(10 * SECOND)));
    advance(6 * SECOND).await;
    assert_eq!(cache.get("b"), Some(2), "Store started a new lifetime");
    advance(4 * SECOND).await;
    assert_eq!(cache.get("b"), None);
}

#[tokio::test(start_paused = true)]
async fn compute_sees_an_expired_entry_as_absent() {
    let cache = ttl_cache(10 * SECOND);
    cache.insert("a".into(), 1);
    advance(10 * SECOND).await;
    let outcome = cache.compute("a", |current| {
        assert!(current.is_none(), "an expired entry is not offered");
        Compute::Store(5, None)
    });
    assert_eq!(outcome, Computed::Inserted);
    assert_eq!(cache.get("a"), Some(5));
}

#[tokio::test(start_paused = true)]
async fn a_read_removes_the_expired_entry_it_finds() {
    let cache = ttl_cache(10 * SECOND);
    for i in 0..10 {
        cache.insert(format!("k{i}"), i);
    }
    advance(10 * SECOND).await;
    assert_eq!(cache.len(), 10, "nothing has looked yet");
    assert_eq!(cache.get("k3"), None);
    assert_eq!(cache.len(), 9, "the read removed the one it found");
    assert_eq!(cache.remove("k4"), None);
    assert_eq!(cache.len(), 8, "so did remove");
}

#[tokio::test(start_paused = true)]
async fn a_sweep_is_bounded_and_resumes() {
    let cache = ttl_cache(10 * SECOND);
    for i in 0..100 {
        cache.insert(format!("k{i}"), i);
    }
    advance(10 * SECOND).await;
    assert_eq!(cache.sweep(0), 0);
    let first = cache.sweep(10);
    assert!(first <= 10, "a budget of 10 slots removes at most 10, removed {first}");
    assert_eq!(cache.len(), 100 - first);
    let mut total = first;
    for _ in 0..20 {
        total += cache.sweep(10);
    }
    assert_eq!(total, 100, "the sweeps resume where the last one stopped");
    assert!(cache.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_sweep_leaves_live_entries_alone() {
    let cache = ttl_cache(10 * SECOND);
    for i in 0..10 {
        cache.insert(format!("old{i}"), i);
    }
    advance(6 * SECOND).await;
    for i in 0..10 {
        cache.insert(format!("new{i}"), i);
    }
    advance(5 * SECOND).await;
    assert_eq!(cache.sweep(usize::MAX), 10);
    assert_eq!(cache.len(), 10);
    assert_eq!(cache.get("new3"), Some(3));
}

#[tokio::test(start_paused = true)]
async fn inserting_new_keys_sweeps_a_few_expired_ones_each_time() {
    let cache = ttl_cache(10 * SECOND);
    for i in 0..40 {
        cache.insert(format!("old{i}"), i);
    }
    advance(10 * SECOND).await;
    // Never read, never swept by hand: only new keys arrive.
    for i in 0..40 {
        cache.insert_with_ttl(format!("new{i}"), i, None);
    }
    assert_eq!(cache.len(), 40, "the old entries were reclaimed by the inserts alone");
    for i in 0..40 {
        assert_eq!(cache.get(&format!("new{i}")), Some(i));
    }
}

#[tokio::test(start_paused = true)]
async fn a_cache_with_nothing_that_expires_does_not_sweep() {
    let cache: Cache<String, u32> = Cache::builder(10).shards(1).build();
    for i in 0..10 {
        cache.insert(format!("k{i}"), i);
    }
    advance(3_600 * SECOND).await;
    assert_eq!(cache.sweep(usize::MAX), 0);
    assert_eq!(cache.len(), 10);
}

#[tokio::test(start_paused = true)]
async fn a_full_cache_reclaims_an_expired_entry_before_evicting_a_live_one() {
    let cache: Cache<u8, u8> = Cache::builder(8).shards(1).build();
    cache.insert_with_ttl(0, 0, Some(10 * SECOND));
    for key in 1..8 {
        cache.insert_with_ttl(key, key, None);
    }
    // Read every live key, so only expiry makes entry 0 the victim.
    for key in 1..8 {
        cache.get(&key);
    }
    advance(10 * SECOND).await;
    cache.insert_with_ttl(8, 8, None);
    assert!(!cache.contains_key(&0));
    assert_eq!(cache.len(), 8);
    for key in 1..=8 {
        assert!(cache.contains_key(&key), "live key {key} was evicted");
    }
}

#[tokio::test(start_paused = true)]
async fn a_loaded_value_expires_and_loads_again() {
    use std::sync::atomic::{AtomicU32, Ordering::SeqCst};

    let cache = ttl_cache(10 * SECOND);
    let loads = AtomicU32::new(0);
    let load = || async {
        loads.fetch_add(1, SeqCst);
        loads.load(SeqCst)
    };
    assert_eq!(cache.get_with("a".into(), load()).await, 1);
    advance(9 * SECOND).await;
    assert_eq!(cache.get_with("a".into(), load()).await, 1, "still fresh");
    advance(SECOND).await;
    assert_eq!(cache.get_with("a".into(), load()).await, 2, "expired: loaded again");
    assert_eq!(loads.load(SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn invalidate_all_drops_expiring_entries_and_resets_the_sweep() {
    let cache = ttl_cache(10 * SECOND);
    for i in 0..10 {
        cache.insert(format!("k{i}"), i);
    }
    cache.invalidate_all();
    assert!(cache.is_empty());
    cache.insert("a".into(), 1);
    advance(10 * SECOND).await;
    assert_eq!(cache.sweep(usize::MAX), 1);
}
