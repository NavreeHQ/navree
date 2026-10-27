<img src="assets/icon.svg" width="64" height="64" alt="">

# navree

Created in [T3 Code](https://t3.codes).

**The complete backend framework for Rust.** Modular, API-first and
AI-native. Everything your backend needs, checked by the compiler.

Navree is a framework for building efficient, secure and scalable
server-side applications in Rust. It gives you an application architecture
out of the box: features live in modules, modules declare what they import
and export, and a dependency-injection container wires them together when
the app boots, refusing to start if anything is missing. Around that
structure sits a complete standard library for backends, from routing and
validation to workspaces, queues, monitoring and AI, designed together,
documented together and released together. The result is code that stays
easy to test, easy to change and easy to hand to a new teammate, or to an
AI agent.

- **Modular by design.** Every feature is a module that declares what it
  imports, provides and exports.
- **Dependency injection.** A type-keyed container wires services at boot
  and refuses to start with a missing or private dependency, naming the
  module and the type.
- **API-first.** Versioned routes, validated input, one JSON error shape
  (`{ "statusCode", "message", "error", "code" }`, with stable codes a
  front end translates) and an OpenAPI 3.1 document generated from your
  code.
- **Secure by default.** Routes deny until you allow them. Rotating refresh
  tokens, two-factor, Argon2id, encrypted cookies, CSRF, rate limits,
  security headers, an audit log and an SSRF guard ship on.
- **Workspaces built in.** Multi-tenancy at the workspace level: members,
  roles, invitations, and isolation by row, schema or database.
- **AI-native.** Agent skills, an MCP server for your coding agent, docs
  written for models as well as people, and an AI SDK for the product
  itself.
- **One binary**, built on Tokio, Axum, Hyper, Tower, SeaORM, sqlx,
  garde, rustls and Argon2.

```sh
cargo install --git https://github.com/NavreeHQ/navree navree-cli
navree new blog
cd blog
navree key:generate     # then set APP_KEY in .env
navree dev              # http://127.0.0.1:8080/api/docs
```

**Documentation:** [navree.com/docs](https://navree.com/docs/prologue/introduction/).
The docs are also published for AI agents as
[`llms.txt`](https://navree.com/llms.txt) and as Markdown, page by page.

## The ecosystem

Everything a production backend needs beyond the framework is first-party,
open source and free, documented on the
[ecosystem page](https://navree.com/ecosystem/):

- **Tools:** Lighthouse (application monitoring), Logbook (a local
  debugging timeline), Fleet (queue dashboard and worker supervisor), Helm
  (an admin panel generated from your entities), Harbor (servers and
  zero-downtime deployments), Dock (local services for development) and
  Navigator (the MCP server for your coding agent).
- **Packages:** search, the AI SDK, MCP servers, microservices, gRPC,
  GraphQL, an OAuth 2.1 server, social sign-in, billing, feature flags,
  webhooks, localization and browser testing.
- **Starter kits:** `navree new <name> --kit saas|api|micro`.
- **Agent skills** for every common task, in [`skills/`](skills/).

The ecosystem build plan in
[`docs/superpowers/plans/`](docs/superpowers/plans/) tracks how each
package lands in this repository.

## This repository

The repository is a Cargo workspace of small module crates, plus a reference
CRM built on top of them and the website:

- `apps/crm-api`: the Navree CRM API, a Navree application.
- `apps/crm-web`: the CRM front end (Next.js 16). Its server is a
  backend-for-frontend (BFF) that holds the API tokens in httpOnly cookies.
- `apps/website`: navree.com and the documentation (Next.js, MDX).
  [`apps/website/AUTHORING.md`](apps/website/AUTHORING.md) is the guide for
  writing docs pages.

Documents:

- Product requirements: [`docs/prd/navree-prd.md`](docs/prd/navree-prd.md)
- Build design (crate hierarchy, decisions, CRM API contract):
  [`docs/superpowers/specs/2026-10-25-navree-framework-design.md`](docs/superpowers/specs/2026-10-25-navree-framework-design.md)
- Implementation plan:
  [`docs/superpowers/plans/2026-10-25-navree-framework.md`](docs/superpowers/plans/2026-10-25-navree-framework.md)
- Framework notes and known open issues:
  [`docs/superpowers/notes/`](docs/superpowers/notes/)

## Crate hierarchy

The crates form layers (spec §3): the framework from L0 to L5, ecosystem
libraries at L6, packages built on them at L7, and the facade and CLI at L8.
A crate may depend only on crates in lower layers, dev-dependencies included,
and its runtime dependencies must also match the spec's table of allowed
edges. Cross-cutting needs (audit,
events, broadcasting, sessions, health checks, jobs, the current principal)
go through traits in `navree-contracts`, so modules don't depend on each
other sideways. `crates/navree/tests/dependency_graph.rs` checks every edge on
each test run.

```
L9+ apps/crm-api (Navree app)                   apps/crm-web (Next.js 16)
L8  navree (facade: re-exports + prelude)       navree-cli (binary `navree`; no runtime navree deps, uses navree-dock and navree-harbor)
L7  navree-billing  navree-search  navree-lighthouse  navree-logbook  navree-fleet  navree-helm (packages built on other packages)
L6  navree-social  navree-webhooks  navree-oauth  navree-flags  navree-graphql  navree-grpc  navree-mcp  navree-ai  navree-micro (ecosystem libraries)
L5  navree-auth  navree-workspace  navree-redis
L4  navree-session  navree-validation  navree-queue  navree-audit  navree-access  navree-schedule  navree-notify  navree-ui  navree-orm  navree-browser
L3  navree-db  navree-testing  navree-observe  navree-api  navree-mail  navree-realtime  navree-storage
L2  navree-http  navree-log  navree-crypto  navree-events  navree-view  navree-cache  navree-http-client  navree-schema
L1  navree-contracts  navree-config  navree-telemetry
L0  navree-kernel                                navree-macros  navree-orm-macros (proc-macros; no runtime navree deps)   navree-dock, navree-harbor (tools; no navree deps)
```

| Layer | Crate | What it provides |
|---|---|---|
| L0 | `navree-kernel` | Module graph, type-keyed DI container, lifecycle, command runner |
| L0 | `navree-macros` | `#[module]`, `#[injectable]`, `#[controller]`, `#[resolver]`, route and access attributes |
| L0 | `navree-orm-macros` | `#[derive(Model)]` and `#[scopes]` (re-exported by `navree-orm`) |
| L0 | `navree-dock` | Dock: `dock.toml`, local PostgreSQL, Redis, Meilisearch, Mailpit, MinIO and NATS through `docker compose` or `podman compose`, the app's `.env` settings (behind `navree dock`; opt-in facade feature `dock`) |
| L0 | `navree-harbor` | Harbor: `harbor.toml`, Ubuntu provisioning over SSH (Caddy with TLS, PostgreSQL, Redis, ufw, a deploy user, systemd units), blue-green releases with health checks and automatic rollback, AES-256-GCM secrets, several servers and roles, `build:image` and a GitHub Actions workflow (behind `navree harbor:*`, `navree deploy*` and `navree build:image`; opt-in facade feature `harbor`) |
| L1 | `navree-contracts` | Shared traits and types: `Secret<T>`, `RequestId`, `Principal`, `AuthState`, sinks, `Broadcaster`, jobs with progress and cancellation, permission checks, `Clock`, conformity checks (`conformity:check`) |
| L1 | `navree-config` | Typed configuration from `.env`, TOML files and `APP_*` variables |
| L1 | `navree-telemetry` | Typed telemetry events (requests, queries, jobs, scheduled tasks, mail, notifications, cache, outgoing HTTP, exceptions, log lines, app events) delivered to registered sinks, `TelemetryRecorder`, W3C trace context |
| L2 | `navree-http` | Axum integration, default Tower layers, JSON errors, controllers, rate limits, trusted proxies |
| L2 | `navree-log` | tracing subscriber, JSON or pretty output, redaction, rotating files |
| L2 | `navree-crypto` | AES-256-GCM encrypter with key rotation, Argon2id hasher, token helpers, JOSE helpers (JWS, JWK, PKCS#8 on `ring`) |
| L2 | `navree-events` | Typed event bus with sync and queued listeners |
| L2 | `navree-view` | MiniJinja templates (mail and other text) |
| L2 | `navree-cache` | Cache stores (memory on moka; Redis with navree-redis), remember, tags, locks |
| L2 | `navree-http-client` | Outgoing HTTP with retries, timeouts, an SSRF policy and fakes |
| L2 | `navree-schema` | The schema builder: tables, columns, indexes and foreign keys in Rust for PostgreSQL, MySQL/MariaDB and SQLite (SQLite alterations by table rebuild); TimescaleDB hypertables, compression, retention, reorder policies and continuous aggregates with fallbacks on other databases; introspection; schema dumps; migration files (`navree::schema`) |
| L3 | `navree-db` | SeaORM integration, per-request unit of work, migrations (schema and SeaORM, batches, steps, rollback, reset, refresh, fresh, `--pretend`, `schema:dump`), seeders, factories, pagination |
| L3 | `navree-testing` | `TestApp`, an in-process `TestClient`, fakes, cross-tenant isolation suites generated from a module's routes |
| L3 | `navree-observe` | Health checks, readiness, Prometheus metrics, tracing (spans from telemetry events, `SpanRecorder`, OTLP export behind the opt-in facade feature `otel`) |
| L3 | `navree-api` | OpenAPI 3.1 from route metadata, docs UI, API resources, `api.require_docs` and the `api.docs` check |
| L3 | `navree-mail` | Mailables from templates; SMTP, log, array and failover transports; queued sending |
| L3 | `navree-realtime` | Pusher-protocol WebSocket server (public, private, presence channels), a signed relay for other processes, and a backplane (Redis with navree-redis) for several serving processes |
| L3 | `navree-storage` | Named disks on object_store (local, memory, S3 and S3-compatible stores, Google Cloud Storage, Azure; facade features `storage-s3`, in `full`, and opt-in `storage-gcs`, `storage-azure`), streaming, provider-signed temporary URLs, direct browser uploads, upload checks, `storage:sync` |
| L4 | `navree-session` | Encrypted session cookies, memory, database and Redis stores, CSRF (optional; the CRM doesn't use it) |
| L4 | `navree-validation` | `Valid<T>` extractors on garde, field-level messages, async unique/exists rules, international input (E.164 `Phone`, `SafeHtml`, names and digits in any script, `Country`, `Locale`) |
| L4 | `navree-queue` | Database, memory and Redis job queues (the `QueueStore` driver trait), workers, retries with backoff, unique and delayed jobs; job batches with callbacks; operations with progress, an SSE stream, cancel and a watchdog |
| L4 | `navree-audit` | Audit trail of data changes, sign-ins, permission changes and denials |
| L4 | `navree-access` | Roles, permissions and abilities (shared with front ends as CASL JSON), record checks, SQL list filters |
| L4 | `navree-schedule` | Cron schedules, overlap prevention, single-node runs |
| L4 | `navree-notify` | Notifications over mail, database and broadcast, with read state |
| L4 | `navree-ui` | The UI kit of the server-rendered tools: the app shell (collapsible sidebar, top bar, command palette), components as MiniJinja macros, embedded and versioned CSS, JS and fonts (facade feature `ui`) |
| L4 | `navree-orm` | The ORM: `#[derive(Model)]` models with typed columns and keys (integers, UUID v7, ULID), a query builder with every where variant, joins, groups, subqueries, unions, locks and TimescaleDB's `time_bucket`/`first`/`last`, records with dirty tracking, casts, scopes, lifecycle hooks, collections and offset, simple and cursor pagination, on PostgreSQL/TimescaleDB, MySQL and SQLite through the unit of work, and a document-driver interface (facade feature `orm`) |
| L4 | `navree-browser` | Browser testing: Chrome, Chromium or Edge over the DevTools Protocol, actions and assertions that wait, failure screenshots, `TestServer` on a real port and `FrontEnd` sessions (a dev-dependency; opt-in facade feature `browser`; `navree browser:install` downloads Chrome for Testing) |
| L5 | `navree-auth` | JWT auth: rotating refresh tokens, strategies, accounts, 2FA, throttles |
| L5 | `navree-workspace` | Workspaces: members, roles, invitations, row, schema or database isolation; organizations above workspaces and partitions inside them; entity classification and isolation checks, workspace isolation suites (feature `testing`) |
| L5 | `navree-redis` | Redis: shared connections (lazy, reconnecting, TLS), Redis drivers for the cache and locks, the queue, sessions and the real-time backplane, shared rate limits (facade feature `redis`, opt-in) |
| L6 | `navree-social` | Social sign-in: Google, GitHub, Apple, Microsoft, GitLab, Slack, LinkedIn, Discord, X and OpenID Connect, linked to Navree users (facade feature `social`, opt-in) |
| L6 | `navree-webhooks` | Webhooks: signed outgoing deliveries with queue-backed retries and a delivery log, verified incoming webhooks (Stripe, GitHub, Paddle, Standard Webhooks) |
| L6 | `navree-oauth` | OAuth 2.1 and OpenID Connect provider: PKCE, client credentials, device and refresh grants, scopes mapped to permissions, personal access tokens (facade feature `oauth`, opt-in) |
| L6 | `navree-flags` | Feature flags: `#[flag]` types, checks per user, workspace, plan or percentage with stable hashing, a kill switch, `#[requires_flag]` routes, database storage with a cache (facade feature `flags`, opt-in) |
| L6 | `navree-graphql` | GraphQL on async-graphql: `#[resolver]` fields in modules with route-style access, per-request dataloaders, subscriptions over WebSockets, GraphiQL, `graphql:schema` (facade feature `graphql`, opt-in) |
| L6 | `navree-grpc` | gRPC on tonic: `#[grpc_service]` services in modules with DI, guards, access rules and the unit of work; gRPC-Web, Connect, health, reflection, named client channels (facade feature `grpc`, opt-in) |
| L6 | `navree-mcp` | MCP servers: `#[mcp_server]` tools, resources and prompts over streamable HTTP at `/mcp` and stdio (`mcp:serve`), behind the app's authenticators and per-tool access checks (opt-in facade feature `mcp`) |
| L6 | `navree-ai` | AI SDK: text, typed structured output and SSE streams from Anthropic, OpenAI, Gemini, Mistral, Ollama and Bedrock, failover, token budgets, agents with `#[tool]` methods run as the user, conversation memory, embeddings, vector stores (memory, pgvector, Qdrant) and `AiFake` (facade feature `ai`, opt-in) |
| L6 | `navree-micro` | Microservices: `#[message_pattern]` and `#[event_pattern]` handlers with the same guards, access, validation and unit of work as routes, `ServiceClient` (`send` with deadlines, `emit`), retries, dead letters, an outbox and an inbox, `MicroFake`; TCP and in-memory transports, NATS, Kafka, RabbitMQ, Redis Streams and MQTT behind features (facade features `micro`, `micro-nats`, ..., opt-in) |
| L7 | `navree-billing` | Billing for users or workspaces: Stripe and Paddle clients, plans, trials, seats, metered usage, invoices, the portal, verified webhooks on the queue, a fake (facade feature `billing`, opt-in) |
| L7 | `navree-search` | Full-text and vector search: `#[derive(Searchable)]`, PostgreSQL/SQLite, Meilisearch, Typesense, Tantivy and memory engines, synced through the queue (facade feature `search`, opt-in) |
| L7 | `navree-lighthouse` | Application monitoring inside the app: a telemetry sink grouping exceptions, slow requests and queries, failed jobs and failing outgoing calls into issues, per-minute aggregates and sampled traces in the app's database, alerts (mail, Slack, signed webhooks, your own channels) through the queue, the `/_lighthouse` dashboard and its JSON API, `lighthouse:prune` (facade feature `lighthouse`, in `full`) |
| L7 | `navree-logbook` | Logbook: a local debugging timeline at `/_logbook` of every request with its queries, events, jobs, mail, cache calls and log lines; filters by tag and user, duplicate-query detection, request replay, a bounded SQLite or memory store (facade feature `logbook`, in `full`; off in release builds) |
| L7 | `navree-fleet` | Fleet: a supervisor of queue worker processes (`fleet`) with auto-balancing, recycling and graceful restarts; throughput, wait and runtime metrics; failed jobs; pause and continue; long-wait alerts; a dashboard at `/_fleet` (facade feature `fleet`, in `full`) |
| L7 | `navree-helm` | Helm: the admin panel at `/admin` generated from your entities (resources with fields, filters, search, actions and metrics), checked against your access rules, audited, server-rendered with an embedded stylesheet and script, plus its JSON API |
| L8 | `navree` | The facade: re-exports every module crate (one Cargo feature each, `full` by default) and the macros |
| L8 | `navree-cli` | The `navree` binary: `navree new`, `navree make:*`, `navree dev`, `navree dock`, `navree check`, `navree gate`, `navree harbor:*`, `navree deploy`, `navree build:image`, `navree browser:install` |

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

## Design decisions

The build covers the PRD's foundation and the modules the CRM needs (spec
§2); spec §10 records the decisions taken during the build.

- **Token authentication.** Stateless JWT access tokens (HS256, 15 minutes)
  plus opaque rotating refresh tokens (30 days, SHA-256 at rest; reusing
  one revokes its whole family), a pluggable `Strategy` trait, a global
  guard with a `#[public]` opt-out and a `CurrentUser` extractor.
  `navree-session` (cookie sessions and CSRF) is a separate, optional
  module, and the CRM doesn't use it.
- **The browser never holds tokens.** The Next.js server is a BFF that keeps
  them in httpOnly cookies and forwards `Authorization: Bearer`.
- **One error shape.** Every error is `{statusCode, message, error, code}`,
  with validation failures as 400 plus `errors: {field: [..]}`; the web app
  branches on `code`, never on the English message. Statuses 422 and
  419 are not used. The PRD text still mentions problem+json, 422 and 419
  in places; spec §10 overrides it.
- **PostgreSQL.** The PostgreSQL-only paths (`FOR UPDATE`, `SKIP LOCKED`,
  advisory locks) are written behind `DbBackend` checks and the `postgres`
  feature, and the performance harness in `bench/` runs on PostgreSQL;
  every test runs on SQLite, the PRD's development and test database.
- **Real time is in-process.** Navree ships a Pusher-protocol WebSocket
  server, so pusher-js and any Pusher client connects. Processes that don't
  serve the sockets, such as queue workers, send their broadcasts to the
  serving process over a signed HTTP relay.
- **Framework crates implement `Module`, `Injectable` and `Controller` by
  hand** (the PRD's escape hatch) instead of using `navree-macros`, so
  every crate could be built before the macros existed. Apps and the
  facade's tests use the macros.

## Layout

| Path | What |
|---|---|
| `crates/navree` | Facade: re-exports every module crate and the macros; holds the full-stack and dependency-graph tests |
| `crates/navree-*` | One crate per module (see [Crate hierarchy](#crate-hierarchy)) |
| `apps/crm-api` | The CRM API, a Navree application (`src/`), its feature crates (`crates/`) and system tests (`tests/`) |
| `apps/crm-web` | The CRM front end (Next.js), with Vitest unit tests and Playwright specs in `e2e/` |
| `apps/website` | navree.com and the documentation |
| `skills/` | The agent skills, one `SKILL.md` per directory |
| `bench/` | The performance harness behind navree.com/performance |
| `docs/` | PRD, build design, implementation plan, notes |

### Features and drivers

- Databases: SQLite (bundled) by default; enable PostgreSQL with the
  `postgres` feature of `navree` (or `navree-db`).
- External versions live only in the root `[workspace.dependencies]`; crates
  use `dep = { workspace = true }`.
