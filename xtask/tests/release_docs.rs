//! Setting the version in the docs that print it, and writing the release
//! notes from the changelog.

use xtask::docs::{
    add_release_notes, release_notes_section, series, set_installation, set_introduction,
    set_kit_version, set_reference, set_roadmap, update,
};

const INSTALLATION: &str = "Check that it works:\n\n```sh\nnavree --version\n```\n\n```text\nnavree 1.0.0\n```\n\nThe CLI says navree 1.0.0 in prose, and `cargo add navree` is not a pin.\n\n3. Otherwise, the published crate at the CLI's own version: `navree = \"1.0.0\"`. This is the case after `cargo install navree-cli`.\n\n`navree = { version = \"1\", features = [\"postgres\"] }` stays.\n";

const KITS: &str = "```toml title=\"Cargo.toml\"\n[package.metadata.navree]\nkit = \"saas\"\nkit_version = \"1.0.0\"\n```\n";

const INTRODUCTION: &str = "---\ntitle: \"Introduction\"\n---\n\nThis documentation describes Navree 1.0. The [release notes](/docs/prologue/release-notes/) list what it ships.\n";

const ROADMAP: &str = "---\ntitle: \"Roadmap\"\ndescription: \"Where Navree goes after 1.0, in the order it is expected.\"\n---\n\nBody.\n";

const REFERENCE: &str = "| `version` | string | `\"1.0.0\"` | `APP_API__VERSION` | The API version, `info.version` (default `1.0.0`). |\n| `user_agent` | string | `\"navree-http-client/1.0.0\"` | `APP_HTTP_CLIENT__USER_AGENT` | The `User-Agent` header sent with every request (default `navree-http-client/<version>`). |\n| `sample_ratio` | number | `1.0` | `APP_OBSERVE__TRACING__SAMPLE_RATIO` | The share of traces. |\n";

const CHANGELOG: &str = "# Changelog\n\n## [Unreleased]\n\n## [1.0.1] - 2026-11-11\n\n### Fixed\n\n- `navree dev` on Linux restarted the server again and again with nothing edited: it took a file\n  being opened for an edit. Only edits start a rebuild now.\n\n### Security\n\n- The development install of `@navree/nuxt` takes node-forge 1.4.3, for the four advisories fixed in it.\n  Only Nuxt's development HTTPS certificates use it.\n\n### Changed\n\n- Dependencies: the minor and patch updates of the Rust crates.\n\n## [1.0.0] - 2026-11-09\n\nFirst release. Read what it includes in the [release notes](https://navree.com/docs/prologue/release-notes/).\n\n[Unreleased]: https://github.com/NavreeHQ/navree/compare/v1.0.1...HEAD\n[1.0.1]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.1\n[1.0.0]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.0\n";

const NOTES: &str = "---\ntitle: \"Release notes\"\n---\n\n## Navree 1.0.0\n\nNavree is a framework.\n\n### Install it\n\nSteps.\n";

#[test]
fn the_series_is_the_major_and_minor() {
    assert_eq!(series("1.0.1"), "1.0");
    assert_eq!(series("1.4.0"), "1.4");
    assert_eq!(series("2.0.0-rc.1"), "2.0");
}

#[test]
fn the_installation_page_prints_the_version_twice() {
    let out = set_installation(INSTALLATION, "1.0.1").unwrap();
    assert_eq!(
        out,
        INSTALLATION
            .replace("\nnavree 1.0.0\n```", "\nnavree 1.0.1\n```")
            .replace("`navree = \"1.0.0\"`", "`navree = \"1.0.1\"`"),
        "only the output line and the pin change: the prose and the other dependency lines stay"
    );
    assert_eq!(set_installation(&out, "1.0.1").unwrap(), out);
}

#[test]
fn the_installation_page_takes_a_pre_release() {
    let out = set_installation(INSTALLATION, "2.0.0-rc.1").unwrap();
    assert!(out.contains("\nnavree 2.0.0-rc.1\n```"), "{out}");
    assert!(out.contains("`navree = \"2.0.0-rc.1\"`"), "{out}");
    assert_eq!(set_installation(&out, "2.0.0-rc.1").unwrap(), out);
    // And back to a release.
    let back = set_installation(&out, "2.0.0").unwrap();
    assert!(back.contains("\nnavree 2.0.0\n```"), "{back}");
    assert!(back.contains("`navree = \"2.0.0\"`"), "{back}");
}

#[test]
fn the_installation_page_keeps_its_line_endings() {
    let crlf = INSTALLATION.replace('\n', "\r\n");
    let out = set_installation(&crlf, "1.0.1").unwrap();
    assert_eq!(
        out,
        set_installation(INSTALLATION, "1.0.1")
            .unwrap()
            .replace('\n', "\r\n")
    );
}

#[test]
fn the_kit_version_is_set() {
    let out = set_kit_version(KITS, "1.0.1").unwrap();
    assert_eq!(out, KITS.replace("\"1.0.0\"", "\"1.0.1\""));
    assert_eq!(set_kit_version(&out, "1.0.1").unwrap(), out);
    let pre = set_kit_version(KITS, "1.1.0-beta.2").unwrap();
    assert!(pre.contains("kit_version = \"1.1.0-beta.2\"\n"), "{pre}");
    assert_eq!(set_kit_version(&pre, "1.1.0-beta.2").unwrap(), pre);
}

#[test]
fn the_series_changes_only_with_the_minor_version() {
    assert_eq!(
        set_introduction(INTRODUCTION, "1.0.1").unwrap(),
        INTRODUCTION,
        "a patch release leaves the series"
    );
    assert_eq!(set_roadmap(ROADMAP, "1.0.7").unwrap(), ROADMAP);

    let intro = set_introduction(INTRODUCTION, "1.1.0").unwrap();
    assert_eq!(
        intro,
        INTRODUCTION.replace("describes Navree 1.0.", "describes Navree 1.1.")
    );
    assert_eq!(set_introduction(&intro, "1.1.0").unwrap(), intro);

    let roadmap = set_roadmap(ROADMAP, "1.1.0").unwrap();
    assert_eq!(roadmap, ROADMAP.replace("after 1.0,", "after 1.1,"));
    assert_eq!(set_roadmap(&roadmap, "1.1.0").unwrap(), roadmap);

    // A pre-release states the series it belongs to.
    assert!(
        set_introduction(INTRODUCTION, "2.0.0-rc.1")
            .unwrap()
            .contains("describes Navree 2.0. The")
    );
    assert!(
        set_roadmap(ROADMAP, "2.0.0-rc.1")
            .unwrap()
            .contains("goes after 2.0,")
    );
}

#[test]
fn the_reference_changes_only_the_http_clients_user_agent() {
    let out = set_reference(REFERENCE, "1.0.1").unwrap();
    assert_eq!(
        out,
        REFERENCE.replace(
            "`\"navree-http-client/1.0.0\"`",
            "`\"navree-http-client/1.0.1\"`"
        ),
        "the API's own version default, the other numbers and the `<version>` description stay"
    );
    assert_eq!(set_reference(&out, "1.0.1").unwrap(), out);
    let pre = set_reference(REFERENCE, "2.0.0-rc.1").unwrap();
    assert!(pre.contains("`\"navree-http-client/2.0.0-rc.1\"`"), "{pre}");
    assert!(
        pre.contains("(default `navree-http-client/<version>`)"),
        "{pre}"
    );
    assert_eq!(set_reference(&pre, "2.0.0-rc.1").unwrap(), pre);
}

#[test]
fn a_page_with_nothing_to_change_is_an_error() {
    assert!(set_installation("no version here\n", "1.0.1").is_err());
    assert!(set_installation("```text\nnavree 1.0.0\n```\n", "1.0.1").is_err());
    assert!(set_kit_version("kit = \"saas\"\n", "1.0.1").is_err());
    assert!(set_introduction("Hello.\n", "1.0.1").is_err());
    assert!(set_roadmap("Hello.\n", "1.0.1").is_err());
    assert!(set_reference("| `user_agent` | string |  | `X` | d |\n", "1.0.1").is_err());
}

#[test]
fn the_release_notes_are_written_from_the_changelog_section() {
    let section = release_notes_section(CHANGELOG, "1.0.1").unwrap();
    assert_eq!(
        section,
        "## Navree 1.0.1\n\nNavree 1.0.1 is a patch release. To update, run `cargo update` in your app, which takes it within the range your `Cargo.toml` allows, and `cargo install navree-cli --force` for the command-line tool.\n\n### Fixed\n\n- `navree dev` on Linux restarted the server again and again with nothing edited: it took a file being opened for an edit. Only edits start a rebuild now.\n\n### Security\n\n- The development install of `@navree/nuxt` takes node-forge 1.4.3, for the four advisories fixed in it. Only Nuxt's development HTTPS certificates use it.\n\n### Changed\n\n- Dependencies: the minor and patch updates of the Rust crates.\n"
    );
    assert!(release_notes_section(CHANGELOG, "1.0.2").is_none());
    assert!(
        release_notes_section(CHANGELOG, "1.0").is_none(),
        "a prefix is not the version"
    );
}

#[test]
fn the_release_notes_section_goes_above_the_newest_and_a_second_run_changes_nothing() {
    let out = add_release_notes(NOTES, "1.0.1", CHANGELOG).unwrap();
    let section = release_notes_section(CHANGELOG, "1.0.1").unwrap();
    assert_eq!(
        out,
        format!(
            "---\ntitle: \"Release notes\"\n---\n\n{section}\n## Navree 1.0.0\n\nNavree is a framework.\n\n### Install it\n\nSteps.\n"
        )
    );
    assert_eq!(add_release_notes(&out, "1.0.1", CHANGELOG).unwrap(), out);
    // The next release goes above this one.
    let changelog = CHANGELOG.replace(
        "## [1.0.1]",
        "## [1.1.0] - 2026-12-01\n\n### Added\n\n- ui: a date picker\n\n## [1.0.1]",
    );
    let next = add_release_notes(&out, "1.1.0", &changelog).unwrap();
    let at = |needle: &str| next.find(needle).unwrap();
    assert!(at("## Navree 1.1.0") < at("## Navree 1.0.1"));
    assert!(at("## Navree 1.0.1") < at("## Navree 1.0.0"));
    assert!(next.contains("Navree 1.1.0 is a minor release."), "{next}");
}

#[test]
fn a_pre_release_has_notes_that_say_how_to_try_it() {
    let changelog = "# Changelog\n\n## [2.0.0-rc.1] - 2026-12-01\n\n### Changed\n\n- **Breaking:** config: the old key is gone\n\n[2.0.0-rc.1]: https://github.com/NavreeHQ/navree/releases/tag/v2.0.0-rc.1\n";
    let section = release_notes_section(changelog, "2.0.0-rc.1").unwrap();
    assert_eq!(
        section,
        "## Navree 2.0.0-rc.1\n\nNavree 2.0.0-rc.1 is a pre-release. To try it, set `navree = \"2.0.0-rc.1\"` in your app's `Cargo.toml` and run `cargo install navree-cli --version 2.0.0-rc.1` for the command-line tool.\n\n### Changed\n\n- **Breaking:** config: the old key is gone\n"
    );
    let out = add_release_notes(NOTES, "2.0.0-rc.1", changelog).unwrap();
    assert!(out.contains("## Navree 2.0.0-rc.1\n"));
    assert_eq!(
        add_release_notes(&out, "2.0.0-rc.1", changelog).unwrap(),
        out
    );
}

#[test]
fn the_notes_are_mdx_and_link_inside_the_site() {
    let changelog = "## [1.0.2] - 2026-11-20\n\n### Fixed\n\n- http: `Vec<u8>` bodies and a {brace} or a <tag> read fine; see the\n  [page](https://navree.com/docs/basics/routing/).\n";
    let section = release_notes_section(changelog, "1.0.2").unwrap();
    assert!(
        section.contains(
            "- http: `Vec<u8>` bodies and a \\{brace} or a \\<tag> read fine; see the [page](/docs/basics/routing/).\n"
        ),
        "{section}"
    );
}

#[test]
fn an_empty_changelog_section_gives_a_section_with_the_sentence_only() {
    let changelog = "## [1.0.3] - 2026-11-20\n\n## [1.0.2] - 2026-11-19\n\n- x\n";
    assert_eq!(
        release_notes_section(changelog, "1.0.3").unwrap(),
        "## Navree 1.0.3\n\nNavree 1.0.3 is a patch release. To update, run `cargo update` in your app, which takes it within the range your `Cargo.toml` allows, and `cargo install navree-cli --force` for the command-line tool.\n"
    );
}

#[test]
fn a_missing_changelog_section_or_release_notes_heading_is_an_error() {
    assert!(add_release_notes(NOTES, "1.0.9", CHANGELOG).is_err());
    assert!(add_release_notes("---\ntitle: x\n---\n", "1.0.1", CHANGELOG).is_err());
}

fn write(root: &std::path::Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

#[test]
fn update_changes_the_six_pages_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pages = [
        ("docs/getting-started/installation.mdx", INSTALLATION),
        ("docs/getting-started/starter-kits.mdx", KITS),
        ("docs/prologue/introduction.mdx", INTRODUCTION),
        ("docs/prologue/roadmap.mdx", ROADMAP),
        ("docs/configuration/reference.mdx", REFERENCE),
        ("docs/prologue/release-notes.mdx", NOTES),
    ];
    for (path, text) in pages {
        write(root, path, text);
    }

    // A patch release changes every page but the two with the series.
    let changed = update(root, "1.0.1", CHANGELOG).unwrap();
    let names: Vec<String> = changed
        .iter()
        .map(|f| {
            f.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(
        names,
        [
            "docs/getting-started/installation.mdx",
            "docs/getting-started/starter-kits.mdx",
            "docs/configuration/reference.mdx",
            "docs/prologue/release-notes.mdx",
        ]
    );
    let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    assert!(read("docs/configuration/reference.mdx").contains("navree-http-client/1.0.1\""));
    assert!(
        read("docs/prologue/release-notes.mdx")
            .starts_with("---\ntitle: \"Release notes\"\n---\n\n## Navree 1.0.1\n")
    );

    // The same version again changes nothing.
    assert_eq!(
        update(root, "1.0.1", CHANGELOG).unwrap(),
        Vec::<std::path::PathBuf>::new()
    );

    // A minor release changes the series too.
    let changelog =
        format!("## [1.1.0] - 2026-12-01\n\n### Added\n\n- ui: a date picker\n\n{CHANGELOG}");
    let changed = update(root, "1.1.0", &changelog).unwrap();
    assert_eq!(changed.len(), 6);
    assert!(read("docs/prologue/introduction.mdx").contains("describes Navree 1.1."));
    assert!(read("docs/prologue/roadmap.mdx").contains("goes after 1.1,"));
}

#[test]
fn update_writes_nothing_when_a_page_cannot_be_updated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "docs/getting-started/installation.mdx", INSTALLATION);
    write(root, "docs/getting-started/starter-kits.mdx", KITS);
    write(root, "docs/prologue/introduction.mdx", INTRODUCTION);
    write(root, "docs/prologue/roadmap.mdx", ROADMAP);
    write(root, "docs/configuration/reference.mdx", "nothing to set\n");
    write(root, "docs/prologue/release-notes.mdx", NOTES);
    let error = update(root, "1.0.1", CHANGELOG).unwrap_err();
    assert!(error.contains("reference.mdx"), "{error}");
    assert_eq!(
        std::fs::read_to_string(root.join("docs/getting-started/installation.mdx")).unwrap(),
        INSTALLATION
    );
}
