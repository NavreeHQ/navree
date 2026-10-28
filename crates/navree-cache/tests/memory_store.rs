//! The bounded memory store: its capacity, eviction, expiry and the
//! atomicity of `add` and `increment` under many tasks.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use navree_cache::{Cache, CacheStore, DEFAULT_MAX_ENTRIES, MemoryStore};

fn bytes(text: &str) -> Bytes {
    Bytes::copy_from_slice(text.as_bytes())
}

#[tokio::test]
async fn a_store_never_holds_more_than_its_capacity() {
    let store = MemoryStore::with_capacity(100);
    assert_eq!(store.capacity(), 100);
    for i in 0..5_000 {
        store
            .put(&format!("key:{i}"), bytes("v"), None)
            .await
            .unwrap();
        assert!(store.len() <= 100, "{} entries after {i} puts", store.len());
    }
    assert!(
        store.len() > 50,
        "the store stays mostly full: {}",
        store.len()
    );
    assert!(
        store.get("key:4999").await.unwrap().is_some(),
        "the newest key is kept"
    );
    assert!(
        store.get("key:0").await.unwrap().is_none(),
        "the oldest unread key went"
    );
}

#[tokio::test]
async fn the_default_store_is_bounded_too() {
    assert_eq!(MemoryStore::new().capacity(), DEFAULT_MAX_ENTRIES);
    assert_eq!(MemoryStore::default().capacity(), DEFAULT_MAX_ENTRIES);
}

#[tokio::test]
async fn entries_that_are_read_outlive_a_burst_of_one_off_keys() {
    let store = MemoryStore::with_capacity(100);
    for i in 0..20 {
        store
            .put(&format!("hot:{i}"), bytes("v"), None)
            .await
            .unwrap();
    }
    for i in 0..2_000 {
        store
            .put(&format!("scan:{i}"), bytes("v"), None)
            .await
            .unwrap();
        // The app keeps reading its hot keys while the burst goes by.
        let hot = format!("hot:{}", i % 20);
        assert!(
            store.get(&hot).await.unwrap().is_some(),
            "{hot} lost at {i}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn expired_entries_are_gone_and_are_reclaimed() {
    let store = MemoryStore::with_capacity(1_000);
    for i in 0..50 {
        store
            .put(
                &format!("short:{i}"),
                bytes("v"),
                Some(Duration::from_secs(10)),
            )
            .await
            .unwrap();
    }
    store.put("forever", bytes("v"), None).await.unwrap();
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(store.get("short:3").await.unwrap().is_none());
    assert!(store.get("forever").await.unwrap().is_some());
    assert_eq!(store.len(), 50, "one read removed the one entry it found");
    let mut removed = 0;
    for _ in 0..10 {
        removed += store.sweep(100);
    }
    assert_eq!(removed, 49);
    assert_eq!(store.len(), 1);
    assert!(!store.is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_expired_key_can_be_added_again() {
    let store = MemoryStore::new();
    assert!(
        store
            .add("once", bytes("a"), Some(Duration::from_secs(5)))
            .await
            .unwrap()
    );
    assert!(!store.add("once", bytes("b"), None).await.unwrap());
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(store.add("once", bytes("c"), None).await.unwrap());
    assert_eq!(store.get("once").await.unwrap(), Some(bytes("c")));
}

#[tokio::test(start_paused = true)]
async fn increment_keeps_the_ttl_of_an_existing_counter() {
    let store = MemoryStore::new();
    store
        .put("hits", bytes("5"), Some(Duration::from_secs(10)))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(6)).await;
    assert_eq!(store.increment("hits", 1).await.unwrap(), 6);
    tokio::time::advance(Duration::from_secs(4)).await;
    assert_eq!(
        store.get("hits").await.unwrap(),
        None,
        "the increment did not extend the TTL"
    );
    assert_eq!(
        store.increment("hits", 1).await.unwrap(),
        1,
        "a new counter starts from zero"
    );
    tokio::time::advance(Duration::from_secs(3_600)).await;
    assert_eq!(
        store.increment("hits", 1).await.unwrap(),
        2,
        "a counter that increment created never expires"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_increments_are_atomic() {
    let store = Arc::new(MemoryStore::new());
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..2_000 {
                store.increment("counter", 1).await.unwrap();
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(store.get("counter").await.unwrap(), Some(bytes("32000")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn exactly_one_concurrent_add_wins() {
    for round in 0..50 {
        let store = Arc::new(MemoryStore::new());
        let key = format!("lock:{round}");
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let (store, key) = (store.clone(), key.clone());
            tasks.push(tokio::spawn(async move {
                store.add(&key, bytes("owner"), None).await.unwrap()
            }));
        }
        let mut winners = 0;
        for task in tasks {
            winners += usize::from(task.await.unwrap());
        }
        assert_eq!(winners, 1, "round {round}");
    }
}

#[tokio::test]
async fn forget_reports_whether_a_live_value_was_removed() {
    let store = MemoryStore::new();
    store.put("a", bytes("1"), None).await.unwrap();
    assert!(store.forget("a").await.unwrap());
    assert!(!store.forget("a").await.unwrap());
    store.put("b", bytes("2"), None).await.unwrap();
    store.flush().await.unwrap();
    assert!(store.is_empty());
}

#[tokio::test]
async fn a_cache_over_a_small_store_still_works() {
    let cache = Cache::new(
        Arc::new(MemoryStore::with_capacity(10)),
        "app:",
        Duration::from_secs(60),
    );
    for i in 0..100u32 {
        cache
            .put(&format!("n{i}"), &i, Duration::from_secs(60))
            .await
            .unwrap();
    }
    assert_eq!(cache.get::<u32>("n99").await.unwrap(), Some(99));
    let total = cache
        .remember("computed", Duration::from_secs(60), || async {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(7u32)
        })
        .await
        .unwrap();
    assert_eq!(total, 7);
}
