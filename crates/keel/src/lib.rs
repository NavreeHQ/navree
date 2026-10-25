//! # keel
//!
//! Keel: a Laravel-class, NestJS-structured application framework on Rust
//! and Tokio. This facade is the one dependency an app needs: it re-exports
//! every module crate (one Cargo feature each, all on by default), the
//! attribute macros and a [`prelude`].
//!
//! | Module | Crate | Feature | What it brings |
//! |---|---|---|---|
//! | [`kernel`] | keel-kernel | always | module graph, DI container, lifecycle, commands |
//! | [`contracts`] | keel-contracts | always | cross-module traits: `Principal`, `Job`, `Clock`, `AuditSink`, ... |
//! | [`config`] | keel-config | always | `ConfigModule`, `.env`, TOML and `APP_*` sources |
//! | `http` | keel-http | `http` | `HttpModule`, controllers, `AppError` (NestJS error format) |
//! | `log` | keel-log | `log` | `LoggerModule` (tracing) |
//! | `crypto` | keel-crypto | `crypto` | `CryptoModule`: encryption, Argon2id hashing, app key |
//! | `events` | keel-events | `events` | `EventsModule`: events and (queued) listeners |
//! | `view` | keel-view | `view` | `ViewModule`: minijinja templates |
//! | `cache` | keel-cache | `cache` | `CacheModule`: cache stores and locks |
//! | `http_client` | keel-http-client | `http-client` | `HttpClientModule`: outgoing HTTP with fakes |
//! | `db` | keel-db | `db` | `DatabaseModule`: SeaORM, unit of work, migrations, pagination |
//! | `testing` | keel-testing | `testing` | `TestApp`, `TestClient`: in-process HTTP tests |
//! | `observe` | keel-observe | `observe` | `ObserveModule`: health checks and metrics |
//! | `api` | keel-api | `api` | `ApiModule`: OpenAPI document and docs UI |
//! | `mail` | keel-mail | `mail` | `MailModule`: mailables, transports, queued mail |
//! | `realtime` | keel-realtime | `realtime` | `RealtimeModule`: Pusher-protocol broadcasting |
//! | `storage` | keel-storage | `storage` | `StorageModule`: disks and uploads |
//! | `session` | keel-session | `session` | `SessionModule`: cookie sessions and CSRF |
//! | `validation` | keel-validation | `validation` | `Valid` extractors on garde |
//! | `queue` | keel-queue | `queue` | `QueueModule`: database and memory job queues |
//! | `audit` | keel-audit | `audit` | `AuditModule`: the audit trail |
//! | `access` | keel-access | `access` | `AccessModule`: roles, permissions, CASL abilities |
//! | `schedule` | keel-schedule | `schedule` | `ScheduleModule`: cron-style tasks |
//! | `notify` | keel-notify | `notify` | `NotifyModule`: database, mail and broadcast notifications |
//! | `auth` | keel-auth | `auth` | `AuthModule`: JWT access and refresh tokens, users |
//!
//! A feature also turns on the features of the crates its crate builds on
//! (`auth` turns on `access`, `db`, `http`, ...). The `postgres` feature adds
//! keel-db's PostgreSQL driver. `axum` is re-exported with `http`.
//!
//! The macros sit at the root: [`module`], [`injectable`] and
//! [`controller`] (with the route attributes [`get`], [`post`], [`put`],
//! [`patch`] and [`delete`], which `#[controller]` consumes). Their
//! expansions name the runtime crates through this facade (`::keel::kernel`,
//! `::keel::http`, `::keel::access`), so an app depends on `keel` alone
//! (plus `serde`, `sea-orm` and `garde`, whose derives name their own crates).
//!
//! ```no_run
//! # // Inside this crate's own doctests the macros name `crate::kernel` and
//! # // `crate::http`; an app's code names `::keel::kernel` instead.
//! # use keel::{http, kernel};
//! use keel::prelude::*;
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
//!     Keel::app()
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
//! use keel::prelude::*;
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
//!     Keel::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(HttpModule::from_config())
//!         .module(GreetingModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

pub mod prelude;

// L0-L1: always present.
pub use keel_config as config;
pub use keel_contracts as contracts;
pub use keel_kernel as kernel;

// L2
#[cfg(feature = "cache")]
pub use keel_cache as cache;
#[cfg(feature = "crypto")]
pub use keel_crypto as crypto;
#[cfg(feature = "events")]
pub use keel_events as events;
#[cfg(feature = "http")]
pub use keel_http as http;
#[cfg(feature = "http-client")]
pub use keel_http_client as http_client;
#[cfg(feature = "log")]
pub use keel_log as log;
#[cfg(feature = "view")]
pub use keel_view as view;

// L3
#[cfg(feature = "api")]
pub use keel_api as api;
#[cfg(feature = "db")]
pub use keel_db as db;
#[cfg(feature = "mail")]
pub use keel_mail as mail;
#[cfg(feature = "observe")]
pub use keel_observe as observe;
#[cfg(feature = "realtime")]
pub use keel_realtime as realtime;
#[cfg(feature = "storage")]
pub use keel_storage as storage;
#[cfg(feature = "testing")]
pub use keel_testing as testing;

// L4
#[cfg(feature = "access")]
pub use keel_access as access;
#[cfg(feature = "audit")]
pub use keel_audit as audit;
#[cfg(feature = "notify")]
pub use keel_notify as notify;
#[cfg(feature = "queue")]
pub use keel_queue as queue;
#[cfg(feature = "schedule")]
pub use keel_schedule as schedule;
#[cfg(feature = "session")]
pub use keel_session as session;
#[cfg(feature = "validation")]
pub use keel_validation as validation;

// L5
#[cfg(feature = "auth")]
pub use keel_auth as auth;

/// Axum, for extractors and types keel-http does not re-export
/// (`keel::axum::extract::Path`, `keel::axum::http::HeaderMap`, ...).
#[cfg(feature = "http")]
pub use axum;

/// The attribute macros: `#[module]`, `#[injectable]`, `#[controller]` and
/// its route attributes.
pub use keel_macros::{controller, delete, get, injectable, module, patch, post, put};

/// The kernel types an app's root and modules use most.
pub use keel_kernel::{
    App, AppBuilder, AppContext, BoxError, BoxFuture, Factory, Injectable, Keel, KernelError, Lazy,
    Module, ModuleBuilder, ModuleId, ProviderKey, Resolver, Scope,
};

/// A handler's result: the error defaults to [`AppError`](keel_http::AppError),
/// rendered in the NestJS error format.
#[cfg(feature = "http")]
pub type Result<T, E = keel_http::AppError> = std::result::Result<T, E>;
