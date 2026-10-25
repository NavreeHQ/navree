<img src="assets/icon.svg" width="64" height="64" alt="">

# navree

Created in [T3 Code](https://t3.codes).

Navree is an application framework for Rust on Tokio. It aims to be as complete as
Laravel (routing, validation, ORM, auth, queues, scheduler, cache, mail,
notifications, storage, real-time, testing, CLI generators) and structured
like NestJS: everything above a small kernel is a module that declares its
imports, providers, controllers and exports, and an app is a composition of
modules. Navree builds on maintained crates (Axum, Tower, SeaORM, garde,
argon2, minijinja, lettre, object_store, utoipa, ...) and adds the glue: a
dependency-injection container, deny-by-default routing with CASL-compatible
abilities, a unit of work per request, NestJS-format errors, logging and an
audit trail that are on by default.

The repository is a Cargo workspace of small module crates, plus a reference
CRM built on top of them:

- `apps/crm-api`: the Navree CRM API, a Navree application.
- `apps/crm-web`: the CRM front end (Next.js 16). Its server is a
  backend-for-frontend (BFF) that holds the API tokens in httpOnly cookies.

Documents:

- Product requirements: [`docs/prd/navree-prd.md`](docs/prd/navree-prd.md)
- Build design (crate hierarchy, decisions, CRM API contract):
  [`docs/design`](docs/design)
- Implementation plan:
  [`docs/design`](docs/design)
- Framework notes and known open issues:
  [`docs/design`](docs/design)

## Crate hierarchy

The crates form eight layers (spec §3). A crate may depend only on crates in
lower layers, dev-dependencies included, and its runtime dependencies must
also match the spec's table of allowed edges. Cross-cutting needs (audit,
events, broadcasting, sessions, health checks, jobs, the current principal)
go through traits in `navree-contracts`, so modules don't depend on each
other sideways. `crates/navree/tests/dependency_graph.rs` checks every edge on
each test run.

```
L7  apps/crm-api (Navree app)                   apps/crm-web (Next.js 16)
L6  navree (facade: re-exports + prelude)       navree-cli (binary `navree`; no runtime navree deps)
L5  navree-auth
L4  navree-session  navree-validation  navree-queue  navree-audit  navree-access  navree-schedule  navree-notify
L3  navree-db  navree-testing  navree-observe  navree-api  navree-mail  navree-realtime  navree-storage
L2  navree-http  navree-log  navree-crypto  navree-events  navree-view  navree-cache  navree-http-client
L1  navree-contracts  navree-config
L0  navree-kernel                                navree-macros (proc-macro; no runtime navree deps)
```

| Layer | Crate | What it provides |
|---|---|---|
| L0 | `navree-kernel` | Module graph, type-keyed DI container, lifecycle, command runner |
| L0 | `navree-macros` | `#[module]`, `#[injectable]`, `#[controller]`, route and access attributes |
| L1 | `navree-contracts` | Shared traits and types: `Secret<T>`, `RequestId`, `Principal`, `AuthState`, sinks, `Broadcaster`, jobs, `Clock` |
| L1 | `navree-config` | Typed configuration from `.env`, TOML files and `APP_*` variables |
| L2 | `navree-http` | Axum integration, default Tower layers, NestJS-format errors, controllers, rate limits, trusted proxies |
| L2 | `navree-log` | tracing subscriber, JSON or pretty output, redaction, rotating files |
| L2 | `navree-crypto` | AES-256-GCM encrypter with key rotation, Argon2id hasher, token helpers |
| L2 | `navree-events` | Typed event bus with sync and queued listeners |
| L2 | `navree-view` | MiniJinja templates (mail and other text) |
| L2 | `navree-cache` | Cache stores (memory on moka), remember, tags, locks |
| L2 | `navree-http-client` | Outgoing HTTP with retries, timeouts, an SSRF policy and fakes |
| L3 | `navree-db` | SeaORM integration, per-request unit of work, migrations, seeders, factories, pagination |
| L3 | `navree-testing` | `TestApp`, an in-process `TestClient`, fakes |
| L3 | `navree-observe` | Health checks, readiness, Prometheus metrics |
| L3 | `navree-api` | OpenAPI 3.1 from route metadata, docs UI, API resources |
| L3 | `navree-mail` | Mailables from templates; SMTP, log, array and failover transports; queued sending |
| L3 | `navree-realtime` | Pusher-protocol WebSocket server (public, private, presence channels) and a signed relay for other processes |
| L3 | `navree-storage` | Named disks on object_store, streaming, signed temporary URLs, upload checks |
| L4 | `navree-session` | Encrypted session cookies, memory and database stores, CSRF (optional; the CRM doesn't use it) |
| L4 | `navree-validation` | `Valid<T>` extractors on garde, class-validator style messages, async unique/exists rules |
| L4 | `navree-queue` | Database and memory job queues, workers, retries with backoff, unique and delayed jobs |
| L4 | `navree-audit` | Audit trail of data changes, sign-ins, permission changes and denials |
| L4 | `navree-access` | Roles, permissions and CASL-compatible abilities, record checks, SQL list filters |
| L4 | `navree-schedule` | Cron-style schedules, overlap prevention, single-node runs |
| L4 | `navree-notify` | Notifications over mail, database and broadcast, with read state |
| L5 | `navree-auth` | NestJS-style JWT auth: rotating refresh tokens, strategies, accounts, 2FA, throttles |
| L6 | `navree` | The facade: re-exports every module crate (one Cargo feature each, `full` by default) and the macros |
| L6 | `navree-cli` | The `navree` binary: `navree new`, `navree make:*`, `navree dev` |

The CRM API is split the same way: `crm-core` (entities, migrations,
abilities, platform modules), `crm-companies` (companies and contacts),
`crm-deals`, `crm-activities` (activities and the dashboard), `crm-users`,
`crm-testkit` (a booted CRM for feature tests) and the `crm-api` binary.

## Requirements

- Rust 1.95 or newer (edition 2024; the build machine uses 1.99.1). On
  Windows, the MSVC build tools (SQLite is compiled from source through the
  bundled `libsqlite3-sys`).
- [sccache](https://github.com/mozilla/sccache): `.cargo/config.toml` sets
  `rustc-wrapper = "sccache"`. Remove that line if you don't want it.
- For the web app: Node.js 20.9 or newer (the build machine uses 24) and
  pnpm 11.

No OpenSSL, CMake or NASM is needed: TLS is rustls with the `ring` provider
everywhere. No database server is needed either: tests and development use
SQLite.

## Running the tests

### Rust (framework and CRM API)

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`cargo test --workspace` runs every crate's unit and integration tests,
including the CRM API's feature tests and the dependency-graph test. Each
test that touches a database gets its own temporary SQLite file.

Several builds (people, CI jobs or agents) often run at the same time. Cargo
locks the target directory, so give each one its own and let sccache share
the compiled dependencies:

```sh
CARGO_TARGET_DIR=target/agents/<label> cargo test -p <crate>
```

For example `CARGO_TARGET_DIR=target/agents/http cargo test -p navree-http`.
Use `-p <crate>` while working on one crate, and keep `CARGO_BUILD_JOBS`
moderate when memory is tight.

### Web (crm-web)

```sh
cd apps/crm-web
pnpm install
pnpm lint        # ESLint
pnpm typecheck   # tsc --noEmit
pnpm test        # Vitest: BFF route handlers, API client, CASL, audit actions, mock API
pnpm build       # production build
```

### End to end (Playwright, the whole system)

```sh
cd apps/crm-web
pnpm exec playwright install chromium   # once
E2E_REAL_API=1 pnpm e2e
```

This needs cargo and nothing listening on ports 8080 or 3000. Playwright
starts crm-api on a fresh, seeded database of its own
(`apps/crm-api/storage/e2e.db`; `migrate:fresh`, `db:seed crm-demo`, then
`serve`, built into `target/agents/crm-e2e`) and the production build of
crm-web (`pnpm build && pnpm start`), then runs every spec in a real
Chromium against them, one spec at a time. The first run compiles the API,
which takes several minutes. Without `E2E_REAL_API=1`, `pnpm e2e` only runs
the smoke tests against the in-browser mock API. See
[`apps/crm-web/README.md`](apps/crm-web/README.md) for the details.

## Running the CRM

Run each long-running command in its own terminal.

**1. The API** (from `apps/crm-api`; it reads `.env` and `config/app.toml`
from the working directory):

```sh
cd apps/crm-api
cargo run -p crm-api -- key:generate            # prints base64:...; put it in .env as APP_KEY=base64:...
cargo run -p crm-api -- migrate:fresh --seed    # schema + demo data in storage/crm.db
cargo run -p crm-api -- serve                   # REST API and Pusher WebSocket on http://127.0.0.1:8080
cargo run -p crm-api -- queue:work              # optional: deal notifications and mail (mail goes to the log)
cargo run -p crm-api -- schedule:work           # optional: 08:00 UTC task reminders, daily audit pruning
```

Without an `APP_KEY` the API signs tokens with a random key per process, so
every restart signs everyone out. `.env.example` lists the other settings
(database URL, trusted proxies, SMTP, the realtime relay). Workers hand
their broadcasts to `serve` through `realtime.relay_url` (`auto` = the
`http.addr` of `serve`), signed with a key derived from `APP_KEY`, so all
processes need the same `.env`. `cargo run -p crm-api -- list` shows every
command. The OpenAPI document is at
http://127.0.0.1:8080/api/openapi.json.

**2. The web app** (from `apps/crm-web`):

```sh
cd apps/crm-web
pnpm install
pnpm dev                                        # http://localhost:3000
```

It talks to the API at `NAVREE_API_URL` (default `http://127.0.0.1:8080`).
`pnpm dev:mock` runs the web app alone against an in-browser mock of the
API, with the same demo accounts.

**Demo accounts** (all with password `password123`):

| Email | Role | Can |
|---|---|---|
| `admin@navree.test` | admin | Everything, including users, roles and the audit log |
| `manager@navree.test` | manager | All customer records, reassign deals, export contacts, reopen won/lost deals (only admin deletes them) |
| `rep1@navree.test` | rep | Read everything; create records; edit and delete only their own; can't delete or reopen won/lost deals |
| `rep2@navree.test` | rep | Same as rep1 (a second owner, to see ownership rules and live updates) |

The seed also creates about 20 companies, 50 contacts, 30 deals across the
pipeline stages and 60 activities. Open the deals board as two users in two
browsers to see moves arrive over the WebSocket.

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

## Layout

| Path | What |
|---|---|
| `crates/navree` | Facade: re-exports every module crate and the macros; holds the full-stack and dependency-graph tests |
| `crates/navree-*` | One crate per module (see [Crate hierarchy](#crate-hierarchy)) |
| `apps/crm-api` | The CRM API, a Navree application (`src/`), its feature crates (`crates/`) and system tests (`tests/`) |
| `apps/crm-web` | The CRM front end (Next.js), with Vitest unit tests and Playwright specs in `e2e/` |
| `docs/` | PRD, build design, implementation plan, notes |

### Features and drivers

- Databases: SQLite (bundled) by default; enable PostgreSQL with the
  `postgres` feature of `navree` (or `navree-db`).
- External versions live only in the root `[workspace.dependencies]`; crates
  use `dep = { workspace = true }`.
