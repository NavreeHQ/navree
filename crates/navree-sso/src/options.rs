//! [`SsoOptions`]: the `sso` configuration section.

use navree_config::{ConfigSection, ConfigTable};
use serde::{Deserialize, Deserializer, Serialize};

/// The `sso` configuration section. Environment variables:
/// `APP_SSO__<KEY>`, nested with `__` (`APP_SSO__SAML__ALLOW_IDP_INITIATED`).
///
/// ```toml
/// [sso]
/// base_url = "https://api.acme.test"                      # default: app.url
/// complete_url = "https://app.acme.test/auth/sso/complete"
/// enforce_for_verified_domains = true
///
/// [sso.saml]
/// allow_idp_initiated = false
/// clock_skew_secs = 120
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigSection)]
#[config(key = "sso", docs = "/docs/security/single-sign-on/", validate = Self::check_values)]
#[serde(default)]
pub struct SsoOptions {
    /// The API's public base URL (`https://api.acme.test`): the assertion
    /// consumer service, SAML metadata and OpenID Connect callback URLs
    /// start with it. `app.url` when unset.
    pub base_url: Option<String>,
    /// The front end's page that finishes a sign-in. The assertion
    /// consumer service and the callback redirect there with `?code=` (a
    /// one-time code for `POST /sso/exchange`), or with `?error=` and
    /// `&message=`. When unset they answer with the login JSON instead.
    pub complete_url: Option<String>,
    /// Refuse password sign-in and registration for emails in a verified
    /// domain of an enabled connection (a connection's own `enforce`
    /// overrides it). Default `false`.
    pub enforce_for_verified_domains: bool,
    /// Whether the routes are mounted (default `true`).
    pub routes: bool,
    /// Whether the management routes (`connections`, `domains`; permission
    /// `sso.manage`) are mounted with the others (default `true`). They
    /// need a `RouteAuthorizer` (`AccessModule`): turn them off in an app
    /// without one.
    pub management_routes: bool,
    /// The routes' prefix under `http.api_prefix` (default `/sso`).
    pub routes_prefix: String,
    /// How long a sign-in may take from the login route to the identity
    /// provider's answer, in seconds (default 600).
    pub state_ttl_secs: u64,
    /// How long the one-time code of a finished sign-in works, in seconds
    /// (default 60).
    pub code_ttl_secs: u64,
    /// Verified email domains.
    #[config(table)]
    pub domains: DomainOptions,
    /// SAML connections.
    #[config(table)]
    pub saml: SamlOptions,
    /// OpenID Connect connections.
    #[config(table)]
    pub oidc: OidcOptions,
}

impl Default for SsoOptions {
    fn default() -> Self {
        SsoOptions {
            base_url: None,
            complete_url: None,
            enforce_for_verified_domains: false,
            routes: true,
            management_routes: true,
            routes_prefix: "/sso".to_owned(),
            state_ttl_secs: 600,
            code_ttl_secs: 60,
            domains: DomainOptions::default(),
            saml: SamlOptions::default(),
            oidc: OidcOptions::default(),
        }
    }
}

/// `sso.domains`: how a domain proves it belongs to a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct DomainOptions {
    /// The TXT record's value is `{txt_prefix}={token}` (default
    /// `navree-sso-verification`).
    pub txt_prefix: String,
    /// The DNS-over-HTTPS endpoint (JSON API) TXT records are read from
    /// (default `https://cloudflare-dns.com/dns-query`).
    pub doh_url: String,
}

impl Default for DomainOptions {
    fn default() -> Self {
        DomainOptions {
            txt_prefix: "navree-sso-verification".to_owned(),
            doh_url: "https://cloudflare-dns.com/dns-query".to_owned(),
        }
    }
}

/// `sso.saml`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct SamlOptions {
    /// Accept responses the identity provider sends without a request
    /// (IdP-initiated sign-in, from a dashboard tile). Default `false`:
    /// such responses cannot be tied to a browser session.
    pub allow_idp_initiated: bool,
    /// How far the identity provider's clock may be off, in seconds
    /// (default 120).
    pub clock_skew_secs: u64,
    /// How long fetched metadata (`metadata_url`) is used, in seconds
    /// (default 3600).
    pub metadata_ttl_secs: u64,
    /// The largest `SAMLResponse` accepted, in bytes (default 1 MiB).
    pub max_response_bytes: usize,
}

impl Default for SamlOptions {
    fn default() -> Self {
        SamlOptions {
            allow_idp_initiated: false,
            clock_skew_secs: 120,
            metadata_ttl_secs: 3600,
            max_response_bytes: 1024 * 1024,
        }
    }
}

/// `sso.oidc`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct OidcOptions {
    /// The scopes asked for when a connection names none (default
    /// `openid email profile`; a comma-separated string from the
    /// environment).
    #[serde(deserialize_with = "comma_or_space_list")]
    pub scopes: Vec<String>,
    /// How far the provider's clock may be off, in seconds (default 60).
    pub clock_skew_secs: u64,
}

impl Default for OidcOptions {
    fn default() -> Self {
        OidcOptions {
            scopes: vec![
                "openid".to_owned(),
                "email".to_owned(),
                "profile".to_owned(),
            ],
            clock_skew_secs: 60,
        }
    }
}

/// A list, or a comma- or space-separated string (environment variables):
/// the comma-separated list of navree-config, with each item split on spaces
/// too, since scopes are words.
fn comma_or_space_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let items = navree_config::comma_list(deserializer)?;
    Ok(items
        .iter()
        .flat_map(|item| item.split_whitespace())
        .map(str::to_owned)
        .collect())
}

fn absolute_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

impl SsoOptions {
    /// Checks the values and joins the problems (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }

    /// Checks the options; the errors name their keys.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        for (key, value) in [
            ("sso.base_url", &self.base_url),
            ("sso.complete_url", &self.complete_url),
        ] {
            if let Some(value) = value
                && !absolute_url(value)
            {
                errors.push(format!("{key} must be an absolute http(s) URL"));
            }
        }
        if !self.routes_prefix.starts_with('/') {
            errors.push("sso.routes_prefix must start with `/`".to_owned());
        }
        if self.state_ttl_secs == 0 {
            errors.push("sso.state_ttl_secs must be at least 1".to_owned());
        }
        if self.code_ttl_secs == 0 {
            errors.push("sso.code_ttl_secs must be at least 1".to_owned());
        }
        if self.domains.txt_prefix.trim().is_empty() || self.domains.txt_prefix.contains('=') {
            errors.push("sso.domains.txt_prefix must be a non-empty word without `=`".to_owned());
        }
        if !absolute_url(&self.domains.doh_url) {
            errors.push("sso.domains.doh_url must be an absolute http(s) URL".to_owned());
        }
        if self.saml.max_response_bytes < 1024 {
            errors.push("sso.saml.max_response_bytes must be at least 1024".to_owned());
        }
        if !self.oidc.scopes.iter().any(|s| s == "openid") {
            errors.push("sso.oidc.scopes must include `openid`".to_owned());
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
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn defaults_are_valid() {
        let options = SsoOptions::default();
        assert_eq!(options.check(), Ok(()));
        assert_eq!(options.routes_prefix, "/sso");
        assert!(!options.saml.allow_idp_initiated);
        assert_eq!(options.saml.clock_skew_secs, 120);
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = SsoOptions::schema().expect("schema");
        assert_eq!(schema.key, "sso");
        assert_eq!(
            schema.docs.as_deref(),
            Some("/docs/security/single-sign-on/")
        );
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("no field {name}"))
        };
        assert_eq!(field("base_url").ty, FieldType::String);
        assert_eq!(field("routes").ty, FieldType::Bool);
        assert_eq!(field("state_ttl_secs").ty, FieldType::Integer);
        for table in ["domains", "saml", "oidc"] {
            assert_eq!(field(table).ty, FieldType::Table, "{table}");
            assert!(!field(table).fields.is_empty(), "{table}");
        }
        let oidc = field("oidc");
        let scopes = oidc.fields.iter().find(|field| field.name == "scopes");
        assert_eq!(
            scopes.map(|field| field.ty.clone()),
            Some(FieldType::list(FieldType::String))
        );
        let defaults = SsoOptions::defaults().expect("defaults");
        assert_eq!(defaults["routes_prefix"], "/sso");
        assert_eq!(defaults["state_ttl_secs"], 600);
        assert_eq!(defaults["saml"]["clock_skew_secs"], 120);
        assert_eq!(
            defaults["oidc"]["scopes"],
            serde_json::json!(["openid", "email", "profile"])
        );
        assert!(SsoOptions::default().validate().is_ok());
    }

    #[test]
    fn scopes_read_from_a_string() {
        let options: OidcOptions =
            serde_json::from_value(serde_json::json!({ "scopes": "openid, email groups" }))
                .unwrap();
        assert_eq!(options.scopes, ["openid", "email", "groups"]);
    }

    #[test]
    fn errors_name_their_keys() {
        let options = SsoOptions {
            base_url: Some("api.acme.test".to_owned()),
            routes_prefix: "sso".to_owned(),
            code_ttl_secs: 0,
            oidc: OidcOptions {
                scopes: vec!["email".to_owned()],
                ..OidcOptions::default()
            },
            ..SsoOptions::default()
        };
        let errors = options.check().unwrap_err();
        for key in [
            "sso.base_url",
            "sso.routes_prefix",
            "sso.code_ttl_secs",
            "sso.oidc.scopes",
        ] {
            assert!(
                errors.iter().any(|e| e.starts_with(key)),
                "{key}: {errors:?}"
            );
        }
    }
}
