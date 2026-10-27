//! `navree new`: the app skeleton.

use std::path::{Path, PathBuf};

use navree_cli::clock::Timestamp;
use navree_cli::names::AppName;
use navree_cli::new_app::{NavreeSource, NewApp, Ui, create_app};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn app(name: &str, ui: Ui) -> NewApp {
    NewApp {
        name: AppName::parse(name).unwrap(),
        ui,
        navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
    }
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
    assert!(parsed.contains_key("workspace"), "a standalone workspace");
    let main = std::fs::read_to_string(root.join("src").join("main.rs")).unwrap();
    assert!(
        main.contains("my_shop::app(ConfigSources::defaults()).run()"),
        "{main}"
    );
    let env = std::fs::read_to_string(root.join(".env.example")).unwrap();
    assert!(
        env.contains("APP_DATABASE__URL=sqlite://storage/my_shop.db?mode=rwc"),
        "{env}"
    );
    assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), env);
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
