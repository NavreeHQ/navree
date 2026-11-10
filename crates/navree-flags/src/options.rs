//! [`FlagsOptions`]: the `flags` configuration section.

use std::time::Duration;

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

/// Where flag state is stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FlagStoreKind {
    /// The `feature_flags` and `feature_flag_overrides` tables (needs
    /// `DatabaseModule`).
    #[default]
    Database,
    /// In process, lost on restart (tests).
    Memory,
}

/// How a check without a user or a workspace (from a job or a command) is
/// resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MissingScope {
    /// The kill switch, the global state, then the flag's default.
    #[default]
    Default,
    /// Off, unless the flag is on for everyone (and not killed).
    Off,
}

/// The settings of `FlagsModule`: the `flags` configuration section. Every
/// key is optional.
///
/// ```toml
/// [flags]
/// store = "database"        # database | memory
/// cache_ttl_secs = 60       # how long resolved state is cached; 0 = no cache
/// routes = true             # GET /api/flags for the front end
/// admin_routes = true       # /api/admin/flags, with the flags.manage permission
/// missing_scope = "default" # checks without a user or workspace: default | off
/// ```
///
/// From the environment: `APP_FLAGS__STORE=memory`,
/// `APP_FLAGS__CACHE_TTL_SECS=10`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigSection)]
#[config(key = "flags", docs = "/docs/packages/feature-flags/")]
#[serde(default)]
pub struct FlagsOptions {
    /// Where flag state is stored (default `database`).
    #[config(one_of = "database, memory")]
    pub store: FlagStoreKind,
    /// How long the global state, the overrides of a scope and a
    /// workspace's plan stay in the app's cache (`CacheModule`), in
    /// seconds (default 60); `0` turns the cache off. Also the `max-age` of
    /// `GET /api/flags`.
    pub cache_ttl_secs: u64,
    /// Mount `GET {api_prefix}/flags` (default `true`; needs an
    /// authenticator).
    pub routes: bool,
    /// Mount `{api_prefix}/admin/flags...` (default `true`). The routes
    /// require the `flags.manage` permission, so the app needs a
    /// `dyn RouteAuthorizer` (navree-access); turn them off in apps without
    /// one.
    pub admin_routes: bool,
    /// How checks without a user or a workspace resolve (default
    /// `default`).
    #[config(one_of = "default, off")]
    pub missing_scope: MissingScope,
}

impl Default for FlagsOptions {
    fn default() -> Self {
        Self {
            store: FlagStoreKind::Database,
            cache_ttl_secs: 60,
            routes: true,
            admin_routes: true,
            missing_scope: MissingScope::Default,
        }
    }
}

impl FlagsOptions {
    /// The cache lifetime, `None` when the cache is off.
    pub fn cache_ttl(&self) -> Option<Duration> {
        (self.cache_ttl_secs > 0).then(|| Duration::from_secs(self.cache_ttl_secs))
    }
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = FlagsOptions::schema().expect("schema");
        assert_eq!(schema.key, "flags");
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(
            field("store").ty,
            FieldType::choice(&["database", "memory"])
        );
        assert_eq!(
            field("missing_scope").ty,
            FieldType::choice(&["default", "off"])
        );
        assert_eq!(field("cache_ttl_secs").ty, FieldType::Integer);
        let defaults = FlagsOptions::defaults().expect("defaults");
        assert_eq!(defaults["store"], "database");
        assert_eq!(defaults["missing_scope"], "default");
        assert_eq!(defaults["cache_ttl_secs"], 60);
    }

    #[test]
    fn defaults_match_the_documentation() {
        let options = FlagsOptions::default();
        assert_eq!(options.store, FlagStoreKind::Database);
        assert_eq!(options.cache_ttl_secs, 60);
        assert!(options.routes && options.admin_routes);
        assert_eq!(options.missing_scope, MissingScope::Default);
        assert_eq!(options.cache_ttl(), Some(Duration::from_secs(60)));
        let off = FlagsOptions {
            cache_ttl_secs: 0,
            ..FlagsOptions::default()
        };
        assert_eq!(off.cache_ttl(), None);
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let options: FlagsOptions =
            serde_json::from_value(serde_json::json!({"store": "memory", "missing_scope": "off"}))
                .unwrap();
        assert_eq!(options.store, FlagStoreKind::Memory);
        assert_eq!(options.missing_scope, MissingScope::Off);
        assert_eq!(options.cache_ttl_secs, 60);
        assert!(
            serde_json::from_value::<FlagsOptions>(serde_json::json!({"store": "redis"})).is_err()
        );
    }
}
