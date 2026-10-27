//! [`AuthOptions`] and [`JwtOptions`]: the `auth` configuration section.

use std::collections::BTreeMap;

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
/// required_for = []               # roles that must use two-factor authentication
///
/// [auth.impersonation]
/// enabled = false                 # POST/DELETE /auth/impersonate
/// ttl_secs = 3600                 # impersonation sessions end after an hour
///
/// [auth.magic_link]
/// enabled = false                 # POST /auth/magic-link, /auth/magic-link/verify
/// ttl_secs = 900
/// throttle_secs = 60
/// # url = "https://app.example.com/magic-link"      # mail link: {url}?token=..
///
/// [auth.otp]
/// enabled = false                 # POST /auth/otp, /auth/otp/verify
/// length = 6
/// ttl_secs = 600
/// max_attempts = 5                # wrong guesses per code, then it is burned
/// throttle_secs = 60
///
/// [auth.platform]
/// enabled = false                 # POST /auth/platform/login, /auth/platform/2fa/verify
/// roles = ["platform-admin"]
/// login_throttle_per_minute = 3
/// access_ttl_secs = 900
/// session_ttl_secs = 28800        # platform sessions end after 8 hours
///
/// [auth.trusted_issuers.legacy]   # POST /auth/exchange
/// issuer = "https://legacy.example.com"
/// jwks_url = "https://legacy.example.com/.well-known/jwks.json"  # or public_key = "-----BEGIN PUBLIC KEY-----..."
/// audience = "navree"
/// enabled = true
///
/// [auth.passkeys]
/// enabled = false                 # /auth/passkeys/*
/// # rp_id = "example.com"
/// rp_name = "Navree"
/// # origins = ["https://app.example.com"]
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
    /// Impersonation (`POST /auth/impersonate/{user}`).
    pub impersonation: ImpersonationOptions,
    /// Magic links (`POST /auth/magic-link`).
    pub magic_link: MagicLinkOptions,
    /// Emailed sign-in codes (`POST /auth/otp`).
    pub otp: OtpOptions,
    /// The platform audience for platform administrators
    /// (`POST /auth/platform/login`).
    pub platform: PlatformOptions,
    /// Sibling systems whose signed tokens `POST /auth/exchange` accepts,
    /// by name.
    pub trusted_issuers: BTreeMap<String, TrustedIssuerOptions>,
    /// Passkeys (WebAuthn, `/auth/passkeys/*`).
    pub passkeys: PasskeyOptions,
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
            impersonation: ImpersonationOptions::default(),
            magic_link: MagicLinkOptions::default(),
            otp: OtpOptions::default(),
            platform: PlatformOptions::default(),
            trusted_issuers: BTreeMap::new(),
            passkeys: PasskeyOptions::default(),
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
        if two_factor
            .required_for
            .iter()
            .any(|role| role.trim().is_empty())
        {
            errors.push("auth.two_factor.required_for must not contain empty roles".to_owned());
        }
        if self.impersonation.ttl_secs == 0 {
            errors.push("auth.impersonation.ttl_secs must be at least 1".to_owned());
        }
        let magic_link = &self.magic_link;
        if magic_link.ttl_secs == 0 {
            errors.push("auth.magic_link.ttl_secs must be at least 1".to_owned());
        }
        if magic_link
            .url
            .as_deref()
            .is_some_and(|url| !is_http_url(url))
        {
            errors.push("auth.magic_link.url must be an absolute http(s) URL".to_owned());
        }
        let otp = &self.otp;
        if !(4..=10).contains(&otp.length) {
            errors.push("auth.otp.length must be between 4 and 10".to_owned());
        }
        if otp.ttl_secs == 0 {
            errors.push("auth.otp.ttl_secs must be at least 1".to_owned());
        }
        if otp.max_attempts == 0 {
            errors.push("auth.otp.max_attempts must be at least 1".to_owned());
        }
        let platform = &self.platform;
        if platform.enabled && platform.roles.iter().all(|role| role.trim().is_empty()) {
            errors.push("auth.platform.roles must name at least one role".to_owned());
        }
        if platform.access_ttl_secs == 0 {
            errors.push("auth.platform.access_ttl_secs must be at least 1".to_owned());
        }
        if platform.session_ttl_secs == 0 {
            errors.push("auth.platform.session_ttl_secs must be at least 1".to_owned());
        }
        for (name, issuer) in &self.trusted_issuers {
            issuer.check(name, &mut errors);
        }
        let passkeys = &self.passkeys;
        if passkeys.enabled {
            if passkeys
                .rp_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
            {
                errors.push("auth.passkeys.rp_id must be set when passkeys are enabled".to_owned());
            }
            if passkeys.origins.is_empty() {
                errors.push(
                    "auth.passkeys.origins must list at least one origin when passkeys are enabled"
                        .to_owned(),
                );
            }
        }
        if passkeys
            .origins
            .iter()
            .any(|origin| !is_http_url(origin) || origin.ends_with('/'))
        {
            errors.push(
                "auth.passkeys.origins must be http(s) origins without a trailing slash".to_owned(),
            );
        }
        if passkeys.timeout_secs == 0 {
            errors.push("auth.passkeys.timeout_secs must be at least 1".to_owned());
        }
        if !["required", "preferred", "discouraged"].contains(&passkeys.user_verification.as_str())
        {
            errors.push(
                "auth.passkeys.user_verification must be required, preferred or discouraged"
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
    /// Roles (navree-access) whose holders must use two-factor
    /// authentication: until they set it up, their sessions reach only the
    /// two-factor setup routes (403 `two_factor_required` elsewhere).
    /// Needs `AccessModule` when not empty. Workspaces and organizations
    /// add their own rules with a [`TwoFactorPolicy`](crate::TwoFactorPolicy).
    pub required_for: Vec<String>,
}

impl Default for TwoFactorOptions {
    fn default() -> Self {
        Self {
            issuer: "Navree".to_owned(),
            challenge_ttl_secs: 300,
            recovery_codes: 8,
            max_attempts_per_minute: 5,
            required_for: Vec::new(),
        }
    }
}

/// Impersonation options (configuration section `auth.impersonation`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ImpersonationOptions {
    /// Whether `POST /auth/impersonate/{user}` and `DELETE
    /// /auth/impersonate` are mounted. They require the permission
    /// [`permission`](Self::permission), so the app needs `AccessModule`.
    pub enabled: bool,
    /// How long an impersonation session lasts, in seconds (refreshing does
    /// not extend it).
    pub ttl_secs: u64,
    /// The permission `POST /auth/impersonate/{user}` requires.
    pub permission: String,
}

impl Default for ImpersonationOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_secs: 3600,
            permission: "users.impersonate".to_owned(),
        }
    }
}

/// Magic link options (configuration section `auth.magic_link`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MagicLinkOptions {
    /// Whether `POST /auth/magic-link` and `POST /auth/magic-link/verify`
    /// are mounted.
    pub enabled: bool,
    /// How long a link works, in seconds.
    pub ttl_secs: u64,
    /// The shortest time between two links to the same address, in
    /// seconds (requests in between are accepted but send nothing).
    pub throttle_secs: u64,
    /// The front-end page the mail links to, as `{url}?token=..`; the page
    /// posts the token to `POST /auth/magic-link/verify`. Without it the
    /// mail carries the token alone.
    pub url: Option<String>,
    /// A navree-view template pair for the mail, rendered with
    /// `{name, email, token, url, expires_in_minutes}`; built-in text when
    /// unset.
    pub mail_template: Option<String>,
}

impl Default for MagicLinkOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_secs: 900,
            throttle_secs: 60,
            url: None,
            mail_template: None,
        }
    }
}

/// Emailed sign-in code options (configuration section `auth.otp`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct OtpOptions {
    /// Whether `POST /auth/otp` and `POST /auth/otp/verify` are mounted.
    pub enabled: bool,
    /// Digits per code (4 to 10).
    pub length: usize,
    /// How long a code works, in seconds.
    pub ttl_secs: u64,
    /// Wrong guesses a code survives; then it is burned.
    pub max_attempts: u32,
    /// The shortest time between two codes to the same address, in seconds.
    pub throttle_secs: u64,
    /// A navree-view template pair for the mail, rendered with
    /// `{name, email, code, expires_in_minutes}`; built-in text when unset.
    pub mail_template: Option<String>,
}

impl Default for OtpOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            length: 6,
            ttl_secs: 600,
            max_attempts: 5,
            throttle_secs: 60,
            mail_template: None,
        }
    }
}

/// Platform audience options (configuration section `auth.platform`):
/// platform administrators sign in apart from the app, with two-factor
/// authentication always.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PlatformOptions {
    /// Whether `POST /auth/platform/login` and `POST
    /// /auth/platform/2fa/verify` are mounted.
    pub enabled: bool,
    /// The roles (navree-access) that make a user a platform administrator.
    pub roles: Vec<String>,
    /// Failed platform sign-ins allowed per email and IP per minute; `0`
    /// turns the limit off.
    pub login_throttle_per_minute: u32,
    /// Lifetime of platform access tokens, in seconds.
    pub access_ttl_secs: u64,
    /// How long a platform session lasts, in seconds (refreshing does not
    /// extend it).
    pub session_ttl_secs: u64,
}

impl Default for PlatformOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            roles: vec!["platform-admin".to_owned()],
            login_throttle_per_minute: 3,
            access_ttl_secs: 900,
            session_ttl_secs: 28_800,
        }
    }
}

/// A sibling system whose signed tokens `POST /auth/exchange` accepts
/// (configuration section `auth.trusted_issuers.<name>`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TrustedIssuerOptions {
    /// The `iss` claim its tokens carry.
    pub issuer: String,
    /// Where its JSON Web Key Set is published (fetched with the app's
    /// `HttpClient` and cached).
    pub jwks_url: Option<String>,
    /// Its public key instead: a PEM `PUBLIC KEY` (SubjectPublicKeyInfo:
    /// RSA, P-256 or Ed25519) or a JWK as JSON.
    pub public_key: Option<String>,
    /// The `aud` its tokens must carry.
    pub audience: String,
    /// The kill switch: `false` refuses its tokens.
    pub enabled: bool,
    /// The longest accepted token lifetime (`exp - iat`), in seconds.
    pub max_lifetime_secs: u64,
    /// How long a fetched key set is cached, in seconds.
    pub jwks_cache_secs: u64,
}

impl Default for TrustedIssuerOptions {
    fn default() -> Self {
        Self {
            issuer: String::new(),
            jwks_url: None,
            public_key: None,
            audience: String::new(),
            enabled: true,
            max_lifetime_secs: 300,
            jwks_cache_secs: 3600,
        }
    }
}

impl TrustedIssuerOptions {
    fn check(&self, name: &str, errors: &mut Vec<String>) {
        let key = format!("auth.trusted_issuers.{name}");
        if self.issuer.trim().is_empty() {
            errors.push(format!("{key}.issuer must not be empty"));
        }
        if self.audience.trim().is_empty() {
            errors.push(format!("{key}.audience must not be empty"));
        }
        match (&self.jwks_url, &self.public_key) {
            (Some(_), Some(_)) => errors.push(format!(
                "{key} must set either jwks_url or public_key, not both"
            )),
            (None, None) => errors.push(format!("{key} must set jwks_url or public_key")),
            (Some(url), None) if !url.starts_with("https://") && !url.starts_with("http://") => {
                errors.push(format!("{key}.jwks_url must be an http(s) URL"));
            }
            _ => {}
        }
        if self.max_lifetime_secs == 0 {
            errors.push(format!("{key}.max_lifetime_secs must be at least 1"));
        }
    }
}

/// Passkey options (configuration section `auth.passkeys`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PasskeyOptions {
    /// Whether the `/auth/passkeys/*` routes are mounted.
    pub enabled: bool,
    /// The relying party id: the app's registrable domain
    /// (`example.com`). Required when enabled.
    pub rp_id: Option<String>,
    /// The relying party name shown by the browser.
    pub rp_name: String,
    /// The origins the front end runs on (`https://app.example.com`).
    /// Required when enabled.
    pub origins: Vec<String>,
    /// How long a registration or sign-in ceremony may take, in seconds.
    pub timeout_secs: u64,
    /// `required`, `preferred` or `discouraged`: whether authenticators
    /// must verify the user (PIN, biometrics).
    pub user_verification: String,
}

impl Default for PasskeyOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            rp_id: None,
            rp_name: "Navree".to_owned(),
            origins: Vec::new(),
            timeout_secs: 300,
            user_verification: "preferred".to_owned(),
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
    fn auth_plus_defaults() {
        let options = load(ConfigSources::new()).unwrap();
        assert!(options.two_factor.required_for.is_empty());
        assert!(!options.impersonation.enabled);
        assert_eq!(options.impersonation.ttl_secs, 3600);
        assert_eq!(options.impersonation.permission, "users.impersonate");
        assert!(!options.magic_link.enabled);
        assert_eq!(options.magic_link.ttl_secs, 900);
        assert_eq!(options.magic_link.throttle_secs, 60);
        assert!(!options.otp.enabled);
        assert_eq!(options.otp.length, 6);
        assert_eq!(options.otp.ttl_secs, 600);
        assert_eq!(options.otp.max_attempts, 5);
        assert!(!options.platform.enabled);
        assert_eq!(options.platform.roles, ["platform-admin"]);
        assert_eq!(options.platform.login_throttle_per_minute, 3);
        assert_eq!(options.platform.access_ttl_secs, 900);
        assert_eq!(options.platform.session_ttl_secs, 28_800);
        assert!(options.trusted_issuers.is_empty());
        assert!(!options.passkeys.enabled);
        assert_eq!(options.passkeys.rp_name, "Navree");
        assert_eq!(options.passkeys.user_verification, "preferred");
    }

    #[test]
    fn trusted_issuers_are_read_by_name() {
        let options = load(
            ConfigSources::new()
                .set(
                    "auth.trusted_issuers.legacy.issuer",
                    "https://legacy.example.com",
                )
                .set(
                    "auth.trusted_issuers.legacy.jwks_url",
                    "https://legacy.example.com/jwks.json",
                )
                .set("auth.trusted_issuers.legacy.audience", "crm")
                .env_vars(
                    "APP_",
                    [("APP_AUTH__TRUSTED_ISSUERS__LEGACY__ENABLED", "false")],
                ),
        )
        .unwrap();
        let legacy = &options.trusted_issuers["legacy"];
        assert_eq!(legacy.issuer, "https://legacy.example.com");
        assert_eq!(legacy.audience, "crm");
        assert!(!legacy.enabled);
        assert_eq!(legacy.max_lifetime_secs, 300);
    }

    #[test]
    fn invalid_auth_plus_values_fail_validation() {
        for (key, value) in [
            ("auth.two_factor.required_for", json!([""])),
            ("auth.impersonation.ttl_secs", json!(0)),
            ("auth.magic_link.ttl_secs", json!(0)),
            ("auth.magic_link.url", json!("magic")),
            ("auth.otp.length", json!(3)),
            ("auth.otp.length", json!(11)),
            ("auth.otp.max_attempts", json!(0)),
            ("auth.platform.access_ttl_secs", json!(0)),
            ("auth.passkeys.origins", json!(["https://app.example.com/"])),
            ("auth.passkeys.user_verification", json!("always")),
            ("auth.trusted_issuers.legacy.issuer", json!("x")),
        ] {
            let result = load(ConfigSources::new().set(key, value.clone()));
            assert!(result.is_err(), "{key} = {value} should be rejected");
        }
        let error = load(ConfigSources::new().set("auth.passkeys.enabled", true))
            .unwrap_err()
            .to_string();
        assert!(error.contains("auth.passkeys.rp_id"), "{error}");
        assert!(error.contains("auth.passkeys.origins"), "{error}");
        let error = load(
            ConfigSources::new()
                .set(
                    "auth.trusted_issuers.legacy.issuer",
                    "https://legacy.example.com",
                )
                .set("auth.trusted_issuers.legacy.audience", "crm"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("auth.trusted_issuers.legacy"), "{error}");
        let error = load(
            ConfigSources::new()
                .set("auth.platform.enabled", true)
                .set("auth.platform.roles", json!([])),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("auth.platform.roles"), "{error}");
    }

    #[test]
    fn secret_is_redacted_in_debug_output() {
        let secret = "s".repeat(40);
        let options = load(ConfigSources::new().set("auth.jwt.secret", secret.clone())).unwrap();
        assert_eq!(options.jwt.secret.as_ref().unwrap().expose(), &secret);
        assert!(!format!("{options:?}").contains(&secret));
    }
}
