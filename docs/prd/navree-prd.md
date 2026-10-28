# PRD — Laravel-Class Application Framework on Rust (Tokio)

Oct 25, 2026 · @Amritpal Singh

## Executive summary

Yes: the full Laravel stack can be built on Rust with Tokio as the base, organised NestJS-style so that everything above a small kernel is a module. Most of it is assembling maintained crates behind Navree's own APIs; a handful of features have no mature Rust equivalent and must be built.

- **Base stack: ready.** Axum 0.8, Hyper 1.12, tower-http 0.7, SeaORM 2.0, garde, argon2, totp-rs and webauthn-rs cover routing through passkeys. Navree must build session stores, login guards and API tokens; an OAuth2 server (Passport) is deferred. Roles, permissions and CASL-style abilities, default logging and an audit trail are built in.
- **Advanced stack: buildable, but young.** Queues, scheduler, cache, mail, storage, real-time, search, OpenAPI, gRPC, observability and AI all have crates, several still pre-1.0 or only just 1.0 (apalis 1.0.0 shipped on 12 Oct 2026; opendal and OpenTelemetry are 0.x). Navree hides each behind a stable module API so drivers can be swapped.
- **Everything else: real gaps.** Subscription billing, database-backed feature flags, an admin panel, Telescope/Pulse-style inspection, SMS and push drivers, social-login presets and multi-tenancy must be built by Navree; browser testing and HTML-to-PDF are only partly covered.
- **UI stack: Next.js or Nuxt on Navree's API.** Navree is API-first: its built-in API drives the whole system, and the UI is a standard Next.js (React) or Nuxt (Vue) app. Navree speaks Laravel Sanctum's SPA auth and the Pusher protocol, so existing front-end packages such as Laravel Echo work unchanged.
- **Plan:** v1.0 in about 12 months with five engineers, targeting 66 of 70 Laravel features (94%), with 90% as the release floor. The largest risks are dependency churn and compile times.

### Headline decisions (proposed; the open questions confirm them)

| Decision | Choice | Why |
| --- | --- | --- |
| Runtime | Tokio 1.x (1.51 LTS tested in CI) | Base of nearly every production Rust web stack |
| HTTP layer | Axum 0.8 on Hyper and Tower | Same maintainers as Tokio; one middleware model |
| Modularity | Navree's own NestJS-style module system; everything is a module | No mature Rust crate provides it |
| ORM | SeaORM 2.0 | Closest to Eloquent: entities, relations, migrations |
| Queue | apalis behind `navree-queue`, native Postgres fallback | Multi-backend plus a Horizon-style board; 1.0.0 stable since 12 Oct 2026 |
| UI | Next.js (React) or Nuxt (Vue) in their default setups, on Navree's API | Standard front-end tools; one API serves web, mobile and partners |
| Templates | minijinja | Email templates only; Navree renders no web pages, and PDFs come from Typst or headless Chrome |
| Real-time | Pusher protocol; Sockudo first, in-process server later | Laravel Echo and pusher-js work unchanged |
| Primary database | PostgreSQL | Gets every feature first (vector search, row-level security) |
| Authorization | Built-in roles, permissions and CASL-compatible abilities; deny by default | One rule set enforced by the API and checked in Next.js or Nuxt with the official CASL packages |
| Logging | On by default: request, error, query, job and audit logs with redaction | A production-grade baseline with nothing to configure |
| Annotations | Rust attributes for modules, routes, validation, access, jobs, logging | NestJS-style declarations, expanded at compile time with no runtime reflection |

## Goals, non-goals and success metrics

The framework must give a Rust team Laravel-level productivity, NestJS-level structure and Rust-level performance in one install. Working name in this document: **Navree** (a placeholder; crate-name and trademark availability not yet checked).

### Goals

1. **Laravel parity for everyday apps.** Cover the features a typical SaaS or business app uses: routing, validation, ORM, auth, queues, scheduler, cache, mail, notifications, storage, real-time, search, testing, CLI generators.
2. **NestJS-style modularity.** Every capability is a module with declared imports, providers, controllers and exports. Apps are compositions of modules. A module can ship as its own crate.
3. **Adopt, don't rewrite.** Build on maintained crates from the Tokio ecosystem. Navree writes the glue, conventions and developer experience, not a new HTTP stack or ORM.
4. **One binary, low cost.** An app compiles to a single binary with predictable memory and latency. Sidecars (an external WebSocket server, the Next.js or Nuxt server, headless Chrome for PDFs) are opt-in.
5. **API-first.** The built-in API drives the whole system. The UI is a Next.js or Nuxt app in its default setup, and mobile apps and partners use the same API.

### Non-goals (v1)

- Reimplementing Tokio, Hyper, Tower or Axum.
- Runtime reflection or decorator metadata in the NestJS sense (Rust has none; Navree uses compile-time macros and boot-time checks instead).
- Supporting async runtimes other than Tokio.
- Running PHP code or reading Laravel projects.
- Server-rendered web UIs of any kind: Blade-style pages, HTMX, Inertia, Livewire, or Rust UI frameworks such as Leptos and Dioxus.
- A hosted platform like Laravel Cloud or Forge.

### Success metrics (proposed targets, to be validated by Phase 0 benchmarks)

| Metric | Target | How measured |
| --- | --- | --- |
| Laravel parity coverage | 66 of 70 parity-matrix rows (94%) Done by v1.0; the release floor is 63 (90%) | Parity matrix in this doc |
| Time from `navree new` to running app with login | Under 5 minutes | Scripted onboarding test |
| Incremental rebuild after a one-file change | Under 5 s on a 16-core dev machine | CI benchmark on the starter app |
| Idle memory of starter app | Under 50 MB RSS | Container metrics |
| p99 latency, DB-backed JSON endpoint | Under 20 ms at 1,000 requests/s on a 4 vCPU instance | Load test (k6 or oha) |
| Known vulnerable dependencies at release | Zero | `cargo audit` / `cargo deny` in CI |

## Users and use cases

Navree serves five kinds of people; the product engineer building features every day is the primary user, and every default is chosen for them.

| Persona | What they need from Navree | Primary surface |
| --- | --- | --- |
| Product engineer (primary) | Generators, conventions, one obvious way to do auth, jobs, mail, tests | `navree` CLI, modules, docs |
| Frontend engineer | Next.js or Nuxt in their default setup; a typed API client generated from OpenAPI | UI starter kits, OpenAPI output |
| Platform engineer | Stable core, clear module boundaries, upgrade path | Core crates, module API |
| SRE / DevOps | Single binary, health checks, metrics, traces, graceful shutdown | Ops module, container templates |
| Module author | A documented way to publish a reusable module crate | Module SDK, `navree make:module` |

### Primary use cases

1. SaaS web app: sign-up, login, teams, billing, email, admin screens.
2. JSON API for mobile and SPA clients, with OpenAPI docs.
3. Real-time product: dashboards, chat, notifications over WebSockets or SSE.
4. Job-heavy backend: imports, exports, reports, scheduled batches.
5. Service in a microservice estate: gRPC, message brokers, tracing across services.
6. Mobile apps and partner integrations that use the same API as the web app.

## Foundation: Tokio and its ecosystem

Tokio is the right base: it is the async runtime under nearly every production Rust web stack, and the same project maintains the HTTP, middleware and web layers Navree needs. Tokio itself is only the runtime; Navree sits four layers above it.

### What Tokio provides ([tokio-rs/tokio](https://github.com/tokio-rs/tokio))

- A multithreaded, work-stealing task scheduler.
- A reactor on the operating system's event queue (epoll, kqueue, IOCP).
- Async TCP and UDP sockets, timers, sync primitives, and companion crates `tokio-util`, `tokio-stream`, `tokio-test`.

### Release, LTS and MSRV policy (as stated in the Tokio README)

| Item | Current value | Navree policy |
| --- | --- | --- |
| Latest release | 1.54.1 (the README example still shows 1.53.1) | Track latest 1.x minor in development |
| Release cadence | About one minor release per month; patch releases as needed | Renovate/Dependabot weekly |
| Active LTS lines | 1.51.x (until March 2027); 1.47.x ended September 2026 | Production builds stay on the 1.51.x LTS line, pinned through Cargo.lock; a tilde version requirement fails if any dependency needs a newer minor. CI tests both latest and LTS |
| MSRV | Rust 1.71, rolling policy of at least 6 months | Navree's own MSRV will be higher (Rust 2024 edition); see Compatibility |

### The layer cake Navree builds on

| Layer | Crate | Maintainer | Role in Navree |
| --- | --- | --- | --- |
| Runtime | [tokio](https://github.com/tokio-rs/tokio) | tokio-rs | Tasks, I/O, timers, signals |
| HTTP | [hyper](https://github.com/hyperium/hyper) | hyperium | HTTP/1.1 and HTTP/2 server and client |
| Middleware | [tower](https://github.com/tower-rs/tower) / tower-http | tower-rs | Service and Layer traits; all Navree middleware is a Tower layer |
| Web | [axum](https://github.com/tokio-rs/axum) | tokio-rs | Routing, extractors, responses |
| gRPC | [tonic](https://github.com/hyperium/tonic) | hyperium | gRPC server and client |
| Diagnostics | [tracing](https://github.com/tokio-rs/tracing) | tokio-rs | Structured logs and spans |
| Framework | Navree | Us | Modules, DI, conventions, CLI, batteries |

All of these are listed as related projects in the Tokio README. Choosing Axum as the HTTP layer keeps Navree inside one maintainer family and one middleware model (Tower), which is the main reason it is preferred over Actix Web (active, 4.18.1, but with its own middleware model), Rocket (last release 0.5.1, May 2024) and Poem (no release since July 2025).

## Architecture overview

Navree is a Cargo workspace of small crates: a small kernel (module graph, DI container, lifecycle) and one crate per module, including config, HTTP, logging and every capability. Apps depend on the `navree` facade crate and switch modules on with Cargo features.

&#91;embedded content: Navree architecture · 6 layers\]

Every box above the kernel is a module the app imports or leaves out; everything below the kernel is adopted from the Tokio ecosystem rather than written by us.

### Workspace layout

| Crate | Laravel equivalent | Responsibility |
| --- | --- | --- |
| `navree` | `laravel/framework` | Facade: re-exports, feature flags, prelude |
| `navree-kernel` | Container, service providers | App builder, `Module` trait, module graph, DI container, lifecycle, boot checks, command runner. The only runtime crate that is not a module; the navree facade, navree-macros and navree-cli are tooling |
| `navree-macros` | Attributes, facades | `#[module]`, `#[injectable]`, `#[controller]`, route and job macros |
| `navree-http` | Routing, middleware, responses | Axum integration, default Tower layers, problem+json, uploads, maintenance mode |
| `navree-validation` | Validator, form requests | Validated extractors, rules, localized messages |
| `navree-db` | Eloquent, migrations, seeders, factories | ORM integration, migrations, seeders, factories, pagination |
| `navree-auth` | Auth, Sanctum, Fortify, Socialite, Passport | Login, registration, password reset, email verification, API tokens, 2FA, passkeys, social login |
| `navree-cache` | Cache, atomic locks | In-memory + Redis cache, tags, locks |
| `navree-queue` | Queues, Horizon | Jobs, workers, retries, batches, dashboard |
| `navree-schedule` | Task scheduler | Cron-style schedules, overlap prevention, single-node run |
| `navree-events` | Events, listeners, observers | Typed event bus, queued listeners |
| `navree-mail` / `navree-notify` | Mail, notifications | Mailables, transports, multi-channel notifications |
| `navree-storage` | Storage (Flysystem) | Local, S3, GCS, Azure; signed URLs |
| `navree-realtime` | Broadcasting, Reverb | WebSocket/SSE server, channels, presence, auth |
| `navree-search` | Scout | Search engine adapters, model indexing |
| `navree-api` | API resources, (no Laravel OpenAPI) | API resources, OpenAPI generation, API versioning |
| `navree-transport` | (none; NestJS microservices) | Message-broker transports and RPC |
| `navree-observe` | Pulse, Telescope | Tracing, metrics, health checks, debug toolbar |
| `navree-ai` | Laravel AI SDK, MCP | LLM providers, embeddings, vector stores, MCP server |
| `navree-i18n`, `navree-flags`, `navree-billing` | Localization, Pennant, Cashier | Translations, feature flags, payments |
| `navree-view` | Blade (email templates only) | Email template rendering; Navree renders no web pages |
| `navree-testing` | HTTP tests, fakes | Test app, HTTP client, fakes, DB refresh |
| `navree-cli` | Artisan | `navree new`, `navree dev`, `make:*`, `check` and `routes`; runtime commands such as `migrate` and `queue:work` live in the app binary |
| `navree-config` | config/\*.php, .env | ConfigModule: typed config sections from .env, files, env vars and secret stores, validated at boot |
| `navree-redis` | Redis facade | RedisModule: one shared connection pool for cache, queue, realtime and locks |
| `navree-log` | Log channels, Context | LoggerModule: tracing subscriber, request IDs, log context |
| `navree-crypto` | Crypt, Hash | CryptoModule: Encrypter with key rotation, password Hasher |
| `navree-session` | Session, cookies, CSRF | SessionModule: session stores, encrypted cookies, CSRF token check |
| `navree-http-client` | Http facade | HttpClientModule: outgoing HTTP with retries, timeouts, SSRF policy, fakes |
| `navree-grpc` | (none) | GrpcModule: gRPC and Connect services sharing the module DI |
| `navree-export` | Laravel Excel, PDF packages | ExportModule: PDF, Excel, CSV and image helpers |
| `navree-admin, navree-graphql` | Nova, Lighthouse | Phase 4 modules: an admin API with a Next.js or Nuxt admin UI, and GraphQL |
| `navree-access` | Gates, policies, laravel-permission (package) | AccessModule: roles, permissions, CASL-style abilities, query and field filtering |
| `navree-audit` | laravel-activitylog (package) | AuditModule: audit trail of data, sign-in and permission changes |
| `navree-contracts` | Contracts (interfaces) | Small traits (AuditSink, EventSink, Broadcaster, SessionStore, HealthCheck) that keep the crate graph acyclic |

Crate dependencies form an explicit acyclic graph that CI checks on every build. Cross-cutting needs go through small traits in a low-level `navree-contracts` crate (`AuditSink`, `EventSink`, `Broadcaster`, `SessionStore`, `HealthCheck`), so, for example, `navree-auth` writes audit entries and `navree-db` raises model events without depending on `navree-audit` or `navree-events`. This is also why logging, crypto and sessions are separate crates rather than parts of `navree-http` or `navree-auth`.

### Request lifecycle

1. Hyper accepts the connection on a Tokio task.
2. Outer layers run in a fixed order: request ID (validated), tracing span, CORS (so even error responses carry CORS headers), panic catcher, timeout, security headers, compression (skipped for `text/event-stream` so SSE streams), maintenance mode, per-IP rate limit.
3. The router matches the path to a module's router; route layers set that route's body-size limit, so uploads can stream.
4. The session layer attaches the session and, for cookie-authenticated state-changing requests, checks the CSRF token against the session (419 on failure). The current user loads lazily through the `Auth` extractor, only on routes that need it.
5. Module layers run, then the route's authorization (`#[roles]`, `#[permissions]`, `#[can]`) and per-user rate limits; failures return 401 or 403.
6. The unit of work opens (transaction, actor, request ID, outbox). Extractors parse and validate input (422 with field errors), and `#[can(..., from = "id")]` loads the record inside the unit of work.
7. The handler runs inside its interceptors (cache, timing, response mapping). `#[transactional]` handlers run to completion even if the client disconnects.
8. Errors become problem+json; success becomes JSON, a file download or a stream, with field rules masking the response.
9. The unit of work commits entity changes, audit rows and outbox entries (jobs, events) in one transaction; a relay moves outbox entries to the queue after commit, using the outbox row ID as the idempotency key.
10. Metrics record after the response on a task tracker that is drained at shutdown.

A single binary can run several roles chosen by subcommand: `serve` (HTTP), `queue:work` (workers), `schedule:work` (scheduler), `realtime:serve` (WebSocket server). All roles share the same module graph. The in-process realtime role arrives in Phase 3; until then Sockudo runs alongside.

## Module system: everything is a module

Everything in Navree is a module, as in NestJS: the HTTP server, config, logging, the database, every capability and every feature you write. Only a small kernel (module graph, DI container, lifecycle) sits outside. Modules are resolved by macros plus a boot-time check, not runtime reflection, so a missing provider or a dependency cycle stops the app before it binds a port, never mid-request.

### Kernel and modules

| Layer | What it contains | A module? |
| --- | --- | --- |
| Kernel (`navree-kernel`) | `Module` trait, module graph, DI container, lifecycle, boot checks, command runner | No. The only non-module part, kept deliberately small |
| Platform modules | `ConfigModule`, `LoggerModule`, `HttpModule`, `RedisModule`, CryptoModule | Yes |
| Capability modules | `DatabaseModule`, `AuthModule`, `QueueModule`, `MailModule`, ViewModule and the rest of the catalogue below | Yes |
| API surface | `ApiModule, RealtimeModule` | Yes |
| Feature modules | Your `UsersModule`, `BillingModule`, `OrdersModule` | Yes |

A Navree app is a root module that imports other modules. Remove a module and its routes, migrations, commands, jobs and config disappear with it.

### Module catalogue (first-party)

| Module | Crate | Configured with | Main exports | Imports |
| --- | --- | --- | --- | --- |
| `ConfigModule` (global) | `navree-config` | `for_root(sources)`: .env, files, env vars, secret stores | Typed `Config<T>` sections | None |
| `LoggerModule` (global) | `navree-log` | `for_root(format, level)` | Tracing subscriber, request IDs | Config |
| `HttpModule` | `navree-http` | `for_root(addr, layers)` | Router, default layers | Config, Logger |
| `RedisModule` | `navree-redis` | `for_root(url, pool)` | `RedisPool` | Config |
| `DatabaseModule` | `navree-db` | `for_root(url, pool)`; `for_feature([User, Post])` | `Db`, repositories, tenant scoping, model-event hooks | Config |
| `ValidationModule` (global) | `navree-validation` | `for_root(messages)` | `Valid<T>` extractors, custom rules | Database (optional, for unique and exists rules) |
| `AuthModule` | `navree-auth` | `for_root(guards, providers)` | `Auth`, `Authenticated guard, login, token, 2FA and passkey flows` | Session, Crypto, Database, Cache, Mail, Http |
| `CacheModule` | `navree-cache` | `for_root(store)` | `Cache`, `Lock` | Config, Redis (optional) |
| `QueueModule` | `navree-queue` | `for_root(driver)`; `for_feature([SendInvoice])` | `Queue`, `dispatch()` | Database or Redis |
| `ScheduleModule` | `navree-schedule` | `for_root()` | `Schedule` | Queue, and Database or Redis for run records |
| `EventsModule` (global) | `navree-events` | `for_root()` | `EventBus` | Queue (queued listeners) |
| `MailModule` | `navree-mail` | `for_root(transports)` | `Mailer` | Queue, View |
| `NotifyModule` | `navree-notify` | `for_root(channels)` | `Notifier` | Mail, Database, HttpClient; broadcasts through the Broadcaster contract |
| `StorageModule` | `navree-storage` | `for_root(disks)` | `Storage`, `Disk` | Config |
| `RealtimeModule` | `navree-realtime` | `for_root(in-process or external server)` | `Broadcaster`, channel auth | Auth, Access, Redis |
| `SearchModule` | `navree-search` | `for_root(engine)`; `for_feature([Post])` | `Search` | Database, Queue |
| `ApiModule` | `navree-api` | `for_root(openapi, versioning)` | OpenAPI document, versioning | Http |
| `GrpcModule` | `navree-grpc` | `for_root(addr)` | Service registry | Http |
| `TransportModule` | `navree-transport` | `for_root(nats / rabbitmq / kafka / redis)` | `Client`, message handlers | Config |
| `ObserveModule` | `navree-observe` | `for_root(otlp, prometheus, sentry)` | Metrics, health registry, request-inspector API | Http, Logger |
| `AiModule` | `navree-ai` | `for_root(providers)` | `Llm`, `Embeddings`, MCP server | HttpClient, Access |
| `I18nModule` | `navree-i18n` | `for_root(locales)` | `Translator` | Http |
| `FlagsModule` | `navree-flags` | `for_root(store)` | `Features` | Database, Cache |
| `BillingModule` | `navree-billing` | `for_root(gateway keys)` | `Billing`, webhook routes | Database, Queue |
| `AdminModule` | `navree-admin` | `for_root()`; `for_feature([UserResource])` | Admin API; its UI is a Next.js or Nuxt kit | Auth, Access, Api |
| `ViewModule` | `navree-view` | `for_root(template dirs)` | Email template rendering | Config |
| `TestingModule` | `navree-testing` | Tests only | `TestApp`, fakes for every module | Any |
| CryptoModule (global) | navree-crypto | for\_root(keys, hashing params) | Encrypter, Hasher | Config |
| SessionModule | navree-session | for\_root(store, cookie, csrf mode) | Session, CSRF token check (419 on failure) | Http, Crypto, Redis or Database |
| HttpClientModule | navree-http-client | for\_root(timeouts, retry, ssrf policy) | HttpClient | Config, Logger |
| ExportModule | navree-export | for\_root(fonts, templates) | Pdf, Spreadsheet, Images | Storage, Queue |
| GraphqlModule (Phase 4) | navree-graphql | for\_root(...) | GraphQL schema and endpoint | Http, Database |
| AccessModule | navree-access | for\_root(abilities fn, super-admin role) | Ability, Roles, Permissions, #\[roles\] / #\[permissions\] / #\[can\] guards, accessible\_by query filter | Auth, Database, Cache |
| AuditModule (global) | navree-audit | for\_root(retention, sinks) | AuditLog, entity change hooks, #\[audit\] support | Database, Logger (the actor comes from the unit of work) |

`for_root` configures a module once for the app; `for_feature` lets a feature module register its own entities, jobs or searchable models with it, exactly as in NestJS.

### What a module can contribute

A module is more than providers. Through its `register` method it can contribute everything a Laravel package publishes:

- Providers and exports (dependency injection)
- Controllers and routes, with their guards and layers
- A typed config section, validated at boot
- Database migrations and seeders; each module has its own migrator that declares which modules' migrations must run first, so cross-module foreign keys apply in the right order, and a migration can opt out of the transaction for steps such as concurrent index builds
- CLI commands (`navree billing:sync-plans`)
- Scheduled tasks, jobs and event listeners
- Email templates and translations
- Health checks and metrics
- OpenAPI tags, gRPC services, real-time channels, admin resources and feature flags

```rust
impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.imports((
            DatabaseModule::for_feature([Subscription::entity()]),
            QueueModule::for_feature([SyncInvoices::job()]),
            ScheduleModule,
        ))
        .provider::<BillingService>()
        .controller::<WebhookController>()
        .config::<BillingConfig>("billing")
        .migrations(include_migrations!("migrations"))
        .command::<SyncPlansCommand>()
        .schedule(|s| s.job::<SyncInvoices>().hourly())
        .listen::<UserDeleted, CancelSubscriptions>()
        .exports::<BillingService>();
    }
}
```

The `#[module]` attribute generates this method for the common cases; hand-written `register` is the escape hatch.

### Anatomy of a feature module

```text
src/modules/users/
├── mod.rs           # #[module] UsersModule
├── controller.rs    # #[controller("/users")]
├── service.rs       # #[injectable] UserService
├── entity.rs        # ORM model
├── dto.rs           # request and response types with validation rules
├── policy.rs        # authorization rules
├── jobs.rs          # #[job] structs
├── events.rs        # events and listeners
├── migrations/      # migrations owned by this module
├── mail/            # email templates
├── lang/            # translations
└── tests/           # tests using TestingModule
```

`navree make:module users --crud` generates it. When a module grows, `navree module:extract users` moves it into its own crate and rewrites its paths automatically, so it can be shared across apps.

### Boundaries between modules

- Modules call each other only through exported providers.
- Loose coupling goes through `EventsModule`; listeners can be queued.
- `TransportModule` offers the same request/reply and event API in-process or over a broker, so a module can later run as its own service, like NestJS hybrid apps.
- `navree check --graph` prints the module graph and fails CI when a module reaches a provider it did not import.

### NestJS concepts and their Navree equivalents

| NestJS | Navree | Built on |
| --- | --- | --- |
| `@Module({ imports, providers, controllers, exports })` | `#[module(...)]` on a struct, generating `impl Module` | Proc macro + trait |
| `@Injectable()` provider | `#[injectable]`; constructor injection from struct fields | Type-keyed container |
| Scopes DEFAULT / REQUEST / TRANSIENT | `Singleton` (built at boot, shared `Arc`), `Request` (built per request, cached in request extensions), `Transient` (new on every resolve) | Container + Axum extractor |
| Dynamic module `forRoot()` / `forRootAsync()` | `ConfigModule::for_root(opts)` returning `impl Module`; async factories allowed | Builder functions |
| `@Global()` module | `#[module(global)]` | Root scope |
| Controller with `@Get()`, `@Post()` | `#[controller("/users")]` impl block with `#[get]`, `#[post]` | Expands to an `axum::Router` |
| Middleware | Tower `Layer` | tower, tower-http |
| Guard | `Guard` trait with `fn check<'a>(&'a self, req: &'a mut Parts) -> BoxFuture<'a, Result<(), Deny>>`; `#[guard(...)]` on controller or route. The boxed Send future lets guards live as trait objects inside Tower layers; per-entity policies run in the handler after extraction | Axum middleware |
| Pipe (validate / transform) | Extractors: `Valid<Json<T>>`, `Path<T>`, `Query<T>`, custom extractors | Axum extractors + validation crate |
| Interceptor | Tower layer around the handler (timing, caching, response mapping) | tower |
| Exception filter | `AppError` + `IntoResponse`; one mapping layer emits RFC 9457 problem+json | Axum `IntoResponse` |
| Lifecycle hooks (`onModuleInit`, `onApplicationBootstrap`, `onModuleDestroy`, `beforeApplicationShutdown`) | `Lifecycle` trait: `on_init`, `on_bootstrap`, `on_shutdown`; graceful shutdown on SIGTERM | tokio signals + `CancellationToken` |
| `@nestjs/event-emitter`, `@nestjs/cqrs` | `navree-events` (typed in-process bus); `navree-cqrs` (Phase 4 candidate) later | tokio channels |
| `@nestjs/microservices` transports | `Transport` trait with NATS, Redis Streams, RabbitMQ and Kafka adapters; gRPC lives in GrpcModule | Client crates per broker |

### What a module looks like

```rust
// Feature module: providers, a controller, and what it shares
#[module(
    imports = [DatabaseModule, AccessModule, MailModule],
    providers = [UserRepository, UserService],
    controllers = [UserController],
    exports = [UserService],
)]
pub struct UsersModule;

#[injectable] // Singleton by default; never holds request state
pub struct UserService {
    repo: Arc<UserRepository>,
    mail: Arc<Mailer>,
    events: Arc<EventBus>,
}

#[injectable]
pub struct UserController {
    users: Arc<UserService>,
}

#[controller("/users")]
impl UserController {
    #[get("/{id}")]
    #[can(Read, User, from = "id")]
    async fn show(&self, user: Authorized<User, Read>) -> Result<Json<UserDto>> {
        Ok(Json(user.into_inner().into()))
    }

    #[post("/")]
    #[permissions("users.create")]
    #[transactional]
    async fn store(&self, uow: Uow, Valid(Json(input)): Valid<Json<CreateUser>>) -> Result<Created<UserDto>> {
        let user = self.users.create(&uow, input).await?;
        Ok(Created::at(format!("/users/{}", user.id), user.into()))
    }
}

// Root module (generated by navree new): every imported module is configured once
fn main() -> navree::Result<()> {
    Navree::app()
        .module(ConfigModule::for_root(Env::from_dotenv()))
        .module(LoggerModule::for_root(LogFormat::Json, Level::INFO))
        .module(CryptoModule::for_root(Keys::from_env()))
        .module(HttpModule::for_root("0.0.0.0:8080", Layers::default()))
        .module(RedisModule::for_root(RedisConfig::from_env()))
        .module(DatabaseModule::for_root(DbConfig::from_env()))
        .module(CacheModule::for_root(CacheStore::Redis))
        .module(QueueModule::for_root(QueueDriver::Postgres))
        .module(EventsModule::for_root())
        .module(ViewModule::for_root("resources/mail"))
        .module(MailModule::for_root(Transports::smtp_from_env()))
        .module(SessionModule::for_root(SessionStore::Redis))
        .module(AuthModule::for_root(Guards::session_and_tokens()))
        .module(AccessModule::for_root(abilities::define))
        .module(AuditModule::for_root(AuditConfig::default()))
        .module(UsersModule)
        .run()
}
```

### How resolution works

1. Each `#[module]` expands to an `impl Module` that lists its imports, providers, controllers and exports.
2. At boot, Navree walks the module graph from the root and builds a provider graph keyed by type plus an optional name, so two databases or several storage disks can coexist. It checks that every dependency is provided or imported, that only exported providers cross module lines, and that there are no cycles. A failure aborts startup with the module and missing type named.
3. Singletons are built once, in dependency order, and stored as `Arc<T>` in an immutable map. Handlers read them without locks.
4. Request-scoped providers are built on first use within a request and cached in the request's extensions.
5. Controllers become Axum routers merged under their prefixes; module-level guards and layers wrap only that module's routes. Each module router is sealed with its own state, and Navree adds the single fallback route last (see the Axum rules below).
6. `navree check` runs the same validation without starting servers, so CI catches wiring errors before deploy.

### Rules

- Visibility works as in NestJS: a provider is private to its module unless exported.
- Constructor injection only. No property injection; a service locator (`ctx.get::<T>()`) exists only inside lifecycle hooks.
- Every first-party capability is a module crate behind a Cargo feature, so unused modules are not compiled into the binary.
- Circular dependencies are an error. The escape hatches are events or a `Lazy<T>` handle resolved after boot.
- Traits as tokens: a module may register `Arc<dyn MailTransport>` and let apps swap the implementation, like NestJS custom providers.

### Scopes and identity

- **One unit-of-work scope.** "Request scope" covers every unit of work, not just HTTP: one HTTP request, one job run, one scheduled task, one gRPC call, one WebSocket message or one CLI command. Each unit of work carries a Uow value (database transaction, actor, request ID and outbox), opened before extractors run; services take it as a parameter, for example self.posts.update(&uow, ...), instead of having it injected, so singletons never hold request state.
- **No scope inversion.** A singleton may not depend on a request-scoped provider; the boot check rejects that edge. Controllers are singletons, so request-scoped values (current user, tenant, transaction) reach handlers as extractors.
- **Named providers.** Providers are keyed by type plus an optional name, e.g. `Db` and `Db("analytics")`, or one `Disk` per configured disk.
- **Configuration merges.** `for_root` configures a module once for the app; each `for_feature` call from another module adds registrations (entities, jobs, searchable models) to that one instance. Importing a module by type alone gives access to its exports without reconfiguring it.
- **Authorization in two steps.** Route guards check identity and abilities that need no loaded data. Per-entity policies run in the handler after extraction: `ability.authorize(Update, &post)?`.

### What cannot be copied one-to-one

- No runtime reflection: macro-generated code replaces decorator metadata, so injected types must be concrete types or explicitly registered trait objects.
- Wiring errors surface at boot or in `navree check`, not as compiler errors, unless compile-time checking is adopted later (see Alternatives considered).
- No runtime hot-swapping of modules; the dev loop relies on fast rebuilds.

### Rules the module system follows on Axum

These come from Axum's own documentation on [routers](https://docs.rs/axum/latest/axum/struct.Router.html), [state](https://docs.rs/axum/latest/axum/extract/struct.State.html) and [middleware](https://docs.rs/axum/latest/axum/middleware/index.html):

- Each module builds its router as `Router<ModuleCtx>` and seals it with `.with_state(ctx)` before Navree nests or merges it, so no app-wide state struct exists and third-party module crates compose freely. Modules never set a fallback, because Axum's `merge` panics when two routers both define one; Navree adds the single fallback last.
- Guards attach with `route_layer`, so a request to an unknown path still returns 404 instead of 401.
- Interceptor order follows Tower: `Router::layer` wraps like an onion, while `ServiceBuilder` runs top to bottom. Navree fixes one order, outermost first: request ID, tracing, CORS, panic catcher, timeout, security headers, compression, maintenance mode, per-IP rate limit, body limit, session and CSRF, module layers, authorization and per-user limits, then the handler. CORS sits outside the panic catcher and timeout so error responses still carry CORS headers, and it lists allowed origins, request headers (X-XSRF-TOKEN, traceparent, X-Request-Id) and exposed headers (X-Request-Id, ETag) explicitly, because credentials cannot be combined with wildcards.
- Middleware errors become responses through `HandleErrorLayer` and the same problem+json mapping as handler errors.
- The container has no global statics, so it can be rebuilt on every dev restart or hot patch.

### Alternatives considered

No existing Rust crate offers a mature NestJS-style module system, so Navree builds its own and borrows from the closest designs.

| Option | Latest (release) | Model | Verdict |
| --- | --- | --- | --- |
| Navree's own module layer (chosen) | — | Macro-generated modules, type-keyed container, full graph check at boot and in `navree check` | Chosen |
| [Pavex](https://docs.rs/crate/pavex/latest) | 0.2.10 (Nov 2025); last reported in closed beta | Compile-time DI through a code-generation step | Strongest guarantees, but not yet 1.0 and adds codegen to every build. Revisit after Navree v1 |
| [a3s-boot](https://docs.rs/crate/a3s-boot/0.1.3) | 0.2.0 (Aug 2026) | Closest to NestJS: imports/exports, three scopes, `ModuleRef`, lazy provider refs, lifecycle hooks, Axum adapter | Too young to depend on; study its design |
| [nestrs](https://users.rust-lang.org/t/nestrs-a-nestjs-style-framework-for-rust-built-on-axum-and-tower/139700) | 1.6.0 (30 Sep 2026; 1.0 on 13 Sep 2026) | NestJS-style on Axum + Tower: modules, guards, pipes, interceptors, filters | Closest NestJS clone on Axum, but 1.0 is only weeks old and early versions drew code-quality concerns. Re-evaluate in Phase 0 before building Navree's own layer |
| [spring-rs](https://docs.rs/crate/spring/latest) | 0.4.8 (Jan 2026) | Plugins auto-registered with `inventory`; `Component<T>` extractor; many plugins (SeaORM, Redis, mail, apalis, gRPC) | Spring-style plugins without NestJS import/export visibility |
| [Loco](https://loco.rs/docs/explanation/appcontext-and-di/) | 1.2.0 (Sep 2026) | Rails-style; hooks and initializers; `SharedStore` type map | By design has no dependency resolution or module visibility |
| [dill](https://docs.rs/crate/dill/latest) | 0.17.2 (Oct 2026) | Runtime DI with transient, singleton and transaction scopes | A container, not a module system; candidate engine under the kernel |
| [nject](https://docs.rs/nject), [shaku](https://docs.rs/crate/shaku/0.6.3), [fundle](https://docs.rs/crate/fundle/0.3.5) | 0.5.1, 0.6.3, 0.3.5 | Compile-time DI with explicit modules | Limits on generic modules and cross-crate re-exports; async setup undocumented |

Compile-time checking of a whole module graph across crates needs a separate code-generation step, which is how Pavex works. Navree instead validates the graph at boot and in CI, which catches the same errors before deploy without slowing every build.

## Annotations (attributes)

Navree is annotation-driven like NestJS: everything declarative is a Rust attribute, much as Laravel 13 added optional PHP attributes for models, jobs and middleware. Each attribute expands at compile time into plain Rust (no runtime reflection) and has an equivalent builder call. `navree-macros` holds the module, DI and route attributes; every other module crate ships its own attributes behind its Cargo feature (for example `navree-access` provides `#[roles]`, `#[permissions]` and `#[can]`), and third-party modules add attributes the same way.

### Attribute reference

| Attribute | On | What it does | NestJS / Laravel equivalent |
| --- | --- | --- | --- |
| **Modules, DI and config** |  |  |  |
| `#[module(imports, providers, controllers, exports, global)]` | struct | Declares a module | `@Module()` / service provider |
| `#[injectable(scope = "singleton" \| "request" \| "transient")]` | struct | Registers a provider; fields are injected | `@Injectable()` |
| `#[inject(name = "analytics")]` | field | Injects a named provider | `@Inject(token)` |
| `#[config(section = "billing")]` | struct | Typed config section, validated at boot | Config file |
| **HTTP** |  |  |  |
| `#[controller("/posts", version = "1")]` | impl | Route prefix and API version; methods take `&self` because the generated handlers capture the controller | `@Controller()` / route group |
| `#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]` | fn | Routes | `@Get()` and friends |
| `#[crud(Post)]` | impl | Generates the five standard JSON routes (index, show, store, update, destroy) | `Route::apiResource` |
| `#[public]` | fn, impl | Route needs no authentication or authorization | Custom `@Public()` |
| `#[guard(...)]` | fn, impl | Runs custom guards before the handler | `@UseGuards()` / middleware |
| `#[layer(...)]` | fn, impl | Wraps the handler in Tower layers (interceptors) | `@UseInterceptors()` |
| `#[throttle(limit = 60, per = "1m")]` | fn, impl | Rate limit, per user when signed in | `@Throttle()` / `throttle:` |
| `#[timeout("5s")]` | fn | Request timeout | — |
| `#[cache(ttl = "60s", per_user)]` | fn | Caches the response; the key includes the user's access version | `@CacheTTL()` / `Cache::remember` |
| `#[transactional]` | fn | Runs the handler in the request's unit of work and commits on success; the body runs to completion even if the client disconnects | `DB::transaction` |
| **Validation and API docs** |  |  |  |
| `#[dto]` with `#[validate(email, length(max = 255), unique("users", "email"))]` | struct, field | Request DTO and rules; database rules (`unique`, `exists`) run in a second async pass inside the unit of work | class-validator / form request rules |
| `#[resource]` | struct | Response shape, with field-level permission masking | API Resource |
| `#[api(summary = "...", tag = "Posts", responses(404, 422))]` | fn | OpenAPI metadata | `@ApiOperation()`, `@ApiResponse()` |
| `Secret<T>` (a type, not an attribute) | field | Redacted in `Debug`, logs and audit; never serialised into API responses | `$hidden` |
| **Authorization** |  |  |  |
| `#[roles("admin", "editor")]` | fn, impl | Requires any of the roles | Custom `@Roles()` / `role:` middleware |
| `#[permissions("posts.publish")]` | fn, impl | Requires the permission; same as `#[can(Publish, Post)]` with no conditions | `permission:` middleware (laravel-permission) |
| `#[can(Read, Post)]` | fn | Ability check on the subject type; list handlers must then take `Accessible<Post, Read>` | `@CheckPolicies()` with CASL / `can:` middleware |
| `#[can(Update, Post, from = "id")]` | fn | Loads the record `FOR UPDATE` in the unit of work and checks the rule conditions; handler takes `Authorized<Post, Update>` | Route model binding + policy |
| `#[abilities]` | fn | Defines the CASL-style rules | `defineAbilityFor()` / `Gate::define` |
| `#[policy(Post)]` | impl | Groups ability rules for one entity | Laravel Policy |
| **Data** |  |  |  |
| `#[entity(table = "posts")]` | struct | ORM model | Eloquent model / `#[Table]` |
| `#[soft_delete]` | struct | Soft deletes | `SoftDeletes` |
| `#[audit(skip)]` | struct, field | Excludes an entity or field from the audit trail (auditing is on by default) | `LogsActivity` options |
| `#[searchable]` | struct | Indexed by the search engine | `Searchable` (Scout) |
| `#[tenant_scoped]` | struct | Row-level tenant filter | Tenancy package scopes |
| `#[seeder]`, `#[factory(Post)]` | struct | Database seeders and model factories | Seeder, factory classes |
| **Background work, events and messaging** |  |  |  |
| `#[job(queue = "emails", tries = 3, backoff = "exponential", timeout = "120s")]` | struct | Queued job | Laravel 13 `#[Queue]`, `#[Tries]`, `#[Timeout]` |
| `#[event]` | struct | Event type | Event class |
| `#[listener(UserRegistered, queued)]` | fn, struct | Event listener | `@OnEvent()` / listener |
| `#[schedule("0 2 * * *", without_overlapping, on_one_server)]` | fn | Scheduled task | `@Cron()` / `Schedule` |
| `#[command("billing:sync-plans")]` | struct | CLI command | Artisan command |
| `#[message_handler("orders.created")]` | fn | Handles a broker message (NATS, RabbitMQ, Kafka, Redis Streams) | `@MessagePattern()` / `@EventPattern()` |
| `#[grpc_service]` | impl | Registers a gRPC service inside the module | `@GrpcMethod()` |
| **Mail, notifications and real-time** |  |  |  |
| `#[mailable(template = "mail/welcome")]` | struct | Email message | Mailable |
| `#[notification(channels(mail, database, broadcast))]` | struct | Multi-channel notification | Notification class |
| `#[channel("orders.{id}")]` | fn | Authorizes a private or presence channel | `Broadcast::channel` |
| **Operations and flags** |  |  |  |
| `#[health_check("database")]` | fn | Registers a readiness check | Health check route |
| `#[feature("new-checkout")]` | fn, impl | Route active only when the flag is on (Phase 3) | Pennant `Feature::active` |
| **Logging and audit** |  |  |  |
| `#[log(level = "debug", skip(password))]` | fn | Tracing span with arguments; skipped fields never logged | `tracing::instrument` |
| `#[audit(action = "invoice.sent")]` | fn | Writes a custom audit entry | `activity()->log()` |
| **Testing** |  |  |  |
| `#[navree::test]` | fn | Test with a booted app on a fresh database cloned from a template, with workers and the outbox running | `RefreshDatabase` |

### A controller using them

```rust
#[controller("/posts", version = "1")]
impl PostController {
    #[get("/")]
    #[can(Read, Post)]
    #[cache(ttl = "30s", per_user)]
    #[api(summary = "List posts the user may read", tag = "Posts")]
    async fn index(&self, uow: Uow, posts: Accessible<Post, Read>, page: PageQuery) -> Result<Page<PostDto>> {
        // Accessible<Post, Read> is a query already filtered by the user's rules
        self.posts.list(&uow, posts, page).await
    }

    #[patch("/{id}")]
    #[can(Update, Post, from = "id")] // loads the post FOR UPDATE and checks e.g. author_id = user.id
    #[transactional]
    async fn update(
        &self,
        uow: Uow,
        post: Authorized<Post, Update>,
        Valid(Json(input)): Valid<Json<UpdatePost>>,
    ) -> Result<Json<PostDto>> {
        Ok(Json(self.posts.update(&uow, post, input).await?.into()))
    }

    #[post("/{id}/publish")]
    #[permissions("posts.publish")]
    #[throttle(limit = 10, per = "1m")]
    #[audit(action = "post.published")]
    #[transactional]
    async fn publish(&self, uow: Uow, Path(id): Path<Uuid>) -> Result<NoContent> {
        self.posts.publish(&uow, id).await?;
        Ok(NoContent)
    }
}
```

**Deny by default.** Every route is authenticated unless marked `#[public]`, and every non-public route must carry an authorization attribute: `#[roles]`, `#[permissions]` or `#[can]`. A custom `#[guard]` alone does not count. `#[controller]` emits a compile error for a route without one, and the app refuses to boot if a hand-registered route lacks one, so a forgotten permission is caught by the compiler, not by a penetration test.

## Roles, permissions and abilities (CASL-style, built in)

Authorization is built in through `AccessModule` (`navree-access`, Phase 1): roles and permissions live in the database, and CASL-style ability rules are defined in Rust. The API enforces the rules on every request, and the same rules ship to Next.js or Nuxt in CASL's own JSON format, so the UI hides exactly what the API would refuse.

### Three layers, one engine

| Layer | What it is | Example | Laravel / NestJS equivalent |
| --- | --- | --- | --- |
| Roles | Named groups of permissions; global in Phase 1, optionally per team once Teams ship (Phase 2) | `admin`, `editor`, `billing-manager` | Roles in spatie/laravel-permission |
| Permissions | Strings of the form `subject.action`, stored in the database and given to roles or directly to users | `posts.publish`, `invoices.export` | Permissions in spatie/laravel-permission |
| Abilities | CASL rules: action + subject + optional conditions + optional fields; can be inverted | Update `Post` where `author_id` = the user, fields `title` and `body` only | CASL `defineAbilityFor`; Laravel gates and policies |

**One vocabulary.** A permission string `posts.publish` compiles to the rule "can `publish` `Post`" with no conditions. `#[permissions("posts.publish")]` is shorthand for `#[can(Publish, Post)]` and passes only if an unconditional rule grants it. API tokens carry a list of permission strings; a request made with a token gets the user's rules restricted to the token's permissions, the equivalent of Sanctum token abilities.

### Defining abilities

```rust
#[abilities]
fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    // AbilityCtx: optional user, current team, preloaded permissions
    let Some(user) = ctx.user else {
        rules.can(READ, Post::SUBJECT).when(post::PUBLISHED.eq(true)); // guests
        return;
    };
    if user.has_role("admin") {
        rules.can(MANAGE, ALL);
        return;
    }
    rules.from_permissions(&ctx.permissions); // e.g. posts.publish from database roles
    rules.can(READ, Post::SUBJECT).when(post::PUBLISHED.eq(true));
    rules.can(UPDATE, Post::SUBJECT)
        .fields([post::TITLE, post::BODY])
        .when(post::AUTHOR_ID.eq(user.id));
    rules.can(DELETE, Post::SUBJECT).when(post::AUTHOR_ID.eq(user.id));
    rules.cannot(DELETE, Post::SUBJECT).when(post::LOCKED.eq(true)); // last: last match wins
}
```

Actions are an open string type with constants for the standard ones (`READ`, `UPDATE`, `MANAGE`, and so on), so custom actions such as `publish` work. Field and column names are generated constants, so a typo is a compile error.

### Rule semantics (CASL-compatible)

- **Order:** rules are checked in order and the last matching rule wins, exactly as in CASL. Put `cannot` rules last. Lists use the same order: Navree compiles the rules into one SQL `CASE` chain, so a record check and a list filter never disagree.
- **Actions:** `create`, `read`, `update`, `delete` and `manage` (any action), plus custom actions.
- **Subjects:** entity types such as `Post` and `Invoice`, or `all`.
- **Conditions:** in v1, only on top-level scalar columns, with `$eq`, `$ne`, `$lt`, `$lte`, `$gt`, `$gte`, `$in` and `$nin`. The SQL translation matches JavaScript behaviour: `$eq: null` becomes `IS NULL`, and `$ne` and `$nin` use `IS DISTINCT FROM` so null columns behave as they do in the browser. Timestamps are compared as RFC 3339 UTC with fixed precision; integers above 2^53 and decimals travel as strings and allow equality operators only. `$exists`, nested fields and other CASL operators are rejected when the rules are defined.
- **Fields:** rules use the API (wire) field names. Request fields are checked against the raw JSON keys before deserialisation, so an absent field and a null field are told apart; responses are masked by the serialiser `#[resource]` generates; maskable fields are optional in the OpenAPI schema.
- **Teams:** once Teams ship (Phase 2), role assignments can be scoped to a team taken from the request.

### Where rules are enforced

1. **Routes:** `#[roles]`, `#[permissions]` and `#[can(Action, Subject)]` check the subject type before the handler runs.
2. **Records:** `#[can(Update, Post, from = "id")]` loads the record `FOR UPDATE` inside the request's unit of work and checks the conditions; the handler receives `Authorized<Post, Update>`. Field rules are checked against the columns that actually change, so a user cannot move a post to another author.
3. **Lists:** a route with `#[can(Read, Post)]` must take `Accessible<Post, Read>`, a query already filtered by the rules; it is the only way to get a list query for that route.
4. **Fields:** request DTOs reject fields the user may not change; responses drop fields the user may not see.
5. **Channels and jobs:** channel authorization checks abilities at subscribe time; when a user's roles change or they log out, Navree disconnects their sockets so stale subscriptions end. Queued jobs check abilities for the user they act for.
6. **Unannotated routes:** a compile error (see Deny by default under Annotations).

### Sharing rules with Next.js and Nuxt

- `GET /api/me/abilities` returns the user's rules as CASL's JSON rule array, which `createMongoAbility` accepts directly, with an ETag. Packing with `packRules` is an optional optimisation once its place in CASL 7 is confirmed. CASL's 7.x majors (May 2026) renamed `PureAbility` to `Ability` and changed the React API, so the kits target 7.x from the start.
- Next.js kit: [`@casl/ability`](https://tessl.io/registry/tessl/npm-casl--ability) 7.0.2 with [`@casl/react`](https://npmjs.com/package/@casl/react) 7.0.1 (`<Can>`, `useAbility`), wired up by `@navree/next`.
- Nuxt kit: `@casl/ability` with [`@casl/vue`](https://www.npmjs.com/package/@casl/vue) 3.0.1 for Vue 3 (`<Can>`, `useAbility`, `abilitiesPlugin`), wired up by `@navree/nuxt`.
- Every resource in the generated TypeScript client carries a constant `__type` field, and the kits configure CASL to read it, so `<Can I="update" this={post}>` works without CASL's `subject()` helper, which does not survive server-to-client serialisation.
- UI checks only decide what to show; the API re-checks every request.

```ts
import { createMongoAbility } from '@casl/ability';

// CASL's JSON rule array from GET /api/me/abilities
const ability = createMongoAbility(await navree.me.abilities(), {
  detectSubjectType: (o) => (typeof o === 'string' ? o : o.__type),
});
// @casl/react 7: provide `ability` once at the app root, then use
// <Can I="update" this={post}>Edit</Can> (7.x removed the `ability` prop)
```

### Storage, caching and tooling

- **Tables** owned by `AccessModule`: `roles`, `permissions`, `role_permissions`, `user_roles` (with `team_id` from Phase 2) and `user_permissions`.
- **Caching:** an `access_version` counter is bumped in the same transaction as any role or permission change. Each user's compiled abilities are cached under (user, team, access\_version, build), and the same value is the ETag, so changing a role's permissions invalidates every holder on every node without fan-out.
- **CLI:** `navree access:role <name>`, `navree access:grant <role> <permission>` and `navree access:explain <user> <action> <subject> <id>`, which prints the rule that allowed or denied the action.
- **Management endpoints:** owned by `AccessModule` (Phase 1); role-management screens ship in the Next.js and Nuxt kits (Phase 2).
- **Audit:** every role or permission change writes an audit entry.

### Alternatives considered

| Option | Latest | Verdict |
| --- | --- | --- |
| CASL-compatible engine inside Navree (chosen) | — | The same rules run on the server and in the browser with the official CASL packages |
| [Casbin](https://docs.rs/crate/casbin/latest) (`casbin`) | 2.20.0 (Feb 2026); ACL, RBAC with domains, ABAC | Mature, but policies live in Casbin model and policy files, there is no browser-side evaluation, and Navree would still need its own list filtering |
| [Cedar](https://docs.rs/crate/cedar-policy) (`cedar-policy`) | 4.14.0 (17 Oct 2026) | Strong policy language from AWS, but a separate language to learn and no CASL front-end interop. Candidate as a pluggable backend after v1 |
| nest-rs-authz (closest Rust prior art) | 6.1.0 (22 Oct 2026); part of the poem-based NestRS framework | CASL-like abilities driving access checks, a SeaORM pre-filter and field masking, but tied to NestRS and without CASL front-end interop; study its design |

## Logging and audit (on by default)

Every Navree app logs out of the box with no setup: structured request, error, query and job logs, plus a database audit trail of data, sign-in and permission changes, with secrets redacted. `LoggerModule` (`navree-log`, Phase 0) and `AuditModule` (`navree-audit`, Phase 1) are imported by every starter app; each log type switches on as the module that produces it lands.

### What is logged by default

| Log | What is recorded | Default destination | Available from | Laravel equivalent |
| --- | --- | --- | --- | --- |
| Requests | Method, route template, status, latency, request ID, user ID, team ID, client IP, user agent | stdout: readable in development, JSON in production | Phase 0 | None by default (Telescope in development) |
| Application | `tracing` events from your code and every module, carrying the request ID and user ID | stdout plus a daily rotating file (tracing-appender 0.2.5), kept 14 days | Phase 0 | `Log` facade, daily channel |
| Errors | Error chain, route, user and request ID, including panics | stdout; Sentry (sentry 0.49.3) once `ObserveModule` is configured | Phase 0; Sentry Phase 3 | Exception handler reporting |
| Queries | SQL, redacted bindings and duration: every query in development, only queries over 200 ms in production | stdout | Phase 1 | `DB::listen` |
| Jobs and schedules | Job or task name, attempt, duration, outcome, error | stdout; failures also in the failed-jobs table | Phase 1 (jobs), Phase 2 (schedules) | Queue events |
| Security | Denied authorization, failed logins, rate limiting, CSRF failures | stdout and the audit table | Phase 1 | — |
| Audit trail | Who changed what: creates, updates and deletes with old and new values; logins and logouts; 2FA, passkey and password changes; role and permission changes; custom `#[audit]` actions | Database table `audit_logs`; optional stream to an external sink | Phase 1 | spatie/laravel-activitylog (package) |

### How the audit trail is written

- **Same transaction as the change.** Each unit of work sets the actor and request ID on its transaction (`set_config('navree.actor', …, true)` on PostgreSQL). On PostgreSQL, generated audit triggers read them and record old and new values, which also covers bulk updates and raw SQL. On MySQL and SQLite, ORM hooks record changes made through the ORM. A rolled-back change therefore leaves no audit row.
- **Security events on their own connection.** Denied authorization and failed logins are written outside the request transaction, so they survive the rollback that usually follows.
- **On for every entity**, with `#[audit(skip)]` to exclude an entity or field. `Secret<T>` fields are never stored.
- **Retention:** `navree audit:prune` keeps audit rows for 365 days by default. It runs daily from the scheduler once that ships (Phase 2); before then, run it from system cron.
- **Reading it:** `AuditModule` owns endpoints that list entries by user, record or action; the Next.js and Nuxt kits include an activity screen (Phase 2).

### Defaults that make the logs useful

- **One request ID end to end.** Navree accepts or creates `X-Request-Id` and the W3C `traceparent` header, returns the ID in the response, and puts it on every log line, job and outgoing HTTP call. An incoming ID longer than 64 characters or containing characters outside letters, digits, `-` and `_` is replaced, so clients cannot forge log lines. The Next.js and Nuxt kits forward the ID from their server calls.
- **Redaction before writing.** `Secret<T>` values, passwords, tokens, `Authorization` headers, cookies and configured keys are written as `[redacted]`.
- **Levels per module at runtime.** A tracing reload handle changes levels without a restart; the change is broadcast to every replica over Redis.
- **Process-wide telemetry.** The tracing subscriber, metrics recorder, OpenTelemetry exporter and Sentry client are installed once per process, so the rebuildable DI container never re-installs them. File logs are flushed at shutdown.

### Configuration (every key is optional)

```toml
[log]
format = "json"            # "pretty" in development
level = "info"
levels = { navree_queue = "debug" }
file = { path = "storage/logs/navree.log", rotation = "daily", keep_days = 14 }
redact = ["password", "token", "authorization", "cookie"]
slow_query_ms = 200

[audit]
enabled = true
retention_days = 365
```

## Stack 1: Base stack

The base stack is buildable today on stable, maintained crates: Axum 0.8, Hyper 1.x, Tower 0.5 and tower-http 0.7 form the HTTP layer, and SeaORM 2.0 provides the ORM. Three gaps need Navree's own work: session stores lag the latest tower-sessions, axum-login has not caught up, and no maintained OAuth2 *server* crate exists.

### HTTP layer (pin these together)

| Crate | Version (release) | Role in Navree | Notes |
| --- | --- | --- | --- |
| axum | 0.8.9 (Apr 2026) | Routing, extractors, responses | Pre-1.0; no 0.9 release yet |
| axum-extra | 0.12.6 (Apr 2026) | Signed and private cookie jars, typed headers, typed routing | Features are opt-in |
| hyper | 1.12.0 (Oct 2026) | HTTP/1.1 and HTTP/2 | Minor release about every 2 months |
| hyper-util | 0.1.21 (Sep 2026) | Server glue, auto HTTP/1-or-2 connections | Still 0.1 |
| tower | 0.5.3 (Jan 2026) | `Service` and `Layer` traits; retry, timeout, limit layers | Slow but stable |
| tower-http | 0.7.1 (Aug 2026) | CORS, compression, timeouts, request ID, tracing, body limits, static files, **CSRF** | 0.7.0 (Jun 2026) was breaking; it added a built-in tokenless `CsrfLayer` |

HTTP/3 is **not** supported by Hyper or Axum: [hyper's HTTP/3 tracking issue](https://github.com/hyperium/hyper/issues/1818) has every item open, and [h3](https://docs.rs/crate/h3/latest) 0.0.8 calls itself very experimental. Navree serves HTTP/1.1 and HTTP/2 and leaves HTTP/3 to the reverse proxy or CDN.

### Requirements by Laravel feature

| Feature | Laravel | Navree module and behaviour | Crates (version) | Decision |
| --- | --- | --- | --- | --- |
| Routing | Route files, groups, model binding, resource routes, signed URLs | `#[controller]` + route attributes; #\[crud\] resource controllers; route-model binding via extractors; signed URLs | axum 0.8.9 | Wrap |
| Middleware | Global, group and route middleware | Tower layers at global, module and route level; fixed documented order | tower 0.5.3, tower-http 0.7.1 | Wrap |
| Request validation | Validator, form requests, 90+ rules | `Valid<Json<T>>` extractor; rules as attributes; Laravel-style rule names; localized messages; 422 with field errors | [garde](https://docs.rs/garde_derive) 0.23.0 (active); validator 0.21.0 (Jul 2026) | Wrap garde; Navree adds the missing Laravel rules (unique, exists, confirmed) |
| Errors | Exception handler | `AppError` mapped to RFC 9457 problem+json on every route, including 422 validation and 403 authorization errors | thiserror 2.0.21, anyhow 1.0.104 | Build |
| Serialization | API resources | `#[resource]` DTOs with field selection and pagination meta | serde 1.0.229, serde\_json 1.0.154 | Wrap |
| Configuration | config files, .env | `ConfigModule`: typed sections, env overrides, secret redaction, validated at boot | toml 1.1.8 and serde_json 1.0.154 (the file parsers); dotenvy 0.15.7 (no release since Mar 2023) | Build: Navree owns the loader (sources, merge, `.env` parser, conversion to typed sections, limits and error messages) on top of the toml and serde_json parsers, because configuration decides how every app behaves |
| ORM, migrations, seeders, factories, pagination | Eloquent, migrations, seeders, factories | `DatabaseModule`: entities, relations, scopes, soft deletes, model events, module-owned migrations, seeders, factories, offset and cursor pagination | SeaORM 2.0.0 (20 Jul 2026, per [SeaQL](https://www.sea-ql.org/blog/2026-07-27-sea-orm-2.0/)), now 2.0.4, after 43 release candidates: typed column constants, entity-first schema sync, nested saves. SQLx 0.9.0 (May 2026) and sea-query 1.0.2 underneath. Diesel 2.3.14 + diesel-async 0.9.2 remain an alternative | Wrap |
| Sessions | Session drivers | Cookie, database and Redis session stores | [tower-sessions](https://docs.rs/crate/tower-sessions/latest) 0.15.0 (Feb 2026); official SQLx and Redis stores lag behind 0.15; [tower-sessions-ext](https://docs.rs/tower-sessions-ext) 1.3.1 is a maintained fork | Build Navree's own stores on tower-sessions 0.15 |
| Cookies | Encrypted cookies | Signed and private (encrypted) cookie jars | axum-extra 0.12.6 on cookie 0.18 | Adopt |
| Login and guards | Auth guards, `auth()` | Session and token guards; the `Authenticated` guard; role, permission and ability checks come from `navree-access` | [axum-login](https://docs.rs/crate/axum-login/latest) 0.18.0 still needs tower-sessions 0.14 | Build guards in `navree-auth` |
| Password hashing | Hash (bcrypt, Argon2) | Argon2id by default, rehash on login | [argon2](https://docs.rs/argon2) 0.6.0 (Aug 2026, first stable since Jan 2024; MSRV 1.85); password-hash 0.6.1 | Adopt |
| API tokens | Sanctum | Personal access tokens with abilities, hashed at rest; Sanctum-compatible SPA cookie auth for the Next.js and Nuxt kits | Navree-built; [jsonwebtoken](https://docs.rs/crate/jsonwebtoken/latest) 11.1.0 for JWT mode | Build |
| Two-factor | Fortify 2FA | TOTP with recovery codes | [totp-rs](https://docs.rs/crate/totp-rs/latest) 6.0.0 (Aug 2026) | Adopt |
| Passkeys | Passkeys in Fortify and the starter kits (May 2026) | WebAuthn registration and login | [webauthn-rs](https://docs.rs/webauthn-rs) 0.5.5 (Apr 2026; Kanidm team; MPL-2.0) | Adopt |
| Social login | Socialite | Provider presets (see Stack 3) | oauth2 5.0.0, openidconnect 4.0.1 | Build |
| OAuth2 server | Passport | Issue tokens to third-party apps | [oxide-auth](https://docs.rs/crate/oxide-auth/latest) 0.6.1 is stale (Jun 2024) and its Axum adapter predates Axum 0.8 | Defer; use an external identity provider until Phase 4 |
| Authorization | Gates, policies | `navree-access`: CASL-style abilities, roles and permissions (see Roles, permissions and abilities); `#[policy]` groups rules per entity; `#[can]` and `#[permissions]` guards | Navree-built | Build |
| CSRF | PreventRequestForgery (ValidateCsrfToken in Laravel 11 and 12) | On for session routes | tower-http 0.7 `CsrfLayer` (tokenless, origin-based); [axum\_csrf](https://docs.rs/crate/axum_csrf/latest) 0.11.0 for token mode | Adopt |
| Rate limiting | RateLimiter, throttle | Named limiters per route or user; Redis-backed across nodes | [tower\_governor](https://docs.rs/tower_governor) 0.8.0 (in-memory); Redis limiter built by Navree | Wrap |
| Encryption | Crypt facade | `Encrypter` with key rotation | aes-gcm 0.11.1 (RustCrypto) | Wrap |
| Logging | Log channels | On by default: request, error, query, job and audit logs (see Logging and audit) | tracing 0.1.44, tracing-subscriber 0.3.23, tracing-appender 0.2.5 (rotating files) | Wrap |
| CLI | Artisan | See Developer experience | clap 4.6.7 | Wrap |

### Upgrade watch-list for the base stack

- RustCrypto shipped a wave of breaking major versions in 2026 (argon2 0.6, password-hash 0.6, hmac 0.13, sha2 0.11). Some dependencies still use the older versions, so two copies may be compiled until they catch up.
- jsonwebtoken moved from 10 to 11 in July 2026 and allows at most one built-in crypto backend, or your own provider.
- tower-http 0.7 changed compression negotiation and static-file behaviour; Navree's defaults are written against 0.7.

## Stack 2: Advanced stack

Every advanced Laravel capability can be built on maintained crates, but several key ones are pre-1.0, in release candidate, or only just 1.0 (opendal, OpenTelemetry, apalis). So each Navree module defines its own stable, Laravel-like API and plugs crates in as swappable drivers behind it.

**The driver rule:** application code calls `navree-queue`, `navree-cache`, `navree-storage` and so on, never the crate underneath. If a crate stalls or breaks, Navree swaps the driver without touching apps.

| Capability | Laravel | Navree module and API | Driver crates (version, release) | Notes and risks |
| --- | --- | --- | --- | --- |
| Queues and jobs | Queues: dispatch, delay, retries, backoff, chains, batches, unique jobs, failed-jobs table | `navree-queue`: `#[job]` structs, `dispatch()`, `.delay()`, `.on_queue()`, batches, chains, middleware, failed-job store | [apalis](https://docs.rs/crate/apalis/latest) 1.0.0 (12 Oct 2026, after release candidates since Dec 2025) with Redis, Postgres, SQLite, MySQL and AMQP backends; [graphile\_worker](https://docs.rs/graphile_worker) 0.14.1 (Postgres-only alternative) | apalis 1.0.0 is only weeks old. Navree pins it behind its own API and keeps a native Postgres `SKIP LOCKED` driver as fallback |
| Queue dashboard | Horizon | `navree-queue` dashboard data as JSON endpoints, shown as screens in the Next.js and Nuxt kits | [apalis-board](https://docs.rs/crate/apalis-board/latest) 1.0.0 (Oct 2026): queues, job status, workers, retry and cancel | Its own Leptos UI is not used (API-first); works only with apalis drivers |
| Scheduler | Task scheduler: cron, overlap prevention, run on one server | `navree-schedule`: `schedule.job(X).daily_at("02:00").without_overlapping().on_one_server()` | [croner](https://docs.rs/croner) 4.0.2 (Oct 2026) for cron parsing; own Tokio loop; Postgres advisory lock, or a Redis lock with lease renewal, for single-node runs; each run is recorded in a schedule\_runs table | [tokio-cron-scheduler](https://gittrend.io/repo/mvniekerk/tokio-cron-scheduler) 0.15.1 had no release for \~12 months, so it is not used |
| Cache | Cache: remember, tags, atomic locks, stale-while-revalidate | `navree-cache`: `cache.remember(key, ttl, f)`, tags, locks, `flexible()` | [moka](https://docs.rs/crate/moka/latest) 0.12.16 (in-memory); [redis](https://docs.rs/crate/redis/1.7.0) 1.8.0 (Oct 2026) + [deadpool-redis](https://docs.rs/deadpool-redis) 0.23.1 | [fred](https://docs.rs/crate/fred/9.0.1) is stale (last release Feb 2025); use redis 1.x |
| Events and listeners | Events, listeners, model observers, queued listeners | `navree-events`: typed events, sync and queued listeners, ORM model hooks | tokio broadcast/mpsc channels; queued listeners via `navree-queue` | Built by Navree |
| Mail | Mailables, Markdown mail, SMTP/SES/Postmark/Mailgun/Resend, failover | `navree-mail`: mailables rendered from templates, queued sending, failover transport | [lettre](https://docs.rs/crate/lettre/0.8.1) 0.11.23 (SMTP); [mrml](https://docs.rs/crate/mrml/latest) 6.0.1 (MJML to responsive HTML); [mail-send](https://docs.rs/mail-send) 0.6.2 (DKIM signing) | API providers (SES, Postmark, Mailgun, Resend) are thin HTTP drivers written by Navree |
| Notifications | Mail, database, broadcast, SMS, Slack channels | `navree-notify`: one notification, many channels; database channel with read state | Mail via `navree-mail`; broadcast through the `Broadcaster` contract, which `navree-realtime` implements; SMS/push drivers in Stack 3 | Built by Navree |
| File storage | Storage disks: local, S3, FTP, SFTP; temporary URLs | `navree-storage`: named disks, streams, signed URLs, visibility | [object\_store](https://docs.rs/object_store/latest) 0.14.2 (S3, GCS, Azure, local, memory, HTTP; Apache Arrow PMC); [opendal](https://docs.rs/crate/opendal/0.59.0) 0.59.5 (50+ services incl. SFTP, FTP) | object\_store is default; opendal is optional because it is pre-1.0 with frequent breaking changes (0.58.0 was yanked) |
| Real-time broadcasting | Broadcasting + Reverb (Pusher protocol), presence channels | `navree-realtime`: Pusher-protocol WebSocket server, public/private/presence channels, auth endpoint, Redis fan-out for many nodes | axum `ws` feature on [tokio-tungstenite](https://releasealert.dev/cratesio/tokio-tungstenite) 0.30.0; SSE in axum; Redis pub/sub via redis 1.8.0; [socketioxide](https://github.com/Totodore/socketioxide) for Socket.IO clients | Sockudo 5.2.0 (Rust, Echo-compatible) can run as an external server until Navree's in-process server ships in Phase 3. The Pusher protocol keeps `laravel-echo` usable (see UI stack). [fastwebsockets](https://docs.rs/crate/fastwebsockets/latest) is stale |
| Microservice transports | (none; NestJS microservices) | `navree-transport`: request/reply and event patterns over a broker | [async-nats](https://docs.rs/crate/async-nats/0.50.0) 0.50.0; [lapin](https://docs.rs/crate/lapin/latest) 4.12.0 (RabbitMQ); [rdkafka](https://docs.rs/crate/rdkafka/latest) 0.39.0 (needs C toolchain); Redis Streams via redis | [rskafka](https://docs.rs/rskafka/) is stale and minimal; not used |
| Search | Scout: model indexing, engines | `navree-search`: `#[searchable]` models, sync on save, engines as drivers | [meilisearch-sdk](https://docs.rs/crate/meilisearch-sdk) 0.33.0; [typesense](https://docs.rs/crate/typesense/latest) 0.4.0; [tantivy](https://docs.rs/tantivy) 0.27.0 (embedded); Postgres full-text via the ORM | The official [elasticsearch](https://docs.rs/crate/elasticsearch/latest) client is still labelled alpha |
| Vector search | Laravel 13 vector search (pgvector) | `navree-search` vector queries; `navree-ai` embeddings | [pgvector](https://docs.rs/crate/pgvector/latest) 0.4.2; [qdrant-client](https://docs.rs/qdrant-client) 1.20.0 (Oct 2026) | Good |
| OpenAPI docs | (none first-party) | `navree-api`: OpenAPI 3.1 generated from controllers and DTOs; Swagger UI and Scalar | [utoipa](https://docs.rs/crate/utoipa/latest) 6.0.0 (Sep 2026) + [utoipa-axum](https://docs.rs/utoipa-axum/) 0.3.0, utoipa-swagger-ui 10.0.2, utoipa-scalar 0.4.0 | utoipa 6 is a new major; start on it directly |
| API versioning | (none first-party) | `#[controller("/users", version = "2")]`; URI, header or media-type versioning | Built by Navree | Matches NestJS |
| GraphQL | (none first-party) | `navree-graphql` (optional, Phase 4) | [async-graphql](https://docs.rs/crate/async-graphql/latest) 7.2.1 stable; 8.0.0-rc.5 (Apr 2026, no release since) | Use 7.x; 8.0 appears stalled |
| gRPC and Connect | (none first-party) | `navree-grpc`: services registered as module providers, sharing the same DI | [tonic](https://docs.rs/crate/tonic/latest) 0.14.6 (prost moved to `tonic-prost` in 0.14); [connectrpc](https://buf.build/blog/connect-rust-joins-the-connect-project) 0.9.1 (Connect, gRPC and gRPC-Web on Tower; part of the official Connect project since Aug 2026) | connectrpc is pre-1.0 |
| HTTP client and resilience | `Http` facade with retries, fakes | `navree-http-client`: retries, timeouts, circuit breaker, test fakes | reqwest 0.13.5 (Sep 2026); backon 1.6.0 (retry with backoff); tower 0.5.3 retry, timeout and limit layers | Fakes record and assert outgoing requests, like Http::fake() |
| Observability | Logging; Pulse | `navree-observe`: tracing, metrics, health checks, OpenTelemetry export | tracing + tracing-opentelemetry 0.34.x; opentelemetry, opentelemetry\_sdk and opentelemetry-otlp 0.33.1; metrics 0.24.6 + metrics-exporter-prometheus 0.18.3; sentry 0.49.3 (official; axum, tower and tracing integrations) | All OpenTelemetry crates are 0.x and must share one version; opentelemetry.io lists Rust traces as Beta, metrics and logs as Stable. Health checks are a Navree route (axum-health 0.2.1 is too small to depend on) |
| AI | Laravel AI SDK, Laravel MCP | `navree-ai`: chat, tools, embeddings, structured output, MCP server | rmcp 3.7.0 (official MCP Rust SDK; weekly releases); genai 0.7.0 (multi-provider); rig-core 0.45.0 (agents, RAG); async-openai 0.42.2 (community); embeddings stored via pgvector or qdrant-client | No official OpenAI or Anthropic Rust SDK was found, so Navree writes thin provider drivers on reqwest and treats genai and rig as optional drivers |

## Stack 3: Everything else, and the true gaps

Seven remaining Laravel features have no mature Rust equivalent and must be built by Navree: subscription billing, database-backed feature flags, an admin panel, Telescope/Pulse-style inspection, SMS and push drivers, social-login presets and multi-tenancy. Two more are only partly covered: Dusk-quality browser testing and HTML-to-PDF.

Decision key: **Adopt** = use the crate directly · **Wrap** = Navree module around the crate · **Build** = Navree writes it · **Defer** = after v1.

| Feature | Laravel | Rust options (version, release) | State | Navree decision |
| --- | --- | --- | --- | --- |
| Payments and subscriptions | Cashier (Stripe, Paddle) | [async-stripe](https://docs.rs/crate/async-stripe/latest) 0.41.0 stable; 1.0.0-rc.9 (Sep 2026) is generated weekly from Stripe's OpenAPI spec and split into per-area crates; [paddle-rust-sdk](https://docs.rs/paddle-rust-sdk) 0.21.0 (unofficial); Razorpay crates stale (0.3.x, 2023–24) | Gap: no billing layer | **Build** `navree-billing` on async-stripe 1.0: plans, subscriptions, trials, invoices, webhooks. Other gateways via thin HTTP clients |
| Feature flags | Pennant | [open-feature](https://docs.rs/crate/open-feature/latest) 0.3.0 (pre-1.0); [unleash-api-client](https://docs.rs/unleash-api-client) 0.17.1; [flagsmith](https://docs.rs/crate/flagsmith/3.0.0) 3.1.2; [growthbook-rust](https://docs.rs/growthbook-rust) 0.3.0. All are clients for external services | Gap: nothing stores flags in the app's own DB | **Build** `navree-flags`: DB/Redis-backed flags scoped to user or team, plus an OpenFeature provider so external services can plug in |
| Localization | `__()`, lang files | [rust-i18n](https://docs.rs/rust-i18n/) 4.2.4; [fluent](https://docs.rs/crate/fluent/latest) 0.17.0; [icu](https://releasealert.dev/cratesio/icu) (ICU4X) 2.4.0 for dates, numbers, plurals | Good | **Wrap** in `navree-i18n` with locale middleware |
| Admin panel | Nova, Filament | [SeaORM Pro](https://www.sea-ql.org/sea-orm-pro/docs/introduction/sea-orm-pro) (tied to Loco + Seaography; RBAC in a paid tier); [Cot](https://docs.rs/crate/cot/latest) admin 0.7.0 (Cot only); [adminx](https://docs.rs/crate/adminx/3.1.0) 3.1.2 (first release Jul 2025) | Gap | **Build** `navree-admin` (Phase 4): resources declared in Rust, an admin API plus an admin UI kit in Next.js or Nuxt |
| Queue dashboard | Horizon | [apalis-board](https://docs.rs/apalis-board) 1.0.0 (apalis queues only; Leptos front end) | Partial | **Wrap**: apalis-board's data served as JSON, with the dashboard screens in the Next.js and Nuxt kits (see Stack 2) |
| Request inspector, app metrics | Telescope, Pulse, Debugbar | None found; [axum-debug](https://docs.rs/crate/axum-debug/latest) is deprecated | Gap | **Build** a request inspector and Pulse-like metrics, served as JSON for kit screens, in `navree-observe` (Phase 3) |
| HTTP tests and fakes | HTTP tests, `Mail::fake()`, `Http::fake()` | [axum-test](https://docs.rs/crate/axum-test/21.0.0) 21.1.0; [wiremock](https://docs.rs/crate/wiremock/latest) 0.6.5; [mockall](https://docs.rs/crate/mockall/latest) 0.15.0; [fake](https://docs.rs/crate/fake/latest) 5.1.0; [insta](https://docs.rs/crate/insta/1.1.0) 1.49.0; [proptest](https://docs.rs/crate/proptest/) 1.12.0; [testcontainers](https://docs.rs/testcontainers) 0.28.0; [cargo-nextest](https://nexte.st/changelog) 0.9.149 | Good | **Wrap** in `navree-testing`; every Navree module ships its own fake |
| Browser tests | Dusk | [thirtyfour](https://docs.rs/crate/thirtyfour/0.37.4) 0.38.1; [fantoccini](https://docs.rs/crate/fantoccini/latest) 0.22.1; [chromiumoxide](https://docs.rs/crate/chromiumoxide) 0.9.1; [playwright-rs](https://docs.rs/playwright-rs) 0.20.1 (community, pre-1.0) | Partial | **Adopt** Playwright (TypeScript) in the Next.js and Nuxt kits; thirtyfour helpers for Rust-only teams |
| Dependency security | `composer audit` | [cargo-audit](https://docs.rs/crate/cargo-audit) 0.22.2; [cargo-deny](https://docs.rs/cargo-deny) 0.20.2; [RustSec advisory DB](https://github.com/RustSec/advisory-db); cargo-vet 0.10.x | Good | **Adopt** in every CI template |
| PDF generation | dompdf, Browsershot | [typst-library](https://docs.rs/typst-library) 0.15.1 (Typst markup, not HTML); [printpdf](https://docs.rs/crate/printpdf/0.12.5) 0.12.8 (basic HTML only); headless Chrome via chromiumoxide | Partial | **Wrap**: Typst templates for documents; headless Chrome for HTML-to-PDF |
| Excel and CSV | Laravel Excel (community) | [rust\_xlsxwriter](https://docs.rs/rust_xlsxwriter/) 0.101.0 (write); [calamine](https://docs.rs/calamine/) 0.37.0 (read); [csv](https://docs.rs/crate/csv/latest) 1.4.0 | Good | **Wrap** as `navree-export` helpers |
| Images and QR codes | Intervention Image (community) | [image](https://docs.rs/crate/image/latest) 0.25.10; [fast\_image\_resize](https://docs.rs/fast_image_resize) 6.1.0; [qrcode2](https://docs.rs/crate/qrcode2/latest) 0.18.0 (active fork of qrcode) | Good | **Adopt** |
| SMS and push channels | Vonage, Slack channels; community FCM/APNs | [twilio](https://docs.rs/twilio) 1.1.0 (community, last release Mar 2024; no official SDK); [a2](https://docs.rs/crate/a2/latest) 0.10.0 (APNs); FCM crates fragmented and stale | Gap | **Build** thin HTTP drivers in `navree-notify` (Twilio, Vonage, FCM HTTP v1, APNs, Slack webhook) |
| Social login | Socialite | oauth2 5.0.0 and openidconnect 4.0.1 (generic flows, no provider presets); early bundles socio 0.1.5, authkestra 0.3.4, rustauth 0.3.0, rs-auth 0.1.2. None mature; none confirmed Apple Sign-In | Gap | **Build** provider presets on oauth2 / openidconnect |
| Multi-tenancy | Community packages | tenaxum 0.2.0 (axum + sqlx + Postgres RLS; author calls it a second draft); corteq 0.1.0 (Actix only); rok-tenant 0.3.9. All early | Gap | **Build** tenant resolver + Postgres row-level security per transaction |
| File uploads | UploadedFile | axum 0.8.9 multipart feature, built on multer 3.1.0 | Good | Adopt; navree-http adds size limits, MIME checks and streaming straight to navree-storage |
| IDs | Str::uuid(), Str::ulid() | uuid 1.29.0 (v7 is a stable feature); ulid 3.0.0 (Jul 2026, jumped 2.0 to 3.0 in a week) | Good | Adopt; UUID v7 is the default primary key type |
| Builds and deployment | Sail, Forge, Vapor | cargo-chef 0.1.78 (Docker dependency caching); dist (cargo-dist) 0.33.1 (release installers); cross 0.2.5 (last crates.io release Feb 2023); Shuttle platform shut down in early 2026 | Partial | Adopt cargo-chef in Navree's Dockerfile template; do not target Shuttle |

### Things Rust cannot reproduce exactly

- **Runtime magic.** Eloquent's magic attributes, facades resolved by string name, and `__call`-style dynamic methods have no Rust equivalent. Navree uses generated, typed code instead; it is more verbose but checked by the compiler.
- **Edit-and-refresh.** PHP reloads every request; Rust recompiles. The Next.js or Nuxt front end hot-reloads, but Rust code changes need a rebuild (see Developer experience).
- **Tinker-style REPL.** Covered in Developer experience; there is no production-grade Rust REPL, so Navree offers a scripted console instead.

## Stack 4: UI stack (Next.js or Nuxt on Navree's API)

Navree is API-first: its built-in API drives the whole system, and the UI is a separate Next.js (React) or Nuxt (Vue) app created with each framework's default setup. Navree ships no HTML UI layer of its own: no server-rendered pages, HTMX, Inertia or Rust UI frameworks.

&#91;embedded content: UI request flow · Next.js or Nuxt on Navree's API\]

The browser loads pages from the Next.js or Nuxt server, and both of them call Navree's API; mobile apps and partners use the same API with tokens, and live updates arrive over Navree's real-time server.

### The UI options

| Option | Framework (latest) | Laravel equivalent | How it talks to Navree | Kit | Phase |
| --- | --- | --- | --- | --- | --- |
| Next.js app (default) | [Next.js 16.4](https://nextjs.org/blog/next-16-4.md) (6 Oct 2026; use the patched 16.4.x from the 14 Oct security update), React | Laravel's API-only setup with a separate Next.js front end (the former Breeze Next.js stack; Breeze stopped receiving updates with Laravel 12) | Typed client generated from Navree's OpenAPI document; session cookie auth for the first-party app; Echo React hooks for real-time (from Phase 2) | `navree new --ui=next` | 1 |
| Nuxt app | [Nuxt 4.6.2](https://releases.sh/nuxt/overview) (16 Oct 2026), Vue; Nuxt 3 is end-of-life | Laravel's API-only setup with a separate SPA (Breeze's docs named Nuxt as an example client) | Same typed client; [nuxt-auth-sanctum](https://nuxt.com/modules/nuxt-auth-sanctum) 3.1.5 works because Navree speaks Sanctum's SPA cookie protocol; Echo Vue hooks | `navree new --ui=nuxt` | 2 |
| API only | Any client: mobile apps, partner integrations, other front ends | Sanctum token API | OpenAPI 3.1 document; personal access tokens | `navree new --ui=none` | 1 |

### Decisions

- **Default framework setups only.** Kits start from `create-next-app` (App Router, TypeScript) and the standard Nuxt 4 starter, plus a short fixed list of standard packages: the Navree package, laravel-echo with pusher-js, CASL and Playwright. No custom bundler or build pipeline.
- **The API is the contract.** Every endpoint is described in the OpenAPI 3.1 document that `navree-api` generates with utoipa 6.0.0. `navree client:generate --ts` regenerates a typed TypeScript client in the front end whenever the API changes, using an established OpenAPI generator chosen in Phase 1.
- **Speak Laravel's front-end protocols.** Navree implements Sanctum-style SPA auth (a CSRF-cookie endpoint, the `XSRF-TOKEN` cookie and `X-XSRF-TOKEN` header, a session cookie; requests send Accept: application/json and an Origin or Referer header) and the Pusher protocol for real-time. The X-XSRF-TOKEN header is checked against the token stored in the session, not against the cookie, failures return 419, and allowed front-end origins are listed explicitly. Existing packages therefore work unchanged: nuxt-auth-sanctum, `laravel-echo`, [`@laravel/echo-react`](https://www.jsdelivr.com/package/npm/@laravel/echo-react) 2.6.1 and `@laravel/echo-vue`.
- **Server-side rendering through the API.** Next.js server components and route handlers, and Nuxt server routes, call Navree from the server and forward the user's session cookie (the backend-for-frontend pattern). The browser calls Navree directly on the same site, for example `app.example.com` and `api.example.com`, so cookies stay first-party. Server-side calls never receive Set-Cookie (sessions are extended on the server), sessions rotate only in route handlers that relay response headers, and the front-end server is a trusted proxy that forwards the user's IP address and request ID.
- **Templates only where no browser is involved.** Navree keeps minijinja for email templates and renders no web pages; PDFs come from Typst templates or headless Chrome.
- **Not offered:** server-rendered HTML pages, HTMX, Inertia, Livewire or Volt-style components, Folio, and Rust UI frameworks (Leptos, Dioxus, Yew).

### What each kit ships

| Area | Next.js kit | Nuxt kit |
| --- | --- | --- |
| Auth screens | Login, registration, password reset, email verification, two-factor, passkeys, profile | Same |
| Teams | Team switcher and invitations (Phase 2) | Same (Phase 2) |
| API access | Generated typed client plus `@navree/next`: session-aware fetch in server components, route protection in proxy.ts (Next.js 16's renamed middleware) | Generated typed client plus `@navree/nuxt` module (nuxt-auth-sanctum also works) |
| Real-time | `laravel-echo` with `@laravel/echo-react` (from Phase 2) | `laravel-echo` with `@laravel/echo-vue` (from Phase 2) |
| Styling | Tailwind CSS, as offered by `create-next-app` | Tailwind CSS through its Nuxt module |
| Localization | Navree returns translated API messages per `Accept-Language`; UI strings use the team's front-end i18n library | Same |
| Tests | Playwright end-to-end tests against a seeded Navree API | Same |
| Deploy | Docker Compose with the Navree binary and the Next.js server, or any Node host | Same, with the Nuxt server |
| Permissions in the UI | @casl/ability and @casl/react (\<Can>, useAbility), loaded from /api/me/abilities | @casl/ability and @casl/vue (\<Can>, useAbility), loaded from /api/me/abilities |
| Admin screens | Roles and permissions, activity log (Phase 2) | Same (Phase 2) |

### Real-time

Navree's `RealtimeModule` speaks the Pusher protocol, as Laravel Reverb does. Until Navree's in-process server ships in Phase 3, Sockudo 5.2.0, a Rust server that documents Laravel Echo compatibility, can serve it. Both kits connect with `laravel-echo` and `pusher-js`; private and presence channels authorise through Navree's `/broadcasting/auth` endpoint.

## Developer experience

Navree's dev loop is a fast restart, not PHP's edit-and-refresh: a watcher rebuilds on save, the listening socket stays open across restarts, and email templates and the Next.js or Nuxt front end reload without any Rust rebuild. Compile time is the biggest DX risk, so it gets its own budget and CI benchmark.

### The `navree` CLI (Artisan equivalent)

Two binaries share one command surface. The installed `navree` tool creates projects, runs generators and drives the dev loop. The app binary itself carries the runtime commands, and `navree` forwards those to it, so production servers only need the app binary.

| Command | Laravel equivalent | What it does |
| --- | --- | --- |
| `navree new <app> --ui=<kit>` | `laravel new` | Creates an app with a chosen UI kit, database and queue driver |
| `navree dev` | `artisan serve` + `npm run dev` | Watches, rebuilds, restarts; starts the Next.js or Nuxt dev server; keeps the port open |
| `navree make:module <name> [--crud]` | `make:model -mcr` and friends | Generates a feature module skeleton |
| `navree make:<thing>` (controller, service, entity, migration, job, event, listener, mail, notification, policy, command, test) | `make:*` | Generates code inside the right module |
| `navree migrate`, `migrate:rollback`, `migrate:fresh --seed`, `db:seed` | Same | Runs each module's migrations in dependency order |
| `navree queue:work`, `queue:failed`, `queue:retry` | Same | Runs and manages workers |
| `navree schedule:run`, `schedule:work`, `schedule:list` | Same | Runs one scheduler tick for system cron; schedule:work runs it as a long-lived process |
| `navree realtime:serve` | `reverb:start` | Runs Navree's in-process Pusher-protocol server as its own process role (Phase 3); until then the kits run Sockudo |
| `navree routes` | `route:list` | Lists routes with their module, guards and layers |
| `navree check [--graph]` | (none) | Validates the module graph without starting servers |
| `navree task <name>` | `tinker` (partly) | Runs a one-off Rust task with the full app booted |
| `navree console` | `tinker` | Experimental REPL on evcxr with the app booted (Phase 4) |
| `navree module:extract <name>` | (none) | Moves a module into its own crate |
| `navree openapi:export`, `navree client:generate --ts` | (none) | Exports API contracts and typed clients |
| `navree build --release`, `navree docker` | (none) | Release binary; Docker image using cargo-chef layer caching |
| `navree <module>:<command>` | Package commands | Commands contributed by modules, e.g. `navree billing:sync-plans` |

Generators live only in the installed navree tool, never in the app binary, so production binaries stay small.

### The dev loop

1. `navree dev` watches the project and rebuilds on save, using the same approach as [bacon](https://tracker.debian.org/pkg/bacon) 3.26.0 and [watchexec](https://docs.rs/watchexec-cli/) 2.8.0. cargo-watch is not used: its README says it [will not receive further updates](https://docs.rs/crate/cargo-watch/latest) and recommends bacon or watchexec.
2. The listening socket is held open across restarts, the [systemfd](https://github.com/mitsuhiko/systemfd) + [listenfd](https://docs.rs/crate/listenfd/latest) 1.0.2 pattern, so the browser waits instead of showing "connection refused".
3. The Next.js or Nuxt dev server hot-reloads the UI, and every API change regenerates the typed TypeScript client automatically. Email templates reload without a rebuild.
4. Rust hot-patching is not part of the dev loop: Dioxus Subsecond, the most advanced option, needs its own `dx` tool and cannot migrate struct layouts. The DI container keeps no global statics, so hot-patching can be evaluated later without a redesign.

### Compile-time budget

Target: an incremental rebuild of the starter app in under 5 seconds after a one-file change (see success metrics).

- **Linker:** lld has been the default linker on x86\_64 Linux since [Rust 1.90](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable) (Sep 2025); the Rust team reported about 7x faster linking on incremental rebuilds. [mold](https://phoronix.com/news/Mold-3.0-Released) 3.0.0 and [wild](https://phoronix.com/news/Wild-Linker-0.10) 0.10 stay optional.
- **Features:** each module is a Cargo feature, so unused modules are never compiled.
- **Crates:** large apps split feature modules into their own crates so Cargo builds them in parallel and incrementally.
- **Macros:** Navree's proc macros stay thin, and the container is type-erased in every profile, so there is one implementation and little generic code.
- **CI:** [sccache](https://packages-cdn.gentoo.org/packages/dev-util/sccache/changelog) 0.18.0 for shared caching, cargo-chef for Docker layers, [cargo-nextest](https://nexte.st/) 0.9.149 for tests (doctests run separately with `cargo test --doc`).
- **Nightly opt-ins, documented but not default:** the [Cranelift](https://awesome.ecosyste.ms/projects/github.com%2Frust-lang%2Frustc_codegen_cranelift) backend for dev builds only (nightly; it cannot unwind, and release builds must use panic = "unwind") and the [parallel front-end](https://goals.rust-lang.org/2026/parallel-front-end.html) (a 2026 project goal; stabilization not started).

### Console (Tinker)

There is no production-grade Rust REPL for a running app. [evcxr](https://docs.rs/crate/evcxr_jupyter) 0.22.0 (Aug 2026) is an active REPL and Jupyter kernel, but it compiles each input. Navree therefore ships `navree task` for one-off code with the full container booted (Loco's precedent), and an experimental `navree console` built on evcxr in Phase 4.

### Conventions and documentation

- One obvious way to do each thing: fixed folder layout per module, naming rules, config keys.
- Boot errors name the module and the missing type; macro errors point at the attribute that caused them.
- A documentation site organised like Laravel's, one guide per module, plus a README and runnable example per module crate.
- An upgrade guide for every minor release while Navree is pre-1.0.

## Non-functional requirements

Navree must be secure by default, survive restarts and deploys without dropping work, and hit the performance targets in the success metrics on commodity hardware. Each requirement below is enforced by a test or CI check planned in Phase 0.

### Performance

| ID | Requirement | Verified by |
| --- | --- | --- |
| P1 | Starter app idle memory under 50 MB RSS | Container metrics in CI |
| P2 | p99 under 20 ms for a DB-backed JSON endpoint at 1,000 requests/s on a 4 vCPU instance | Nightly load test |
| P3 | No blocking calls on Tokio worker threads; CPU-heavy work goes to `spawn_blocking` or the queue | Review rules plus a dev-build detector on Tokio's unstable task poll-time hooks; Argon2 runs in spawn\_blocking behind a semaphore |
| P4 | Database pool, Redis pool and HTTP client are shared singletons; no per-request connection setup | Module tests |
| P5 | Responses stream (files, SSE, large JSON) without buffering whole bodies | Integration tests |

### Security (secure by default)

| ID | Requirement | Owner module |
| --- | --- | --- |
| S1 | Passwords hashed with Argon2id; parameters configurable | `navree-auth` |
| S2 | Session cookies `HttpOnly`, `Secure`, `SameSite=Lax`; signed or encrypted | `navree-session` |
| S3 | CSRF protection on for all session-authenticated, state-changing routes | `navree-session` |
| S4 | Security headers on by default: HSTS, `X-Content-Type-Options`, frame options, a starter CSP | `navree-http` |
| S5 | Rate limiting on login, registration, password reset and API routes | `navree-auth`, `navree-http` |
| S6 | Request body size limits and upload MIME checks | `navree-http` |
| S7 | Mass assignment impossible by construction: input goes through typed DTOs, never straight into entities | `navree-validation`, `navree-db` |
| S8 | SQL only through parameterised queries; raw SQL requires an explicit `raw` call | `navree-db` |
| S9 | Secrets never logged; config marks secret fields and redacts them | `navree-config`, `navree-log` |
| S10 | `cargo audit` and `cargo deny` gate every release; zero known-vulnerable dependencies | CI template |
| S11 | No `unsafe` in Navree crates without a documented justification and review | Lint |
| S12 | Session ID rotates on login and on privilege change | navree-session |
| S13 | CORS allows only the configured Next.js or Nuxt origins, with credentials and explicitly listed request and exposed headers (no wildcards); the session cookie is scoped to the shared parent domain | navree-http, navree-session |
| S14 | WebSocket connections check Origin; private and presence channels require authorization | navree-realtime |
| S15 | Incoming webhooks verify signatures; a route is exempt from CSRF only by explicit opt-in | navree-http |
| S16 | Outgoing HTTP from the HTTP client and AI tools blocks private and cloud-metadata IP ranges by default (SSRF policy) | navree-http-client, navree-ai |
| S17 | The MCP server endpoint requires authentication and per-tool permissions | navree-ai |
| S18 | Deny by default: navree check fails when a route has no #\[public\], #\[roles\], #\[permissions\], #\[can\] or explicit guard | navree-access, navree-cli |
| S19 | Denied authorization, sign-in events and every role or permission change are written to the audit trail | navree-audit |

### Reliability

| ID | Requirement |
| --- | --- |
| R1 | Graceful shutdown on SIGTERM: stop accepting, finish in-flight requests and jobs, run `on_shutdown` hooks, within a configurable timeout. WebSocket clients get a reconnect close code; WebSocket and SSE connections are tracked by a TaskTracker so shutdown waits for them; modules stop in reverse dependency order |
| R2 | Liveness reports only that the process is serving; readiness aggregates per-module checks (database, Redis, queue, storage), so an outage takes instances out of rotation instead of restarting them |
| R3 | Jobs are at-least-once; Navree offers idempotency keys and unique-job locks to make handlers safe to retry; jobs dispatched inside a transaction go through an outbox, so they are never lost; the relay claims outbox rows with FOR UPDATE SKIP LOCKED, so two relays never send the same row |
| R4 | Failed jobs are stored with error, attempts and payload, and can be retried from CLI or dashboard |
| R5 | Scheduler runs each task on one node only, using a Postgres advisory lock (or a Redis lock with lease renewal), with overlap prevention; each run is recorded in a schedule\_runs table keyed by task and tick (INSERT … ON CONFLICT DO NOTHING), so no tick runs twice |
| R6 | Each module has its own migrator; migrators run in dependency order, and each migration runs inside a transaction where the database allows unless it sets a no\_transaction flag (for example for CREATE INDEX CONCURRENTLY) |
| R7 | Panics in handlers and Navree-spawned tasks become 500 responses or failed jobs and are reported; release builds must use panic = "unwind", which navree-kernel enforces at compile time with #\[cfg(not(panic = "unwind"))\] compile\_error!(…) |

### Compatibility

| Area | Requirement |
| --- | --- |
| Rust toolchain | Rust 2024 edition (stable since 1.85). MSRV is a Rust release at least 6 months old, matching Tokio's policy; today that is 1.95. Latest stable is 1.99.1 (15 Oct 2026). |
| Tokio | Latest 1.x in development; CI also tests the 1.51 LTS line |
| Operating systems | Production: Linux x86\_64 and aarch64, glibc and static musl builds. Development: Linux, macOS, Windows |
| Databases | PostgreSQL (primary, gets every feature first), MySQL/MariaDB, SQLite (development and tests). SQL Server is out of scope: SQLx dropped its MSSQL driver in 0.7 and SeaORM supports it only in the commercial SeaORM X |
| Browsers (UI kits) | Last two versions of evergreen browsers |
| Semver | Navree 0.x may break on minor releases with an upgrade guide; from 1.0, breaking changes only in majors |

## Laravel parity matrix

66 of the 70 Laravel features below (94%) are in scope for v1.0. The 4 deferred to Phase 4 are the OAuth2 server (Passport), an admin panel, GraphQL and the Tinker console. Each row is one feature; rows marked (package) are common community packages rather than first-party Laravel. The target is 66 rows (94%); the v1.0 gate floor is 63 rows (90%), so at most three Phase 3 rows may slip to a 1.x release. The Status column tracks delivery against that gate.

Approach: **Adopt** = use a crate directly · **Wrap** = Navree module around a crate · **Build** = Navree writes it · **Defer** = after v1.0. Rust maturity = how ready the Rust ecosystem is for this feature today. Phases follow module dependencies: a row never depends on a module from a later phase.

| # | Laravel feature | Navree module | Main crates | Approach | Rust maturity | Phase | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Routing and controllers | `navree-http` | axum 0.8.9 | Wrap | High | 0 | Not started |
| 2 | Named routes, URL generation, signed URLs | `navree-http` | Navree-built | Build | Medium | 0 | Not started |
| 3 | Middleware | `navree-http` | tower 0.5.3, tower-http 0.7.1 | Wrap | High | 0 | Not started |
| 4 | Service container and service providers | `navree-kernel` | Navree-built | Build | Low | 0 | Not started |
| 5 | Configuration and .env | `navree-config` | toml 1.1.8, serde_json 1.0.154 | Build | High | 0 | Not started |
| 6 | Logging (request and error logs on by default; query and job logs arrive with their modules) | `navree-log` | tracing 0.1.44, tracing-appender 0.2.5 | Wrap | High | 0 | Not started |
| 7 | Exception handling (problem+json errors) | `navree-http` | thiserror 2.0.21 | Build | High | 0 | Not started |
| 8 | Artisan console and generators | `navree-cli` | clap 4.6.7 | Build | High | 0 | Not started |
| 9 | CORS and security headers | `navree-http` | tower-http 0.7.1 | Adopt | High | 0 | Not started |
| 10 | Maintenance mode | `navree-http` | Navree-built | Build | High | 2 | Not started |
| 11 | Validation and form requests | `navree-validation` | garde 0.23.0 | Wrap | Medium | 1 | Not started |
| 12 | Validation errors as 422 JSON for Next.js and Nuxt forms (replaces redirect-back, old input and flash) | `navree-validation` | Navree-built | Build | Medium | 1 | Not started |
| 13 | Eloquent ORM | `navree-db` | SeaORM 2.0.4 | Wrap | High | 1 | Not started |
| 14 | Migrations | `navree-db` | sea-orm-migration | Wrap | High | 1 | Not started |
| 15 | Seeders and factories | `navree-db` | fake 5.1.0 | Wrap | High | 1 | Not started |
| 16 | Multiple connections and read replicas | `navree-db` | SeaORM 2.0.4, SQLx 0.9.0 | Wrap | Medium | 2 | Not started |
| 17 | Pagination | `navree-db` | Navree-built | Build | Medium | 1 | Not started |
| 18 | API resources | `navree-api` | serde 1.0.229 | Wrap | High | 1 | Not started |
| 19 | Sessions | `navree-session` | tower-sessions 0.15.0 | Build | Medium | 1 | Not started |
| 20 | Encrypted cookies | `navree-session` | axum-extra 0.12.6 | Adopt | High | 1 | Not started |
| 21 | Request forgery (CSRF) protection | `navree-session` | tower-http 0.7.1, axum\_csrf 0.11.0 | Adopt | High | 1 | Not started |
| 22 | Login, registration, password reset, email verification | `navree-auth` | argon2 0.6.0 | Build | Medium | 1 | Not started |
| 23 | API tokens (Sanctum) | `navree-auth` | jsonwebtoken 11.1.0 | Build | Medium | 1 | Not started |
| 24 | Two-factor authentication (Fortify) | `navree-auth` | totp-rs 6.0.0 | Adopt | Medium | 1 | Not started |
| 25 | Passkeys (Fortify, starter kits) | `navree-auth` | webauthn-rs 0.5.5 | Adopt | Medium | 1 | Not started |
| 26 | Gates and policies (on the same ability engine) | `navree-access` | Navree-built | Build | Medium | 1 | Not started |
| 27 | Rate limiting | `navree-http` | tower\_governor 0.8.0 + Redis | Wrap | Medium | 1 | Not started |
| 28 | Encryption and hashing | `navree-crypto` | aes-gcm 0.11.1, argon2 0.6.0 | Wrap | High | 1 | Not started |
| 29 | Cache and atomic locks | `navree-cache` | moka 0.12.16, redis 1.8.0 | Wrap | High | 1 | Not started |
| 30 | Redis connections | `navree-redis` | redis 1.8.0, deadpool-redis 0.23.1 | Wrap | High | 1 | Not started |
| 31 | Events, listeners, model observers | `navree-events` | tokio channels | Build | High | 1 | Not started |
| 32 | Queues and jobs | `navree-queue` | apalis 1.0.0 | Wrap | Medium | 1 | Not started |
| 33 | Mail | `navree-mail` | lettre 0.11.23, mrml 6.0.1 | Wrap | High | 1 | Not started |
| 34 | HTTP client | `navree-http-client` | reqwest 0.13.5, backon 1.6.0 | Wrap | High | 1 | Not started |
| 35 | HTTP tests and fakes | `navree-testing` | axum-test 21.1.0, wiremock 0.6.5 | Wrap | High | 1 | Not started |
| 36 | Blade templates (email templates only) | `navree-view` | minijinja 3.0.3 | Wrap | High | 1 | Not started |
| 37 | API-only starter kit (Breeze API stack) | `navree-cli` | Navree-built | Build | Medium | 1 | Not started |
| 38 | Next.js starter kit with auth screens (Breeze Next.js front end) | navree-cli, @navree/next | Next.js 16.4, generated TypeScript client | Build | Medium | 1 | Not started |
| 39 | Localization | `navree-i18n` | rust-i18n 4.2.4, icu 2.4.0 | Wrap | High | 2 | Not started |
| 40 | Sail and Docker deployment | `navree-cli` | cargo-chef 0.1.78 | Adopt | Medium | 1 | Not started |
| 41 | Roles and permissions with CASL-style abilities (spatie/laravel-permission package) | `navree-access` | Navree-built, CASL rule format | Build | Low | 1 | Not started |
| 42 | Audit and activity log (spatie/laravel-activitylog package) | `navree-audit` | Navree-built on tracing and the ORM | Build | Medium | 1 | Not started |
| 43 | Horizon (queue dashboard) | `navree-queue` | apalis-board 1.0.0 | Adopt | Medium | 2 | Not started |
| 44 | Task scheduler | `navree-schedule` | croner 4.0.2 | Build | Medium | 2 | Not started |
| 45 | File storage | `navree-storage` | object\_store 0.14.2 | Wrap | High | 2 | Not started |
| 46 | File uploads | `navree-http` | axum multipart, multer 3.1.0 | Adopt | High | 2 | Not started |
| 47 | Notifications: mail, database, broadcast | `navree-notify` | Navree-built | Build | Medium | 2 | Not started |
| 48 | Broadcasting and Reverb | `navree-realtime` | Sockudo 5.2.0, axum ws | Wrap | Medium | 2 | Not started |
| 49 | Scout full-text search | `navree-search` | meilisearch-sdk 0.33.0, tantivy 0.27.0 | Wrap | Medium | 2 | Not started |
| 50 | Vector search | `navree-search` | pgvector 0.4.2, qdrant-client 1.20.0 | Wrap | High | 3 | Not started |
| 51 | Nuxt starter kit with auth screens | `navree-cli, @navree/nuxt` | Nuxt 4.6.2, generated TypeScript client | Build | Medium | 2 | Not started |
| 52 | Teams | `navree-auth` | Navree-built | Build | Medium | 2 | Not started |
| 53 | Context (request-scoped log context) | `navree-log` | tracing spans | Wrap | High | 0 | Not started |
| 54 | Concurrency and Process helpers | `navree` facade | tokio JoinSet, tokio::process | Wrap | High | 2 | Not started |
| 55 | Health checks, metrics, trace export | `navree-observe` | opentelemetry 0.33.1, metrics 0.24.6 | Wrap | Medium | 3 | Not started |
| 56 | Pulse and Telescope | `navree-observe` | None mature | Build | Low | 3 | Not started |
| 57 | AI SDK and MCP | `navree-ai` | rmcp 3.7.0, genai 0.7.0 | Wrap | Medium | 3 | Not started |
| 58 | Feature flags (Pennant) | `navree-flags` | open-feature 0.3.0 | Build | Low | 3 | Not started |
| 59 | Subscription billing (Cashier) | `navree-billing` | async-stripe 1.0.0-rc.9 | Build | Low | 3 | Not started |
| 60 | Social login (Socialite) | `navree-auth` | oauth2 5.0.0, openidconnect 4.0.1 | Build | Low | 3 | Not started |
| 61 | SMS and push notification channels | `navree-notify` | twilio 1.1.0, a2 0.10.0, own drivers | Build | Low | 3 | Not started |
| 62 | Multi-tenancy (package) | `navree-db` | None mature | Build | Low | 3 | Not started |
| 63 | PDF and Excel exports (packages) | `navree-export` | typst-library 0.15.1, rust\_xlsxwriter 0.101.0, calamine 0.37.0 | Wrap | Medium | 3 | Not started |
| 64 | Image processing (package) | `navree-export` | image 0.25.10, fast\_image\_resize 6.1.0 | Adopt | High | 3 | Not started |
| 65 | Browser tests (Dusk) | `navree-testing` | Playwright (TypeScript), thirtyfour 0.38.1 | Adopt | Medium | 1 | Not started |
| 66 | Precognition (live validation) | `navree-validation` | Navree-built | Build | Medium | 3 | Not started |
| 67 | OAuth2 server (Passport) | `navree-auth` | None maintained | Defer | Low | 4 | Deferred |
| 68 | Admin panel (Nova) | `navree-admin` | None mature | Defer | Low | 4 | Deferred |
| 69 | GraphQL (Lighthouse package) | `navree-graphql` | async-graphql 7.2.1 | Defer | Medium | 4 | Deferred |
| 70 | Tinker interactive console | `navree-cli` | evcxr 0.22.0 | Defer | Low | 4 | Deferred |

Not tracked as rows: Livewire, Volt, Folio and Blade-rendered web pages (the UI is Next.js or Nuxt), Collections and `Str` helpers (Rust's iterators and standard library cover them), serverless deployment (Vapor), hosted platforms (Forge, Laravel Cloud), Prompts (Rust CLI crates such as dialoguer and inquire cover it), Pint (rustfmt and Clippy), Envoy (teams keep their deploy tooling), Octane (Navree is always a long-running server), Vite (each front-end framework brings its own bundler) and MongoDB (out of scope for v1).

### Beyond Laravel: Navree extras

| Extra | Navree module | Main crates | Phase |
| --- | --- | --- | --- |
| Module graph check and module extraction | `navree-cli` | Navree-built | 0 |
| OpenAPI documents and API versioning | `navree-api` | utoipa 6.0.0 | 1 |
| gRPC and Connect services | `navree-grpc` | tonic 0.14.6, connectrpc 0.9.1 | 3 |
| Microservice transports | `navree-transport` | async-nats 0.50.0, lapin 4.12.0, rdkafka 0.39.0 | 3 |
| Front-end packages for Next.js and Nuxt (npm) | `@navree/next, @navree/nuxt` | Navree-built; typed client runtime, auth helpers, Echo setup | 1 (Next.js), 2 (Nuxt) |
| Typed TypeScript client generation | navree-cli | Established OpenAPI TypeScript generator, chosen in Phase 1 | 1 |
| In-process Pusher-protocol real-time server (replaces the Sockudo sidecar) | `navree-realtime` | axum ws, tokio-tungstenite 0.30.0, redis 1.8.0 pub/sub | 3 |
| CQRS module: command, query and event buses (as in @nestjs/cqrs) | `navree-cqrs` | Navree-built | 4 |

## Roadmap, team and effort

Navree can reach a v1.0 with 94% Laravel parity targeted (90% is the release floor) in roughly 12 months with five engineers; the admin panel, an OAuth2 server, GraphQL and an interactive console follow after v1. All durations are estimates to be re-planned at each gate.

&#91;embedded content: Navree roadmap · 5 phases, 4 gates (estimates)\]

Each phase ends at a gate: a demo module with rebuilds under 5 s (Phase 0); a pilot app with auth, CRUD and queued mail on the Next.js kit (Phase 1); a v0.5 public beta at 70% parity (Phase 2); and v1.0 at 90% parity or more, with no known vulnerable dependencies and two production apps (Phase 3).

### Team (estimate)

| Role | Count | Focus |
| --- | --- | --- |
| Core Rust engineers | 3 | Kernel, macros, database, auth, queue, real-time, then the Phase 3 modules |
| Front-end / UI kit engineer | 1 | Next.js and Nuxt kits, the @navree/next and @navree/nuxt packages, starter screens, later the admin UI |
| DX and docs engineer | 1 | CLI, generators, docs site, examples, upgrade guides |
| Security reviewer | Part time | Threat model, Phase 3 audit, release sign-off |

That is about 60 engineer-months to v1.0, plus part-time security review. The estimate is low-confidence until Phase 0 measures real velocity; if it slips, Phase 3 rows move to v1.1 rather than cutting the base stack.

## Risks, mitigations and open questions

The two largest risks are dependency churn (many key crates are pre-1.0 or in release candidate) and compile times; both are mitigated by design choices already in this PRD rather than by hoping they improve.

### Risks

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| Young or fast-moving crates break or stall (apalis 1.0.0 only weeks old, opendal 0.x, utoipa's new major 6.0, async-stripe 1.0 RC, OpenTelemetry 0.x, connectrpc 0.9, rmcp weekly releases) | High | Medium | The driver rule: apps call Navree APIs only; versions pinned; weekly dependency updates; contract tests per driver; native fallback drivers for queue and storage |
| Compile times make the dev loop feel slow | High | High | Compile-time budget with a CI gate; modules as Cargo features; thin macros; crate splitting; lld default linker |
| Macro-generated modules give confusing errors | Medium | High | Errors point at the attribute; `navree check` explains wiring failures; compile-fail test suite for every macro |
| Scope creep: Laravel's ecosystem is very large | High | High | The parity matrix is the contract for v1; the admin panel, OAuth2 server, GraphQL and interactive console are Phase 4 |
| Single-maintainer crates disappear (apalis, Sockudo, smaller auth and session crates) | Medium | Medium | Same driver rule; Navree owns its Sanctum-compatible auth and the @navree front-end packages; fork-and-maintain policy for anything on the critical path |
| Security flaws in auth, sessions or crypto | Medium | High | RustCrypto and other widely used crates only; threat model in Phase 0; external audit in Phase 3; `cargo audit` and `cargo deny` gates |
| Async pitfalls: blocking the runtime, cancelled futures leaving partial work | Medium | Medium | Blocking detector in dev builds; transactional jobs; guides on cancellation safety |
| Hiring and onboarding Rust developers | Medium | High | Generators and conventions; the Next.js and Nuxt kits let front-end developers work in tools they already know |
| Licence conflicts in dependencies | Low | Medium | `cargo deny` licence allow-list; avoid GPL crates such as soketi-rs and paid tiers such as SeaORM Pro's RBAC |
| Long-term maintenance cost after v1 | Medium | High | Small kernel, module ownership per engineer, documented LTS policy from 1.0 |
| Two deployables (the Navree API and the Next.js or Nuxt server) add operations and auth complexity | Medium | Medium | Kits ship Docker Compose, same-site cookie setup and a backend-for-frontend guide; token mode for every other client |
| Navree's CASL-style rules and @casl/ability in the browser disagree on a decision | Medium | High | Limited operator set; CI contract tests run the same rules through Navree and @casl/ability on every release; the API always re-checks |

### Open questions

- [ ] Which two pilot apps will be built on Navree before v1.0? The v1.0 gate needs two in production, and their needs set module priority.
- [ ] Confirm PostgreSQL as the primary database (proposed). Must MySQL/MariaDB be fully supported at launch?
- [ ] Confirm Next.js as the first UI kit in Phase 1 with Nuxt in Phase 2 (proposed), or should both ship together?
- [ ] Which OpenAPI TypeScript generator should navree client:generate use, for example openapi-typescript with openapi-fetch, Orval or Hey API? To be chosen in Phase 1 after a trial against the starter app's API.
- [ ] Will Navree be open source or internal only? This changes documentation, governance and licence choices.
- [ ] Confirm Stripe first for billing (proposed). Which other gateways, such as Razorpay or Paddle, are required, and when?
- [ ] Is multi-tenancy needed before Phase 3, where it currently sits?
- [ ] Is "Navree" acceptable as the name after a crate-name and trademark check?

## Appendix: sources

Versions and dates were checked on 25 October 2026, mainly against docs.rs release lists, official project sites and GitHub. Where docs.rs showed only a page-build date, the release date may be slightly earlier. Re-verify versions at each phase gate.

**Foundation and HTTP**

- [Tokio README: components, LTS and MSRV policy](https://github.com/tokio-rs/tokio)
- [axum on docs.rs](https://docs.rs/crate/axum/) · [Router](https://docs.rs/axum/latest/axum/struct.Router.html) · [State](https://docs.rs/axum/latest/axum/extract/struct.State.html) · [middleware](https://docs.rs/axum/latest/axum/middleware/index.html)
- [hyper HTTP/3 tracking issue](https://github.com/hyperium/hyper/issues/1818) · [h3](https://docs.rs/crate/h3/latest)

**Base stack**

- [tower-sessions](https://docs.rs/crate/tower-sessions/latest) · [tower-sessions-ext](https://docs.rs/tower-sessions-ext) · [axum-login](https://docs.rs/crate/axum-login/latest)
- [jsonwebtoken](https://docs.rs/crate/jsonwebtoken/latest) · [webauthn-rs](https://docs.rs/webauthn-rs) · [argon2](https://docs.rs/argon2) · [totp-rs](https://docs.rs/crate/totp-rs/latest) · [oxide-auth](https://docs.rs/crate/oxide-auth/latest)
- [axum\_csrf](https://docs.rs/crate/axum_csrf/latest) · [tower\_governor](https://docs.rs/tower_governor) · [garde](https://docs.rs/garde_derive)
- [SeaORM 2.0 announcement](https://www.sea-ql.org/blog/2026-07-27-sea-orm-2.0/)

**Authorization**

- [@casl/ability API reference](https://tessl.io/registry/tessl/npm-casl--ability) · [@casl/react](https://npmjs.com/package/@casl/react) · [@casl/vue](https://www.npmjs.com/package/@casl/vue) · [@casl/ability README](https://npmjs.com/package/@casl/ability)
- [casbin](https://docs.rs/crate/casbin/latest) · [cedar-policy](https://docs.rs/crate/cedar-policy)

**Advanced stack**

- Jobs: [apalis](https://docs.rs/crate/apalis/latest) · [apalis-board](https://docs.rs/crate/apalis-board/latest) · [graphile\_worker](https://docs.rs/graphile_worker) · [croner](https://docs.rs/croner)
- Cache and mail: [moka](https://docs.rs/crate/moka/latest) · [deadpool-redis](https://docs.rs/deadpool-redis) · [mail-send](https://docs.rs/mail-send) · [mrml](https://docs.rs/crate/mrml/latest)
- Storage and real-time: [object\_store](https://docs.rs/object_store/latest) · [opendal](https://docs.rs/crate/opendal/0.59.0) · [socketioxide](https://github.com/Totodore/socketioxide) · [Sockudo with Laravel Echo](https://sockudo.io/integrations/laravel-echo)
- Messaging and search: [async-nats](https://docs.rs/crate/async-nats/0.50.0) · [lapin](https://docs.rs/crate/lapin/latest) · [rdkafka](https://docs.rs/crate/rdkafka/latest) · [meilisearch-sdk](https://docs.rs/crate/meilisearch-sdk) · [tantivy](https://docs.rs/tantivy) · [qdrant-client](https://docs.rs/qdrant-client) · [pgvector](https://docs.rs/crate/pgvector/latest)
- API: [utoipa](https://docs.rs/crate/utoipa/latest) · [async-graphql](https://docs.rs/crate/async-graphql/latest) · [tonic](https://docs.rs/crate/tonic/latest) · [connect-rust joins the Connect project](https://buf.build/blog/connect-rust-joins-the-connect-project)
- HTTP client and observability: [reqwest](https://docs.rs/crate/reqwest/latest) · [backon](https://docs.rs/crate/backon/latest) · [opentelemetry-otlp](https://docs.rs/opentelemetry-otlp) · [OpenTelemetry Rust status](https://opentelemetry.io/docs/languages/rust/index.md) · [sentry](https://docs.rs/crate/sentry/latest)
- AI: [rmcp](https://docs.rs/crate/rmcp) · [rig](https://docs.rs/rig/latest/) · [genai](https://docs.rs/crate/genai/0.5.1) · [async-openai](https://docs.rs/crate/async-openai) · [OpenAI libraries page](https://developers.openai.com/api/docs/libraries)

**Everything else**

- [async-stripe](https://docs.rs/crate/async-stripe/latest) · [paddle-rust-sdk](https://docs.rs/paddle-rust-sdk) · [open-feature](https://docs.rs/crate/open-feature/latest) · [unleash-api-client](https://docs.rs/unleash-api-client)
- [rust-i18n](https://docs.rs/rust-i18n/) · [ICU4X](https://releasealert.dev/cratesio/icu) · [SeaORM Pro](https://www.sea-ql.org/sea-orm-pro/docs/introduction/sea-orm-pro) · [Cot](https://docs.rs/crate/cot/latest)
- Testing: [axum-test](https://docs.rs/crate/axum-test/21.0.0) · [testcontainers](https://docs.rs/testcontainers) · [cargo-nextest](https://nexte.st/changelog) · [thirtyfour](https://docs.rs/crate/thirtyfour/0.37.4)
- Security and files: [cargo-audit](https://docs.rs/crate/cargo-audit) · [cargo-deny](https://docs.rs/cargo-deny) · [RustSec advisory DB](https://github.com/RustSec/advisory-db) · [typst-library](https://docs.rs/typst-library) · [rust\_xlsxwriter](https://docs.rs/rust_xlsxwriter/)
- Social login, tenancy, IDs, deploy: [oauth2](https://docs.rs/crate/oauth2/latest) · [openidconnect](https://docs.rs/openidconnect) · [tenaxum](https://docs.rs/crate/tenaxum/latest) · [uuid](https://docs.rs/uuid) · [cargo-chef](https://docs.rs/cargo-chef) · [Shuttle shutdown notice](https://docs.shuttle.dev/docs/shuttle-shutdown.md)

**UI stack**

- Next.js: [Next.js 16.4 release post](https://nextjs.org/blog/next-16-4.md) · [Next.js blog and release history](https://nextjs.org/blog)
- Nuxt: [Nuxt releases (4.6.2; Nuxt 3 end-of-life)](https://releases.sh/nuxt/overview) · [nuxt-auth-sanctum module](https://nuxt.com/modules/nuxt-auth-sanctum)
- Real-time clients: [@laravel/echo-react](https://www.jsdelivr.com/package/npm/@laravel/echo-react) · [Sockudo with Laravel Echo](https://sockudo.io/integrations/laravel-echo)
- Email templates: [minijinja](https://docs.rs/crate/minijinja)
- Laravel 13 release notes: [laravel.com/docs/13.x/releases](https://laravel.com/docs/13.x/releases.md)

**Modularity and developer experience**

- [Pavex](https://docs.rs/crate/pavex/latest) · [spring-rs](https://docs.rs/crate/spring/latest) · [Loco DI explanation](https://loco.rs/docs/explanation/appcontext-and-di/) · [dill](https://docs.rs/crate/dill/latest) · [nestrs announcement](https://users.rust-lang.org/t/nestrs-a-nestjs-style-framework-for-rust-built-on-axum-and-tower/139700)
- [Rust 1.90 lld default](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable) · [Rust releases](https://blog.rust-lang.org/releases) · [parallel front-end goal](https://goals.rust-lang.org/2026/parallel-front-end.html) · [cargo-watch status](https://docs.rs/crate/cargo-watch/latest) · [systemfd](https://github.com/mitsuhiko/systemfd) · [evcxr](https://docs.rs/crate/evcxr_jupyter)
- [Actix Web](https://docs.rs/actix-web/4/) · [Rocket](https://docs.rs/crate/rocket/latest) · [Poem](https://docs.rs/crate/poem/latest)