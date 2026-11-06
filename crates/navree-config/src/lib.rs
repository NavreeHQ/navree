//! # navree-config
//!
//! `ConfigModule`: typed configuration sections from `.env`, TOML and JSON
//! files and `APP_` environment variables, validated at boot.
//!
//! The loader is Navree's own. The `toml` and `serde_json` crates parse the
//! files; the sources, their precedence, the deep merge, the environment
//! mapping, the conversion of values to the types of a section and the error
//! messages are this crate's code.
//!
//! - [`ConfigSources`] lists where configuration comes from, in precedence
//!   order (later over earlier, [`ConfigSources::set`] above all).
//!   Environment variables map with prefix `APP_` and nesting separator `__`:
//!   `APP_DATABASE__URL` → `database.url`. Only variables under the prefix
//!   are read. Files are limited in size and nesting
//!   ([`max_file_size`](ConfigSources::max_file_size),
//!   [`max_depth`](ConfigSources::max_depth)).
//! - [`Config`] is the merged tree; [`Config::section`] deserializes a typed
//!   section, converting scalars as needed (`"10"` fills a `u32`).
//! - [`ConfigModule`] is global: it loads [`Config`] at boot and exports it to
//!   every module. A module declares its own typed section with
//!   [`ModuleBuilderConfigExt::config`]; a section that fails to deserialize
//!   or [validate](ConfigSection::validate) stops the boot, naming the section.
//! - [`parse_dotenv`] is Navree's own `.env` parser. It never calls
//!   `std::env::set_var` (unsafe in edition 2024): `.env` entries are a
//!   configuration source, not process environment.
//! - The loader records where every value came from ([`Config::origin`],
//!   [`ValueOrigin`]), notes the keys that `.env` and a config file both set
//!   with different values ([`Config::shadowed`]) and the keys and sections
//!   that nothing reads
//!   ([`Config::unused`]). `ConfigModule` reports both at boot.
//! - The `app` section is typed: [`AppOptions`] and [`Environment`].
//! - `navree config:show` and `navree config:check` (see [`show`] and
//!   [`check`]) print the effective configuration with secrets masked, and
//!   check it without starting the app.
//!
//! Secrets stay out of logs: [`Config::redacted`] (and `Debug`) masks
//! secret-looking keys, URL passwords and secret-looking URL parameters, and
//! no [`ConfigError`] message contains a string value or the value of a
//! secret-looking key.
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

mod app;
pub mod check;
mod commands;
mod config;
mod de;
mod dotenv;
mod error;
mod layers;
mod module;
mod origin;
mod redact;
mod registry;
pub mod show;
mod sources;
mod usage;
mod value;

pub use app::{AppOptions, Environment};
pub use config::Config;
pub use dotenv::{DotenvEntryExists, parse_dotenv, set_dotenv_value};
pub use error::ConfigError;
pub use module::{ConfigModule, ConfigSection, ModuleBuilderConfigExt, defaults_of};
pub use origin::{Shadowed, ValueOrigin};
pub use registry::{FRAMEWORK_SECTIONS, SectionRegistration};
pub use sources::ConfigSources;
pub use usage::UnusedKey;
