//! The version the docs print is the version of the CLI that ships with them.
//! Raising the workspace version fails this test until the places that print
//! it follow. (The website checks the version it prints with a check of its own.)

use std::path::PathBuf;

fn repo_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("cannot read {path}: {error}"))
        .replace('\r', "")
}

/// `1.0` for `1.0.0`.
fn series(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

fn must_contain(path: &str, needle: &str) {
    let text = repo_file(path);
    assert!(
        text.contains(needle),
        "{path} should contain `{needle}` (the workspace version is {}); update it with the version bump",
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn new_apps_depend_on_this_version() {
    assert_eq!(
        navree_cli::new_app::NavreeSource::DEFAULT_VERSION,
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn the_docs_print_the_current_version() {
    let version = env!("CARGO_PKG_VERSION");
    let series = series(version);

    let installation = "docs/getting-started/installation.mdx";
    must_contain(installation, &format!("navree {version}\n"));
    must_contain(installation, &format!("navree = \"{version}\""));
    must_contain(
        "docs/getting-started/starter-kits.mdx",
        &format!("kit_version = \"{version}\""),
    );
    must_contain(
        "docs/prologue/introduction.mdx",
        &format!("This documentation describes Navree {series}."),
    );
    must_contain(
        "docs/prologue/roadmap.mdx",
        &format!("Where Navree goes after {series}"),
    );
    must_contain(
        "docs/prologue/release-notes.mdx",
        &format!("## Navree {version}\n"),
    );
}
