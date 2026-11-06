//! The sections an app registers.
//!
//! `m.config::<T>()` contributes a [`SectionRegistration`] for `T`, so the app
//! knows its sections without building them: a failing boot lists the
//! problems of every registered section, and `config:check` and `config:show`
//! work on a module graph that was never booted.

use serde_json::{Map, Value as Json, json};

use crate::schema::{EnvFallback, SectionSchema};
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
    schema: fn() -> Option<SectionSchema>,
}

impl SectionRegistration {
    /// The registration of section `T`.
    pub fn of<T: ConfigSection>() -> Self {
        SectionRegistration {
            key: T::KEY,
            type_name: std::any::type_name::<T>(),
            check: |config| config.typed::<T>().map(drop),
            defaults: T::defaults,
            schema: T::schema,
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

    /// What the section says about itself, when its type describes its
    /// fields ([`ConfigSection::schema`]).
    pub fn schema(&self) -> Option<SectionSchema> {
        (self.schema)()
    }
}

/// A key, and everything below it, that a module reads by itself in a way
/// the notes of the configuration cannot see (a billing provider that reads
/// `billing.<name>` when it is built). `config:check` and the boot do not
/// report such keys as unknown.
///
/// A module contributes it with
/// [`ModuleBuilderConfigExt::config_claim`](crate::ModuleBuilderConfigExt::config_claim).
///
/// ```
/// use navree_config::ConfigClaim;
///
/// assert_eq!(ConfigClaim::new("billing.razorpay").key(), "billing.razorpay");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigClaim {
    key: String,
}

impl ConfigClaim {
    /// A claim on `key`.
    pub fn new(key: impl Into<String>) -> Self {
        ConfigClaim { key: key.into() }
    }

    /// The claimed key.
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// One section in a [`ConfigRegistry`].
#[derive(Clone, Debug)]
pub struct SectionInfo {
    /// The section key (`mail`, `database.mongodb`).
    pub key: &'static str,
    /// The Rust type that reads the section.
    pub type_name: &'static str,
    /// Its fields, when the type describes them.
    pub schema: Option<SectionSchema>,
    /// Its defaults, when the type publishes them.
    pub defaults: Option<Json>,
}

/// The sections an app registers, with what each says about itself: fields,
/// defaults, credentials and environment fallbacks. Built from the app's
/// [`SectionRegistration`]s, so it needs no booted app.
///
/// ```
/// use navree_config::{AppOptions, ConfigRegistry, SectionRegistration};
///
/// let registry = ConfigRegistry::new(&[SectionRegistration::of::<AppOptions>()]);
/// assert_eq!(registry.sections()[0].key, "app");
/// assert_eq!(registry.defaults_tree()["app"]["previous_keys"], serde_json::json!([]));
/// ```
#[derive(Clone, Debug)]
pub struct ConfigRegistry {
    sections: Vec<SectionInfo>,
}

impl ConfigRegistry {
    /// The registry of `registrations`: each section once (a section
    /// registered by two modules is one), in key order.
    pub fn new(registrations: &[SectionRegistration]) -> Self {
        let mut sections: Vec<SectionInfo> = Vec::new();
        for registration in registrations {
            let known = sections.iter().any(|section| {
                section.key == registration.key() && section.type_name == registration.type_name()
            });
            if known {
                continue;
            }
            sections.push(SectionInfo {
                key: registration.key(),
                type_name: registration.type_name(),
                schema: registration.schema(),
                defaults: registration.defaults(),
            });
        }
        sections.sort_by(|a, b| a.key.cmp(b.key));
        ConfigRegistry { sections }
    }

    /// The sections, by key.
    pub fn sections(&self) -> &[SectionInfo] {
        &self.sections
    }

    /// The section registered at `key`.
    pub fn section(&self, key: &str) -> Option<&SectionInfo> {
        self.sections.iter().find(|section| section.key == key)
    }

    /// The defaults of every section that publishes them, as one JSON
    /// object with each section at its key (`database.mongodb` nests).
    pub fn defaults_tree(&self) -> Json {
        let mut tree = Json::Object(Map::new());
        for section in &self.sections {
            if let Some(defaults) = &section.defaults {
                crate::show::insert(&mut tree, section.key, defaults.clone());
            }
        }
        tree
    }

    /// The keys of the fields that are credentials (`mail.api_key`).
    pub fn secret_paths(&self) -> Vec<String> {
        self.schemas()
            .flat_map(SectionSchema::secret_paths)
            .collect()
    }

    /// The environment variables fields fall back to.
    pub fn env_fallbacks(&self) -> Vec<EnvFallback> {
        self.schemas()
            .flat_map(SectionSchema::env_fallbacks)
            .collect()
    }

    /// The keys of the fields that are public though their names look like
    /// a credential's (`#[config(public)]`).
    pub fn public_paths(&self) -> Vec<String> {
        self.schemas()
            .flat_map(|schema| {
                schema
                    .leaves()
                    .into_iter()
                    .filter(|(_, field)| field.public)
                    .map(|(path, _)| path)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The redactor of this app's configuration: the framework's list of
    /// secret words and `key`, then what the sections know about their own
    /// fields. A field typed as a secret is masked whatever its name, and a
    /// field marked public (`realtime.key`) is not masked because of it.
    pub fn redactor(&self) -> navree_contracts::Redactor {
        crate::redact::redactor()
            .clone()
            .with_secret_paths(self.secret_paths())
            .with_public_paths(self.public_paths())
    }

    /// The text fields named like a credential that are not marked as one
    /// (see [`SectionSchema::unmarked_secrets`]).
    pub fn unmarked_secrets(&self) -> Vec<String> {
        self.schemas()
            .flat_map(SectionSchema::unmarked_secrets)
            .collect()
    }

    /// The sections that describe themselves.
    fn schemas(&self) -> impl Iterator<Item = &SectionSchema> {
        self.sections
            .iter()
            .filter_map(|section| section.schema.as_ref())
    }

    /// A JSON Schema of the whole configuration tree: a property per
    /// section, with its fields, their rustdoc as `description`, `x-secret`
    /// on credentials and `x-env-fallback` on fields that fall back to an
    /// environment variable. Editors read it for completion and type checks
    /// of `config/app.toml`.
    pub fn schema_document(&self) -> Json {
        let mut root = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "Navree configuration",
            "type": "object",
            "properties": {},
        });
        // Key order puts `database` before `database.mongodb`.
        for section in &self.sections {
            if let Some(schema) = &section.schema {
                let path: Vec<&str> = section.key.split('.').collect();
                insert_section(&mut root, &path, schema.json_schema());
            }
        }
        root
    }
}

/// Puts `schema` at `path` below the `properties` of `node`, making the
/// tables on the way.
fn insert_section(node: &mut Json, path: &[&str], schema: Json) {
    let Some(properties) = node.get_mut("properties").and_then(Json::as_object_mut) else {
        return;
    };
    match path {
        [] => {}
        [last] => {
            properties.insert((*last).to_owned(), schema);
        }
        [head, rest @ ..] => {
            let child = properties
                .entry((*head).to_owned())
                .or_insert_with(|| json!({ "type": "object", "properties": {} }));
            insert_section(child, rest, schema);
        }
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
    "flags",
    "fleet",
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
