//! `RateLimiter` and the HTTP server's `RedisRateLimitStore` on a real Redis
//! server (`NAVREE_TEST_REDIS_URL`).

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_config::{ConfigModule, ConfigSources};
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RateLimitStore, RouteDef};
use navree_kernel::{
    App, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey, Resolver,
};
use navree_redis::{DRIVER, RateLimit, RateLimiter, RedisModule, RedisRateLimitStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::support::TestRedis;

#[tokio::test]
async fn a_burst_then_one_request_per_interval() {
    let Some(t) = TestRedis::connect("a_burst_then_one_request_per_interval").await else {
        return;
    };
    let limiter = RateLimiter::new(t.redis.connection().clone(), &t.prefix);
    // Two processes share the count.
    let other = RateLimiter::new(t.redis.connection().clone(), &t.prefix);
    let minute = Duration::from_secs(60);

    assert_eq!(
        limiter.hit("exports:anu", 2, minute).await.unwrap(),
        RateLimit::Allowed { remaining: 1 }
    );
    assert_eq!(
        other.hit("exports:anu", 2, minute).await.unwrap(),
        RateLimit::Allowed { remaining: 0 }
    );
    let RateLimit::Limited { retry_after } = limiter.hit("exports:anu", 2, minute).await.unwrap()
    else {
        panic!("the third request is over the limit");
    };
    assert!(
        retry_after > Duration::from_secs(29) && retry_after <= Duration::from_secs(30),
        "{retry_after:?}"
    );
    assert!(
        !limiter
            .hit("exports:anu", 2, minute)
            .await
            .unwrap()
            .is_allowed()
    );
    assert!(
        limiter
            .hit("exports:bob", 2, minute)
            .await
            .unwrap()
            .is_allowed(),
        "keys are independent"
    );

    limiter.reset("exports:anu").await.unwrap();
    assert!(
        limiter
            .hit("exports:anu", 2, minute)
            .await
            .unwrap()
            .is_allowed()
    );

    // The window refills one request per interval.
    let fast = Duration::from_millis(200);
    assert!(limiter.hit("fast", 1, fast).await.unwrap().is_allowed());
    assert!(!limiter.hit("fast", 1, fast).await.unwrap().is_allowed());
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(limiter.hit("fast", 1, fast).await.unwrap().is_allowed());
    t.cleanup().await;
}

#[tokio::test]
async fn the_http_store_counts_every_process_together_and_reports_a_flood_once() {
    let Some(t) =
        TestRedis::connect("the_http_store_counts_every_process_together_and_reports_a_flood_once")
            .await
    else {
        return;
    };
    let one = RedisRateLimitStore::new(t.redis.connection().clone(), &t.prefix);
    let two = RedisRateLimitStore::new(t.redis.connection().clone(), &t.prefix);
    let minute = Duration::from_secs(60);
    let key = "http:ip:192.0.2.1";

    assert!(one.hit(key, 2, minute).await.unwrap().is_ok());
    assert!(two.hit(key, 2, minute).await.unwrap().is_ok());
    let limited = one.hit(key, 2, minute).await.unwrap().unwrap_err();
    assert!(limited.report, "the first refusal is reported");
    assert!(
        limited.retry_after > Duration::from_secs(29)
            && limited.retry_after <= Duration::from_secs(30),
        "{limited:?}"
    );
    let again = two.hit(key, 2, minute).await.unwrap().unwrap_err();
    assert!(!again.report, "once a window, whichever process refuses");
    assert!(
        one.hit("http:ip:192.0.2.2", 2, minute)
            .await
            .unwrap()
            .is_ok(),
        "keys are independent"
    );
    let keys = t.keys().await;
    assert!(keys.contains(&one.redis_key(key)), "{keys:?}");

    // The window refills one request per interval.
    let fast = Duration::from_millis(200);
    assert!(one.hit("http:fast", 1, fast).await.unwrap().is_ok());
    assert!(two.hit("http:fast", 1, fast).await.unwrap().is_err());
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(two.hit("http:fast", 1, fast).await.unwrap().is_ok());
    t.cleanup().await;
}

/// The HTTP server's limits keep to keys of their own: an app's limit named
/// like one of them neither counts with it nor resets it.
#[tokio::test]
async fn the_http_store_and_the_apps_own_limits_keep_apart() {
    let Some(t) = TestRedis::connect("the_http_store_and_the_apps_own_limits_keep_apart").await
    else {
        return;
    };
    let store = RedisRateLimitStore::new(t.redis.connection().clone(), &t.prefix);
    let limiter = RateLimiter::new(t.redis.connection().clone(), &t.prefix);
    let minute = Duration::from_secs(60);
    let key = "http:ip:192.0.2.1";
    assert_ne!(store.redis_key(key), limiter.redis_key(key));

    assert!(store.hit(key, 1, minute).await.unwrap().is_ok());
    assert!(
        limiter.hit(key, 1, minute).await.unwrap().is_allowed(),
        "not counted together"
    );
    limiter.reset(key).await.unwrap();
    assert!(
        store.hit(key, 1, minute).await.unwrap().is_err(),
        "not reset by the app"
    );
    t.cleanup().await;
}

/// `GET /export`, two a minute, and `GET /other`.
struct Exports;

impl Injectable for Exports {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Exports) })
    }
}

impl Controller for Exports {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/export", || async { "exported" })
                .public()
                .throttle(2, Duration::from_secs(60)),
            RouteDef::on(Method::GET, "/other", || async { "other" }).public(),
        ]
    }
}

struct ExportsModule;

impl Module for ExportsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Exports>();
    }
}

/// One `serve` process of an app whose limits count in Redis, on
/// 127.0.0.1:0: a client may send four requests a minute.
async fn process(t: &TestRedis) -> (App, SocketAddr) {
    let sources = ConfigSources::new()
        .set("http.rate_limit_store", DRIVER)
        .set("http.per_ip_rate_limit", 4);
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(RedisModule::for_root(t.options()))
        .module(HttpModule::from_config())
        .module(ExportsModule)
        .boot()
        .await
        .expect("app boots");
    let router = navree_http::build_router(app.ctx())
        .await
        .expect("router builds");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(
        navree_http::serve(listener, router)
            .with_graceful_shutdown(app.ctx().shutdown_token().cancelled_owned())
            .into_future(),
    );
    (app, addr)
}

/// The status line of `GET {path}` on a new connection.
async fn status(addr: SocketAddr, path: &str) -> String {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut answer = String::new();
    socket.read_to_string(&mut answer).await.unwrap();
    answer.lines().next().unwrap_or_default().to_owned()
}

/// Two processes behind a load balancer share one route throttle and one
/// per-IP limit through Redis.
#[tokio::test]
async fn two_apps_share_one_route_throttle_and_the_per_ip_limit() {
    let Some(t) =
        TestRedis::connect("two_apps_share_one_route_throttle_and_the_per_ip_limit").await
    else {
        return;
    };
    let (one, at_one) = process(&t).await;
    let (two, at_two) = process(&t).await;
    one.ctx()
        .get_named::<dyn RateLimitStore>(DRIVER)
        .await
        .expect("RedisModule registers the store as `redis`");

    assert_eq!(status(at_one, "/api/export").await, "HTTP/1.1 200 OK");
    assert_eq!(status(at_two, "/api/export").await, "HTTP/1.1 200 OK");
    assert!(
        status(at_one, "/api/export")
            .await
            .starts_with("HTTP/1.1 429")
    );
    assert!(
        status(at_two, "/api/export")
            .await
            .starts_with("HTTP/1.1 429")
    );
    // Four requests from 127.0.0.1 in all: the fifth is over the per-IP limit.
    assert!(
        status(at_one, "/api/other")
            .await
            .starts_with("HTTP/1.1 429")
    );

    one.shutdown().await.unwrap();
    two.shutdown().await.unwrap();
    t.cleanup().await;
}
