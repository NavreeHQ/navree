//! # navree-harbor
//!
//! Harbor: servers and zero-downtime deployments for Navree apps, from the
//! `navree` CLI. It provisions Ubuntu servers over SSH, ships releases that
//! go live only after a health check, rolls back, keeps the environment's
//! secrets encrypted in `harbor.toml`, builds container images and writes a
//! push-to-deploy workflow. It depends on no other navree crate.
//!
//! | Command | What it does | Module |
//! |---|---|---|
//! | `navree harbor:init` | `harbor.toml`, `harbor.key`, `.gitignore` (only the key when `harbor.toml` exists) | [`init`] |
//! | `navree harbor:provision [server]` | Idempotent provisioning scripts over SSH | [`provision`] |
//! | `navree harbor:secrets set\|list\|reveal\|unset\|export\|rotate-key` | AES-256-GCM secrets | [`secrets`], [`project`] |
//! | `navree deploy` | Build, upload, migrate, health-check, switch, drain, prune | [`deploy`], [`build`], [`host`] |
//! | `navree deploy:rollback [--to]`, `deploy:releases` | Back to an earlier release | [`deploy`] |
//! | `navree harbor:status\|logs\|run\|ssh\|backup` | Working with servers | [`ops`] |
//! | `navree harbor:ci github` | A GitHub Actions workflow | [`ci`] |
//! | `navree build:image` | An OCI image with Docker or Podman | [`image`] |
//!
//! Every program Harbor runs (`ssh`, `cargo`, `docker`, `git`) goes through
//! an [`exec::Executor`]; tests use [`exec::FakeExecutor`]. The release
//! state machine runs against the [`host::Host`] trait, implemented over SSH
//! by [`host::SshHost`] and in memory by [`fake::FakeHost`]. Every file and
//! script Harbor writes is a MiniJinja template in `templates/`
//! ([`render`]).
//!
//! ```
//! use navree_harbor::{HarborConfig, Role};
//!
//! let config = HarborConfig::parse(
//!     r#"
//!     app = "crm-api"
//!     domain = "crm.example.com"
//!
//!     [servers.app-1]
//!     host = "203.0.113.10"
//!     roles = ["web", "worker", "scheduler", "postgres", "redis"]
//!     "#,
//!     std::path::Path::new("harbor.toml"),
//! )?;
//! assert!(config.validate().is_empty());
//! assert_eq!(config.deploy_path(), "/srv/crm-api");
//! assert_eq!(config.postgres_server().map(|s| s.name.as_str()), Some("app-1"));
//! assert!(config.servers[0].has(Role::Redis));
//! # Ok::<(), navree_harbor::HarborError>(())
//! ```
#![forbid(unsafe_code)]

pub mod build;
pub mod ci;
pub mod cli;
pub mod config;
pub mod deploy;
pub mod document;
pub mod env;
pub mod error;
pub mod exec;
pub mod fake;
pub mod host;
pub mod image;
pub mod init;
pub mod layout;
pub mod ops;
pub mod output;
pub mod project;
pub mod provision;
pub mod render;
pub mod secrets;
pub mod ssh;

pub use cli::{Context, HarborCommand, run};
pub use config::{Environment, HarborConfig, Role, Server};
pub use error::{HarborError, Result};
pub use project::Project;
pub use secrets::SecretKey;
