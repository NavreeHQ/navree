//! Where the per-IP limit and route throttles count: in each process, or in a
//! store every process of the app shares (`http.rate_limit_store`).

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_config::ConfigSources;
use navree_http::{MemoryRateLimitStore, RateLimitStore, RouteDef};
use navree_kernel::Factory;

use crate::support::{Setup, TestApp};

/// One process of an app: `/api/export` allows two requests a minute, and a
/// client four in all. With a `store`, the limits count in it, under the name
/// `shared`.
async fn process(store: Option<&Arc<MemoryRateLimitStore>>) -> TestApp {
    let mut sources = ConfigSources::new().set("http.per_ip_rate_limit", 4);
    if store.is_some() {
        sources = sources.set("http.rate_limit_store", "shared");
    }
    let store = store.cloned();
    Setup::new(vec![
        RouteDef::on(Method::GET, "/export", || async { "exported" })
            .public()
            .throttle(2, Duration::from_secs(60)),
        RouteDef::on(Method::GET, "/other", || async { "other" }).public(),
    ])
    .configured_by(sources)
    .register(move |m| {
        if let Some(store) = store.clone() {
            m.factory(
                Factory::<dyn RateLimitStore>::new(move |_| {
                    let store = store.clone();
                    async move { Ok(store as Arc<dyn RateLimitStore>) }
                })
                .named("shared"),
            );
        }
    })
    .boot()
    .await
}

/// Two processes behind a load balancer: a route throttle and the per-IP
/// limit count their requests together, so the limit is the limit and not
/// twice it.
#[tokio::test]
async fn processes_that_share_a_store_share_the_route_throttle_and_the_per_ip_limit() {
    let store = Arc::new(MemoryRateLimitStore::default());
    let one = process(Some(&store)).await;
    let two = process(Some(&store)).await;

    assert_eq!(one.get("/api/export").await.text(), "exported");
    assert_eq!(two.get("/api/export").await.text(), "exported");
    one.get("/api/export").await.assert_error(429);
    two.get("/api/export").await.assert_error(429);

    // Four requests from the client so far, two on each: its fifth is over
    // the per-IP limit, on either.
    let refused = one.get("/api/other").await;
    refused.assert_error(429);
    assert!(refused.header("retry-after").is_some());
}

/// Without a store each process counts on its own.
#[tokio::test]
async fn without_a_store_each_process_counts_on_its_own() {
    let one = process(None).await;
    let two = process(None).await;
    for app in [&one, &two] {
        assert_eq!(app.get("/api/export").await.text(), "exported");
        assert_eq!(app.get("/api/export").await.text(), "exported");
        app.get("/api/export").await.assert_error(429);
    }
    assert_eq!(one.get("/api/other").await.text(), "other");
}

/// A store that is not there is a mistake in the configuration: the app does
/// not boot, rather than count on its own unseen.
#[tokio::test]
async fn a_store_that_is_not_registered_fails_the_boot() {
    let error = Setup::new(vec![
        RouteDef::on(Method::GET, "/export", || async { "exported" }).public(),
    ])
    .configured_by(ConfigSources::new().set("http.rate_limit_store", "redis"))
    .boot_error()
    .await;
    assert!(error.contains("http.rate_limit_store"), "{error}");
    assert!(error.contains("RedisModule"), "{error}");
}
