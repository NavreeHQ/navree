//! `Locks`: atomic locks with owner tokens on the memory lock store.

use std::time::Duration;

use navis_cache::{CacheError, Locks};
use tokio::time::Instant;

const MINUTE: Duration = Duration::from_secs(60);

#[tokio::test]
async fn lock_exclusive_and_released_on_drop() {
    let locks = Locks::memory();
    let guard = locks.lock("invoices", MINUTE).acquire().await.unwrap();
    let guard = guard.expect("a free lock is acquired");
    assert_eq!(guard.name(), "invoices");

    let rival = locks.lock("invoices", MINUTE);
    assert!(
        rival.acquire().await.unwrap().is_none(),
        "the lock is exclusive"
    );
    assert!(
        locks
            .lock("other", MINUTE)
            .acquire()
            .await
            .unwrap()
            .is_some(),
        "other names are independent"
    );

    drop(guard);
    assert!(
        rival.acquire().await.unwrap().is_some(),
        "dropping the guard releases it"
    );
}

#[tokio::test(start_paused = true)]
async fn lock_expires_after_ttl() {
    let locks = Locks::memory();
    let stale = locks
        .lock("job", Duration::from_secs(10))
        .acquire()
        .await
        .unwrap()
        .unwrap();

    tokio::time::advance(Duration::from_secs(9)).await;
    assert!(locks.lock("job", MINUTE).acquire().await.unwrap().is_none());

    tokio::time::advance(Duration::from_secs(2)).await;
    let current = locks.lock("job", MINUTE).acquire().await.unwrap();
    assert!(current.is_some(), "an expired lock can be taken over");

    drop(stale);
    assert!(
        locks.lock("job", MINUTE).acquire().await.unwrap().is_none(),
        "the previous owner's guard must not release the new owner's lock"
    );
}

#[tokio::test]
async fn release_returns_whether_this_owner_held_the_lock() {
    let locks = Locks::memory();
    let guard = locks.lock("k", MINUTE).acquire().await.unwrap().unwrap();
    assert!(
        !locks.lock("k", MINUTE).release().await.unwrap(),
        "not the owner"
    );
    assert!(guard.release().await.unwrap());
    assert!(locks.lock("k", MINUTE).acquire().await.unwrap().is_some());
}

#[tokio::test]
async fn lock_can_be_released_elsewhere_with_its_owner_token() {
    let locks = Locks::memory();
    let owner = locks
        .lock("import", MINUTE)
        .acquire()
        .await
        .unwrap()
        .unwrap()
        .into_owner();
    assert!(
        locks
            .lock("import", MINUTE)
            .acquire()
            .await
            .unwrap()
            .is_none(),
        "into_owner keeps the lock held"
    );

    let restored = locks.lock("import", MINUTE).with_owner(owner.clone());
    assert_eq!(restored.owner(), owner);
    assert!(restored.release().await.unwrap());
    assert!(
        locks
            .lock("import", MINUTE)
            .acquire()
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test(start_paused = true)]
async fn block_waits_until_released() {
    let locks = Locks::memory();
    let guard = locks.lock("job", MINUTE).acquire().await.unwrap().unwrap();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        drop(guard);
    });

    let started = Instant::now();
    let guard = locks
        .lock("job", MINUTE)
        .block(Duration::from_secs(5))
        .await
        .unwrap();
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(guard.name(), "job");
}

#[tokio::test(start_paused = true)]
async fn block_times_out() {
    let locks = Locks::memory();
    let _held = locks.lock("job", MINUTE).acquire().await.unwrap().unwrap();

    let err = locks
        .lock("job", MINUTE)
        .block(Duration::from_secs(2))
        .await
        .unwrap_err();
    match err {
        CacheError::LockTimeout { name, waited } => {
            assert_eq!(name, "job");
            assert!(waited >= Duration::from_secs(2));
        }
        other => panic!("expected LockTimeout, got {other:?}"),
    }
}

#[tokio::test]
async fn owners_are_unique_per_lock() {
    let locks = Locks::memory();
    let a = locks.lock("k", MINUTE);
    let b = locks.lock("k", MINUTE);
    assert_ne!(a.owner(), b.owner());
    assert!(!a.owner().is_empty());
}

/// A store that only implements the required methods, so a dropped guard
/// goes through the default (spawned) `release_on_drop`.
struct AsyncOnly(navis_cache::MemoryLockStore);

impl navis_cache::LockStore for AsyncOnly {
    fn acquire<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
        ttl: Duration,
    ) -> navis_kernel::BoxFuture<'a, Result<bool, CacheError>> {
        self.0.acquire(name, owner, ttl)
    }

    fn release<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
    ) -> navis_kernel::BoxFuture<'a, Result<bool, CacheError>> {
        self.0.release(name, owner)
    }
}

#[tokio::test(start_paused = true)]
async fn dropped_guard_of_an_async_store_is_released_in_the_background() {
    let locks = Locks::new(
        std::sync::Arc::new(AsyncOnly(navis_cache::MemoryLockStore::new())),
        "app:",
    );
    drop(locks.lock("job", MINUTE).acquire().await.unwrap().unwrap());

    let guard = locks
        .lock("job", MINUTE)
        .block(Duration::from_secs(1))
        .await;
    assert!(guard.is_ok(), "the spawned release frees the lock");
}
