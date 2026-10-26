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
///
/// [auth.password_reset]
/// ttl_secs = 3600                 # reset tokens expire after an hour
/// throttle_secs = 60              # at most one reset mail per address per minute
/// # url = "https://app.example.com/reset-password"  # mail link: {url}?token=..&email=..
/// # mail_template = "mail/reset-password"           # navree-view template pair
///
/// [auth.verification]
/// ttl_secs = 3600                 # verification links expire after an hour
/// # base_url = "https://app.example.com"            # default: app.url
/// # mail_template = "mail/verify-email"
///
/// [auth.two_factor]
/// issuer = "Navree"                 # shown in authenticator apps
/// challenge_ttl_secs = 300        # the login challenge token lives 5 minutes
/// recovery_codes = 8
/// max_attempts_per_minute = 5     # wrong codes per user, then 429; 0 = off
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
    /// Password reset (`POST /auth/forgot-password`, `POST
    /// /auth/reset-password`).
    pub password_reset: PasswordResetOptions,
    /// Email verification (`POST /auth/email/verification`, `GET
    /// /auth/email/verify/{id}/{hash}`).
    pub verification: VerificationOptions,
    /// TOTP two-factor authentication (`/auth/2fa/*`).
    pub two_factor: TwoFactorOptions,
}

impl Default for AuthOptions {
    fn default() -> Self {
        Self {
            jwt: JwtOptions::default(),
            registration: true,
            login_throttle_per_minute: 5,
            password_min_length: 8,
            routes_prefix: "/auth".to_owned(),
            password_reset: PasswordResetOptions::default(),
            verification: VerificationOptions::default(),
            two_factor: TwoFactorOptions::default(),
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
        let reset = &self.password_reset;
        if reset.ttl_secs == 0 {
            errors.push("auth.password_reset.ttl_secs must be at least 1".to_owned());
        }
        if reset.url.as_deref().is_some_and(|url| !is_http_url(url)) {
            errors.push("auth.password_reset.url must be an absolute http(s) URL".to_owned());
        }
        let verification = &self.verification;
        if verification.ttl_secs == 0 {
            errors.push("auth.verification.ttl_secs must be at least 1".to_owned());
        }
        if verification
            .base_url
            .as_deref()
            .is_some_and(|url| !is_http_url(url))
        {
            errors.push("auth.verification.base_url must be an absolute http(s) URL".to_owned());
        }
        let two_factor = &self.two_factor;
        if two_factor.issuer.trim().is_empty() || two_factor.issuer.contains(':') {
            errors.push("auth.two_factor.issuer must not be empty or contain `:`".to_owned());
        }
        if two_factor.challenge_ttl_secs == 0 {
            errors.push("auth.two_factor.challenge_ttl_secs must be at least 1".to_owned());
        }
        if !(1..=MAX_RECOVERY_CODES).contains(&two_factor.recovery_codes) {
            errors.push(format!(
                "auth.two_factor.recovery_codes must be between 1 and {MAX_RECOVERY_CODES}"
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Whether `url` is an absolute `http://` or `https://` URL with a host.
fn is_http_url(url: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
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

/// The most recovery codes `auth.two_factor.recovery_codes` accepts.
pub const MAX_RECOVERY_CODES: usize = 32;

/// Password reset options (configuration section `auth.password_reset`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PasswordResetOptions {
    /// How long a reset token is valid, in seconds.
    pub ttl_secs: u64,
    /// The shortest time between two reset mails to the same address, in
    /// seconds (requests in between are accepted but send nothing); `0`
    /// sends a mail for every request.
    pub throttle_secs: u64,
    /// The front-end page the mail links to, as `{url}?token=..&email=..`;
    /// the page posts the new password to `POST /auth/reset-password`.
    /// Without it the mail carries the token alone (API and mobile clients).
    pub url: Option<String>,
    /// A navree-view template pair (`{template}.html` / `.txt`) for the mail,
    /// rendered with `{name, email, token, url, expires_in_minutes}` (`url`
    /// is empty without [`url`](Self::url)); built-in text when unset.
    pub mail_template: Option<String>,
}

impl Default for PasswordResetOptions {
    fn default() -> Self {
        Self {
            ttl_secs: 3600,
            throttle_secs: 60,
            url: None,
            mail_template: None,
        }
    }
}

/// Email verification options (configuration section `auth.verification`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct VerificationOptions {
    /// How long a verification link is valid, in seconds.
    pub ttl_secs: u64,
    /// The absolute URL the signed path (`/api/auth/email/verify/..`) is
    /// appended to in the mail: the API's public URL, or the front end when
    /// it proxies `/api` to the API. Default: the `app.url` setting; without
    /// either the mail carries a relative link.
    pub base_url: Option<String>,
    /// A navree-view template pair for the mail, rendered with
    /// `{name, email, url, expires_in_minutes}`; built-in text when unset.
    pub mail_template: Option<String>,
}

impl Default for VerificationOptions {
    fn default() -> Self {
        Self {
            ttl_secs: 3600,
            base_url: None,
            mail_template: None,
        }
    }
}

/// TOTP two-factor options (configuration section `auth.two_factor`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TwoFactorOptions {
    /// The issuer shown in authenticator apps (`otpauth://totp/{issuer}:{email}`).
    pub issuer: String,
    /// Lifetime of the challenge token a two-factor login returns, in seconds.
    pub challenge_ttl_secs: u64,
    /// How many single-use recovery codes are issued (1 to 32).
    pub recovery_codes: usize,
    /// Wrong codes accepted per user per minute at `POST /auth/2fa/verify`
    /// before it answers 429; `0` turns the limit off.
    pub max_attempts_per_minute: u32,
}

impl Default for TwoFactorOptions {
    fn default() -> Self {
        Self {
            issuer: "Navree".to_owned(),
            challenge_ttl_secs: 300,
            recovery_codes: 8,
            max_attempts_per_minute: 5,
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
    fn defaults_are_the_documented_ones() {
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
    fn account_flow_defaults() {
        let options = load(ConfigSources::new()).unwrap();
        let reset = &options.password_reset;
        assert_eq!(reset.ttl_secs, 3600);
        assert_eq!(reset.throttle_secs, 60);
        assert_eq!(reset.url, None);
        assert_eq!(reset.mail_template, None);
        let verification = &options.verification;
        assert_eq!(verification.ttl_secs, 3600);
        assert_eq!(verification.base_url, None);
        assert_eq!(verification.mail_template, None);
        let two_factor = &options.two_factor;
        assert_eq!(two_factor.issuer, "Navree");
        assert_eq!(two_factor.challenge_ttl_secs, 300);
        assert_eq!(two_factor.recovery_codes, 8);
        assert_eq!(two_factor.max_attempts_per_minute, 5);
    }

    #[test]
    fn invalid_account_flow_values_fail_validation() {
        for (key, value) in [
            ("auth.password_reset.ttl_secs", json!(0)),
            ("auth.password_reset.url", json!("reset-password")),
            ("auth.verification.ttl_secs", json!(0)),
            ("auth.verification.base_url", json!("api.example.com")),
            ("auth.two_factor.issuer", json!("")),
            ("auth.two_factor.issuer", json!("Acme:CRM")),
            ("auth.two_factor.challenge_ttl_secs", json!(0)),
            ("auth.two_factor.recovery_codes", json!(0)),
            ("auth.two_factor.recovery_codes", json!(33)),
        ] {
            let result = load(ConfigSources::new().set(key, value.clone()));
            assert!(result.is_err(), "{key} = {value} should be rejected");
        }
        let options = load(
            ConfigSources::new()
                .set("auth.password_reset.url", "https://app.example.com/reset")
                .set("auth.verification.base_url", "http://localhost:3000")
                .set("auth.two_factor.issuer", "Acme CRM"),
        )
        .unwrap();
        assert_eq!(
            options.password_reset.url.as_deref(),
            Some("https://app.example.com/reset")
        );
        assert_eq!(options.two_factor.issuer, "Acme CRM");
    }

    #[test]
    fn secret_is_redacted_in_debug_output() {
        let secret = "s".repeat(40);
        let options = load(ConfigSources::new().set("auth.jwt.secret", secret.clone())).unwrap();
        assert_eq!(options.jwt.secret.as_ref().unwrap().expose(), &secret);
        assert!(!format!("{options:?}").contains(&secret));
    }
}
