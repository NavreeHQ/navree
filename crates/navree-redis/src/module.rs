//! [`RedisModule`] and the `redis` health check.

use std::sync::Arc;

use futures_util::future::join_all;
use navree_cache::{CacheStore, LockStore};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::{BoxFuture, Clock, HealthCheck, HealthStatus, SessionStore};
use navree_kernel::{Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};
use navree_queue::QueueStore;
use navree_realtime::Backplane;

use crate::{
    RateLimiter, Redis, RedisBackplane, RedisCacheStore, RedisLockStore, RedisOptions,
    RedisQueueStore, RedisSessionStore,
};

/// The name of every driver this module registers.
pub const DRIVER: &str = "redis";

/// The Redis module. Global; provides and exports [`Redis`] (the
/// connections), [`RedisOptions`] and [`RateLimiter`], registers the
/// `redis` readiness check, and registers the Redis drivers the other
/// modules select by name:
///
/// | Setting | Provider (named `redis`) | Driver |
/// |---|---|---|
/// | `cache.store = "redis"` | `dyn CacheStore`, `dyn LockStore` | [`RedisCacheStore`], [`RedisLockStore`] |
/// | `queue.driver = "redis"` | `dyn QueueStore` | [`RedisQueueStore`] |
/// | `session.store = "redis"` | `dyn SessionStore` | [`RedisSessionStore`] |
/// | `realtime.backplane = "redis"` | `dyn Backplane` | [`RedisBackplane`] |
///
/// ```no_run
/// use navree_cache::CacheModule;
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
/// use navree_redis::RedisModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// // config/app.toml: [redis] url = "redis://127.0.0.1:6379"
/// //                  [cache] store = "redis"
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(RedisModule::from_config()) // section `redis`
///     .module(CacheModule::from_config())
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(RedisModule)`) or through
/// [`from_config`](RedisModule::from_config), it reads the `redis`
/// configuration section, so the app must include `ConfigModule`. Invalid
/// options fail the boot; an unreachable server does not (connections
/// open on first use).
#[derive(Clone, Copy, Debug, Default)]
pub struct RedisModule;

impl RedisModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: RedisOptions) -> impl Module {
        ConfiguredRedisModule {
            options: Some(options),
        }
    }

    /// The module configured from the `redis` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredRedisModule { options: None }
    }
}

impl Module for RedisModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `RedisModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredRedisModule {
    options: Option<RedisOptions>,
}

impl Module for ConfiguredRedisModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<RedisModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<RedisOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<RedisOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid redis options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<RedisOptions>(),
    };
    let redis_key = ProviderKey::of::<Redis>();
    m.exports::<RedisOptions>()
        .factory(
            Factory::<Redis>::new(|r: Resolver| async move {
                let options = r.get::<RedisOptions>().await?;
                Ok(Arc::new(Redis::new(&options)?))
            })
            .deps(vec![ProviderKey::of::<RedisOptions>()]),
        )
        .exports::<Redis>()
        .factory(
            Factory::<RateLimiter>::new(|r: Resolver| async move {
                let redis = r.get::<Redis>().await?;
                Ok(Arc::new(RateLimiter::new(
                    redis.connection_for("rate_limiter").clone(),
                    redis.prefix(),
                )))
            })
            .deps(vec![redis_key.clone()]),
        )
        .exports::<RateLimiter>()
        .multi(
            Factory::<dyn HealthCheck>::new(|r: Resolver| async move {
                let redis = r.get::<Redis>().await?;
                Ok(Arc::new(RedisHealth { redis }) as Arc<dyn HealthCheck>)
            })
            .deps(vec![redis_key.clone()]),
        )
        .factory(
            Factory::<dyn CacheStore>::new(|r: Resolver| async move {
                let redis = r.get::<Redis>().await?;
                let store =
                    RedisCacheStore::new(redis.connection_for("cache").clone(), redis.prefix());
                Ok(Arc::new(store) as Arc<dyn CacheStore>)
            })
            .named(DRIVER)
            .deps(vec![redis_key.clone()]),
        )
        .export_key(ProviderKey::named::<dyn CacheStore>(DRIVER))
        .factory(
            Factory::<dyn LockStore>::new(|r: Resolver| async move {
                let redis = r.get::<Redis>().await?;
                let store =
                    RedisLockStore::new(redis.connection_for("cache").clone(), redis.prefix());
                Ok(Arc::new(store) as Arc<dyn LockStore>)
            })
            .named(DRIVER)
            .deps(vec![redis_key.clone()]),
        )
        .export_key(ProviderKey::named::<dyn LockStore>(DRIVER))
        .factory(
            Factory::<dyn QueueStore>::new(|r: Resolver| async move {
                let redis = r.get::<Redis>().await?;
                let store =
                    RedisQueueStore::new(redis.connection_for("queue").clone(), redis.prefix());
                Ok(Arc::new(store) as Arc<dyn QueueStore>)
            })
            .named(DRIVER)
            .deps(vec![redis_key.clone()]),
        )
        .export_key(ProviderKey::named::<dyn QueueStore>(DRIVER))
        .factory(
            Factory::<dyn SessionStore>::new(|r: Resolver| async move {
                let redis = r.get::<Redis>().await?;
                let mut store =
                    RedisSessionStore::new(redis.connection_for("session").clone(), redis.prefix());
                if let Some(clock) = r.app().try_get::<dyn Clock>().await {
                    store = store.with_clock(clock);
                }
                Ok(Arc::new(store) as Arc<dyn SessionStore>)
            })
            .named(DRIVER)
            .deps(vec![redis_key.clone()]),
        )
        .export_key(ProviderKey::named::<dyn SessionStore>(DRIVER))
        .factory(
            Factory::<dyn Backplane>::new(|r: Resolver| async move {
                let redis = r.get::<Redis>().await?;
                let app = r.app();
                let backplane = RedisBackplane::new(
                    redis.connection_for("realtime").clone(),
                    redis.prefix(),
                    app.shutdown_token(),
                    app.tracker(),
                );
                Ok(Arc::new(backplane) as Arc<dyn Backplane>)
            })
            .named(DRIVER)
            .deps(vec![redis_key]),
        )
        .export_key(ProviderKey::named::<dyn Backplane>(DRIVER));
}

/// Readiness: every connection answers `PING`.
struct RedisHealth {
    redis: Arc<Redis>,
}

impl HealthCheck for RedisHealth {
    fn name(&self) -> &str {
        "redis"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async move {
            let pings = join_all(self.redis.connections().map(|connection| async move {
                connection.ping().await.err().map(|error| error.to_string())
            }))
            .await;
            let failures: Vec<String> = pings.into_iter().flatten().collect();
            if failures.is_empty() {
                HealthStatus::Up
            } else {
                HealthStatus::Down(failures.join("; "))
            }
        })
    }
}
