//! # navree-redis
//!
//! `RedisModule`: one set of Redis connections for the whole app, and the
//! Redis drivers of the other modules, so several processes (API servers,
//! `queue:work`, `schedule:work`) share their cache, locks, jobs, sessions,
//! rate limits and WebSocket broadcasts.
//!
//! - [`RedisModule`] is global. Configure it with [`RedisModule::for_root`]
//!   or [`RedisModule::from_config`] (section `redis`, [`RedisOptions`]:
//!   `url`, `prefix`, `connect_timeout_ms`, `response_timeout_ms` and
//!   `connections.<name>.url`; `url` falls back to `REDIS_URL`, which
//!   `navree dock` and Harbor set). It provides [`Redis`]: lazily opened,
//!   reconnecting [`RedisConnection`]s (a multiplexed connection manager
//!   per server, plus dedicated and pub/sub connections), and registers
//!   the `redis` readiness check.
//! - Drivers, selected by name in the other modules' settings:
//!   `cache.store = "redis"` ([`RedisCacheStore`], [`RedisLockStore`]),
//!   `queue.driver = "redis"` ([`RedisQueueStore`]),
//!   `session.store = "redis"` ([`RedisSessionStore`]) and
//!   `realtime.backplane = "redis"` ([`RedisBackplane`]). Each uses the
//!   connection named after it (`cache`, `queue`, `session`, `realtime`)
//!   when one is configured, the default connection otherwise.
//! - [`RateLimiter`]: named rate limits shared by every process, and
//!   `http.rate_limit_store = "redis"` ([`RedisRateLimitStore`]): the HTTP
//!   server's per-IP limit and route throttles, counted together by every
//!   process.
//! - Every key starts with `redis.prefix` (default `navree:`): the cache
//!   under `cache:`, locks under `locks:`, jobs under `queue:`, sessions
//!   under `session:`, rate limits under `limit:` (the HTTP server's under
//!   `http-limit:`), and the backplane's pub/sub channel is
//!   `realtime:<app_id>`.
//! - [`redis`] is the redis-rs crate, re-exported for raw commands
//!   (`redis::AsyncCommands`).
//!
//! A single Redis server (or a primary with replicas behind one address)
//! is supported; Redis Cluster is not, because the queue's Lua scripts
//! touch keys they compute.
//!
//! ```no_run
//! use navree_cache::CacheModule;
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_kernel::Navree;
//! use navree_queue::QueueModule;
//! use navree_redis::RedisModule;
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     // config/app.toml:
//!     //   [redis]
//!     //   url = "redis://127.0.0.1:6379"
//!     //   [cache]
//!     //   store = "redis"
//!     //   [queue]
//!     //   driver = "redis"
//!     Navree::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(RedisModule::from_config())
//!         .module(CacheModule::from_config())
//!         .module(QueueModule::from_config())
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

mod backplane;
mod cache;
mod connection;
mod error;
mod limiter;
mod module;
mod options;
mod queue;
mod session;

pub use backplane::RedisBackplane;
pub use cache::{RedisCacheStore, RedisLockStore};
pub use connection::{Redis, RedisConnection};
pub use error::RedisError;
pub use limiter::{RateLimit, RateLimiter, RedisRateLimitStore};
pub use module::{DRIVER, RedisModule};
pub use options::{DEFAULT_CONNECTION, RedisConnectionOptions, RedisOptions, URL_ENV};
pub use queue::RedisQueueStore;
pub use session::RedisSessionStore;

/// The redis-rs client, for commands the drivers do not wrap
/// (`navree_redis::redis::AsyncCommands`).
pub use redis;
