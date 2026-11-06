//! [`SessionOptions`]: the `session` configuration section.

use navree_config::ConfigSection;
use serde::Deserialize;

/// Where session records are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStoreKind {
    /// In the process ([`MemoryStore`](crate::MemoryStore)): lost on
    /// restart and not shared between instances. The default.
    #[default]
    Memory,
    /// In the `sessions` table ([`DatabaseStore`](crate::DatabaseStore));
    /// needs `DatabaseModule` in the app.
    Database,
    /// In Redis: the `dyn SessionStore` provider named `redis`, which
    /// navree-redis's `RedisModule` registers; needs it in the app.
    Redis,
}

/// The `SameSite` attribute of the session and `XSRF-TOKEN` cookies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SameSite {
    /// Sent on same-site requests only.
    Strict,
    /// Sent on same-site requests and top-level cross-site navigations
    /// (links, not form posts). The default.
    #[default]
    Lax,
    /// Sent on every request; requires `secure = true`.
    None,
}

/// Session settings (configuration section `session`; every key optional).
///
/// ```toml
/// [session]
/// store = "database"          # memory (default) | database | redis
/// cookie = "navree_session"
/// lifetime_minutes = 120      # idle lifetime; every request within it extends it
/// secure = true               # send the cookies over HTTPS only
/// same_site = "lax"           # strict | lax | none
/// domain = ".example.com"     # share the cookies with sibling hosts
/// host_prefix = true          # `__Host-navree_session` whenever the cookie may carry it
/// csrf_except = ["/webhooks/*"]
/// ```
///
/// From the environment: `APP_SESSION__STORE=database`,
/// `APP_SESSION__SECURE=false`, and `APP_SESSION__CSRF_EXCEPT` as a
/// comma-separated list.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "session", docs = "/docs/basics/sessions-and-csrf/", validate = Self::check_values)]
#[serde(default)]
pub struct SessionOptions {
    /// Where records are kept (default memory).
    #[config(one_of = "memory, database, redis")]
    pub store: SessionStoreKind,
    /// The session cookie name (default `navree_session`).
    #[config(public)]
    pub cookie: String,
    /// How long an idle session lives, in minutes (default 120). Requests
    /// extend it (at most one store write per minute when nothing else
    /// changed); the cookies carry the same `Max-Age`.
    pub lifetime_minutes: u64,
    /// The `Secure` cookie attribute (default `true`; set it to `false` to
    /// develop over plain HTTP on a host other than `localhost`).
    pub secure: bool,
    /// The `SameSite` cookie attribute (default lax).
    #[config(one_of = "strict, lax, none")]
    pub same_site: SameSite,
    /// The `Domain` cookie attribute (default none: the API host only). Set
    /// the shared parent domain when the SPA and the API are sibling hosts,
    /// so the SPA can read `XSRF-TOKEN`.
    pub domain: Option<String>,
    /// Whether the session cookie's name starts with `__Host-` when it can
    /// (default `true`): with `secure` on and no `domain`, browsers then
    /// accept the cookie only from this host, over HTTPS, for the whole
    /// site, so a sibling subdomain can't plant a session cookie of its
    /// own. See [`cookie_name`](SessionOptions::cookie_name).
    pub host_prefix: bool,
    /// Request paths exempt from the CSRF check, as globs (`*` matches any
    /// characters, `?` one), e.g. `/webhooks/*`. A pattern without a
    /// leading `/` gets one.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub csrf_except: Vec<String>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            store: SessionStoreKind::default(),
            cookie: "navree_session".to_owned(),
            lifetime_minutes: 120,
            secure: true,
            same_site: SameSite::default(),
            domain: None,
            host_prefix: true,
            csrf_except: Vec::new(),
        }
    }
}

impl SessionOptions {
    /// Checks the values and joins the problems (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }

    /// Every problem with these options (empty when valid).
    pub(crate) fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.cookie.is_empty() || !self.cookie.bytes().all(is_cookie_name_byte) {
            errors.push(format!(
                "session.cookie must be a non-empty cookie name of letters, digits and `-_.` (got `{}`)",
                self.cookie
            ));
        } else if self.cookie == crate::XSRF_COOKIE {
            errors.push(format!(
                "session.cookie must not be `{}`, the CSRF cookie",
                crate::XSRF_COOKIE
            ));
        } else if starts_with_ignore_case(&self.cookie, HOST_PREFIX)
            || starts_with_ignore_case(&self.cookie, SECURE_PREFIX)
        {
            errors.push(format!(
                "session.cookie must be a plain name such as `navree_session` (got `{}`): \
                 the `__Host-` prefix is added when the cookie can carry it (session.host_prefix)",
                self.cookie
            ));
        }
        if self.lifetime_minutes == 0 {
            errors.push("session.lifetime_minutes must be at least 1".to_owned());
        } else if self.lifetime_minutes > MAX_LIFETIME_MINUTES {
            errors.push(format!(
                "session.lifetime_minutes must be at most {MAX_LIFETIME_MINUTES} (about a year)"
            ));
        }
        if self.same_site == SameSite::None && !self.secure {
            errors.push(
                "session.same_site = \"none\" needs session.secure = true (browsers reject it otherwise)"
                    .to_owned(),
            );
        }
        if let Some(domain) = &self.domain
            && (domain.is_empty() || !domain.bytes().all(is_domain_byte))
        {
            errors.push(format!(
                "session.domain must be a host name such as `.example.com` (got `{domain}`)"
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The lifetime as a duration.
    pub(crate) fn lifetime(&self) -> chrono::TimeDelta {
        chrono::TimeDelta::minutes(i64::try_from(self.lifetime_minutes).unwrap_or(i64::MAX))
    }

    /// The name the session cookie is set and read under: `__Host-` and
    /// [`cookie`](SessionOptions::cookie) when [`host_prefix`](SessionOptions::host_prefix)
    /// is on, the cookie is `secure` and has no `domain` (what browsers
    /// require of the prefix), else `cookie` as it is.
    ///
    /// ```
    /// use navree_session::SessionOptions;
    ///
    /// assert_eq!(SessionOptions::default().cookie_name(), "__Host-navree_session");
    /// let plain_http = SessionOptions { secure: false, ..SessionOptions::default() };
    /// assert_eq!(plain_http.cookie_name(), "navree_session");
    /// let shared = SessionOptions { domain: Some(".example.com".into()), ..SessionOptions::default() };
    /// assert_eq!(shared.cookie_name(), "navree_session");
    /// ```
    pub fn cookie_name(&self) -> String {
        if self.host_prefix && self.secure && self.domain.is_none() {
            format!("{HOST_PREFIX}{}", self.cookie)
        } else {
            self.cookie.clone()
        }
    }
}

/// The cookie name prefix browsers bind to one host (RFC 6265bis 4.1.3.2).
const HOST_PREFIX: &str = "__Host-";

/// The cookie name prefix browsers accept over HTTPS only.
const SECURE_PREFIX: &str = "__Secure-";

/// Whether `name` starts with `prefix`, ignoring ASCII case (as browsers
/// compare cookie prefixes).
fn starts_with_ignore_case(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len()
        && name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

// About a year: longer lifetimes are almost certainly a unit mistake
// (seconds for minutes).
use crate::session::MAX_LIFETIME_MINUTES;

/// The cookie name characters accepted (a safe subset of RFC 6265 tokens).
fn is_cookie_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
}

/// The characters of a host name (with an optional leading dot).
fn is_domain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.')
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources, FieldType};

    use super::*;

    fn from(sources: ConfigSources) -> Result<SessionOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<SessionOptions>()
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = SessionOptions::schema().expect("schema");
        assert_eq!(schema.key, "session");
        assert_eq!(
            schema.docs.as_deref(),
            Some("/docs/basics/sessions-and-csrf/")
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
        assert_eq!(
            field("store").ty,
            FieldType::choice(&["memory", "database", "redis"])
        );
        assert_eq!(
            field("same_site").ty,
            FieldType::choice(&["strict", "lax", "none"])
        );
        assert_eq!(field("lifetime_minutes").ty, FieldType::Integer);
        assert_eq!(field("csrf_except").ty, FieldType::list(FieldType::String));
        let defaults = SessionOptions::defaults().expect("defaults");
        assert_eq!(defaults["store"], "memory");
        assert_eq!(defaults["same_site"], "lax");
        assert_eq!(defaults["cookie"], "navree_session");
        assert_eq!(defaults["lifetime_minutes"], 120);
        assert_eq!(defaults["secure"], true);
    }

    #[test]
    fn defaults_match_the_plan() {
        let options = SessionOptions::default();
        assert_eq!(options.store, SessionStoreKind::Memory);
        assert_eq!(options.cookie, "navree_session");
        assert_eq!(options.lifetime_minutes, 120);
        assert!(options.secure);
        assert_eq!(options.same_site, SameSite::Lax);
        assert_eq!(options.domain, None);
        assert!(options.host_prefix);
        assert!(options.csrf_except.is_empty());
        assert!(options.check().is_ok());
        assert_eq!(options.lifetime(), chrono::TimeDelta::minutes(120));
    }

    // The session cookie carries the `__Host-` prefix whenever
    // browsers allow it (Secure, no Domain), so no sibling host can plant one.
    #[test]
    fn the_cookie_name_carries_the_host_prefix_when_it_can() {
        assert_eq!(
            SessionOptions::default().cookie_name(),
            "__Host-navree_session"
        );
        let renamed = SessionOptions {
            cookie: "crm_session".into(),
            ..SessionOptions::default()
        };
        assert_eq!(renamed.cookie_name(), "__Host-crm_session");
        for options in [
            SessionOptions {
                secure: false,
                ..SessionOptions::default()
            },
            SessionOptions {
                domain: Some(".example.com".into()),
                ..SessionOptions::default()
            },
            SessionOptions {
                host_prefix: false,
                ..SessionOptions::default()
            },
        ] {
            assert_eq!(options.cookie_name(), "navree_session", "{options:?}");
        }
    }

    #[test]
    fn a_missing_section_gives_the_defaults() {
        assert_eq!(
            from(ConfigSources::new()).unwrap(),
            SessionOptions::default()
        );
    }

    #[test]
    fn every_key_reads_from_configuration() {
        let options = from(ConfigSources::new().set(
            "session",
            serde_json::json!({
                "store": "database",
                "cookie": "crm_session",
                "lifetime_minutes": 30,
                "secure": false,
                "same_site": "strict",
                "domain": ".example.com",
                "csrf_except": ["/webhooks/*", "stripe/*"],
            }),
        ))
        .unwrap();
        assert_eq!(
            options,
            SessionOptions {
                store: SessionStoreKind::Database,
                cookie: "crm_session".into(),
                lifetime_minutes: 30,
                secure: false,
                same_site: SameSite::Strict,
                domain: Some(".example.com".into()),
                host_prefix: true,
                csrf_except: vec!["/webhooks/*".into(), "stripe/*".into()],
            }
        );
    }

    #[test]
    fn environment_variables_are_strings() {
        let options = from(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_SESSION__STORE", "database"),
                ("APP_SESSION__SECURE", "false"),
                ("APP_SESSION__LIFETIME_MINUTES", "45"),
                ("APP_SESSION__SAME_SITE", "lax"),
                ("APP_SESSION__CSRF_EXCEPT", "/webhooks/*, /stripe/*,"),
            ],
        ))
        .unwrap();
        assert_eq!(options.store, SessionStoreKind::Database);
        assert!(!options.secure);
        assert_eq!(options.lifetime_minutes, 45);
        assert_eq!(options.csrf_except, ["/webhooks/*", "/stripe/*"]);
    }

    #[test]
    fn unknown_store_and_same_site_values_fail() {
        assert!(from(ConfigSources::new().set("session.store", "carrier-pigeon")).is_err());
        assert!(from(ConfigSources::new().set("session.same_site", "sometimes")).is_err());
    }

    #[test]
    fn the_redis_store_is_a_store_name() {
        let options = from(ConfigSources::new().set("session.store", "redis")).unwrap();
        assert_eq!(options.store, SessionStoreKind::Redis);
    }

    #[test]
    fn unknown_same_site_values_fail() {
        assert!(from(ConfigSources::new().set("session.same_site", "sometimes")).is_err());
    }

    #[test]
    fn invalid_options_are_rejected_with_their_key() {
        let cases = [
            (
                SessionOptions {
                    cookie: String::new(),
                    ..SessionOptions::default()
                },
                "session.cookie",
            ),
            (
                SessionOptions {
                    cookie: "my session".into(),
                    ..SessionOptions::default()
                },
                "session.cookie",
            ),
            (
                SessionOptions {
                    cookie: "XSRF-TOKEN".into(),
                    ..SessionOptions::default()
                },
                "session.cookie",
            ),
            (
                SessionOptions {
                    cookie: "__Host-navree_session".into(),
                    ..SessionOptions::default()
                },
                "session.cookie",
            ),
            (
                SessionOptions {
                    cookie: "__secure-navree_session".into(),
                    ..SessionOptions::default()
                },
                "session.cookie",
            ),
            (
                SessionOptions {
                    lifetime_minutes: 0,
                    ..SessionOptions::default()
                },
                "session.lifetime_minutes",
            ),
            (
                SessionOptions {
                    lifetime_minutes: 10_000_000,
                    ..SessionOptions::default()
                },
                "session.lifetime_minutes",
            ),
            (
                SessionOptions {
                    same_site: SameSite::None,
                    secure: false,
                    ..SessionOptions::default()
                },
                "session.same_site",
            ),
            (
                SessionOptions {
                    domain: Some("exa mple.com".into()),
                    ..SessionOptions::default()
                },
                "session.domain",
            ),
            (
                SessionOptions {
                    domain: Some(String::new()),
                    ..SessionOptions::default()
                },
                "session.domain",
            ),
        ];
        for (options, key) in cases {
            let errors = options.check().expect_err(key);
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(errors[0].contains(key), "{errors:?}");
        }
        let fine = SessionOptions {
            same_site: SameSite::None,
            secure: true,
            ..SessionOptions::default()
        };
        assert!(fine.check().is_ok());
    }

    #[test]
    fn validation_runs_when_read_from_configuration() {
        let error = from(ConfigSources::new().set("session.lifetime_minutes", 0)).unwrap_err();
        assert!(error.to_string().contains("session"), "{error}");
    }
}
