# Navree Framework + CRM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task also follows superpowers:test-driven-development.

**Goal:** Build the Navree framework (PRD Phase 0–1 plus CRM-relevant Phase 2 modules) as a Rust workspace of module crates with TDD, then a CRM (Navree API + Next.js UI) on top.

**Architecture:** A small kernel (module graph, type-keyed DI container, lifecycle, command runner) and one crate per module. Modules contribute providers, routes, migrations, commands, jobs and layers through `ModuleBuilder`. HTTP is Axum 0.8 on Tower; data is SeaORM 2.0; everything cross-cutting goes through `navree-contracts`.

**Tech Stack:** Rust 1.99 / edition 2024, Tokio, Axum 0.8.9, tower-http 0.7.1, SeaORM 2.0.4 (SQLite in tests), garde 0.23, argon2 0.6, aes-gcm 0.11, totp-rs 6, minijinja 3, lettre 0.11, croner 4, utoipa 6, object_store 0.14, reqwest 0.13, moka 0.12, tokio-tungstenite 0.30, metrics 0.24; Next.js 16.4, Tailwind, @casl/ability 7, @casl/react 7, openapi-typescript + openapi-fetch, laravel-echo + pusher-js, Playwright.

**Spec:** `docs/superpowers/specs/2026-10-25-navree-framework-design.md` (decisions, hierarchy, waves, CRM API contract) and `docs/prd/navree-prd.md` (the PRD).

## Global Constraints

- Edition 2024, `rust-version = "1.95"`, `#![forbid(unsafe_code)]` in every crate; `panic = "unwind"` enforced by navree-kernel with `#[cfg(not(panic = "unwind"))] compile_error!`.
- External versions only in root `[workspace.dependencies]`; crates use `{ workspace = true }`.
- rustls with the `ring` provider; never require cmake/NASM/OpenSSL.
- `chrono` for time, UUID v7 for ids (`Uuid::now_v7()`).
- Crate dependency edges must match the table in spec §3 (enforced by `crates/navree/tests/dependency_graph.rs`).
- Framework crates never use navree-macros.
- Error body (NestJS format, user decision 2026-10-25, spec §10): `{"statusCode": <u16>, "message": <string | string[]>, "error": <reason phrase>}`, content type `application/json`; validation failures are 400 with `message: string[]` and an `errors: {field: [messages]}` map; CSRF failure (optional navree-session) is 403 "Invalid CSRF token"; 500 bodies say "Internal server error". 422 and 419 are not used. Auth is NestJS-style JWT (Task 28), not Sanctum.
- Request IDs: accept incoming `X-Request-Id` only if ≤ 64 chars of `[A-Za-z0-9_-]`, otherwise generate a UUID v7; echo in the response header.
- Config env mapping: prefix `APP_`, nesting separator `__`, keys lower-cased (`APP_DATABASE__URL` → `database.url`).
- Pagination defaults: `page` 1, `per_page` 15, max 100; meta keys `current_page, per_page, total, last_page`.
- Each agent: own `CARGO_TARGET_DIR=target/agents/<label>`; edit only owned crates; no commits except wave integrators.

## Review Focus

1. **Module wiring mistakes** (missing provider, provider not exported, import cycle, singleton → request-scoped edge, a module configured twice): boot must fail before binding a port, naming the module and the type. Owned by Task 2 (tests listed there).
2. **A route with no access declaration**: must not be reachable. Compile error from `#[controller]` (Task 13, trybuild compile-fail test) and boot error from navree-http for hand-built `RouteDef`s (Task 4 test `boot_fails_for_route_without_access_declaration`).
3. **CSRF on cookie-authenticated writes**: a POST with a valid session cookie but no/incorrect `X-XSRF-TOKEN` must get 403 "Invalid CSRF token", while bearer-token requests and GETs pass (navree-session is optional; the CRM uses JWT and does not import it). Task 19 tests.
4. **Error responses keep CORS + request id**: a handler panic, a timeout, a 404 and a 400 validation error must all carry `X-Request-Id` and the CORS headers for an allowed origin. Task 4 tests.
5. **Last-match-wins authorization parity between record checks and list filters**: for any rule set, `ability.can(action, record)` must equal "record is in `Accessible` list query". Task 9 property test (`proptest`) and Task 22 SQL test.

---

## Wave 0 — Foundation

### Task 1: Workspace skeleton (agent `skeleton`)

**Files:**
- Create: `Cargo.toml` (workspace, `resolver = "3"`, members `crates/*`, `apps/crm-api`), `.cargo/config.toml` (`[build] rustc-wrapper = "sccache"`), `.gitignore` (`target/`, `node_modules/`, `.next/`, `storage/`, `*.db`, `.env`), `rustfmt.toml` (`edition = "2024"`), `clippy.toml` if needed.
- Create: every crate directory from spec §2 under `crates/<name>/` with `Cargo.toml` (deps per spec §3 table + the external crates its plan task names) and a stub `src/lib.rs` containing `#![forbid(unsafe_code)]` and a crate doc comment. `crates/navree-cli` has `src/main.rs`. `crates/navree-macros` is `proc-macro = true`.
- Create: `apps/crm-api/Cargo.toml` + stub `src/main.rs` (binary `crm-api`).
- Create: `crates/navree/tests/dependency_graph.rs` — runs `cargo metadata --format-version 1 --no-deps`, and asserts every `navree-*` dependency edge (normal and dev, excluding `navree-testing` as a dev-dependency of L4+ crates) is allowed by the spec §3 table encoded as a `const` array.
- Create: `README.md` section "Building" (target-dir convention, `cargo test --workspace`).

**Steps:**
- [ ] Root `Cargo.toml` profiles to keep memory and link time down: `[profile.dev] debug = "line-tables-only"`, `[profile.dev.package."*"] debug = false`, `[profile.test] debug = "line-tables-only"`.
- [ ] Verify each external version with `cargo search <crate> --limit 1`; put all external crates any task below names into `[workspace.dependencies]` with default-features choices that avoid aws-lc/openssl (e.g. `reqwest` with `rustls` + `ring`-backed provider, `lettre` with `tokio1-rustls`, `builder`, `smtp-transport`, `ring`; `sea-orm` with `sqlx-sqlite`, `runtime-tokio-rustls`, `macros`, `with-chrono`, `with-uuid`, `with-json`; `sqlx`/`libsqlite3-sys` bundled).
- [ ] Write `dependency_graph.rs` first; run it; it must pass on the stub workspace.
- [ ] `CARGO_TARGET_DIR=target/agents/skeleton cargo check --workspace --all-targets` must pass (this also warms sccache with every dependency).
- [ ] `cargo test --workspace --no-run` to warm sccache for dev-dependencies too; then report the final dependency list and any feature choices made for Windows.

### Task 2: navree-kernel + navree-contracts (agent `kernel`)

**Files:** `crates/navree-kernel/src/{lib.rs,error.rs,key.rs,module.rs,builder.rs,container.rs,resolver.rs,lifecycle.rs,app.rs,command.rs,graph.rs,lazy.rs}`, tests in `crates/navree-kernel/tests/`; `crates/navree-contracts/src/{lib.rs,secret.rs,request_id.rs,principal.rs,audit.rs,events.rs,broadcast.rs,health.rs,session.rs,jobs.rs,clock.rs}`.

**Interfaces (Produces) — navree-kernel:**

```rust
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub struct ModuleId { /* TypeId + &'static str name */ }
impl ModuleId { pub fn of<M: ?Sized + 'static>() -> Self; pub fn name(&self) -> &'static str; }

#[derive(Clone, PartialEq, Eq, Hash, Debug)] pub struct ProviderKey { /* TypeId, type_name, Option<Cow<'static, str>> */ }
impl ProviderKey { pub fn of<T: ?Sized + 'static>() -> Self; pub fn named<T: ?Sized + 'static>(name: impl Into<Cow<'static, str>>) -> Self; pub fn type_name(&self) -> &'static str; pub fn name(&self) -> Option<&str>; }

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)] pub enum Scope { #[default] Singleton, Request, Transient }

pub trait Module: Send + Sync + 'static {
    fn id(&self) -> ModuleId { ModuleId::of::<Self>() }
    fn name(&self) -> &'static str { /* short type name of Self */ }
    /// `for_root(...)` values return true; at most one configured instance per id.
    fn is_configured(&self) -> bool { false }
    fn register(&self, m: &mut ModuleBuilder);
}

pub trait Injectable: Sized + Send + Sync + 'static {
    fn scope() -> Scope { Scope::Singleton }
    fn dependencies() -> Vec<ProviderKey> { Vec::new() }
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>>;
}

pub struct Factory<T: ?Sized> { /* scope, deps, name, Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>, BoxError>>> */ }
impl<T: ?Sized + Send + Sync + 'static> Factory<T> {
    pub fn new<F, Fut>(f: F) -> Self where F: Fn(Resolver) -> Fut + Send + Sync + 'static, Fut: Future<Output = Result<Arc<T>, BoxError>> + Send + 'static;
    pub fn scope(self, s: Scope) -> Self; pub fn deps(self, d: Vec<ProviderKey>) -> Self; pub fn named(self, n: &'static str) -> Self;
}

impl ModuleBuilder {
    pub fn module_id(&self) -> ModuleId;
    pub fn import(&mut self, module: impl Module) -> &mut Self;
    pub fn imports(&mut self, modules: impl IntoModules) -> &mut Self; // tuples (1..=16) of Modules, Vec<Box<dyn Module>>
    pub fn global(&mut self) -> &mut Self;
    pub fn provider<T: Injectable>(&mut self) -> &mut Self;
    pub fn provider_named<T: Injectable>(&mut self, name: &'static str) -> &mut Self;
    pub fn value<T: Send + Sync + 'static>(&mut self, value: T) -> &mut Self;
    pub fn value_arc<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) -> &mut Self;
    pub fn factory<T: ?Sized + Send + Sync + 'static>(&mut self, f: Factory<T>) -> &mut Self;
    pub fn multi<T: ?Sized + Send + Sync + 'static>(&mut self, f: Factory<T>) -> &mut Self; // multi-binding, see get_all
    pub fn exports<T: ?Sized + 'static>(&mut self) -> &mut Self;
    pub fn export_key(&mut self, key: ProviderKey) -> &mut Self;
    pub fn contribute<C: Send + Sync + 'static>(&mut self, c: C) -> &mut Self;
    pub fn lifecycle<T: Lifecycle + Send + Sync + 'static>(&mut self) -> &mut Self; // T is a provider of this module
    pub fn command(&mut self, cmd: CommandSpec) -> &mut Self;
}

#[derive(Clone)] pub struct Resolver { /* Arc<container>, ModuleId, Option<RequestScope>, resolution stack */ }
impl Resolver {
    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, KernelError>;
    pub async fn get_named<T: ?Sized + Send + Sync + 'static>(&self, name: &str) -> Result<Arc<T>, KernelError>;
    pub async fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Vec<Arc<T>>, KernelError>;
    pub fn lazy<T: ?Sized + Send + Sync + 'static>(&self) -> Lazy<T>;
    pub fn app(&self) -> AppContext;
    pub fn module(&self) -> ModuleId;
}
pub struct Lazy<T: ?Sized> { /* resolved on first get, no graph edge */ }
impl<T: ?Sized + Send + Sync + 'static> Lazy<T> { pub async fn get(&self) -> Result<Arc<T>, KernelError>; }

#[derive(Clone, Default)] pub struct RequestScope { /* per-unit-of-work cache */ }

#[derive(Clone)] pub struct AppContext { /* Arc<inner> */ }
impl AppContext {
    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, KernelError>; // service locator, ignores visibility
    pub async fn try_get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>>;
    pub async fn get_named<T: ?Sized + Send + Sync + 'static>(&self, name: &str) -> Result<Arc<T>, KernelError>;
    pub async fn get_all<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Vec<Arc<T>>, KernelError>;
    pub fn contributions<C: Send + Sync + 'static>(&self) -> Vec<Contributed<C>>;
    pub fn resolver_for(&self, module: ModuleId) -> Resolver;
    pub fn request_resolver(&self, module: ModuleId, scope: RequestScope) -> Resolver;
    pub fn modules(&self) -> Vec<ModuleInfo>; // dependency order
    pub fn commands(&self) -> Vec<CommandSpec>;
    pub fn shutdown_token(&self) -> tokio_util::sync::CancellationToken;
    pub fn tracker(&self) -> tokio_util::task::TaskTracker;
}
pub struct Contributed<C> { pub module: ModuleInfo, pub value: Arc<C> }
#[derive(Clone, Debug)] pub struct ModuleInfo { pub id: ModuleId, pub name: &'static str, pub global: bool }

pub trait Lifecycle: Send + Sync + 'static {
    fn on_init(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> { Box::pin(async { Ok(()) }) }
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> { Box::pin(async { Ok(()) }) }
    fn on_shutdown(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> { Box::pin(async { Ok(()) }) }
}

#[derive(Clone)] pub struct CommandSpec { pub name: &'static str, pub about: &'static str, pub run: Arc<dyn Fn(AppContext, Vec<String>) -> BoxFuture<'static, Result<(), BoxError>> + Send + Sync> }
impl CommandSpec { pub fn new<F, Fut>(name: &'static str, about: &'static str, f: F) -> Self where F: Fn(AppContext, Vec<String>) -> Fut + Send + Sync + 'static, Fut: Future<Output = Result<(), BoxError>> + Send + 'static; }

pub struct Navree; impl Navree { pub fn app() -> AppBuilder; }
impl AppBuilder {
    pub fn module(self, m: impl Module) -> Self;
    pub fn override_value<T: ?Sized + Send + Sync + 'static>(self, v: Arc<T>) -> Self; // replaces unnamed provider T wherever registered; registers globally if absent
    pub fn shutdown_timeout(self, d: Duration) -> Self; // default 30 s
    pub fn validate(&self) -> Result<GraphReport, BootErrors>; // no construction
    pub async fn boot(self) -> Result<App, BootErrors>;
    pub async fn run_with_args(self, args: Vec<String>) -> Result<(), BoxError>; // "check [--graph]", "help"/"list" built in; default command "serve" if registered else "help"
    pub fn run(self) -> Result<(), BoxError>; // multi-thread runtime, std::env::args, ctrl-c/SIGTERM → cancel token → on_shutdown (reverse dependency order) within timeout
}
pub struct App; impl App { pub fn ctx(&self) -> &AppContext; pub async fn run_command(&self, name: &str, args: Vec<String>) -> Result<(), BoxError>; pub async fn shutdown(self) -> Result<(), BoxError>; }
pub struct GraphReport { pub modules: Vec<ModuleReport> } // Display prints the module tree for `check --graph`
pub struct BootErrors(pub Vec<KernelError>); // Display lists every error, one per line
#[derive(Debug, thiserror::Error)] pub enum KernelError {
    MissingProvider { module: &'static str, requested: String, required_by: String },
    NotExported { module: &'static str, requested: String, owner: &'static str },
    ProviderCycle { path: Vec<String> },
    ModuleCycle { path: Vec<&'static str> },
    ScopeInversion { singleton: String, request_scoped: String },
    ConfiguredTwice { module: &'static str },
    DuplicateProvider { module: &'static str, key: String },
    Construction { provider: String, source: BoxError },
    UnknownCommand { name: String },
    Lifecycle { module: &'static str, hook: &'static str, source: BoxError },
}
```

Rules: a module resolves its own providers, the exports of modules it imports directly, and exports of global modules; root modules passed to `AppBuilder::module` are imported by an implicit root. Singletons are built once (memoized, on demand, eagerly at boot in module dependency order) and shared as `Arc<T>`; request-scoped providers are memoized per `RequestScope`; transient providers are built on every `get`. Static validation uses declared dependencies; runtime resolution detects cycles through the resolver's stack. `register` is called once per `ModuleId`. No global statics anywhere in the kernel.

**Interfaces (Produces) — navree-contracts:**

```rust
pub use navree_kernel::{BoxError, BoxFuture};
pub struct Secret<T>(T); // new, expose(&self) -> &T, into_inner; Debug/Display "[redacted]"; Deserialize transparent; Clone/PartialEq when T is; NO Serialize impl
#[derive(Clone, Debug, PartialEq, Eq, Hash)] pub struct RequestId(String); // RequestId::parse(&str) -> Option<Self> (≤64, [A-Za-z0-9_-]), RequestId::generate() (uuid v7 simple), parse_or_generate, as_str, Display
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct Principal { pub user_id: Uuid, pub via: AuthVia, pub token_abilities: Option<Vec<String>>, pub session_id: Option<String> }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum AuthVia { Session, Token, Test }
#[derive(Clone, Debug, Default)] pub struct AuthState(pub Option<Principal>);
pub trait AuditSink: Send + Sync + 'static { fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>>; }
#[derive(Clone, Debug, Serialize, Deserialize)] pub struct AuditEntry { pub kind: AuditKind, pub action: String, pub actor_id: Option<Uuid>, pub subject_type: Option<String>, pub subject_id: Option<String>, pub old_values: Option<Value>, pub new_values: Option<Value>, pub request_id: Option<String>, pub ip: Option<String>, pub user_agent: Option<String>, pub metadata: Value }
// AuditEntry::new(kind, action) + builder setters (actor, subject, old, new, request_id, ip, user_agent, metadata)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")] pub enum AuditKind { Data, Auth, Access, Security, Custom }
pub trait EventSink: Send + Sync + 'static { fn publish(&self, name: &str, payload: Value) -> BoxFuture<'_, Result<(), BoxError>>; }
pub trait Broadcaster: Send + Sync + 'static { fn broadcast(&self, channels: Vec<String>, event: String, data: Value) -> BoxFuture<'_, Result<(), BoxError>>; }
pub trait HealthCheck: Send + Sync + 'static { fn name(&self) -> &str; fn check(&self) -> BoxFuture<'_, HealthStatus>; }
#[derive(Clone, Debug, PartialEq)] pub enum HealthStatus { Up, Down(String) }
pub trait SessionStore: Send + Sync + 'static { fn load(&self, id: &str) -> BoxFuture<'_, Result<Option<SessionRecord>, BoxError>>; fn save(&self, id: &str, record: SessionRecord) -> BoxFuture<'_, Result<(), BoxError>>; fn delete(&self, id: &str) -> BoxFuture<'_, Result<(), BoxError>>; }
#[derive(Clone, Debug, Default, Serialize, Deserialize)] pub struct SessionRecord { pub data: serde_json::Map<String, Value>, pub expires_at: Option<DateTime<Utc>> }
pub trait Clock: Send + Sync + 'static { fn now(&self) -> DateTime<Utc>; } pub struct SystemClock; pub struct FixedClock(Mutex<DateTime<Utc>>) /* set, advance */;
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static { const NAME: &'static str; fn options() -> JobOptions { JobOptions::default() } fn handle(self, ctx: JobContext) -> impl Future<Output = Result<(), BoxError>> + Send; }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)] pub struct JobOptions { pub queue: String /* "default" */, pub max_attempts: u32 /* 3 */, pub backoff: Backoff, pub timeout_secs: u64 /* 120 */, pub unique_for_secs: Option<u64> }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)] pub enum Backoff { Fixed { secs: u64 }, Exponential { base_secs: u64, max_secs: u64 } } // default Exponential{1, 300}; fn delay_for(&self, attempt: u32) -> Duration (attempt 1 → base, doubles, capped)
#[derive(Clone)] pub struct JobContext { pub app: AppContext, pub job_id: String, pub attempt: u32 }
#[derive(Clone, Debug, Serialize, Deserialize)] pub struct JobEnvelope { pub job_type: String, pub payload: Value, pub options: JobOptions, pub delay_secs: Option<u64>, pub unique_key: Option<String> }
pub trait Dispatcher: Send + Sync + 'static { fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>>; }
impl dyn Dispatcher { pub async fn dispatch<J: Job>(&self, job: J) -> Result<String, BoxError>; pub async fn dispatch_later<J: Job>(&self, job: J, delay: Duration) -> Result<String, BoxError>; }
#[derive(Clone)] pub struct JobRegistration { pub name: &'static str, pub options: JobOptions, pub run: fn(Value, JobContext) -> BoxFuture<'static, Result<(), BoxError>> }
impl JobRegistration { pub fn of<J: Job>() -> Self; }
pub trait ModuleBuilderJobsExt { fn job<J: Job>(&mut self) -> &mut Self; } // contributes JobRegistration::of::<J>()
```

**Steps (TDD, one cycle each; test names are the contract):**
- [ ] `boots_singleton_once_and_shares_arc` — two providers depending on `Arc<Repo>` get `Arc::ptr_eq` instances; construction counter = 1.
- [ ] `resolves_exported_provider_from_imported_module` and `rejects_unexported_provider` (error is `NotExported` naming both modules).
- [ ] `reports_missing_provider_with_module_and_type` — `BootErrors` Display contains the module name and the requested type name.
- [ ] `detects_provider_cycle`, `detects_module_import_cycle`, `rejects_singleton_depending_on_request_scoped`, `rejects_module_configured_twice`, `dedupes_plain_module_imported_twice` (register called once).
- [ ] `global_module_exports_visible_everywhere`, `named_providers_coexist` (`Db` and `Db("analytics")`), `trait_object_provider_via_factory` (`Arc<dyn Greeter>`), `multi_binding_get_all_returns_every_registration`, `override_value_replaces_provider`.
- [ ] `request_scope_memoizes_per_scope`, `transient_builds_every_time`, `lazy_resolves_after_boot_without_graph_edge`.
- [ ] `contributions_are_collected_with_module_info`.
- [ ] `lifecycle_hooks_run_in_dependency_order_and_shutdown_in_reverse`.
- [ ] `run_with_args_dispatches_command`, `default_command_is_serve_when_registered`, `check_validates_without_constructing` (factory with side effect not called), `unknown_command_errors`.
- [ ] `shutdown_cancels_token_and_drains_tracker`.
- [ ] contracts: `secret_debug_is_redacted`, `secret_deserializes_transparently`, `request_id_rejects_long_or_bad_chars`, `backoff_exponential_doubles_and_caps`, `job_registration_runs_typed_handler`, `dispatch_serializes_job_into_envelope` (test dispatcher capturing envelopes).
- [ ] `cargo test -p navree-kernel -p navree-contracts`, clippy, fmt clean.

### Task 3: navree-config (agent `config`)

**Interfaces (Produces):**

```rust
pub struct ConfigModule; impl ConfigModule { pub fn for_root(sources: ConfigSources) -> impl Module; } // global; exports Config
#[derive(Clone, Default)] pub struct ConfigSources { /* ordered */ }
impl ConfigSources { pub fn new() -> Self; pub fn dotenv(self, path: impl Into<PathBuf>) -> Self /* missing file ok */; pub fn file(self, path) -> Self /* toml; missing ok */; pub fn env(self) -> Self /* APP_ prefix, "__" */; pub fn env_prefixed(self, prefix: &str) -> Self; pub fn set(self, key: &str, value: impl Into<serde_json::Value>) -> Self /* highest precedence; used by tests */; pub fn defaults() -> Self /* .env, config/app.toml, env */; }
#[derive(Clone)] pub struct Config { /* merged tree */ }
impl Config { pub fn load(sources: &ConfigSources) -> Result<Config, ConfigError>; pub fn section<T: DeserializeOwned>(&self, key: &str) -> Result<T, ConfigError>; pub fn section_or_default<T: DeserializeOwned + Default>(&self, key: &str) -> Result<T, ConfigError>; pub fn get_string(&self, key: &str) -> Option<String>; pub fn get_bool(&self, key: &str) -> Option<bool>; pub fn redacted(&self) -> serde_json::Value /* keys matching password|secret|token|key|authorization|cookie → "[redacted]" */; }
pub trait ConfigSection: DeserializeOwned + Send + Sync + 'static { const KEY: &'static str; fn validate(&self) -> Result<(), String> { Ok(()) } }
pub trait ModuleBuilderConfigExt { fn config<T: ConfigSection>(&mut self) -> &mut Self; } // registers singleton T built from Config at boot; missing section → T deserialized from {} ; validation error → Construction error naming T::KEY
pub fn parse_dotenv(src: &str) -> Result<Vec<(String, String)>, ConfigError>; // KEY=value, quotes, `export ` prefix, comments, ${VAR} expansion from earlier keys; never touches process env
#[derive(Debug, thiserror::Error)] pub enum ConfigError { … }
```

Precedence low→high in the order sources were added. `.env` entries go through the same `APP_`/`__` mapping as environment variables.

- [ ] Tests: `dotenv_parses_quotes_comments_export_and_expansion`, `env_maps_prefix_and_double_underscore`, `later_sources_override_earlier`, `section_deserializes_typed_struct`, `config_section_validation_failure_fails_boot_naming_section`, `redacted_masks_secret_keys`, `set_overrides_everything` (boot an app with `ConfigModule` + a module using `.config::<T>()`).

### Task 3b: Wave 0 integrator (agent `w0-review`)

- [ ] Review kernel/contracts/config against Tasks 2–3 interfaces and PRD "Module system" rules; add any missing tests from the Task 2 list; run `cargo test --workspace`, clippy, fmt; fix; commit `feat(kernel): module graph, DI container, lifecycle, contracts, config`.

---

## Wave 1 — Platform (parallel)

### Task 4: navree-http (agent `http`)

**Interfaces (Consumes):** kernel, contracts (`RequestId`, `AuthState`, `Principal`, `AuditSink`), config (`Config::section("http")`).

**Interfaces (Produces):**

```rust
pub struct HttpModule; impl HttpModule { pub fn for_root(opts: HttpOptions) -> impl Module; pub fn from_config() -> impl Module; } // global; contributes commands "serve", "routes", "down", "up"
#[derive(Clone, Debug, Deserialize)] #[serde(default)] pub struct HttpOptions { pub addr: String /* "127.0.0.1:8080" */, pub api_prefix: String /* "/api" */, pub cors: CorsOptions, pub timeout_secs: u64 /* 30 */, pub body_limit_bytes: usize /* 2 MiB */, pub per_ip_rate_limit: Option<u32> /* per minute; default Some(600) */, pub security_headers: bool /* true */, pub compression: bool /* true */, pub maintenance_file: PathBuf /* "storage/framework/down" */, pub trust_proxy: bool /* false */, pub signing_key: Option<Secret<String>> }
#[derive(Clone, Debug, Deserialize, Default)] pub struct CorsOptions { pub allowed_origins: Vec<String>, pub allow_credentials: bool /* true */, pub max_age_secs: u64 }

pub trait Controller: Send + Sync + 'static { fn prefix(&self) -> &'static str; fn version(&self) -> Option<&'static str> { None } fn routes(self: Arc<Self>) -> Vec<RouteDef>; }
pub trait ModuleBuilderHttpExt { fn controller<C: Controller + Injectable>(&mut self) -> &mut Self; fn http_layer(&mut self, layer: GlobalLayer) -> &mut Self; }
pub struct RouteDef { pub method: Method, pub path: String, pub handler: axum::routing::MethodRouter, pub meta: RouteMeta }
impl RouteDef { pub fn new(method: Method, path: impl Into<String>, handler: MethodRouter) -> Self; pub fn public(self) -> Self; pub fn roles(self, r: &[&str]) -> Self; pub fn permissions(self, p: &[&str]) -> Self; pub fn can(self, action: &str, subject: &str, from: Option<&str>) -> Self; pub fn name(self, n: &str) -> Self; pub fn throttle(self, limit: u32, per: Duration) -> Self; pub fn timeout(self, d: Duration) -> Self; pub fn transactional(self) -> Self; pub fn guard(self, g: Arc<dyn Guard>) -> Self; pub fn api(self, m: ApiMeta) -> Self; pub fn audit(self, action: &str) -> Self; pub fn body_limit(self, bytes: usize) -> Self; }
#[derive(Clone, Debug, Default)] pub struct RouteMeta { pub name: Option<String>, pub access: AccessSpec, pub guards: Vec<Arc<dyn Guard>>, pub throttle: Option<Throttle>, pub timeout: Option<Duration>, pub transactional: bool, pub api: Option<ApiMeta>, pub audit_action: Option<String>, pub body_limit: Option<usize>, pub module: Option<&'static str>, pub full_path: String, pub method: String }
#[derive(Clone, Debug, Default)] pub struct AccessSpec { pub public: bool, pub roles: Vec<String>, pub permissions: Vec<String>, pub can: Option<CanSpec> } // is_declared()
#[derive(Clone, Debug)] pub struct CanSpec { pub action: String, pub subject: String, pub from: Option<String> }
#[derive(Clone, Copy, Debug)] pub struct Throttle { pub limit: u32, pub per: Duration }
#[derive(Clone, Debug, Default)] pub struct ApiMeta { pub summary: Option<String>, pub description: Option<String>, pub tags: Vec<String>, pub responses: Vec<u16>, pub success_status: u16, pub request: Option<SchemaFn>, pub response: Option<SchemaFn> }
pub type SchemaFn = fn() -> (String, serde_json::Value); // (schema name, JSON schema)
pub fn schema_fn<T: utoipa::ToSchema>() -> (String, serde_json::Value); // lives in navree-http so macros never need navree-api
pub async fn route_table(ctx: &AppContext) -> Result<Vec<RouteMeta>, BoxError>; // every route with full path + meta; used by `routes`, navree-api, navree-access
pub trait Guard: Send + Sync + 'static + std::fmt::Debug { fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>>; }
pub trait Authenticator: Send + Sync + 'static { fn authenticate<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<Option<Principal>, AppError>>; } // resolved via ctx.try_get::<dyn Authenticator>()
pub trait RouteAuthorizer: Send + Sync + 'static { fn authorize<'a>(&'a self, parts: &'a mut Parts, meta: &'a RouteMeta) -> BoxFuture<'a, Result<(), AppError>>; } // resolved via ctx.try_get::<dyn RouteAuthorizer>()
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)] pub enum LayerSlot { Session, AuthContext, Uow }
#[derive(Clone)] pub struct GlobalLayer { pub slot: LayerSlot, pub name: &'static str, pub apply: Arc<dyn Fn(&AppContext, Router) -> Result<Router, BoxError> + Send + Sync> }
pub async fn build_router(ctx: &AppContext) -> Result<Router, BoxError>; // full app with every layer; used by `serve` and navree-testing
pub struct AppError { pub status: StatusCode, pub code: String, pub title: String, pub detail: Option<String>, pub errors: Option<BTreeMap<String, Vec<String>>>, pub headers: HeaderMap, pub source: Option<BoxError> }
impl AppError { pub fn new(status, code, title) -> Self; pub fn bad_request(detail) ; pub fn unauthenticated() /*401*/; pub fn forbidden() /*403*/; pub fn not_found(); pub fn conflict(detail); pub fn csrf_mismatch() /*419*/; pub fn validation(errors: BTreeMap<String, Vec<String>>) /*422*/; pub fn too_many_requests(retry_after: Duration) /*429 + Retry-After*/; pub fn service_unavailable(detail); pub fn internal(err: impl Into<BoxError>) /*500; detail hidden*/; pub fn with_detail(self, d) -> Self; }
impl IntoResponse for AppError; impl<E: Into<BoxError>> From<E> for AppError where … // anyhow-like: map unknown errors to 500 (implement via a helper trait to avoid coherence conflicts)
pub type Result<T, E = AppError> = std::result::Result<T, E>;
pub struct Created<T>(pub String /* location */, pub T); pub struct NoContent; pub use axum::Json;
pub struct CurrentPrincipal(pub Principal); // extractor: 401 when AuthState is None
pub struct ClientIp(pub IpAddr); // extractor + extension
pub struct UrlSigner; impl UrlSigner { pub fn new(key: &[u8]) -> Self; pub fn sign(&self, path_and_query: &str, expires_at: DateTime<Utc>) -> String; pub fn verify(&self, uri: &Uri, now: DateTime<Utc>) -> bool; }
pub struct ValidSignature; // Guard (401 on bad/expired signature)
```

Behaviour: full path = `api_prefix` (only when the controller prefix does not start with `/`… rule: controllers mount under `api_prefix`; `RouteDef` paths beginning with `!` are absolute, used for `/sanctum/csrf-cookie`, `/broadcasting/auth`, `/health/*`, `/app/{key}`) + `/v{version}` + prefix + path. Axum 0.8 `{param}` syntax. Single 404 problem+json fallback added last. Request log line at INFO with target `navree::request` (method, route template, status, latency_ms, request_id, user_id, ip). Metrics via the `metrics` facade (`http_requests_total`, `http_request_duration_seconds`). Per-route audit action → `AuditSink` (if registered) on 2xx.

- [ ] Tests (in-process `tower::ServiceExt::oneshot` against `build_router`): `problem_json_shape_and_content_type`, `request_id_is_generated_and_echoed`, `forged_request_id_is_replaced`, `cors_headers_on_error_responses` (panic, timeout, 404, 422 for allowed origin), `cors_rejects_unknown_origin`, `security_headers_present`, `compression_skipped_for_event_stream`, `maintenance_mode_returns_503`, `per_ip_rate_limit_returns_429_with_retry_after`, `body_limit_returns_413`, `boot_fails_for_route_without_access_declaration`, `non_public_route_without_authenticator_fails_boot`, `non_public_route_returns_401_without_principal`, `authorizer_called_with_route_meta` (fake `RouteAuthorizer` returning 403), `guard_runs_before_handler`, `per_user_throttle`, `global_layers_applied_in_slot_order`, `unknown_path_is_404_not_401`, `versioned_controller_mounts_under_v1`, `signed_url_roundtrip_and_expiry`, `serve_command_binds_and_shuts_down_gracefully`, `routes_command_lists_routes`.

### Task 5: navree-log (agent `log`)

**Produces:** `LoggerModule::for_root(LogOptions)` / `from_config()` (section `log`), global. `LogOptions { format: LogFormat (Pretty|Json; default Pretty in debug builds, Json in release), level: String ("info"), levels: BTreeMap<String,String>, file: Option<FileLog { path, rotation: daily|hourly|never, keep_days: 14 }>, redact: Vec<String> (default password, token, authorization, cookie, secret), slow_query_ms: u64 (200) }`. Installs the subscriber once per process (`OnceLock`; a second boot reuses it). Provider `LogControl` with `set_level(target: &str, level: &str) -> Result<()>` via a reload handle. Redacting formatter: any field whose name contains a redact key is written as `[redacted]`. Flushes file appender guard on shutdown (Lifecycle). `pub fn init_for_tests()`.
- [ ] Tests: `redacts_configured_fields` (capture writer), `json_format_has_level_target_fields`, `level_override_per_target`, `set_level_at_runtime`, `second_boot_does_not_panic`, `file_appender_writes`.

### Task 6: navree-crypto (agent `crypto`)

**Produces:** `CryptoModule::for_root(CryptoOptions)` / `from_config()` (section `app`: `key`, `previous_keys`; format `base64:<32 bytes>`), global, exports `Encrypter`, `Hasher`. `Encrypter { encrypt(&self, &[u8]) -> Result<String>, decrypt(&self, &str) -> Result<Vec<u8>>, encrypt_string, decrypt_string, encrypt_json<T: Serialize>, decrypt_json<T: DeserializeOwned> }` — AES-256-GCM, random 96-bit nonce, payload `base64url(json {"kid","iv","value","tag"?})` or compact `v1.<kid>.<nonce>.<ct>`; decrypts with any configured key (rotation). `Hasher { async hash(&self, password: &str) -> Result<String>, async verify(&self, password: &str, hash: &str) -> Result<bool>, needs_rehash(&self, hash: &str) -> bool }` — Argon2id via `spawn_blocking` behind a `Semaphore` (`max_concurrent_hashes`, default = CPUs). Free functions: `random_token(len) -> String` (base62), `sha256_hex(&[u8]) -> String`, `hmac_sha256(key, msg) -> [u8; 32]`, `constant_time_eq`, `generate_key() -> String` (`base64:...`). Command `key:generate` prints a new key.
- [ ] Tests: `encrypt_decrypt_roundtrip`, `decrypt_with_previous_key_after_rotation`, `tampered_ciphertext_fails`, `nonces_unique`, `hash_verify_roundtrip`, `wrong_password_fails`, `needs_rehash_when_params_change`, `hashing_runs_off_runtime_threads` (semaphore bound respected), `key_parse_rejects_wrong_length`.

### Task 7: navree-events (agent `events`)

**Produces:** `EventsModule::for_root()` (global, exports `EventBus`, implements `EventSink`). `pub trait Event: Clone + Serialize + DeserializeOwned + Send + Sync + 'static { const NAME: &'static str; }`; `pub trait Listener<E: Event>: Send + Sync + 'static { fn handle<'a>(&'a self, event: &'a E, ctx: &'a AppContext) -> BoxFuture<'a, Result<(), BoxError>>; fn queued(&self) -> bool { false } }`; `ModuleBuilderEventsExt::listen::<E, L: Listener<E> + Injectable>()` (registers L as provider + contributes a listener registration; queued listeners also contribute a `JobRegistration` named `events.listener.<E::NAME>.<L type name>` and are dispatched through `Arc<dyn Dispatcher>` from the context). `EventBus::emit<E: Event>(&self, e: E) -> Result<(), BoxError>` runs sync listeners in registration order, queues queued ones, logs listener errors (sync listener errors returned as an aggregated error after all run). `EventBus::subscribe_json(name) -> broadcast::Receiver<Value>` for EventSink consumers.
- [ ] Tests: `sync_listener_receives_event`, `listeners_run_in_registration_order`, `queued_listener_dispatches_job_and_job_runs_listener`, `error_in_one_listener_does_not_skip_others`, `event_sink_publish_reaches_json_subscribers`.

### Task 8: navree-view (agent `view`)

**Produces:** `ViewModule::for_root(ViewOptions { dirs: Vec<PathBuf> /* ["resources/views"] */, reload: bool })` / `from_config()` (section `view`), global, exports `Views`. `Views::render(&self, name: &str, ctx: impl Serialize) -> Result<String, ViewError>` — names like `mail/welcome` resolve to `<dir>/mail/welcome.html` (or `.txt`/`.j2`); HTML auto-escaping for `.html`; embedded templates registered via `ModuleBuilderViewExt::templates(&[(name, source)])` (contribution; files override embedded). Filters: `money(cents, currency)`, `date(fmt)`.
- [ ] Tests: `renders_file_template`, `embedded_template_used_when_no_file`, `file_overrides_embedded`, `html_is_escaped`, `missing_template_error_names_template`, `money_filter`.

### Task 9: navree-access engine (agent `access-engine`) — pure, no DB/HTTP yet

**Files:** `crates/navree-access/src/engine/{mod.rs,rule.rs,condition.rs,ability.rs,sql.rs,casl.rs}`; `src/lib.rs` declares `pub mod engine;` and re-exports.

**Produces:**

```rust
pub const READ: &str = "read"; CREATE, UPDATE, DELETE, MANAGE ("manage"); pub const ALL: &str = "all";
pub trait Subject { const SUBJECT: &'static str; const PERMISSION_PREFIX: &'static str; }
pub trait ActionMarker { const NAME: &'static str; } pub struct Read; Create; Update; Delete; Manage;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct Rule { pub action: Vec<String>, pub subject: Vec<String>, #[serde(skip_serializing_if)] pub conditions: Option<Conditions>, pub fields: Option<Vec<String>>, #[serde(default, skip_serializing_if = "is_false")] pub inverted: bool, pub reason: Option<String> }
// CASL JSON: action/subject serialize as a string when single-element, array otherwise
#[derive(Clone, Debug, PartialEq)] pub struct Conditions(pub BTreeMap<String, Cond>); // serde as CASL mongo-query object {"field": value | {"$op": value}}
#[derive(Clone, Debug, PartialEq)] pub enum Cond { Eq(Value), Ne(Value), Lt(Value), Lte(Value), Gt(Value), Gte(Value), In(Vec<Value>), Nin(Vec<Value>) } // multiple ops on one field allowed: store Vec<Cond>
pub fn field(name: &str) -> FieldRef; impl FieldRef { pub fn eq(..) -> Conditions; ne, lt, lte, gt, gte, is_in, not_in } impl Conditions { pub fn and(self, other: Conditions) -> Conditions }
pub struct Rules { /* Vec<Rule> */ }
impl Rules { pub fn new() -> Self; pub fn can(&mut self, action: &str, subject: &str) -> RuleBuilder<'_>; pub fn cannot(&mut self, action: &str, subject: &str) -> RuleBuilder<'_>; pub fn from_permissions(&mut self, perms: &[String], prefix_to_subject: &BTreeMap<String, String>); pub fn build(self) -> Ability; }
impl RuleBuilder<'_> { pub fn when(self, c: Conditions) -> Self; pub fn fields<I: IntoIterator<Item = S>, S: Into<String>>(self, f: I) -> Self; pub fn because(self, reason: &str) -> Self; }
#[derive(Clone, Debug)] pub struct Ability { /* rules */ }
impl Ability {
  pub fn rules(&self) -> &[Rule]; pub fn to_casl_json(&self) -> Value;
  pub fn can(&self, action: &str, subject_type: &str, record: Option<&Value>) -> bool; // record None = type-level check (CASL semantics)
  pub fn can_field(&self, action: &str, subject_type: &str, record: Option<&Value>, field: &str) -> bool;
  pub fn permitted_fields(&self, action: &str, subject_type: &str, record: Option<&Value>, all_fields: &[&str]) -> Vec<String>;
  pub fn has_unconditional(&self, action: &str, subject_type: &str) -> bool; // used by #[permissions]
  pub fn restrict_to(&self, token_abilities: &[String], prefix_to_subject: &BTreeMap<String,String>) -> Ability; // "*" keeps all
  pub fn sql_filter(&self, action: &str, subject_type: &str) -> sea_query::SimpleExpr; // CASE chain, last match wins, FALSE when no rule
}
#[derive(Debug, thiserror::Error)] pub enum RuleError { UnsupportedOperator(String), NestedField(String) } // returned when parsing CASL JSON / building rules
```

Semantics exactly per PRD "Rule semantics (CASL-compatible)": last matching rule wins; `manage` matches any action; `all` matches any subject; `$eq: null` ⇔ IS NULL; `$ne`/`$nin` use IS DISTINCT FROM / NOT IN with NULL handling identical to JS; integers > 2^53 and decimals as strings (equality only); unsupported operators (`$exists`, `$regex`, nested `a.b`) rejected. Type-level `can` (no record) returns true if the last relevant rule for the action/subject — ignoring conditions — is non-inverted or any conditional non-inverted rule exists (CASL behaviour).

- [ ] Tests: `last_matching_rule_wins`, `manage_matches_any_action_all_matches_any_subject`, `inverted_rule_denies`, `conditions_eq_ne_lt_in_nin`, `eq_null_and_ne_null_semantics`, `type_level_check_with_conditional_rule_is_true`, `fields_restrict_permitted_fields`, `from_permissions_maps_prefix_to_subject`, `restrict_to_token_abilities`, `casl_json_roundtrip_matches_casl_format` (snapshot), `rejects_unsupported_operator`, `sql_filter_case_chain_snapshot` (SQLite query builder string), `prop_record_check_equals_sql_filter` (proptest: random rules + random records evaluated by `can` vs executing `sql_filter` against an in-memory SQLite table via `rusqlite`-free path: use `sea-query` + `sqlx` sqlite in dev-dependencies).

### Task 10: navree-cache (agent `cache`)

**Produces:** `CacheModule::for_root(CacheOptions { store: CacheStoreKind::Memory, prefix: String, default_ttl_secs: u64 /* 3600 */ })` / `from_config()` (section `cache`), global, exports `Cache`, `Locks`. `pub trait CacheStore: Send + Sync + 'static { get, put(key, bytes, ttl), forget, flush, add (only if absent), increment }` (BoxFuture). Memory store on moka with per-entry TTL. `Cache { get<T: DeserializeOwned>, put<T: Serialize>(key, &T, ttl), forget, has, remember<T, F, Fut>(key, ttl, f) -> Result<T>, flexible<T>(key, fresh: Duration, stale: Duration, f) /* stale-while-revalidate in background */, tags(&[&str]) -> TaggedCache /* flush by tag via tag version keys */, increment }`. `Locks { lock(name, ttl) -> Lock; Lock::acquire() -> Result<Option<LockGuard>>; Lock::block(timeout) }` with owner tokens; memory implementation. The store is `Arc<dyn CacheStore>` so a database or Redis store can be added.
- [ ] Tests: `put_get_ttl_expiry` (tokio time pause), `remember_computes_once`, `add_only_if_absent`, `tags_flush_only_tagged`, `flexible_serves_stale_and_refreshes`, `lock_exclusive_and_released_on_drop`, `lock_expires_after_ttl`.

### Task 11: navree-http-client (agent `http-client`)

**Produces:** `HttpClientModule::for_root(HttpClientOptions { timeout_secs: 30, connect_timeout_secs: 5, retries: 2, retry_base_ms: 200, ssrf: SsrfPolicy { block_private: true, allow_hosts: Vec<String> }, user_agent })` / `from_config()` (section `http_client`), global, exports `HttpClient`. `HttpClient::{get,post,put,patch,delete}(url) -> RequestBuilder`; `RequestBuilder::{header, bearer, json, form, query, timeout, retries, with_request_id, send() -> Result<Response, HttpClientError>}`; `Response::{status, json::<T>, text, bytes, header}`. Retries (backon, exponential) only for idempotent methods on connect errors/5xx/429. SSRF: custom DNS resolver rejecting loopback, private, link-local, CGNAT, unique-local, and `169.254.169.254`/metadata ranges unless host allow-listed; literal IP URLs checked too. Fakes: `HttpClient::fake() -> (HttpClient, FakeServer)`; `FakeServer::{on(method, url_glob, FakeResponse), recorded() -> Vec<RecordedRequest>, assert_sent(predicate)}`; fake mode never touches the network.
- [ ] Tests: `fake_records_and_responds`, `retries_on_503_then_succeeds` (fake), `does_not_retry_post`, `ssrf_blocks_private_and_metadata_ips`, `ssrf_allows_allowlisted_host`, `request_id_header_propagated`, `timeout_errors` (local `tokio::net::TcpListener` that never responds; allow-list 127.0.0.1).

### Task 12: navree-macros part 1 — DI macros (agent `macros-di`)

**Produces (proc macros; paths resolved with `proc-macro-crate`: prefer `navree` → `::navree::kernel::…`, else `navree-kernel` → `::navree_kernel::…`, and likewise per crate):**
- `#[injectable]` / `#[injectable(scope = "singleton" | "request" | "transient")]` on a struct with named fields: `Arc<T>` fields → `r.get::<T>()`, `#[inject(name = "x")]` → `get_named`, `Lazy<T>` → `r.lazy::<T>()`, `#[inject(default)]` → `Default::default()`. Generates `impl Injectable` with `dependencies()` listing non-lazy keys. Unit structs allowed.
- `#[module(imports = [A, B::for_feature(x)], providers = [P], controllers = [C], exports = [P], global)]` on a struct: generates `impl Module` whose `register` calls `m.import(...)`, `m.provider::<P>()`, `m.controller::<C>()` (via `navree_http::ModuleBuilderHttpExt`), `m.exports::<P>()`, `m.global()`. Expressions allowed in `imports`.
- [ ] Tests in `crates/navree-macros/tests/` (dev-deps: navree-kernel, navree-http; trybuild): `injectable_resolves_fields`, `injectable_named_and_lazy`, `module_registers_imports_providers_exports`, compile-fail `injectable_rejects_tuple_struct`, `module_rejects_unknown_key` with readable messages pointing at the attribute.

### Task 12b: Wave 1 integrator (agent `w1-review`) — as Task 3b, commit `feat: wave 1 platform modules`.

---

## Wave 2 — Data & surface (parallel)

### Task 13: navree-macros part 2 — controller macros (agent `macros-http`)

**Produces:** `#[controller("/prefix")]` / `#[controller("/prefix", version = "1")]` on an `impl` block. Methods with `#[get("/p")]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]` become routes; inert helper attributes on those methods: `#[public]`, `#[roles("a","b")]`, `#[permissions("x.y")]`, `#[can(Action, Subject)]`, `#[can(Action, Subject, from = "id")]` (→ `CanSpec { action: <Action as ActionMarker>::NAME, subject: <Subject as Subject>::SUBJECT, from }` using navree-access paths), `#[guard(Expr)]`, `#[throttle(limit = 10, per = "1m")]`, `#[timeout("5s")]`, `#[transactional]`, `#[audit(action = "x")]`, `#[name("deals.show")]`, `#[api(summary = "...", tag = "Deals", request = T, response = U, responses(404, 422), status = 201)]` (→ `ApiMeta` with `request: Some(::navree_http::schema_fn::<T>)`). `#[public]`/`#[roles]`/`#[permissions]`/`#[can]` may also sit on the `impl` (applies to all routes). Generated: `impl Controller` with `prefix()`, `version()`, `routes(self: Arc<Self>)` building one `RouteDef` per method whose handler is an axum closure capturing `Arc<Self>` and forwarding the method's extractor arguments. Duration strings: `"500ms"`, `"5s"`, `"1m"`, `"1h"`.
- [ ] Compile-fail tests (trybuild): `route_without_access_attribute_is_error` ("route `show` has no access declaration: add #[public], #[roles], #[permissions] or #[can]"), `bad_duration_is_error`, `guard_alone_is_not_access`. Pass tests building a real router via navree-http + fake authenticator: `controller_routes_are_mounted`, `extractors_forwarded`, `impl_level_public_applies`, `api_meta_populated`.

### Task 14: navree-db (agent `db`)

**Produces:**

```rust
pub struct DatabaseModule; impl DatabaseModule { pub fn for_root(opts: DbOptions) -> impl Module; pub fn from_config() -> impl Module; } // global; exports Db; contributes GlobalLayer(Uow) and commands migrate, migrate:rollback, migrate:fresh, migrate:status, db:seed; HealthCheck "database"
#[derive(Clone, Debug, Deserialize)] #[serde(default)] pub struct DbOptions { pub url: String /* "sqlite://storage/app.db?mode=rwc" */, pub max_connections: u32 /* 10 */, pub min_connections: u32, pub connect_timeout_secs: u64, pub log_queries: bool, pub slow_query_ms: u64 /* 200 */, pub migrate_on_boot: bool /* false */ }
#[derive(Clone)] pub struct Db { /* DatabaseConnection */ } impl Db { pub fn conn(&self) -> &DatabaseConnection; pub fn backend(&self) -> DbBackend; pub async fn uow(&self, actor: Option<Uuid>, request_id: Option<String>) -> Uow; pub async fn transaction<F, T>(&self, f: F) -> Result<T, DbErr>; }
#[derive(Clone)] pub struct Uow { /* Arc<inner>: Db, OnceCell<Arc<DatabaseTransaction>>, actor, request_id, changes: Mutex<Vec<ChangeEvent>>, after_commit hooks */ }
impl Uow { pub async fn tx(&self) -> Result<Arc<DatabaseTransaction>, DbErr>; pub fn actor(&self) -> Option<Uuid>; pub fn request_id(&self) -> Option<&str>; pub fn db(&self) -> &Db; pub fn record_change(&self, ev: ChangeEvent); pub fn after_commit(&self, f: impl FnOnce() -> BoxFuture<'static, ()> + Send + 'static); pub async fn commit(&self) -> Result<(), DbErr> /* runs ChangeObservers inside tx, commits, then after_commit hooks */; pub async fn rollback(&self) -> Result<(), DbErr>; pub fn lock_for_update(&self) -> bool /* false on SQLite */; }
impl FromRequestParts for Uow; // from extensions (500 problem if DatabaseModule missing)
#[derive(Clone, Debug, Serialize)] pub struct ChangeEvent { pub entity: String, pub id: String, pub op: ChangeOp, pub old: Option<Value>, pub new: Option<Value> } pub enum ChangeOp { Created, Updated, Deleted }
pub trait ChangeObserver: Send + Sync + 'static { fn observe<'a>(&'a self, tx: &'a DatabaseTransaction, uow: &'a Uow, changes: &'a [ChangeEvent]) -> BoxFuture<'a, Result<(), DbErr>>; } // multi-binding; collected with ctx.get_all::<dyn ChangeObserver>()
pub struct Repo<E>; // generic helpers that record ChangeEvents automatically:
impl<E: EntityTrait> Repo<E> where E::Model: Serialize + IntoActiveModel<E::ActiveModel> { pub async fn insert(uow: &Uow, am: E::ActiveModel) -> Result<E::Model, DbErr>; pub async fn update(uow: &Uow, am: E::ActiveModel) -> Result<E::Model, DbErr>; pub async fn delete(uow: &Uow, model: E::Model) -> Result<(), DbErr>; pub async fn find(uow: &Uow, id: impl Into<Value>) -> Result<Option<E::Model>, DbErr>; }
pub struct MigrationSet { pub module: &'static str, pub after: Vec<&'static str>, pub migrations: Vec<Box<dyn MigrationTrait>> }
pub trait ModuleBuilderDbExt { fn migrations(&mut self, set: MigrationSet) -> &mut Self; fn seeder<S: Seeder + Injectable>(&mut self) -> &mut Self; }
pub trait Seeder: Send + Sync + 'static { fn name(&self) -> &'static str; fn run<'a>(&'a self, ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>>; }
pub struct Migrator; impl Migrator { pub async fn up(ctx: &AppContext) -> Result<Vec<String>, BoxError>; pub async fn rollback(ctx) ; pub async fn fresh(ctx); pub async fn status(ctx) -> Result<Vec<MigrationStatus>, BoxError>; } // table navree_migrations(module, name, batch, applied_at), topo order by `after`, then name; `after` entries naming absent modules are ignored
#[derive(Clone, Copy, Debug, Deserialize)] pub struct PageQuery { pub page: u64, pub per_page: u64 } // Query extractor; defaults 1/15; per_page clamped to 100; page 0 → 422
#[derive(Clone, Debug, Serialize)] pub struct Page<T> { pub data: Vec<T>, pub meta: PageMeta } impl<T> Page<T> { pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U>; }
#[derive(Clone, Debug, Serialize)] pub struct PageMeta { pub current_page: u64, pub per_page: u64, pub total: u64, pub last_page: u64 }
pub async fn paginate<E: EntityTrait, C: ConnectionTrait>(select: Select<E>, conn: &C, q: PageQuery) -> Result<Page<E::Model>, DbErr>;
pub struct CursorQuery { pub cursor: Option<String>, pub limit: u64 } pub struct CursorPage<T> { pub data: Vec<T>, pub next_cursor: Option<String> } pub async fn cursor_paginate<E>(…) // ordered by a UUID v7 primary key column
pub trait Factory: Send + Sync { type Entity: EntityTrait; fn definition(&self, n: usize) -> <Self::Entity as EntityTrait>::ActiveModel; } pub async fn create_many<F: Factory>(f: &F, conn: &impl ConnectionTrait, n: usize) -> Result<Vec<…Model>, DbErr>;
impl From<DbErr> for AppError via navree_http (RecordNotFound → 404, unique violation → 409, else 500)
```

UowLayer (`LayerSlot::Uow`): build `Uow` with actor from `AuthState` and the request id, insert into extensions, run the inner service in `tokio::spawn`, commit when status < 400 else rollback, return the response. SQLite: enable WAL and `busy_timeout=5000`; `foreign_keys=ON`.

- [ ] Tests: `migrations_run_in_dependency_order`, `migrate_is_idempotent_and_status_reports`, `rollback_last_batch`, `fresh_drops_and_reruns`, `uow_commits_on_2xx_and_rolls_back_on_error` (via build_router + test controller), `uow_completes_even_if_client_disconnects` (drop response future; row still committed), `repo_records_change_events_and_observers_run_in_same_tx` (observer insert rolled back with the tx), `paginate_meta`, `page_query_clamps_per_page`, `cursor_pagination`, `seeders_run_via_db_seed`, `unique_violation_maps_to_409`, `health_check_up`.

### Task 15: navree-testing (agent `testing`)

**Produces:** `TestApp::from_builder(AppBuilder).await -> TestApp` (for apps that build their own root, e.g. `crm_api::app(sources)`), `pub fn temp_sqlite_url() -> (String, TempDirGuard)`, `TestApp::builder() -> TestAppBuilder`; `TestAppBuilder::{module(m), config(key, value) /* ConfigSources::set override */, temp_database() /* sets database.url to a fresh temp SQLite file and database.migrate_on_boot = true */, override_value::<T>(Arc<T>), build().await -> TestApp}`. `TestApp::{ctx(), client() -> TestClient, get::<T>() , shutdown()}`. `TestClient` (cookie jar persisted across requests; default headers `Accept: application/json`, `Origin` from config `http.cors.allowed_origins[0]` when set): `get/post/put/patch/delete(path) -> TestRequest`; `TestRequest::{json(&T), header, bearer(token), xsrf() /* reads XSRF-TOKEN cookie, sends X-XSRF-TOKEN */, send().await -> TestResponse}`; `TestClient::csrf_cookie().await` (GET /sanctum/csrf-cookie). `TestResponse::{status(), assert_status(u16) -> Self, json::<T>(), json_value(), assert_json_path("data.0.name", json!(..)), assert_problem(status, code?), header(name), text()}`. `TestAuthenticator` + `TestApp::acting_as(Principal)` (overrides `dyn Authenticator`). `FixedClock` re-export.
- [ ] Tests (using a tiny test module with a hand-written controller): `client_round_trip_json`, `cookies_persist_between_requests`, `xsrf_helper_sends_header`, `acting_as_sets_principal`, `temp_database_is_isolated_per_app`, `assert_json_path_reports_actual_on_failure`.

### Task 16: navree-observe (agent `observe`)

**Produces:** `ObserveModule::for_root(ObserveOptions { metrics: bool, metrics_token: Option<Secret<String>> })` / `from_config()` (section `observe`). Routes `!GET /health/live` (200 `{status:"up"}`), `!GET /health/ready` (aggregates `ctx.get_all::<dyn HealthCheck>()` concurrently with a 2 s timeout each; 200 or 503 with `{status, checks: {name: {status, latency_ms, error?}}}`), `!GET /metrics` (Prometheus text from `metrics-exporter-prometheus`; bearer token required when configured). Installs the recorder once per process.
- [ ] Tests: `live_is_always_up`, `ready_aggregates_checks_and_returns_503_when_one_down`, `slow_check_times_out_as_down`, `metrics_endpoint_exposes_http_counters`, `metrics_token_required_when_configured`.

### Task 17: navree-api (agent `api`)

**Produces:** `ApiModule::for_root(ApiOptions { title, version, servers: Vec<String>, docs_ui: bool })` / `from_config()` (section `api`). OpenAPI 3.1 document built at boot from `navree_http::route_table(ctx)` (schemas come from `navree_http::schema_fn`) — path params from `{x}`, tags, summary, request body (component ref), response schema, listed error responses as problem+json, security scheme (cookie + bearer) on non-public routes. Routes: `GET /api/openapi.json`, `GET /api/docs` (Scalar UI via utoipa-scalar when `docs_ui`). Command `openapi:export [path]`. Resource helpers: `pub trait ApiResource: Serialize { const TYPE: &'static str; }`, `pub fn resource_json<T: ApiResource>(r: &T) -> Value` (adds `"__type"`), `pub fn mask_fields(v: Value, allowed: Option<&[String]>) -> Value` (keeps `id` and `__type`).
- [ ] Tests: `document_lists_paths_with_methods_and_params`, `request_and_response_schemas_registered_as_components`, `non_public_routes_have_security`, `error_responses_reference_problem_schema`, `openapi_export_writes_file`, `resource_json_adds_type`, `mask_fields_keeps_allowed_and_id`.

### Task 18: navree-mail (agent `mail`)

**Produces:** `MailModule::for_root(MailOptions { from: String, transport: TransportConfig /* smtp{url} | log | array | failover[..] */, queue: Option<String> })` / `from_config()` (section `mail`), global, exports `Mailer`. `#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)] pub struct Message { from, to: Vec<String>, cc, bcc, reply_to, subject, html: Option<String>, text: Option<String> }` + builder. `pub trait Mailable: Send + Sync { fn build(&self, views: &Views) -> Result<Message, MailError>; }` with helper `Mailable` impl via `TemplateMail { to, subject, template, context }`. `pub trait MailTransport: Send + Sync + 'static { fn send<'a>(&'a self, m: &'a Message) -> BoxFuture<'a, Result<(), MailError>>; }`: `SmtpTransport` (lettre tokio rustls), `LogTransport`, `ArrayTransport` (`sent() -> Vec<Message>`, `assert_sent(|m| ..)`, `clear()`), `FailoverTransport`. `Mailer::{send(&impl Mailable), send_message(Message), queue(&impl Mailable) /* renders now, dispatches job "mail.send" through Arc<dyn Dispatcher> */}`. Contributes `JobRegistration` for `SendMailJob`.
- [ ] Tests: `send_renders_template_into_message`, `array_transport_records`, `failover_uses_second_on_error`, `queue_dispatches_send_job_and_job_sends` (capturing dispatcher, then run the JobRegistration), `smtp_transport_builds_from_url` (no network).

### Task 19 (wave 3, see below) is navree-session.

### Task 20: navree-realtime (agent `realtime`)

**Produces:** `RealtimeModule::for_root(RealtimeOptions { app_id, key, secret: Secret<String>, allowed_origins: Vec<String>, activity_timeout_secs: 120 })` / `from_config()` (section `realtime`), global, exports `Realtime` (implements `Broadcaster`). Pusher protocol 7 over WebSocket at `!GET /app/{key}` (public route; Origin checked): `pusher:connection_established {socket_id, activity_timeout}`, `pusher:subscribe` / `unsubscribe`, `pusher:ping` → `pusher:pong`, `pusher_internal:subscription_succeeded`, presence `member_added`/`member_removed` with `presence:{ids,hash,count}`, `pusher:error` codes 4001/4009. Private/presence subscription requires `auth = "<key>:<hex hmac_sha256(secret, socket_id:channel[:channel_data])>"`. `!POST /broadcasting/auth` (authenticated; form or JSON `socket_id`, `channel_name`): looks up a channel authorizer registered with `ModuleBuilderRealtimeExt::channel(pattern: &'static str /* "crm.deals", "users.{id}" */, f: fn(&Principal, &HashMap<String,String>, &AppContext) -> BoxFuture<'static, Result<ChannelDecision, BoxError>>)`, `ChannelDecision::{Deny, Allow, Presence(Value /* user_info */)}`; returns `{auth, channel_data?}` or 403. `Realtime::broadcast(channels, event, data)` fans out in-process. `Realtime::disconnect_user(user_id)`.
- [ ] Tests (tokio-tungstenite client against `axum::serve` on 127.0.0.1:0): `connect_receives_connection_established`, `public_subscribe_and_receive_broadcast`, `private_subscribe_requires_valid_auth_signature`, `auth_endpoint_signs_for_authorized_channel_and_403_otherwise`, `presence_member_added_and_removed`, `ping_pong`, `rejects_disallowed_origin`, `disconnect_user_closes_sockets`.

### Task 21: navree-storage (agent `storage`)

**Produces:** `StorageModule::for_root(StorageOptions { default: String, disks: BTreeMap<String, DiskConfig /* local{root, public_url?} | memory */> })` / `from_config()` (section `storage`), global, exports `Storage`. `Storage::{disk(name) -> Result<Disk>, default_disk()}`; `Disk::{put(path, Bytes), put_stream, get(path) -> Bytes, get_stream, exists, delete, list(prefix) -> Vec<String>, size, url(path), temporary_url(path, expires_at) -> String}` (object_store `LocalFileSystem`/`InMemory`); paths normalised, `..` rejected. Temporary URLs signed with `navree_crypto::hmac_sha256` over `disk/path/expires`, served by `!GET /storage/{disk}/{*path}` (verifies signature/expiry, streams with content type from `mime_guess`). Upload helper: `pub async fn store_upload(field: axum::extract::multipart::Field<'_>, disk: &Disk, dir: &str, rules: UploadRules { max_bytes, allowed_mime: Vec<&str> }) -> Result<StoredFile, AppError>` — streams, sniffs MIME with `infer`, 413/422 on violation.
- [ ] Tests: `put_get_delete_local_and_memory`, `rejects_path_traversal`, `temporary_url_valid_then_expired`, `upload_rejects_wrong_mime_and_too_large`, `list_prefix`.

### Task 21b: Wave 2 integrator (agent `w2-review`) — as Task 3b; commit `feat: wave 2 data, testing, api, mail, realtime, storage`.

---

## Wave 3 — Capabilities (parallel)

### Task 19: navree-session (agent `session`)

**Produces:** `SessionModule::for_root(SessionOptions { store: SessionStoreKind /* memory | database */, cookie: String /* "navree_session" */, lifetime_minutes: 120, secure: bool, same_site: "lax", domain: Option<String>, csrf_except: Vec<String> /* path globs */ })` / `from_config()` (section `session`). Database store table `sessions(id TEXT PK, data TEXT, expires_at TIMESTAMP)` via `MigrationSet { module: "session" }`. Contributes `GlobalLayer(LayerSlot::Session)`: reads the encrypted session cookie (`Encrypter`), loads the record, inserts `Session`, enforces CSRF, saves when dirty and sets cookies (HttpOnly, SameSite, Secure per options). `Session` (extractor, cheap clone): `id()`, `get<T>(key)`, `insert<T>(key, &T)`, `remove(key)`, `regenerate()` (new id, keep data, delete old record), `invalidate()` (new id, empty data), `csrf_token()` (created lazily, 40 chars), `flush()`. CSRF rule: for POST/PUT/PATCH/DELETE when the request carried the session cookie and no `Authorization: Bearer` header, require `X-XSRF-TOKEN` (URL-decoded) or `X-CSRF-TOKEN` equal to the session token, else 419 problem; `csrf_except` paths skip. Route `!GET /sanctum/csrf-cookie` (public) → 204 and `XSRF-TOKEN` cookie (not HttpOnly, path `/`, SameSite Lax). Implements `SessionStore` for memory and database.
- [ ] Tests: `session_persists_across_requests`, `cookie_is_encrypted_httponly_samesite`, `csrf_cookie_endpoint_sets_xsrf_token`, `post_with_session_without_xsrf_is_419`, `post_with_matching_xsrf_passes`, `bearer_request_skips_csrf`, `get_is_never_csrf_checked`, `csrf_except_path_skips`, `regenerate_changes_id_keeps_data`, `invalidate_clears`, `expired_session_is_new`, `database_store_roundtrip`.

### Task 22: navree-access module (agent `access-module`)

**Produces:** `AccessModule::for_root(AccessOptions { abilities: fn(&AbilityCtx, &mut Rules), super_admin_role: Option<String> })`, global, exports `AccessService`, `AbilityFactory`; registers `dyn RouteAuthorizer`. `AccessModule::for_feature::<E>()` registers a subject loader for `E: EntityTrait + Subject` (primary key `Uuid`, `E::Model: Serialize + Clone + Send + Sync`). Tables (`MigrationSet { module: "access", after: ["auth"] }`; FK to `users.id` only when the table exists — use no FK to keep access usable without navree-auth): `roles(id, name UNIQUE, description, created_at)`, `permissions(id, name UNIQUE)`, `role_permissions(role_id, permission_id)`, `user_roles(user_id, role_id, team_id NULL)`, `user_permissions(user_id, permission_id)`, `access_meta(key PK, version)`. `AbilityCtx { user: Option<AccessUser { id: Uuid, roles: Vec<String> }>, permissions: Vec<String>, team_id: Option<Uuid> }` with `user.has_role(&str)`. `AccessService::{create_role, delete_role, grant(role, perm), revoke, sync_role_permissions, assign_role(user, role), remove_role, sync_user_roles, roles_of(user), permissions_of(user), version()}` (all bump `access_meta.version` in the same transaction and write `AuditKind::Access` entries through `dyn AuditSink` when present). `AbilityFactory::for_principal(&Option<Principal>) -> Result<Arc<Ability>>` cached in moka by (user, team, version); token principals are restricted with `Ability::restrict_to`. RouteAuthorizer: roles → any-of match (super admin passes); permissions → `has_unconditional`; `can` without `from` → type-level check; `can` with `from` → loads record via the subject loader inside the request `Uow` (`FOR UPDATE` when the backend supports it), 404 if missing, 403 if denied, then stores it for `Authorized<E, A>`. Denials write `AuditKind::Security` entries (`access.denied`). Extractors: `Abilities(Arc<Ability>)`, `Authorized<E, A>` (`into_inner() -> E::Model`, `check_fields(&self, changed: &[&str]) -> Result<(), AppError>`), `Accessible<E, A>` (`apply(self, select: Select<E>) -> Select<E>` using `sql_filter`). Route `GET /api/me/abilities` (authenticated; CASL JSON; `ETag` = `"<user>-<version>"`; 304 on `If-None-Match`). Management routes under `/api/access` (permission `roles.manage`): list roles with permissions, create role, sync role permissions, sync user roles. Commands `access:role <name>`, `access:grant <role> <perm>`, `access:explain <user> <action> <subject> [id]`.
- [ ] Tests: `role_route_allows_matching_role_and_denies_other`, `permission_route_requires_unconditional_rule`, `can_from_loads_record_and_checks_conditions` (owner vs non-owner → 200/403), `can_from_missing_record_is_404`, `accessible_filters_list_to_permitted_rows`, `record_check_and_list_filter_agree` (Review Focus 5, SQL against SQLite), `token_principal_restricted_to_token_abilities`, `abilities_endpoint_returns_casl_json_with_etag_and_304`, `version_bump_invalidates_cached_ability`, `denial_writes_security_audit_entry`, `super_admin_role_passes_everything`, `explain_command_prints_matching_rule`.

### Task 23: navree-validation (agent `validation`)

**Produces:** `ValidationModule::for_root()` (global). `Valid<T>` extractor wrapping `Json<T>`, `Query<T>`, `Form<T>` where `T: garde::Validate<Context = ()> + DeserializeOwned`: deserialization errors → 422 with field errors (serde path) and validation failures → 422 `errors` map `{ "field.path": ["message"] }` (Laravel-style messages: "The email field must be a valid email address." etc., message table in `messages.rs`, overridable via `ValidationOptions { messages }`). Async database rules: `pub trait AsyncRules { fn async_rules(&self) -> Vec<AsyncRule>; }` with `AsyncRule::unique(field, table, column, value, ignore_id: Option<Uuid>)`, `AsyncRule::exists(field, table, column, value)`; `ValidAsync<T>` runs garde first, then async rules inside the request `Uow` (parameterised `SELECT 1 … LIMIT 1` through sea-query; table/column identifiers validated against `[a-z_][a-z0-9_]*`). Extra rules as garde custom fns: `confirmed(field)` helper (`password_confirmation`), `rules::uuid`, `rules::date_ymd`, `rules::one_of(&[..])`. Re-export `garde`.
- [ ] Tests: `invalid_json_is_422_with_body_error`, `field_errors_use_laravel_messages`, `nested_field_paths`, `unique_rule_rejects_existing_and_ignores_self`, `exists_rule`, `rejects_bad_identifier_in_rule`, `confirmed_rule`.

### Task 24: navree-queue (agent `queue`)

**Produces:** `QueueModule::for_root(QueueOptions { driver: QueueDriver /* database | memory */, default_queue: "default", workers: 4, poll_interval_ms: 1000, queues: Vec<String> })` / `from_config()` (section `queue`), global, exports `Queue`, registers `dyn Dispatcher`. Tables (`MigrationSet { module: "queue" }`): `jobs(id, queue, job_type, payload, attempts, max_attempts, available_at, reserved_at, reserved_by, unique_key UNIQUE NULL, created_at)`, `failed_jobs(id, queue, job_type, payload, error, attempts, failed_at)`. `Queue::{dispatch<J: Job>(job) -> Result<String>, dispatch_in<J>(&Uow, job) /* same transaction (database) or after_commit (memory) */, later<J>(job, delay), work(opts: WorkOptions { queues, once, max_jobs, stop_when_empty }) -> Result<WorkStats>, failed() -> Vec<FailedJob>, retry(id | all), flush_failed(), pending_count(queue)}`. Reservation is atomic: SQLite `UPDATE … WHERE id = (SELECT … LIMIT 1) RETURNING *`; Postgres adds `FOR UPDATE SKIP LOCKED`. Retries with `Backoff::delay_for`; per-job timeout (`tokio::time::timeout`); stale reservations older than timeout released; after `max_attempts` → `failed_jobs`. Unique jobs: `unique_for_secs` → `unique_key = job_type + sha256(payload)`, duplicate dispatch while pending returns the existing id. Job handlers come from every `JobRegistration` contribution; unknown job types fail. Logs at target `navree::job` (name, attempt, duration_ms, outcome). Commands `queue:work [--queue q] [--once]`, `queue:failed`, `queue:retry <id|all>`, `queue:flush`. Testing: `QueueFake` (`dispatched::<J>() -> Vec<J>`, `assert_dispatched::<J>(pred)`), `Queue::run_pending()` drains synchronously in tests.
- [ ] Tests: `dispatch_and_work_runs_handler`, `dispatch_in_uow_rolled_back_is_not_queued`, `retry_with_backoff_then_fail_to_failed_jobs`, `timeout_counts_as_failure`, `unique_job_deduplicated`, `delayed_job_not_reserved_early`, `concurrent_workers_never_run_same_job` (two workers, 50 jobs, each handled once), `stale_reservation_released`, `retry_failed_requeues`, `memory_driver_parity` (same suite).

### Task 25: navree-audit (agent `audit`)

**Produces:** `AuditModule::for_root(AuditOptions { enabled: true, retention_days: 365, skip_entities: Vec<String>, skip_fields: Vec<String> /* default password, password_hash, token, token_hash, secret, two_factor_secret, two_factor_recovery_codes, remember_token */ })` / `from_config()` (section `audit`), global, exports `AuditLog`; registers `dyn AuditSink` (writes on its own connection, outside the request transaction) and multi-binds `dyn ChangeObserver` (writes `AuditKind::Data` rows in the same transaction, with actor and request id from the `Uow`, dropping skipped fields; `old_values`/`new_values` only contain changed fields on update). Table `audit_logs(id, kind, action /* "<entity>.created|updated|deleted" or custom */, actor_id, subject_type, subject_id, old_values, new_values, request_id, ip, user_agent, metadata, created_at)` with indexes on (subject_type, subject_id), actor_id, created_at. Route `GET /api/audit` (permission `audit.read`; filters `actor_id, subject_type, subject_id, action, kind`; paginated, newest first). Command `audit:prune [--days N]`.
- [ ] Tests: `data_change_writes_audit_row_in_same_tx`, `rolled_back_change_leaves_no_audit_row`, `skipped_fields_never_stored`, `update_records_only_changed_fields`, `security_entry_survives_rollback`, `list_endpoint_filters_and_paginates`, `prune_deletes_older_than_retention`.

### Task 26: navree-schedule (agent `schedule`)

**Produces:** `ScheduleModule::for_root()` (global, exports `Scheduler`). `ModuleBuilderScheduleExt::schedule(f: impl FnOnce(&mut Schedule))`. `Schedule::{job<J: Job + Clone>(job: J) -> TaskBuilder /* dispatches via dyn Dispatcher */, task(name: &'static str, f: fn(AppContext) -> BoxFuture<'static, Result<(), BoxError>>) -> TaskBuilder, command(name, args) -> TaskBuilder}`; `TaskBuilder::{cron(expr), every_minute, every_five_minutes, every_fifteen_minutes, hourly, hourly_at(min), daily, daily_at("HH:MM"), weekly, without_overlapping, on_one_server, name(&str)}` (UTC; croner). Table `schedule_runs(task, tick, started_at, finished_at, status, error, PRIMARY KEY(task, tick))`; a tick runs only if `INSERT … ON CONFLICT DO NOTHING` inserted the row (single node per tick); overlap prevention via a running-row check + in-process mutex. `Scheduler::{tick(now: DateTime<Utc>) -> Result<Vec<RunOutcome>>, list(now) -> Vec<(name, expr, next_run)>}` using the `dyn Clock` (default `SystemClock`). Commands `schedule:run` (one tick), `schedule:work` (loop aligned to minute boundaries until shutdown), `schedule:list`.
- [ ] Tests (FixedClock): `daily_at_due_only_at_that_minute`, `cron_expression_parsed`, `same_tick_runs_once_across_two_schedulers`, `without_overlapping_skips_while_running`, `job_task_dispatches_job`, `failed_task_records_error`, `list_shows_next_run`.

### Task 27: navree-notify (agent `notify`)

**Produces:** `NotifyModule::for_root(NotifyOptions { queue: bool })`, global, exports `Notifier`. `pub trait Notifiable: Send + Sync { fn notifiable_type(&self) -> &'static str; fn notifiable_id(&self) -> String; fn route_mail(&self) -> Option<String>; fn broadcast_channel(&self) -> Option<String> /* default "private-users.{id}" */; }`. `pub trait Notification: Serialize + DeserializeOwned + Send + Sync + 'static { const TYPE: &'static str; fn via(&self, n: &dyn Notifiable) -> Vec<Channel>; fn to_mail(&self, n: &dyn Notifiable) -> Option<TemplateMail>; fn to_database(&self, n: &dyn Notifiable) -> Option<Value>; fn to_broadcast(&self, n: &dyn Notifiable) -> Option<Value>; }`, `enum Channel { Mail, Database, Broadcast }`. `Notifier::{send(&n, notification), send_now(&n, notification)}` (queued via `dyn Dispatcher` when `queue` is on; job `notify.send` carries notifiable type/id/mail/channel + notification JSON + type). Database channel table `notifications(id, notifiable_type, notifiable_id, type, data, read_at, created_at)`; broadcast via `dyn Broadcaster` (event `notification.created`). Routes (authenticated; notifiable = current user, type `user`): `GET /api/notifications?unread=`, `GET /api/notifications/unread-count`, `POST /api/notifications/{id}/read`, `POST /api/notifications/read-all`.
- [ ] Tests: `database_channel_stores_notification`, `mail_channel_sends_via_mailer_array_transport`, `broadcast_channel_calls_broadcaster`, `queued_send_dispatches_job_and_job_delivers`, `endpoints_list_count_and_mark_read_only_own`.

### Task 27b: Wave 3 integrator (agent `w3-review`) — commit `feat: wave 3 session, access, validation, queue, audit, schedule, notify`.

---

## Wave 4 — Identity & tooling

### Task 28a: NestJS error format across the framework (agent `errors`)

User decision (spec §10): replace RFC 9457 problem+json with the NestJS error format everywhere.

**Owns:** `crates/navree-http/src` error rendering and every crate's tests/assertions that depend on the error body or on 422/419 (navree-http, navree-validation, navree-session, navree-db, navree-access, navree-audit, navree-api, navree-notify, navree-realtime, navree-storage, navree-testing, navree-observe, navree-macros, navree-mail, navree-kernel, navree-contracts as needed). Do not touch navree-auth or navree-cli (other agents are building them now).

**Produces:**
- `AppError` keeps its Rust API (constructors, `with_detail`, `with_header`, `with_source`) but renders `{"statusCode", "message", "error"}` (+ `"errors"` for validation) with `application/json`. `message` = the detail when given, else the reason phrase; `error` = the reason phrase. `AppError::validation(errors)` → 400 with `message` = every field message flattened in field order (each prefixed by nothing; messages already read like "email must be a valid email address") and `errors` = the map. `AppError::csrf_mismatch()` → 403 "Invalid CSRF token". `AppError::internal` → 500 "Internal server error" (source logged, never rendered). Replace the `problem()` method with `fn body(&self) -> serde_json::Value`.
- navree-validation: messages become NestJS/class-validator style sentences without the Laravel "The x field" phrasing (e.g. "email must be a valid email address", "name should not be empty", "name must be shorter than or equal to 255 characters"); status 400.
- navree-api: the OpenAPI error component becomes `HttpError {statusCode: integer, message: string | string[], error: string, errors?: object}` and documented error responses use 400 instead of 422.
- navree-testing: `TestResponse::assert_error(status: u16, message: Option<&str>)` replaces `assert_problem` (keep `assert_problem` as a deprecated alias calling it); `assert_validation_error(field)` asserts 400 and a key in `errors`.
- Rename tests whose names mention problem/422/419 (e.g. `problem_json_shape_and_content_type` → `nest_error_shape_and_content_type`, `invalid_json_is_422_with_body_error` → `invalid_json_is_400_with_body_error`, `post_with_session_without_xsrf_is_419` → `post_with_session_without_xsrf_is_403`).
- [ ] TDD: first change the navree-http shape test to the new format and watch it fail; implement; then sweep every crate (`grep -rn "problem\|422\|419\|UNPROCESSABLE" crates`) updating code and tests; `cargo test --workspace` (excluding nothing) green, clippy clean.

### Task 28: navree-auth core — NestJS-style JWT auth (agent `auth-core`)

> **Direction change (user, 2026-10-25): auth and the API follow the NestJS approach, not Laravel Sanctum.** No Sanctum SPA cookie flow, no `/sanctum/csrf-cookie`, no personal access tokens, no session-based login. Authentication is stateless JWT bearer access tokens with rotating refresh tokens, Passport-style strategies, a global guard with `#[public]` opt-out (navree-http's deny-by-default already is NestJS's global `APP_GUARD` + `@Public()` pattern), and a `CurrentUser` extractor (NestJS `@CurrentUser()`). navree-auth does not use navree-session; navree-session stays an optional module.

**Produces:** `AuthModule::for_root(AuthOptions)` / `from_config()` (section `auth`), global, exports `AuthService`, `JwtService`; registers `dyn Authenticator` (runs the registered strategies in order).

```rust
#[derive(Clone, Debug, Deserialize)] #[serde(default)] pub struct AuthOptions { pub jwt: JwtOptions, pub registration: bool /* true */, pub login_throttle_per_minute: u32 /* 5 */, pub password_min_length: usize /* 8 */, pub routes_prefix: String /* "/auth" */ }
#[derive(Clone, Debug, Deserialize)] #[serde(default)] pub struct JwtOptions { pub secret: Option<Secret<String>> /* falls back to config "app.key" */, pub issuer: String /* "navree" */, pub audience: Option<String>, pub access_ttl_secs: u64 /* 900 */, pub refresh_ttl_secs: u64 /* 2_592_000 */ }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)] pub struct Claims { pub sub: Uuid, pub iat: i64, pub exp: i64, pub iss: String, #[serde(skip_serializing_if = "Option::is_none")] pub aud: Option<String>, pub jti: String, pub typ: TokenType }
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)] #[serde(rename_all = "snake_case")] pub enum TokenType { Access, TwoFactorChallenge }
impl JwtService { pub fn sign(&self, user_id: Uuid, typ: TokenType, ttl: Duration) -> Result<String, AuthError>; pub fn verify(&self, token: &str, expected: TokenType) -> Result<Claims, AuthError>; }
// HS256 via jsonwebtoken 11 with default-features = false, features = ["rust_crypto"] (no aws-lc); leeway 30 s
pub trait Strategy: Send + Sync + 'static { fn name(&self) -> &'static str; fn authenticate<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<Option<Principal>, AppError>>; }
// Passport-style; multi-binding `dyn Strategy`; built-in JwtStrategy ("jwt": `Authorization: Bearer <access token>`) → Principal { via: AuthVia::Token, token_abilities: None, session_id: None }.
// A present-but-invalid/expired bearer token → 401 (never silently anonymous).
#[derive(Clone, Debug, Serialize)] pub struct TokenPair { pub access_token: String, pub refresh_token: String, pub token_type: &'static str /* "Bearer" */, pub expires_in: u64 }
impl AuthService {
    pub async fn register(&self, uow: &Uow, input: NewUser) -> Result<User, AuthError>;
    pub async fn validate_user(&self, email: &str, password: &str) -> Result<User, AuthError>; // LocalStrategy
    pub async fn issue_tokens(&self, user: &User, meta: ClientMeta) -> Result<TokenPair, AuthError>;
    pub async fn refresh(&self, refresh_token: &str, meta: ClientMeta) -> Result<TokenPair, AuthError>;
    pub async fn logout(&self, refresh_token: Option<&str>, user_id: Uuid, all: bool) -> Result<(), AuthError>;
    pub async fn find_user(&self, id: Uuid) -> Result<Option<User>, AuthError>;
    pub async fn create_user(&self, name: &str, email: &str, password: &str) -> Result<User, AuthError>; // seeders
    pub async fn update_password(&self, user_id: Uuid, new_password: &str) -> Result<(), AuthError>;
}
pub struct CurrentUser(pub User); // extractor (NestJS @CurrentUser()): loads the user for the Principal; 401 if none
```

Refresh tokens are opaque (`<id>.<48 random base62>`), stored as SHA-256 in `refresh_tokens(id, user_id, family_id, token_hash UNIQUE, expires_at, revoked_at, replaced_by, ip, user_agent, created_at)`; every refresh rotates (old row revoked and `replaced_by` set), and presenting an already-revoked token revokes the whole family (reuse detection). Tables (`MigrationSet { module: "auth" }`): `users(id, name, email UNIQUE, password_hash, email_verified_at, two_factor_secret, two_factor_recovery_codes, two_factor_confirmed_at, created_at, updated_at)`, `refresh_tokens`, `password_reset_tokens(email PK, token_hash, created_at)`. Public entity module `navree_auth::user::{Entity, Model, Column}` (`password_hash` and 2FA columns never serialised), `impl Subject for user::Entity { SUBJECT = "User", PERMISSION_PREFIX = "users" }`, `UserResource` (`{__type:"User", id, name, email, email_verified_at, created_at}`). Argon2id via `Hasher`, rehash on login when `needs_rehash`.

Routes under `{api_prefix}{routes_prefix}` (NestJS docs style):
- `POST /auth/register` (public; 404 when disabled) → 201 `{user, access_token, refresh_token, token_type, expires_in}`
- `POST /auth/login` (public; body `{email, password}`; throttle 5/min per email+IP → 429) → 200 `{user, access_token, refresh_token, token_type, expires_in}`; bad credentials → 401 "Invalid credentials" (UnauthorizedException style)
- `POST /auth/refresh` (public; body `{refresh_token}`) → 200 new pair; invalid, expired or reused → 401
- `POST /auth/logout` (authenticated; body `{refresh_token?, all?: bool}`) → 204
- `GET /auth/me` (authenticated) → `UserResource`

Security audit entries: `auth.login`, `auth.login_failed`, `auth.logout`, `auth.refresh`, `auth.refresh_reuse_detected`.

- [ ] Tests: `register_hashes_password_and_returns_tokens`, `login_returns_token_pair_and_user`, `bad_credentials_401_and_audited`, `login_throttled_after_5_attempts`, `bearer_access_token_sets_principal`, `expired_or_tampered_access_token_is_401`, `refresh_rotates_and_old_token_stops_working`, `reused_refresh_token_revokes_family`, `logout_revokes_refresh_token`, `logout_all_revokes_every_session`, `me_401_when_guest`, `public_route_works_without_token`, `refresh_token_hash_only_at_rest`, `password_hash_never_serialized`, `custom_strategy_via_multi_binding` (an API-key strategy registered by a test module).

### Task 29: navree-cli (agent `cli`)

**Produces:** binary `navree` (clap 4): `navree new <name> [--ui next|none] [--navree-path <path>]` (app crate with `src/main.rs` root module, `src/modules/users`, `.env.example`, `config/app.toml`, `migrations` example, `tests/`), `navree make:module <name> [--crud] [--path src/modules]` (mod.rs, controller.rs, service.rs, entity.rs, dto.rs, migrations/, tests/ — matching PRD "Anatomy of a feature module" and using the facade macros), `navree make:{controller,service,entity,migration,job,event,listener,mail,notification,policy,command,test} <module> <name>`, `navree dev` (watch with `notify`, rebuild and restart `cargo run -- serve`, debounced), and forwarding of runtime commands (`migrate`, `migrate:*`, `db:seed`, `queue:*`, `schedule:*`, `routes`, `check`, `openapi:export`, any `x:y`) to `cargo run --quiet -- <cmd> <args>`. Templates via minijinja embedded with `include_str!`.
- [ ] Tests: snapshot (insta) of `make:module --crud` output, `new` creates expected tree, name validation (`snake_case`), forwarding builds the right cargo invocation (command builder unit test), an `#[ignore]` test that `navree new` output compiles against the workspace.

### Task 29b: Wave 4 integrator (agent `w4-review`) — commit `feat: auth core and navree cli`.

---

## Wave 5 — Assembly

### Task 30: navree-auth extras — NestJS-style account flows (agent `auth-extras`)

Adds to navree-auth (all JSON, bearer-token based, no sessions):
- `POST /auth/forgot-password {email}`: always 202; mail via `Mailer` when registered; token SHA-256 at rest, 60 min expiry.
- `POST /auth/reset-password {email, token, password}`: 400 on bad/expired token; revokes all refresh tokens.
- `POST /auth/email/verification` (authenticated; sends a signed link) and `GET /auth/email/verify/{id}/{hash}` (`UrlSigner`).
- `PATCH /auth/me/password {current_password, password}`, `PATCH /auth/me {name?, email?}`.
- TOTP two-factor (totp-rs): `POST /auth/2fa/enable` → `{secret, otpauth_url, qr_svg}` (secret encrypted with `Encrypter`); `POST /auth/2fa/confirm {code}` → `{recovery_codes}`; `POST /auth/2fa/disable {password}`; `POST /auth/2fa/recovery-codes` (regenerate). When 2FA is on, `POST /auth/login` returns `{two_factor_required: true, challenge_token}` (a short-lived JWT with `typ = two_factor_challenge`, 5 min) and `POST /auth/2fa/verify {challenge_token, code | recovery_code}` returns the token pair; recovery codes are single-use.
- Audit entries for each.

- [ ] Tests: `forgot_password_always_202_and_sends_mail_for_known_email`, `reset_password_with_valid_token_revokes_refresh_tokens`, `reset_token_expires`, `verify_email_signed_link`, `two_factor_enable_confirm_and_login_challenge_flow`, `challenge_token_cannot_be_used_as_access_token`, `recovery_code_single_use`, `wrong_totp_code_rejected`, `update_password_requires_current`.

### Task 31: navree facade + full-stack tests (agent `facade`)

**Files:** `crates/navree/src/lib.rs`, `crates/navree/Cargo.toml` (features: `default = ["full"]`, one feature per module crate), `crates/navree/tests/{fullstack_blog.rs, macros_end_to_end.rs, dependency_graph.rs}`.

**Produces:** `pub use navree_kernel::{self as kernel, Navree, Module, ...}`, `pub mod http { pub use navree_http::*; }` (and one module per crate: `config, log, crypto, db, validation, session, auth, access, audit, cache, events, queue, schedule, view, mail, notify, storage, http_client, api, observe, realtime, testing, contracts`), macros re-exported at the root (`navree::module`, `navree::injectable`, `navree::controller`, …), `pub mod prelude` (common types: `Navree, Module, Injectable, Arc, AppError, Result, Json, Created, NoContent, Uow, Page, PageQuery, Valid, CurrentUser, Authorized, Accessible, Abilities, Read, Create, Update, Delete, Manage, Subject, ActionMarker, Rules, AbilityCtx, field`, the module types), `pub type Result<T, E = navree_http::AppError>`.
- [ ] Tests: `fullstack_blog` — a mini app using **only** the facade and macros (posts module with `#[module]`, `#[injectable]`, `#[controller]`, SeaORM entity, migration, `#[can(Update, Post, from = "id")]`, `Accessible`, validation, queued mail on publish, audit, JWT login + refresh) exercised through `TestApp`: guest list sees only published posts, author updates own post, other user gets 403, invalid input 400 (spec §10), publish queues mail, audit rows written, `/api/openapi.json` lists the routes, `/api/me/abilities` matches CASL format; `check` command passes; a wiring mistake fails boot with a readable error.

### Task 31b: Wave 5 integrator (agent `w5-review`) — commit `feat: navree facade, auth extras, full-stack tests`.

---

## Wave 6 — CRM API (`apps/crm-api`)

### Wave 6 layout (amended 2026-10-25, supersedes the file lists in Tasks 32–36)

The CRM API is split into crates so that three agents can build feature modules in parallel without breaking each other's compiles (PRD: "a module can ship as its own crate"). Root `Cargo.toml` members gain `apps/crm-api/crates/*`. Every CRM crate uses only the `navree` facade and its macros, as an app would. Auth is NestJS-style JWT and errors use the NestJS format (spec §10). Pagination keeps the `{data, meta: {current_page, per_page, total, last_page}}` shape.

| Crate | Owner | Contents |
|---|---|---|
| `apps/crm-api/crates/crm-core` | crm-scaffold | SeaORM entities for `companies`, `contacts`, `deals`, `activities` (UUID v7 keys, `owner_id` → users, timestamps, `deleted_at` on companies/contacts); enums `CompanySize`, `ContactStatus`, `Stage` (with `probability()` and `label()`), `ActivityKind`, `SubjectType`; one `MigrationSet { module: "crm", after: ["auth", "access"] }` creating all CRM tables and indexes; `Subject` impls (`Company`/companies, `Contact`/contacts, `Deal`/deals, `Activity`/activities) and action markers `Move` ("move"), `Export` ("export"), `Complete` ("complete"); `abilities::define` implementing spec §6 + §9 exactly; role and permission constants; embed types `OwnerRef {id, name}`, `CompanyRef {id, name}`, `ContactRef {id, full_name}`; `CrmCoreModule` (registers the migrations and `AccessModule::for_feature` for every CRM entity and for `navree_auth::user::Entity`); `platform(builder: AppBuilder, sources: ConfigSources) -> AppBuilder` adding every platform module the CRM uses (Config, Logger, Crypto, Http, Database, Cache, Events, View, Mail, Queue, Schedule, Validation, Auth with `registration = false`, Access with `abilities::define` and super-admin role `admin`, Audit, Notify, Realtime with key `navree-crm-key` and channels `crm.deals` / `users.{id}`, Api, Observe) |
| `apps/crm-api/crates/crm-testkit` | crm-scaffold | `TestCrm::builder().module(m).build().await -> TestCrm` (temp SQLite, platform + `CrmCoreModule` + the given feature modules, mail array transport, queue memory or database driver, fast hashing, per-IP limit off, `FixedClock` override), seeds roles/permissions and the four demo users (admin, manager, rep1, rep2; password `password123`); `TestCrm::login_as(Role) -> TestClient` (real `POST /api/auth/login`, then bearer); `TestCrm::user_id(Role)`, `TestCrm::clock()`, `TestCrm::run_queue()` |
| `apps/crm-api/crates/crm-users` | crm-scaffold | `UsersModule`: `GET /api/v1/me`, `GET/POST/PATCH /api/v1/users` (spec §7), and a `UserResourceExtension` adding `roles` to `/api/auth/me` and the login `user` |
| `apps/crm-api/crates/crm-companies` | crm-companies | `CompaniesModule`, `ContactsModule` (Task 33) |
| `apps/crm-api/crates/crm-deals` | crm-deals | `DealsModule` (Task 34): events `DealAssigned`, `DealWon`; queued listener sending `DealNotification` (database + mail); `deal.updated` broadcasts |
| `apps/crm-api/crates/crm-activities` | crm-activities | `ActivitiesModule`, `DashboardModule`, the daily `crm:task-reminders` schedule, `audit:prune` daily (Task 35) |
| `apps/crm-api` (bin `crm-api` + lib `crm_api`) | crm-scaffold, then crm-integrate | `crm_api::app(sources) -> AppBuilder` = `crm_core::platform` + every feature module; `main.rs`; the `crm-demo` seeder (spec §6 volumes); `.env.example`, `config/app.toml` (dev: SQLite `storage/crm.db`, `http.addr = "127.0.0.1:8080"`, realtime allowed origin `http://localhost:3000`, `auth.password_reset.url = "http://localhost:3000/reset-password"`, `app.url = "http://localhost:3000"`); end-to-end tests across modules |

Before the feature agents start, the scaffold leaves every crate compiling, with empty feature modules already wired into `crm_api::app`, so the feature agents only fill in their own crate.



### Task 32: CRM scaffold (agent `crm-scaffold`)

**Files:** `apps/crm-api/{Cargo.toml, .env.example, config/app.toml, src/main.rs, src/app.rs (pub fn app() -> AppBuilder used by main and tests), src/abilities.rs, src/common/{mod.rs, owner.rs (OwnerRef {id,name}), stages.rs}, src/modules/{users,companies,contacts,deals,activities,dashboard}/mod.rs, src/seed.rs, tests/support/mod.rs}`.

**Produces:** `crm_api::app(sources: ConfigSources) -> AppBuilder` importing Config, Logger, Crypto, Http (`api_prefix "/api"`, CORS `http://localhost:3000`), Database, Cache, Events, View, Mail (log transport in dev, array in tests), Queue (database), Schedule, Session (database store, `secure=false` in dev), Validation, Auth (`registration: false`), Access (`abilities::define`, `super_admin_role: "admin"`), Audit, Notify, Realtime (key `navree-crm-key`; channels `crm.deals`, `users.{id}`), Api, Observe, and the six feature modules. `abilities::define` implements spec §6 roles exactly (admin super-admin; manager permissions `companies.manage, contacts.manage, contacts.export, deals.manage, activities.manage, users.read` granted by seeder; rep rules: read Company/Contact/Deal/Activity/User, create Company/Contact/Deal/Activity, update+delete Company/Contact/Activity when `owner_id = me`, update + move Deal when `owner_id = me`, delete Deal when `owner_id = me`, cannot delete Deal when `stage $in [won, lost]` last). Users module: `GET /api/v1/me`, `GET/POST/PATCH /api/v1/users` (spec §7). Seeder `crm-demo` (spec §6 accounts and volumes). `tests/support`: `test_app()`, `login_as(role) -> TestClient`.
- [ ] Tests: `boots_and_check_passes`, `me_returns_roles`, `admin_can_create_user_with_role`, `rep_cannot_create_user` (403), `seeder_creates_demo_accounts`.

### Task 33: Companies + Contacts (agent `crm-companies`)

Entities, migrations (`after: ["auth"]`), DTOs with garde rules, services using `Repo`, controllers exactly per spec §7 (filters, sort, search on name/domain/email via `LIKE`, soft delete with `deleted_at`, `__type`, `owner` embed, CSV export with `csv` crate for admin/manager via `#[can(Export, Contact)]`). Subjects `Company` (`companies`), `Contact` (`contacts`).
- [ ] Tests: CRUD happy paths, `rep_cannot_update_other_reps_company` (403), `rep_can_update_own`, validation 400 (missing name, bad email, unknown company_id via `exists`), `search_and_filters`, `soft_deleted_hidden`, `export_csv_header_and_rows`, `rep_cannot_export`.

### Task 34: Deals (agent `crm-deals`)

Entity (stage enum string, probability derived), list/board/CRUD/move per spec §7; move rules: `lost` requires `lost_reason` (400), `won`/`lost` set `closed_at`, moving back reopens (`closed_at = null`); `position` reorders within the stage; reassigning owner or winning emits `DealAssigned` / `DealWon` events → queued listener sends `DealNotification` (database + mail) to the owner; every change broadcasts `deal.updated` on `private-crm.deals` through `dyn Broadcaster`. Subject `Deal` (`deals`), custom action `Move` (`move`).
- [ ] Tests: `board_groups_by_stage_with_totals`, `move_to_lost_requires_reason`, `won_sets_closed_at`, `rep_cannot_move_others_deal`, `rep_cannot_delete_won_deal`, `manager_can_reassign_and_owner_notified` (notification row + array mail after `run_pending`), `deal_update_broadcasts`.

### Task 35: Activities + Dashboard (agent `crm-activities`)

Activities per spec §7 (subject validation: subject must exist; `subject_label` resolved; `due` filters relative to `Clock`; complete endpoint), dashboard aggregates (pipeline by stage, won this month, open deal totals + weighted by probability, task counts, 10 recent activities), scheduled task `crm:task-reminders` daily at 08:00 notifying assignees of tasks due today, `audit:prune` daily.
- [ ] Tests: `create_task_on_deal`, `subject_must_exist`, `due_filters_overdue_today_upcoming` (FixedClock), `complete_sets_completed_at`, `rep_cannot_edit_others_activity`, `dashboard_numbers_match_seeded_fixture`, `reminder_tick_notifies_assignees_of_tasks_due_today`.

### Task 36: CRM integrate (agent `crm-integrate`)

- [ ] Ensure every route has `#[api(...)]` with request/response schemas; `openapi:export apps/crm-web/openapi.json`; run `migrate:fresh` + `db:seed` against `storage/crm.db` and smoke-test with the `serve` command and curl (csrf-cookie → login → board); `cargo test -p crm-api`; clippy; commit `feat(crm-api): navree CRM API`.

---

## Wave 7 — Web, E2E, review

### Task 37: crm-web build (agent `crm-web`, runs from Wave 0 in parallel)

`apps/crm-web`: `pnpm create next-app@latest` (App Router, TypeScript, Tailwind, ESLint, `src/`), shadcn/ui, `@tanstack/react-query`, `openapi-fetch` + `openapi-typescript` (types hand-written from spec §7 until the real `openapi.json` exists), `@casl/ability@7` + `@casl/react@7` (rules from `/api/me/abilities`, `detectSubjectType: o => typeof o === 'string' ? o : o.__type`), `laravel-echo` + `pusher-js` (custom authorizer posting to `/broadcasting/auth` with `X-XSRF-TOKEN`), `@dnd-kit` for the deal board. `next.config.ts` rewrites `/api/:path*`, `/sanctum/:path*`, `/broadcasting/:path*` → `NAVREE_API_URL` (default `http://127.0.0.1:8080`). Screens: login; dashboard; companies list/detail; contacts list/detail + CSV export; deals board (drag between stages, lost-reason dialog) + deal detail; tasks (overdue/today/upcoming, complete); notifications bell; settings → users & roles (admin), audit log (admin). `<Can>` hides actions the API would refuse. Mock layer (MSW) so `pnpm dev` works without the API; `pnpm build` and `pnpm lint` clean; Vitest unit tests for the API client, CASL setup and money formatting.

### Task 38: crm-web integration + Playwright (agent `crm-e2e`)

- [ ] Regenerate `src/lib/api/schema.d.ts` from `apps/crm-web/openapi.json` with openapi-typescript; fix type drift. Playwright config with `webServer` entries starting the seeded API (`cargo run -p crm-api -- serve`) and `pnpm start`. Specs: `login.spec.ts` (bad password message, success → dashboard), `companies.spec.ts` (rep creates a company and a contact), `deals.spec.ts` (create deal, drag to proposal, mark lost requires reason, rep cannot edit another rep's deal: no edit button and API 403), `tasks.spec.ts` (create task due today, appears under today, complete), `admin.spec.ts` (admin creates a user, sees audit log entries). All pass headless.

### Task 39: Security review (agent `review-security`) / Task 40: PRD-conformance review (agent `review-prd`) / Task 41: fixer (agent `fixer`)

Reviewers report findings with file:line and a failing test idea; the fixer writes the failing test, fixes, and re-runs the full suite (`cargo test --workspace`, clippy, `pnpm build`, Playwright). Final commit `fix: review findings`.
