//! # navree-settings
//!
//! `SettingsModule`: typed settings stored per scope and inherited down the
//! chain global → organization → workspace → user.
#![forbid(unsafe_code)]

mod error;
mod group;
mod resolve;
mod scope;

pub use error::SettingsError;
pub use group::{MAX_KEY_LEN, SettingsDefinition, SettingsGroup, check_key};
pub use resolve::{Resolved, ResolvedSettings};
pub use scope::{Scope, ScopeKind, ScopeTarget, Source};
