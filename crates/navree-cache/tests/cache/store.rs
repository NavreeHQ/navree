//! The `CacheStore` contract, exercised against `MemoryStore`.

use std::time::Duration;

use bytes::Bytes;
use navree_cache::{CacheError, CacheStore, MemoryStore};

const MINUTE: Duration = Duration::from_secs(60);

fn bytes(s: &str) -> Bytes {
    Bytes::copy_from_slice(s.as_bytes())
}

#[tokio::test(start_paused = true)]
async fn memory_store_entries_expire_after_their_ttl() {
    let store = MemoryStore::new();
    store.put("a", bytes("1"), Some(MINUTE)).await.unwrap();
    store.put("forever", bytes("2"), None).await.unwrap();

    tokio::time::advance(Duration::from_secs(59)).await;
    assert_eq!(store.get("a").await.unwrap(), Some(bytes("1")));

    tokio::time::advance(Duration::from_secs(2)).await;
    assert_eq!(store.get("a").await.unwrap(), None);
    assert_eq!(store.get("forever").await.unwrap(), Some(bytes("2")));
}

#[tokio::test(start_paused = true)]
async fn memory_store_add_only_if_absent_or_expired() {
    let store = MemoryStore::new();
    assert!(store.add("k", bytes("first"), Some(MINUTE)).await.unwrap());
    assert!(!store.add("k", bytes("second"), Some(MINUTE)).await.unwrap());
    assert_eq!(store.get("k").await.unwrap(), Some(bytes("first")));

    tokio::time::advance(Duration::from_secs(61)).await;
    assert!(store.add("k", bytes("third"), None).await.unwrap());
    assert_eq!(store.get("k").await.unwrap(), Some(bytes("third")));
}

#[tokio::test(start_paused = true)]
async fn memory_store_increment_starts_at_zero_and_keeps_ttl() {
    let store = MemoryStore::new();
    assert_eq!(store.increment("fresh", 3).await.unwrap(), 3);
    assert_eq!(store.increment("fresh", -5).await.unwrap(), -2);
    assert_eq!(store.get("fresh").await.unwrap(), Some(bytes("-2")));

    store.put("hits", bytes("10"), Some(MINUTE)).await.unwrap();
    tokio::time::advance(Duration::from_secs(30)).await;
    assert_eq!(store.increment("hits", 1).await.unwrap(), 11);
    tokio::time::advance(Duration::from_secs(31)).await;
    assert_eq!(
        store.get("hits").await.unwrap(),
        None,
        "the original TTL still applies"
    );
}

#[tokio::test]
async fn memory_store_increment_rejects_non_integers_and_overflow() {
    let store = MemoryStore::new();
    store.put("name", bytes("\"navree\""), None).await.unwrap();
    let err = store.increment("name", 1).await.unwrap_err();
    assert!(
        matches!(err, CacheError::NotAnInteger { ref key } if key == "name"),
        "{err:?}"
    );

    store
        .put("big", bytes(&i64::MAX.to_string()), None)
        .await
        .unwrap();
    let err = store.increment("big", 1).await.unwrap_err();
    assert!(
        matches!(err, CacheError::Overflow { ref key } if key == "big"),
        "{err:?}"
    );
    assert_eq!(
        store.get("big").await.unwrap(),
        Some(bytes(&i64::MAX.to_string()))
    );
}

#[tokio::test]
async fn memory_store_forget_and_flush() {
    let store = MemoryStore::new();
    store.put("a", bytes("1"), None).await.unwrap();
    store.put("b", bytes("2"), None).await.unwrap();

    assert!(store.forget("a").await.unwrap());
    assert!(!store.forget("a").await.unwrap());
    assert_eq!(store.get("a").await.unwrap(), None);

    store.flush().await.unwrap();
    assert_eq!(store.get("b").await.unwrap(), None);
}

#[tokio::test]
async fn memory_store_is_usable_as_a_trait_object() {
    let store: std::sync::Arc<dyn CacheStore> = std::sync::Arc::new(MemoryStore::new());
    store.put("k", bytes("v"), Some(MINUTE)).await.unwrap();
    assert_eq!(store.get("k").await.unwrap(), Some(bytes("v")));
}

#[tokio::test]
async fn memory_store_flush_removes_entries_written_just_before() {
    let store = MemoryStore::new();
    for i in 0..20_000 {
        store.put("k", bytes("1"), None).await.unwrap();
        store.flush().await.unwrap();
        assert_eq!(store.get("k").await.unwrap(), None, "iteration {i}");
    }
}
