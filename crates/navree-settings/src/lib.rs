//! # navree-settings
//!
//! `SettingsModule`: typed settings stored per scope and inherited down the
//! chain global → organization → workspace → user, with the source of
//! every value.
//!
//! - **Settings are types.** `#[derive(Settings)]` (navree-macros,
//!   re-exported here) implements [`SettingsGroup`] for a struct whose
//!   fields are the settings: its stored key, the levels it may be stored
//!   at (`scopes(global, organization, workspace, user)`), its description
//!   and its permissions. `Default` gives the values when nothing is
//!   stored; garde's rules check every write. Register each type with
//!   [`SettingsModule::for_feature`] or `m.settings::<T>()`
//!   ([`ModuleBuilderSettingsExt`]).
//! - **Resolution.** [`Settings::resolve`] reads the chain of a [`Scope`]
//!   (a user's preferences over a workspace's values, over its
//!   organization's, over the global ones, over the defaults) one field at
//!   a time and returns a [`Resolved<T>`]: the value and, for every field,
//!   its [`Source`]. The [`Setting<T>`] extractor resolves for the
//!   request's user and workspace.
//! - **Writes.** [`Settings::update`] sets fields at a scope's level,
//!   validated with garde and refused with the failing fields named;
//!   [`Settings::inherit`] makes a field follow the level above again.
//!   Writes take the unit of work they belong to, clear the cached values
//!   they affect (cache tags per key and level), write a `settings.updated`
//!   audit entry and emit [`SettingsChanged`] after the commit.
//! - **Storage.** One table, `settings` ([`DatabaseSettingsStore`]), or in
//!   process ([`MemorySettingsStore`]), behind [`SettingsStore`].
//! - **Routes and commands.** `GET /api/settings`, `GET`, `PATCH` and
//!   `DELETE /api/settings/{key}?scope=` (`{data, sources, overridden}`),
//!   and `settings:list`, `settings:show`, `settings:set`,
//!   `settings:inherit`.
//! - **Tests.** `settings.store = "memory"`, or a [`SettingsFake`] that
//!   records every change.
//!
//! ```
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! use garde::Validate;
//! use navree_settings::{Scope, SettingsFake, SettingsGroup, Source};
//! use serde::{Deserialize, Serialize};
//! use serde_json::json;
//! use uuid::Uuid;
//!
//! /// What the member portal shows.
//! #[derive(Serialize, Deserialize, Validate, Default)] // and #[derive(Settings)]
//! struct PortalSettings {
//!     #[garde(skip)]
//!     show_leaderboard: bool,
//!     #[garde(length(max = 500))]
//!     welcome_text: String,
//! }
//!
//! impl SettingsGroup for PortalSettings { // what #[derive(Settings)] writes
//!     const KEY: &'static str = "portal";
//! }
//!
//! let acme = Uuid::now_v7();
//! let fake = SettingsFake::new();
//! fake.set::<PortalSettings>(Scope::global(), json!({"show_leaderboard": true})).await.unwrap();
//! fake.set::<PortalSettings>(Scope::workspace(acme), json!({"welcome_text": "Hi"})).await.unwrap();
//!
//! let portal = fake.settings().resolve::<PortalSettings>(Scope::workspace(acme)).await.unwrap();
//! assert!(portal.show_leaderboard);
//! assert_eq!(portal.welcome_text, "Hi");
//! assert_eq!(portal.source("show_leaderboard"), Some(Source::Global));
//! assert_eq!(portal.source("welcome_text"), Some(Source::Workspace));
//! # });
//! ```
#![forbid(unsafe_code)]

pub mod entity;

mod commands;
mod controller;
mod error;
mod events;
mod fake;
mod group;
mod migration;
mod module;
mod options;
mod registry;
mod request;
mod resolve;
mod scope;
mod service;
mod store;

pub use controller::{
    ORGANIZATION_WRITE_PERMISSION, SETTINGS_MANAGE_PERMISSION, ScopeQuery, SettingsController,
    SettingsInfo, SettingsListResource, WORKSPACE_WRITE_PERMISSION,
};
pub use error::SettingsError;
pub use events::SettingsChanged;
pub use fake::SettingsFake;
pub use group::{MAX_KEY_LEN, SettingsDefinition, SettingsGroup, check_key};
pub use migration::SETTINGS_MIGRATIONS;
pub use module::{ModuleBuilderSettingsExt, SettingsModule};
/// `#[derive(Settings)]`: implements [`SettingsGroup`] (see
/// `navree_macros::Settings`).
pub use navree_macros::Settings;
pub use options::{SettingsOptions, SettingsStoreKind};
pub use registry::{RegisteredSettings, SettingsRegistry};
pub use request::Setting;
pub use resolve::{Resolved, ResolvedSettings};
pub use scope::{Scope, ScopeKind, ScopeTarget, Source};
pub use service::Settings;
pub use store::{DatabaseSettingsStore, MemorySettingsStore, SettingsStore, StoredSetting};
