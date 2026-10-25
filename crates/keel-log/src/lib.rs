//! # keel-log
//!
//! `LoggerModule`: Keel's logging, on by default. It installs one `tracing`
//! subscriber per process with:
//!
//! - **pretty or JSON output** on stdout ([`LogFormat`]): readable lines in
//!   development, one JSON object per line
//!   (`{"timestamp","level","target","fields","span","spans"}`) in
//!   production; span fields (request id, user id) travel with every line;
//! - **redaction before writing**: any event or span field whose name
//!   contains a configured key (`password`, `token`, `authorization`,
//!   `cookie`, `secret` by default; ASCII case-insensitive) is written as
//!   `[redacted]`, in every format and in the log file;
//! - **levels per target**, changeable at runtime through [`LogControl`]
//!   (a reload handle, no restart);
//! - an optional **rotating log file** ([`FileLog`]: daily, hourly or never,
//!   kept `keep_days`), written by a background thread and flushed when the
//!   app shuts down;
//! - `log` crate records bridged into `tracing`.
//!
//! Settings come from [`LogOptions`], the `log` configuration section
//! ([`LoggerModule::from_config`]) or [`LoggerModule::for_root`].
//!
//! ```no_run
//! use keel_config::{ConfigModule, ConfigSources};
//! use keel_kernel::Keel;
//! use keel_log::{LogFormat, LogOptions, LoggerModule};
//!
//! # fn main() -> Result<(), keel_kernel::BoxError> {
//! Keel::app()
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

pub use capture::CaptureWriter;
pub use control::LogControl;
pub use error::LogError;
pub use install::init_for_tests;
use install::install;
pub use module::LoggerModule;
pub use options::{FileLog, LogFormat, LogOptions, Rotation};
pub use subscriber::build_subscriber;
