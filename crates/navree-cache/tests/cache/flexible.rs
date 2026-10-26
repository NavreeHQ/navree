//! `Cache::flexible`: stale-while-revalidate.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_cache::Cache;

const FRESH: Duration = Duration::from_secs(10);
const STALE: Duration = Duration::from_secs(60);

/// Polls `check` (letting background tasks run) until it holds.
async fn eventually<F, Fut>(mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    // Up to 5 s of (paused) time; every window in these tests is longer.
    for _ in 0..500 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition never became true");
}

/// A computation that counts its calls and returns `value`.
fn counted(
    calls: &Arc<AtomicUsize>,
    value: u32,
) -> impl FnOnce()
    -> std::pin::Pin<Box<dyn Future<Output = Result<u32, navree_kernel::BoxError>> + Send>>
+ Send
+ 'static {
    let calls = calls.clone();
    move || {
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        })
    }
}

#[tokio::test(start_paused = true)]
async fn flexible_serves_stale_and_refreshes() {
    let cache = Cache::memory();
    let calls = Arc::new(AtomicUsize::new(0));

    assert_eq!(
        cache
            .flexible("k", FRESH, STALE, counted(&calls, 1))
            .await
            .unwrap(),
        1
    );

    // Fresh: served from the cache.
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(
        cache
            .flexible("k", FRESH, STALE, counted(&calls, 99))
            .await
            .unwrap(),
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Stale: the old value is served at once and refreshed in the background.
    tokio::time::advance(Duration::from_secs(10)).await;
    assert_eq!(
        cache
            .flexible("k", FRESH, STALE, counted(&calls, 2))
            .await
            .unwrap(),
        1
    );
    eventually(|| async { cache.get::<u32>("k").await.unwrap() == Some(2) }).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // The refreshed value is fresh again.
    assert_eq!(
        cache
            .flexible("k", FRESH, STALE, counted(&calls, 3))
            .await
            .unwrap(),
        2
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn flexible_recomputes_inline_after_the_stale_window() {
    let cache = Cache::memory();
    let calls = Arc::new(AtomicUsize::new(0));
    cache
        .flexible("k", FRESH, STALE, counted(&calls, 1))
        .await
        .unwrap();

    tokio::time::advance(FRESH + STALE + Duration::from_secs(1)).await;
    assert_eq!(
        cache
            .flexible("k", FRESH, STALE, counted(&calls, 2))
            .await
            .unwrap(),
        2
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn flexible_starts_one_refresh_at_a_time() {
    let cache = Cache::memory();
    cache
        .flexible("k", FRESH, STALE, || async { Ok(1u32) })
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(15)).await;

    let refreshes = Arc::new(AtomicUsize::new(0));
    for _ in 0..3 {
        let refreshes = refreshes.clone();
        let served = cache
            .flexible("k", FRESH, STALE, move || async move {
                refreshes.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok(2u32)
            })
            .await
            .unwrap();
        assert_eq!(served, 1);
    }
    eventually(|| async { cache.get::<u32>("k").await.unwrap() == Some(2) }).await;
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn failed_refresh_keeps_the_stale_value_and_retries_later() {
    let cache = Cache::memory();
    cache
        .flexible("k", FRESH, STALE, || async { Ok(1u32) })
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(15)).await;

    let attempts = Arc::new(AtomicUsize::new(0));
    let failing = {
        let attempts = attempts.clone();
        move || async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err::<u32, _>("upstream down".into())
        }
    };
    assert_eq!(cache.flexible("k", FRESH, STALE, failing).await.unwrap(), 1);
    eventually(|| async { attempts.load(Ordering::SeqCst) == 1 }).await;
    assert_eq!(cache.get::<u32>("k").await.unwrap(), Some(1));

    // The failed refresh released its claim, so the next stale read retries.
    let calls = Arc::new(AtomicUsize::new(0));
    eventually(|| {
        let cache = cache.clone();
        let calls = calls.clone();
        async move {
            cache
                .flexible("k", FRESH, STALE, counted(&calls, 3))
                .await
                .unwrap();
            cache.get::<u32>("k").await.unwrap() == Some(3)
        }
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn tagged_flexible_is_flushed_with_its_tag() {
    let cache = Cache::memory();
    let reports = cache.tags(&["reports"]);
    let first: u32 = reports
        .flexible("k", FRESH, STALE, || async { Ok(1) })
        .await
        .unwrap();
    reports.flush().await.unwrap();
    let second: u32 = reports
        .flexible("k", FRESH, STALE, || async { Ok(2) })
        .await
        .unwrap();
    assert_eq!((first, second), (1, 2));
}
