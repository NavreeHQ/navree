//! [`HttpOptions`]: the `http` configuration section.

use std::net::SocketAddr;
use std::path::PathBuf;

use http::HeaderValue;
use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::{Deserialize, Deserializer};

/// HTTP server settings (configuration section `http`; every key optional).
///
/// ```toml
/// [http]
/// addr = "0.0.0.0:8080"
/// api_prefix = "/api"
/// timeout_secs = 30
/// cors = { allowed_origins = ["http://localhost:3000"] }
/// errors = { code = true }
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct HttpOptions {
    /// The address `serve` binds (default `127.0.0.1:8080`).
    pub addr: String,
    /// The prefix of every controller route (default `/api`); routes whose
    /// path starts with `!` ignore it.
    pub api_prefix: String,
    /// Cross-origin settings.
    pub cors: CorsOptions,
    /// The request timeout (default 30 s); routes may override it.
    pub timeout_secs: u64,
    /// The default request body limit (default 2 MiB); routes may override it.
    pub body_limit_bytes: usize,
    /// Requests per minute per client IP (default `Some(600)`; `None` disables).
    pub per_ip_rate_limit: Option<u32>,
    /// Send the default security headers (default `true`).
    pub security_headers: bool,
    /// Compress responses with gzip or brotli (default `true`).
    pub compression: bool,
    /// While this file exists the app answers 503 (default `storage/framework/down`).
    pub maintenance_file: PathBuf,
    /// Request paths still served during maintenance, matched exactly
    /// (default `/health/live`, `/health/ready` and `/metrics`, so probes and
    /// scrapes keep working and an orchestrator does not restart instances
    /// that are only down for maintenance). From the environment, a
    /// comma-separated list (`APP_HTTP__MAINTENANCE_EXCEPT=/health/live,/up`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub maintenance_except: Vec<String>,
    /// Take the client IP from `X-Forwarded-For` / `X-Real-IP` sent by any
    /// peer (default `false`; enable only when every request comes through
    /// a reverse proxy that sets them). Prefer `trusted_proxies`.
    pub trust_proxy: bool,
    /// The reverse proxies (and backends-for-frontends) whose
    /// `X-Forwarded-For` / `X-Real-IP` name the client: IP addresses or
    /// CIDR blocks (`127.0.0.1`, `::1`, `10.0.0.0/8`). A request from any
    /// other peer is identified by its own address, so clients reaching the
    /// app directly cannot choose theirs. In `X-Forwarded-For`, the client
    /// is the first address from the right that is not a listed proxy.
    /// When set, it replaces `trust_proxy`. From the environment, a
    /// comma-separated list (`APP_HTTP__TRUSTED_PROXIES=127.0.0.1,::1`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub trusted_proxies: Vec<String>,
    /// The key for signed URLs. When unset, a key derived from the app key
    /// (`app.key`, or `APP_KEY`) is used; with no app key either, a random
    /// per-process key, so signed URLs do not survive a restart.
    pub signing_key: Option<Secret<String>>,
    /// How long `serve` waits for in-flight requests after a shutdown signal
    /// (default 30 s).
    pub shutdown_grace_secs: u64,
    /// How error bodies are rendered (`http.errors`).
    pub errors: ErrorOptions,
}

/// How error bodies are rendered (configuration section `http.errors`).
///
/// ```toml
/// [http.errors]
/// code = true
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ErrorOptions {
    /// Send the machine-readable `code` member in every error body (default
    /// `true`). `false` leaves it out (`APP_HTTP__ERRORS__CODE=false`).
    pub code: bool,
}

impl Default for ErrorOptions {
    fn default() -> Self {
        Self { code: true }
    }
}

impl Default for HttpOptions {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:8080".to_owned(),
            api_prefix: "/api".to_owned(),
            cors: CorsOptions::default(),
            timeout_secs: 30,
            body_limit_bytes: 2 * 1024 * 1024,
            per_ip_rate_limit: Some(600),
            security_headers: true,
            compression: true,
            maintenance_file: PathBuf::from("storage/framework/down"),
            maintenance_except: ["/health/live", "/health/ready", "/metrics"]
                .map(str::to_owned)
                .to_vec(),
            trust_proxy: false,
            trusted_proxies: Vec::new(),
            signing_key: None,
            shutdown_grace_secs: 30,
            errors: ErrorOptions::default(),
        }
    }
}

/// Cross-origin resource sharing: only the listed origins, with credentials,
/// and explicitly listed request and exposed headers (no wildcards). Applied
/// by [`CorsLayer`](crate::CorsLayer); an origin that is not listed gets no
/// CORS headers.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CorsOptions {
    /// Exact origins allowed to call the API, e.g. `http://localhost:3000`.
    /// Empty (the default) disables CORS. `*` is rejected. From the
    /// environment, a comma-separated list
    /// (`APP_HTTP__CORS__ALLOWED_ORIGINS=http://localhost:3000,https://app.example.com`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub allowed_origins: Vec<String>,
    /// Allow cookies on cross-origin requests (default `true`).
    pub allow_credentials: bool,
    /// How long browsers may cache a preflight answer (default 600 s).
    pub max_age_secs: u64,
}

impl Default for CorsOptions {
    fn default() -> Self {
        Self {
            allowed_origins: Vec::new(),
            allow_credentials: true,
            max_age_secs: 600,
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
    enum Origins {
        List(Vec<String>),
        Joined(String),
    }
    let origins = match Origins::deserialize(deserializer)? {
        Origins::List(origins) => origins,
        Origins::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(origins
        .into_iter()
        .map(|origin| origin.trim().to_owned())
        .filter(|origin| !origin.is_empty())
        .collect())
}

impl ConfigSection for HttpOptions {
    const KEY: &'static str = "http";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

impl HttpOptions {
    /// Every problem with these options (empty when valid).
    pub(crate) fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.addr.parse::<SocketAddr>().is_err() && !self.addr.contains(':') {
            errors.push(format!(
                "http.addr must be HOST:PORT, e.g. 127.0.0.1:8080 (got `{}`)",
                self.addr
            ));
        }
        if !self.api_prefix.is_empty() && !self.api_prefix.starts_with('/') {
            errors.push("http.api_prefix must be empty or start with `/`".to_owned());
        }
        if self.timeout_secs == 0 {
            errors.push("http.timeout_secs must be at least 1".to_owned());
        }
        if self.per_ip_rate_limit == Some(0) {
            errors.push("http.per_ip_rate_limit must be at least 1 (or unset)".to_owned());
        }
        for proxy in &self.trusted_proxies {
            if crate::extract::IpNetwork::parse(proxy).is_none() {
                errors.push(format!(
                    "http.trusted_proxies: `{proxy}` is not an IP address or CIDR block like 10.0.0.0/8"
                ));
            }
        }
        for origin in &self.cors.allowed_origins {
            if origin.trim() == "*" {
                errors.push(
                    "http.cors.allowed_origins must list exact origins; `*` is not allowed"
                        .to_owned(),
                );
            } else if HeaderValue::from_str(origin).is_err()
                || !(origin.starts_with("http://") || origin.starts_with("https://"))
            {
                errors.push(format!(
                    "http.cors.allowed_origins: `{origin}` is not an origin like https://app.example.com"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_plan() {
        let options = HttpOptions::default();
        assert_eq!(options.addr, "127.0.0.1:8080");
        assert_eq!(options.api_prefix, "/api");
        assert_eq!(options.timeout_secs, 30);
        assert_eq!(options.body_limit_bytes, 2 * 1024 * 1024);
        assert_eq!(options.per_ip_rate_limit, Some(600));
        assert!(options.security_headers && options.compression && !options.trust_proxy);
        assert!(options.cors.allow_credentials);
        assert!(options.check().is_ok());
    }

    #[test]
    fn error_codes_are_rendered_unless_the_environment_turns_them_off() {
        assert!(HttpOptions::default().errors.code, "on by default");
        let config = navree_config::Config::load(
            &navree_config::ConfigSources::new()
                .env_vars("APP_", [("APP_HTTP__ERRORS__CODE", "false")]),
        )
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert!(!options.errors.code);
        assert_eq!(options.api_prefix, "/api", "other keys keep defaults");
    }

    #[test]
    fn missing_keys_deserialize_to_defaults() {
        let options: HttpOptions = serde_json::from_value(
            serde_json::json!({"cors": {"allowed_origins": ["https://a.example"]}}),
        )
        .unwrap();
        assert_eq!(options.api_prefix, "/api");
        assert!(options.cors.allow_credentials);
        assert_eq!(options.cors.allowed_origins, ["https://a.example"]);
        assert!(options.errors.code);
    }

    #[test]
    fn trusted_proxies_come_from_a_list_or_the_environment_and_are_checked() {
        let config = navree_config::Config::load(&navree_config::ConfigSources::new().env_vars(
            "APP_",
            [("APP_HTTP__TRUSTED_PROXIES", "127.0.0.1, ::1,10.0.0.0/8")],
        ))
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert_eq!(options.trusted_proxies, ["127.0.0.1", "::1", "10.0.0.0/8"]);
        assert!(HttpOptions::default().trusted_proxies.is_empty());

        let options = HttpOptions {
            trusted_proxies: vec![
                "127.0.0.1".into(),
                "proxy.internal".into(),
                "10.0.0.0/40".into(),
            ],
            ..HttpOptions::default()
        };
        let errors = options.check().unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("proxy.internal"), "{errors:?}");
    }

    #[test]
    fn allowed_origins_can_come_from_one_environment_variable() {
        let config = navree_config::Config::load(&navree_config::ConfigSources::new().env_vars(
            "APP_",
            [(
                "APP_HTTP__CORS__ALLOWED_ORIGINS",
                "http://localhost:3000, https://app.example.com,",
            )],
        ))
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert_eq!(
            options.cors.allowed_origins,
            ["http://localhost:3000", "https://app.example.com"]
        );
        assert!(options.cors.allow_credentials, "other keys keep defaults");
    }

    #[test]
    fn wildcard_and_malformed_origins_are_rejected() {
        let mut options = HttpOptions::default();
        options.cors.allowed_origins = vec!["*".into(), "localhost:3000".into()];
        let errors = options.check().unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains('*'));
    }
}
