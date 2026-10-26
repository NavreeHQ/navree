---
name: navree-cache-and-http-client
description: "Caching, locks and outgoing HTTP in Navree: `Cache` (`remember`, `flexible`, counters, tags), atomic `Locks`, and `HttpClient` with retries, timeouts, the SSRF policy and `HttpClient::fake()` for tests. Use when caching an expensive computation or third-party answer, limiting an action per user, preventing two tasks from running at once, calling an external API or webhook, or faking outgoing HTTP in tests."
---

# Cache, locks and the HTTP client

`CacheModule` (in a new app) gives `Cache` and `Locks` on an in-process
store (moka). `HttpClientModule` (add `HttpClientModule::from_config()` to
the root) gives one shared `HttpClient` on reqwest/rustls with retries,
timeouts, a response size limit, an SSRF guard and a fake. All are global:
inject `Arc<Cache>`, `Arc<Locks>`, `Arc<HttpClient>`.

## Cache

```rust
use std::time::Duration;

use navree::cache::{Cache, CacheError, Ttl};
use navree::prelude::*;

#[injectable]
pub struct PipelineService {
    cache: Arc<Cache>,
}

impl PipelineService {
    pub async fn totals(&self) -> Result<Totals, CacheError> {
        // computes on a miss, stores for 60 s; concurrent callers share one computation
        self.cache
            .remember("dashboard:pipeline", Duration::from_secs(60), || self.compute())
            .await
    }

    async fn compute(&self) -> Result<Totals, BoxError> {
        // ... the expensive queries
        Ok(Totals::default())
    }
}
```

| Call | Does |
|---|---|
| `get::<T>(key)` / `has(key)` / `pull::<T>(key)` | read (JSON-decoded) |
| `put(key, &value, ttl)` | store; ttl = `Duration`, `Ttl::Default` (`cache.default_ttl_secs`) or `Ttl::Forever` |
| `add(key, &value, ttl)` | store only if absent, atomically; returns whether it stored |
| `forget(key)` / `flush()` | remove one / everything |
| `increment(key, n)` / `decrement` | integer counters (a counter `increment` creates never expires: `add` it first with a TTL) |
| `remember(key, ttl, f)` / `remember_forever` | read or compute and store |
| `flexible(key, fresh, stale, f)` | serve stale while one background refresh runs (`f` must be `Send + 'static`) |
| `tags(&["deals", "company:1"])` | a `TaggedCache`; `.flush()` invalidates everything under those tags |

Values must be `Serialize + Deserialize`. Errors are `CacheError` (a 500 via
`?`). A per-user limit with counters:

```rust
let key = format!("exports:{user_id}");
cache.add(&key, &0, Duration::from_secs(86_400)).await?; // opens a 24-hour window
if cache.increment(&key, 1).await? > 10 {
    return Err(AppError::too_many_requests(Duration::from_secs(3600)));
}
```

## Locks

```rust
use navree::cache::Locks;

let lock = self.locks.lock("imports:acme", Duration::from_secs(120)); // TTL longer than the work
let Some(_guard) = lock.acquire().await? else {
    return Ok(false); // someone else holds it
};
// ... work; dropping the guard releases the lock
```

`lock.block(Duration::from_secs(10)).await?` waits (or `CacheError::LockTimeout`);
`guard.into_owner()` and `locks.lock(name, ttl).with_owner(owner).release()`
hand the lock to another task.

**One process only:** the memory store is per process, so `serve` and
`queue:work`, or two servers, don't share entries or locks. For
once-per-cluster work use the scheduler (runs each tick once) or unique
jobs. `cache:clear` in a new process doesn't clear a running server.

## HTTP client

```rust
use std::time::Duration;

use navree::axum::http::StatusCode;
use navree::http_client::{HttpClient, HttpClientError};
use navree::prelude::*;

#[injectable]
pub struct ExchangeRates {
    http: Arc<HttpClient>,
}

impl ExchangeRates {
    pub async fn latest(&self, base: &str) -> Result<Rates, HttpClientError> {
        self.http
            .get("https://rates.example.com/latest")
            .query(&[("base", base)])
            .timeout(Duration::from_secs(5))
            .send()
            .await?               // reads the whole body
            .error_for_status()?  // 4xx/5xx are NOT errors until you ask
            .json()
    }
}

// In a handler: the provider failing is a 502, not your 500.
let rates = self.rates.latest(&base).await.map_err(|error| {
    AppError::new(StatusCode::BAD_GATEWAY, "rates_unavailable", "Bad Gateway").with_source(error)
})?;
```

Builder: `get`, `post`, `put`, `patch`, `delete`, `request(method, url)`,
then `header`, `bearer(token)`, `with_request_id(&id)` (not automatic),
`query`, `json`, `form`, `body`, `timeout`, `retries(n)`. Response:
`status()`, `is_success()`, `header(name)`, `text()`, `bytes()`, `json()`.

- Retries: idempotent methods (GET, HEAD, OPTIONS, TRACE, PUT, DELETE) on
  connection failures, 5xx and 429, with exponential backoff and jitter.
  POST and PATCH are **never** retried. A timed-out attempt isn't retried.
- SSRF: private, loopback, link-local and metadata addresses are refused
  (`HttpClientError::Blocked`), checked on every resolved address and
  redirect. Allow internal services explicitly:

```toml
[http_client]
timeout_secs = 30
retries = 2
max_response_bytes = 10485760

[http_client.ssrf]
block_private = true
allow_hosts = ["billing.internal", "*.svc.cluster.local", "10.20.0.0/16"]
```

Header values and bodies are never logged. There is no streaming response
API (raise `max_response_bytes` or use reqwest directly for big downloads).

## Test with the fake

```rust
use navree::axum::http::Method;
use navree::http_client::{FakeResponse, HttpClient};
use navree::prelude::*;
use navree::testing::TestApp;
use serde_json::json;

#[tokio::test]
async fn rates_come_from_the_provider() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://rates.example.com/latest?*",                      // a glob over the full URL
        FakeResponse::new(200).json(&json!({ "base": "EUR", "rates": { "USD": 1.08 } })),
    );
    let app = TestApp::builder()
        .module(CacheModule::from_config())
        .module(HttpClientModule::from_config())
        .module(RatesModule)
        .override_value(Arc::new(client))
        .build()
        .await;
    let rates = app.get::<ExchangeRates>().await.latest("EUR").await.unwrap();
    assert_eq!(rates.rates["USD"], 1.08);
    server.assert_sent(|request| request.url().query() == Some("base=EUR"));
    server.assert_sent_count(1);
    app.shutdown().await.unwrap();
}
```

`server.on_sequence(method, glob, [FakeResponse::new(503), FakeResponse::connection_failed(), FakeResponse::new(200)])`
tests retries; `FakeResponse::delay(..)` tests timeouts (pause Tokio's clock
with `#[tokio::test(start_paused = true)]` and tokio's `test-util`).
An unmatched request fails with `HttpClientError::NoFakeResponse`.
`Cache::memory()` and `Locks::memory()` give standalone stores for unit tests.

## Common mistakes

- Expecting the cache or locks to be shared between processes or servers.
- Retrying a POST yourself without an idempotency key.
- Fetching user-supplied URLs with `block_private = false`.
- Forgetting `error_for_status()` and parsing an error body as success.
- Giving tagged entries `Ttl::Forever` (flushed tags leave them in memory).

## Docs

- https://navree.com/docs/digging-deeper/cache/
- https://navree.com/docs/digging-deeper/http-client/
