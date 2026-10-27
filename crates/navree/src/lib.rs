//! # navree
//!
//! Navree: the complete backend framework for Rust, modular, API-first and
//! AI-native, on Tokio. This facade is the one dependency an app needs: it re-exports
//! every module crate (one Cargo feature each, all on by default except the
//! opt-in ecosystem libraries), the attribute macros and a [`prelude`].
//!
//! | Module | Crate | Feature | What it brings |
//! |---|---|---|---|
//! | [`kernel`] | navree-kernel | always | module graph, DI container, lifecycle, commands |
//! | [`contracts`] | navree-contracts | always | cross-module traits: `Principal`, `Job`, `Clock`, `AuditSink`, ... |
//! | [`config`] | navree-config | always | `ConfigModule`, `.env`, TOML and `APP_*` sources |
//! | `telemetry` | navree-telemetry | `telemetry` | typed telemetry events, sinks, W3C trace context |
//! | `http` | navree-http | `http` | `HttpModule`, controllers, `AppError` (JSON errors) |
//! | `log` | navree-log | `log` | `LoggerModule` (tracing) |
//! | `crypto` | navree-crypto | `crypto` | `CryptoModule`: encryption, Argon2id hashing, app key; `jose`: JWS and JWK |
//! | `events` | navree-events | `events` | `EventsModule`: events and (queued) listeners |
//! | `view` | navree-view | `view` | `ViewModule`: minijinja templates |
//! | `cache` | navree-cache | `cache` | `CacheModule`: cache stores and locks |
//! | `http_client` | navree-http-client | `http-client` | `HttpClientModule`: outgoing HTTP with fakes |
//! | `schema` | navree-schema | `schema` | `Schema`, `Blueprint`, `Migration`: the schema builder, TimescaleDB hypertables, schema dumps |
//! | `db` | navree-db | `db` | `DatabaseModule`: SeaORM, unit of work, migrations, pagination |
//! | `testing` | navree-testing | `testing` | `TestApp`, `TestClient`: in-process HTTP tests |
//! | `observe` | navree-observe | `observe` | `ObserveModule`: health checks and metrics |
//! | `api` | navree-api | `api` | `ApiModule`: OpenAPI document and docs UI |
//! | `mail` | navree-mail | `mail` | `MailModule`: mailables, transports, queued mail |
//! | `realtime` | navree-realtime | `realtime` | `RealtimeModule`: Pusher-protocol broadcasting |
//! | `storage` | navree-storage | `storage` | `StorageModule`: disks, signed links, direct uploads, disk sync |
//! | | | `storage-s3` | S3 disks: AWS, MinIO, Cloudflare R2, DigitalOcean Spaces |
//! | | | `storage-gcs`, `storage-azure` (opt-in) | Google Cloud Storage and Azure Blob Storage disks |
//! | `session` | navree-session | `session` | `SessionModule`: cookie sessions and CSRF |
//! | `validation` | navree-validation | `validation` | `Valid` extractors on garde |
//! | `queue` | navree-queue | `queue` | `QueueModule`: database and memory job queues, batches, operations |
//! | `audit` | navree-audit | `audit` | `AuditModule`: the audit trail |
//! | `access` | navree-access | `access` | `AccessModule`: roles, permissions, abilities |
//! | `schedule` | navree-schedule | `schedule` | `ScheduleModule`: tasks on cron schedules |
//! | `notify` | navree-notify | `notify` | `NotifyModule`: database, mail and broadcast notifications |
//! | `ui` | navree-ui | `ui` | `UiModule`: the app shell and components for server-rendered tools, embedded assets |
//! | `orm` | navree-orm | `orm` | `#[derive(Model)]` models, the query builder, records, collections, pagination |
//! | `auth` | navree-auth | `auth` | `AuthModule`: JWT access and refresh tokens, users |
//! | `workspace` | navree-workspace | `workspace` | `WorkspaceModule`: workspaces, members, roles, invitations, isolation, organizations, partitions |
//! | `redis` | navree-redis | `redis` (opt-in) | `RedisModule`: shared Redis connections; Redis cache, locks, queue, sessions, rate limits and real-time backplane |
//! | `webhooks` | navree-webhooks | `webhooks` | `WebhooksModule`: signed outgoing webhooks with retries, verified incoming ones |
//! | `sso` | navree-sso | `sso` | `SsoModule`: SAML 2.0 and OpenID Connect single sign-on per organization or workspace, verified domains, just-in-time provisioning |
//! | `social` | navree-social | `social` (opt-in) | `SocialModule`: sign-in with Google, GitHub, Apple and other providers |
//! | `oauth` | navree-oauth | `oauth` (opt-in) | `OAuthServerModule`: OAuth 2.1 and OpenID Connect provider, personal access tokens |
//! | `flags` | navree-flags (plus `#[flag]`) | `flags` (opt-in) | `FlagsModule`: feature flags per user, workspace, plan or percentage |
//! | `graphql` | navree-graphql | `graphql` (opt-in) | `GraphqlModule`: `#[resolver]`, dataloaders, subscriptions |
//! | `grpc` | navree-grpc | `grpc` (opt-in) | `GrpcModule`: tonic services with gRPC-Web, Connect, health and reflection |
//! | `lighthouse` | navree-lighthouse | `lighthouse` | `LighthouseModule`: issues, traces, alerts and the `/_lighthouse` dashboard |
//! | `billing` | navree-billing | `billing` (opt-in) | `BillingModule`: subscriptions, trials, usage, invoices with Stripe or Paddle |
//! | `search` | navree-search | `search` (opt-in) | `SearchModule`: full-text and vector search, `#[derive(Searchable)]` |
//! | `logbook` | navree-logbook | `logbook` | `LogbookModule`: the local debugging timeline at `/_logbook`, with replay |
//! | `micro` | navree-micro | `micro` (opt-in) | `MicroModule`, `ServiceClient`: messages and events between services over TCP, NATS (`micro-nats`), Kafka (`micro-kafka`), RabbitMQ (`micro-rabbitmq`), Redis Streams (`micro-redis`) or MQTT (`micro-mqtt`) |
//! | `fleet` | navree-fleet | `fleet` | `FleetModule`: supervised queue workers, metrics, failed jobs, the `/_fleet` dashboard |
//! | `helm` | navree-helm | `helm` | `HelmModule`: the admin panel generated from your entities |
//! | `dock` | navree-dock | `dock` (opt-in) | Dock's `dock.toml`, the services' settings and the Compose file |
//! | `mcp` | navree-mcp | `mcp` (opt-in) | `McpModule`: MCP servers over streamable HTTP and stdio, with `#[mcp_server]` |
//! | `ai` | navree-ai | `ai` (opt-in) | `AiModule`: models, structured output, streaming, agents, embeddings, vector stores |
//! | `harbor` | navree-harbor | `harbor` (opt-in) | Harbor as a library: `harbor.toml`, provisioning, deploys, secrets |
//!
//! `full` (the default) turns on every module except the opt-in ones
//! (`social`, `oauth`, `flags`, `graphql`, `grpc`, `billing`, `search`,
//! `mcp`, `ai`, `redis`, `micro` and its transports, `dock`, `harbor`,
//! `otel`), and the `storage-s3` driver but not `storage-gcs` or
//! `storage-azure`. A feature also turns on the features of the crates its
//! crate builds on (`auth` turns on `access`, `db`, `http`, ...). SQLite is
//! built in; the `postgres` feature adds navree-db's PostgreSQL driver and
//! the `mysql` feature its MySQL and MariaDB driver, and the opt-in `otel`
//! feature adds navree-observe's OTLP span exporter. `validation-phone` and
//! `validation-html` (both in `full`) add navree-validation's `Phone` and
//! `SafeHtml`. `search-tantivy` adds search's embedded Tantivy engine.
//! `axum` is re-exported with `http`. `dock` is a development tool
//! (`navree dock`), for tools and tests that read `dock.toml`. `harbor` is a
//! deployment tool: the `navree` CLI has Harbor built in, and an app turns it
//! on only to read `harbor.toml` or drive deploys from its own code.
//!
//! The macros sit at the root: [`module`], [`injectable`] and
//! [`controller`] (with the route attributes [`get`], [`post`], [`put`],
//! [`patch`] and [`delete`], which `#[controller]` consumes). Their
//! expansions name the runtime crates through this facade (`::navree::kernel`,
//! `::navree::http`, `::navree::access`), so an app depends on `navree` alone
//! (plus `serde`, `sea-orm` and `garde`, whose derives name their own crates).
//!
//! ```no_run
//! # // Inside this crate's own doctests the macros name `crate::kernel` and
//! # // `crate::http`; an app's code names `::navree::kernel` instead.
//! # use navree::{http, kernel};
//! use navree::prelude::*;
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
//!     Navree::app()
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
//! use navree::prelude::*;
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
//!     Navree::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(HttpModule::from_config())
//!         .module(GreetingModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

pub mod prelude;

// L0-L1: always present.
pub use navree_config as config;
pub use navree_contracts as contracts;
pub use navree_kernel as kernel;
#[cfg(feature = "telemetry")]
pub use navree_telemetry as telemetry;

// L2
#[cfg(feature = "cache")]
pub use navree_cache as cache;
#[cfg(feature = "crypto")]
pub use navree_crypto as crypto;
#[cfg(feature = "events")]
pub use navree_events as events;
#[cfg(feature = "http")]
pub use navree_http as http;
#[cfg(feature = "http-client")]
pub use navree_http_client as http_client;
#[cfg(feature = "log")]
pub use navree_log as log;
#[cfg(feature = "schema")]
pub use navree_schema as schema;
#[cfg(feature = "view")]
pub use navree_view as view;

// L3
#[cfg(feature = "api")]
pub use navree_api as api;
#[cfg(feature = "db")]
pub use navree_db as db;
#[cfg(feature = "mail")]
pub use navree_mail as mail;
#[cfg(feature = "observe")]
pub use navree_observe as observe;
#[cfg(feature = "realtime")]
pub use navree_realtime as realtime;
#[cfg(feature = "storage")]
pub use navree_storage as storage;
#[cfg(feature = "testing")]
pub use navree_testing as testing;

// L4
#[cfg(feature = "access")]
pub use navree_access as access;
#[cfg(feature = "audit")]
pub use navree_audit as audit;
#[cfg(feature = "notify")]
pub use navree_notify as notify;
#[cfg(feature = "orm")]
pub use navree_orm as orm;
#[cfg(feature = "queue")]
pub use navree_queue as queue;
#[cfg(feature = "schedule")]
pub use navree_schedule as schedule;
#[cfg(feature = "session")]
pub use navree_session as session;
#[cfg(feature = "ui")]
pub use navree_ui as ui;
#[cfg(feature = "validation")]
pub use navree_validation as validation;

// L5
#[cfg(feature = "auth")]
pub use navree_auth as auth;
#[cfg(feature = "workspace")]
pub use navree_workspace as workspace;

// L5 (opt-in)
#[cfg(feature = "redis")]
pub use navree_redis as redis;

// L6: ecosystem libraries.
#[cfg(feature = "sso")]
pub use navree_sso as sso;
#[cfg(feature = "webhooks")]
pub use navree_webhooks as webhooks;

// L6 (opt-in)
#[cfg(feature = "grpc")]
pub use navree_grpc as grpc;
#[cfg(feature = "oauth")]
pub use navree_oauth as oauth;
#[cfg(feature = "social")]
pub use navree_social as social;
/// navree-graphql, plus the `#[resolver]` macro
/// (`use navree::graphql::{ContextExt, Result, resolver};`).
#[cfg(feature = "graphql")]
pub mod graphql {
    pub use navree_graphql::*;
    /// `#[resolver]`: GraphQL root fields on an impl block (see
    /// [`navree_macros::resolver`]).
    pub use navree_macros::resolver;
}
#[cfg(feature = "micro")]
pub use navree_micro as micro;

/// Feature flags (the `flags` feature): navree-flags, re-exported whole,
/// plus the `#[flag]` attribute macro (`use navree::flags::flag;`).
#[cfg(feature = "flags")]
pub mod flags {
    pub use navree_flags::*;
    /// `#[flag]`: a feature flag as a unit struct (see navree-macros).
    pub use navree_macros::flag;
}
#[cfg(feature = "ai")]
pub use navree_ai as ai;

/// MCP servers: navree-mcp, plus the `#[mcp_server]` attribute and the
/// `#[mcp_tool]`, `#[mcp_resource]` and `#[mcp_prompt]` markers it consumes.
#[cfg(feature = "mcp")]
pub mod mcp {
    pub use navree_macros::{mcp_prompt, mcp_resource, mcp_server, mcp_tool};
    pub use navree_mcp::*;
}

// L7: packages built on other packages.
#[cfg(feature = "helm")]
pub use navree_helm as helm;

// L7 (opt-in)
#[cfg(feature = "billing")]
pub use navree_billing as billing;
#[cfg(feature = "fleet")]
pub use navree_fleet as fleet;
#[cfg(feature = "lighthouse")]
pub use navree_lighthouse as lighthouse;
#[cfg(feature = "logbook")]
pub use navree_logbook as logbook;
#[cfg(feature = "search")]
pub use navree_search as search;

// Tools (opt-in)
#[cfg(feature = "dock")]
pub use navree_dock as dock;
#[cfg(feature = "harbor")]
pub use navree_harbor as harbor;

/// Axum, for extractors and types navree-http does not re-export
/// (`navree::axum::extract::Path`, `navree::axum::http::HeaderMap`, ...).
#[cfg(feature = "http")]
pub use axum;

/// The attribute macros: `#[module]`, `#[injectable]`, `#[controller]` and
/// its route attributes.
pub use navree_macros::{controller, delete, get, injectable, module, patch, post, put};

/// `#[message_handlers]` and the pattern attributes it consumes
/// (`#[message_pattern]`, `#[event_pattern]`), with the `micro` feature.
#[cfg(feature = "micro")]
pub use navree_macros::{event_pattern, message_handlers, message_pattern};

/// The kernel types an app's root and modules use most.
pub use navree_kernel::{
    App, AppBuilder, AppContext, BoxError, BoxFuture, Factory, Injectable, KernelError, Lazy,
    Module, ModuleBuilder, ModuleId, Navree, ProviderKey, Resolver, Scope,
};

/// A handler's result: the error defaults to [`AppError`](navree_http::AppError),
/// rendered in Navree's JSON error shape.
#[cfg(feature = "http")]
pub type Result<T, E = navree_http::AppError> = std::result::Result<T, E>;
