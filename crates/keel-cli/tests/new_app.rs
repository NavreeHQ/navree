//! `keel new`: the app skeleton.

use std::path::{Path, PathBuf};

use keel_cli::clock::Timestamp;
use keel_cli::names::AppName;
use keel_cli::new_app::{KeelSource, NewApp, Ui, create_app};

fn now() -> Timestamp {
    Timestamp::from_unix(1_792_938_605)
}

fn app(name: &str, ui: Ui) -> NewApp {
    NewApp {
        name: AppName::parse(name).unwrap(),
        ui,
        keel: KeelSource::Path(PathBuf::from("/opt/keel/crates/keel")),
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
fn cargo_manifest_names_the_package_and_the_keel_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_app(dir.path(), &app("my-shop", Ui::None), now()).unwrap();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let parsed: toml::Table = manifest.parse().expect("Cargo.toml is valid TOML");
    assert_eq!(parsed["package"]["name"].as_str(), Some("my-shop"));
    assert_eq!(
        parsed["dependencies"]["keel"]["path"].as_str(),
        Some("/opt/keel/crates/keel")
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

#[test]
fn a_published_keel_is_a_version_requirement() {
    let source = KeelSource::Version("0.1.0".to_owned());
    assert_eq!(source.dependency(), r#""0.1.0""#);
    let windows = KeelSource::Path(PathBuf::from(r"C:\src\keel\crates\keel"));
    assert_eq!(
        windows.dependency(),
        r#"{ path = "C:/src/keel/crates/keel" }"#
    );
}

/// `canonicalize()` on Windows gives verbatim paths (`\\?\C:\..`), which
/// cargo rejects in a `path` dependency ("invalid path url").
#[test]
fn verbatim_windows_paths_become_plain_paths() {
    for (path, expected) in [
        (
            r"\\?\C:\src\keel\crates\keel",
            r#"{ path = "C:/src/keel/crates/keel" }"#,
        ),
        (
            r"\\?\UNC\server\share\keel\crates\keel",
            r#"{ path = "//server/share/keel/crates/keel" }"#,
        ),
    ] {
        assert_eq!(KeelSource::Path(PathBuf::from(path)).dependency(), expected);
    }
}

#[test]
fn keel_path_accepts_the_repository_or_the_facade_crate() {
    let repo = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let facade = repo.join("crates").join("keel");
    let KeelSource::Path(from_repo) = KeelSource::resolve(Some(&repo)).unwrap() else {
        panic!("expected a path source");
    };
    assert!(
        from_repo.ends_with(Path::new("crates").join("keel")),
        "{}",
        from_repo.display()
    );
    let KeelSource::Path(from_facade) = KeelSource::resolve(Some(&facade)).unwrap() else {
        panic!("expected a path source");
    };
    assert_eq!(from_facade, from_repo);

    let elsewhere = tempfile::tempdir().unwrap();
    let error = KeelSource::resolve(Some(elsewhere.path())).unwrap_err();
    assert!(format!("{error:#}").contains("--keel-path"), "{error:#}");

    // Without the flag: the checkout this binary was built from.
    assert!(matches!(
        KeelSource::resolve(None).unwrap(),
        KeelSource::Path(_)
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
