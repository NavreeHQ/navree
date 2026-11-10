//! Setting the version in each kind of file, the changelog section, the
//! static checks, the index paths, the dates and the staging of
//! Navigator's files.

mod common;

use std::time::{Duration, UNIX_EPOCH};

use common::{Member, member, workspace};
use xtask::changelog::{add_section, draft, has_section};
use xtask::check::problems;
use xtask::index::{index_path, lists_version};
use xtask::prepare::{is_valid_version, set_member_manifest, set_package_json, set_root_manifest};
use xtask::time::{civil_from_days, date_utc, days_from_civil, format_utc, parse_http_date};

const ROOT: &str = r#"[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "1.0.0" # every crate
edition = "2024"

[workspace.dependencies]
# Navree crates
navree = { path = "crates/navree", version = "=1.0.0" }
navree-kernel = { path = "crates/navree-kernel", version = "=1.0.0", default-features = false }
navree-new = { path = "crates/navree-new" }

# Others
serde = { version = "1.0.229", features = ["derive"] }
"#;

#[test]
fn the_root_manifest_gets_the_version_and_exact_pins_and_keeps_its_layout() {
    let internal = vec![
        "navree".to_owned(),
        "navree-kernel".to_owned(),
        "navree-new".to_owned(),
    ];
    let out = set_root_manifest(ROOT, "1.2.3", &internal).unwrap();
    assert_eq!(
        out,
        ROOT.replace(
            "version = \"1.0.0\" # every crate",
            "version = \"1.2.3\" # every crate"
        )
        .replace("version = \"=1.0.0\"", "version = \"=1.2.3\"")
        .replace(
            "navree-new = { path = \"crates/navree-new\" }",
            "navree-new = { path = \"crates/navree-new\", version = \"=1.2.3\" }"
        )
    );
}

#[test]
fn a_member_with_its_own_version_is_set_and_an_inheriting_one_is_left() {
    let own = "[package]\nname = \"a\"\nversion = \"1.0.0\"\nedition = \"2024\"\n";
    assert_eq!(
        set_member_manifest(own, "1.0.1").unwrap().unwrap(),
        own.replace("1.0.0", "1.0.1")
    );
    let inherits = "[package]\nname = \"a\"\nversion.workspace = true\n";
    assert_eq!(set_member_manifest(inherits, "1.0.1").unwrap(), None);
}

#[test]
fn a_package_json_gets_the_version_and_keeps_everything_else() {
    let text = "{\n  \"name\": \"@navree/client\",\n  \"version\": \"1.0.0\",\n  \"dependencies\": {\n    \"x\": \"1.0.0\",\n    \"version\": \"2\"\n  }\n}\n";
    assert_eq!(
        set_package_json(text, "1.1.0").unwrap(),
        text.replace("\"version\": \"1.0.0\"", "\"version\": \"1.1.0\"")
    );
    assert_eq!(
        set_package_json("{\n  \"name\": \"x\",\n  \"private\": true\n}\n", "1.1.0"),
        None
    );
}

#[test]
fn versions_are_checked() {
    for good in ["1.0.0", "1.2.30", "2.0.0-rc.1", "2.0.0-beta.2"] {
        assert!(is_valid_version(good), "{good}");
    }
    for bad in [
        "1.0",
        "v1.0.0",
        "1.0.0.0",
        "01.0.0",
        "1.0.0-",
        "1.0.0-rc..1",
        "1.0.x",
    ] {
        assert!(!is_valid_version(bad), "{bad}");
    }
}

const CHANGELOG: &str = "# Changelog\n\nEvery change to Navree, newest first.\n\n## [1.0.0] - 2026-11-10\n\nFirst release.\n\n[1.0.0]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.0\n";

#[test]
fn the_changelog_gets_a_section_above_the_newest_with_its_link() {
    assert!(has_section(CHANGELOG, "1.0.0"));
    assert!(!has_section(CHANGELOG, "1.0.1"));
    assert!(!has_section(CHANGELOG, "1.0"));
    let out = add_section(
        CHANGELOG,
        "1.0.1",
        "2026-11-20",
        "### Fixed\n\n- http: a fix\n",
    );
    assert_eq!(
        out,
        "# Changelog\n\nEvery change to Navree, newest first.\n\n## [1.0.1] - 2026-11-20\n\n### Fixed\n\n- http: a fix\n\n## [1.0.0] - 2026-11-10\n\nFirst release.\n\n[1.0.1]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.1\n[1.0.0]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.0\n"
    );
    assert_eq!(
        add_section(&out, "1.0.1", "2026-11-21", ""),
        out,
        "a second run changes nothing"
    );
    assert_eq!(
        add_section("# Changelog\n", "1.0.0", "2026-11-10", ""),
        "# Changelog\n\n## [1.0.0] - 2026-11-10\n\n"
    );
}

#[test]
fn the_draft_sorts_conventional_commits() {
    let subjects: Vec<String> = [
        "feat(ui): a date picker",
        "fix(http): keep-alive after a 304",
        "docs(website): a typo",
        "perf(orm): one query for eager loads",
        "feat(config)!: the old key is gone",
        "Merge branch 'x'",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_eq!(
        draft(&subjects),
        "### Added\n\n- ui: a date picker\n\n### Changed\n\n- orm: one query for eager loads\n- **Breaking:** config: the old key is gone\n\n### Fixed\n\n- http: keep-alive after a 304\n\n"
    );
}

#[test]
fn the_checks_name_every_problem() {
    let ws = workspace(&[
        member(
            "navree",
            &[
                ("navree-http", "normal", "=1.0.0"),
                ("navree-kernel", "normal", "^1.0.0"),
            ],
        ),
        member(
            "navree-http",
            &[
                ("navree-kernel", "normal", "=1.0.0"),
                ("navree", "dev", "*"),
                ("navree-testkit", "dev", "*"),
            ],
        ),
        member("navree-kernel", &[("navree-kernel", "dev", "=1.0.0")]),
        member(
            "navree-orm",
            &[
                ("navree-kernel", "normal", "*"),
                ("navree-testkit", "normal", "=1.0.0"),
            ],
        ),
        Member {
            name: "navree-old",
            published: true,
            version: "0.9.0",
            deps: &[],
        },
        Member {
            name: "navree-testkit",
            published: false,
            version: "1.0.0",
            deps: &[],
        },
    ]);
    let manifest = "[workspace.package]\nversion = \"1.0.0\"\n[workspace.dependencies]\nnavree = { path = \"crates/navree\", version = \"=1.0.0\" }\nnavree-http = { path = \"crates/navree-http\", version = \"1.0.0\" }\nnavree-orm = { path = \"crates/navree-orm\" }\nnavree-testkit = { path = \"crates/navree-testkit\" }\n";
    let found = problems(&ws, "1.0.0", manifest);
    let expected = [
        "navree: dependency navree-kernel requires `^1.0.0`, not `=1.0.0`",
        "navree-old is at 0.9.0, the workspace at 1.0.0",
        "navree-orm: dependency navree-kernel requires `*`, not `=1.0.0` (a path without a version cannot be published)",
        "navree-orm: dependency navree-testkit is not published",
        "[workspace.dependencies] navree-http requires `1.0.0`, not `=1.0.0`",
        "[workspace.dependencies] navree-orm has no version",
        "\n  navree-kernel",
    ];
    for want in expected {
        assert!(
            found.iter().any(|p| p.contains(want)),
            "missing {want:?} in {found:#?}"
        );
    }
    // The path-only dev-dependencies of navree-http are fine; the
    // unpublished testkit has no pin to check.
    assert!(
        !found.iter().any(|p| p.starts_with("navree-http")),
        "{found:#?}"
    );
    assert!(
        !found.iter().any(|p| p.contains("] navree-testkit")),
        "{found:#?}"
    );
    assert_eq!(found.len(), expected.len(), "{found:#?}");
}

#[test]
fn a_clean_workspace_has_no_problems() {
    let ws = workspace(&[
        member(
            "navree",
            &[
                ("navree-kernel", "normal", "=1.0.0"),
                ("serde", "normal", "^1"),
            ],
        ),
        member(
            "navree-kernel",
            &[("navree", "dev", "*"), ("navree-kernel", "dev", "*")],
        ),
    ]);
    let manifest = "[workspace.package]\nversion = \"1.0.0\"\n[workspace.dependencies]\nnavree = { path = \"crates/navree\", version = \"=1.0.0\" }\nnavree-kernel = { path = \"crates/navree-kernel\", version = \"=1.0.0\" }\nserde = \"1\"\n";
    assert_eq!(problems(&ws, "1.0.0", manifest), Vec::<String>::new());
}

#[test]
fn index_paths_follow_the_registry_layout() {
    assert_eq!(index_path("a"), "1/a");
    assert_eq!(index_path("ab"), "2/ab");
    assert_eq!(index_path("abc"), "3/a/abc");
    assert_eq!(index_path("navree-kernel"), "na/vr/navree-kernel");
    assert_eq!(index_path("Navree"), "na/vr/navree");
    let file = "{\"name\":\"navree\",\"vers\":\"1.0.0\",\"yanked\":false}\n{\"name\":\"navree\",\"vers\":\"1.0.1\",\"yanked\":true}\n";
    assert!(lists_version(file, "1.0.0"));
    assert!(
        lists_version(file, "1.0.1"),
        "a yanked version is still taken"
    );
    assert!(!lists_version(file, "1.0.2"));
    assert!(!lists_version("", "1.0.0"));
}

#[test]
fn dates_are_read_and_written_in_utc() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    for days in [0, 11_017, 20_402, -1, 60_000] {
        let (y, m, d) = civil_from_days(days);
        assert_eq!(days_from_civil(y, m, d), days);
    }
    let time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    assert_eq!(format_utc(time), "2023-11-14 22:13:20 UTC");
    assert_eq!(date_utc(time), "2023-11-14");
    assert_eq!(parse_http_date("Tue, 14 Nov 2023 22:13:20 GMT"), Some(time));
    assert_eq!(parse_http_date("14 Nov 2023 22:13:20 GMT"), Some(time));
    assert_eq!(parse_http_date("Tue, 14 Nov 2023 22:13:20 PST"), None);
    assert_eq!(parse_http_date("Tue, 34 Nov 2023 22:13:20 GMT"), None);
    assert_eq!(parse_http_date("soon"), None);
}

#[test]
fn navigator_files_are_staged_from_the_docs_and_skills_and_cleaned() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let write = |path: &str, text: &str| {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    };
    write(
        "crates/navree-navigator/Cargo.toml",
        "[package]\nname = \"navree-navigator\"\n",
    );
    write("skills/README.md", "not a skill");
    write("skills/navree/SKILL.md", "# navree");
    write("skills/navree-orm/SKILL.md", "# orm");
    write("skills/navree-orm/references/queries.md", "queries");
    write("skills/other/SKILL.md", "not ours");
    write("docs/index.mdx", "home");
    write("docs/prologue/release-notes.mdx", "notes");
    write("docs/prologue/meta.json", "{}");
    write(
        "docs/ui-data/catalog.json",
        "{\"groups\":[],\"components\":[]}",
    );
    write("docs/ui-data/stray.mdx", "not a page");

    let staged = xtask::stage::stage(root).unwrap();
    assert_eq!(staged.skill_files, 3);
    assert_eq!(staged.doc_pages, 2);
    let navigator = root.join("crates/navree-navigator");
    for file in [
        "skills/navree/SKILL.md",
        "skills/navree-orm/references/queries.md",
        "docs/index.mdx",
        "docs/prologue/release-notes.mdx",
        "catalog.json",
    ] {
        assert!(navigator.join(file).is_file(), "{file}");
    }
    for file in [
        "skills/README.md",
        "skills/other",
        "docs/prologue/meta.json",
        "docs/ui-data",
    ] {
        assert!(!navigator.join(file).exists(), "{file}");
    }

    // A page removed from the docs is gone from the next staging.
    std::fs::remove_file(root.join("docs/prologue/release-notes.mdx")).unwrap();
    assert_eq!(xtask::stage::stage(root).unwrap().doc_pages, 1);
    assert!(!navigator.join("docs/prologue/release-notes.mdx").exists());

    xtask::stage::clean(root).unwrap();
    for file in ["skills", "docs", "catalog.json"] {
        assert!(!navigator.join(file).exists(), "{file}");
    }
    assert!(navigator.join("Cargo.toml").is_file());
}
