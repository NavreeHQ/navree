//! Loading sources: `.env`, TOML files, environment variables and `set`.

use std::path::PathBuf;

use navree_config::{Config, ConfigSources};
use serde_json::json;
use tempfile::TempDir;

/// Writes `contents` to `name` inside `dir` and returns the path.
fn write(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

#[test]
fn later_sources_override_earlier() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let dotenv = write(
        &dir,
        ".env",
        "APP_NAME=from-dotenv\n\
         APP_DATABASE__URL=sqlite://dotenv.db\n\
         APP_DATABASE__POOL=2\n\
         APP_ONLY__DOTENV=yes\n",
    );
    let toml = write(
        &dir,
        "app.toml",
        "name = \"from-file\"\n\
         [database]\n\
         url = \"sqlite://file.db\"\n\
         [only]\n\
         file = true\n",
    );
    let env = [("APP_DATABASE__URL", "sqlite://env.db")];

    let config = Config::load(
        &ConfigSources::new()
            .dotenv(&dotenv)
            .file(&toml)
            .env_vars("APP_", env),
    )
    .expect("load");
    assert_eq!(config.get_string("name").as_deref(), Some("from-file"));
    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://env.db")
    );
    // Tables merge key by key: keys a later source does not set survive.
    assert_eq!(config.get_string("database.pool").as_deref(), Some("2"));
    assert_eq!(config.get_string("only.dotenv").as_deref(), Some("yes"));
    assert_eq!(config.get_string("only.file").as_deref(), Some("true"));

    // Precedence follows insertion order, not the kind of source.
    let reversed = Config::load(
        &ConfigSources::new()
            .env_vars("APP_", env)
            .file(&toml)
            .dotenv(&dotenv),
    )
    .expect("load");
    assert_eq!(reversed.get_string("name").as_deref(), Some("from-dotenv"));
    assert_eq!(
        reversed.get_string("database.url").as_deref(),
        Some("sqlite://dotenv.db")
    );
}

#[test]
fn set_values_win_whatever_the_order_and_merge_objects() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(
        &dir,
        "app.toml",
        "[database]\nurl = \"sqlite://file.db\"\npool = 5\n",
    );
    let config = Config::load(
        &ConfigSources::new()
            .set("database.url", "sqlite://set.db")
            .set("mail", json!({ "host": "smtp.test", "port": 2525 }))
            .set("retries", 1)
            .set("retries", 2)
            .file(&toml)
            .env_vars(
                "APP_",
                [
                    ("APP_DATABASE__URL", "sqlite://env.db"),
                    ("APP_MAIL__HOST", "smtp.env"),
                    ("APP_MAIL__FROM", "app@example.com"),
                ],
            ),
    )
    .expect("load");

    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://set.db")
    );
    assert_eq!(config.get_string("database.pool").as_deref(), Some("5"));
    assert_eq!(config.get_string("mail.host").as_deref(), Some("smtp.test"));
    assert_eq!(config.get_string("mail.port").as_deref(), Some("2525"));
    // An object merges into the table instead of replacing it.
    assert_eq!(
        config.get_string("mail.from").as_deref(),
        Some("app@example.com")
    );
    assert_eq!(config.get_string("retries").as_deref(), Some("2"));
}

#[test]
fn defaults_read_dotenv_and_app_toml_in_the_declared_order_then_app_env() {
    // `ConfigSources::DOTENV_RANKS_ABOVE_FILES` is the one switch for the
    // order of the two files; the real environment is always read last.
    let expected = if ConfigSources::DOTENV_RANKS_ABOVE_FILES {
        r#"ConfigSources { sources: [File("config/app.toml"), Dotenv(".env"), Env { prefix: "APP_" }], set: [] }"#
    } else {
        r#"ConfigSources { sources: [Dotenv(".env"), File("config/app.toml"), Env { prefix: "APP_" }], set: [] }"#
    };
    assert_eq!(format!("{:?}", ConfigSources::defaults()), expected);
    assert_eq!(
        format!("{:?}", ConfigSources::new().env()),
        r#"ConfigSources { sources: [Env { prefix: "APP_" }], set: [] }"#
    );
}

#[test]
fn defaults_in_reads_the_files_of_another_directory() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/app.toml"),
        "[mail]\nhost = \"smtp.toml\"\nport = 25\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".env"),
        "APP_MAIL__HOST=smtp.env\nAPP_KEY=k\n",
    )
    .unwrap();
    let config = Config::load(&ConfigSources::defaults_in(dir.path())).expect("load");
    // The same order as `defaults()`.
    let winner = if ConfigSources::DOTENV_RANKS_ABOVE_FILES {
        "smtp.env"
    } else {
        "smtp.toml"
    };
    assert_eq!(config.get_string("mail.host").as_deref(), Some(winner));
    assert_eq!(config.get_string("mail.port").as_deref(), Some("25"));
    assert_eq!(config.get_string("key").as_deref(), Some("k"));
}

#[test]
fn sources_debug_shows_keys_but_not_values() {
    let sources = ConfigSources::new()
        .env_vars("APP", [("APP_DATABASE__PASSWORD", "hunter2")])
        .set("mail.password", "hunter2");
    let debug = format!("{sources:?}");
    assert_eq!(
        debug,
        r#"ConfigSources { sources: [EnvVars { prefix: "APP_", names: ["APP_DATABASE__PASSWORD"] }], set: ["mail.password"] }"#
    );
}

#[test]
fn missing_files_are_skipped() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let config = Config::load(
        &ConfigSources::new()
            .dotenv(dir.path().join("missing.env"))
            .file(dir.path().join("missing.toml")),
    )
    .expect("missing files are not an error");
    assert_eq!(config.get_string("name"), None);
}

#[test]
fn dotenv_entries_use_the_app_mapping_and_leave_the_process_env_alone() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let dotenv = write(
        &dir,
        ".env",
        "APP_MAIL__HOST=smtp.example.com\n\
         NAVREE_CONFIG_TEST_UNPREFIXED_7F3A=ignored\n",
    );
    let config = Config::load(&ConfigSources::new().dotenv(&dotenv)).expect("load");

    assert_eq!(
        config.get_string("mail.host").as_deref(),
        Some("smtp.example.com")
    );
    assert_eq!(
        config.get_string("navree_config_test_unprefixed_7f3a"),
        None
    );
    assert!(std::env::var_os("APP_MAIL__HOST").is_none());
    assert!(std::env::var_os("NAVREE_CONFIG_TEST_UNPREFIXED_7F3A").is_none());
}

#[test]
fn file_errors_name_the_file_and_line_but_not_the_values() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir");
    let toml = write(
        &dir,
        "app.toml",
        "[database]\nurl = \"sqlite://app.db\"\npassword = \"hunter2\n",
    );
    let error = Config::load(&ConfigSources::new().file(&toml)).expect_err("invalid TOML");
    let text = error.to_string();
    assert!(text.contains("app.toml"), "{text}");
    assert!(text.contains("line 3"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");

    let dotenv = write(&dir, ".env", "APP_A=1\nAPP_PASSWORD=\"hunter2\n");
    let error = Config::load(&ConfigSources::new().dotenv(&dotenv)).expect_err("invalid .env");
    let text = error.to_string();
    assert!(text.contains(".env"), "{text}");
    assert!(text.contains("line 2"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
}

#[test]
fn env_maps_prefix_and_double_underscore() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_DATABASE__URL", "sqlite://app.db"),
            ("APP_LOG__LEVELS__NAVREE_QUEUE", "debug"),
            ("APP_NAME", "Navree"),
            ("app_lower__case", "windows keeps the case it was set with"),
            ("OTHER_NAME", "ignored"),
            ("APP_", "ignored: nothing after the prefix"),
            ("APP_BAD____KEY", "ignored: empty segment"),
        ],
    );
    let config = Config::load(&sources).expect("load");

    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://app.db")
    );
    assert_eq!(
        config.get_string("log.levels.navree_queue").as_deref(),
        Some("debug")
    );
    assert_eq!(config.get_string("name").as_deref(), Some("Navree"));
    assert_eq!(
        config.get_string("lower.case").as_deref(),
        Some("windows keeps the case it was set with")
    );
    assert_eq!(config.get_string("other_name"), None);
    assert_eq!(config.get_string("bad"), None);
}

#[test]
fn env_prefixed_reads_the_process_environment() {
    // `cargo test` exports CARGO_PKG_* to the test process; the prefix is
    // accepted with or without its trailing underscore.
    let config = Config::load(&ConfigSources::new().env_prefixed("CARGO_PKG")).expect("load");
    assert_eq!(config.get_string("name").as_deref(), Some("navree-config"));
    assert_eq!(
        config.get_string("version").as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}
