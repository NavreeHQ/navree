# Roadmap and known limitations removed from the site

Date: 2026-10-26. Written by the scrub worker before rewriting
`apps/website/content/docs/prologue/roadmap.mdx` (now forward-looking only) and
`prologue/release-notes.mdx` (now Navree 0.1 as the new site describes it).
The planner needs the list of missing features and known gaps; the site no
longer carries it.

## 1. `prologue/roadmap.mdx`, verbatim

````mdx
---
title: "Roadmap"
description: "What was deliberately left out of Navree 0.1, and the performance work the benchmark identified. No dates, no promises."
---

This page lists the work that lies ahead of Navree: the modules and drivers deliberately left out of 0.1, the performance problems the benchmark found, and the smaller gaps 0.1 shipped with. It is a list of known work, not a schedule. Nothing here has a date, and nothing here is promised; until something ships, plan as if it won't.

If one of these matters to you, [Contributing](/docs/prologue/contributing/) explains how to help.

## Deferred from 0.1

Navree's original requirements describe a much larger framework than 0.1. The first release covers the foundation and the modules a real application, the reference CRM, needed. These were left out on purpose:

| Deferred | What it would be | Why it isn't in 0.1 |
|---|---|---|
| Redis (`navree-redis`) and every Redis driver | A shared Redis connection pool, with Redis drivers for the cache, the queue, sessions, locks and scaling real time across servers | There was no Redis server to test against while 0.1 was built, so nothing Redis-backed could be verified. Each module has a driver abstraction, so drivers can be added without changing its API. |
| apalis as a queue driver | Another queue backend, built on the apalis crate | The database and memory drivers cover what the CRM needs. |
| Passkeys | WebAuthn sign-in, on webauthn-rs | webauthn-rs needs OpenSSL on Windows, and Navree avoids OpenSSL everywhere. |
| Social login and an OAuth2 server | Sign-in through other providers, and issuing tokens to third parties | Later phases of the original plan. |
| `navree-i18n` | Translations and locale handling | Not needed by the CRM. |
| `navree-flags` | Feature flags stored in the app's database | Not needed by the CRM. |
| `navree-billing` | Plans, subscriptions, invoices and payment webhooks | Depends on external payment services. |
| `navree-ai` | LLM providers, embeddings and an MCP server | Depends on external services. |
| `navree-grpc` and `navree-transport` | gRPC services, and message-broker transports for request/reply and events | Depend on external services; not needed by the CRM. |
| `navree-export` | PDF, spreadsheet and image helpers | Not needed by the CRM. |
| `navree-search` | Search-engine adapters and model indexing | Depends on external services. |
| `navree-admin` and `navree-graphql` | An admin API with a front-end kit, and GraphQL | The latest phase of the original plan. |
| Front-end kits, teams, multi-tenancy, OpenTelemetry and Sentry, a request inspector | A Nuxt kit next to the Next.js template, team-scoped roles, tenant isolation, tracing export, error reporting and a Telescope-style inspector | Later phases of the original plan. |
| PostgreSQL behaviour verified by tests | Running the test suite against a real PostgreSQL server | There was no PostgreSQL server while 0.1 was built. The PostgreSQL code paths exist, and the benchmark now runs Navree on PostgreSQL, but the test suite runs on SQLite. |

Two decisions taken during 0.1 also shape what comes next. Authentication follows NestJS (JWTs and rotating refresh tokens), so Laravel Sanctum's SPA cookie authentication and personal access tokens aren't built. Errors use the NestJS format, so there are no `application/problem+json` responses. Both were choices, not omissions.

## Performance work

The [benchmark](/benchmarks/) shows that Navree's own per-request cost is high. On plain JSON it delivers 22% of the throughput of a hand-written Axum and sqlx service built on the same libraries, and on a single database query 43%. Navree used nearly all its CPU in both tests while PostgreSQL had capacity to spare, so the time goes into Navree's request pipeline, not the database.

Reading the code behind those numbers found these causes. Fixing them is the next performance step:

1. **An extra database round trip per connection.** navree-db leaves sqlx's `test_before_acquire` on, so every checkout from the pool pings the database first: a single-query request costs two round trips instead of one. There is no setting to turn it off yet.
2. **An extra query per transaction.** Every unit-of-work transaction on PostgreSQL first runs `SELECT set_config('navree.actor', ...)` to record the acting user and the request id on the transaction: one more round trip before its first real query.
3. **A task per request.** The unit-of-work layer spawns a Tokio task for every request, including requests that never touch the database, such as the plain JSON test.
4. **Process-wide locks on every request.** Maintenance mode takes a mutex and a read-write lock, and the per-IP rate limiter uses one global `Mutex<HashMap>`.
5. **Work done for nobody.** The request-context layer builds metrics labels (fresh strings) even when no metrics recorder is installed, and creates a request scope and a UUID request id.
6. **A slow `CurrentUser`.** It finds `AuthService` by scanning every provider, and loads every column of the user, including the password hash and the two-factor secrets.
7. **JSON parsed twice.** Request bodies are parsed into a `serde_json::Value`, then into the DTO.
8. **Change events nobody reads.** `Repo::insert` serializes each new row for a change event even when nothing listens for change events.

A related kernel issue: `AppContext::get`, `try_get` and `get_named` search every provider on each call, so services that resolve optional dependencies per request pay for it every time.

Navree's memory use and startup are already good: 4 MB at idle, 38 MB at peak and 23 ms to ready in the benchmark. The work above is about throughput and latency.

## Smaller gaps

These shipped in 0.1 and are described in the [release notes](/docs/prologue/release-notes/#known-limitations):

- publishing the crates to crates.io;
- the configuration precedence of `.env` and `config/app.toml`, which is the opposite of what the generated files say;
- an `auth:prune` command for old refresh tokens;
- route-specific rate limits for registration and password reset, and a password confirmation for regenerating two-factor recovery codes;
- query parameters and non-JSON bodies in the OpenAPI document, and an OpenAPI schema for `Page<T>`;
- a soft-delete scope for record checks and `exists` rules;
- a size limit for the memory cache store;
- cloud disks for file storage, beyond the local filesystem;
- keeping the listening socket open across `navree dev` restarts;
- a command to assign roles to users;
- reporting validation's serde errors together with the garde rules, instead of one at a time.
````

## 2. `prologue/release-notes.mdx`, the "Design decisions", "Known limitations" and "Deliberately deferred" sections, verbatim

````mdx
## Design decisions

A few things work differently from what the original plan described:

- **Authentication and errors follow NestJS, not Laravel Sanctum.** Navree issues stateless JWT access tokens and rotating refresh tokens; navree-auth has no SPA cookie authentication, no personal access tokens and no session login. Errors use NestJS's `{statusCode, message, error}` format; there are no `application/problem+json` responses and no 422 or 419 statuses.
- **Real time is built in.** Instead of relying on an external WebSocket server, Navree ships an in-process Pusher-protocol server. Processes that don't serve the sockets send their broadcasts to the one that does over a signed HTTP relay.
- **The framework's own crates don't use the macros.** They implement `Module`, `Injectable` and `Controller` by hand, the escape hatch every app can use too. Apps use the macros.

## Known limitations

These are the gaps in 0.1.0 you may run into. Most are listed again, as future work, on the [roadmap](/docs/prologue/roadmap/).

### Installation and tooling

- The crates aren't on crates.io. Without a local checkout, `navree new` writes `navree = "0.1.0"`, which doesn't resolve yet.
- `navree help` prints the CLI's own help, not your app's commands, although the generated README says otherwise. Use `navree list`.
- `navree dev` doesn't keep the listening socket open across restarts, so requests fail while the server restarts. If `navree dev` is killed forcibly, the server it started keeps running.

### Configuration

- In `ConfigSources::defaults()`, `config/app.toml` overrides `.env`, not the other way round. A key set in both keeps the TOML value, so editing `APP_HTTP__ADDR` or `APP_DATABASE__URL` in a new app's `.env` has no effect. Real environment variables do override the file. The comments in the generated `.env.example` and `config/app.toml` describe this the wrong way round. See [Configuration](/docs/getting-started/configuration/#where-settings-come-from).
- `ConfigSources::defaults()` reads the real `APP_*` environment variables, so tests that use it can be affected by the machine they run on. Build test configuration with `ConfigSources::new()`.

### Database

- PostgreSQL support compiles and runs in the benchmark, but the test suite runs on SQLite only. Some PostgreSQL paths haven't been run against a real server yet, among them migrations, the `FOR UPDATE` row locks in `Repo::update`, `Repo::find_for_update` and record checks, and the queue's `FOR UPDATE SKIP LOCKED` job reservation; the [database page](/docs/database/getting-started/#postgresql) has the full list.
- navree-db leaves sqlx's `test_before_acquire` on, so each connection checkout costs an extra round trip, and there is no setting to turn it off.

### Authentication and security

- A request with a present but invalid JWT gets a 401 on every route, public ones included. Clients must not send stale access tokens to `/api/auth/login`, `/api/auth/refresh` or the health probes.
- Access tokens are stateless: they stay valid until they expire (15 minutes by default), even after logout or a password change.
- The `refresh_tokens` table grows with every refresh and there is no command to prune it yet. A refresh token's lifetime slides: each rotation gets a fresh 30 days.
- Registration, forgot-password and reset-password have no route-specific rate limits: only the global per-IP limit and the per-address mail throttle apply.
- `POST /api/auth/2fa/enable` and `POST /api/auth/2fa/recovery-codes` need only a valid access token, not the password.
- A bad or expired signed URL answers 401, where Laravel answers 403.

### Validation and access

- serde errors (a wrong type, a missing field, an unknown enum value) are reported one at a time, before and instead of the garde rules. A body with an empty `name` and an invalid `size` reports only `size`.
- The record loader behind `#[can(.., from = "id")]` has no soft-delete scope, so a user whose rules deny the action gets 403 rather than 404 on a soft-deleted record. Async `exists` rules can't add `deleted_at IS NULL` either; check deleted records in your service.
- A record check that fails answers 403 even when the user may not read the record, which tells the caller the id exists.
- There is no command to assign a role to a user; use the access management routes or a seeder.

### OpenAPI

- The document can't describe query parameters or non-JSON bodies, such as a CSV export.
- navree-db's `Page<T>` has no OpenAPI schema, so a paginated route documents its response with a page struct of its own, as the CRM's `DealPage` does (`#[api(response = DealPage)]`).
- The `user` in login responses and `GET /api/auth/me` is documented without the fields an app adds to it.
- Framework routes without schemas: the abilities and access management routes, broadcasting auth, the WebSocket endpoint, the health probes and `/metrics`.
- Every operation lists a `cookie` security scheme, even in apps that use bearer tokens only.
- The docs UI loads its script from jsDelivr without a subresource integrity hash.

### HTTP and other modules

- navree-http doesn't map multipart errors itself: only apps with `StorageModule` answer malformed uploads with a 4xx instead of a 500.
- navree-http-client can't pick up the current request's id on its own; pass it with `.with_request_id(&id)`.
- The memory cache store has no size limit, and there are no Redis drivers for the cache, the queue, sessions, locks or real-time scaling.
- Storage disks are the local filesystem and memory only.
- navree-session still serves `GET /sanctum/csrf-cookie`, a leftover from the original Sanctum plan. The module is optional.

### Kernel and testing

- A singleton that stores the `AppContext` creates a reference cycle, so that app's memory is never freed. It doesn't matter for a long-running process, but leaks in tests that boot many apps; resolve lazily with `r.lazy::<T>()` instead.
- A provider registered with `.lifecycle::<T>()` must be a singleton. A request-scoped one fails at boot, and `check` doesn't catch it.
- On Windows, a `TestApp` dropped without `app.shutdown().await` may leave its SQLite files in the temporary directory, because the pool is still open.
- `TestAppBuilder::fake_auth` replaces the app's authenticator completely, so one test app can't mix `acting_as` with real bearer tokens.

### Performance

Navree's own per-request cost is high for a Rust framework: on plain JSON it delivers 22% of a hand-written Axum service's throughput. The benchmark's findings trace this to specific parts of the pipeline, listed on the [roadmap](/docs/prologue/roadmap/#performance-work).

## Deliberately deferred

Redis drivers, apalis as a queue backend, passkeys, social login, an OAuth2 server, billing, AI, gRPC, search and other modules were left out of 0.1 on purpose. The [roadmap](/docs/prologue/roadmap/) lists them with the reasons.
````

## 3. Root `README.md`, the "Deliberate deviations from the PRD" section, verbatim

Replaced in the README by a "Design decisions" section in Navree's own terms; the deferred table is kept here for the planner.

````md
## Deliberate deviations from the PRD

The PRD is a 60 engineer-month plan. This build covers its Phase 0 and
Phase 1 plus the Phase 2 modules the CRM needs (spec §2), and the user
changed the auth and error direction during the build (spec §10).

**Auth and errors follow NestJS, not Laravel Sanctum (spec §10).**

- Authentication is stateless JWT access tokens (HS256, 15 minutes) plus
  opaque rotating refresh tokens (30 days, SHA-256 at rest; reusing one
  revokes its whole family), a Passport-style `Strategy` trait, a global
  guard with a `#[public]` opt-out and a `CurrentUser` extractor. The PRD's
  Sanctum SPA cookie auth, CSRF-cookie endpoint, personal access tokens and
  session login are not built. `navree-session` exists but is optional, and
  the CRM doesn't use it.
- The browser never holds tokens: the Next.js server is a BFF that keeps
  them in httpOnly cookies and forwards `Authorization: Bearer`.
- Errors use the NestJS format `{statusCode, message, error}`, with
  validation failures as 400 plus `errors: {field: [..]}`, instead of the
  PRD's RFC 9457 `application/problem+json`. Statuses 422 and 419 are not
  used. The PRD text still mentions problem+json, 422 and 419 in places;
  spec §10 overrides it.

**Deferred modules and drivers (spec §2).**

| Deferred | Why |
|---|---|
| `navree-redis` and every Redis driver (cache, queue, session, locks, real-time scaling) | No Redis server on the build machine, so nothing could be tested. Each module has a driver trait, so Redis drivers can be added without API changes |
| apalis as the queue driver | The native database driver and a memory driver cover the PRD's fallback; apalis can come later as another driver |
| Passkeys (webauthn-rs), social login, an OAuth2 server | webauthn-rs needs OpenSSL on Windows; the other two are PRD Phase 3-4 |
| navree-i18n, navree-flags, navree-billing, navree-ai, navree-grpc, navree-transport, navree-export, navree-search, navree-admin, navree-graphql | Phase 2-4 modules with external services; the CRM doesn't need them |
| Nuxt kit, teams, multi-tenancy, OpenTelemetry and Sentry, the request inspector | Phase 2-3 |

**Other differences.**

- PostgreSQL is the PRD's primary database, but there was none on the build
  machine. PostgreSQL-only paths (`FOR UPDATE`, `SKIP LOCKED`, advisory
  locks) are written behind `DbBackend` checks and the `postgres` feature
  compiles, but every test runs on SQLite, the PRD's development and test
  database.
- Real-time: the PRD plans Sockudo first and an in-process server later.
  Navree ships the in-process Pusher-protocol server now (Laravel Echo and
  pusher-js work unchanged). Processes that don't serve the sockets, such
  as queue workers, send their broadcasts to the serving process over a
  signed HTTP relay.
- Framework crates implement `Module`, `Injectable` and `Controller` by hand
  (the PRD's escape hatch) instead of using `navree-macros`, so every crate
  could be built before the macros existed. Apps and the facade's tests use
  the macros.

````
