//! Telemetry: cache hits, misses, writes and forgets as `CacheEvent`s.

use std::sync::Arc;
use std::time::Duration;

use navree_cache::{Cache, CacheModule, CacheOptions, MemoryStore};
use navree_kernel::Navree;
use navree_telemetry::{CacheOperation, Telemetry, TelemetryRecorder};

fn recorded_cache(prefix: &str) -> (Cache, TelemetryRecorder) {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let cache = Cache::new(
        Arc::new(MemoryStore::new()),
        prefix,
        Duration::from_secs(60),
    )
    .with_telemetry(telemetry, "memory");
    (cache, recorder)
}

fn operations(recorder: &TelemetryRecorder) -> Vec<(CacheOperation, String)> {
    recorder
        .cache_events()
        .into_iter()
        .map(|event| (event.operation, event.key))
        .collect()
}

#[tokio::test]
async fn reads_writes_and_forgets_are_reported_with_the_callers_key() {
    let (cache, recorder) = recorded_cache("app:");
    cache
        .put("stats", &1, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(cache.get::<u32>("stats").await.unwrap(), Some(1));
    assert_eq!(cache.get::<u32>("missing").await.unwrap(), None);
    assert!(cache.has("stats").await.unwrap());
    cache.forget("stats").await.unwrap();
    assert!(
        cache
            .add("fresh", &2, Duration::from_secs(60))
            .await
            .unwrap()
    );
    assert!(
        !cache
            .add("fresh", &3, Duration::from_secs(60))
            .await
            .unwrap()
    );
    cache.increment("hits", 1).await.unwrap();

    use CacheOperation::{Forget, Hit, Miss, Write};
    assert_eq!(
        operations(&recorder),
        [
            (Write, "stats".to_owned()),
            (Hit, "stats".to_owned()),
            (Miss, "missing".to_owned()),
            (Hit, "stats".to_owned()),
            (Forget, "stats".to_owned()),
            (Write, "fresh".to_owned()),
            (Write, "hits".to_owned()),
        ]
    );
    assert_eq!(recorder.cache_events()[0].store, "memory");
}

#[tokio::test]
async fn remember_reports_one_miss_and_a_write_then_hits() {
    let (cache, recorder) = recorded_cache("");
    let ttl = Duration::from_secs(60);
    let first: u32 = cache
        .remember("total", ttl, || async { Ok(42) })
        .await
        .unwrap();
    let second: u32 = cache
        .remember("total", ttl, || async { Ok(0) })
        .await
        .unwrap();
    assert_eq!((first, second), (42, 42));
    use CacheOperation::{Hit, Miss, Write};
    assert_eq!(
        operations(&recorder),
        [
            (Miss, "total".to_owned()),
            (Write, "total".to_owned()),
            (Hit, "total".to_owned()),
        ]
    );
}

#[tokio::test]
async fn flexible_and_pull_are_reported() {
    let (cache, recorder) = recorded_cache("");
    let window = Duration::from_secs(60);
    let _: u32 = cache
        .flexible("rates", window, window, || async { Ok(1) })
        .await
        .unwrap();
    let _: u32 = cache
        .flexible("rates", window, window, || async { Ok(2) })
        .await
        .unwrap();
    let pulled: Option<u32> = cache.pull("rates").await.unwrap();
    assert_eq!(pulled, Some(1));
    use CacheOperation::{Forget, Hit, Miss, Write};
    assert_eq!(
        operations(&recorder),
        [
            (Miss, "rates".to_owned()),
            (Write, "rates".to_owned()),
            (Hit, "rates".to_owned()),
            (Hit, "rates".to_owned()),
            (Forget, "rates".to_owned()),
        ]
    );
}

#[tokio::test]
async fn tagged_calls_are_reported() {
    let (cache, recorder) = recorded_cache("");
    let users = cache.tags(&["users"]);
    users
        .put("u:1", &"Anu", Duration::from_secs(60))
        .await
        .unwrap();
    let _: Option<String> = users.get("u:1").await.unwrap();
    users.forget("u:1").await.unwrap();
    use CacheOperation::{Forget, Hit, Write};
    assert_eq!(
        operations(&recorder),
        [
            (Write, "u:1".to_owned()),
            (Hit, "u:1".to_owned()),
            (Forget, "u:1".to_owned()),
        ]
    );
}

#[tokio::test]
async fn a_cache_without_telemetry_reports_nothing() {
    let recorder = TelemetryRecorder::new();
    let cache = Cache::memory();
    cache.put("k", &1, Duration::from_secs(1)).await.unwrap();
    assert!(recorder.is_empty());
    assert!(!cache.telemetry().is_active());
}

#[tokio::test]
async fn the_module_cache_reports_to_the_app() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(CacheModule::for_root(CacheOptions::default()))
        .module(recorder.module())
        .boot()
        .await
        .unwrap();
    let cache = app.ctx().get::<Cache>().await.unwrap();
    cache.put("k", &1, Duration::from_secs(1)).await.unwrap();
    assert_eq!(recorder.cache_events()[0].store, "memory");
    app.shutdown().await.unwrap();
}
