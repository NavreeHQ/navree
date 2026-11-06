//! # navree-log
//!
//! `LoggerModule`: Navree's logging, on by default. It installs one `tracing`
//! subscriber per process with:
//!
//! - **pretty or JSON output** on stdout ([`LogFormat`]): readable lines in
//!   development, one JSON object per line
//!   (`{"timestamp","level","target","fields","span","spans"}`) in
//!   production; span fields (request id, user id) travel with every line.
//!   When the app runs a command that speaks a protocol on stdout
//!   (`CommandSpec::stdio`, such as `mcp:serve`), the lines go to stderr;
//! - **redaction before writing**: any event or span field whose name
//!   contains a configured key (the shared list of secret words by default:
//!   `password`, `passwd`, `pwd`, `passphrase`, `secret`, `token`,
//!   `authorization`, `cookie`, `credential`, `dsn`, `api_key`, `apikey`,
//!   `private_key` and `connection_string`; ASCII case-insensitive) is
//!   written as `[redacted]`, in every format and in the log file;
//! - **levels per target**, changeable at runtime through [`LogControl`]
//!   (a reload handle, no restart);
//! - an optional **rotating log file** ([`FileLog`]: daily, hourly or never,
//!   kept `keep_days`), written by a background thread and flushed when the
//!   app shuts down;
//! - `log` crate records bridged into `tracing`;
//! - **telemetry**: inside a request or job with a trace, JSON lines carry
//!   its `trace_id` and `span_id`; every line that passes the level filter
//!   is also a `LogRecord` event (fields redacted) for the telemetry sinks
//!   that want logs: the current request's or job's app, else the app
//!   whose `LoggerModule` booted last (see navree-telemetry).
//!
//! Settings come from [`LogOptions`], the `log` configuration section
//! ([`LoggerModule::from_config`]) or [`LoggerModule::for_root`].
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_kernel::Navree;
//! use navree_log::{LogFormat, LogOptions, LoggerModule};
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(LoggerModule::for_root(LogOptions {
//!         format: LogFormat::Json,
//!         ..LogOptions::default()
//!     }))
//!     .run()
//! # }
//! ```
//!
//! The subscriber is installed once per process: the first boot installs it,
//! later boots reuse it and apply only their levels. Tests either call
//! [`init_for_tests`] (output through the test harness, levels from
//! `RUST_LOG`) or capture logs without touching the global subscriber via
//! [`build_subscriber`] and [`CaptureWriter`].
#![forbid(unsafe_code)]

mod capture;
mod control;
mod error;
mod file;
mod filter;
mod install;
mod json;
mod module;
mod options;
mod redact;
mod subscriber;
mod telemetry;

pub use capture::CaptureWriter;
pub use control::LogControl;
pub use error::LogError;
pub use install::init_for_tests;
use install::{Console, install};
pub use module::LoggerModule;
pub use options::{FileLog, LogFormat, LogOptions, Rotation};
pub use subscriber::build_subscriber;
