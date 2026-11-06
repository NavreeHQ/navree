//! `navree new`: the app skeleton.

use std::path::{Path, PathBuf};

use navree_cli::clock::Timestamp;
use navree_cli::names::AppName;
use navree_cli::new_app::{
    Database, NavreeSource, NewApp, Repository, Ui, create_app, created_message, init_repository,
    write_harbor_key,
};
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
        kit: None,
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

/// The `features` of the generated `navree` dev-dependency: the SQLite
/// driver the tests run on.
fn navree_test_features(root: &Path) -> Vec<String> {
    let manifest: toml::Table = read(root, "Cargo.toml").parse().unwrap();
    manifest["dev-dependencies"]["navree"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_owned())
        .collect()
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
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("my-shop", Ui::None), now()).unwrap();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let parsed: toml::Table = manifest.parse().expect("Cargo.toml is valid TOML");
    assert_eq!(parsed["package"]["name"].as_str(), Some("my-shop"));
    assert_eq!(
        parsed["dependencies"]["navree"]["path"].as_str(),
        Some("/opt/navree/crates/navree")
    );
    assert_eq!(navree_features(&root), ["postgres"]);
    // The tests run on SQLite files, so the dev-dependency turns the driver
    // on; the app's own build compiles only PostgreSQL.
    assert_eq!(navree_test_features(&root), ["sqlite"]);
    assert_eq!(
        parsed["dev-dependencies"]["navree"]["path"].as_str(),
        Some("/opt/navree/crates/navree")
    );
    assert!(parsed.contains_key("workspace"), "a standalone workspace");
    let main = std::fs::read_to_string(root.join("src").join("main.rs")).unwrap();
    assert!(
        main.contains("my_shop::app(ConfigSources::defaults()).run()"),
        "{main}"
    );
    // The URL is set in config/app.toml; `.env` does not repeat it, because
    // `.env` ranks above the config file and the TOML copy would do nothing
    // (CONFIG-002).
    let env = std::fs::read_to_string(root.join(".env.example")).unwrap();
    assert!(!env.contains("APP_DATABASE__URL"), "{env}");
    assert_eq!(
        configured_url(&root),
        "postgres://navree:navree@127.0.0.1:5432/my_shop"
    );
    assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), env);
}

/// TimescaleDB is the default database: a PostgreSQL URL, the `postgres`
/// driver, and a dock.toml whose `postgres` service runs TimescaleDB and
/// gives the app the very URL config/app.toml names.
#[test]
fn timescaledb_is_the_default_database() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
        !env.contains("APP_DATABASE__URL"),
        "set in config/app.toml only: {env}"
    );

    let sqlite = create_app(
        dir.path(),
        &app_on("lite", Ui::None, Database::Sqlite),
        now(),
    )
    .unwrap();
    assert_eq!(configured_url(&sqlite), "sqlite://storage/lite.db?mode=rwc");
    assert_eq!(navree_features(&sqlite), ["sqlite"]);
    assert_eq!(navree_test_features(&sqlite), ["sqlite"]);
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
    let source = NavreeSource::Version("1.0.0".to_owned());
    assert_eq!(
        source.dependency_with_features(&["postgres"]),
        r#"{ version = "1.0.0", features = ["postgres"] }"#
    );
    assert_eq!(source.dependency_with_features(&[]), r#""1.0.0""#);
    let path = NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree"));
    assert_eq!(
        path.dependency_with_features(&["mysql"]),
        r#"{ path = "/opt/navree/crates/navree", features = ["mysql"] }"#
    );
    assert_eq!(Database::default(), Database::Timescaledb);
    assert_eq!(Database::Timescaledb.feature(), Some("postgres"));
    assert_eq!(Database::Postgres.feature(), Some("postgres"));
    assert_eq!(Database::Mysql.feature(), Some("mysql"));
    assert_eq!(Database::Sqlite.feature(), Some("sqlite"));
}

/// The steps `navree new` prints: start the database with Dock first when
/// Dock runs it.
#[test]
fn the_next_steps_start_the_database_first() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
    // The front end links the Navree packages from the checkout, which are installed
    // and built there first; then its own packages.
    let steps = [
        "  pnpm --dir /opt/navree/packages/js install # the Navree packages (not on npm yet)\n",
        "  pnpm --dir /opt/navree/packages/js build # once; the front end links them\n",
        "  pnpm --dir web install     # the front end; `navree dev` starts it\n",
    ];
    let mut at = 0;
    for step in steps {
        let found = message[at..]
            .find(step)
            .unwrap_or_else(|| panic!("{step:?} after byte {at} in\n{message}"));
        at += found + step.len();
    }
    assert!(!message.contains("npm --prefix"), "{message}");

    // A published navree has published packages: only the front end's own install.
    let published = NewApp {
        navree: NavreeSource::Version("1.0.0".to_owned()),
        ..app_on("lite", Ui::Next, Database::Sqlite)
    };
    let message = created_message(&published, &dir.path().join("lite"));
    assert!(
        message.contains("  pnpm --dir web install     # the front end"),
        "{message}"
    );
    assert!(!message.contains("packages/js"), "{message}");

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
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
        !env.contains("APP_DATABASE__URL"),
        "set in config/app.toml only: {env}"
    );
    let config = read(&root, "config/app.toml");
    assert!(
        config
            .contains("# TimescaleDB (PostgreSQL with time-series hypertables): `navree dock up`"),
        "{config}"
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
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
        "navree make:model posts Comment --all",
        "navree make:seeder posts demo_posts",
        // deny by default and the error shape
        "#[authenticated]",
        r#"{ "statusCode": 404, "message": "Not Found", "error": "Not Found", "code": "not_found" }"#,
        ".with_code(",
        // where the docs and skills are
        "https://navree.com/docs/",
        "npx skills add NavreeHQ/navree",
        "navree skills:install",
        "navree mcp",
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

    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("blog", Ui::Next), now()).unwrap();
    let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("web/                   the Next.js front end"),
        "{agents}"
    );
}

#[test]
fn a_published_navree_is_a_version_requirement() {
    let source = NavreeSource::Version("1.0.0".to_owned());
    assert_eq!(source.dependency(), r#""1.0.0""#);
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

    let elsewhere = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
            "web/app/layout.tsx",
            "web/app/login/page.tsx",
            "web/app/page.tsx",
            "web/app/sign-out.tsx",
            "web/lib/navree.ts",
            "web/next.config.ts",
            "web/package.json",
            "web/pnpm-workspace.yaml",
            "web/proxy.ts",
            "web/tsconfig.json",
        ]
    );
    let package: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("web").join("package.json")).unwrap(),
    )
    .expect("package.json is valid JSON");
    assert_eq!(package["name"], "blog-web");
    assert!(
        package["packageManager"]
            .as_str()
            .unwrap()
            .starts_with("pnpm@")
    );
    let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/web/node_modules/"), "{gitignore}");
}

// MOD-012
#[test]
fn ui_next_runs_on_the_navree_packages_and_carries_no_bff_of_its_own() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("blog", Ui::Next), now()).unwrap();

    // The packages are linked from the checkout `navree` comes from.
    let package: serde_json::Value =
        serde_json::from_str(&read(&root, "web/package.json")).unwrap();
    assert_eq!(
        package["dependencies"]["@navree/next"],
        "link:/opt/navree/packages/js/next"
    );
    assert_eq!(
        package["dependencies"]["@navree/client"],
        "link:/opt/navree/packages/js/client"
    );
    // ... which sits outside the app: Next may read it, and compiles it with the app.
    let config = read(&root, "web/next.config.ts");
    for needle in [
        "\"/opt/navree/packages/js\"",
        "turbopack: { root }",
        "outputFileTracingRoot: root",
        "transpilePackages: [\"@navree/next\", \"@navree/client\", \"@navree/bff\"]",
    ] {
        assert!(config.contains(needle), "{needle} in\n{config}");
    }
    assert!(!config.contains("{{") && !config.contains("{%"), "{config}");

    // The BFF, the proxy and the server components come from `createNavree`.
    assert!(read(&root, "web/lib/navree.ts").contains("createNavree()"));
    assert!(read(&root, "web/app/api/[...path]/route.ts").contains("navree.handlers"));
    assert!(read(&root, "web/proxy.ts").contains("navree.proxy("));
    assert!(read(&root, "web/app/page.tsx").contains("navree.auth"));

    // No file keeps tokens or cookies itself.
    for file in tree(&root).into_iter().filter(|p| p.starts_with("web/")) {
        let text = read(&root, &file);
        for needle in [
            "ACCESS_COOKIE",
            "REFRESH_COOKIE",
            "createBff",
            "navree_at",
            "Set-Cookie",
        ] {
            assert!(!text.contains(needle), "{file} mentions {needle}");
        }
    }
    assert!(!root.join("web/lib/bff.ts").exists());

    // The package manager that can read `link:` is pnpm, and `navree dev` picks it from here.
    assert!(read(&root, "web/pnpm-workspace.yaml").contains("allowBuilds"));
    assert!(read(&root, "web/README.md").contains("pnpm --dir /opt/navree/packages/js build"));
}

// MOD-012
#[test]
fn ui_next_from_a_published_navree_asks_npm_for_the_packages() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let published = NewApp {
        navree: NavreeSource::Version("1.0.0".to_owned()),
        ..app("blog", Ui::Next)
    };
    let root = create_app(dir.path(), &published, now()).unwrap();

    let package: serde_json::Value =
        serde_json::from_str(&read(&root, "web/package.json")).unwrap();
    assert_eq!(package["dependencies"]["@navree/next"], "^1.0.0");
    assert_eq!(package["dependencies"]["@navree/client"], "^1.0.0");
    // Nothing is linked, so Next needs no settings for files outside the app.
    let config = read(&root, "web/next.config.ts");
    for needle in [
        "turbopack",
        "outputFileTracingRoot",
        "transpilePackages",
        "node:path",
    ] {
        assert!(!config.contains(needle), "{needle} in\n{config}");
    }
    // The README says the packages install from a checkout until they are published.
    assert!(read(&root, "web/README.md").contains("not on npm yet"));
}

#[test]
fn refuses_a_directory_that_is_not_empty() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    std::fs::create_dir(dir.path().join("blog")).unwrap();
    std::fs::write(dir.path().join("blog").join("notes.txt"), "keep me").unwrap();
    let error = create_app(dir.path(), &app("blog", Ui::None), now()).unwrap_err();
    assert!(error.to_string().contains("is not empty"), "{error}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("blog").join("notes.txt")).unwrap(),
        "keep me"
    );
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_new_app_outside_a_repository_gets_one() {
    if !git_available() {
        println!("SKIPPED: git is not available");
        return;
    }
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("blog", Ui::None), now()).unwrap();
    assert_eq!(init_repository(&root), Repository::Created);
    assert!(root.join(".git").exists());
    // The files are for the first commit: nothing is committed for the user.
    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .output()
        .unwrap();
    assert!(!log.status.success());
    // Running it again keeps the repository.
    assert_eq!(init_repository(&root), Repository::Inside);
}

#[test]
fn a_new_app_inside_a_repository_leaves_it_alone() {
    if !git_available() {
        println!("SKIPPED: git is not available");
        return;
    }
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    let root = create_app(dir.path(), &app("blog", Ui::None), now()).unwrap();
    assert_eq!(init_repository(&root), Repository::Inside);
    assert!(!root.join(".git").exists());
}

#[test]
fn a_kit_gets_its_harbor_key_and_an_app_without_one_does_not() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let kit = NewApp {
        kit: Some(navree_cli::kits::Kit::Api),
        ..app("crm", Ui::None)
    };
    let root = create_app(dir.path(), &kit, now()).unwrap();
    assert!(!root.join("harbor.key").exists());
    write_harbor_key(&root).unwrap();
    let key = read(&root, "harbor.key");
    assert!(key.starts_with("base64:"), "{key}");
    assert!(
        read(&root, ".gitignore")
            .lines()
            .any(|line| line == "harbor*.key")
    );
    // `harbor:secrets` can use it, and a second run keeps it.
    let project =
        navree_harbor::Project::load(&root, navree_harbor::Environment::production()).unwrap();
    assert!(project.key(None).is_ok());
    write_harbor_key(&root).unwrap();
    assert_eq!(read(&root, "harbor.key"), key);

    let plain = create_app(dir.path(), &app("blog", Ui::None), now()).unwrap();
    write_harbor_key(&plain).unwrap();
    assert!(!plain.join("harbor.key").exists());
}

#[test]
fn generated_files_are_free_of_template_syntax() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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

#[test]
fn the_readme_says_how_to_list_the_apps_commands_and_names_those_the_cli_keeps() {
    use clap::CommandFactory;
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("lister", Ui::None), now()).unwrap();
    let readme = read(&root, "README.md");
    // `navree help` prints the CLI's own help; `navree list` is the app's commands.
    assert!(
        readme.contains("| `navree list` | Lists every command of the app"),
        "{readme}"
    );
    assert!(!readme.contains("| `navree help` |"), "{readme}");
    // Every command the CLI runs itself is named, so nobody looks for it in the app.
    let cli = navree_cli::cli::Cli::command();
    let mut checked = 0;
    for command in cli.get_subcommands() {
        let name = command.get_name();
        if name == "help" || name == "new" || name.starts_with("make:") {
            continue;
        }
        let group = name.split(':').next().unwrap_or(name);
        assert!(
            readme.contains(&format!("`{name}`")) || readme.contains(&format!("`{group}:*`")),
            "the README does not say that `{name}` is the CLI's own:\n{readme}"
        );
        checked += 1;
    }
    assert!(checked >= 8, "{checked}");
    // The commands the generated binary says it runs are the app's, not the CLI's.
    let main = read(&root, "src/main.rs");
    assert!(
        !main.contains("`help`") && !main.contains("`check`"),
        "{main}"
    );
}

/// A README that sends readers to `navree help` for the app's commands is wrong:
/// `navree list` lists them, and `navree --help` is the CLI's own.
#[test]
fn the_readme_does_not_send_readers_to_navree_help() {
    for ui in [Ui::None, Ui::Navree] {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let root = create_app(dir.path(), &app("lister", ui), now()).unwrap();
        let readme = read(&root, "README.md");
        assert!(!readme.contains("navree help"), "{ui:?}: {readme}");
        assert!(
            readme.contains("`navree --help` lists the CLI's own"),
            "{ui:?}"
        );
    }
}

// MOD-002
/// Navree UI is the default front end of the minimal app; `--ui none` is the API alone.
#[test]
fn ui_navree_is_the_default() {
    use clap::ValueEnum;
    assert_eq!(Ui::default(), Ui::Navree);
    let names: Vec<String> = Ui::value_variants()
        .iter()
        .filter_map(|ui| ui.to_possible_value())
        .map(|value| value.get_name().to_owned())
        .collect();
    assert_eq!(names, ["next", "navree", "none"]);
}

/// The files of `navree new demo --ui navree`: the minimal app, plus the shell,
/// sign-in and live stream in `src/ui/`, the customers module, and the journey
/// test. No `web/` folder: there is no Node project.
#[test]
fn ui_navree_writes_the_minimal_app_plus_screens_drawn_on_the_server() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let files = tree(&root);
    assert_eq!(
        files,
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
            "src/modules/customers/entity.rs",
            "src/modules/customers/form.rs",
            "src/modules/customers/home.rs",
            "src/modules/customers/migrations/m20261025_143005_create_customers_table.rs",
            "src/modules/customers/migrations/mod.rs",
            "src/modules/customers/mod.rs",
            "src/modules/customers/pages.rs",
            "src/modules/customers/seed.rs",
            "src/modules/customers/service.rs",
            "src/modules/customers/table.rs",
            "src/modules/mod.rs",
            "src/modules/users/controller.rs",
            "src/modules/users/dto.rs",
            "src/modules/users/entity.rs",
            "src/modules/users/migrations/m20261025_143005_create_user_profiles_table.rs",
            "src/modules/users/migrations/mod.rs",
            "src/modules/users/mod.rs",
            "src/modules/users/service.rs",
            "src/modules/users/tests/mod.rs",
            "src/ui/auth.rs",
            "src/ui/format.rs",
            "src/ui/live.rs",
            "src/ui/mod.rs",
            "src/ui/pages.rs",
            "src/ui/shell.rs",
            "storage/.gitignore",
            "tests/app.rs",
            "tests/journey.rs",
        ]
    );
    assert!(
        !files.iter().any(|file| file.starts_with("web/")),
        "no front-end project"
    );
    assert!(!root.join("package.json").exists());
    let listing = files.join("\n");
    insta::assert_snapshot!("ui_navree_files", listing);
}

/// The three files that wire the screens in: the root module, the library and the
/// modules list.
#[test]
fn ui_navree_wires_the_screens_into_the_app() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let app_rs = read(&root, "src/app.rs");
    for line in [
        "use navree::auth::{AuthModule, AuthPages};",
        "use navree::realtime::RealtimeModule;",
        "use navree::session::SessionModule;",
        "use navree::ui::UiModule;",
        "use navree::workspace::WorkspaceModule;",
        "use crate::{abilities, modules, ui};",
        ".module(UiModule::from_config())",
        ".module(SessionModule::from_config())",
        ".module(RealtimeModule::from_config())",
        ".module(AuthPages::new().app_name(\"Demo\"))",
        ".module(WorkspaceModule::from_config())",
        ".module(ui::UiAppModule)",
        ".module(modules::customers::CustomersModule)",
        ".module(modules::users::UsersModule)",
    ] {
        assert!(
            app_rs.contains(line),
            "src/app.rs lacks `{line}`:\n{app_rs}"
        );
    }
    // The marker `navree make:module` adds modules above is still the last line.
    assert!(app_rs.contains("    // navree:modules\n"), "{app_rs}");
    assert!(read(&root, "src/lib.rs").contains("pub mod ui;\n"));
    assert!(read(&root, "src/modules/mod.rs").contains("pub mod customers;\n"));
    // The minimal app without screens has none of it, but it has workspaces
    // from the start.
    let plain = create_app(dir.path(), &app("plain", Ui::None), now()).unwrap();
    for file in [
        "src/app.rs",
        "src/lib.rs",
        "src/modules/mod.rs",
        "Cargo.toml",
    ] {
        let text = read(&plain, file);
        for word in [
            "SessionModule",
            "UiModule",
            "AuthPages",
            "RealtimeModule",
            "customers",
            "pub mod ui",
            "futures-util",
        ] {
            assert!(
                !text.contains(word),
                "{file} of the plain app mentions {word}"
            );
        }
    }
    let plain_app = read(&plain, "src/app.rs");
    assert!(plain_app.contains("use navree::workspace::WorkspaceModule;"));
    assert!(plain_app.contains(".module(WorkspaceModule::from_config())"));
    assert!(read(&plain, "tests/app.rs").contains("a_new_app_has_workspaces_and_resolves_one"));
}

/// The pages build on the facade's default features, and on a few crates of their own.
#[test]
fn ui_navree_has_the_features_and_crates_its_pages_need() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let manifest: toml::Table = read(&root, "Cargo.toml").parse().unwrap();
    let navree = &manifest["dependencies"]["navree"];
    assert_ne!(
        navree
            .get("default-features")
            .and_then(toml::Value::as_bool),
        Some(false),
        "the default features carry the ones the pages use"
    );
    assert_eq!(navree_features(&root), ["postgres"]);
    for dependency in ["chrono", "futures-util", "http", "serde", "tokio", "uuid"] {
        assert!(
            manifest["dependencies"].get(dependency).is_some(),
            "{dependency}"
        );
    }
    let tokio = &manifest["dependencies"]["tokio"];
    let tokio_features: Vec<&str> = tokio["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(tokio_features, ["sync", "time"]);
    // `default = ["full"]` in the facade, and `full` has every feature the pages use.
    let facade: toml::Table = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("navree")
            .join("Cargo.toml"),
    )
    .unwrap()
    .parse()
    .unwrap();
    let list = |name: &str| -> Vec<String> {
        facade["features"][name]
            .as_array()
            .unwrap()
            .iter()
            .map(|feature| feature.as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(list("default"), ["full"]);
    let full = list("full");
    for feature in [
        "auth",
        "crypto",
        "db",
        "http",
        "orm",
        "schema",
        "session",
        "testing",
        "ui",
        "validation",
        "validation-phone",
    ] {
        assert!(
            full.iter().any(|name| name == feature),
            "the `{feature}` feature is not in the facade's `full`"
        );
    }
    // The tests run on SQLite like every other app's.
    assert_eq!(navree_test_features(&root), ["sqlite"]);
}

/// The settings the screens need: sessions in the database, guests sent to sign in,
/// Indian phone numbers, and the pages left out of the API's own documentation check.
#[test]
fn ui_navree_configures_sessions_sign_in_and_the_documentation_check() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let config: toml::Table = read(&root, "config/app.toml").parse().unwrap();
    assert_eq!(config["session"]["store"].as_str(), Some("database"));
    assert_eq!(config["session"]["secure"].as_bool(), Some(false));
    assert_eq!(config["ui"]["sign_in"].as_str(), Some("/sign-in"));
    assert_eq!(config["ui"]["sign_in_redirect"].as_bool(), Some(true));
    assert_eq!(
        config["validation"]["phone"]["default_region"].as_str(),
        Some("IN")
    );
    assert_eq!(config["auth"]["registration"].as_bool(), Some(false));
    assert_eq!(
        config["auth"]["password_reset"]["url"].as_str(),
        Some("http://127.0.0.1:8080/reset-password"),
        "the reset mail links to the reset page"
    );
    // A browser is in the workspace it switched to; API clients name theirs.
    let resolvers: Vec<&str> = config["workspace"]["resolvers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(resolvers, ["session", "header"]);
    let undocumented: Vec<&str> = config["api"]["undocumented"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    for route in [
        "/_navree/*",
        "/",
        "/sign-in",
        "/sign-in/*",
        "/sign-out",
        "/account/*",
        "/workspace/*",
        "/broadcasting/*",
        "/customers*",
        "/fragments/*",
    ] {
        assert!(undocumented.contains(&route), "{route} in {undocumented:?}");
    }
    // The minimal app keeps sign-up on and has none of the page settings.
    let plain = create_app(dir.path(), &app("plain", Ui::None), now()).unwrap();
    let config: toml::Table = read(&plain, "config/app.toml").parse().unwrap();
    assert_eq!(config["auth"]["registration"].as_bool(), Some(true));
    for table in ["session", "ui", "validation"] {
        assert!(config.get(table).is_none(), "{table}");
    }
    let resolvers: Vec<&str> = config["workspace"]["resolvers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    assert_eq!(resolvers, ["header"], "the API names its workspace in a header");
}

/// The sign-in pages are the framework's `AuthPages`; the app keeps one small file
/// for who is signed in and the demo account, and no other file touches the
/// cookie, the session or the password check.
#[test]
fn ui_navree_signs_in_through_auth_pages() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let auth = read(&root, "src/ui/auth.rs");
    assert!(
        auth.starts_with("//! Who is signed in, and the demo account.\n"),
        "{auth}"
    );
    for needle in [
        "pub struct CurrentAccount",
        "impl FromRequestParts for CurrentAccount",
        "impl OptionalFromRequestParts for CurrentAccount",
        "pub async fn ensure_demo_account(",
        "`AuthPages` (navree-auth, registered in",
    ] {
        assert!(auth.contains(needle), "src/ui/auth.rs lacks `{needle}`");
    }
    // No sign-in code of the app's own is left.
    for gone in [
        "pub struct Browser",
        "pub struct BrowserStrategy",
        "pub async fn sign_in(",
        "validate_user",
        "regenerate",
    ] {
        assert!(!auth.contains(gone), "src/ui/auth.rs still has `{gone}`");
    }
    // The demo account is the one the CLI, the README and the message name.
    let navree_cli_email = navree_cli::new_app::DEMO_EMAIL;
    let navree_cli_password = navree_cli::new_app::DEMO_PASSWORD;
    assert!(auth.contains(&format!(
        "pub const DEMO_EMAIL: &str = \"{navree_cli_email}\";"
    )));
    assert!(auth.contains(&format!(
        "pub const DEMO_PASSWORD: &str = \"{navree_cli_password}\";"
    )));
    let readme = read(&root, "README.md");
    assert!(
        readme.contains(&format!("`{navree_cli_email}`")),
        "{readme}"
    );
    assert!(
        readme.contains(&format!("`{navree_cli_password}`")),
        "{readme}"
    );

    // Everything else asks the framework's pages and knows nothing more.
    let mut checked = 0;
    for file in tree(&root) {
        if !(file.starts_with("src/") && file.ends_with(".rs"))
            || file == "src/ui/auth.rs"
            || file == "src/ui/shell.rs"
            || file == "src/app.rs"
        {
            continue;
        }
        let lower = read(&root, &file).to_lowercase();
        for needle in ["hasher", "password_hash", "validate_user", "authservice"] {
            assert!(
                !lower.contains(needle),
                "{file} mentions `{needle}`: signing in is AuthPages' work"
            );
        }
        checked += 1;
    }
    assert!(checked >= 18, "{checked}");
    // The sign-in and sign-out routes are not the app's.
    let pages = read(&root, "src/ui/pages.rs");
    assert!(!pages.contains("/sign-in"), "{pages}");
    assert!(!pages.contains("/sign-out"), "{pages}");
    let customers = read(&root, "src/modules/customers/pages.rs");
    assert!(
        customers.contains("use crate::ui::shell::Desk;"),
        "{customers}"
    );
}

/// The created message lists what to run, the address, and the demo sign-in.
#[test]
fn ui_navree_says_how_to_run_it_and_how_to_sign_in() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let demo = app("demo", Ui::Navree);
    let root = create_app(dir.path(), &demo, now()).unwrap();
    let message = created_message(&demo, &root);
    let steps: Vec<&str> = message
        .lines()
        .skip_while(|line| *line != "Next steps:")
        .skip(1)
        .collect();
    assert_eq!(
        steps,
        [
            "  cd demo",
            "  navree dock up             # TimescaleDB on 127.0.0.1:5432",
            "  navree key:generate        # then set APP_KEY in .env",
            "  navree dev                 # http://127.0.0.1:8080",
            "",
            "Sign in at http://127.0.0.1:8080/sign-in as harpreet.singh@example.com with the password demo-password-2026.",
            "The account exists in development only.",
        ]
    );
    // SQLite needs no database to start.
    let lite = app_on("lite", Ui::Navree, Database::Sqlite);
    let message = created_message(&lite, &dir.path().join("lite"));
    assert!(!message.contains("dock"), "{message}");
    assert!(message.contains("navree key:generate"), "{message}");
    // Other apps say nothing of signing in.
    let message = created_message(&app("plain", Ui::None), &dir.path().join("plain"));
    assert!(!message.contains("Sign in at"), "{message}");
}

/// A kit brings its own front end, or none: `--ui navree` goes with the minimal app only.
#[test]
fn ui_navree_is_refused_with_a_kit() {
    for kit in navree_cli::kits::Kit::ALL {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let error = create_app(
            dir.path(),
            &NewApp {
                kit: Some(kit),
                ..app("demo", Ui::Navree)
            },
            now(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("--ui navree goes with the minimal app"),
            "{error}"
        );
        assert!(error.contains(&format!("the {kit} kit")), "{error}");
        assert!(error.contains("--kit"), "{error}");
        assert!(!dir.path().join("demo").exists(), "nothing is written");
    }
}

/// Every Rust file of the app parses, and none carries template syntax or a
/// carriage return.
#[test]
fn ui_navree_files_parse_and_are_clean_text() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let mut parsed = 0;
    for file in tree(&root) {
        let text = read(&root, &file);
        if file.ends_with(".rs") {
            syn::parse_file(&text).unwrap_or_else(|error| panic!("{file} does not parse: {error}"));
            parsed += 1;
        }
        for marker in ["{%", "%}", "{#"] {
            assert!(!text.contains(marker), "{file} contains `{marker}`");
        }
        if file.ends_with(".rs") || file.ends_with(".toml") {
            assert!(!text.contains("{{"), "{file} contains `{{{{`");
        }
        assert!(!text.contains('\r'), "{file} uses LF line endings");
    }
    assert!(parsed >= 28, "{parsed} Rust files");
}

/// The demo data is Indian: names, cities, rupees in lakhs, +91 numbers.
#[test]
fn ui_navree_demo_data_is_indian() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let seed = read(&root, "src/modules/customers/seed.rs");
    for city in [
        "Ludhiana",
        "Amritsar",
        "Jalandhar",
        "Chandigarh",
        "Delhi",
        "Jaipur",
        "Lucknow",
        "Gurugram",
    ] {
        assert_eq!(
            seed.matches(&format!("| {city} |")).count(),
            5,
            "five customers in {city}"
        );
    }
    assert!(seed.contains("+9198765"), "made-up +91 numbers");
    let format = read(&root, "src/ui/format.rs");
    assert!(format.contains("assert_eq!(rupees(10_000_000), \"₹1,00,000\");"));
    assert!(format.contains("assert_eq!(grouped(1_234_567), \"12,34,567\");"));
    // The form for a new customer starts on today's date in IST.
    assert!(read(&root, "src/modules/customers/pages.rs").contains("TimeDelta::minutes(330)"));
}

/// The shell file: the app's shell, and the extractor that fills in the person
/// and the workspace of each request.
#[test]
fn ui_navree_shell_draws_the_person_and_the_workspace_of_the_request() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let root = create_app(dir.path(), &app("demo", Ui::Navree), now()).unwrap();
    let shell = read(&root, "src/ui/shell.rs");
    for needle in [
        "Shell::new(\"Demo\")",
        "NavItem::new(\"Customers\", \"/customers\")",
        "pub struct Desk",
        "impl FromRequestParts for Desk",
        "WorkspaceSwitcher",
        ".context(workspace.name.clone())",
        "\"Stop impersonating\", \"/impersonate/stop\"",
        "\"Sign out\", \"/sign-out\"",
    ] {
        assert!(shell.contains(needle), "src/ui/shell.rs lacks `{needle}`");
    }
    // Customers belong to a workspace and are read through its repository.
    let entity = read(&root, "src/modules/customers/entity.rs");
    assert!(entity.contains("pub workspace_id: Uuid,"), "{entity}");
    assert!(entity.contains("impl WorkspaceScoped for Customer {}"), "{entity}");
    let service = read(&root, "src/modules/customers/service.rs");
    assert!(service.contains("WorkspaceRepo::<Customer>::insert("), "{service}");
    let live = read(&root, "src/ui/live.rs");
    assert!(live.contains("Topic::workspace(workspace, TOPIC)"), "{live}");
    assert!(!live.contains("broadcast::channel"), "the app's own hub is gone");
}
