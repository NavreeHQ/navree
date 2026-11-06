//! `Cache`: typed values, TTLs, the key prefix, `add`, counters and `remember`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use navree_cache::{Cache, CacheError, CacheStore, MemoryStore, Ttl};
use serde::{Deserialize, Serialize};

const MINUTE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct User {
    id: u32,
    name: String,
}

fn ada() -> User {
    User {
        id: 1,
        name: "Anu".into(),
    }
}

#[tokio::test(start_paused = true)]
async fn put_get_ttl_expiry() {
    let cache = Cache::memory();
    cache.put("user:1", &ada(), MINUTE).await.unwrap();

    assert_eq!(cache.get::<User>("user:1").await.unwrap(), Some(ada()));
    assert!(cache.has("user:1").await.unwrap());

    tokio::time::advance(Duration::from_secs(59)).await;
    assert_eq!(cache.get::<User>("user:1").await.unwrap(), Some(ada()));

    tokio::time::advance(Duration::from_secs(2)).await;
    assert_eq!(cache.get::<User>("user:1").await.unwrap(), None);
    assert!(!cache.has("user:1").await.unwrap());
}

#[tokio::test(start_paused = true)]
async fn default_ttl_and_forever() {
    let cache = Cache::new(Arc::new(MemoryStore::new()), "", Duration::from_secs(10));
    cache.put("default", &1, Ttl::Default).await.unwrap();
    cache.put("forever", &2, Ttl::Forever).await.unwrap();
    cache.forever("also-forever", &3).await.unwrap();

    tokio::time::advance(Duration::from_secs(11)).await;
    assert_eq!(cache.get::<i32>("default").await.unwrap(), None);
    assert_eq!(cache.get::<i32>("forever").await.unwrap(), Some(2));
    assert_eq!(cache.get::<i32>("also-forever").await.unwrap(), Some(3));
}

#[tokio::test]
async fn zero_ttl_forgets_the_key() {
    let cache = Cache::memory();
    cache.put("k", &1, MINUTE).await.unwrap();
    cache.put("k", &2, Duration::ZERO).await.unwrap();
    assert_eq!(cache.get::<i32>("k").await.unwrap(), None);
}

#[tokio::test]
async fn prefix_is_applied_to_store_keys() {
    let store = Arc::new(MemoryStore::new());
    let cache = Cache::new(store.clone(), "app:", MINUTE);
    cache.put("greeting", "hello", MINUTE).await.unwrap();

    assert_eq!(
        store.get("app:greeting").await.unwrap(),
        Some(Bytes::from_static(b"\"hello\""))
    );
    assert_eq!(store.get("greeting").await.unwrap(), None);
    assert_eq!(cache.prefix(), "app:");

    assert!(cache.forget("greeting").await.unwrap());
    assert_eq!(store.get("app:greeting").await.unwrap(), None);
}

#[tokio::test]
async fn add_only_if_absent() {
    let cache = Cache::memory();
    assert!(cache.add("k", &1, MINUTE).await.unwrap());
    assert!(!cache.add("k", &2, MINUTE).await.unwrap());
    assert_eq!(cache.get::<i32>("k").await.unwrap(), Some(1));

    cache.forget("k").await.unwrap();
    assert!(cache.add("k", &3, MINUTE).await.unwrap());
    assert_eq!(cache.get::<i32>("k").await.unwrap(), Some(3));
}

#[tokio::test]
async fn increment_and_decrement_counters() {
    let cache = Cache::memory();
    assert_eq!(cache.increment("hits", 1).await.unwrap(), 1);
    assert_eq!(cache.increment("hits", 4).await.unwrap(), 5);
    assert_eq!(cache.decrement("hits", 2).await.unwrap(), 3);
    assert_eq!(cache.get::<i64>("hits").await.unwrap(), Some(3));

    let err = cache.decrement("hits", i64::MIN).await.unwrap_err();
    assert!(matches!(err, CacheError::Overflow { .. }), "{err:?}");
}

#[tokio::test]
async fn pull_returns_and_removes() {
    let cache = Cache::memory();
    cache.put("once", &ada(), MINUTE).await.unwrap();
    assert_eq!(cache.pull::<User>("once").await.unwrap(), Some(ada()));
    assert_eq!(cache.pull::<User>("once").await.unwrap(), None);
}

#[tokio::test]
async fn flush_clears_everything() {
    let cache = Cache::memory();
    cache.put("a", &1, MINUTE).await.unwrap();
    cache.forever("b", &2).await.unwrap();
    cache.flush().await.unwrap();
    assert!(!cache.has("a").await.unwrap());
    assert!(!cache.has("b").await.unwrap());
}

#[tokio::test]
async fn get_reports_values_of_another_type() {
    let cache = Cache::memory();
    cache.put("k", "text", MINUTE).await.unwrap();
    let err = cache.get::<u32>("k").await.unwrap_err();
    assert!(
        matches!(err, CacheError::Deserialize { ref key, .. } if key == "k"),
        "{err:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn remember_computes_once() {
    let cache = Cache::memory();
    let calls = Arc::new(AtomicUsize::new(0));

    let mut callers = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let cache = cache.clone();
        let calls = calls.clone();
        callers.spawn(async move {
            cache
                .remember("report", MINUTE, || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Ok(ada())
                })
                .await
        });
    }
    while let Some(result) = callers.join_next().await {
        assert_eq!(result.unwrap().unwrap(), ada());
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "concurrent callers share one computation"
    );

    let again = cache
        .remember("report", MINUTE, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(ada())
        })
        .await
        .unwrap();
    assert_eq!(again, ada());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a cached value is not recomputed"
    );
}

#[tokio::test(start_paused = true)]
async fn remember_recomputes_after_expiry() {
    let cache = Cache::memory();
    let first: u32 = cache
        .remember("n", MINUTE, || async { Ok(1) })
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(61)).await;
    let second: u32 = cache
        .remember("n", MINUTE, || async { Ok(2) })
        .await
        .unwrap();
    assert_eq!((first, second), (1, 2));
}

#[tokio::test]
async fn remember_does_not_cache_errors() {
    let cache = Cache::memory();
    let err = cache
        .remember::<u32, _, _>("flaky", MINUTE, || async { Err("upstream down".into()) })
        .await
        .unwrap_err();
    match err {
        CacheError::Compute { key, source } => {
            assert_eq!(key, "flaky");
            assert_eq!(source.to_string(), "upstream down");
        }
        other => panic!("expected Compute, got {other:?}"),
    }
    assert!(!cache.has("flaky").await.unwrap());

    let value: u32 = cache
        .remember("flaky", MINUTE, || async { Ok(7) })
        .await
        .unwrap();
    assert_eq!(value, 7);
}

#[tokio::test]
async fn remember_replaces_values_it_cannot_decode() {
    let store = Arc::new(MemoryStore::new());
    store
        .put("shape", Bytes::from_static(b"{\"old\":true}"), None)
        .await
        .unwrap();
    let cache = Cache::new(store, "", MINUTE);

    let user = cache
        .remember("shape", MINUTE, || async { Ok(ada()) })
        .await
        .unwrap();
    assert_eq!(user, ada());
    assert_eq!(cache.get::<User>("shape").await.unwrap(), Some(ada()));
}

#[tokio::test]
async fn counter_errors_name_the_store_key() {
    let cache = Cache::new(Arc::new(MemoryStore::new()), "app:", MINUTE);
    cache.put("n", &i64::MAX, MINUTE).await.unwrap();

    let up = cache.increment("n", 1).await.unwrap_err();
    assert!(
        matches!(up, CacheError::Overflow { ref key } if key == "app:n"),
        "{up:?}"
    );
    let down = cache.decrement("n", i64::MIN).await.unwrap_err();
    assert!(
        matches!(down, CacheError::Overflow { ref key } if key == "app:n"),
        "{down:?}"
    );

    let tagged = cache.tags(&["t"]);
    let err = tagged.decrement("n", i64::MIN).await.unwrap_err();
    assert!(
        matches!(err, CacheError::Overflow { ref key } if key.starts_with("app:navree:tagged:")),
        "{err:?}"
    );
}
