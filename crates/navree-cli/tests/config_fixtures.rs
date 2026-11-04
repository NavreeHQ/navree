//! Regenerates the rendered configuration fixtures of `navree-config`.
//!
//! `crates/navree-config/tests/fixtures/rendered/<case>/` holds the
//! `config/app.toml` and `.env` that `navree new` writes for each starter kit
//! and database, rendered with the defaults. `navree-config`'s tests load them
//! and compare the merged tree with the JSON recorded next to them. The files
//! are inputs frozen at the time they were rendered: this test only rewrites
//! them on request, so a template change never fails `navree-config`'s tests.
//!
//! ```text
//! cargo test -p navree-cli --test config_fixtures -- --ignored
//! ```
//!
//! Then record the trees again with `NAVREE_CONFIG_RECORD=1 cargo test -p
//! navree-config --test config fixtures` and review the diff.

use std::path::{Path, PathBuf};

use navree_cli::clock::Timestamp;
use navree_cli::kits::Kit;
use navree_cli::names::AppName;
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, app_files};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("navree-config")
        .join("tests")
        .join("fixtures")
        .join("rendered")
}

fn new_app(kit: Option<Kit>, database: Database) -> NewApp {
    NewApp {
        name: AppName::parse("acme-cloud").unwrap(),
        ui: Ui::None,
        kit,
        database,
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    }
}

/// Every starter kit and database `navree new` can write.
const CASES: [(Option<Kit>, Database); 8] = [
    (Some(Kit::Saas), Database::Timescaledb),
    (Some(Kit::Api), Database::Timescaledb),
    (Some(Kit::Api), Database::Postgres),
    (Some(Kit::Micro), Database::Timescaledb),
    (None, Database::Timescaledb),
    (None, Database::Postgres),
    (None, Database::Mysql),
    (None, Database::Sqlite),
];

/// The config keys that the `APP_` entries of a `.env` file set: the lines
/// `KEY=value`, comments and blank lines skipped. `APP_DATABASE__URL` sets
/// `database.url`; `APP_URL` is the short name of `app.url`.
fn dotenv_keys(text: &str) -> Vec<String> {
    const SHORT: [&str; 6] = ["name", "url", "version", "env", "key", "previous_keys"];
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .filter_map(|(name, _)| name.trim().strip_prefix("APP_").map(str::to_lowercase))
        .map(|name| name.split("__").collect::<Vec<_>>().join("."))
        .map(|key| {
            if SHORT.contains(&key.as_str()) {
                format!("app.{key}")
            } else {
                key
            }
        })
        .collect()
}

/// Every key a TOML table sets, as dotted paths (a list is one key).
fn toml_keys(table: &toml::Table, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => toml_keys(inner, &path, out),
            _ => out.push(path),
        }
    }
}

/// CONFIG-002: a config file ranks above `.env`, so a key written in both
/// would ignore the `.env` copy. The files `navree new` writes set no key in
/// both.
#[test]
fn no_generated_key_is_set_in_both_dotenv_and_config_app_toml() {
    for (kit, database) in CASES {
        let name = format!("{kit:?} on {database:?}");
        let files = app_files(&new_app(kit, database), Timestamp::from_unix(1_792_938_605))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let dotenv = files
            .get(".env")
            .unwrap_or_else(|| panic!("{name}: no .env"));
        let toml_text = files
            .get("config/app.toml")
            .unwrap_or_else(|| panic!("{name}: no config/app.toml"));
        let table: toml::Table = toml_text
            .parse()
            .unwrap_or_else(|e| panic!("{name}: config/app.toml: {e}"));
        let mut file_keys = Vec::new();
        toml_keys(&table, "", &mut file_keys);
        let twice: Vec<String> = dotenv_keys(dotenv)
            .into_iter()
            .filter(|key| file_keys.contains(key))
            .collect();
        assert!(
            twice.is_empty(),
            "{name}: set in both .env and config/app.toml: {twice:?}"
        );
    }
}

/// The helper itself: aliases and nesting.
#[test]
fn dotenv_entries_map_to_config_keys() {
    let text = "# comment\n\nAPP_KEY=\nAPP_DATABASE__URL=x\nexport_not=1\nAPP_LOG__LEVELS__NAVREE_QUEUE=debug\nOTHER=1\n";
    assert_eq!(
        dotenv_keys(text),
        ["app.key", "database.url", "log.levels.navree_queue"]
    );
}

/// The minimal app keeps only what its config file leaves out in `.env`.
#[test]
fn the_minimal_app_env_keeps_the_key_and_the_sender() {
    for database in [
        Database::Timescaledb,
        Database::Postgres,
        Database::Mysql,
        Database::Sqlite,
    ] {
        let files = app_files(
            &new_app(None, database),
            Timestamp::from_unix(1_792_938_605),
        )
        .expect("files");
        let mut keys = dotenv_keys(files.get(".env").expect(".env"));
        keys.sort();
        assert_eq!(keys, ["app.key", "mail.from"], "{database:?}");
    }
}

#[test]
#[ignore = "rewrites crates/navree-config/tests/fixtures/rendered"]
fn render_config_fixtures() {
    let now = Timestamp::from_unix(1_792_938_605);
    let cases = [
        ("kit-saas", Some(Kit::Saas), Database::Timescaledb),
        ("kit-api", Some(Kit::Api), Database::Timescaledb),
        ("kit-api-postgres", Some(Kit::Api), Database::Postgres),
        ("kit-micro", Some(Kit::Micro), Database::Timescaledb),
        ("app-timescaledb", None, Database::Timescaledb),
        ("app-postgres", None, Database::Postgres),
        ("app-mysql", None, Database::Mysql),
        ("app-sqlite", None, Database::Sqlite),
    ];
    for (name, kit, database) in cases {
        let files = app_files(&new_app(kit, database), now).unwrap();
        let dir = fixtures().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        for path in ["config/app.toml", ".env"] {
            let contents = files
                .get(path)
                .unwrap_or_else(|| panic!("{name}: {path} is not generated"));
            let target = dir.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, contents).unwrap();
        }
    }
    // The SaaS kit's front end reads its own `.env.local` (no `APP_` entries).
    let files = app_files(&new_app(Some(Kit::Saas), Database::Timescaledb), now).unwrap();
    let web = fixtures().join("kit-saas-web");
    let _ = std::fs::remove_dir_all(&web);
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(
        web.join(".env"),
        files.get("web/.env.local").expect("web/.env.local"),
    )
    .unwrap();
}
