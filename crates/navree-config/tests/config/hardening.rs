//! Limits on what a load accepts, the reach of the environment prefix, and
//! secrets in `Debug` and error output.

use std::collections::BTreeSet;

use navree_config::{Config, ConfigError, ConfigSources};
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::TempDir;

fn write(dir: &TempDir, name: &str, contents: impl AsRef<[u8]>) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn scrub(dir: &TempDir, text: impl ToString) -> String {
    text.to_string()
        .replace(&dir.path().display().to_string(), "<dir>")
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// File size
// ---------------------------------------------------------------------------

#[test]
fn a_file_over_the_size_limit_is_refused_and_one_at_the_limit_is_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let toml = write(&dir, "app.toml", "a = 1\n");
    let size = std::fs::metadata(&toml).expect("metadata").len();

    assert!(Config::load(&ConfigSources::new().file(&toml).max_file_size(size)).is_ok());
    let error = Config::load(&ConfigSources::new().file(&toml).max_file_size(size - 1))
        .expect_err("one byte over");
    assert!(
        matches!(&error, ConfigError::TooLarge { limit, .. } if *limit == size - 1),
        "{error:?}"
    );
    assert_eq!(
        scrub(&dir, &error),
        format!(
            "configuration file `<dir>/app.toml` is larger than the limit of {} bytes",
            size - 1
        )
    );
}

#[test]
fn the_size_limit_applies_to_dotenv_toml_and_json_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dotenv = write(&dir, ".env", "APP_A=1\nAPP_B=2\n");
    let toml = write(&dir, "app.toml", "a = 1\nb = 2\n");
    let json = write(&dir, "app.json", r#"{"a": 1, "b": 2}"#);
    for sources in [
        ConfigSources::new().dotenv(&dotenv),
        ConfigSources::new().file(&toml),
        ConfigSources::new().file(&json),
    ] {
        let error = Config::load(&sources.max_file_size(4)).expect_err("too large");
        assert!(
            matches!(error, ConfigError::TooLarge { limit: 4, .. }),
            "{error:?}"
        );
    }
}

#[test]
fn the_default_size_limit_is_one_mebibyte() {
    assert_eq!(ConfigSources::DEFAULT_MAX_FILE_SIZE, 1024 * 1024);
    let dir = tempfile::tempdir().expect("tempdir");
    let limit = usize::try_from(ConfigSources::DEFAULT_MAX_FILE_SIZE).expect("fits");
    // A TOML comment padded to exactly the limit loads; one byte more does not.
    let padding = |size: usize| format!("a = 1\n#{}", "x".repeat(size - "a = 1\n#".len()));
    let at_limit = write(&dir, "at.toml", padding(limit));
    let over = write(&dir, "over.toml", padding(limit + 1));
    let config = Config::load(&ConfigSources::new().file(&at_limit)).expect("at the limit");
    assert_eq!(config.get_string("a").as_deref(), Some("1"));
    let error = Config::load(&ConfigSources::new().file(&over)).expect_err("over the limit");
    assert!(matches!(error, ConfigError::TooLarge { .. }), "{error:?}");
}

#[test]
fn a_larger_limit_can_be_asked_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    let size = usize::try_from(ConfigSources::DEFAULT_MAX_FILE_SIZE).expect("fits") + 10;
    let big = write(&dir, "big.toml", format!("a = 1\n#{}", "x".repeat(size)));
    assert!(Config::load(&ConfigSources::new().file(&big)).is_err());
    assert!(
        Config::load(
            &ConfigSources::new()
                .file(&big)
                .max_file_size(2 * size as u64)
        )
        .is_ok()
    );
}

#[test]
fn a_file_over_the_limit_is_refused_even_when_it_is_not_valid() {
    // The size check comes before parsing, so a huge garbage file costs a
    // bounded read, not a parse.
    let dir = tempfile::tempdir().expect("tempdir");
    let garbage = write(&dir, "app.toml", vec![0xFF; 4096]);
    let error = Config::load(&ConfigSources::new().file(&garbage).max_file_size(100))
        .expect_err("too large");
    assert!(matches!(error, ConfigError::TooLarge { .. }), "{error:?}");
}

// ---------------------------------------------------------------------------
// Depth
// ---------------------------------------------------------------------------

#[test]
fn nesting_deeper_than_the_limit_is_refused_in_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Levels count the root table: `[a.b]` is three.
    let toml_ok = write(&dir, "ok.toml", "[a.b]\nc = 1\n");
    let toml_deep = write(&dir, "deep.toml", "[a.b.c]\nd = 1\n");
    let arrays_deep = write(&dir, "arrays.toml", "x = [[[1]]]\n");
    let json_deep = write(&dir, "deep.json", r#"{"a": {"b": {"c": {"d": 1}}}}"#);
    let json_ok = write(&dir, "ok.json", r#"{"a": {"b": 1}}"#);
    assert!(Config::load(&ConfigSources::new().file(&toml_ok).max_depth(3)).is_ok());
    assert!(Config::load(&ConfigSources::new().file(&json_ok).max_depth(3)).is_ok());
    for deep in [&toml_deep, &arrays_deep, &json_deep] {
        let error =
            Config::load(&ConfigSources::new().file(deep).max_depth(3)).expect_err("too deep");
        assert!(
            matches!(error, ConfigError::TooDeep { limit: 3, .. }),
            "{error:?}"
        );
        assert!(scrub(&dir, &error).starts_with("configuration file `<dir>/"));
        assert!(
            error
                .to_string()
                .ends_with("nests deeper than the limit of 3 levels")
        );
    }
}

#[test]
fn nesting_deeper_than_the_limit_is_refused_in_environment_names_and_set_keys() {
    let deep = "APP_A__B__C__D";
    let error = Config::load(
        &ConfigSources::new()
            .env_vars("APP_", [(deep, "x")])
            .max_depth(3),
    )
    .expect_err("too deep");
    assert_eq!(
        error.to_string(),
        "environment variable `APP_A__B__C__D` nests deeper than the limit of 3 levels"
    );
    assert!(
        Config::load(
            &ConfigSources::new()
                .env_vars("APP_", [("APP_A__B__C", "x")])
                .max_depth(3)
        )
        .is_ok()
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let dotenv = write(&dir, ".env", "APP_A__B__C__D=x\n");
    let error =
        Config::load(&ConfigSources::new().dotenv(&dotenv).max_depth(3)).expect_err("too deep");
    assert_eq!(
        scrub(&dir, &error),
        "`.env` entry `APP_A__B__C__D` in `<dir>/.env` nests deeper than the limit of 3 levels"
    );

    let error =
        Config::load(&ConfigSources::new().set("a.b.c.d", 1).max_depth(3)).expect_err("too deep");
    assert_eq!(
        error.to_string(),
        "`ConfigSources::set` key `a.b.c.d` nests deeper than the limit of 3 levels"
    );
    let error = Config::load(
        &ConfigSources::new()
            .set("a.b", json!({ "c": { "d": 1 } }))
            .max_depth(3),
    )
    .expect_err("too deep");
    assert!(
        matches!(error, ConfigError::TooDeep { limit: 3, .. }),
        "{error:?}"
    );
}

#[test]
fn the_default_limit_is_32_levels() {
    assert_eq!(ConfigSources::DEFAULT_MAX_DEPTH, 32);
    let name = |levels: usize| format!("APP_{}", vec!["a"; levels].join("__"));
    let at_limit = name(32);
    let over = name(33);
    assert!(
        Config::load(&ConfigSources::new().env_vars("APP_", [(at_limit.as_str(), "x")])).is_ok()
    );
    assert!(Config::load(&ConfigSources::new().env_vars("APP_", [(over.as_str(), "x")])).is_err());
}

#[test]
fn a_hostile_variable_name_is_an_error_not_a_stack_overflow() {
    let name = format!("APP_{}", "a__".repeat(200_000));
    let error = Config::load(&ConfigSources::new().env_vars("APP_", [(name.as_str(), "x")]))
        .expect_err("too deep");
    assert!(matches!(error, ConfigError::TooDeep { .. }), "{error:?}");
    // The message names the variable but is cut, not 600 KB long.
    assert!(error.to_string().len() < 300, "{}", error.to_string().len());

    let key = "a.".repeat(200_000);
    let error = Config::load(&ConfigSources::new().set(&key, 1)).expect_err("too deep");
    assert!(matches!(error, ConfigError::TooDeep { .. }), "{error:?}");
    assert!(error.to_string().len() < 300);
}

#[test]
fn the_depth_limit_is_clamped_to_what_the_stack_can_hold() {
    let name = format!(
        "APP_{}",
        vec!["a"; ConfigSources::HARD_MAX_DEPTH + 1].join("__")
    );
    let sources = ConfigSources::new()
        .env_vars("APP_", [(name.as_str(), "x")])
        .max_depth(usize::MAX);
    assert!(Config::load(&sources).is_err());
    // And a limit of zero is one: the root table alone.
    assert!(Config::load(&ConfigSources::new().set("a", 1).max_depth(0)).is_ok());
    assert!(Config::load(&ConfigSources::new().set("a.b", 1).max_depth(0)).is_err());
}

#[test]
fn deep_files_that_the_parsers_refuse_are_errors_too() {
    let dir = tempfile::tempdir().expect("tempdir");
    let toml = write(
        &dir,
        "app.toml",
        format!("a = {}1{}\n", "[".repeat(100_000), "]".repeat(100_000)),
    );
    assert!(Config::load(&ConfigSources::new().file(&toml).max_depth(128)).is_err());
    let json = write(
        &dir,
        "app.json",
        format!("{}1{}", r#"{"a":"#.repeat(100_000), "}".repeat(100_000)),
    );
    assert!(Config::load(&ConfigSources::new().file(&json).max_depth(128)).is_err());
}

// ---------------------------------------------------------------------------
// The environment prefix
// ---------------------------------------------------------------------------

#[test]
fn the_process_environment_source_maps_exactly_the_variables_under_the_prefix() {
    // `cargo test` exports a dozen `CARGO_*` variables, `CARGO_PKG_*` among
    // them, plus everything the machine has (`PATH`, ...).
    let under_prefix: BTreeSet<String> = std::env::vars()
        .filter_map(|(name, _)| name.strip_prefix("CARGO_PKG_").map(str::to_lowercase))
        .collect();
    assert!(under_prefix.contains("name"), "run under cargo");
    let config = Config::load(&ConfigSources::new().env_prefixed("CARGO_PKG_")).expect("load");
    let mapped: BTreeSet<String> = config
        .section::<serde_json::Map<String, Value>>("")
        .expect("tree")
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert_eq!(mapped, under_prefix);
}

#[test]
fn another_prefix_never_reads_the_default_one_and_the_reverse() {
    let vars = [
        ("APP_A", "default"),
        ("MYAPP_A", "mine"),
        ("MYAPP_B", "mine too"),
        ("PATH", "p"),
    ];
    let default = Config::load(&ConfigSources::new().env_vars("APP_", vars)).expect("load");
    let mine = Config::load(&ConfigSources::new().env_vars("MYAPP", vars)).expect("load");
    assert_eq!(default.redacted(), json!({ "a": "default" }));
    assert_eq!(mine.redacted(), json!({ "a": "mine", "b": "mine too" }));
}

#[test]
fn dotenv_entries_outside_the_app_prefix_never_reach_the_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dotenv = write(
        &dir,
        ".env",
        "DATABASE_URL=postgres://u:hunter2@h/d\nSECRET_KEY=hunter2\nAPP_OK=1\nPATH=/bin\n",
    );
    let config = Config::load(&ConfigSources::new().dotenv(&dotenv)).expect("load");
    assert_eq!(config.redacted(), json!({ "ok": "1" }));
    assert!(!format!("{config:?}").contains("hunter2"));
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

fn load(layer: Value) -> Config {
    Config::load(&ConfigSources::new().set("", layer)).expect("load")
}

#[test]
fn debug_and_redacted_mask_dsn_credentials_and_connection_strings() {
    let config = load(json!({
        "sentry": { "dsn": "https://abc123@o1.ingest.sentry.io/42" },
        "db": { "credentials": { "user": "u", "pass": "p" }, "connection_string": "Server=x;Password=hunter2" },
        "legacy": { "passwd": "hunter2", "PWD": "hunter2" },
        "host": "db.example.com",
    }));
    assert_eq!(
        config.redacted(),
        json!({
            "sentry": { "dsn": "[redacted]" },
            "db": { "credentials": "[redacted]", "connection_string": "[redacted]" },
            "legacy": { "passwd": "[redacted]", "PWD": "[redacted]" },
            "host": "db.example.com",
        })
    );
    let debug = format!("{config:?}");
    for secret in ["abc123", "hunter2"] {
        assert!(!debug.contains(secret), "{secret}: {debug}");
    }
}

#[test]
fn urls_with_credentials_are_masked_in_every_place_they_can_hide() {
    let config = load(json!({
        "db": { "url": "postgres://app:hunter2@db:5432/app?sslmode=require&password=hunter3" },
        "cache": { "urls": ["redis://:hunter4@a:6379/0", "redis://:hunter5@b:6379/0"] },
        "mirror": "primary=postgres://u:hunter6@a/d,replica=postgres://u:hunter7@b/d",
        "callback": "https://app.example.com/cb?code=1&access_token=hunter8#state=ok",
        "plain": "https://example.com/docs",
    }));
    let debug = format!("{config:?}");
    for secret in [
        "hunter2", "hunter3", "hunter4", "hunter5", "hunter6", "hunter7", "hunter8",
    ] {
        assert!(!debug.contains(secret), "{secret}: {debug}");
    }
    assert_eq!(
        config.redacted()["db"]["url"],
        "postgres://app:[redacted]@db:5432/app?sslmode=require&password=[redacted]"
    );
    assert_eq!(config.redacted()["plain"], "https://example.com/docs");
    assert_eq!(
        config.redacted()["callback"],
        "https://app.example.com/cb?code=1&access_token=[redacted]#state=ok"
    );
}

/// A value whose `Deserialize` rejects it and says what it was, the way
/// parsers of addresses and URLs do.
#[derive(Debug)]
struct Echo;

impl<'de> Deserialize<'de> for Echo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let text = String::deserialize(d)?;
        Err(D::Error::custom(format!("cannot use {text}")))
    }
}

#[test]
fn errors_scrub_values_a_deserializer_echoes_when_the_key_looks_secret() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Auth {
        api_key: Echo,
    }
    let config = load(json!({ "auth": { "api_key": "sk_live_0123456789" } }));
    let error = config.section::<Auth>("auth").expect_err("rejected");
    assert!(matches!(error, ConfigError::Section { .. }));
    assert_eq!(
        error.to_string(),
        "configuration section `auth` is invalid: cannot use [redacted] for key `api_key`"
    );
}

#[test]
fn errors_scrub_url_passwords_a_deserializer_echoes() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Hook {
        target: Echo,
    }
    let config = load(json!({ "hook": { "target": "https://svc:hunter2@example.com/x" } }));
    let text = config
        .section::<Hook>("hook")
        .expect_err("rejected")
        .to_string();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(
        text.contains("cannot use https://svc:[redacted]@example.com/x"),
        "{text}"
    );
}

#[test]
fn errors_about_a_number_under_a_secret_key_do_not_print_it() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Lock {
        pin_secret: u8,
        port: u8,
    }
    let secret = load(json!({ "pin_secret": 123456, "port": 1 }));
    let text = secret.section::<Lock>("").expect_err("range").to_string();
    assert!(!text.contains("123456"), "{text}");
    assert_eq!(
        text,
        "configuration section `` is invalid: invalid type: 64-bit integer [redacted], expected an unsigned 8 bit integer for key `pin_secret` in ConfigSources::set"
    );
    // A number under another key is shown: it is how you find the mistake.
    let open = load(json!({ "pin_secret": 1, "port": 123456 }));
    let text = open.section::<Lock>("").expect_err("range").to_string();
    assert!(text.contains("`123456`"), "{text}");
}

#[test]
fn a_secret_section_key_hides_numbers_in_everything_below_it() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Digits {
        first: u8,
    }
    let config = load(json!({ "secrets": { "first": 999 } }));
    let text = config
        .section::<Digits>("secrets")
        .expect_err("range")
        .to_string();
    assert!(!text.contains("999"), "{text}");
}

#[test]
fn file_errors_never_contain_the_text_that_failed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json = write(&dir, "app.json", r#"{"password": "hunter2", "oops" 1}"#);
    let error = Config::load(&ConfigSources::new().file(&json)).expect_err("invalid JSON");
    let text = scrub(&dir, &error);
    assert!(
        !text.contains("hunter2") && !text.contains("oops"),
        "{text}"
    );
    assert!(
        text.starts_with("invalid JSON in `<dir>/app.json` at line 1, column "),
        "{text}"
    );
    assert!(
        matches!(error, ConfigError::Json { line: 1, .. }),
        "{error:?}"
    );
}

#[test]
fn a_debug_print_of_any_error_has_no_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let toml = write(&dir, "app.toml", "password = \"hunter2\nmore = 1\n");
    let errors = [
        Config::load(&ConfigSources::new().file(&toml)).expect_err("toml"),
        Config::load(&ConfigSources::new().set("a.b.c", "hunter2").max_depth(2)).expect_err("deep"),
        load(json!({ "password": "hunter2" }))
            .section::<u8>("password")
            .expect_err("type"),
    ];
    for error in errors {
        assert!(!format!("{error:?}").contains("hunter2"), "{error:?}");
        assert!(!error.to_string().contains("hunter2"), "{error}");
    }
}
