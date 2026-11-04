//! [`ApiOptions`]: the `api` configuration section.

use navree_config::{ConfigSection, Environment};
use serde::{Deserialize, Deserializer};

/// OpenAPI document and docs UI settings (configuration section `api`;
/// every key optional).
///
/// ```toml
/// [api]
/// title = "CRM API"
/// version = "1.0.0"
/// servers = ["https://crm.example.com"]
/// docs_ui = true                 # unset: off in production, see below
/// openapi_json = true
/// require_docs = true            # an undocumented route stops the boot
/// undocumented = ["GET /storage/*"]
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ApiOptions {
    /// The document title, `info.title` (default `Navree API`).
    pub title: String,
    /// The API version, `info.version` (default `1.0.0`).
    pub version: String,
    /// Server URLs listed in the document (default none: clients use the
    /// URL they loaded the document from). From the environment, a
    /// comma-separated list (`APP_API__SERVERS=https://a.example,https://b.example`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub servers: Vec<String>,
    /// Serve the API reference page at `{api_prefix}/docs`, when
    /// [`openapi_json`](Self::openapi_json) is on too: the page shows what the
    /// document says. Public: nothing signs a reader in. Unset (`None`, the
    /// default), it follows the environment (see
    /// [`serves_docs_ui_in`](Self::serves_docs_ui_in)).
    pub docs_ui: Option<bool>,
    /// Serve the document at `{api_prefix}/openapi.json`. Public: anyone can
    /// read every documented route there. Off, nothing of it is served: not
    /// this route, and not the page. `openapi:export` writes the document
    /// either way. Unset (`None`, the default), it follows the environment
    /// (see [`serves_openapi_json_in`](Self::serves_openapi_json_in)).
    pub openapi_json: Option<bool>,
    /// Refuse to boot while a route has no `#[api]` documentation (default
    /// `false`; `navree check` reports undocumented routes either way).
    pub require_docs: bool,
    /// Routes allowed without documentation: `"METHOD /path"` or `"/path"`
    /// (every method), with a trailing `*` matching any rest of the path
    /// (`"GET /storage/*"`). The path is the mounted one, prefix and
    /// version included. From the environment, a comma-separated list.
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub undocumented: Vec<String>,
}

impl Default for ApiOptions {
    fn default() -> Self {
        Self {
            title: "Navree API".to_owned(),
            version: "1.0.0".to_owned(),
            servers: Vec::new(),
            docs_ui: None,
            openapi_json: None,
            require_docs: false,
            undocumented: Vec::new(),
        }
    }
}

impl ApiOptions {
    /// Whether `{api_prefix}/openapi.json` is served in environment `env`
    /// (`None` when the app names none): [`openapi_json`](Self::openapi_json)
    /// when it is set; otherwise off in `production`, on in any other
    /// environment, and with no environment on in a debug build and off in
    /// a release build.
    pub fn serves_openapi_json_in(&self, env: Option<&str>) -> bool {
        self.openapi_json
            .unwrap_or_else(|| on_by_default(env, cfg!(debug_assertions)))
    }

    /// Whether the reference page at `{api_prefix}/docs` is served in
    /// environment `env`: the document is served
    /// ([`serves_openapi_json_in`](Self::serves_openapi_json_in)) and
    /// [`docs_ui`](Self::docs_ui) is on, by the same rule when it is unset.
    pub fn serves_docs_ui_in(&self, env: Option<&str>) -> bool {
        self.serves_openapi_json_in(env)
            && self
                .docs_ui
                .unwrap_or_else(|| on_by_default(env, cfg!(debug_assertions)))
    }
}

/// The default of `docs_ui` and `openapi_json`: off in `production` (in any
/// case), on in any other environment; with no environment (or a blank
/// one), on in a debug build and off in a release build.
fn on_by_default(env: Option<&str>, debug_build: bool) -> bool {
    match env.map(str::trim).filter(|env| !env.is_empty()) {
        Some(env) => !Environment::parse(env).behaves_like_production(),
        None => debug_build,
    }
}

impl ConfigSection for ApiOptions {
    const KEY: &'static str = "api";

    fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        if self.title.trim().is_empty() {
            errors.push("api.title must not be empty".to_owned());
        }
        if self.version.trim().is_empty() {
            errors.push("api.version must not be empty".to_owned());
        }
        for entry in &self.undocumented {
            if !entry.contains('/') {
                errors.push(format!(
                    "api.undocumented: `{entry}` is not a route (\"METHOD /path\" or \"/path\", `*` at the end matches the rest)"
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

/// A list, or one comma-separated string (what an environment variable
/// holds); entries are trimmed and empty ones dropped.
fn list_or_comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum List {
        Items(Vec<String>),
        Joined(String),
    }
    let items = match List::deserialize(deserializer)? {
        List::Items(items) => items,
        List::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_in_production_and_on_in_any_other_environment() {
        for debug_build in [true, false] {
            assert!(!on_by_default(Some("production"), debug_build));
            assert!(!on_by_default(Some("PRODUCTION"), debug_build));
            assert!(!on_by_default(Some(" production "), debug_build));
            assert!(on_by_default(Some("development"), debug_build));
            assert!(on_by_default(Some("staging"), debug_build));
            assert!(on_by_default(Some("testing"), debug_build));
        }
    }

    #[test]
    fn with_no_environment_on_in_a_debug_build_and_off_in_a_release_build() {
        assert!(on_by_default(None, true));
        assert!(!on_by_default(None, false));
        assert!(on_by_default(Some(""), true));
        assert!(!on_by_default(Some("  "), false));
    }

    #[test]
    fn an_explicit_setting_wins_in_every_environment() {
        let on = ApiOptions {
            docs_ui: Some(true),
            openapi_json: Some(true),
            ..ApiOptions::default()
        };
        let off = ApiOptions {
            docs_ui: Some(false),
            openapi_json: Some(false),
            ..ApiOptions::default()
        };
        for env in [None, Some("production"), Some("development")] {
            assert!(on.serves_openapi_json_in(env), "{env:?}");
            assert!(on.serves_docs_ui_in(env), "{env:?}");
            assert!(!off.serves_openapi_json_in(env), "{env:?}");
            assert!(!off.serves_docs_ui_in(env), "{env:?}");
        }
    }

    #[test]
    fn the_page_needs_the_document() {
        let options = ApiOptions {
            docs_ui: Some(true),
            ..ApiOptions::default()
        };
        assert!(!options.serves_docs_ui_in(Some("production")));
        let options = ApiOptions {
            openapi_json: Some(true),
            ..ApiOptions::default()
        };
        assert!(options.serves_openapi_json_in(Some("production")));
        assert!(!options.serves_docs_ui_in(Some("production")));
    }

    #[test]
    fn unset_by_default() {
        let options = ApiOptions::default();
        assert_eq!((options.docs_ui, options.openapi_json), (None, None));
        assert_eq!(options.serves_openapi_json_in(None), cfg!(debug_assertions));
        assert_eq!(options.serves_docs_ui_in(None), cfg!(debug_assertions));
    }
}
