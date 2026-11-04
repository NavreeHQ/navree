//! D01 (CONFIG-003): a config file that ranks above `.env` hides its values,
//! and the boot says so, naming the key and both sources. Where the file
//! ranks below `.env` nothing is hidden and there is nothing to say. The
//! order of the default sources is one constant,
//! `ConfigSources::DOTENV_RANKS_ABOVE_FILES`; these tests cover both orders.

use navree_config::{Config, ConfigSources, Shadowed, ValueOrigin};
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

fn load(dir: &TempDir, env: &str, file: &str) -> Config {
    let env = write(dir, ".env", env);
    let file = write(dir, "app.toml", file);
    Config::load(&ConfigSources::new().dotenv(env).file(file)).expect("load")
}

#[test]
fn the_config_file_wins_when_it_is_read_after_dotenv() {
    // Order 1: `.env`, then the file. The later source wins, and the `.env`
    // value is the one that is hidden.
    let dir = dir();
    let config = load(
        &dir,
        "APP_DATABASE__URL=sqlite://from-dotenv.db\n",
        "[database]\nurl = \"sqlite://from-file.db\"\n",
    );
    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://from-file.db")
    );
}

#[test]
fn dotenv_wins_when_it_is_read_after_the_config_file_and_nothing_is_hidden() {
    // Order 2: the file, then `.env`. The same two files, the other winner,
    // and no warning: `.env` hiding a file value is the order people expect.
    let dir = dir();
    let env = write(&dir, ".env", "APP_DATABASE__URL=sqlite://from-dotenv.db\n");
    let file = write(
        &dir,
        "app.toml",
        "[database]\nurl = \"sqlite://from-file.db\"\n",
    );
    let config = Config::load(&ConfigSources::new().file(file).dotenv(env)).expect("load");
    assert_eq!(
        config.get_string("database.url").as_deref(),
        Some("sqlite://from-dotenv.db")
    );
    assert!(config.shadowed().is_empty(), "{:?}", config.shadowed());
    assert!(config.origin("database.url").unwrap().is_dotenv());
}

#[test]
fn the_default_sources_follow_the_declared_order() {
    // `ConfigSources::DOTENV_RANKS_ABOVE_FILES` decides; flipping it flips
    // the winner of `defaults_in` and removes the warning.
    let dir = dir();
    std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
    std::fs::write(
        dir.path().join(".env"),
        "APP_DATABASE__URL=sqlite://from-dotenv.db\n",
    )
    .expect("write");
    std::fs::write(
        dir.path().join("config/app.toml"),
        "[database]\nurl = \"sqlite://from-file.db\"\n",
    )
    .expect("write");
    let config = Config::load(&ConfigSources::defaults_in(dir.path())).expect("load");
    if ConfigSources::DOTENV_RANKS_ABOVE_FILES {
        assert_eq!(
            config.get_string("database.url").as_deref(),
            Some("sqlite://from-dotenv.db")
        );
        assert!(config.shadowed().is_empty());
    } else {
        assert_eq!(
            config.get_string("database.url").as_deref(),
            Some("sqlite://from-file.db")
        );
        assert_eq!(config.shadowed().len(), 1);
    }
}

#[test]
fn a_file_value_that_hides_a_dotenv_value_is_reported() {
    let dir = dir();
    let config = load(
        &dir,
        "APP_KEY=base64:abc\nAPP_DATABASE__URL=sqlite://from-dotenv.db\nAPP_LOG__LEVEL=info\n",
        "[database]\nurl = \"sqlite://from-file.db\"\n\n[log]\nlevel = \"debug\"\n",
    );
    let shadows = config.shadowed();
    let keys: Vec<&str> = shadows.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["database.url", "log.level"]);

    let first = &shadows[0];
    assert_eq!(
        first.hidden,
        ValueOrigin::DotEnv {
            file: dir.path().join(".env").display().to_string(),
            line: 2,
            variable: "APP_DATABASE__URL".to_owned(),
        }
    );
    assert_eq!(
        first.winner,
        ValueOrigin::File {
            file: dir.path().join("app.toml").display().to_string()
        }
    );
}

#[test]
fn the_warning_names_the_key_and_both_sources_and_no_value() {
    let dir = dir();
    let config = load(
        &dir,
        "APP_DATABASE__URL=postgres://user:hunter2@db/acme\n",
        "[database]\nurl = \"sqlite://other-secret.db\"\n",
    );
    let message = config.shadowed()[0].message();
    assert!(message.contains("`database.url`"), "{message}");
    assert!(message.contains("APP_DATABASE__URL"), "{message}");
    assert!(message.contains(".env:1"), "{message}");
    assert!(message.contains("app.toml"), "{message}");
    assert!(
        !message.contains("later release"),
        "no promise about releases: {message}"
    );
    assert!(!message.contains("hunter2"), "{message}");
    assert!(!message.contains("other-secret"), "{message}");
    assert!(!message.contains("postgres://"), "{message}");
}

#[test]
fn equal_values_hide_nothing() {
    let dir = dir();
    let config = load(
        &dir,
        "APP_HTTP__ADDR=127.0.0.1:8080\nAPP_QUEUE__WORKERS=4\nAPP_AUTH__REGISTRATION=true\n",
        "[http]\naddr = \"127.0.0.1:8080\"\n[queue]\nworkers = 4\n[auth]\nregistration = true\n",
    );
    assert!(config.shadowed().is_empty(), "{:?}", config.shadowed());
}

#[test]
fn keys_only_one_of_them_sets_are_not_reported() {
    let dir = dir();
    let config = load(
        &dir,
        "APP_KEY=base64:abc\nAPP_MAIL__FROM=\"Acme <hello@acme.test>\"\n",
        "[http]\naddr = \"127.0.0.1:8080\"\n[mail]\ntransport = \"log\"\n",
    );
    assert!(config.shadowed().is_empty());
}

#[test]
fn a_value_that_the_environment_overrides_again_is_not_reported() {
    // The real environment beats both: removing either leaves the value as
    // it is, so there is nothing to warn about.
    let dir = dir();
    let env = write(&dir, ".env", "APP_LOG__LEVEL=info\n");
    let file = write(&dir, "app.toml", "[log]\nlevel = \"debug\"\n");
    let config = Config::load(
        &ConfigSources::new()
            .dotenv(env)
            .file(file)
            .env_vars("APP_", [("APP_LOG__LEVEL", "error")]),
    )
    .expect("load");
    assert!(config.shadowed().is_empty());
}

#[test]
fn a_file_that_replaces_a_dotenv_value_with_a_table_hides_it() {
    let dir = dir();
    let config = load(
        &dir,
        "APP_STORAGE=local\n",
        "[storage.disks.local]\nroot = \"storage\"\n",
    );
    let keys: Vec<&str> = config.shadowed().iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["storage"]);
}

#[test]
fn only_dotenv_values_are_shadowed_not_environment_values() {
    // The real environment ranks above the file, so a file never hides it.
    let dir = dir();
    let file = write(&dir, "app.toml", "[log]\nlevel = \"debug\"\n");
    let config = Config::load(
        &ConfigSources::new()
            .file(file)
            .env_vars("APP_", [("APP_LOG__LEVEL", "error")]),
    )
    .expect("load");
    assert!(config.shadowed().is_empty());
}

#[test]
fn a_second_file_over_a_first_file_is_not_a_dotenv_shadow() {
    let dir = dir();
    let first = write(&dir, "a.toml", "[log]\nlevel = \"info\"\n");
    let second = write(&dir, "b.toml", "[log]\nlevel = \"debug\"\n");
    let config = Config::load(&ConfigSources::new().file(first).file(second)).expect("load");
    assert!(config.shadowed().is_empty());
}

#[test]
fn shadows_have_a_display_that_matches_their_message() {
    let shadow = Shadowed {
        key: "log.level".to_owned(),
        hidden: ValueOrigin::DotEnv {
            file: ".env".to_owned(),
            line: 7,
            variable: "APP_LOG__LEVEL".to_owned(),
        },
        winner: ValueOrigin::File {
            file: "config/app.toml".to_owned(),
        },
    };
    assert_eq!(shadow.to_string(), shadow.message());
    assert!(shadow.message().contains(".env:7 (APP_LOG__LEVEL)"));
    assert!(shadow.message().contains("config/app.toml"));
}

#[test]
fn the_generated_apps_do_not_hide_their_dotenv() {
    // CONFIG-002: the `.env` and `config/app.toml` that `navree new` writes
    // (frozen under tests/fixtures/rendered) hide nothing, in the order where
    // the file ranks above `.env`.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rendered");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).expect("fixtures") {
        let case = entry.expect("entry").path();
        if !case.join("config/app.toml").is_file() || !case.join(".env").is_file() {
            continue;
        }
        let config = Config::load(
            &ConfigSources::new()
                .dotenv(case.join(".env"))
                .file(case.join("config/app.toml")),
        )
        .expect("fixture loads");
        assert!(
            config.shadowed().is_empty(),
            "{}: {:?}",
            case.display(),
            config.shadowed()
        );
        checked += 1;
    }
    assert!(checked >= 8, "rendered fixtures are missing: {checked}");
}

#[test]
fn a_file_value_hides_the_short_dotenv_name_of_an_app_setting() {
    // `APP_URL` in `.env` is `app.url`; `[app] url` in the file hides it.
    let dir = dir();
    let config = load(
        &dir,
        "APP_URL=https://from-dotenv.test\nAPP_NAME=acme\n",
        "[app]\nurl = \"https://from-file.test\"\nname = \"acme\"\n",
    );
    let shadows = config.shadowed();
    assert_eq!(shadows.len(), 1, "{shadows:?}");
    assert_eq!(shadows[0].key, "app.url");
    assert!(
        matches!(&shadows[0].hidden, ValueOrigin::DotEnv { variable, line: 1, .. } if variable == "APP_URL"),
        "{:?}",
        shadows[0].hidden
    );
    assert_eq!(config.app_url().as_deref(), Some("https://from-file.test"));
}
