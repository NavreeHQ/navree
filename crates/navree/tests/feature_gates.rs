//! Every facade test says which features it needs in `Cargo.toml`
//! (`required-features`), so a feature set that lacks one leaves the test out of
//! the build and names it, instead of compiling an empty file that reports no
//! tests. This file keeps that true: no test file gates itself with
//! `#![cfg(feature = ..)]`, and every test target but the few that need no
//! feature names the features it needs.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Test files that need none of the facade's features: they read the
/// workspace (its manifests, its crates and its lockfile) and compile with any
/// feature set.
const NEEDS_NO_FEATURE: &[&str] = &[
    "config_reference",
    "dependency_graph",
    "feature_gates",
    "release_markers",
];

/// The facade's package in `cargo metadata`.
fn facade() -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .expect("failed to run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value =
        serde_json::from_slice(&output.stdout).expect("`cargo metadata` printed invalid JSON");
    metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|package| package["name"] == "navree")
        .expect("the navree package")
        .clone()
}

/// The test targets of the facade: (name, required features).
fn test_targets() -> Vec<(String, Vec<String>)> {
    facade()["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
        })
        .map(|target| {
            let name = target["name"].as_str().expect("a target name").to_owned();
            let required = target["required-features"]
                .as_array()
                .map(|features| {
                    features
                        .iter()
                        .map(|feature| feature.as_str().expect("a feature").to_owned())
                        .collect()
                })
                .unwrap_or_default();
            (name, required)
        })
        .collect()
}

/// The `.rs` files directly in `tests/`.
fn test_files() -> Vec<std::path::PathBuf> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    std::fs::read_dir(&tests)
        .expect("the tests folder")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect()
}

#[test]
fn every_test_file_is_a_target_that_names_its_features() {
    let targets = test_targets();
    let mut problems = Vec::new();
    for path in test_files() {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("a file name")
            .to_owned();
        match targets.iter().find(|(target, _)| *target == name) {
            None => problems.push(format!("tests/{name}.rs is not a test target")),
            Some((_, required)) => {
                if required.is_empty() && !NEEDS_NO_FEATURE.contains(&name.as_str()) {
                    problems.push(format!(
                        "tests/{name}.rs has no `required-features`: add a `[[test]]` entry to \
                         crates/navree/Cargo.toml naming the features it needs"
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn no_test_file_gates_itself_on_a_feature() {
    let mut gated = Vec::new();
    for path in test_files() {
        let source = std::fs::read_to_string(&path).expect("a test file");
        if source
            .lines()
            .any(|line| line.starts_with("#![cfg(") && line.contains("feature"))
        {
            gated.push(
                path.file_name()
                    .expect("a file name")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    assert!(
        gated.is_empty(),
        "{gated:?} gate themselves with `#![cfg(feature = ..)]`, which compiles an empty test \
         that reports no tests when the feature is off; list the features in the file's \
         `[[test]]` entry (`required-features`) in crates/navree/Cargo.toml"
    );
}
