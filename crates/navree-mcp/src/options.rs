//! [`McpOptions`]: the `mcp` configuration section, and [`McpSettings`],
//! the options with their defaults resolved at boot.

use std::fmt;

use navree_config::{Config, ConfigSection};
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

/// How MCP clients authenticate (`mcp.auth`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpAuth {
    /// `Authorization: Bearer <token>`, checked by the app's authenticators
    /// like any API request (a navree-auth access token, a personal access
    /// token, ...). A request without a valid token gets a 401 with
    /// `WWW-Authenticate: Bearer`.
    #[default]
    Bearer,
    /// Bearer tokens that MCP clients obtain themselves through OAuth: the
    /// app serves the protected resource metadata (RFC 9728) at
    /// `/.well-known/oauth-protected-resource`, and a 401 names it in
    /// `WWW-Authenticate: Bearer resource_metadata="..."`. Tokens are checked
    /// by the app's authenticators (the OAuth server's strategy).
    Oauth,
}

impl McpAuth {
    /// `bearer` or `oauth`.
    pub fn as_str(self) -> &'static str {
        match self {
            McpAuth::Bearer => "bearer",
            McpAuth::Oauth => "oauth",
        }
    }
}

impl fmt::Display for McpAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The settings of `McpModule`: the `mcp` configuration section. Every key
/// is optional.
///
/// ```toml
/// [mcp]
/// path = "/mcp"
/// name = "navree-crm"
/// title = "Navree CRM"
/// version = "1.4.0"
/// instructions = "Tools for the Navree CRM. Amounts are in cents."
/// auth = "bearer"                       # or "oauth"
/// allowed_origins = ["https://crm.example.com"]
/// session_ttl_secs = 3600
/// max_request_bytes = 1048576
///
/// [mcp.oauth]                           # with auth = "oauth"
/// resource = "https://crm.example.com/mcp"          # default: app.url + path
/// authorization_servers = ["https://crm.example.com"] # default: [app.url]
/// scopes = ["deals.read", "activities.create"]
/// ```
///
/// Environment variables are strings, so the lists also accept a
/// comma-separated value (`APP_MCP__ALLOWED_ORIGINS=https://a.example,https://b.example`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct McpOptions {
    /// Where the HTTP transport is served (default `/mcp`). Absolute: no
    /// API prefix, no version.
    pub path: String,
    /// The server name clients show; default the `app.name` setting, else
    /// the executable's name.
    pub name: Option<String>,
    /// A display title; default the name.
    pub title: Option<String>,
    /// The server version clients see; default the `app.version` setting,
    /// else `0.0.0`.
    pub version: Option<String>,
    /// Text the client gives the model about the server: what it is for
    /// and how its data is shaped.
    pub instructions: Option<String>,
    /// `bearer` (default) or `oauth`.
    pub auth: McpAuth,
    /// Browser origins allowed to call the endpoint (`https://crm.example.com`).
    /// A request with any other `Origin` header gets a 403; requests without
    /// one (MCP clients are not browsers) pass. A list, or a comma-separated
    /// string.
    #[serde(deserialize_with = "list_or_csv")]
    pub allowed_origins: Vec<String>,
    /// How long an idle session lives, in seconds (default 3600).
    pub session_ttl_secs: u64,
    /// The largest JSON-RPC message accepted, in bytes (default 1 MiB).
    pub max_request_bytes: usize,
    /// The OAuth settings, used with `auth = "oauth"`.
    pub oauth: McpOAuthOptions,
}

impl Default for McpOptions {
    fn default() -> Self {
        Self {
            path: "/mcp".to_owned(),
            name: None,
            title: None,
            version: None,
            instructions: None,
            auth: McpAuth::Bearer,
            allowed_origins: Vec::new(),
            session_ttl_secs: 3600,
            max_request_bytes: 1024 * 1024,
            oauth: McpOAuthOptions::default(),
        }
    }
}

/// The `mcp.oauth` section: what the protected resource metadata says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct McpOAuthOptions {
    /// The resource identifier clients request tokens for: the endpoint's
    /// absolute URL. Default: the `app.url` setting followed by
    /// [`McpOptions::path`].
    pub resource: Option<String>,
    /// The issuers of the authorization servers that grant tokens for it.
    /// Default: `[app.url]` (the app's own OAuth server).
    #[serde(deserialize_with = "list_or_csv")]
    pub authorization_servers: Vec<String>,
    /// The scopes clients may request, listed as `scopes_supported`
    /// (none by default).
    #[serde(deserialize_with = "list_or_csv")]
    pub scopes: Vec<String>,
}

impl McpOptions {
    /// Checks every key; each problem names its key.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if !self.path.starts_with('/') || self.path.len() < 2 {
            errors.push("mcp.path must be an absolute path such as `/mcp`".to_owned());
        }
        if self.path.contains(['{', '}', '*', '?', '#', ' ']) || self.path.ends_with('/') {
            errors.push(
                "mcp.path must be a plain path without parameters, query or trailing slash"
                    .to_owned(),
            );
        }
        for (key, value) in [("name", &self.name), ("version", &self.version)] {
            if value.as_deref().is_some_and(|v| v.trim().is_empty()) {
                errors.push(format!("mcp.{key} must not be empty"));
            }
        }
        if self.session_ttl_secs == 0 {
            errors.push("mcp.session_ttl_secs must be at least 1".to_owned());
        }
        if self.max_request_bytes == 0 {
            errors.push("mcp.max_request_bytes must be at least 1".to_owned());
        }
        for origin in &self.allowed_origins {
            if origin != "null" && !is_origin(origin) {
                errors.push(format!(
                    "mcp.allowed_origins: `{origin}` is not an origin such as `https://crm.example.com`"
                ));
            }
        }
        if let Some(resource) = &self.oauth.resource
            && !is_http_url(resource)
        {
            errors.push("mcp.oauth.resource must be an absolute http(s) URL".to_owned());
        }
        for server in &self.oauth.authorization_servers {
            if !is_http_url(server) {
                errors.push(format!(
                    "mcp.oauth.authorization_servers: `{server}` is not an absolute http(s) URL"
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl ConfigSection for McpOptions {
    const KEY: &'static str = "mcp";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// The options with every default resolved (from `app.name`,
/// `app.version`, `app.url` and the executable's name): what the module
/// runs with. Provided and exported by `McpModule`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpSettings {
    /// The endpoint path.
    pub path: String,
    /// The server name.
    pub name: String,
    /// The display title.
    pub title: String,
    /// The server version.
    pub version: String,
    /// The instructions for the model.
    pub instructions: Option<String>,
    /// How clients authenticate.
    pub auth: McpAuth,
    /// Browser origins allowed to call the endpoint.
    pub allowed_origins: Vec<String>,
    /// Idle session lifetime, in seconds.
    pub session_ttl_secs: u64,
    /// The largest JSON-RPC message, in bytes.
    pub max_request_bytes: usize,
    /// The protected resource metadata (with `auth = "oauth"`).
    pub oauth: Option<OAuthSettings>,
}

/// The resolved `mcp.oauth` settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthSettings {
    /// The resource identifier (the endpoint's absolute URL).
    pub resource: String,
    /// The authorization servers' issuers.
    pub authorization_servers: Vec<String>,
    /// The scopes listed as supported.
    pub scopes: Vec<String>,
}

impl OAuthSettings {
    /// The URL of the metadata document for `resource`, per RFC 9728:
    /// `/.well-known/oauth-protected-resource` inserted between the host and
    /// the resource's path (`https://crm.example.com/.well-known/oauth-protected-resource/mcp`).
    pub fn metadata_url(&self) -> String {
        let (origin, path) = split_origin(&self.resource);
        format!("{origin}/.well-known/oauth-protected-resource{path}")
    }

    /// The metadata document (RFC 9728).
    pub fn metadata(&self, title: &str) -> serde_json::Value {
        let mut document = serde_json::json!({
            "resource": self.resource,
            "authorization_servers": self.authorization_servers,
            "bearer_methods_supported": ["header"],
            "resource_name": title,
        });
        if !self.scopes.is_empty() {
            document["scopes_supported"] = serde_json::json!(self.scopes);
        }
        document
    }
}

impl McpSettings {
    /// Resolves `options` against the app's configuration (`app.name`,
    /// `app.version`, `app.url`); `config` is `None` without `ConfigModule`.
    pub fn resolve(options: &McpOptions, config: Option<&Config>) -> Result<Self, String> {
        options.check().map_err(|errors| errors.join("; "))?;
        let setting = |key: &str| {
            config
                .and_then(|config| config.get_string(key))
                .filter(|value| !value.trim().is_empty())
        };
        let name = options
            .name
            .clone()
            .or_else(|| setting("app.name"))
            .unwrap_or_else(executable_name);
        let title = options.title.clone().unwrap_or_else(|| name.clone());
        let version = options
            .version
            .clone()
            .or_else(|| setting("app.version"))
            .unwrap_or_else(|| "0.0.0".to_owned());
        let oauth = match options.auth {
            McpAuth::Bearer => None,
            McpAuth::Oauth => {
                let app_url = setting("app.url").map(|url| url.trim_end_matches('/').to_owned());
                let resource = options
                    .oauth
                    .resource
                    .clone()
                    .or_else(|| app_url.as_ref().map(|url| format!("{url}{}", options.path)))
                    .ok_or(
                        "mcp.auth = \"oauth\" needs mcp.oauth.resource or the app.url setting",
                    )?;
                let authorization_servers = if options.oauth.authorization_servers.is_empty() {
                    vec![app_url.ok_or(
                        "mcp.auth = \"oauth\" needs mcp.oauth.authorization_servers or the app.url setting",
                    )?]
                } else {
                    options.oauth.authorization_servers.clone()
                };
                Some(OAuthSettings {
                    resource,
                    authorization_servers,
                    scopes: options.oauth.scopes.clone(),
                })
            }
        };
        Ok(Self {
            path: options.path.clone(),
            name,
            title,
            version,
            instructions: options.instructions.clone(),
            auth: options.auth,
            allowed_origins: options.allowed_origins.clone(),
            session_ttl_secs: options.session_ttl_secs,
            max_request_bytes: options.max_request_bytes,
            oauth,
        })
    }

    /// Whether a browser request from `origin` may call the endpoint.
    pub fn origin_allowed(&self, origin: &str) -> bool {
        let origin = origin.trim_end_matches('/');
        self.allowed_origins
            .iter()
            .any(|allowed| allowed.trim_end_matches('/').eq_ignore_ascii_case(origin))
    }
}

/// The running executable's file name without its extension.
fn executable_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "navree-app".to_owned())
}

/// `https://host[:port]` without a path.
fn is_origin(value: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| value.strip_prefix(scheme))
        .is_some_and(|rest| {
            let rest = rest.trim_end_matches('/');
            !rest.is_empty() && !rest.contains(['/', '?', '#', ' '])
        })
}

/// An absolute `http://` or `https://` URL with a host.
fn is_http_url(value: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| value.strip_prefix(scheme))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}

/// (`https://host:port`, `/path`) of an absolute URL.
fn split_origin(url: &str) -> (&str, &str) {
    let after_scheme = url.find("://").map_or(0, |i| i + 3);
    match url[after_scheme..].find('/') {
        Some(slash) => {
            let (origin, path) = url.split_at(after_scheme + slash);
            (origin, path.trim_end_matches('/'))
        }
        None => (url, ""),
    }
}

/// A list, or a comma-separated string (from an environment variable).
fn list_or_csv<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ListOrCsv;

    impl<'de> Visitor<'de> for ListOrCsv {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list or a comma-separated string")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Vec<String>, E> {
            Ok(value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect())
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                items.push(item);
            }
            Ok(items)
        }
    }

    deserializer.deserialize_any(ListOrCsv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_config::ConfigSources;
    use serde_json::json;

    fn config(values: serde_json::Value) -> Config {
        let mut sources = ConfigSources::new();
        if let serde_json::Value::Object(map) = values {
            for (key, value) in map {
                sources = sources.set(&key, value);
            }
        }
        Config::load(&sources).expect("config")
    }

    #[test]
    fn defaults_match_the_documentation() {
        let options = McpOptions::default();
        assert_eq!(options.path, "/mcp");
        assert_eq!(options.auth, McpAuth::Bearer);
        assert!(options.allowed_origins.is_empty());
        assert_eq!(options.session_ttl_secs, 3600);
        assert_eq!(options.max_request_bytes, 1_048_576);
        assert!(options.check().is_ok());
    }

    #[test]
    fn the_section_reads_lists_from_strings_and_arrays() {
        let options: McpOptions = config(json!({
            "mcp": {
                "auth": "oauth",
                "allowed_origins": "https://a.example, https://b.example",
                "oauth": { "scopes": ["deals.read"] },
            }
        }))
        .typed()
        .expect("mcp section");
        assert_eq!(options.auth, McpAuth::Oauth);
        assert_eq!(
            options.allowed_origins,
            ["https://a.example", "https://b.example"]
        );
        assert_eq!(options.oauth.scopes, ["deals.read"]);
    }

    #[test]
    fn invalid_values_name_their_keys() {
        let options = McpOptions {
            path: "mcp/".to_owned(),
            session_ttl_secs: 0,
            max_request_bytes: 0,
            allowed_origins: vec!["crm.example.com".to_owned()],
            oauth: McpOAuthOptions {
                resource: Some("/mcp".to_owned()),
                ..McpOAuthOptions::default()
            },
            ..McpOptions::default()
        };
        let errors = options.check().unwrap_err().join("\n");
        for key in [
            "mcp.path",
            "mcp.session_ttl_secs",
            "mcp.max_request_bytes",
            "mcp.allowed_origins",
            "mcp.oauth.resource",
        ] {
            assert!(errors.contains(key), "{key} in {errors}");
        }
    }

    #[test]
    fn settings_fall_back_to_the_app_settings() {
        let app = config(json!({ "app": { "name": "crm", "version": "2.1.0" } }));
        let settings = McpSettings::resolve(&McpOptions::default(), Some(&app)).unwrap();
        assert_eq!(
            (settings.name.as_str(), settings.title.as_str(), settings.version.as_str()),
            ("crm", "crm", "2.1.0")
        );
        assert!(settings.oauth.is_none());

        let settings = McpSettings::resolve(&McpOptions::default(), None).unwrap();
        assert!(!settings.name.is_empty(), "the executable's name");
        assert_eq!(settings.version, "0.0.0");

        let named = McpOptions {
            name: Some("navree-crm".to_owned()),
            title: Some("Navree CRM".to_owned()),
            ..McpOptions::default()
        };
        let settings = McpSettings::resolve(&named, Some(&app)).unwrap();
        assert_eq!((settings.name.as_str(), settings.title.as_str()), ("navree-crm", "Navree CRM"));
    }

    #[test]
    fn oauth_settings_default_to_the_app_url() {
        let oauth = McpOptions {
            auth: McpAuth::Oauth,
            ..McpOptions::default()
        };
        let error = McpSettings::resolve(&oauth, None).unwrap_err();
        assert!(error.contains("app.url"), "{error}");

        let app = config(json!({ "app": { "url": "https://crm.example.com/" } }));
        let settings = McpSettings::resolve(&oauth, Some(&app)).unwrap();
        let oauth = settings.oauth.expect("oauth settings");
        assert_eq!(oauth.resource, "https://crm.example.com/mcp");
        assert_eq!(oauth.authorization_servers, ["https://crm.example.com"]);
        assert_eq!(
            oauth.metadata_url(),
            "https://crm.example.com/.well-known/oauth-protected-resource/mcp"
        );
        assert_eq!(
            oauth.metadata("CRM"),
            json!({
                "resource": "https://crm.example.com/mcp",
                "authorization_servers": ["https://crm.example.com"],
                "bearer_methods_supported": ["header"],
                "resource_name": "CRM",
            })
        );
    }

    #[test]
    fn origins_match_exactly_and_case_insensitively() {
        let settings = McpSettings::resolve(
            &McpOptions {
                allowed_origins: vec!["https://crm.example.com".to_owned()],
                ..McpOptions::default()
            },
            None,
        )
        .unwrap();
        assert!(settings.origin_allowed("https://CRM.example.com"));
        assert!(!settings.origin_allowed("https://evil.example"));
        assert!(!settings.origin_allowed("http://crm.example.com"));
    }
}
