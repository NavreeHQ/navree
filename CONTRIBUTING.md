# Contributing to Navree

This guide is for people working on Navree itself: how the repository is
laid out, how to build and test it, and how to run the reference CRM. To
build an app *with* Navree, start with the
[documentation](https://navree.com/docs/prologue/introduction/).

When you add a crate, give it a row in the [crate hierarchy](#crate-hierarchy)
table below, in `crates/navree/tests/dependency_graph.rs` (HIERARCHY) and on
the docs page `apps/website/content/docs/architecture/crate-hierarchy.mdx`.

## This repository

The repository is a Cargo workspace of small module crates, plus a reference
CRM built on top of them and the website:

- `apps/crm-api`: the Navree CRM API, a Navree application.
- `apps/evidence-app`: the evidence app, one Navree application that wires
  every package and tool, built into a Docker image and run under load by
  the evidence harness in `e2e/` (see [`e2e/README.md`](e2e/README.md)).
- `apps/crm-web`: the CRM front end (Next.js 16). Its server is a
  backend-for-frontend (BFF) that holds the API tokens in httpOnly cookies.
- `apps/crm-nuxt`: the same CRM front end in Nuxt 4, with its BFF in Nitro
  server routes (see [`apps/crm-nuxt/README.md`](apps/crm-nuxt/README.md)).
- `packages/js`: the front-end npm packages (`@navree/client`, `@navree/bff`,
  `@navree/next`, `@navree/nuxt`, `@navree/theme`), a pnpm workspace.
- `apps/website`: navree.com and the documentation (Next.js, MDX).
  [`apps/website/AUTHORING.md`](apps/website/AUTHORING.md) is the guide for
  writing docs pages.

## Crate hierarchy

The crates form layers: the framework from L0 to L5, ecosystem
libraries at L6, packages built on them at L7, and the facade and CLI at L8.
A crate may depend only on crates in lower layers, dev-dependencies included,
and its runtime dependencies must also match the table of allowed edges
in `crates/navree/tests/dependency_graph.rs`. Cross-cutting needs (audit,
events, broadcasting, sessions, health checks, jobs, the current principal)
go through traits in `navree-contracts`, so modules don't depend on each
other sideways. `crates/navree/tests/dependency_graph.rs` checks every edge on
each test run.

```
L9+ apps/crm-api (Navree app)  apps/evidence-app (Navree app, e2e)  apps/crm-web (Next.js 16)
L8  navree (facade: re-exports + prelude)       navree-cli (binary `navree`; no runtime navree deps, uses navree-dock, navree-harbor and navree-navigator)
L7  navree-billing  navree-search  navree-lighthouse  navree-logbook  navree-fleet  navree-helm  navree-workflows  navree-media  navree-directory  navree-domains (packages built on other packages)
L6  navree-social  navree-sso  navree-webhooks  navree-integrations  navree-sheets  navree-oauth  navree-flags  navree-settings  navree-graphql  navree-grpc  navree-mcp  navree-ai  navree-micro  navree-i18n  navree-documents (ecosystem libraries)
L5  navree-auth  navree-workspace  navree-redis  navree-mongo
L4  navree-session  navree-validation  navree-queue  navree-audit  navree-access  navree-schedule  navree-notify  navree-ui  navree-orm  navree-browser
L3  navree-db  navree-testing  navree-observe  navree-api  navree-mail  navree-realtime  navree-storage  navree-time
L2  navree-http  navree-log  navree-crypto  navree-events  navree-view  navree-cache  navree-http-client  navree-schema  navree-navigator (dev tool)
L1  navree-contracts  navree-config  navree-telemetry
L0  navree-kernel                                navree-macros  navree-orm-macros  navree-i18n-macros (proc-macros; no runtime navree deps)   navree-dock, navree-harbor (tools; no navree deps)
```

| Layer | Crate | What it provides |
|---|---|---|
| L0 | `navree-kernel` | Module graph, type-keyed DI container, lifecycle, command runner |
| L0 | `navree-macros` | `#[module]`, `#[injectable]`, `#[controller]`, `#[resolver]`, route and access attributes |
| L0 | `navree-orm-macros` | `#[derive(Model)]` and `#[scopes]` (re-exported by `navree-orm`) |
| L0 | `navree-i18n-macros` | `embed_locales!`: Fluent files embedded in the binary, their syntax checked at compile time (used through navree-i18n) |
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
| L2 | `navree-cache` | Cache stores (memory on navree-bounded; Redis with navree-redis), remember, tags, locks |
| L2 | `navree-http-client` | Outgoing HTTP with retries, timeouts, an SSRF policy and fakes |
| L2 | `navree-schema` | The schema builder: tables, columns, indexes, foreign keys and check constraints in Rust for PostgreSQL, MySQL/MariaDB and SQLite (SQLite alterations by table rebuild); TimescaleDB hypertables, compression, retention, reorder policies and continuous aggregates with fallbacks on other databases; introspection; schema dumps; migration files (`navree::schema`) |
| L2 | `navree-navigator` | Navigator, the MCP server for coding agents behind `navree mcp`: routes, the module graph and providers, the schema, read-only SQL, logs and the last error, redacted configuration and the docs of the app's Navree version; and the agent skills of `navree skills:install` (a dev tool; opt-in facade feature `navigator`) |
| L3 | `navree-db` | SeaORM integration, per-request unit of work, migrations (schema and SeaORM, batches, steps, rollback, reset, refresh, fresh, `--pretend`, `schema:dump`), seeders, factories, pagination |
| L3 | `navree-testing` | `TestApp`, an in-process `TestClient`, fakes, cross-tenant isolation suites generated from a module's routes |
| L3 | `navree-observe` | Health checks, readiness, Prometheus metrics, tracing (spans from telemetry events, `SpanRecorder`, OTLP export behind the opt-in facade feature `otel`) |
| L3 | `navree-api` | OpenAPI 3.1 from route metadata, docs UI, API resources, `api.require_docs` and the `api.docs` check |
| L3 | `navree-mail` | Mailables from templates with CSS inlined; SMTP (DKIM-signed), SES, Mailgun, Postmark, Resend, log, array and failover transports; queued sending; tenant transports, a delivery log fed by provider webhooks, suppressions, hold for approval, open and click tracking, SPF/DKIM/DMARC checks; inbound mail (feature `inbound`, facade `mail-inbound`, in `full`): messages parsed with mail-parser, SES (SNS and S3), Mailgun, Postmark and raw MIME sources, mailboxes routed by address (`m.mailbox("reply+{token}@...", handler)`), signed reply tokens, raw `.eml` files on a storage disk and `InboundMailReceived` |
| L3 | `navree-realtime` | Pusher-protocol WebSocket server (public, private, presence channels), a signed relay for other processes, and a backplane (Redis with navree-redis) for several serving processes |
| L3 | `navree-storage` | Named disks on object_store (local, memory, S3 and S3-compatible stores, Google Cloud Storage, Azure; facade features `storage-s3`, in `full`, and opt-in `storage-gcs`, `storage-azure`), streaming, provider-signed temporary URLs, direct browser uploads, upload checks, `storage:sync` |
| L3 | `navree-time` | Time zones: checked IANA zones with PostgreSQL-safe names, the request's zone (user, workspace, organization, default), local days and report windows, send windows, public holidays, `AT TIME ZONE` with a checked zone, `time:zones` |
| L4 | `navree-session` | Encrypted session cookies, memory, database and Redis stores, CSRF (optional; the CRM doesn't use it) |
| L4 | `navree-validation` | `Valid<T>` extractors on garde, field-level messages, async unique/exists rules, international input (E.164 `Phone`, `SafeHtml`, names and digits in any script, `Country`, `Locale`) |
| L4 | `navree-queue` | Database, memory and Redis job queues (the `QueueStore` driver trait), workers, retries with backoff, unique and delayed jobs; job batches with callbacks; operations with progress, an SSE stream, cancel and a watchdog |
| L4 | `navree-audit` | Audit trail of data changes, sign-ins, permission changes and denials |
| L4 | `navree-access` | Roles, permissions and abilities (shared with front ends as CASL JSON), record checks, SQL list filters |
| L4 | `navree-schedule` | Cron schedules in UTC, a zone or each workspace's local time, overlap prevention, single-node runs |
| L4 | `navree-notify` | Notifications over mail, database, broadcast, Slack (webhooks, bot tokens), Microsoft Teams (webhooks, proactive bot messages), SMS (Twilio, Vonage, Amazon SNS; E.164 numbers, STOP opt-outs), WhatsApp (Twilio, the Cloud API) and web push (VAPID ES256 on `ring`, RFC 8291 encryption), with read state, a delivery log updated from provider callbacks, push subscriptions and a fake per channel; preferences per category and channel (people and workspaces), digests, quiet hours and send windows on the recipient's clock, held deliveries released by `notify:release`, per-workspace template overrides and `#[notification]` |
| L4 | `navree-ui` | The UI kit of the server-rendered tools: the app shell (collapsible sidebar, top bar, command palette), components as MiniJinja macros, embedded and versioned CSS, JS and fonts (facade feature `ui`) |
| L4 | `navree-orm` | The ORM: `#[derive(Model)]` models with typed columns and keys (integers, UUID v7, ULID), a query builder with every where variant, joins, groups, subqueries, unions, locks and TimescaleDB's `time_bucket`/`first`/`last`, records with dirty tracking, casts, scopes, lifecycle hooks, collections and offset, simple and cursor pagination, on PostgreSQL/TimescaleDB, MySQL and SQLite through the unit of work, and a document-driver interface (facade feature `orm`) |
| L4 | `navree-browser` | Browser testing: Chrome, Chromium or Edge over the DevTools Protocol, actions and assertions that wait, failure screenshots, `TestServer` on a real port and `FrontEnd` sessions (a dev-dependency; opt-in facade feature `browser`; `navree browser:install` downloads Chrome for Testing) |
| L5 | `navree-auth` | JWT auth: rotating refresh tokens, strategies, accounts, 2FA and two-factor policy, sessions, step-up, impersonation, magic links and email codes, platform sign-in, token exchange, passkeys |
| L5 | `navree-workspace` | Workspaces: members, roles, invitations, time zones and countries, row, schema or database isolation; organizations above workspaces and partitions inside them; shared libraries of global, organization and workspace content cloned on use; entity classification and isolation checks, workspace isolation suites (feature `testing`); demo workspaces seeded from fixtures, read-only or reset nightly |
| L5 | `navree-redis` | Redis: shared connections (lazy, reconnecting, TLS), Redis drivers for the cache and locks, the queue, sessions and the real-time backplane, shared rate limits (facade feature `redis`, opt-in) |
| L5 | `navree-mongo` | MongoDB on the official driver: `MongoModule` (connections from `[database.mongodb]`, pools, readiness), the ORM's document driver (filters, aggregation pipelines, `$lookup` joins and subqueries, time buckets, ObjectId keys, bulk writes), sessions and units of work with change events, the schema builder on collections (JSON-schema validators, indexes, time-series collections) and migrations (facade feature `mongo` or `mongodb`, opt-in) |
| L6 | `navree-social` | Social sign-in: Google, GitHub, Apple, Microsoft, GitLab, Slack, LinkedIn, Discord, X and OpenID Connect, linked to Navree users (facade feature `social`, opt-in) |
| L6 | `navree-sso` | Single sign-on: SAML 2.0 and OpenID Connect connections per organization or workspace, verified email domains that route sign-in and can refuse passwords, just-in-time provisioning with group to role mapping; XML signatures verified on `ring` |
| L6 | `navree-webhooks` | Webhooks: signed outgoing deliveries with queue-backed retries and a delivery log, verified incoming webhooks (Stripe, GitHub, Paddle, Standard Webhooks, AWS SNS, Google Pub/Sub, Microsoft Graph, Twilio, Slack, Mailgun, Postmark, Vonage, Meta) with their handshakes answered, and the notification channels' provider callbacks (feature `notify`) |
| L6 | `navree-integrations` | Integrations: workspace and organization connections to Microsoft 365 (admin consent, app-only tokens), Google Workspace (service-account keys with domain-wide delegation, RS256 signed on `ring`), Slack, any OAuth 2.0 API and API keys; credentials encrypted in `integration_connections` (a navree-orm model), token refresh under a lock, least-privilege scope sets, the scheduled `integrations:check` with `ConnectionUnhealthy`, typed clients (`Connection<MicrosoftGraph>`, `graph.get("/users")`), `IntegrationsFake`, connection tokens for notification bot routes (feature `notify`), and Microsoft 365 and Gmail mailboxes watched for new mail (feature `mail`: Graph subscriptions, Gmail push, `integrations:mailboxes`) (facade feature `integrations`, in `full`) |
| L6 | `navree-sheets` | CSV and Excel: `#[derive(Exportable)]` rows, downloads with a byte-order mark and formula escaping, XLSX with typed cells, named exports streamed or queued to a disk with a signed link; imports with encoding detection (UTF-16, Windows-1256 and the rest), CSV, XLSX, XLS and ODS reading, automatic and saved header mappings, garde validation per row, a staging preview, duplicate modes, batched apply with progress and error files |
| L6 | `navree-oauth` | OAuth 2.1 and OpenID Connect provider: PKCE, client credentials, device and refresh grants, scopes mapped to permissions, personal access tokens (facade feature `oauth`, opt-in) |
| L6 | `navree-flags` | Feature flags: `#[flag]` types, checks per user, workspace, plan or percentage with stable hashing, a kill switch, `#[requires_flag]` routes, database storage with a cache (facade feature `flags`, opt-in) |
| L6 | `navree-settings` | Typed settings: `#[derive(Settings)]` types stored per global, organization, workspace or user level and inherited field by field, with the source of every value; validated, audited and cached writes, `GET`/`PATCH /api/settings/{key}`, the `Setting<T>` extractor, `settings:*` commands, a Helm screen and `SettingsFake`; with `notify`, the `notification-templates` group restyles notification mail per workspace (facade feature `settings`, in `full`) |
| L6 | `navree-graphql` | GraphQL on async-graphql: `#[resolver]` fields in modules with route-style access, per-request dataloaders, subscriptions over WebSockets, GraphiQL, `graphql:schema` (facade feature `graphql`, opt-in) |
| L6 | `navree-grpc` | gRPC on tonic: `#[grpc_service]` services in modules with DI, guards, access rules and the unit of work; gRPC-Web, Connect, health, reflection, named client channels (facade feature `grpc`, opt-in) |
| L6 | `navree-mcp` | MCP servers: `#[mcp_server]` tools, resources and prompts over streamable HTTP at `/mcp` and stdio (`mcp:serve`), behind the app's authenticators and per-tool access checks (opt-in facade feature `mcp`) |
| L6 | `navree-ai` | AI SDK: text, typed structured output and SSE streams from Anthropic, OpenAI, Gemini, Mistral, Ollama and Bedrock, failover, token budgets, agents with `#[tool]` methods run as the user, conversation memory, embeddings, vector stores (memory, pgvector, Qdrant) and `AiFake` (facade feature `ai`, opt-in) |
| L6 | `navree-micro` | Microservices: `#[message_pattern]` and `#[event_pattern]` handlers with the same guards, access, validation and unit of work as routes, `ServiceClient` (`send` with deadlines, `emit`), retries, dead letters, an outbox and an inbox, `MicroFake`; TCP and in-memory transports, NATS, Kafka, RabbitMQ, Redis Streams and MQTT behind features (facade features `micro`, `micro-nats`, ..., opt-in) |
| L6 | `navree-i18n` | Localization: Fluent files per locale with plurals, variables and fallbacks, `t!`, the request's `Locale` negotiated from the query, the user's and workspace's saved locale, a cookie and `Accept-Language`, ICU4X number, money and date formatting, translated validation messages (seven languages shipped), `t()` in templates, notifications in the recipient's locale, `i18n:check` (facade feature `i18n`, opt-in) |
| L6 | `navree-documents` | Documents: PDF from HTML templates or documents built in code, laid out by a native engine (shaping, bidirectional text, tagged PDF, bundled Noto fonts with Arabic, Hebrew and Devanagari) or printed by headless Chrome with a page pool; DOCX; merge, stamps and overlays; text extraction in reading order; queued renders to a storage disk; a fake (facade feature `documents`, opt-in) |
| L7 | `navree-billing` | Billing for users or workspaces: Stripe and Paddle clients, plans, trials, seats, metered usage, invoices, the portal, verified webhooks on the queue, a fake (facade feature `billing`, opt-in) |
| L7 | `navree-search` | Full-text and vector search: `#[derive(Searchable)]`, PostgreSQL/SQLite, Meilisearch, Typesense, Tantivy and memory engines, synced through the queue (facade feature `search`, opt-in) |
| L7 | `navree-lighthouse` | Application monitoring inside the app: a telemetry sink grouping exceptions, slow requests and queries, failed jobs and failing outgoing calls into issues, per-minute aggregates and sampled traces in the app's database, alerts (mail, Slack, signed webhooks, your own channels) through the queue, the `/_lighthouse` dashboard and its JSON API, MCP tools for agents (with `mcp`), `lighthouse:prune` (facade feature `lighthouse`, in `full`) |
| L7 | `navree-logbook` | Logbook: a local debugging timeline at `/_logbook` of every request with its queries, events, jobs, mail, cache calls and log lines; filters by tag and user, duplicate-query detection, request replay, a bounded SQLite or memory store (facade feature `logbook`, in `full`; off in release builds) |
| L7 | `navree-fleet` | Fleet: a supervisor of queue worker processes (`fleet`) with auto-balancing, recycling and graceful restarts; throughput, wait and runtime metrics; failed jobs; pause and continue; long-wait alerts; a dashboard at `/_fleet` (facade feature `fleet`, in `full`) |
| L7 | `navree-helm` | Helm: the admin panel at `/admin` generated from your entities (resources with fields, filters, search, actions and metrics), checked against your access rules, audited, server-rendered with an embedded stylesheet and script, plus its JSON API |
| L7 | `navree-workflows` | Durable workflows in code: `#[workflow]` async functions whose steps are stored and replayed on resume, durable sleeps for minutes or months, event waits with timeouts, versions, cancellation, retries and leases that survive crashes (`workflow_runs`, `workflow_steps`); end-user automations: `#[trigger]`, `#[condition]` and `#[action]` blocks with JSON Schemas for the editor, versioned definitions per workspace validated against the catalog, enrollments with policies, simulation, templates, the `/api/workflows` and `/api/automations` routes and `WorkflowsFake` (facade feature `workflows`, in `full`) |
| L7 | `navree-media` | Media: files attached to models (`impl HasMedia`) in collections that check types and sizes and keep one or the latest few files, rows in `media` (a navree-orm model), bytes on storage disks; image conversions (resize, crop, WebP, JPEG, PNG, greyscale, blur, responsive variants with `srcset`, SVG through resvg) with EXIF stripped, queued as `media.conversions` and regenerated with `media:regenerate`; remote files through the SSRF guard; zip archives with zip-slip and size-bomb guards; text from PDF, DOCX and HTML; video and audio through an FFmpeg sidecar (`media-ffmpeg`); `MediaFake` (facade feature `media`, opt-in) |
| L7 | `navree-directory` | Directory sync: a SCIM 2.0 server (`/scim/v2/Users`, `/Groups`, `/ServiceProviderConfig`, `/ResourceTypes`, `/Schemas`; the full filter language, PATCH as identity providers send it, paging, projection; a bearer token per connection kept as a SHA-256 hash) and pull connectors for Microsoft Graph (delta queries), Google Workspace and Okta (through navree-integrations connections, scope set `directory`) and LDAP or Active Directory (ldap3 with rustls, paged searches); a normalized `DirectoryUser` and `DirectoryGroup` per connection (navree-orm models), mapped into your model with `impl DirectorySink`; runs that stage, diff and apply in chunks, with dry runs, a deactivation limit that holds a run for approval, progress on navree-queue operations and recorded changes; events `DirectoryUserCreated`, `Updated`, `Deactivated`, `DirectorySyncFinished`; commands `directory:sync` and `directory:runs`; `DirectoryFake` (facade feature `directory`, in `full`) |
| L7 | `navree-domains` | Custom domains: workspaces on their own domains (`Domains::add` answers with the TXT and CNAME records to create; reserved, verified-elsewhere and per-workspace limits; claims are proved by DNS, so the first TXT proof wins atomically and a verified domain moves only through `domains:transfer`), verified through DNS (hickory-resolver; `domains:verify --due` every minute, giving up after 72 hours, daily routing rechecks); automatic TLS through an on-demand proxy's ask endpoint (`GET /_domains/allow?domain=`, which Harbor's Caddy calls) or ACME certificates the app gets itself (instant-acme on ring, HTTP-01 answers served from the database, keys sealed, renewals, a rustls certificate resolver and an HTTPS listener); the `domain` workspace resolver and the `CurrentDomain` extractor; managed Cloudflare zones; certificate expiry probes and reports (an event, and a Lighthouse issue with the `lighthouse` feature); routes under `/api/domains`; `DnsFake`, `AcmeFake`, `ProbeFake`, `DnsProviderFake` (facade feature `domains`, in `full`) |
| L8 | `navree` | The facade: re-exports every module crate (one Cargo feature each, `full` by default) and the macros |
| L8 | `navree-cli` | The `navree` binary: `navree new` (with the starter kits: `--kit saas`, `api` or `micro`), `navree make:*`, `navree dev`, `navree dock`, `navree check`, `navree gate`, `navree harbor:*`, `navree deploy`, `navree build:image`, `navree mcp`, `navree skills:install`, `navree browser:install`, `navree openapi:client` |

The front-end packages are npm packages in `packages/js/` (a pnpm workspace):

| Package | What it does |
|---|---|
| `@navree/client` | A typed client on openapi-fetch and the schema `navree openapi:client` generates: cookie or bearer mode, `X-Workspace`, request ids, `NavreeError` with the error `code`, list queries, page helpers, query keys, operations over SSE, locale helpers; abilities (`@casl/ability`) and real time (`pusher-js`) as separate entry points |
| `@navree/bff` | The backend-for-frontend on web-standard `Request` and `Response`: httpOnly token cookies, single-flight refresh, two-factor sign-in, the Origin check, a streaming proxy, `ensureSession` for server rendering, test helpers (`fakeNavree`, `serveFakeNavree`) |
| `@navree/next` | Next.js: route handlers, `auth()` and `client()` for server components, the proxy, `DataTable` and `useListQuery`, `useOperation`, workspaces, form errors; `AbilityProvider`/`Can`, `RealtimeProvider`/`useChannel` |
| `@navree/nuxt` | A Nuxt module: the BFF as Nitro routes, a session gate, auto-imported composables (`useNavreeAuth`, `useWorkspace`, `useNavreeClient`, `useListQuery`, `useOperation`, `useChannel`, `useAbility`) and `<NavreeDataTable>` |
| `@navree/theme` | navree.com's dark theme: Tailwind CSS 4 tokens, shadcn/ui and shadcn-vue colors, self-hosted fonts, the tokens as data, a shadcn registry theme |

The CRM API is split the same way: `crm-core` (models, factories,
migrations, abilities, platform modules), `crm-companies` (companies and contacts),
`crm-deals`, `crm-activities` (activities and the dashboard), `crm-users`,
`crm-testkit` (a booted CRM for feature tests) and the `crm-api` binary.

## Requirements

- Rust 1.95 or newer (edition 2024; the build machine uses 1.99.1). On
  Windows, the MSVC build tools (SQLite is compiled from source through the
  bundled `libsqlite3-sys`).
- Optional: [sccache](https://github.com/mozilla/sccache), to share compiled
  dependencies between target directories. The repository doesn't set a
  compiler wrapper; set `RUSTC_WRAPPER=sccache`, or `rustc-wrapper =
  "sccache"` under `[build]` in a Cargo config outside the repository (for
  example `~/.cargo/config.toml`).
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
locks the target directory, so give each one its own (and, if you use it, let
sccache share the compiled dependencies):

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

### Front-end packages (packages/js)

```sh
cd packages/js
pnpm install
pnpm check       # build, typecheck and Vitest for every package
pnpm smoke       # builds a Next.js and a Nuxt app on the packages and checks the running servers
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

## Dependencies and security

Every new or updated Rust dependency goes through three gates in CI
(`.github/workflows/supply-chain.yml` and `vulnerabilities.yml`), which you
can run locally first:

```sh
cargo install --locked cargo-deny cargo-vet   # once
cargo deny check                              # advisories, licenses, bans, sources (deny.toml)
cargo vet                                     # every crate audited or exempted (supply-chain/)
```

- `deny.toml` allows crates.io (plus the one pinned `docx-rs` git
  dependency), a fixed list of permissive licenses, and bans `openssl`,
  `openssl-sys`, `native-tls` and `rsa`; use rustls and ring. A new duplicate
  version of a crate warns; remove it or name it under `skip` with a reason.
  A vulnerability that has no fixed release gets an entry with a reason and
  an expiry date in both `osv-scanner.toml` and `deny.toml`.
- A new crate version needs a review or a deliberate exemption in
  `supply-chain/`; [`supply-chain/README.md`](supply-chain/README.md) says how.
- GitHub Actions are pinned to a full commit SHA with the version in a
  comment (`uses: owner/action@<sha> # v1.2.3`), workflows declare the least
  `permissions:` they need, and checkouts that don't push set
  `persist-credentials: false`. Dependabot proposes SHA bumps.
- Changes to `.github/`, `deny.toml`, `supply-chain/` and the security-critical
  crates (auth, crypto, sso, oauth, session, access, orm, schema, db, http)
  need a review from the code owners (`.github/CODEOWNERS`).
- Tagged releases get a CycloneDX SBOM from `.github/workflows/sbom.yml`.
- Report a vulnerability to security@navree.com, never in a public issue; see
  [`SECURITY.md`](SECURITY.md).

### Fuzzing

Every parser that reads untrusted bytes has a libFuzzer target in `fuzz/` (its
own workspace: it needs a nightly compiler). A target checks that its parser
does not panic, hang (`-timeout`) or use unbounded memory (`-rss_limit_mb`),
and, where it can, an invariant: print and parse give the same value, a signed
token's claims read back, a rendered query holds only the user's words.

| Target | What it feeds |
|---|---|
| `jose_token`, `jose_keys` | compact JWS verification against fixed keys and key sets; PEM, DER, certificates and JWKs |
| `auth_jwt` | the bearer-token check of `JwtService`, against an oracle for each claim |
| `mcp_protocol`, `mcp_server` | JSON-RPC parsing and stdio framing; a booted MCP server over HTTP and stdio |
| `http_client_ip`, `http_cors` | `X-Forwarded-For`, `X-Real-IP`, trusted proxies, the rate-limit key; CORS origins; signed URLs |
| `host_names` | email domains, custom-domain normalization, the `Host` header |
| `sso_saml_response`, `sso_saml_metadata`, `sso_xml` | SAML responses, identity provider metadata, XML and Canonical XML |
| `webhooks_signatures` | the signature headers and bodies of every incoming webhook verifier |
| `validation_international` | E.164 phone numbers, names, countries, locales, sanitized HTML |
| `search_query` | query text and filter values rendered for each search engine |
| `schedule_cron` | cron expressions: `parse`, `next_after`, `is_due` |
| `config_sources` | the `.env` parser, TOML and JSON files, environment merging |
| `qr_roundtrip`, `totp_uri` | QR encode then decode with `rqrr`; `otpauth://` URIs and base32 secrets |
| `scim_filter`, `mail_inbound`, `micro_envelope`, `time_parse` | SCIM filters and paths; raw MIME messages; broker envelopes; zone names, days and send windows |
| `small_parsers` | the logbook search box, OAuth `scope`, `X-Request-ID`, the app key, Web Push keys |

Run them in Docker, so the nightly toolchain and cargo-fuzz are the pinned
ones (`fuzz/Dockerfile`) and nothing is built inside the checkout. The script
builds the image, mounts the repository and keeps build output, the cargo
registry and the working corpora in named volumes:

```sh
fuzz/docker.sh list                                  # the targets
fuzz/docker.sh build                                 # build every target
fuzz/docker.sh run jose_token -- -max_total_time=300 -timeout=10 -rss_limit_mb=2048 -max_len=8192
```

Without the script, the same thing by hand (one line):

```sh
docker build -t navree-fuzz fuzz && docker run --rm -v "$PWD:/work" -v navree-fuzz-target:/fuzz-target -v navree-fuzz-registry:/usr/local/cargo/registry -v navree-fuzz-corpus:/corpus navree-fuzz cargo fuzz run jose_token /corpus/jose_token /work/fuzz/seeds/jose_token -- -max_total_time=300
```

- Seeds (test fixtures and documentation examples) are in `fuzz/seeds/<target>/`
  and always read; what a run finds goes to the corpus volume.
- A crash is written to `fuzz/artifacts/<target>/crash-*`. Reproduce it with
  `fuzz/docker.sh run <target> artifacts/<target>/crash-...`, minimize it
  with `fuzz/docker.sh tmin <target> artifacts/<target>/crash-...`, then fix the
  parser and add a regression test to **the crate's own tests** (not only to `fuzz/`).
- A new parser of untrusted input gets a target: add
  `fuzz/fuzz_targets/<name>.rs`, a `[[bin]]` entry in `fuzz/Cargo.toml`, seeds,
  a line in the table above and the name in the matrix of
  `.github/workflows/fuzz.yml`. Internals a target needs are exposed through a
  hidden `fuzzing` feature of the crate (`navree_mcp::fuzzing`, for example),
  never through the public API.
- CI (`.github/workflows/fuzz.yml`) runs every target for 60 seconds on pushes
  to main that touch a fuzzed crate and for 10 minutes every night, caching the
  corpora between runs and uploading the inputs that crashed.

## Design decisions

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
  419 are not used.
- **PostgreSQL.** The PostgreSQL-only paths (`FOR UPDATE`, `SKIP LOCKED`,
  advisory locks) are written behind `DbBackend` checks and the `postgres`
  feature, and the performance harness in `bench/` runs on PostgreSQL;
  the default test suite runs on SQLite, and the database tests run on
  real PostgreSQL/TimescaleDB, MySQL and MongoDB servers when
  `target/test-databases.env` is sourced (`scripts/dev-databases.ps1 start`).
- **Real time is in-process.** Navree ships a Pusher-protocol WebSocket
  server, so pusher-js and any Pusher client connects. Processes that don't
  serve the sockets, such as queue workers, send their broadcasts to the
  serving process over a signed HTTP relay.
- **Framework crates implement `Module`, `Injectable` and `Controller` by
  hand** instead of using `navree-macros`, so
  every crate could be built before the macros existed. Apps and the
  facade's tests use the macros.

## Layout

| Path | What |
|---|---|
| `crates/navree` | Facade: re-exports every module crate and the macros; holds the full-stack and dependency-graph tests |
| `crates/navree-*` | One crate per module (see [Crate hierarchy](#crate-hierarchy)) |
| `apps/crm-api` | The CRM API, a Navree application (`src/`), its feature crates (`crates/`) and system tests (`tests/`) |
| `apps/crm-web` | The CRM front end (Next.js), with Vitest unit tests and Playwright specs in `e2e/` |
| `apps/evidence-app` | The evidence app: a Navree application wiring every package and tool, with domain models and seeders, for the Docker evidence harness |
| `e2e/` | The evidence harness: the compose stack (`docker/`), the evidence app's Dockerfile, k6 load profiles and scenarios (`load/`), HTTP check rows (`checks/`), browser screenshots (`browser/`) and `run.sh`, which writes `target/evidence/<run-id>/` |
| `apps/website` | navree.com and the documentation |
| `packages/js` | The front-end npm packages: `@navree/client`, `@navree/bff`, `@navree/next`, `@navree/nuxt`, `@navree/theme` |
| `skills/` | The agent skills, one `SKILL.md` per directory |
| `bench/` | The performance harness behind navree.com/performance |
| `docs/` | Design notes |

### Features and drivers

- Databases: SQLite (bundled) by default; enable PostgreSQL with the
  `postgres` feature of `navree` (or `navree-db`).
- External versions live only in the root `[workspace.dependencies]`; crates
  use `dep = { workspace = true }`.
