//! # navree-dock
//!
//! Dock runs the services an app needs during development (PostgreSQL with
//! TimescaleDB, Redis, Meilisearch, Mailpit, MinIO and NATS) in containers,
//! with Docker or Podman, and points the app's settings at them. It is the
//! library behind `navree dock` (navree-cli); the facade re-exports it as
//! `navree::dock` with its opt-in `dock` feature.
//!
//! - **`dock.toml`** ([`DockFile`]) lists the services, next to the app's
//!   `Cargo.toml`. [`add_service`] and [`remove_service`] edit it and keep
//!   its comments. `[services.postgres]` runs TimescaleDB
//!   ([`TIMESCALEDB_IMAGE`], Navree's default database) unless it sets
//!   `timescaledb = false`.
//! - **A Compose file** ([`compose::render`]) is generated from it under
//!   `.navree/dock/` and run with `docker compose` or `podman compose`
//!   ([`Runtime`]; `NAVREE_DOCK_RUNTIME` picks one).
//! - **[`Dock`]** starts services and waits for their readiness checks
//!   ([`Dock::up`]), stops them ([`Dock::down`]), lists them
//!   ([`Dock::status`]), and opens their logs and clients ([`Dock::logs`],
//!   [`Dock::shell`]).
//! - **Settings** ([`DockFile::env`]): each service gives the app
//!   environment variables (`APP_DATABASE__URL`, `APP_MAIL__TRANSPORT`, ...),
//!   written into `.env` between `# dock:start` and `# dock:end`
//!   ([`write_dotenv`]), printed by `navree dock env` ([`env_lines`]) and
//!   passed by `navree dev` ([`app_env`]).
//! - **Everything external** goes through an [`Executor`]:
//!   [`ProcessExecutor`] for real, [`FakeExecutor`] in tests.
//!
//! ```
//! use navree_dock::{Dock, DockFile, FakeExecutor};
//!
//! # fn main() -> Result<(), navree_dock::DockError> {
//! let dir = tempfile::tempdir().unwrap();
//! let file = DockFile::parse("[services.postgres]\n[services.mailpit]\n", "crm-api")?;
//! assert_eq!(
//!     file.env()[0],
//!     ("APP_DATABASE__URL".to_owned(), "postgres://navree:navree@127.0.0.1:5432/crm_api".to_owned())
//! );
//!
//! let dock = Dock::new(dir.path(), file, FakeExecutor::new())?;
//! let mut out = Vec::new();
//! dock.up(&[], &mut out)?;
//! let printed = String::from_utf8(out).unwrap();
//! assert!(printed.starts_with("Starting crm-api services with docker\n"));
//! assert!(std::fs::read_to_string(dir.path().join(".env")).unwrap().contains("# dock:start"));
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod compose;

mod config;
mod dock;
mod dotenv;
mod edit;
mod error;
mod exec;
mod fake;
mod runtime;
mod service;
mod template;

pub use config::{DOCK_FILE, DockFile, package_name};
pub use dock::{Dock, READY_TIMEOUT, ServiceStatus, app_env, status_table};
pub use dotenv::{BLOCK_END, BLOCK_START, DotenvUpdate, env_lines, update_dotenv, write_dotenv};
pub use edit::{AddReport, add_service, remove_service};
pub use error::{DockError, PortConflict};
pub use exec::{Cmd, CmdOutput, Executor, ProcessExecutor};
pub use fake::FakeExecutor;
pub use runtime::{RUNTIME_VARIABLE, Runtime, RuntimeChoice};
pub use service::{
    HOST, NATS_BOX_IMAGE, POSTGRES_VERSION, Port, Readiness, Service, ServiceOptions, ServiceSpec,
    SetupStep, Shell, TIMESCALEDB_IMAGE, TIMESCALEDB_POSTGRES_VERSIONS, TIMESCALEDB_VERSION,
};
