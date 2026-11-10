//! JSON files: a source like a TOML file, chosen by the `.json` extension.

use std::path::PathBuf;

use navree_config::{Config, ConfigError, ConfigSources};
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::TempDir;

fn write(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn tree(config: &Config) -> Value {
    config.section::<Value>("").expect("the whole tree")
}

#[test]
fn a_json_file_loads_like_a_toml_file() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let json_file = write(
        &dir,
        "app.json",
        r#"{
            "name": "from-json",
            "http": { "addr": "127.0.0.1:8080", "trusted_proxies": ["127.0.0.1", "::1"] },
            "database": { "pool": 5, "ratio": 0.5, "migrate": true, "url": null },
            "big": 18446744073709551615
        }"#,
    );
    let config = Config::load(&ConfigSources::new().file(&json_file)).expect("load");
    assert_eq!(
        tree(&config),
        json!({
            "name": "from-json",
            "http": { "addr": "127.0.0.1:8080", "trusted_proxies": ["127.0.0.1", "::1"] },
            "database": { "pool": 5, "ratio": 0.5, "migrate": true, "url": null },
            "big": 18446744073709551615u64,
        })
    );
}

#[test]
fn the_extension_picks_the_format_in_any_case() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let upper = write(&dir, "APP.JSON", r#"{"a": 1}"#);
    let toml = write(&dir, "app.toml", "b = 2\n");
    // Anything that is not `.json` is TOML, as before.
    let other = write(&dir, "app.conf", "c = 3\n");
    let config =
        Config::load(&ConfigSources::new().file(&upper).file(&toml).file(&other)).expect("load");
    assert_eq!(tree(&config), json!({ "a": 1, "b": 2, "c": 3 }));
}

#[test]
fn json_and_toml_files_stack_in_the_order_they_were_added() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let base = write(
        &dir,
        "base.toml",
        "name = \"base\"\n[db]\nurl = \"a\"\nreplicas = [\"r1\", \"r2\"]\n[db.pool]\nmin = 1\nmax = 5\n",
    );
    let production = write(
        &dir,
        "production.json",
        r#"{ "db": { "replicas": ["r3"], "pool": { "max": 9 } }, "extra": true }"#,
    );
    let config = Config::load(
        &ConfigSources::new()
            .file(&base)
            .file(&production)
            .env_vars("APP_", [("APP_DB__POOL__MIN", "2")]),
    )
    .expect("load");
    assert_eq!(
        tree(&config),
        json!({
            "name": "base",
            "db": { "url": "a", "replicas": ["r3"], "pool": { "min": "2", "max": 9 } },
            "extra": true,
        })
    );
}

#[test]
fn a_json_null_replaces_what_an_earlier_source_set_and_reads_as_none() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let base = write(&dir, "base.toml", "[http]\nper_ip_rate_limit = 600\n");
    let off = write(
        &dir,
        "off.json",
        r#"{ "http": { "per_ip_rate_limit": null } }"#,
    );
    let config = Config::load(&ConfigSources::new().file(&base).file(&off)).expect("load");
    #[derive(Debug, Deserialize)]
    struct Http {
        per_ip_rate_limit: Option<u32>,
    }
    assert_eq!(
        config
            .section::<Http>("http")
            .expect("http")
            .per_ip_rate_limit,
        None
    );
    assert_eq!(config.get_string("http.per_ip_rate_limit"), None);
}

#[test]
fn json_values_convert_like_any_other() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let file = write(
        &dir,
        "app.json",
        r#"{ "port": "8080", "flag": "yes", "ratio": 2, "name": 7 }"#,
    );
    #[derive(Debug, Deserialize)]
    struct Mixed {
        port: u16,
        flag: bool,
        ratio: f64,
        name: String,
    }
    let mixed = Config::load(&ConfigSources::new().file(&file))
        .expect("load")
        .section::<Mixed>("")
        .expect("mixed");
    assert_eq!(
        (mixed.port, mixed.flag, mixed.ratio, mixed.name.as_str()),
        (8080, true, 2.0, "7")
    );
}

#[test]
fn a_missing_json_file_is_skipped_and_a_byte_order_mark_is_accepted() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let missing = dir.path().join("missing.json");
    let bom = write(&dir, "bom.json", "\u{feff}{\"a\": 1}");
    let config = Config::load(&ConfigSources::new().file(&missing).file(&bom)).expect("load");
    assert_eq!(tree(&config), json!({ "a": 1 }));
}

#[test]
fn json_syntax_errors_name_the_file_the_line_and_the_column() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let bad = write(&dir, "app.json", "{\n  \"a\": 1,\n  \"b\": ,\n}\n");
    let error = Config::load(&ConfigSources::new().file(&bad)).expect_err("invalid JSON");
    assert!(
        matches!(
            error,
            ConfigError::Json {
                line: 3,
                column: 8,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error
            .to_string()
            .replace(&dir.path().display().to_string(), "<dir>")
            .replace('\\', "/"),
        "invalid JSON in `<dir>/app.json` at line 3, column 8: expected value"
    );
}

#[test]
fn json_files_must_be_objects_and_may_not_have_comments_or_trailing_commas() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    for (name, src) in [
        ("array.json", "[1, 2]"),
        ("scalar.json", "5"),
        ("null.json", "null"),
        ("empty.json", ""),
        ("comment.json", "{ // no\n \"a\": 1 }"),
        ("trailing.json", r#"{ "a": 1, }"#),
        ("single.json", "{ 'a': 1 }"),
    ] {
        let file = write(&dir, name, src);
        let error = Config::load(&ConfigSources::new().file(&file)).expect_err(name);
        assert!(
            matches!(error, ConfigError::Json { .. }),
            "{name}: {error:?}"
        );
    }
    let top_level = write(&dir, "top.json", "[1]");
    assert!(
        Config::load(&ConfigSources::new().file(&top_level))
            .expect_err("array")
            .to_string()
            .ends_with("the top level must be an object")
    );
}

#[test]
fn a_toml_file_named_json_is_an_error_naming_json() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let file = write(&dir, "app.json", "a = 1\n");
    let error = Config::load(&ConfigSources::new().file(&file)).expect_err("not JSON");
    assert!(matches!(error, ConfigError::Json { .. }), "{error:?}");
}

#[test]
fn duplicate_keys_in_json_take_the_last_value() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let file = write(&dir, "app.json", r#"{ "a": 1, "a": 2 }"#);
    let config = Config::load(&ConfigSources::new().file(&file)).expect("load");
    assert_eq!(tree(&config), json!({ "a": 2 }));
}
