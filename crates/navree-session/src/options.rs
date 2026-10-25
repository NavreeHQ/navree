//! [`SessionOptions`]: the `session` configuration section.

use navree_config::ConfigSection;
use serde::{Deserialize, Deserializer};

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
/// store = "database"          # memory (default) | database
/// cookie = "navree_session"
/// lifetime_minutes = 120      # idle lifetime; every request within it extends it
/// secure = true               # send the cookies over HTTPS only
/// same_site = "lax"           # strict | lax | none
/// domain = ".example.com"     # share the cookies with sibling hosts
/// csrf_except = ["/webhooks/*"]
/// ```
///
/// From the environment: `APP_SESSION__STORE=database`,
/// `APP_SESSION__SECURE=false`, and `APP_SESSION__CSRF_EXCEPT` as a
/// comma-separated list.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SessionOptions {
    /// Where records are kept (default memory).
    pub store: SessionStoreKind,
    /// The session cookie name (default `navree_session`).
    pub cookie: String,
    /// How long an idle session lives, in minutes (default 120). Requests
    /// extend it (at most one store write per minute when nothing else
    /// changed); the cookies carry the same `Max-Age`.
    pub lifetime_minutes: u64,
    /// The `Secure` cookie attribute (default `true`; set it to `false` to
    /// develop over plain HTTP on a host other than `localhost`).
    pub secure: bool,
    /// The `SameSite` cookie attribute (default lax).
    pub same_site: SameSite,
    /// The `Domain` cookie attribute (default none: the API host only). Set
    /// the shared parent domain when the SPA and the API are sibling hosts,
    /// so the SPA can read `XSRF-TOKEN`.
    pub domain: Option<String>,
    /// Request paths exempt from the CSRF check, as globs (`*` matches any
    /// characters, `?` one), e.g. `/webhooks/*`. A pattern without a
    /// leading `/` gets one.
    #[serde(deserialize_with = "list_or_comma_separated")]
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
            csrf_except: Vec::new(),
        }
    }
}

impl ConfigSection for SessionOptions {
    const KEY: &'static str = "session";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

impl SessionOptions {
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
}

/// About a year: longer lifetimes are almost certainly a unit mistake
/// (seconds for minutes).
const MAX_LIFETIME_MINUTES: u64 = 366 * 24 * 60;

/// The cookie name characters accepted (a safe subset of RFC 6265 tokens).
fn is_cookie_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
}

/// The characters of a host name (with an optional leading dot).
fn is_domain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.')
}

/// A list, or one comma-separated string (what an environment variable
/// holds); entries are trimmed and empty ones dropped.
fn list_or_comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entries {
        List(Vec<String>),
        Joined(String),
    }
    let entries = match Entries::deserialize(deserializer)? {
        Entries::List(entries) => entries,
        Entries::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(entries
        .into_iter()
        .map(|entry| entry.trim().to_owned())
        .filter(|entry| !entry.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};

    use super::*;

    fn from(sources: ConfigSources) -> Result<SessionOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<SessionOptions>()
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
        assert!(options.csrf_except.is_empty());
        assert!(options.check().is_ok());
        assert_eq!(options.lifetime(), chrono::TimeDelta::minutes(120));
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
        assert!(from(ConfigSources::new().set("session.store", "redis")).is_err());
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
