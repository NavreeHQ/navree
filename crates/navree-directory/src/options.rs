//! [`DirectoryOptions`]: the `directory` configuration section.

use navree_config::ConfigSection;
use navree_integrations::Level;
use serde::Deserialize;

/// Options of the directory module (configuration section `directory`).
///
/// ```toml
/// [directory]
/// level = "workspace"              # workspace | organization | app
///
/// [directory.scim]
/// path = "/scim/v2"
/// base_url = "https://app.acme.test/scim/v2"
///
/// [directory.sync]
/// default_interval_mins = 60
/// max_deactivation_percent = 20
/// ```
///
/// Every key also comes from the environment:
/// `APP_DIRECTORY__SYNC__MAX_DEACTIVATION_PERCENT=10`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DirectoryOptions {
    /// Mount the `{http.api_prefix}/directory` routes (default true;
    /// permission `directory.manage`).
    pub routes: bool,
    /// Whose connections the routes manage: the request's workspace
    /// (default), its organization, or the app's own. Without
    /// `WorkspaceModule` in the app, `workspace` means the app's own.
    pub level: Level,
    /// The SCIM server.
    pub scim: ScimOptions,
    /// Sync runs.
    pub sync: SyncSettings,
}

impl Default for DirectoryOptions {
    fn default() -> Self {
        Self {
            routes: true,
            level: Level::Workspace,
            scim: ScimOptions::default(),
            sync: SyncSettings::default(),
        }
    }
}

/// The SCIM server (`directory.scim`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ScimOptions {
    /// Serve SCIM (default true).
    pub enabled: bool,
    /// Where the SCIM endpoints are mounted, outside the API prefix
    /// (default `/scim/v2`).
    pub path: String,
    /// The public URL of `path`, for `meta.location` (default `app.url`
    /// followed by `path`; a relative location without either).
    pub base_url: Option<String>,
    /// The most resources one list response holds (default 200).
    pub max_results: usize,
}

impl Default for ScimOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/scim/v2".to_owned(),
            base_url: None,
            max_results: 200,
        }
    }
}

/// Sync runs (`directory.sync`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    /// Minutes between a new connection's scheduled syncs (default 60;
    /// each connection has its own `sync_interval_mins`, and 0 turns its
    /// scheduled syncs off).
    pub default_interval_mins: u32,
    /// Changes applied per transaction (default 100).
    pub chunk_size: usize,
    /// A run that would deactivate more than this share of the active
    /// users is held (default 20 percent; 100 turns the limit off).
    pub max_deactivation_percent: u8,
    /// ...and more than this many users (default 10).
    pub min_deactivations: u32,
    /// Changes recorded per run, for review (default 5000; the counts are
    /// always complete).
    pub max_recorded_changes: usize,
    /// `directory:prune` deletes runs older than this many days (default
    /// 90).
    pub keep_runs_days: u32,
    /// A queued or running run older than this many minutes is taken for
    /// a crashed worker's: it is marked failed, and a new sync may start
    /// (default 120; the sync job's own time limit is one hour).
    pub stale_after_mins: u32,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            default_interval_mins: 60,
            chunk_size: 100,
            max_deactivation_percent: 20,
            min_deactivations: 10,
            max_recorded_changes: 5000,
            keep_runs_days: 90,
            stale_after_mins: 120,
        }
    }
}

impl DirectoryOptions {
    /// Every problem with these options, each naming its key.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let path = &self.scim.path;
        if !path.starts_with('/') || path.len() < 2 || path.ends_with('/') || path.contains(' ') {
            errors.push(format!(
                "directory.scim.path must start with `/` and not end with one, got `{path}`"
            ));
        }
        if let Some(base) = &self.scim.base_url
            && !url::Url::parse(base).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
        {
            errors.push(format!(
                "directory.scim.base_url must be an absolute http(s) URL, got `{base}`"
            ));
        }
        if self.scim.max_results == 0 {
            errors.push("directory.scim.max_results must be at least 1".to_owned());
        }
        if self.sync.chunk_size == 0 {
            errors.push("directory.sync.chunk_size must be at least 1".to_owned());
        }
        if self.sync.max_deactivation_percent > 100 {
            errors.push("directory.sync.max_deactivation_percent must be at most 100".to_owned());
        }
        if self.sync.stale_after_mins < 61 {
            errors.push(
                "directory.sync.stale_after_mins must be more than 60 (the sync job's time limit)"
                    .to_owned(),
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl ConfigSection for DirectoryOptions {
    const KEY: &'static str = "directory";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_are_the_documented_ones() {
        let options = DirectoryOptions::default();
        assert!(options.routes);
        assert_eq!(options.level, Level::Workspace);
        assert!(options.scim.enabled);
        assert_eq!(options.scim.path, "/scim/v2");
        assert_eq!(options.scim.max_results, 200);
        assert_eq!(options.sync.default_interval_mins, 60);
        assert_eq!(options.sync.chunk_size, 100);
        assert_eq!(options.sync.max_deactivation_percent, 20);
        assert_eq!(options.sync.min_deactivations, 10);
        assert_eq!(options.sync.max_recorded_changes, 5000);
        assert_eq!(options.sync.keep_runs_days, 90);
        assert_eq!(options.sync.stale_after_mins, 120);
        assert!(options.check().is_ok());
    }

    #[test]
    fn options_read_from_configuration() {
        let options: DirectoryOptions = serde_json::from_value(json!({
            "level": "organization",
            "scim": { "path": "/identity/scim", "base_url": "https://app.acme.test/identity/scim" },
            "sync": { "max_deactivation_percent": 5 }
        }))
        .unwrap();
        assert_eq!(options.level, Level::Organization);
        assert_eq!(options.scim.path, "/identity/scim");
        assert_eq!(options.sync.max_deactivation_percent, 5);
        assert_eq!(options.sync.chunk_size, 100);
    }

    #[test]
    fn bad_options_name_their_keys() {
        let options = DirectoryOptions {
            scim: ScimOptions {
                path: "scim/".into(),
                base_url: Some("ftp://x".into()),
                max_results: 0,
                ..ScimOptions::default()
            },
            sync: SyncSettings {
                chunk_size: 0,
                max_deactivation_percent: 101,
                stale_after_mins: 30,
                ..SyncSettings::default()
            },
            ..DirectoryOptions::default()
        };
        let errors = options.check().unwrap_err();
        assert_eq!(errors.len(), 6, "{errors:?}");
        for key in [
            "directory.scim.path",
            "directory.scim.base_url",
            "directory.scim.max_results",
            "directory.sync.chunk_size",
            "directory.sync.max_deactivation_percent",
            "directory.sync.stale_after_mins",
        ] {
            assert!(errors.iter().any(|e| e.starts_with(key)), "{key}: {errors:?}");
        }
    }
}
