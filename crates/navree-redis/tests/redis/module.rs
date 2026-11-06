//! `RedisModule` wiring, without a server: options at boot, the providers
//! the other modules select by name, and the health check of a server that
//! does not answer.

use std::sync::Arc;

use navree_cache::{CacheModule, CacheOptions, CacheStore, CacheStoreKind, LockStore};
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{HealthCheck, HealthStatus, Secret, SessionStore};
use navree_http::{Authenticator, HttpModule, HttpOptions};
use navree_kernel::{App, Navree};
use navree_queue::{Queue, QueueDriver, QueueModule, QueueOptions, QueueStore};
use navree_realtime::{Backplane, BackplaneKind, Realtime, RealtimeModule, RealtimeOptions};
use navree_redis::{DRIVER, RateLimiter, Redis, RedisModule, RedisOptions};
use navree_session::{SessionModule, SessionOptions, SessionStoreKind};
use serde_json::json;

use crate::support::{NoAuthenticator, UNREACHABLE};

fn unreachable() -> RedisOptions {
    RedisOptions {
        url: UNREACHABLE.to_owned(),
        connect_timeout_ms: 200,
        response_timeout_ms: 200,
        ..RedisOptions::default()
    }
}

/// Whether two `Arc`s of a trait object point at the same value.
fn same<T: ?Sized>(a: &Arc<T>, b: &Arc<T>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

async fn boot_everything() -> App {
    Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(RedisModule::for_root(unreachable()))
        .module(CacheModule::for_root(CacheOptions {
            store: CacheStoreKind::Redis,
            ..CacheOptions::default()
        }))
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Redis,
            ..QueueOptions::default()
        }))
        .module(navree_crypto::CryptoModule)
        .module(HttpModule::for_root(HttpOptions::default()))
        .module(SessionModule::for_root(SessionOptions {
            store: SessionStoreKind::Redis,
            ..SessionOptions::default()
        }))
        .module(RealtimeModule::for_root(RealtimeOptions {
            secret: Secret::new("shared".to_owned()),
            backplane: BackplaneKind::Redis,
            ..RealtimeOptions::default()
        }))
        .override_value::<dyn Authenticator>(Arc::new(NoAuthenticator))
        .boot()
        .await
        .expect("the app boots while Redis is down")
}

#[tokio::test]
async fn the_other_modules_select_the_redis_drivers_by_name() {
    let app = boot_everything().await;
    let ctx = app.ctx();

    let named = ctx.get_named::<dyn CacheStore>(DRIVER).await.unwrap();
    assert!(same(&ctx.get::<dyn CacheStore>().await.unwrap(), &named));
    let named = ctx.get_named::<dyn LockStore>(DRIVER).await.unwrap();
    assert!(same(&ctx.get::<dyn LockStore>().await.unwrap(), &named));
    let named = ctx.get_named::<dyn SessionStore>(DRIVER).await.unwrap();
    assert!(same(&ctx.get::<dyn SessionStore>().await.unwrap(), &named));
    ctx.get_named::<dyn QueueStore>(DRIVER).await.unwrap();
    let queue = ctx.get::<Queue>().await.unwrap();
    assert!(format!("{queue:?}").contains("Redis"), "{queue:?}");
    ctx.get_named::<dyn Backplane>(DRIVER).await.unwrap();
    assert!(ctx.get::<Realtime>().await.unwrap().has_backplane());

    let redis = ctx.get::<Redis>().await.unwrap();
    assert_eq!(redis.prefix(), "navree:");
    ctx.get::<RateLimiter>().await.unwrap();
    ctx.get_named::<dyn navree_http::RateLimitStore>(DRIVER)
        .await
        .unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_health_check_reports_a_server_that_does_not_answer() {
    let mut options = unreachable();
    options.connections.insert(
        "queue".to_owned(),
        navree_redis::RedisConnectionOptions {
            url: "redis://:hunter2@127.0.0.1:2".to_owned(),
        },
    );
    let app = Navree::app()
        .module(RedisModule::for_root(options))
        .boot()
        .await
        .unwrap();
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    let check = checks
        .iter()
        .find(|check| check.name() == "redis")
        .expect("the redis health check is registered");
    let HealthStatus::Down(reason) = check.check().await else {
        panic!("nothing listens, so the check is down");
    };
    assert!(
        reason.contains("Redis connection `default`")
            && reason.contains("Redis connection `queue`"),
        "{reason}"
    );
    assert!(!reason.contains("hunter2"), "{reason}");
}

#[tokio::test]
async fn from_config_reads_the_redis_section() {
    let sources = ConfigSources::new().set(
        "redis",
        json!({ "url": UNREACHABLE, "prefix": "crm:", "connections": { "queue": { "url": UNREACHABLE } } }),
    );
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(RedisModule::from_config())
        .boot()
        .await
        .unwrap();
    let redis = app.ctx().get::<Redis>().await.unwrap();
    assert_eq!(redis.prefix(), "crm:");
    assert_eq!(redis.named("queue").unwrap().name(), "queue");
    let options = app.ctx().get::<RedisOptions>().await.unwrap();
    assert_eq!(options.url, UNREACHABLE);
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    let sources = ConfigSources::new().set("redis", json!({ "url": "http://:pw@example.com" }));
    let error = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(RedisModule)
        .boot()
        .await
        .expect_err("not a Redis URL")
        .to_string();
    assert!(error.contains("redis.url is not a Redis URL"), "{error}");
    assert!(!error.contains("pw"), "{error}");

    let error = Navree::app()
        .module(RedisModule::for_root(RedisOptions {
            response_timeout_ms: 0,
            ..RedisOptions::default()
        }))
        .boot()
        .await
        .expect_err("zero timeout")
        .to_string();
    assert!(error.contains("redis.response_timeout_ms"), "{error}");
}
