//! Single-flight loading: one load per key at a time, errors not cached,
//! and no cancellation, panic or write able to wedge a key. On Tokio's
//! paused clock, so every interleaving here is the same on every run.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::time::Duration;

use futures::FutureExt;
use futures::future::join_all;
use navree_bounded::{Cache, Compute};
use tokio::time::{sleep, timeout};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn cache() -> Cache<String, u32> {
    Cache::builder(100).shards(1).build()
}

fn explode() -> u32 {
    panic!("the loader exploded")
}

#[tokio::test(start_paused = true)]
async fn concurrent_callers_share_one_load() {
    let cache = cache();
    let loads = AtomicU32::new(0);
    let callers = (0..64).map(|_| {
        cache.get_with("k".to_owned(), async {
            loads.fetch_add(1, SeqCst);
            sleep(ms(10)).await;
            42
        })
    });
    let values = join_all(callers).await;
    assert!(values.iter().all(|v| *v == 42), "{values:?}");
    assert_eq!(loads.load(SeqCst), 1, "one load for 64 callers");
    assert_eq!(cache.get("k"), Some(42));
}

#[tokio::test(start_paused = true)]
async fn different_keys_load_independently() {
    let cache = cache();
    let loads = AtomicU32::new(0);
    let callers = (0..32u32).map(|i| {
        let (cache, loads) = (&cache, &loads);
        async move {
            // Four callers per key.
            cache
                .get_with(format!("k{}", i % 8), async move {
                    loads.fetch_add(1, SeqCst);
                    sleep(ms(5)).await;
                    i % 8
                })
                .await
        }
    });
    let values = join_all(callers).await;
    assert_eq!(loads.load(SeqCst), 8, "one load per key");
    for (i, value) in values.iter().enumerate() {
        assert_eq!(*value, i as u32 % 8);
    }
}

#[tokio::test(start_paused = true)]
async fn a_loaded_value_is_served_without_loading_again() {
    let cache = cache();
    let loads = AtomicU32::new(0);
    for _ in 0..5 {
        let value = cache
            .get_with("k".to_owned(), async {
                loads.fetch_add(1, SeqCst);
                1
            })
            .await;
        assert_eq!(value, 1);
    }
    assert_eq!(loads.load(SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_failed_load_is_shared_by_its_waiters_and_not_cached() {
    let cache = cache();
    let loads = AtomicU32::new(0);
    let callers = (0..16).map(|_| {
        cache.try_get_with("k".to_owned(), async {
            loads.fetch_add(1, SeqCst);
            sleep(ms(5)).await;
            Err::<u32, String>("boom".to_owned())
        })
    });
    let results = join_all(callers).await;
    assert_eq!(loads.load(SeqCst), 1, "one load, one failure");
    let first = results[0].as_ref().unwrap_err();
    assert_eq!(**first, "boom");
    for result in &results {
        assert!(
            Arc::ptr_eq(first, result.as_ref().unwrap_err()),
            "every waiter gets the very same error"
        );
    }
    assert_eq!(cache.get("k"), None, "the failure was not cached");
    assert!(cache.is_empty());

    // The next caller loads again, and may succeed.
    let value = cache
        .try_get_with("k".to_owned(), async {
            loads.fetch_add(1, SeqCst);
            Ok::<_, String>(7)
        })
        .await
        .unwrap();
    assert_eq!(value, 7);
    assert_eq!(loads.load(SeqCst), 2);
    assert_eq!(cache.get("k"), Some(7));
}

#[tokio::test(start_paused = true)]
async fn the_error_of_a_load_reaches_a_lone_caller_intact() {
    #[derive(Debug, PartialEq, Eq)]
    struct NotFound(u32);

    let cache = cache();
    let error = cache
        .try_get_with("k".to_owned(), async { Err::<u32, _>(NotFound(404)) })
        .await
        .unwrap_err();
    assert_eq!(*error, NotFound(404));
    // The caller can take the error out when it is the only holder.
    assert_eq!(Arc::try_unwrap(error).unwrap(), NotFound(404));
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_loader_hands_the_load_to_a_waiter() {
    let cache = cache();
    let started = AtomicU32::new(0);
    let leader = timeout(
        ms(10),
        cache.get_with("k".to_owned(), async {
            started.fetch_add(1, SeqCst);
            sleep(ms(1_000)).await;
            1
        }),
    );
    let b = cache.get_with("k".to_owned(), async {
        started.fetch_add(1, SeqCst);
        sleep(ms(100)).await;
        2
    });
    let c = cache.get_with("k".to_owned(), async {
        started.fetch_add(1, SeqCst);
        sleep(ms(100)).await;
        3
    });
    let (leader, b, c) = futures::join!(leader, b, c);
    assert!(leader.is_err(), "the first caller timed out");
    assert_eq!(b, c, "both waiters got the value of the one that took over");
    assert!(b == 2 || b == 3);
    assert_eq!(
        started.load(SeqCst),
        2,
        "the cancelled load and the one that replaced it"
    );
    assert_eq!(cache.get("k"), Some(b));
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_loader_with_no_waiters_leaves_the_key_free() {
    let cache = cache();
    let result = timeout(
        ms(10),
        cache.get_with("k".to_owned(), async {
            sleep(ms(1_000)).await;
            1
        }),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(cache.get("k"), None);
    // Not wedged: the next caller loads at once.
    let value = timeout(ms(10), cache.get_with("k".to_owned(), async { 2 }))
        .await
        .expect("the key is not held by the cancelled load");
    assert_eq!(value, 2);
}

#[tokio::test(start_paused = true)]
async fn a_panicking_loader_does_not_wedge_the_key() {
    let cache = cache();
    let leader = AssertUnwindSafe(cache.get_with("k".to_owned(), async {
        sleep(ms(10)).await;
        explode()
    }))
    .catch_unwind();
    let waiter = cache.get_with("k".to_owned(), async {
        sleep(ms(10)).await;
        9
    });
    let (leader, waiter) = futures::join!(leader, waiter);
    assert!(
        leader.is_err(),
        "the panic reached the caller that ran the load"
    );
    assert_eq!(waiter, 9, "the waiter loaded the value itself");
    assert_eq!(cache.get("k"), Some(9));
}

#[tokio::test(start_paused = true)]
async fn a_panicking_loader_alone_leaves_the_key_loadable() {
    let cache = cache();
    let panicked = AssertUnwindSafe(cache.get_with("k".to_owned(), async { explode() }))
        .catch_unwind()
        .await;
    assert!(panicked.is_err());
    assert_eq!(cache.get("k"), None);
    assert_eq!(cache.get_with("k".to_owned(), async { 3 }).await, 3);
    assert_eq!(cache.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_load_that_is_invalidated_meanwhile_is_not_stored() {
    let cache = cache();
    let load = cache.get_with("k".to_owned(), async {
        sleep(ms(10)).await;
        1
    });
    let invalidate = async {
        sleep(ms(5)).await;
        cache.invalidate("k");
    };
    let (value, ()) = futures::join!(load, invalidate);
    assert_eq!(value, 1, "its own caller still gets the value");
    assert_eq!(cache.get("k"), None, "but the stale result was not stored");
}

#[tokio::test(start_paused = true)]
async fn a_load_is_not_stored_over_a_newer_insert() {
    let cache = cache();
    let load = cache.get_with("k".to_owned(), async {
        sleep(ms(10)).await;
        1
    });
    let insert = async {
        sleep(ms(5)).await;
        cache.insert("k".to_owned(), 5);
    };
    let (value, ()) = futures::join!(load, insert);
    assert_eq!(value, 1);
    assert_eq!(cache.get("k"), Some(5), "the insert is the newer write");
}

#[tokio::test(start_paused = true)]
async fn a_compute_and_invalidate_all_also_abandon_a_load() {
    let cache = cache();
    let slow = |value| async move {
        sleep(ms(10)).await;
        value
    };
    let load = cache.get_with("a".to_owned(), slow(1));
    let compute = async {
        sleep(ms(5)).await;
        cache.compute("a", |_| Compute::Store(7, None));
    };
    let (value, ()) = futures::join!(load, compute);
    assert_eq!(value, 1);
    assert_eq!(cache.get("a"), Some(7));

    let load = cache.get_with("b".to_owned(), slow(2));
    let clear = async {
        sleep(ms(5)).await;
        cache.invalidate_all();
    };
    let (value, ()) = futures::join!(load, clear);
    assert_eq!(value, 2);
    assert_eq!(cache.get("b"), None);
}

#[tokio::test(start_paused = true)]
async fn a_call_after_an_invalidation_starts_a_fresh_load() {
    let cache = cache();
    let loads = AtomicU32::new(0);
    let first = cache.get_with("k".to_owned(), async {
        loads.fetch_add(1, SeqCst);
        sleep(ms(10)).await;
        1
    });
    let second = async {
        sleep(ms(5)).await;
        cache.invalidate("k");
        cache
            .get_with("k".to_owned(), async {
                loads.fetch_add(1, SeqCst);
                sleep(ms(10)).await;
                2
            })
            .await
    };
    let (first, second) = futures::join!(first, second);
    assert_eq!((first, second), (1, 2));
    assert_eq!(loads.load(SeqCst), 2);
    assert_eq!(
        cache.get("k"),
        Some(2),
        "the load started after the invalidation is the one stored"
    );
}

#[tokio::test(start_paused = true)]
async fn a_waiter_whose_error_type_differs_loads_for_itself() {
    let cache = cache();
    let leader = cache.try_get_with("k".to_owned(), async {
        sleep(ms(5)).await;
        Err::<u32, String>("boom".to_owned())
    });
    let other = cache.try_get_with("k".to_owned(), async { Ok::<u32, std::io::Error>(7) });
    let (leader, other) = futures::join!(leader, other);
    assert_eq!(*leader.unwrap_err(), "boom");
    assert_eq!(other.unwrap(), 7);
    assert_eq!(cache.get("k"), Some(7));
}

#[tokio::test(start_paused = true)]
async fn a_hit_never_runs_the_loader() {
    let cache = cache();
    cache.insert("k".to_owned(), 1);
    let value = cache
        .get_with("k".to_owned(), async {
            unreachable!("the loader must not run on a hit")
        })
        .await;
    assert_eq!(value, 1);
}

#[test]
fn a_panicking_compute_closure_empties_its_shard_and_the_cache_carries_on() {
    let cache: Cache<u8, u8> = Cache::builder(10).shards(1).build();
    cache.insert(1, 1);
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        cache.compute(&2, |_| -> Compute<u8> { panic!("the closure panicked") })
    }));
    assert!(result.is_err());
    assert_eq!(
        cache.get(&1),
        None,
        "the shard was emptied, not left half updated"
    );
    cache.insert(3, 3);
    assert_eq!(cache.get(&3), Some(3));
    assert_eq!(cache.len(), 1);
}
