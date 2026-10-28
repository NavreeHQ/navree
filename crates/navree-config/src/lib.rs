//! # navree-config
//!
//! `ConfigModule`: typed configuration sections from `.env`, TOML files and
//! `APP_` environment variables, validated at boot.
//!
//! - [`ConfigSources`] lists where configuration comes from, in precedence
//!   order (later over earlier, [`ConfigSources::set`] above all).
//!   Environment variables map with prefix `APP_` and nesting separator `__`:
//!   `APP_DATABASE__URL` → `database.url`.
//! - [`Config`] is the merged tree; [`Config::section`] deserializes a typed
//!   section, converting environment strings as needed.
//! - [`ConfigModule`] is global: it loads [`Config`] at boot and exports it to
//!   every module. A module declares its own typed section with
//!   [`ModuleBuilderConfigExt::config`]; a section that fails to deserialize
//!   or [validate](ConfigSection::validate) stops the boot, naming the section.
//! - [`parse_dotenv`] is Navree's own `.env` parser. It never calls
//!   `std::env::set_var` (unsafe in edition 2024): `.env` entries are a
//!   configuration source, not process environment.
//!
//! Secrets stay out of logs: [`Config::redacted`] (and `Debug`) masks
//! secret-looking keys and URL passwords, and no [`ConfigError`] message
//! contains a configuration value.
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt};
//! use navree_kernel::{Navree, Module, ModuleBuilder};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! struct BillingConfig {
//!     currency: String,
//! }
//!
//! impl ConfigSection for BillingConfig {
//!     const KEY: &'static str = "billing";
//! }
//!
//! struct BillingModule;
//!
//! impl Module for BillingModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         // Providers of this module can now depend on `Arc<BillingConfig>`.
//!         m.config::<BillingConfig>();
//!     }
//! }
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     Navree::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(BillingModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

mod config;
mod de;
mod dotenv;
mod error;
mod layers;
mod module;
mod redact;
mod sources;
mod value;

pub use config::Config;
pub use dotenv::parse_dotenv;
pub use error::ConfigError;
pub use module::{ConfigModule, ConfigSection, ModuleBuilderConfigExt};
pub use sources::ConfigSources;
