//! # navis
//!
//! Navis: a Laravel-class, NestJS-structured application framework on Rust
//! and Tokio. This facade is the one dependency an app needs: it re-exports
//! every module crate (one Cargo feature each, all on by default), the
//! attribute macros and a [`prelude`].
//!
//! | Module | Crate | Feature | What it brings |
//! |---|---|---|---|
//! | [`kernel`] | navis-kernel | always | module graph, DI container, lifecycle, commands |
//! | [`contracts`] | navis-contracts | always | cross-module traits: `Principal`, `Job`, `Clock`, `AuditSink`, ... |
//! | [`config`] | navis-config | always | `ConfigModule`, `.env`, TOML and `APP_*` sources |
//! | `http` | navis-http | `http` | `HttpModule`, controllers, `AppError` (NestJS error format) |
//! | `log` | navis-log | `log` | `LoggerModule` (tracing) |
//! | `crypto` | navis-crypto | `crypto` | `CryptoModule`: encryption, Argon2id hashing, app key |
//! | `events` | navis-events | `events` | `EventsModule`: events and (queued) listeners |
//! | `view` | navis-view | `view` | `ViewModule`: minijinja templates |
//! | `cache` | navis-cache | `cache` | `CacheModule`: cache stores and locks |
//! | `http_client` | navis-http-client | `http-client` | `HttpClientModule`: outgoing HTTP with fakes |
//! | `db` | navis-db | `db` | `DatabaseModule`: SeaORM, unit of work, migrations, pagination |
//! | `testing` | navis-testing | `testing` | `TestApp`, `TestClient`: in-process HTTP tests |
//! | `observe` | navis-observe | `observe` | `ObserveModule`: health checks and metrics |
//! | `api` | navis-api | `api` | `ApiModule`: OpenAPI document and docs UI |
//! | `mail` | navis-mail | `mail` | `MailModule`: mailables, transports, queued mail |
//! | `realtime` | navis-realtime | `realtime` | `RealtimeModule`: Pusher-protocol broadcasting |
//! | `storage` | navis-storage | `storage` | `StorageModule`: disks and uploads |
//! | `session` | navis-session | `session` | `SessionModule`: cookie sessions and CSRF |
//! | `validation` | navis-validation | `validation` | `Valid` extractors on garde |
//! | `queue` | navis-queue | `queue` | `QueueModule`: database and memory job queues |
//! | `audit` | navis-audit | `audit` | `AuditModule`: the audit trail |
//! | `access` | navis-access | `access` | `AccessModule`: roles, permissions, CASL abilities |
//! | `schedule` | navis-schedule | `schedule` | `ScheduleModule`: cron-style tasks |
//! | `notify` | navis-notify | `notify` | `NotifyModule`: database, mail and broadcast notifications |
//! | `auth` | navis-auth | `auth` | `AuthModule`: JWT access and refresh tokens, users |
//!
//! A feature also turns on the features of the crates its crate builds on
//! (`auth` turns on `access`, `db`, `http`, ...). The `postgres` feature adds
//! navis-db's PostgreSQL driver. `axum` is re-exported with `http`.
//!
//! The macros sit at the root: [`module`], [`injectable`] and
//! [`controller`] (with the route attributes [`get`], [`post`], [`put`],
//! [`patch`] and [`delete`], which `#[controller]` consumes). Their
//! expansions name the runtime crates through this facade (`::navis::kernel`,
//! `::navis::http`, `::navis::access`), so an app depends on `navis` alone
//! (plus `serde`, `sea-orm` and `garde`, whose derives name their own crates).
//!
//! ```no_run
//! # // Inside this crate's own doctests the macros name `crate::kernel` and
//! # // `crate::http`; an app's code names `::navis::kernel` instead.
//! # use navis::{http, kernel};
//! use navis::prelude::*;
//!
//! #[injectable]
//! pub struct GreetingController;
//!
//! #[controller("/greeting", version = "1")]
//! impl GreetingController {
//!     #[get("/{name}")]
//!     #[public]
//!     async fn show(&self, Path(name): Path<String>) -> Json<String> {
//!         Json(format!("Hello, {name}!"))
//!     }
//! }
//!
//! #[module(controllers = [GreetingController])]
//! pub struct GreetingModule;
//!
//! fn main() -> Result<(), BoxError> {
//!     Navis::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(HttpModule::from_config()) // GET /api/v1/greeting/{name}
//!         .module(GreetingModule)
//!         .run()
//! }
//! ```
//!
//! The same app without the macros (the hand-written escape hatch):
//!
//! ```no_run
//! use navis::prelude::*;
//!
//! struct GreetingModule;
//!
//! impl Module for GreetingModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.value(String::from("Hello"));
//!     }
//! }
//!
//! fn main() -> Result<(), BoxError> {
//!     Navis::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(HttpModule::from_config())
//!         .module(GreetingModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

pub mod prelude;

// L0-L1: always present.
pub use navis_config as config;
pub use navis_contracts as contracts;
pub use navis_kernel as kernel;

// L2
#[cfg(feature = "cache")]
pub use navis_cache as cache;
#[cfg(feature = "crypto")]
pub use navis_crypto as crypto;
#[cfg(feature = "events")]
pub use navis_events as events;
#[cfg(feature = "http")]
pub use navis_http as http;
#[cfg(feature = "http-client")]
pub use navis_http_client as http_client;
#[cfg(feature = "log")]
pub use navis_log as log;
#[cfg(feature = "view")]
pub use navis_view as view;

// L3
#[cfg(feature = "api")]
pub use navis_api as api;
#[cfg(feature = "db")]
pub use navis_db as db;
#[cfg(feature = "mail")]
pub use navis_mail as mail;
#[cfg(feature = "observe")]
pub use navis_observe as observe;
#[cfg(feature = "realtime")]
pub use navis_realtime as realtime;
#[cfg(feature = "storage")]
pub use navis_storage as storage;
#[cfg(feature = "testing")]
pub use navis_testing as testing;

// L4
#[cfg(feature = "access")]
pub use navis_access as access;
#[cfg(feature = "audit")]
pub use navis_audit as audit;
#[cfg(feature = "notify")]
pub use navis_notify as notify;
#[cfg(feature = "queue")]
pub use navis_queue as queue;
#[cfg(feature = "schedule")]
pub use navis_schedule as schedule;
#[cfg(feature = "session")]
pub use navis_session as session;
#[cfg(feature = "validation")]
pub use navis_validation as validation;

// L5
#[cfg(feature = "auth")]
pub use navis_auth as auth;

/// Axum, for extractors and types navis-http does not re-export
/// (`navis::axum::extract::Path`, `navis::axum::http::HeaderMap`, ...).
#[cfg(feature = "http")]
pub use axum;

/// The attribute macros: `#[module]`, `#[injectable]`, `#[controller]` and
/// its route attributes.
pub use navis_macros::{controller, delete, get, injectable, module, patch, post, put};

/// The kernel types an app's root and modules use most.
pub use navis_kernel::{
    App, AppBuilder, AppContext, BoxError, BoxFuture, Factory, Injectable, KernelError, Lazy,
    Module, ModuleBuilder, ModuleId, Navis, ProviderKey, Resolver, Scope,
};

/// A handler's result: the error defaults to [`AppError`](navis_http::AppError),
/// rendered in the NestJS error format.
#[cfg(feature = "http")]
pub type Result<T, E = navis_http::AppError> = std::result::Result<T, E>;
