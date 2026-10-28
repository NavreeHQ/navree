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
