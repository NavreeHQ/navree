//! Where each value came from: `Config::origin` and
//! `Config::keys`, for every kind of source, and the precedence they report.

use navree_config::{Config, ConfigSources, ValueOrigin};
use tempfile::TempDir;

fn dir() -> TempDir {
    tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir")
}

fn write(dir: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

#[test]
fn a_dotenv_value_names_its_file_line_and_variable() {
    let dir = dir();
    let env = write(
        &dir,
        ".env",
        "# comment\n\nAPP_MAIL__FROM=\"Agra <hello@acme.test>\"\nAPP_LOG__LEVEL=debug\n",
    );
    let config = Config::load(&ConfigSources::new().dotenv(&env)).expect("load");
    let file = env.display().to_string();
    assert_eq!(
        config.origin("mail.from"),
        Some(ValueOrigin::DotEnv {
            file: file.clone(),
            line: 3,
            variable: "APP_MAIL__FROM".to_owned()
        })
    );
    assert_eq!(
        config.origin("log.level").unwrap().to_string(),
        format!("{file}:4 (APP_LOG__LEVEL)")
    );
}

#[test]
fn a_repeated_dotenv_variable_reports_the_line_that_wins() {
    let dir = dir();
    let env = write(&dir, ".env", "APP_LOG__LEVEL=info\nAPP_LOG__LEVEL=debug\n");
    let config = Config::load(&ConfigSources::new().dotenv(&env)).expect("load");
    assert!(
        matches!(
            config.origin("log.level"),
            Some(ValueOrigin::DotEnv { line: 2, .. })
        ),
        "{:?}",
        config.origin("log.level")
    );
}

#[test]
fn a_file_value_names_the_file() {
    let dir = dir();
    let file = write(&dir, "app.toml", "[http]\naddr = \"0.0.0.0:80\"\n");
    let json = write(&dir, "extra.json", "{\"cache\": {\"driver\": \"memory\"}}");
    let config = Config::load(&ConfigSources::new().file(&file).file(&json)).expect("load");
    assert_eq!(
        config.origin("http.addr"),
        Some(ValueOrigin::File {
            file: file.display().to_string()
        })
    );
    assert_eq!(
        config.origin("cache.driver"),
        Some(ValueOrigin::File {
            file: json.display().to_string()
        })
    );
}

#[test]
fn an_environment_value_names_its_variable() {
    let config = Config::load(
        &ConfigSources::new().env_vars("APP_", [("APP_DATABASE__URL", "sqlite://app.db")]),
    )
    .expect("load");
    let origin = config.origin("database.url").expect("origin");
    assert_eq!(
        origin,
        ValueOrigin::Environment {
            variable: "APP_DATABASE__URL".to_owned()
        }
    );
    assert_eq!(origin.to_string(), "environment (APP_DATABASE__URL)");
}

#[test]
fn a_set_value_and_a_missing_key() {
    let config = Config::load(&ConfigSources::new().set("a.b", 1)).expect("load");
    assert_eq!(config.origin("a.b"), Some(ValueOrigin::Set));
    assert_eq!(ValueOrigin::Set.to_string(), "ConfigSources::set");
    assert_eq!(config.origin("a.c"), None, "no source sets it");
    assert_eq!(config.origin("a"), None, "a table has no single origin");
    assert_eq!(ValueOrigin::Default.to_string(), "default");
}

#[test]
fn the_origin_is_the_source_that_wins() {
    let dir = dir();
    let env = write(
        &dir,
        ".env",
        "APP_HTTP__ADDR=1.1.1.1:1\nAPP_LOG__LEVEL=warn\n",
    );
    let file = write(&dir, "app.toml", "[http]\naddr = \"2.2.2.2:2\"\n");
    let config = Config::load(
        &ConfigSources::new()
            .dotenv(&env)
            .file(&file)
            .env_vars("APP_", [("APP_LOG__LEVEL", "debug")])
            .set("cache.driver", "memory"),
    )
    .expect("load");
    assert!(
        config.origin("http.addr").unwrap().is_file(),
        "file over .env"
    );
    assert!(
        matches!(
            config.origin("log.level"),
            Some(ValueOrigin::Environment { .. })
        ),
        "environment over .env"
    );
    assert_eq!(config.origin("cache.driver"), Some(ValueOrigin::Set));
}

#[test]
fn keys_lists_every_leaf_in_order() {
    let config = Config::load(
        &ConfigSources::new()
            .set("b.x", 1)
            .set("a", "one")
            .set("b.y.z", true)
            .set("list", serde_json::json!([1, 2])),
    )
    .expect("load");
    assert_eq!(config.keys(), ["a", "b.x", "b.y.z", "list"]);
}

#[test]
fn origins_are_never_part_of_error_messages_beyond_the_label() {
    // The label that type errors quote is unchanged: the file path, the
    // `.env` path, `environment` or `ConfigSources::set`.
    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct Port {
        port: u16,
    }
    let config = Config::load(&ConfigSources::new().env_vars("APP_", [("APP_NET__PORT", "http")]))
        .expect("load");
    let text = config.section::<Port>("net").unwrap_err().to_string();
    assert!(text.ends_with("in environment"), "{text}");
}

#[test]
fn keys_list_the_short_names_of_the_app_settings_under_app() {
    let config = Config::load(
        &ConfigSources::new()
            .env_vars("APP_", [("APP_ENV", "staging"), ("APP_LOG__LEVEL", "info")]),
    )
    .expect("load");
    assert_eq!(config.keys(), ["app.env", "log.level"]);
}
