---
name: navree-redis
description: "Redis with navree-redis: `RedisModule` and the `redis` config section (`url`, `prefix`, `connect_timeout_ms`, `response_timeout_ms`, `connections.<name>.url`, `APP_REDIS__URL`), the `Redis` connections (`manager()`, `key()`, `named()`, `dedicated()`, `pubsub()`), the Redis drivers selected by name (`cache.store = \"redis\"`, `queue.driver = \"redis\"`, `session.store = \"redis\"`, `realtime.backplane = \"redis\"`), shared rate limits with `RateLimiter::hit`, the `redis` health check, key layout and production settings (eviction, persistence). Use when an app runs several processes or servers that must share the cache, locks, jobs, sessions, rate limits or WebSocket broadcasts and presence; when moving any of those to Redis; when running raw Redis commands; or when debugging `needs RedisModule` boot errors, `realtime.backplane needs realtime.secret`, readiness down for `redis`, jobs or sessions lost to eviction, or broadcasts that don't reach clients on another server."
---

# Redis

`navree::redis` (crate `navree-redis`, facade feature `redis`, **opt-in**:
not in `full`) opens the app's Redis connections once and registers a Redis
driver for each module that keeps state. Nothing changes until a module's
setting names `redis`, so drivers are switched one by one. The source is
`crates/navree-redis/src`; tests are in `crates/navree-redis/tests/redis/`
(the server-backed ones run with `NAVREE_TEST_REDIS_URL`). Docs page:
`/docs/digging-deeper/redis/`.

## Steps

1. Turn on the feature:

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["redis"] }
```

2. Add `RedisModule` (global; in the prelude with `redis`) and switch the
   drivers you want in configuration:

```rust
Navree::app()
    .module(ConfigModule::for_root(ConfigSources::defaults()))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(RedisModule::from_config()) // section `redis`
    .module(CacheModule::from_config())
    .module(QueueModule::from_config())
    .module(SessionModule::from_config())
    .module(RealtimeModule::from_config())
```

```toml
[redis]
url = "redis://127.0.0.1:6379"   # APP_REDIS__URL; rediss:// for TLS
prefix = "navree:"               # one per app sharing a server

[cache]
store = "redis"                  # entries and locks

[queue]
driver = "redis"                 # jobs, delayed and failed jobs

[session]
store = "redis"                  # one key per session, expiring with it

[realtime]
backplane = "redis"              # broadcasts, presence, disconnects across processes
secret = "..."                   # required with a backplane (APP_REALTIME__SECRET)
```

   In code: `CacheOptions { store: CacheStoreKind::Redis, .. }`,
   `QueueOptions { driver: QueueDriver::Redis, .. }`,
   `SessionOptions { store: SessionStoreKind::Redis, .. }`,
   `RealtimeOptions { backplane: BackplaneKind::Redis, secret, .. }`,
   `RedisModule::for_root(RedisOptions { url, prefix, ..RedisOptions::default() })`.

3. Put the URL (with its password) in the environment:
   `APP_REDIS__URL=rediss://default:secret@redis.internal:6380/0`.

4. Check readiness: the `redis` health check pings every connection
   (`/health/ready` with `ObserveModule`).

## Templates

Raw commands through the shared, reconnecting manager:

```rust
use navree::prelude::*;
use navree::redis::Redis;
use navree::redis::redis::AsyncCommands;

#[injectable]
pub struct Preferences {
    redis: Arc<Redis>,
}

impl Preferences {
    pub async fn save(&self, user: &str, view: &str) -> Result<(), BoxError> {
        let key = self.redis.key(&format!("views:{user}")); // prefix + key
        let _: () = self.redis.manager().set_ex(&key, view, 86_400).await?;
        Ok(())
    }
}
```

A limit shared by every process (same rule as the per-IP limit: a burst of
`limit`, then one every `per / limit`, on Redis's clock):

```rust
use std::time::Duration;
use navree::redis::{RateLimit, RateLimiter};

// in a handler of a controller holding `limiter: Arc<RateLimiter>`
if let RateLimit::Limited { retry_after } =
    self.limiter.hit(&format!("exports:{}", user.id), 10, Duration::from_secs(3600)).await?
{
    return Err(AppError::too_many_requests(retry_after));
}
```

Two apps in one test sharing a real-time backplane (no Redis needed):

```rust
use navree::realtime::{Backplane, MemoryBackplane};

let backplane = MemoryBackplane::new();
let app = || TestApp::builder()
    .config("realtime.secret", "test-secret")
    .override_value::<dyn Backplane>(Arc::new(backplane.clone()))
    .module(RealtimeModule::from_config())
    .build();
```

## What each driver does

| Driver | Keys (after `redis.prefix`) | Notes |
|---|---|---|
| `RedisCacheStore`, `RedisLockStore` | `cache:<key>`, `locks:<name>` | `SET PX`, `SET NX PX`, `INCRBY` (keeps TTL); locks released by an owner-checked Lua script; `flush`/`cache:clear` delete `cache:` keys only |
| `RedisQueueStore` | `queue:job:<id>`, `queue:pending:<q>`, `queue:reserved:<q>`, `queue:unique:<key>`, `queue:failed`, `queue:failed:index` | Lua scripts: atomic reservations, stale takeover after timeout + `STALE_RESERVATION_GRACE_SECS`, unique jobs, failed jobs kept; at least once; jobs dispatched in a unit of work are pushed after commit |
| `RedisSessionStore` | `session:<id>` | JSON record with the session's expiry as `PX`; expiry also checked with the app clock |
| `RedisBackplane` | channel `realtime:<app_id>` | pub/sub; hello, heartbeats every 5 s, presence resend every 30 s, nodes silent 20 s forgotten |
| `RateLimiter` | `limit:<key>` | GCRA in Lua |

Each driver uses the connection named after it when
`redis.connections.<name>.url` exists (`cache`, `queue`, `session`,
`realtime`, `rate_limiter`), else `redis.url`. `redis.named("x")` returns any
named connection; `RedisConnection::dedicated()` is for blocking commands;
`pubsub()` for `SUBSCRIBE`.

## Checks

- `cargo test` passes without Redis: tests keep the memory drivers.
- Against a server: `NAVREE_TEST_REDIS_URL=redis://127.0.0.1:6379 cargo test -p navree-redis`
  (`NAVREE_TEST_REQUIRE_SERVICES=1` makes a missing server fail instead of skip).
- `GET /health/ready` lists `redis` up in each environment.
- With a backplane, every `serve` process has the same `APP_REALTIME__SECRET`.

## Gotchas

- `cache.store = "redis"` (or the queue, session or backplane setting)
  without `RedisModule` fails the boot: `... needs RedisModule (navree-redis) in the app`.
- The app boots while Redis is down; commands fail until it is back and
  readiness reports it. Connections open on first use.
- `realtime.backplane = "redis"` without `realtime.secret` fails the boot:
  the process that signs a subscription is often not the one holding the socket.
- `maxmemory-policy allkeys-lru` can evict jobs, sessions and locks. Use
  `volatile-lru` or `noeviction`, or `redis.connections.cache` for an
  evicting cache server. Turn on `appendonly yes` where the queue lives.
- Redis Cluster is not supported (the queue's scripts touch computed keys);
  use one server or a primary with replicas.
- The per-IP limit and `#[throttle]` stay in process memory; only the
  cache-based throttles (login, two-factor, password reset) and
  `RateLimiter` are shared.
- `Realtime::broadcast` and `disconnect_user` return this process's count;
  other processes deliver too.
- Delivery on the backplane is at most once; presence converges within
  30 seconds after a lost connection.
