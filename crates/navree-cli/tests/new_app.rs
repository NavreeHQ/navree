//! `navree new`: the app skeleton.

use std::path::{Path, PathBuf};

use navree_cli::clock::Timestamp;
use navree_cli::names::AppName;
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app, created_message};
use navree_dock::{DockFile, ServiceOptions};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn app(name: &str, ui: Ui) -> NewApp {
    app_on(name, ui, Database::default())
}

fn app_on(name: &str, ui: Ui, database: Database) -> NewApp {
    NewApp {
        name: AppName::parse(name).unwrap(),
        ui,
        database,
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    }
}

fn read(root: &Path, file: &str) -> String {
    std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// `database.url` of the generated config/app.toml.
fn configured_url(root: &Path) -> String {
    let config: toml::Table = read(root, "config/app.toml").parse().unwrap();
    config["database"]["url"].as_str().unwrap().to_owned()
}

/// The `features` of the generated `navree` dependency.
fn navree_features(root: &Path) -> Vec<String> {
    let manifest: toml::Table = read(root, "Cargo.toml").parse().unwrap();
    manifest["dependencies"]["navree"]
        .get("features")
        .and_then(toml::Value::as_array)
        .map(|features| {
            features
                .iter()
                .map(|f| f.as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Every file under `root`, relative, with `/` separators, sorted.
fn tree(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                let parts: Vec<_> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn new_creates_expected_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("blog", Ui::None), now()).unwrap();
    assert_eq!(root, dir.path().join("blog"));
    assert_eq!(
        tree(&root),
        [
            ".env",
            ".env.example",
            ".gitignore",
            "AGENTS.md",
            "CLAUDE.md",
            "Cargo.toml",
            "README.md",
            "config/app.toml",
            "dock.toml",
            "src/abilities.rs",
            "src/app.rs",
            "src/lib.rs",
            "src/main.rs",
            "src/modules/mod.rs",
            "src/modules/users/controller.rs",
            "src/modules/users/dto.rs",
            "src/modules/users/entity.rs",
            "src/modules/users/migrations/m20261025_143005_create_user_profiles_table.rs",
            "src/modules/users/migrations/mod.rs",
            "src/modules/users/mod.rs",
            "src/modules/users/service.rs",
            "src/modules/users/tests/mod.rs",
            "storage/.gitignore",
            "tests/app.rs",
        ]
    );
}

#[test]
fn cargo_manifest_names_the_package_and_the_navree_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("my-shop", Ui::None), now()).unwrap();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let parsed: toml::Table = manifest.parse().expect("Cargo.toml is valid TOML");
    assert_eq!(parsed["package"]["name"].as_str(), Some("my-shop"));
    assert_eq!(
        parsed["dependencies"]["navree"]["path"].as_str(),
        Some("/opt/navree/crates/navree")
    );
    assert_eq!(navree_features(&root), ["postgres"]);
    assert!(parsed.contains_key("workspace"), "a standalone workspace");
    let main = std::fs::read_to_string(root.join("src").join("main.rs")).unwrap();
    assert!(
        main.contains("my_shop::app(ConfigSources::defaults()).run()"),
        "{main}"
    );
    let env = std::fs::read_to_string(root.join(".env.example")).unwrap();
    assert!(
        env.contains("APP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/my_shop"),
        "{env}"
    );
    assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), env);
}

/// TimescaleDB is the default database: a PostgreSQL URL, the `postgres`
/// driver, and a dock.toml whose `postgres` service runs TimescaleDB and
/// gives the app the very URL config/app.toml names.
#[test]
fn timescaledb_is_the_default_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("my-shop", Ui::None), now()).unwrap();
    let url = "postgres://navree:navree@127.0.0.1:5432/my_shop";
    assert_eq!(configured_url(&root), url);
    let dock = DockFile::parse(&read(&root, "dock.toml"), "my-shop").unwrap();
    assert_eq!(dock.services.len(), 1);
    let postgres = &dock.services[0];
    assert_eq!(postgres.image, "timescale/timescaledb:2.31.0-pg17");
    assert!(matches!(
        postgres.options,
        ServiceOptions::Postgres {
            timescaledb: true,
            ..
        }
    ));
    assert_eq!(
        dock.env(),
        [("APP_DATABASE__URL".to_owned(), url.to_owned())]
    );
    let config = read(&root, "config/app.toml");
    assert!(config.contains("[schema]\n"), "{config}");
    assert!(config.contains("timescaledb = \"auto\""), "{config}");
    let readme = read(&root, "README.md");
    assert!(readme.contains("navree dock up"), "{readme}");
    let agents = read(&root, "AGENTS.md");
    assert!(agents.contains("TimescaleDB"), "{agents}");
    assert!(agents.contains("navree dock up"), "{agents}");
}

#[test]
fn plain_postgres_mysql_and_sqlite_are_options() {
    let dir = tempfile::tempdir().unwrap();
    let postgres = create_app(
        dir.path(),
        &app_on("pg-shop", Ui::None, Database::Postgres),
        now(),
    )
    .unwrap();
    assert_eq!(
        configured_url(&postgres),
        "postgres://navree:navree@127.0.0.1:5432/pg_shop"
    );
    assert_eq!(navree_features(&postgres), ["postgres"]);
    let dock = DockFile::parse(&read(&postgres, "dock.toml"), "pg-shop").unwrap();
    assert_eq!(dock.services[0].image, "postgres:17");
    assert!(!read(&postgres, "config/app.toml").contains("[schema]"));

    let mysql = create_app(
        dir.path(),
        &app_on("my-shop", Ui::None, Database::Mysql),
        now(),
    )
    .unwrap();
    assert_eq!(
        configured_url(&mysql),
        "mysql://navree:navree@127.0.0.1:3306/my_shop"
    );
    assert_eq!(navree_features(&mysql), ["mysql"]);
    assert!(
        !mysql.join("dock.toml").exists(),
        "Dock runs no MySQL server"
    );
    let env = read(&mysql, ".env.example");
    assert!(
        env.contains("APP_DATABASE__URL=mysql://navree:navree@127.0.0.1:3306/my_shop"),
        "{env}"
    );

    let sqlite = create_app(
        dir.path(),
        &app_on("lite", Ui::None, Database::Sqlite),
        now(),
    )
    .unwrap();
    assert_eq!(configured_url(&sqlite), "sqlite://storage/lite.db?mode=rwc");
    assert!(navree_features(&sqlite).is_empty());
    let manifest: toml::Table = read(&sqlite, "Cargo.toml").parse().unwrap();
    assert_eq!(
        manifest["dependencies"]["navree"]["path"].as_str(),
        Some("/opt/navree/crates/navree")
    );
    assert!(!sqlite.join("dock.toml").exists());
    let readme = read(&sqlite, "README.md");
    assert!(!readme.contains("navree dock up"), "{readme}");
}

#[test]
fn a_published_navree_takes_the_driver_feature_too() {
    let source = NavreeSource::Version("0.1.0".to_owned());
    assert_eq!(
        source.dependency_with_features(&["postgres"]),
        r#"{ version = "0.1.0", features = ["postgres"] }"#
    );
    assert_eq!(source.dependency_with_features(&[]), r#""0.1.0""#);
    let path = NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree"));
    assert_eq!(
        path.dependency_with_features(&["mysql"]),
        r#"{ path = "/opt/navree/crates/navree", features = ["mysql"] }"#
    );
    assert_eq!(Database::default(), Database::Timescaledb);
    assert_eq!(Database::Timescaledb.feature(), Some("postgres"));
    assert_eq!(Database::Postgres.feature(), Some("postgres"));
    assert_eq!(Database::Mysql.feature(), Some("mysql"));
    assert_eq!(Database::Sqlite.feature(), None);
}

/// The steps `navree new` prints: start the database with Dock first when
/// Dock runs it.
#[test]
fn the_next_steps_start_the_database_first() {
    let dir = tempfile::tempdir().unwrap();
    let timescale = app("my-shop", Ui::None);
    let root = create_app(dir.path(), &timescale, now()).unwrap();
    let message = created_message(&timescale, &root);
    let steps: Vec<&str> = message
        .lines()
        .skip_while(|line| *line != "Next steps:")
        .skip(1)
        .collect();
    assert_eq!(
        steps,
        [
            "  cd my-shop",
            "  navree dock up             # TimescaleDB on 127.0.0.1:5432",
            "  navree key:generate        # then set APP_KEY in .env",
            "  navree dev                 # http://127.0.0.1:8080/api/docs",
        ]
    );
    assert!(message.starts_with("Created my-shop in "), "{message}");
    assert!(
        message.contains("It depends on navree at /opt/navree/crates/navree."),
        "{message}"
    );

    let lite = app_on("lite", Ui::Next, Database::Sqlite);
    let message = created_message(&lite, &dir.path().join("lite"));
    assert!(!message.contains("dock"), "{message}");
    assert!(
        message.contains("  npm --prefix web install   # the front end; `navree dev` starts it"),
        "{message}"
    );

    let mysql = app_on("my-shop", Ui::None, Database::Mysql);
    let message = created_message(&mysql, &dir.path().join("my-shop"));
    assert!(!message.contains("dock"), "{message}");
    assert!(
        message
            .contains("a MySQL or MariaDB server at mysql://navree:navree@127.0.0.1:3306/my_shop"),
        "{message}"
    );
}

/// The generated files explain the database they were made for.
#[test]
fn the_generated_files_describe_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("my-shop", Ui::None), now()).unwrap();
    let dock = read(&root, "dock.toml");
    assert!(
        dock.starts_with("# Local services for my-shop. `navree dock up` starts them"),
        "{dock}"
    );
    assert!(dock.contains("PostgreSQL with TimescaleDB"), "{dock}");
    assert!(
        dock.ends_with("[services.postgres]\n"),
        "an empty table: TimescaleDB, Dock's default: {dock}"
    );
    let env = read(&root, ".env.example");
    assert!(
        env.contains("# TimescaleDB, run by `navree dock up` (see dock.toml).\nAPP_DATABASE__URL="),
        "{env}"
    );
    let readme = read(&root, "README.md");
    assert!(
        readme.contains("navree dock up               # the database on 127.0.0.1:5432"),
        "{readme}"
    );
    assert!(
        readme.contains("dock.toml              the local services"),
        "{readme}"
    );
    let agents = read(&root, "AGENTS.md");
    assert!(agents.contains("storage/               runtime data: logs, uploads"));
    assert!(
        agents.contains("schema.create_hypertable(\"events\""),
        "{agents}"
    );

    let pg = create_app(
        dir.path(),
        &app_on("pg-shop", Ui::None, Database::Postgres),
        now(),
    )
    .unwrap();
    let dock = read(&pg, "dock.toml");
    assert!(
        dock.contains("[services.postgres]\ntimescaledb = false\n"),
        "{dock}"
    );
    assert!(!read(&pg, "AGENTS.md").contains("hypertable"));

    let lite = create_app(
        dir.path(),
        &app_on("lite", Ui::None, Database::Sqlite),
        now(),
    )
    .unwrap();
    let agents = read(&lite, "AGENTS.md");
    assert!(
        agents.contains("storage/               runtime data: the SQLite database, logs, uploads"),
        "{agents}"
    );
    assert!(!agents.contains("dock.toml"), "{agents}");
    let config = read(&lite, "config/app.toml");
    assert!(
        config.contains("[database]\nurl = \"sqlite://storage/lite.db?mode=rwc\""),
        "{config}"
    );
}

/// Coding agents get the project's conventions: AGENTS.md, and a CLAUDE.md
/// that imports it rather than repeating it.
#[test]
fn agents_md_describes_the_conventions_and_claude_md_imports_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("my-shop", Ui::None), now()).unwrap();
    let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(agents.starts_with("# AGENTS.md\n"), "{agents}");
    assert!(
        agents.contains("working on My shop, a Navree app"),
        "{agents}"
    );
    for expected in [
        // layout and commands
        "src/modules/<name>/",
        "cargo test",
        "cargo clippy --all-targets -- -D warnings",
        "navree dev",
        // the generators
        "navree make:module posts --crud",
        "navree make:migration posts create_tags_table",
        "navree make:job posts publish_post",
        "navree make:test posts publishing",
        // deny by default and the error shape
        "#[authenticated]",
        r#"{ "statusCode": 404, "message": "Not Found", "error": "Not Found", "code": "not_found" }"#,
        ".with_code(",
        // where the docs and skills are
        "https://navree.com/docs/",
        "npx skills add NavreeHQ/navree",
    ] {
        assert!(agents.contains(expected), "AGENTS.md lacks `{expected}`");
    }
    assert!(!agents.contains("web/ "), "no front end without --ui next");

    let claude = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert_eq!(claude.lines().next(), Some("@AGENTS.md"), "{claude}");
    assert!(
        claude.lines().count() <= 3,
        "CLAUDE.md only points: {claude}"
    );

    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("blog", Ui::Next), now()).unwrap();
    let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("web/                   the Next.js front end"),
        "{agents}"
    );
}

#[test]
fn a_published_navree_is_a_version_requirement() {
    let source = NavreeSource::Version("0.1.0".to_owned());
    assert_eq!(source.dependency(), r#""0.1.0""#);
    let windows = NavreeSource::Path(PathBuf::from(r"C:\src\navree\crates\navree"));
    assert_eq!(
        windows.dependency(),
        r#"{ path = "C:/src/navree/crates/navree" }"#
    );
}

/// `canonicalize()` on Windows gives verbatim paths (`\\?\C:\..`), which
/// cargo rejects in a `path` dependency ("invalid path url").
#[test]
fn verbatim_windows_paths_become_plain_paths() {
    for (path, expected) in [
        (
            r"\\?\C:\src\navree\crates\navree",
            r#"{ path = "C:/src/navree/crates/navree" }"#,
        ),
        (
            r"\\?\UNC\server\share\navree\crates\navree",
            r#"{ path = "//server/share/navree/crates/navree" }"#,
        ),
    ] {
        assert_eq!(
            NavreeSource::Path(PathBuf::from(path)).dependency(),
            expected
        );
    }
}

#[test]
fn navree_path_accepts_the_repository_or_the_facade_crate() {
    let repo = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let facade = repo.join("crates").join("navree");
    let NavreeSource::Path(from_repo) = NavreeSource::resolve(Some(&repo)).unwrap() else {
        panic!("expected a path source");
    };
    assert!(
        from_repo.ends_with(Path::new("crates").join("navree")),
        "{}",
        from_repo.display()
    );
    let NavreeSource::Path(from_facade) = NavreeSource::resolve(Some(&facade)).unwrap() else {
        panic!("expected a path source");
    };
    assert_eq!(from_facade, from_repo);

    let elsewhere = tempfile::tempdir().unwrap();
    let error = NavreeSource::resolve(Some(elsewhere.path())).unwrap_err();
    assert!(format!("{error:#}").contains("--navree-path"), "{error:#}");

    // Without the flag: the checkout this binary was built from.
    assert!(matches!(
        NavreeSource::resolve(None).unwrap(),
        NavreeSource::Path(_)
    ));
}

#[test]
fn ui_next_adds_the_web_front_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("blog", Ui::Next), now()).unwrap();
    let web: Vec<String> = tree(&root)
        .into_iter()
        .filter(|p| p.starts_with("web/"))
        .collect();
    assert_eq!(
        web,
        [
            "web/.env.example",
            "web/README.md",
            "web/app/api/[...path]/route.ts",
            "web/app/api/auth/login/route.ts",
            "web/app/api/auth/logout/route.ts",
            "web/app/layout.tsx",
            "web/app/login/page.tsx",
            "web/app/page.tsx",
            "web/lib/bff.ts",
            "web/next.config.ts",
            "web/package.json",
            "web/tsconfig.json",
        ]
    );
    let package: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("web").join("package.json")).unwrap(),
    )
    .expect("package.json is valid JSON");
    assert_eq!(package["name"], "blog-web");
    let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/web/node_modules/"), "{gitignore}");
}

#[test]
fn refuses_a_directory_that_is_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("blog")).unwrap();
    std::fs::write(dir.path().join("blog").join("notes.txt"), "keep me").unwrap();
    let error = create_app(dir.path(), &app("blog", Ui::None), now()).unwrap_err();
    assert!(error.to_string().contains("is not empty"), "{error}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("blog").join("notes.txt")).unwrap(),
        "keep me"
    );
}

#[test]
fn generated_files_are_free_of_template_syntax() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("blog", Ui::Next), now()).unwrap();
    for file in tree(&root) {
        let contents = std::fs::read_to_string(root.join(&file)).unwrap();
        for marker in ["{%", "%}", "{#"] {
            assert!(!contents.contains(marker), "{file} contains `{marker}`");
        }
        if file.ends_with(".rs") || file.ends_with(".toml") {
            assert!(!contents.contains("{{"), "{file} contains `{{{{`");
        }
        assert!(!contents.contains('\r'), "{file} uses LF line endings");
    }
}
