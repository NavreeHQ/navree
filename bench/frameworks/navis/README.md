# Navis

The benchmark API as an ordinary Navis application: modules and controllers
with Navis's route macros, navis-auth's JWT strategy and `CurrentUser`,
navis-validation extractors, navis-db (SeaORM) entities, Navis's unit of work,
error format and pagination.

| | |
|---|---|
| Framework | Navis 0.1.0 from this repository (`crates/navis`, a path dependency; features `auth`, `log`, `postgres`) |
| Runtime | Rust 1.99.1 (`rust:1-bookworm` build stage, `debian:bookworm-slim` runtime); release profile `opt-level = 3`, `lto = "thin"`, `codegen-units = 1` |
| HTTP | navis-http on Axum 0.8.9, hyper 1.12.0, tower-http 0.7.1 |
| Workers | One process. `AppBuilder::run` builds Tokio's (1.54.1) default multi-thread runtime: one worker per CPU, 8 under the benchmark's cpuset. The `serve` command runs `axum::serve` on `0.0.0.0:8080` |
| Database | navis-db `DatabaseModule`: SeaORM 2.0.4 on sqlx 0.9.0 (PostgreSQL), one pool of 64 connections (`database.max_connections = 64`, `migrate_on_boot = false`) |
| JWT | navis-auth `AuthModule`: its `JwtStrategy` (jsonwebtoken 11.1.0, RustCrypto backend; HS256, signature, `exp`, `iss = navis`, token type `access`), then the `CurrentUser` extractor loads the `users` row (navis-auth's `user` entity) |
| Validation | navis-validation `Valid<Query<T>>` and `Valid<Json<T>>` on garde 0.23.0; failures are 400s in Navis's error format |

Configuration: `config/app.toml` (Navis's conventional file) plus the
harness's variables, mapped in `main.rs`: `DATABASE_URL` → `database.url`,
`JWT_SECRET` → `auth.jwt.secret`, `JWT_ISSUER` → `auth.jwt.issuer`. The log
level is `warn` (JSON lines in release builds), so the per-request log line
(`navis::request`, info) is filtered out.

## Code

- `src/app.rs`: the app root (the modules below).
- `src/world/`: `WorldModule` with `WorldController` (`#[controller("!/")]`,
  so no `/api` prefix): `GET /json`, `GET /db`, `GET /queries`, all
  `#[public]`, and the `world` SeaORM entity (`randomnumber` serialized as
  `randomNumber`).
- `src/deals/`: `DealsModule` with `DealsController` (`#[controller("/deals")]`
  → `/api/deals`, both routes `#[authenticated]`), `DealService`, the
  `deals` entity (with its `Stage` active enum and the `owner` relation to
  navis-auth's `users`), the DTOs and the `DealResource` / `OwnerRef` shapes.

## Navis modules in the app

`ConfigModule`, `LoggerModule`, `CryptoModule` (required by `AuthModule`),
`HttpModule`, `DatabaseModule`, `ValidationModule`, `AuthModule`, and the
app's `WorldModule` and `DealsModule`. Nothing else: no `ObserveModule`
(health checks and Prometheus metrics), `CacheModule`, `AccessModule`,
`AuditModule`, `EventsModule`, `QueueModule` or `ApiModule`, because this
API uses none of them (and the other apps in the benchmark have no metrics,
audit trail or OpenAPI either).

## Request pipeline (all of Navis's defaults are on)

Global layers, outermost first (navis-http `build_router`):

1. Request context: accepts or generates `X-Request-Id`, resolves the
   client IP, stores the app context and a request scope in the
   extensions, opens the request's tracing span, writes the request log
   line (filtered at `warn`) and calls the `http_requests_total` /
   `http_request_duration_seconds` metrics macros (no recorder is installed
   without `ObserveModule`, so they record nothing).
2. Panic catcher (a panic becomes a 500).
3. Request timeout (30 s).
4. Security headers (`X-Content-Type-Options`, `X-Frame-Options`,
   `Referrer-Policy`, `Strict-Transport-Security`, `Content-Security-Policy`).
5. Compression (gzip/brotli; only when the client sends `Accept-Encoding`,
   which wrk does not).
6. Error rendering (every error in Navis's NestJS-style JSON format).
7. Maintenance mode (`storage/framework/down`, checked at most once a second).
8. Per-IP rate limit (see below).
9. Body limit (2 MiB).
10. `AuthContext` slot: navis-auth's authenticator (the `JwtStrategy`) runs on
    every request and stores the principal.
11. `Uow` slot: navis-db's unit of work. A lazy unit of work per request; the
    rest of the request runs in a task on the app's tracker (so it completes
    even if the client disconnects); it commits when the status is below 400
    and rolls back otherwise.

Then the router and the per-route pipeline (route metadata, body limit,
the access check: `#[public]` or `#[authenticated]`), then the handler.
CORS is off (Navis's default when `http.cors.allowed_origins` is empty).

## What each endpoint does

- `/json`: a `Message` struct serialized per request.
- `/db`, `/queries`: `World::find_by_id(..).one(db.conn())` on the pool
  (the unit of work stays idle, so no transaction); `/queries` runs its N
  queries one after another in a plain loop. N is clamped to 1..=500, and a
  missing or non-numeric value is 1.
- `GET /api/deals`: the bearer token is verified in the pipeline,
  `CurrentUser` loads the user (one `SELECT` on the pool), `Valid<Query<DealsQuery>>`
  checks `page` ≥ 1 and `per_page` 1..=100 (defaults 1 and 20), then in the
  request's transaction: `paginate` (a `COUNT` and the page, ordered by
  `created_at DESC, id DESC`) and one `SELECT id, name FROM users WHERE id
  IN (..)` for the page's owners. The response is navis-db's `Page<T>`
  (`{"data", "meta": {"current_page", "per_page", "total", "last_page"}}`).
- `POST /api/deals`: token, `CurrentUser`, `Valid<Json<CreateDeal>>`
  (`title` 1..=200 characters, `amount_cents` integer 0..=10^12, `stage` one
  of the six), then `Repo::<Deal>::insert` in the unit of work (`INSERT ...
  RETURNING`), committed by the pipeline; 201 with a `Location` header.

## Judgement calls

- **Per-IP rate limit.** Navis limits every client IP to 600 requests a
  minute by default. wrk is one client IP, so the default would answer
  nearly everything with 429. The layer stays in the pipeline (its cost is
  measured) with `http.per_ip_rate_limit = 1000000000`; a production
  deployment behind a load balancer would instead list the proxy in
  `http.trusted_proxies` and keep a real limit per client.
- **Pagination input.** Navis's own `PageQuery` extractor clamps
  `per_page` to 100 and defaults to 15, while the spec wants a 400 above 100
  and a default of 20. So the query goes through navis-validation
  (`DealsQuery` with garde `range` rules) and feeds `PageQuery::new`,
  `paginate` and `Page`, Navis's pagination shape.
- **Transactions.** The deals routes use the request's unit of work, as the
  reference CRM (`apps/crm-api`) does: on PostgreSQL every unit-of-work
  transaction is `BEGIN`, `SELECT set_config('navis.actor', ..),
  set_config('navis.request_id', ..)`, the queries, `COMMIT`. `/db` and
  `/queries` read from the pool, since a single read needs no transaction.
- **Owners** come from a second query (`WHERE id IN (..)`, only `id` and
  `name`), the CRM's embed pattern, rather than a join.
- **Validation status** is 400: Navis answers every validation failure with
  400 (it has no 422s). Serde errors (missing field, fractional amount,
  unknown stage) are reported one at a time, before the garde rules.
- New deals get UUID v7 ids (Navis's convention) and `Utc::now()` timestamps.
- `auth.registration = false`; navis-auth's other routes (login, refresh,
  account) are mounted but unused.
- No `APP_KEY` is set: `CryptoModule` logs one warning at boot ("encryption
  is disabled"). Nothing in this API encrypts, and the JWT secret is
  configured explicitly.
- The pool is sqlx's default apart from its size, as navis-db configures it:
  `test_before_acquire` is on, so every checkout pings the connection (one
  extra round trip per checkout). navis-db has no setting for it.
- No Navis crate was changed for PostgreSQL: the paths this API uses (pool,
  unit of work with `set_config`, `paginate`, `Repo::insert`, navis-auth's
  user lookup) worked as they are.

## Build

The Docker build context is the repository root (`frameworks.json`), so
the app can depend on `crates/navis` by path; `Dockerfile.dockerignore`
narrows it to `Cargo.toml` (the workspace manifest the crates inherit
from), `crates/` and this directory (about 2 MB). The app is a Cargo
project of its own (`[workspace]` in its manifest; the root workspace
excludes `bench`), with its own `Cargo.lock`, built with `cargo build
--release --locked --jobs 6` and BuildKit cache mounts for the registry
and the target directory.

Locally: `cd bench/frameworks/navis && DATABASE_URL=postgres://bench:bench@127.0.0.1:55432/<db>
JWT_SECRET=... cargo run --release -- serve --addr 127.0.0.1:18080`.
