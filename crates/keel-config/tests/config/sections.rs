//! Reading typed sections and single values.

use keel_config::{Config, ConfigError, ConfigSources};
use keel_contracts::Secret;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct DatabaseConfig {
    url: String,
    pool: u32,
    #[serde(default)]
    log_queries: bool,
    password: Secret<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
struct CacheConfig {
    ttl_secs: u64,
}

fn config() -> Config {
    let dir = tempfile::tempdir().expect("tempdir");
    let toml = dir.path().join("app.toml");
    std::fs::write(
        &toml,
        "[database]\nurl = \"sqlite://app.db\"\npool = 5\npassword = \"s3cret\"\n",
    )
    .expect("write fixture");
    Config::load(&ConfigSources::new().file(&toml).env_vars(
        "APP_",
        [
            ("APP_DATABASE__POOL", "10"),
            ("APP_DATABASE__LOG_QUERIES", "true"),
        ],
    ))
    .expect("load")
}

#[test]
fn section_deserializes_typed_struct() {
    let db: DatabaseConfig = config().section("database").expect("database section");
    assert_eq!(db.url, "sqlite://app.db");
    // Environment values are strings; typed fields convert them.
    assert_eq!(db.pool, 10);
    assert!(db.log_queries);
    assert_eq!(db.password.expose(), "s3cret");
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ServerConfig {
    port: u16,
}

/// A value whose deserializer rejects bad input with `invalid_value`, as
/// `FromStr`-style types (URLs, addresses) do.
#[derive(Debug)]
struct HttpsUrl;

impl<'de> Deserialize<'de> for HttpsUrl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, Unexpected};
        let url = String::deserialize(d)?;
        if url.starts_with("https://") {
            Ok(HttpsUrl)
        } else {
            Err(D::Error::invalid_value(
                Unexpected::Str(&url),
                &"an https URL",
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct WebhookConfig {
    url: HttpsUrl,
}

#[test]
fn section_errors_name_the_key_but_not_the_value() {
    let config = Config::load(&ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_SERVER__PORT", "hunter2"),
            ("APP_WEBHOOK__URL", "http://user:hunter2@example.com"),
        ],
    ))
    .expect("load");

    let error = config
        .section::<ServerConfig>("server")
        .expect_err("port is not a number");
    let text = error.to_string();
    assert!(
        matches!(&error, ConfigError::Section { key, .. } if key == "server"),
        "{error:?}"
    );
    assert!(text.contains("port"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");

    let text = config
        .section::<WebhookConfig>("webhook")
        .expect_err("not https")
        .to_string();
    assert!(text.contains("webhook"), "{text}");
    assert!(text.contains("an https URL"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
}

#[test]
fn get_string_and_get_bool_convert_scalars() {
    let config = Config::load(&ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_DEBUG", "yes"),
            ("APP_MAINTENANCE", "0"),
            ("APP_NAME", "Keel"),
        ],
    ))
    .expect("load");
    assert_eq!(config.get_bool("debug"), Some(true));
    assert_eq!(config.get_bool("maintenance"), Some(false));
    assert_eq!(config.get_bool("name"), None, "not a boolean");
    assert_eq!(config.get_bool("missing"), None);

    let dir = tempfile::tempdir().expect("tempdir");
    let toml = dir.path().join("app.toml");
    std::fs::write(&toml, "[server]\nport = 8080\n").expect("write fixture");
    let config = Config::load(&ConfigSources::new().file(&toml)).expect("load");
    assert_eq!(config.get_string("server.port").as_deref(), Some("8080"));
    assert_eq!(config.get_string("server"), None, "a table is not a string");
}

#[test]
fn missing_section_is_an_error_unless_defaulted() {
    let config = config();
    let error = config
        .section::<CacheConfig>("cache")
        .expect_err("no cache section");
    assert!(
        matches!(&error, ConfigError::MissingSection { key } if key == "cache"),
        "{error:?}"
    );
    assert_eq!(
        config
            .section_or_default::<CacheConfig>("cache")
            .expect("default"),
        CacheConfig::default()
    );
}
