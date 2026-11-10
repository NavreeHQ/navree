//! A key that `.env` and a config file both set with different
//! values. One of the two is used and the other has no effect, and the boot
//! says so, naming the key and both sources. `.env` ranks above the config
//! file in `ConfigSources::defaults`; the loader also reads them the other way
//! round when the sources are listed that way, and these tests cover both
//! orders.

use std::path::PathBuf;

use navree_config::{Config, ConfigSources, Shadowed, ValueOrigin};
use tempfile::TempDir;

fn dir() -> TempDir {
    tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("tempdir")
}

fn write(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

/// `.env` is read first and the file after it: the file wins.
fn file_over_dotenv(dir: &TempDir, env: &str, file: &str) -> Config {
    let env = write(dir, ".env", env);
    let file = write(dir, "app.toml", file);
    Config::load(&ConfigSources::new().dotenv(env).file(file)).expect("load")
}

/// The file is read first and `.env` after it: `.env` wins.
fn dotenv_over_file(dir: &TempDir, env: &str, file: &str) -> Config {
    let env = write(dir, ".env", env);
    let file = write(dir, "app.toml", file);
    Config::load(&ConfigSources::new().file(file).dotenv(env)).expect("load")
}

fn dotenv_origin(dir: &TempDir, line: usize, variable: &str) -> ValueOrigin {
    ValueOrigin::DotEnv {
        file: dir.path().join(".env").display().to_string(),
        line,
        variable: variable.to_owned(),
    }
}

fn file_origin(dir: &TempDir) -> ValueOrigin {
    ValueOrigin::File {
        file: dir.path().join("app.toml").display().to_string(),
    }
}

const ENV: &str = "APP_DATABASE__URL=sqlite://from-dotenv.db\n";
const FILE: &str = "[database]\nurl = \"sqlite://from-file.db\"\n";

// ---------------------------------------------------------------------------
// The two orders
// ---------------------------------------------------------------------------

#[test]
fn dotenv_wins_when_it_is_read_after_the_config_file() {
    let dir = dir();
    let config = dotenv_over_file(&dir, ENV, FILE);
    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://from-dotenv.db")
    );
    assert!(config.origin("database.url").unwrap().is_dotenv());
    let shadows = config.shadowed();
    assert_eq!(shadows.len(), 1, "{shadows:?}");
    assert_eq!(shadows[0].key, "database.url");
    assert_eq!(
        shadows[0].winner,
        dotenv_origin(&dir, 1, "APP_DATABASE__URL")
    );
    assert_eq!(shadows[0].hidden, file_origin(&dir));
}

#[test]
fn the_config_file_wins_when_it_is_read_after_dotenv() {
    let dir = dir();
    let config = file_over_dotenv(&dir, ENV, FILE);
    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://from-file.db")
    );
    assert!(config.origin("database.url").unwrap().is_file());
    let shadows = config.shadowed();
    assert_eq!(shadows.len(), 1, "{shadows:?}");
    assert_eq!(shadows[0].winner, file_origin(&dir));
    assert_eq!(
        shadows[0].hidden,
        dotenv_origin(&dir, 1, "APP_DATABASE__URL")
    );
}

#[test]
fn the_default_sources_rank_dotenv_above_the_config_file() {
    // `ConfigSources::DOTENV_RANKS_ABOVE_FILES` is the one switch; this test
    // pins what it is set to.
    const { assert!(ConfigSources::DOTENV_RANKS_ABOVE_FILES) };
    let dir = dir();
    std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
    std::fs::write(dir.path().join(".env"), ENV).expect("write");
    std::fs::write(dir.path().join("config/app.toml"), FILE).expect("write");
    let config = Config::load(&ConfigSources::defaults_in(dir.path())).expect("load");
    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://from-dotenv.db")
    );
    let shadows = config.shadowed();
    assert_eq!(shadows.len(), 1, "{shadows:?}");
    assert!(shadows[0].winner.is_dotenv() && shadows[0].hidden.is_file());
}

#[test]
fn the_real_environment_ranks_above_both() {
    let dir = dir();
    let env = write(&dir, ".env", ENV);
    let file = write(&dir, "app.toml", FILE);
    for sources in [
        ConfigSources::new().file(&file).dotenv(&env),
        ConfigSources::new().dotenv(&env).file(&file),
    ] {
        let config = Config::load(
            &sources.env_vars("APP_", [("APP_DATABASE__URL", "sqlite://from-process.db")]),
        )
        .expect("load");
        assert_eq!(
            config.get_string("database.url").as_deref(),
            Some("sqlite://from-process.db")
        );
        // The process environment decides, so what the two files say about
        // the key does not matter.
        assert!(config.shadowed().is_empty(), "{:?}", config.shadowed());
    }
}

// ---------------------------------------------------------------------------
// What is reported
// ---------------------------------------------------------------------------

#[test]
fn several_keys_are_reported_in_key_order() {
    let dir = dir();
    let env = "APP_KEY=base64:abc\nAPP_DATABASE__URL=sqlite://a.db\nAPP_LOG__LEVEL=info\n";
    let file = "[database]\nurl = \"sqlite://b.db\"\n\n[log]\nlevel = \"debug\"\n";
    for config in [
        dotenv_over_file(&dir, env, file),
        file_over_dotenv(&dir, env, file),
    ] {
        let keys: Vec<&str> = config.shadowed().iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["database.url", "log.level"]);
    }
}

#[test]
fn the_message_names_the_key_and_both_sources_and_no_value() {
    let dir = dir();
    let env = "APP_DATABASE__URL=postgres://user:hunter2@db/agra\n";
    let file = "[database]\nurl = \"sqlite://other-secret.db\"\n";

    let config = dotenv_over_file(&dir, env, file);
    let message = config.shadowed()[0].message();
    assert!(message.contains("`database.url`"), "{message}");
    assert!(message.contains("set in both places"), "{message}");
    assert!(message.contains("the `.env` value is used"), "{message}");
    assert!(message.contains(".env:1 (APP_DATABASE__URL)"), "{message}");
    assert!(message.contains("app.toml"), "{message}");

    let config = file_over_dotenv(&dir, env, file);
    let message = config.shadowed()[0].message();
    assert!(message.contains("`database.url`"), "{message}");
    assert!(message.contains("set in both places"), "{message}");
    assert!(
        message.contains("the config file value is used"),
        "{message}"
    );
    assert!(message.contains(".env:1 (APP_DATABASE__URL)"), "{message}");

    for config in [
        dotenv_over_file(&dir, env, file),
        file_over_dotenv(&dir, env, file),
    ] {
        let message = config.shadowed()[0].message();
        assert!(!message.contains("hunter2"), "{message}");
        assert!(!message.contains("other-secret"), "{message}");
        assert!(!message.contains("postgres://"), "{message}");
        assert!(!message.contains("later release"), "no promise: {message}");
    }
}

#[test]
fn equal_values_conflict_with_nothing() {
    let dir = dir();
    let env = "APP_HTTP__ADDR=127.0.0.1:8080\nAPP_QUEUE__WORKERS=4\nAPP_AUTH__REGISTRATION=true\n";
    let file =
        "[http]\naddr = \"127.0.0.1:8080\"\n[queue]\nworkers = 4\n[auth]\nregistration = true\n";
    assert!(dotenv_over_file(&dir, env, file).shadowed().is_empty());
    assert!(file_over_dotenv(&dir, env, file).shadowed().is_empty());
}

#[test]
fn keys_only_one_of_them_sets_are_not_reported() {
    let dir = dir();
    let env = "APP_KEY=base64:abc\nAPP_MAIL__FROM=\"Agra <hello@acme.test>\"\n";
    let file = "[http]\naddr = \"127.0.0.1:8080\"\n[mail]\ntransport = \"log\"\n";
    assert!(dotenv_over_file(&dir, env, file).shadowed().is_empty());
    assert!(file_over_dotenv(&dir, env, file).shadowed().is_empty());
}

#[test]
fn a_plain_value_against_a_table_conflicts_in_both_orders() {
    let dir = dir();
    let env = "APP_STORAGE=local\n";
    let file = "[storage.disks.local]\nroot = \"storage\"\n";

    // The file replaces the plain `.env` value with a table.
    let config = file_over_dotenv(&dir, env, file);
    let keys: Vec<&str> = config.shadowed().iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["storage"]);

    // `.env` replaces the file's table with a plain value.
    let config = dotenv_over_file(&dir, env, file);
    let keys: Vec<&str> = config.shadowed().iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["storage.disks.local.root"]);
}

#[test]
fn only_dotenv_against_a_file_is_reported() {
    // The real environment against a file, and a file against a file, are
    // ordinary layering.
    let dir = dir();
    let file = write(&dir, "app.toml", "[log]\nlevel = \"debug\"\n");
    let config = Config::load(
        &ConfigSources::new()
            .file(file)
            .env_vars("APP_", [("APP_LOG__LEVEL", "error")]),
    )
    .expect("load");
    assert!(config.shadowed().is_empty());

    let first = write(&dir, "a.toml", "[log]\nlevel = \"info\"\n");
    let second = write(&dir, "b.toml", "[log]\nlevel = \"debug\"\n");
    let config = Config::load(&ConfigSources::new().file(first).file(second)).expect("load");
    assert!(config.shadowed().is_empty());
}

#[test]
fn the_short_name_of_an_app_setting_is_the_same_key_as_app_dot_name() {
    // `APP_URL` in `.env` is `app.url`; `[app] url` in the file is the same key.
    let env = "APP_URL=https://from-dotenv.test\nAPP_NAME=agra\n";
    let file = "[app]\nurl = \"https://from-file.test\"\nname = \"agra\"\n";

    let dir = dir();
    let config = file_over_dotenv(&dir, env, file);
    let shadows = config.shadowed();
    assert_eq!(shadows.len(), 1, "only `url` differs: {shadows:?}");
    assert_eq!(shadows[0].key, "app.url");
    assert!(
        matches!(&shadows[0].hidden, ValueOrigin::DotEnv { variable, line: 1, .. } if variable == "APP_URL"),
        "{:?}",
        shadows[0].hidden
    );
    assert_eq!(config.app_url().as_deref(), Some("https://from-file.test"));

    let dir = self::dir();
    let config = dotenv_over_file(&dir, env, file);
    let shadows = config.shadowed();
    assert_eq!(shadows.len(), 1, "{shadows:?}");
    assert_eq!(shadows[0].key, "app.url");
    assert!(shadows[0].winner.is_dotenv() && shadows[0].hidden.is_file());
    assert_eq!(
        config.app_url().as_deref(),
        Some("https://from-dotenv.test")
    );
}

#[test]
fn shadows_have_a_display_that_matches_their_message() {
    let shadow = Shadowed {
        key: "log.level".to_owned(),
        hidden: ValueOrigin::File {
            file: "config/app.toml".to_owned(),
        },
        winner: ValueOrigin::DotEnv {
            file: ".env".to_owned(),
            line: 7,
            variable: "APP_LOG__LEVEL".to_owned(),
        },
    };
    assert_eq!(shadow.to_string(), shadow.message());
    assert!(shadow.message().contains(".env:7 (APP_LOG__LEVEL)"));
    assert!(shadow.message().contains("config/app.toml"));
    assert!(shadow.message().contains("the `.env` value is used"));
}

#[test]
fn the_generated_apps_set_no_key_twice() {
    // The `.env` and `config/app.toml` that `navree new` writes
    // (frozen under tests/fixtures/rendered) never disagree, whichever of the
    // two ranks higher.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rendered");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).expect("fixtures") {
        let case = entry.expect("entry").path();
        if !case.join("config/app.toml").is_file() || !case.join(".env").is_file() {
            continue;
        }
        let (env, file) = (case.join(".env"), case.join("config/app.toml"));
        for sources in [
            ConfigSources::new().dotenv(&env).file(&file),
            ConfigSources::new().file(&file).dotenv(&env),
        ] {
            let config = Config::load(&sources).expect("fixture loads");
            assert!(
                config.shadowed().is_empty(),
                "{}: {:?}",
                case.display(),
                config.shadowed()
            );
        }
        checked += 1;
    }
    assert!(checked >= 8, "rendered fixtures are missing: {checked}");
}
