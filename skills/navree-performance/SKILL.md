---
name: navree-performance
description: "Make a Navree app faster per request, or check that a change didn't make it slower: the request-pipeline benchmarks (`cargo bench -p navree --features sqlite --bench pipeline`), the `database` pool settings (`test_before_acquire`, `idle_timeout_secs`, `max_lifetime_secs`, `max_connections`) and `database.transaction_settings`, what the fixed HTTP layers cost and when they merge into one middleware, which modules make model writes serialize change events, reading through the pool instead of a transaction, and the service locator in hot paths. Use when throughput or latency matters, when a load test shows Navree using its CPUs while the database idles, or before changing navree-http, navree-db or navree-kernel code on the request path."
---

# Performance

What Navree adds to a request is the fixed pipeline (`navree-http`), the
unit of work (`navree-db`) and the extractors your handler takes. This skill
covers how to measure that cost, the settings that change it, and the
habits that keep it low. Read `crates/navree/benches/pipeline.rs`,
`crates/navree-http/src/pipeline.rs`, `crates/navree-http/src/router.rs`
and `crates/navree-db/src/http.rs` when you need the detail.

## Measure first

```sh
cargo bench -p navree --features sqlite --bench pipeline              # every benchmark
cargo bench -p navree --features sqlite --bench pipeline -- pipeline/json --save-baseline before
# ...change something...
cargo bench -p navree --features sqlite --bench pipeline -- pipeline/json --baseline before
cargo bench -p navree --features sqlite --bench pipeline -- --test    # run each once (CI)
```

| Benchmark | What it measures |
|---|---|
| `pipeline/json` | `GET /json` through the whole router: every fixed layer, the unit-of-work layer, the per-route pipeline |
| `bare/json` | the same handler on a bare `routing::Router`: the floor to compare `pipeline/json` with |
| `pipeline/json_x64` | 64 `/json` requests at once on four worker threads: shows contention |
| `pipeline/db` | `GET /db`: one `SELECT` by primary key through the pool (SQLite) |
| `pipeline/insert` | `POST /deals`: `Valid<Json<T>>`, then `Deal::create` in the request's unit of work, committed |
| `uow/begin_commit` | a unit of work that begins and commits a SQLite transaction |
| `kernel/get` | `AppContext::get::<Db>()` |

Compare a change against a saved baseline on the same machine, and read
`pipeline/json` relative to `bare/json` of the same run: absolute times
move with the machine's load. The full load test is
`node run.mjs` in https://github.com/NavreeHQ/benchmarks (Docker; see `SPEC.md` there).

## The settings that matter

```toml
[database]
max_connections = 10          # size it to the database, not to the CPU count
test_before_acquire = false   # default: only connections idle for 30 s or more are pinged
idle_timeout_secs = 600       # 0 keeps idle connections
max_lifetime_secs = 1800      # 0: no limit
transaction_settings = true   # false: no navree.actor / @navree_actor statement per transaction

[http]
compression = true            # one more middleware between the fixed stages; a route opts out with #[no_compression], a response with no-transform
per_ip_rate_limit = 600       # sharded by IP; a single client IP shares one key
```

- `test_before_acquire = true` pings every connection taken from the pool:
  one more round trip before every query. Leave it off unless the network
  drops connections within 30 seconds.
- `transaction_settings = false` saves one statement per PostgreSQL
  transaction (two on MySQL and MariaDB). Turn it off only when no trigger
  or row-level security policy reads `navree.actor` / `navree.request_id`
  (`@navree_actor` / `@navree_request_id`). Workspace isolation sets its
  own `navree.workspace` and is not affected. `uow.actor()` and
  `uow.request_id()` still work.

## What the pipeline does for you

- No task per request: the request runs in the connection's task. Only a
  request dropped before it finishes (client gone, global timeout) moves
  the rest of its work to a task on the app's tracker, so it still commits.
- The fixed layers between two Tower layers run as the stages of one
  middleware: without CORS (`http.cors.allowed_origins` empty) and without
  compression they are all one middleware. Navree's authentication joins
  them when no `Session`-slot layer (sessions, logbook capture) sits in
  between.
- Maintenance mode is an atomic flag while the app is up; the file is read
  at most once a second. The per-IP limiter and route throttles are sharded
  maps. Request ids come from a per-thread UUID v7 counter.
- Metric labels are built only when a `metrics` recorder is installed
  (`ObserveModule` installs one).
- `Valid<Json<T>>` deserializes the body straight from its bytes; a
  `serde_json::Value` is built only to report a failure.
- `AppContext::get`, `try_get` and `get_named` are a map lookup.
- Model creates, saves and deletes serialize their change event only if
  the app registers a `dyn ChangeObserver` (`AuditModule`, `LogbookModule`,
  `SearchModule` do); otherwise only `uow.changes()` builds them.

## Habits

- A single read needs no transaction: `Deal::find(&db, id)` through the
  `Db` costs one round trip on the pool; through `&uow` it is `BEGIN`, the
  query, the transaction settings and `COMMIT`.
- In a service that takes no `&Uow`, read through `&db`: it uses the
  request's open transaction when there is one, the pool otherwise.
- Inject providers (`Arc<T>` fields, `#[injectable]`) instead of calling
  `ctx.get` per request.
- Register a `GlobalLayer` as a Tower `Layer` with its own service rather
  than `navree::http::middleware::from_fn` when it runs on every request:
  every `from_fn` layer adds boxed futures.
- Leave `LogbookModule` out of production builds you benchmark: even with
  `logbook.enabled = false` it registers a change observer (so model writes
  serialize their events) and a `Session`-slot layer (so authentication
  stays a middleware of its own).

## Checks

- `cargo bench -p navree --features sqlite --bench pipeline -- --baseline before` shows no
  regression for the benchmarks your change touches.
- `cargo test -p navree-http -p navree-db -p navree-kernel` stay green: the
  pipeline tests pin the order of the fixed layers, the request id on every
  response, the timeout and panic responses, run-to-completion on
  disconnect, and the statements a unit of work sends
  (`crates/navree-db/tests/engine/units.rs`).

## Gotchas

- A request that keeps a row stream of its unit of work alive past the
  handler holds the connection: the commit waits until the stream ends or is
  dropped.
- `transaction_settings = false` with a trigger that reads
  `current_setting('navree.actor', true)` silently records an empty actor.
- Benchmarks of one client IP put every request on the same limiter key: the
  sharding helps many clients, not one.
- `uow.changes()` in an app with no change observer prepares the events when
  you call it; an event whose record can't be serialized is left out with a
  warning there (with an observer, the write itself fails).
