//! The sections an app registers.
//!
//! `m.config::<T>()` contributes a [`SectionRegistration`] for `T`, so the app
//! knows its sections without building them: a failing boot lists the
//! problems of every registered section, and `config:check` and `config:show`
//! work on a module graph that was never booted.

use crate::{Config, ConfigError, ConfigSection};

/// A section that a module registered with
/// [`ModuleBuilderConfigExt::config`](crate::ModuleBuilderConfigExt::config).
///
/// ```
/// use navree_config::{AppOptions, Config, ConfigSection, ConfigSources, SectionRegistration};
///
/// let registration = SectionRegistration::of::<AppOptions>();
/// assert_eq!(registration.key(), AppOptions::KEY);
///
/// let config = Config::load(&ConfigSources::new().set("app.url", "not a url"))?;
/// assert!(registration.check(&config).is_err());
/// # Ok::<(), navree_config::ConfigError>(())
/// ```
#[derive(Clone)]
pub struct SectionRegistration {
    key: &'static str,
    type_name: &'static str,
    check: fn(&Config) -> Result<(), ConfigError>,
    defaults: fn() -> Option<serde_json::Value>,
}

impl SectionRegistration {
    /// The registration of section `T`.
    pub fn of<T: ConfigSection>() -> Self {
        SectionRegistration {
            key: T::KEY,
            type_name: std::any::type_name::<T>(),
            check: |config| config.typed::<T>().map(drop),
            defaults: T::defaults,
        }
    }

    /// The section key (`"mail"`).
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The Rust type that reads the section.
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// Reads and validates the section from `config`, as the boot does.
    pub fn check(&self, config: &Config) -> Result<(), ConfigError> {
        (self.check)(config)
    }

    /// The values the section has when nothing configures it, when its type
    /// publishes them ([`ConfigSection::defaults`]).
    pub fn defaults(&self) -> Option<serde_json::Value> {
        (self.defaults)()
    }
}

impl std::fmt::Debug for SectionRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SectionRegistration")
            .field("key", &self.key)
            .field("type", &self.type_name)
            .finish()
    }
}

/// Every problem the registered sections have in `config`, one per failing
/// section, in registration order. A section registered twice is read once.
pub(crate) fn problems(config: &Config, registrations: &[SectionRegistration]) -> Vec<ConfigError> {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    let mut problems = Vec::new();
    for registration in registrations {
        let id = (registration.key, registration.type_name);
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        if let Err(error) = registration.check(config) {
            problems.push(error);
        }
    }
    problems
}

/// The error for a boot that failed on `first`: the problems of all
/// registered sections when more than one fails, else `first` alone.
pub(crate) fn boot_error(
    config: &Config,
    registrations: &[SectionRegistration],
    first: ConfigError,
) -> ConfigError {
    let mut all = problems(config, registrations);
    match all.len() {
        0 => first,
        1 => all.remove(0),
        _ => ConfigError::Invalid { problems: all },
    }
}

/// The top-level sections, and the nested sections, that the framework's
/// modules read. `config:check` has no booted app to ask which of them a
/// given app uses, so it does not report them as unknown.
///
/// The framework's own tests keep this list equal to the `ConfigSection`
/// keys of its crates.
pub const FRAMEWORK_SECTIONS: &[&str] = &[
    "ai",
    "api",
    "app",
    "audit",
    "auth",
    "billing",
    "cache",
    "database",
    "database.mongodb",
    "directory",
    "documents",
    "domains",
    "fleet",
    "flags",
    "graphql",
    "grpc",
    "hashing",
    "helm",
    "http",
    "http_client",
    "i18n",
    "integrations",
    "lighthouse",
    "log",
    "logbook",
    "mail",
    "mcp",
    "media",
    "micro",
    "notify",
    "oauth",
    "observe",
    "observe.tracing",
    "orm",
    "queue",
    "realtime",
    "redis",
    "schema",
    "search",
    "session",
    "settings",
    "sheets",
    "social",
    "sso",
    "storage",
    "time",
    "ui",
    "validation",
    "view",
    "webhooks",
    "workflows",
    "workspace",
];
