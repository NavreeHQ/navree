//! [`SettingsOptions`]: the `settings` configuration section.

use std::time::Duration;

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

/// Where stored values live.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsStoreKind {
    /// The `settings` table (needs `DatabaseModule`).
    #[default]
    Database,
    /// In process, lost on restart (tests).
    Memory,
}

/// The options of `SettingsModule`: the `settings` configuration section.
/// Every key is optional.
///
/// ```toml
/// [settings]
/// store = "database"    # database | memory
/// cache_ttl_secs = 300  # how long resolved values are cached; 0 = no cache
/// routes = true         # GET /api/settings and GET/PATCH/DELETE /api/settings/{key}
/// ```
///
/// From the environment: `APP_SETTINGS__STORE=memory`,
/// `APP_SETTINGS__CACHE_TTL_SECS=60`, `APP_SETTINGS__ROUTES=false`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsOptions {
    /// Where stored values live (default `database`).
    pub store: SettingsStoreKind,
    /// How long resolved values stay in the app's cache (`CacheModule`), in
    /// seconds (default 300); `0` turns the cache off. Every write clears
    /// the entries it affects, so the lifetime only bounds what other
    /// processes see after a change made without a cache they share.
    pub cache_ttl_secs: u64,
    /// Mount `{api_prefix}/settings...` (default `true`; needs an
    /// authenticator).
    pub routes: bool,
}

impl Default for SettingsOptions {
    fn default() -> Self {
        Self {
            store: SettingsStoreKind::Database,
            cache_ttl_secs: 300,
            routes: true,
        }
    }
}

impl SettingsOptions {
    /// The cache lifetime, `None` when the cache is off.
    pub fn cache_ttl(&self) -> Option<Duration> {
        (self.cache_ttl_secs > 0).then(|| Duration::from_secs(self.cache_ttl_secs))
    }
}

impl ConfigSection for SettingsOptions {
    const KEY: &'static str = "settings";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documentation() {
        let options = SettingsOptions::default();
        assert_eq!(options.store, SettingsStoreKind::Database);
        assert_eq!(options.cache_ttl_secs, 300);
        assert!(options.routes);
        assert_eq!(options.cache_ttl(), Some(Duration::from_secs(300)));
        let off = SettingsOptions {
            cache_ttl_secs: 0,
            ..SettingsOptions::default()
        };
        assert_eq!(off.cache_ttl(), None);
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let options: SettingsOptions =
            serde_json::from_value(serde_json::json!({"store": "memory"})).unwrap();
        assert_eq!(options.store, SettingsStoreKind::Memory);
        assert_eq!(options.cache_ttl_secs, 300);
        assert!(options.routes);
        assert!(
            serde_json::from_value::<SettingsOptions>(serde_json::json!({"store": "redis"}))
                .is_err()
        );
    }
}
