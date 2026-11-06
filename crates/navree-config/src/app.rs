//! The `app` section, typed: the app's name, address, version, environment
//! and keys; and [`Environment`], the name of the environment the app runs in.

use std::fmt;

use navree_contracts::Secret;
use serde::{Deserialize, Deserializer};

use crate::{Config, ConfigError, ConfigSection};

/// The environment an app runs in, from `env` (`APP_ENV`) or `app.env`.
///
/// `production` and `staging` change what modules do (see
/// [`behaves_like_production`](Environment::behaves_like_production)).
/// Names are lower-cased and trimmed. `local`, `testing`, `staging` and
/// `production` have their own variants; any other name (`development`,
/// `qa`) is kept as [`Other`](Environment::Other), so the name a person wrote
/// is the name that is read back.
///
/// ```
/// use navree_config::Environment;
///
/// assert_eq!(Environment::parse(" Production "), Environment::Production);
/// assert!(Environment::parse("production").is_production());
/// assert!(!Environment::parse("staging").is_production());
/// assert_eq!(Environment::parse("qa").as_str(), "qa");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Environment {
    /// A developer's machine (`local`).
    Local,
    /// Automated tests (`testing`).
    Testing,
    /// A rehearsal of production (`staging`).
    Staging,
    /// The live app (`production`).
    Production,
    /// Any other name, lower-cased (`development`, `qa`, `preview`).
    Other(String),
}

impl Environment {
    /// The environment named `name`: case and surrounding spaces are ignored.
    /// Never fails; a name this crate does not know is [`Other`](Environment::Other).
    pub fn parse(name: &str) -> Environment {
        let name = name.trim().to_lowercase();
        match name.as_str() {
            "local" => Environment::Local,
            "testing" => Environment::Testing,
            "staging" => Environment::Staging,
            "production" => Environment::Production,
            _ => Environment::Other(name),
        }
    }

    /// The name, lower-cased: `local`, `testing`, `staging`, `production` or
    /// the other name itself.
    pub fn as_str(&self) -> &str {
        match self {
            Environment::Local => "local",
            Environment::Testing => "testing",
            Environment::Staging => "staging",
            Environment::Production => "production",
            Environment::Other(name) => name,
        }
    }

    /// Whether this is `production`.
    pub fn is_production(&self) -> bool {
        matches!(self, Environment::Production)
    }

    /// Whether this is `staging`.
    pub fn is_staging(&self) -> bool {
        matches!(self, Environment::Staging)
    }

    /// Whether this is a developer's machine (`local`).
    pub fn is_local(&self) -> bool {
        matches!(self, Environment::Local)
    }

    /// Whether modules take their production behaviour: destructive
    /// `migrate` and `db:seed` commands ask for `--force`, the OpenAPI
    /// document and its page are off, the ORM's strict modes are off, and
    /// error pages hide their details.
    ///
    /// `production` and `staging` do: a rehearsal of production has to
    /// behave as production does. Every module asks here, so which
    /// environments take the production behaviour is decided in this one
    /// method.
    ///
    /// ```
    /// use navree_config::Environment;
    ///
    /// assert!(Environment::Production.behaves_like_production());
    /// assert!(Environment::Staging.behaves_like_production());
    /// assert!(!Environment::Local.behaves_like_production());
    /// assert!(!Environment::parse("qa").behaves_like_production());
    /// ```
    pub fn behaves_like_production(&self) -> bool {
        matches!(self, Environment::Production | Environment::Staging)
    }

    /// Whether the environment name is usable: letters, digits, `_` and `-`,
    /// starting with a letter or digit.
    fn is_valid_name(&self) -> bool {
        let name = self.as_str();
        name.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Environment {
    fn from(name: &str) -> Self {
        Environment::parse(name)
    }
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|name| Environment::parse(&name))
    }
}

/// The `app` section: what the app is called, where it lives, which
/// environment it runs in, and its keys.
///
/// ```toml
/// [app]
/// name = "agra"
/// url = "https://agra.example.com"   # absolute http or https address
/// version = "1.4.0"                  # optional
/// env = "production"                 # also APP_ENV
/// # key and previous_keys: APP_KEY and APP_PREVIOUS_KEYS (keep secrets out of files)
/// ```
///
/// The short environment variables `APP_NAME`, `APP_URL`, `APP_VERSION`,
/// `APP_ENV`, `APP_KEY` and `APP_PREVIOUS_KEYS` set these keys, as do the
/// full spellings (`APP_APP__URL`). When both are set, the source that ranks
/// higher in the load order wins; within one source `app.<key>` wins, except
/// for `env`, where `APP_ENV` does.
///
/// A blank value counts as not set. Read it with [`Config::app`]; the module
/// registry validates it at boot.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "app", docs = "/docs/getting-started/configuration/", validate = Self::check_values)]
#[serde(default)]
pub struct AppOptions {
    /// The app's name.
    #[serde(deserialize_with = "blank_is_none")]
    pub name: Option<String>,
    /// The address users reach the app at (`https://agra.example.com`): used
    /// to build links in mail and callbacks. An absolute http or https URL.
    #[serde(deserialize_with = "blank_is_none")]
    pub url: Option<String>,
    /// The app's version.
    #[serde(deserialize_with = "blank_is_none")]
    pub version: Option<String>,
    /// The environment the app runs in; none when not set.
    #[serde(deserialize_with = "environment_or_none")]
    pub env: Option<Environment>,
    /// The key that encrypts data and signs tokens (`base64:...`).
    #[serde(deserialize_with = "secret_or_none")]
    pub key: Option<Secret<String>>,
    /// Earlier keys that still decrypt, most recent first: a list, or one
    /// comma-separated string.
    #[serde(deserialize_with = "key_list")]
    pub previous_keys: Vec<Secret<String>>,
    /// Refuse to start when the configuration has a key nothing reads, or,
    /// in an environment that behaves like production, a setting the
    /// production profile fails (no `app.key`, a SQLite database, mail that
    /// goes to the log). Off by default: the boot only logs them.
    pub strict_config: bool,
}

impl AppOptions {
    /// Checks the values (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        if let Some(url) = &self.url
            && !is_absolute_http_url(url)
        {
            problems.push(
                "app.url must be an absolute http or https address, such as https://example.com",
            );
        }
        if let Some(env) = &self.env
            && !env.is_valid_name()
        {
            problems.push(
                "app.env must be a name such as local, testing, staging or production (letters, digits, `_` and `-`)",
            );
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

impl Config {
    /// The `app` section, validated: name, URL, version, environment and
    /// keys. The short names (`APP_URL`, `APP_ENV`, ...) are resolved into it.
    pub fn app(&self) -> Result<AppOptions, ConfigError> {
        self.typed::<AppOptions>()
    }

    /// The `app` section for a reader that has no use for an error: if the
    /// section is invalid, every setting counts as not set. The boot reports
    /// the error.
    fn app_lenient(&self) -> AppOptions {
        self.app().unwrap_or_default()
    }

    /// The environment the app runs in (`APP_ENV` or `app.env`); `None` when
    /// it names none or the name is blank.
    ///
    /// ```
    /// use navree_config::{Config, ConfigSources, Environment};
    ///
    /// let config = Config::load(&ConfigSources::new().env_vars("APP_", [("APP_ENV", "Production")]))?;
    /// assert_eq!(config.environment(), Some(Environment::Production));
    /// # Ok::<(), navree_config::ConfigError>(())
    /// ```
    pub fn environment(&self) -> Option<Environment> {
        self.app_lenient().env
    }

    /// Whether the app runs in an environment that takes the production
    /// behaviour (see [`Environment::behaves_like_production`]); `false` when
    /// no environment is named.
    pub fn behaves_like_production(&self) -> bool {
        self.environment()
            .is_some_and(|env| env.behaves_like_production())
    }

    /// The application key (`app.key`, `APP_KEY`), if set and not blank. The
    /// value stays a [`Secret`].
    pub fn app_key(&self) -> Option<Secret<String>> {
        self.app_lenient().key
    }

    /// The app's name (`app.name`, `APP_NAME`), if set.
    pub fn app_name(&self) -> Option<String> {
        self.app_lenient().name
    }

    /// The app's address (`app.url`, `APP_URL`), if set.
    pub fn app_url(&self) -> Option<String> {
        self.app_lenient().url
    }

    /// The app's version (`app.version`, `APP_VERSION`), if set.
    pub fn app_version(&self) -> Option<String> {
        self.app_lenient().version
    }
}

/// An absolute `http://` or `https://` address with a host and no spaces.
fn is_absolute_http_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|rest| {
        !rest.is_empty()
            && !rest.starts_with(['/', '?', '#'])
            && !url.chars().any(char::is_whitespace)
    })
}

/// A string, or none when it is blank.
fn blank_is_none<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty()))
}

/// An environment name, or none when it is blank.
fn environment_or_none<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Environment>, D::Error> {
    Ok(blank_is_none(deserializer)?.map(|name| Environment::parse(&name)))
}

/// A secret string, or none when it is blank.
fn secret_or_none<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Secret<String>>, D::Error> {
    Ok(blank_is_none(deserializer)?.map(Secret::new))
}

/// `previous_keys` as configured: a list, or one comma-separated string.
#[derive(Deserialize)]
#[serde(untagged)]
enum KeyList {
    List(Vec<String>),
    Joined(String),
}

fn key_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Secret<String>>, D::Error> {
    let keys = match Option::<KeyList>::deserialize(deserializer)? {
        None => return Ok(Vec::new()),
        Some(KeyList::List(keys)) => keys,
        Some(KeyList::Joined(joined)) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(keys
        .into_iter()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .map(Secret::new)
        .collect())
}
