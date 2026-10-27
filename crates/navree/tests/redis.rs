//! The examples of `content/docs/digging-deeper/redis.mdx` (and the Redis
//! sections of the cache, queues, sessions, broadcasting and rate-limiting
//! pages), written as an app writes them: the facade, the prelude and the
//! macros. They boot without a Redis server (connections open on first
//! use); the ones that need a server read `REDIS_URL`, as the docs do, and
//! return early without one.

use std::time::Duration;

use navree::cache::{CacheOptions, CacheStoreKind};
use navree::prelude::*;
use navree::queue::{Queue, QueueDriver, QueueOptions};
use navree::realtime::{Backplane, BackplaneKind, MemoryBackplane, Realtime, RealtimeOptions};
use navree::redis::redis::AsyncCommands;
use navree::redis::{RateLimit, RateLimiter, Redis, RedisOptions};
use navree::session::{SessionOptions, SessionStoreKind};
use navree::testing::TestApp;
use serde_json::{Value, json};
use uuid::Uuid;

/// Nothing listens here: the boots below prove nothing connects at boot.
const UNREACHABLE: &str = "redis://127.0.0.1:1";

/// The app of the "Installation" section, on a test configuration.
fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(RedisModule::from_config()) // section `redis`
        .module(CacheModule::from_config()) // cache.store = "redis"
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config()) // queue.driver = "redis"
}

#[tokio::test]
async fn the_installation_example_selects_the_redis_drivers() {
    let sources = ConfigSources::new()
        .set(
            "redis",
            json!({ "url": UNREACHABLE, "connect_timeout_ms": 200 }),
        )
        .set("cache", json!({ "store": "redis" }))
        .set("queue", json!({ "driver": "redis" }))
        .set("database", json!({ "url": "sqlite::memory:" }));
    let app = app(sources)
        .boot()
        .await
        .expect("boots while Redis is down");
    let queue = app.ctx().get::<Queue>().await.unwrap();
    assert!(format!("{queue:?}").contains("Redis"), "{queue:?}");
    let options = app.ctx().get::<CacheOptions>().await.unwrap();
    assert_eq!(options.store, CacheStoreKind::Redis);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn options_in_code() {
    let app = Navree::app()
        .module(RedisModule::for_root(RedisOptions {
            url: "redis://127.0.0.1:6379/1".into(),
            prefix: "crm:".into(),
            ..RedisOptions::default()
        }))
        .module(CacheModule::for_root(CacheOptions {
            store: CacheStoreKind::Redis,
            ..CacheOptions::default()
        }))
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Redis,
            ..QueueOptions::default()
        }))
        .boot()
        .await
        .unwrap();
    let redis = app.ctx().get::<Redis>().await.unwrap();
    assert_eq!(redis.key("greeting"), "crm:greeting");
    let _ = SessionOptions {
        store: SessionStoreKind::Redis,
        ..SessionOptions::default()
    };
    let _ = RealtimeOptions {
        backplane: BackplaneKind::Redis,
        ..RealtimeOptions::default()
    };
}

/// "Using Redis directly".
#[injectable]
pub struct ViewPreferences {
    redis: Arc<Redis>,
}

impl ViewPreferences {
    /// Remembers the user's view for 30 days.
    pub async fn save(&self, user_id: Uuid, view: &str) -> Result<(), BoxError> {
        let key = self.redis.key(&format!("views:{user_id}")); // "navree:views:…"
        let _: () = self.redis.manager().set_ex(&key, view, 30 * 86_400).await?;
        Ok(())
    }

    /// The user's view, if remembered.
    pub async fn load(&self, user_id: Uuid) -> Result<Option<String>, BoxError> {
        let key = self.redis.key(&format!("views:{user_id}"));
        Ok(self.redis.manager().get(&key).await?)
    }
}

/// "Shared rate limits".
#[injectable]
pub struct ExportsController {
    limiter: Arc<RateLimiter>,
}

#[controller("/exports", version = "1")]
#[authenticated]
impl ExportsController {
    #[post("/")]
    async fn create(&self, CurrentUser(user): CurrentUser) -> Result<Json<Value>> {
        let key = format!("exports:{}", user.id);
        match self
            .limiter
            .hit(&key, 10, Duration::from_secs(3600))
            .await?
        {
            RateLimit::Allowed { remaining } => {
                Ok(Json(json!({ "queued": true, "remaining": remaining })))
            }
            RateLimit::Limited { retry_after } => Err(AppError::too_many_requests(retry_after)),
        }
    }
}

/// Registers the examples' provider and controller.
#[module(providers = [ViewPreferences], controllers = [ExportsController])]
pub struct PreferencesModule;

#[tokio::test]
async fn services_and_controllers_resolve_their_redis_providers() {
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("redis", json!({ "url": UNREACHABLE })),
        ))
        .module(RedisModule)
        .module(PreferencesModule)
        .boot()
        .await
        .unwrap();
    app.ctx().get::<ViewPreferences>().await.unwrap();
    app.ctx().get::<RateLimiter>().await.unwrap();
}

/// The "Testing" section of the Redis page.
#[tokio::test]
async fn preferences_round_trip() {
    let Ok(url) = std::env::var("REDIS_URL") else {
        return; // no server on this machine
    };
    let options = RedisOptions {
        url,
        prefix: format!("test:{}:", uuid::Uuid::new_v4().simple()),
        ..RedisOptions::default()
    };
    let redis = Redis::new(&options).unwrap();
    redis.connection().ping().await.unwrap();
    let preferences = ViewPreferences {
        redis: Arc::new(redis),
    };
    let user = Uuid::now_v7();
    preferences.save(user, "board").await.unwrap();
    assert_eq!(
        preferences.load(user).await.unwrap().as_deref(),
        Some("board")
    );
}

/// The broadcasting page's backplane test.
#[tokio::test]
async fn broadcasts_reach_the_other_process() {
    let backplane = MemoryBackplane::new();
    let app = || {
        TestApp::builder()
            .config("realtime.secret", "test-secret")
            .override_value::<dyn Backplane>(Arc::new(backplane.clone()))
            .module(RealtimeModule::from_config())
            .build()
    };
    let (first, second) = (app().await, app().await);
    let realtime = first.get::<Realtime>().await;
    assert!(realtime.has_backplane() && second.get::<Realtime>().await.has_backplane());
    // A client connected to `second` receives this:
    realtime
        .broadcast(["announcements"], "posted", json!({ "id": 1 }))
        .unwrap();
}
