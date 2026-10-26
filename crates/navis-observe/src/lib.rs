//! # navis-observe
//!
//! `ObserveModule`: liveness and readiness probes and Prometheus metrics.
//!
//! - `GET /health/live`: `200 {"status":"up"}` whenever the process serves
//!   HTTP. It runs no checks, so a slow dependency never gets the process
//!   restarted.
//! - `GET /health/ready`: runs every `dyn HealthCheck` multi-binding
//!   ([`navis_contracts::HealthCheck`]) concurrently, each limited to
//!   [`DEFAULT_CHECK_TIMEOUT`] (2 s); a check that times out or panics is
//!   down. `200` when every check is up, else `503`, with the body
//!   `{"status": "up"|"down", "checks": {name: {status, latency_ms, error?}}}`
//!   ([`HealthReport`]). Down checks are logged at WARN (target `navis::health`).
//! - `GET /metrics`: the Prometheus text exposition of the process-wide
//!   [`metrics`](https://docs.rs/metrics) recorder, which navis-observe
//!   installs once per process. navis-http records `http_requests_total` and
//!   the `http_request_duration_seconds` histogram (labels `method`, `route`
//!   template, `status`); any module can add its own through the `metrics`
//!   macros. With `observe.metrics_token` set, the route requires
//!   `Authorization: Bearer <token>` (401 otherwise).
//!
//! The paths are absolute (no API prefix or version), the routes are public
//! (no authenticator needed) and the responses carry `Cache-Control: no-store`.
//!
//! A module contributes a readiness check as a multi-binding:
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use navis_contracts::{BoxFuture, HealthCheck, HealthStatus};
//! use navis_http::{HttpModule, HttpOptions};
//! use navis_kernel::{Factory, Navis, Module, ModuleBuilder};
//! use navis_observe::{ObserveModule, ObserveOptions};
//!
//! struct QueueHealth;
//!
//! impl HealthCheck for QueueHealth {
//!     fn name(&self) -> &str {
//!         "queue"
//!     }
//!
//!     fn check(&self) -> BoxFuture<'_, HealthStatus> {
//!         Box::pin(async { HealthStatus::Up })
//!     }
//! }
//!
//! struct QueueModule;
//!
//! impl Module for QueueModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.multi(Factory::<dyn HealthCheck>::new(|_| async {
//!             Ok(Arc::new(QueueHealth) as Arc<dyn HealthCheck>)
//!         }));
//!     }
//! }
//!
//! # fn main() -> Result<(), navis_kernel::BoxError> {
//! Navis::app()
//!     .module(HttpModule::for_root(HttpOptions::default()))
//!     .module(ObserveModule::for_root(ObserveOptions::default()))
//!     .module(QueueModule)
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

mod controller;
mod health;
mod metrics;
mod module;
mod options;

pub use health::{CheckReport, CheckState, DEFAULT_CHECK_TIMEOUT, Health, HealthReport};
pub use module::ObserveModule;
pub use options::ObserveOptions;
