---
name: navree-testing
description: "Tests Navree apps in-process with `navree::testing`: `TestApp` with a fresh SQLite database per test, `TestClient` requests, `acting_as`, response assertions, booting the whole app, database assertions and the fakes (queue, mail, clock, outgoing HTTP, storage, broadcasts). Use when writing or fixing tests for a route, service, job, listener, migration or command, when a test needs a signed-in user, fixed time, fake mail or queue, or when tests are flaky or slow."
---

# Testing

A Navree test boots real modules in-process and sends requests through the
whole pipeline (request id, CORS, rate limit, authentication, access checks,
the unit of work, the error format) to a fresh SQLite database. Only the
edges you choose are faked. `cargo test` runs everything in parallel: each
test has its own app and database.

The kit is `navree::testing` (not in the prelude). Tests need
`tokio = { version = "1", features = ["macros", "rt-multi-thread"] }` in
`[dev-dependencies]` (a new app has it).

## Where tests go

- Module tests: `src/modules/<module>/tests/mod.rs` (declared
  `#[cfg(test)] mod tests;`). `navree make:module posts --crud` writes them;
  `navree make:test posts publishing` adds `tests/publishing.rs`.
- App tests: `tests/*.rs`, booting `my_app::app(..)` (a new app has
  `tests/app.rs`).

## A module test

```rust
use navree::access::{AccessModule, AccessOptions};
use navree::db::DatabaseModule;
use navree::testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use super::PostsModule;

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()                                    // a new SQLite file, migrated at boot
        .config("http.per_ip_rate_limit", Value::Null)      // every test request comes from 127.0.0.1
        .module(DatabaseModule)
        .module(AccessModule::for_root(AccessOptions {
            abilities: crate::abilities::define,
            super_admin_role: None,
        }))
        .module(PostsModule)
        .build()
        .await
}

#[tokio::test]
async fn only_the_owner_may_update() {
    let app = app().await;
    let owner = app.acting_as(principal(Uuid::now_v7()));
    let created = owner
        .post("/api/v1/posts")
        .json(&json!({ "name": "Mine" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let path = format!("/api/v1/posts/{}", created["id"].as_str().unwrap());

    let other = app.acting_as(principal(Uuid::now_v7()));
    other.patch(&path).json(&json!({ "name": "Taken" })).send().await.assert_error(403, "Forbidden");
    app.client().get("/api/v1/posts/feed").send().await.assert_error(401, "Unauthorized");
    owner.post("/api/v1/posts").json(&json!({ "name": "" })).send().await.assert_validation_error("name");
    app.shutdown().await.unwrap(); // closes the pool, deletes the database (needed on Windows)
}
```

`TestApp::builder()` adds `ConfigModule` (with **no** sources: no `.env`,
TOML or environment) and `HttpModule`; don't add your own `ConfigModule`.
`.temp_database()` sets the URL only: add `DatabaseModule` (or a module
importing it). `.build()` panics with the boot error; `.try_build()` returns it.

| On the app | Does |
|---|---|
| `app.client()` | a guest client (own cookie jar) |
| `app.acting_as(principal(id))` | a client signed in as that user id (no roles; give roles through `AccessService`) |
| `app.get::<T>().await` | resolve a provider (`Db`, a service, `Queue`, `Mailer`) |
| `app.run_command("db:seed", &["demo"]).await` | run a registered command |
| `app.ctx()` | the `AppContext` |
| `app.shutdown().await` | graceful shutdown |

Requests: `.get/.post/.put/.patch/.delete(path)` with the full path
(`/api/v1/..`), then `.json(&v)`, `.query(&[("per_page", "10")])`,
`.header(name, value)`, `.bearer(token)`, `.form(&v)`, `.body(bytes)`,
`.send().await`. Assertions chain and print the body on failure:
`assert_status(201)`, `assert_json_path("data.0.name", json!("x"))`,
`assert_error(404, "Not Found")` or `assert_error(404, None)`,
`assert_error_code(401, "invalid_credentials")` (the error `code`),
`assert_validation_error("field")`; then `json_value()`, `json::<T>()`,
`header("location")`, `text()`.

## Apps with AuthModule

`acting_as` needs the test authenticator: build with `.fake_auth()` (then
real bearer tokens don't work, and `CurrentUser` needs a real `users` row:
create it with `AuthService::create_user`), or sign in for real and send
`.bearer(token)` (`auth.jwt().sign_access(user.id)` or `POST /api/auth/login`).
Set `hashing.memory_kib = 1024` and `hashing.iterations = 1` for fast hashes.

## Boot the whole app

```rust
use navree::config::ConfigSources;
use navree::crypto::generate_key;
use navree::testing::{TempDirGuard, TestApp, TestAuthenticator, principal, temp_sqlite_url};
use serde_json::Value;
use uuid::Uuid;

fn sources() -> (ConfigSources, TempDirGuard) {
    let (url, guard) = temp_sqlite_url();
    let sources = ConfigSources::new()
        .set("app.key", generate_key())
        .set("database.url", url)
        .set("database.migrate_on_boot", true)
        .set("http.per_ip_rate_limit", Value::Null)
        .set("mail.transport", "array")
        .set("observe.metrics", false)
        .set("hashing.memory_kib", 1024)
        .set("hashing.iterations", 1);
    (sources, guard) // keep the guard alive: dropping it deletes the database
}

#[test]
fn module_graph_is_valid() {
    let (sources, _guard) = sources();
    my_app::app(sources).validate().expect("every module's imports and providers line up");
}

#[tokio::test]
async fn the_app_boots() {
    let (sources, _guard) = sources();
    let app = TestApp::from_builder(TestAuthenticator::install(my_app::app(sources))).await;
    app.client().get("/health/live").send().await.assert_status(200);
    app.acting_as(principal(Uuid::now_v7())).get("/api/v1/profile").send().await.assert_status(200);
    app.shutdown().await.unwrap();
}
```

## Data and the database

```rust
use navree::db::Db;

let db = app.get::<Db>().await;
Post::factory().state(PostFactory::published).count(3).create(&*db).await.unwrap(); // see navree-orm
app.run_command("db:seed", &["demo-posts"]).await.unwrap();
assert_eq!(Post::query().count(&*db).await.unwrap(), 6);
```

Prefer creating the data under test through the API; factories skip your
rules. A rejected request must leave nothing: count rows after it. Change
events: write through `db.uow(Some(user), None).await` and read
`uow.changes()` before `uow.rollback()`. Migrations: `app.run_command("migrate:rollback", &[])`
then `navree::db::Migrator::status(app.ctx())`.

## Fakes

| Edge | Install | Assert |
|---|---|---|
| queue | `let fake = QueueFake::new();` `.override_value(Arc::new(fake.queue()))` | `fake.assert_dispatched::<J>(\|j\| ..)`, `assert_dispatched_times`, `assert_not_dispatched` |
| real queue | `.config("queue.driver", "memory")` | `app.get::<Queue>().await.run_pending().await` |
| mail | `.config("mail.transport", "array")` | `mailer.array_transport().unwrap().assert_sent(\|m\| ..)`, `.sent()` |
| time | `let clock = Arc::new(FixedClock::new(t));` `.override_value::<dyn Clock>(clock.clone())` | `clock.advance(d)`, `clock.set(t)` |
| outgoing HTTP | `let (client, server) = HttpClient::fake();` `.override_value(Arc::new(client))` | `server.on(..)`, `server.assert_sent(..)` |
| storage | `.config("storage.disks.local", json!({ "driver": "memory" }))` | `app.get::<Disk>()` |
| broadcasts | a `Broadcaster` recorder, `.override_value::<dyn Broadcaster>(rec.clone())` | inspect the recorder |
| events | `app.get::<EventBus>().await.subscribe_json("deal.won")` before the request | `rx.try_recv()` |

`FixedClock` (`navree::testing::FixedClock`) only controls code reading the
app's `dyn Clock` (Navree's own modules do; `Utc::now()` doesn't). The
`QueueFake` records jobs even from requests that roll back, and accepts
unregistered jobs: keep one test on the real queue.

## Isolation suites

Multi-tenant modules get cross-workspace tests generated from their route
table: `app.isolation_suite::<DealsModule>().factory(Deal::factory()).run().await`
(`navree::workspace::testing::IsolationSuiteExt`). From workspace A, B's
records must be a 404, missing from lists and never created there. See
skill `navree-module-conformity` for bindings, bodies and the report.

## Running

```sh
cargo test                        # all
cargo test posts                  # names containing "posts"
cargo test -- --nocapture         # show println!
```

Use `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]` only for
concurrency tests.

## Common mistakes

- Forgetting `.config("http.per_ip_rate_limit", Value::Null)` in tests that
  send many requests (429s after 600).
- Booting with `ConfigSources::defaults()` in tests (reads the developer's
  `.env` and environment).
- `acting_as` in an app with `AuthModule` without `.fake_auth()` (panics).
- Dropping the `TempDirGuard` early, or not calling `shutdown` on Windows
  (temporary databases stay in `%TEMP%`).
- Asserting on `Utc::now()` values: inject `dyn Clock` and fix it.

## Docs

- https://navree.com/docs/testing/getting-started/
- https://navree.com/docs/testing/http-tests/
- https://navree.com/docs/testing/database-tests/
- https://navree.com/docs/testing/fakes/
- https://navree.com/docs/testing/isolation-tests/
