//! [`AuthOptions`] and [`JwtOptions`]: the `auth` configuration section.

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::Deserialize;

/// The shortest accepted `auth.jwt.secret`, in bytes: HS256 keys should be
/// at least as long as the hash output (256 bits).
pub const MIN_SECRET_LEN: usize = 32;

/// Options of the auth module (configuration section `auth`).
///
/// ```toml
/// [auth]
/// registration = true             # POST /auth/register (404 when false)
/// login_throttle_per_minute = 5   # failed logins per email + IP; 0 = off
/// password_min_length = 8
/// routes_prefix = "/auth"         # under http.api_prefix: /api/auth/login
///
/// [auth.jwt]
/// # secret = "..."                # >= 32 bytes; default: derived from app.key
/// issuer = "navree"
/// access_ttl_secs = 900           # 15 minutes
/// refresh_ttl_secs = 2592000      # 30 days
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct AuthOptions {
    /// JWT access tokens and refresh tokens.
    pub jwt: JwtOptions,
    /// Whether `POST /auth/register` is mounted (public sign-up).
    pub registration: bool,
    /// Failed login attempts allowed per email and client IP per minute
    /// before `POST /auth/login` answers 429; `0` turns the limit off.
    pub login_throttle_per_minute: u32,
    /// The shortest accepted password, in characters.
    pub password_min_length: usize,
    /// The path of the auth routes under the API prefix (`/auth` mounts
    /// `/api/auth/login`).
    pub routes_prefix: String,
}

impl Default for AuthOptions {
    fn default() -> Self {
        Self {
            jwt: JwtOptions::default(),
            registration: true,
            login_throttle_per_minute: 5,
            password_min_length: 8,
            routes_prefix: "/auth".to_owned(),
        }
    }
}

impl AuthOptions {
    /// Every problem with the options, naming the configuration keys (never
    /// a secret value).
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.password_min_length == 0 {
            errors.push("auth.password_min_length must be at least 1".to_owned());
        }
        if !self.routes_prefix.is_empty() && !self.routes_prefix.starts_with('/') {
            errors.push("auth.routes_prefix must be empty or start with `/`".to_owned());
        }
        let jwt = &self.jwt;
        if let Some(secret) = &jwt.secret
            && secret.expose().len() < MIN_SECRET_LEN
        {
            errors.push(format!(
                "auth.jwt.secret must be at least {MIN_SECRET_LEN} bytes long"
            ));
        }
        if jwt.issuer.trim().is_empty() {
            errors.push("auth.jwt.issuer must not be empty".to_owned());
        }
        if jwt.audience.as_deref().is_some_and(|a| a.trim().is_empty()) {
            errors.push("auth.jwt.audience must not be empty when set".to_owned());
        }
        if jwt.access_ttl_secs == 0 {
            errors.push("auth.jwt.access_ttl_secs must be at least 1".to_owned());
        }
        if jwt.refresh_ttl_secs == 0 {
            errors.push("auth.jwt.refresh_ttl_secs must be at least 1".to_owned());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl ConfigSection for AuthOptions {
    const KEY: &'static str = "auth";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// JWT options (configuration section `auth.jwt`).
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct JwtOptions {
    /// The HS256 signing secret (at least [`MIN_SECRET_LEN`] bytes). When
    /// unset, a key derived from the application key (`app.key` /
    /// `APP_KEY`) is used; without either, a random per-process key (tokens
    /// stop working on restart, with a warning).
    pub secret: Option<Secret<String>>,
    /// The `iss` claim written and required.
    pub issuer: String,
    /// The `aud` claim written and required, if any.
    pub audience: Option<String>,
    /// Lifetime of access tokens, in seconds.
    pub access_ttl_secs: u64,
    /// Lifetime of refresh tokens, in seconds (each refresh issues a new one).
    pub refresh_ttl_secs: u64,
}

impl Default for JwtOptions {
    fn default() -> Self {
        Self {
            secret: None,
            issuer: "navree".to_owned(),
            audience: None,
            access_ttl_secs: 900,
            refresh_ttl_secs: 2_592_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};
    use serde_json::json;

    use super::*;

    fn load(sources: ConfigSources) -> Result<AuthOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<AuthOptions>()
    }

    #[test]
    fn defaults_follow_the_nestjs_recipe() {
        let options = load(ConfigSources::new()).unwrap();
        assert!(options.registration);
        assert_eq!(options.login_throttle_per_minute, 5);
        assert_eq!(options.password_min_length, 8);
        assert_eq!(options.routes_prefix, "/auth");
        assert!(options.jwt.secret.is_none());
        assert_eq!(options.jwt.issuer, "navree");
        assert_eq!(options.jwt.audience, None);
        assert_eq!(options.jwt.access_ttl_secs, 900);
        assert_eq!(options.jwt.refresh_ttl_secs, 2_592_000);
    }

    #[test]
    fn section_is_read_from_config_and_environment() {
        let options = load(
            ConfigSources::new()
                .set("auth.jwt.issuer", "crm")
                .set("auth.routes_prefix", "/identity")
                .env_vars(
                    "APP_",
                    [
                        ("APP_AUTH__REGISTRATION", "false"),
                        ("APP_AUTH__JWT__ACCESS_TTL_SECS", "60"),
                    ],
                ),
        )
        .unwrap();
        assert!(!options.registration);
        assert_eq!(options.jwt.access_ttl_secs, 60);
        assert_eq!(options.jwt.issuer, "crm");
        assert_eq!(options.routes_prefix, "/identity");
    }

    #[test]
    fn short_secret_fails_validation_without_echoing_it() {
        let error = load(ConfigSources::new().set("auth.jwt.secret", json!("too-short-secret")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("auth.jwt.secret"), "{error}");
        assert!(!error.contains("too-short-secret"), "{error}");
    }

    #[test]
    fn invalid_values_fail_validation() {
        for (key, value) in [
            ("auth.jwt.access_ttl_secs", json!(0)),
            ("auth.jwt.refresh_ttl_secs", json!(0)),
            ("auth.password_min_length", json!(0)),
            ("auth.routes_prefix", json!("auth")),
            ("auth.jwt.issuer", json!("")),
        ] {
            let result = load(ConfigSources::new().set(key, value));
            assert!(result.is_err(), "{key} should be rejected");
        }
    }

    #[test]
    fn secret_is_redacted_in_debug_output() {
        let secret = "s".repeat(40);
        let options = load(ConfigSources::new().set("auth.jwt.secret", secret.clone())).unwrap();
        assert_eq!(options.jwt.secret.as_ref().unwrap().expose(), &secret);
        assert!(!format!("{options:?}").contains(&secret));
    }
}
