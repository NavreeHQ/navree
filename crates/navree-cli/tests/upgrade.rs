//! `navree upgrade`: fixture-pair tests per rule group, idempotency, `--check`
//! mode, crate-vs-facade path selection, foreign collision safety, and CLI exit codes.

use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::upgrade::{self, TargetCrate};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("upgrade")
}

fn read_fixture(stage: &str, file: &str) -> String {
    let path = fixtures_dir().join(stage).join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn assert_fixture_pair(file: &str) {
    let dir = tempfile::Builder::new()
        .prefix("navree-upgrade-")
        .tempdir()
        .unwrap();
    let before = read_fixture("before", file);
    let expected = read_fixture("after", file);

    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let target_file = src_dir.join(file);
    std::fs::write(&target_file, &before).unwrap();

    let report = upgrade::run(dir.path(), false).unwrap();
    assert_eq!(report.relative_paths(), [format!("src/{file}")]);
    let actual = std::fs::read_to_string(&target_file).unwrap();
    assert_eq!(actual, expected, "fixture mismatch for {file}");
}

#[test]
fn paths_and_types_move_to_navree_datetime() {
    assert_fixture_pair("paths_and_types.rs");
}

#[test]
fn constructors_sentinels_and_weekdays_rewrite() {
    assert_fixture_pair("constructors.rs");
}

#[test]
fn methods_rfc3339_and_pattern_arguments_rewrite() {
    assert_fixture_pair("methods_and_text.rs");
}

#[test]
fn orm_timestamp_trait_renames_to_has_timestamps() {
    assert_fixture_pair("orm_timestamps.rs");
}

#[test]
fn ambiguous_sites_receive_upgrade_notes() {
    assert_fixture_pair("notes.rs");
}

#[test]
fn a_second_run_changes_nothing() {
    let dir = tempfile::Builder::new()
        .prefix("navree-upgrade-idempotent-")
        .tempdir()
        .unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // Include an app manifest with `chrono` and all five fixture files (including `notes.rs`
    // where unresolved `chrono` items and `// navree upgrade:` comments remain).
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"kesar-mandi\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = \"1.5.0\"\nchrono = { version = \"0.4\", features = [\"serde\"] }\n",
    )
    .unwrap();

    let files = [
        "paths_and_types.rs",
        "constructors.rs",
        "methods_and_text.rs",
        "orm_timestamps.rs",
        "notes.rs",
    ];
    for file in files {
        std::fs::write(src_dir.join(file), read_fixture("before", file)).unwrap();
    }

    let first = upgrade::run(dir.path(), false).unwrap();
    assert_eq!(first.files.len(), files.len());
    assert!(first.rewrites > 0);
    assert_eq!(first.notes, 4);

    let snapshot_after_first: Vec<(String, String)> = files
        .iter()
        .map(|f| {
            (
                (*f).to_owned(),
                std::fs::read_to_string(src_dir.join(f)).unwrap(),
            )
        })
        .collect();
    let cargo_after_first = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();

    let second = upgrade::run(dir.path(), false).unwrap();
    assert!(
        second.is_empty(),
        "expected no changes on second run, got: {:?}",
        second.files
    );
    assert_eq!(second.rewrites, 0);
    assert_eq!(second.notes, 0);

    for (file, expected) in snapshot_after_first {
        let current = std::fs::read_to_string(src_dir.join(&file)).unwrap();
        assert_eq!(current, expected, "{file} drifted on second run");
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(),
        cargo_after_first
    );
}

#[test]
fn check_only_changes_no_file_and_lists_what_it_would() {
    let dir = tempfile::Builder::new()
        .prefix("navree-upgrade-check-")
        .tempdir()
        .unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    let manifest_before = "[package]\nname = \"jaipur-bazaar\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = \"1.5.0\"\nchrono = { version = \"0.4\", features = [\"serde\"] }\nserde = \"1\"\n";
    std::fs::write(dir.path().join("Cargo.toml"), manifest_before).unwrap();

    let constructors_before = read_fixture("before", "constructors.rs");
    let notes_before = read_fixture("before", "notes.rs");
    std::fs::write(src_dir.join("constructors.rs"), &constructors_before).unwrap();
    std::fs::write(src_dir.join("notes.rs"), &notes_before).unwrap();

    // `notes.rs` still keeps `chrono` until disambiguated, so test a clean app too.
    let clean_dir = tempfile::Builder::new()
        .prefix("navree-upgrade-check-clean-")
        .tempdir()
        .unwrap();
    std::fs::create_dir_all(clean_dir.path().join("src")).unwrap();
    std::fs::write(clean_dir.path().join("Cargo.toml"), manifest_before).unwrap();
    std::fs::write(
        clean_dir.path().join("src").join("constructors.rs"),
        &constructors_before,
    )
    .unwrap();

    let report = upgrade::run(clean_dir.path(), true).unwrap();
    assert!(report.check_only);
    assert_eq!(
        report.relative_paths(),
        ["Cargo.toml", "src/constructors.rs"]
    );
    assert!(report.rewrites > 0);

    // Nothing on disk was touched.
    assert_eq!(
        std::fs::read_to_string(clean_dir.path().join("Cargo.toml")).unwrap(),
        manifest_before
    );
    assert_eq!(
        std::fs::read_to_string(clean_dir.path().join("src").join("constructors.rs")).unwrap(),
        constructors_before
    );

    // Also verify note counts are reported in `--check` mode without touching files.
    let notes_report = upgrade::run(dir.path(), true).unwrap();
    assert_eq!(
        notes_report.relative_paths(),
        ["src/constructors.rs", "src/notes.rs"]
    );
    assert_eq!(notes_report.notes, 4);
    assert_eq!(
        std::fs::read_to_string(src_dir.join("notes.rs")).unwrap(),
        notes_before
    );
}

#[test]
fn internal_crates_use_navree_datetime_and_respect_foreign_timestamp_imports() {
    let dir = tempfile::Builder::new()
        .prefix("navree-upgrade-internal-")
        .tempdir()
        .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"navree-sample\"\nversion = \"1.5.0\"\nedition = \"2024\"\n\n[dependencies]\nchrono = { workspace = true }\nuuid = { workspace = true }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src").join("lib.rs"),
        "use chrono::{DateTime, NaiveDate, Utc};\nuse uuid::timestamp::Timestamp;\n\npub fn stamp(day: NaiveDate) -> DateTime<Utc> {\n    let _ = day;\n    Utc::now()\n}\n",
    )
    .unwrap();

    let report = upgrade::run(dir.path(), false).unwrap();
    assert_eq!(report.relative_paths(), ["Cargo.toml", "src/lib.rs"]);

    let rewritten_rs = std::fs::read_to_string(dir.path().join("src").join("lib.rs")).unwrap();
    assert_eq!(
        rewritten_rs,
        "use navree_datetime::Date;\nuse uuid::timestamp::Timestamp;\n\npub fn stamp(day: Date) -> navree_datetime::Timestamp {\n    let _ = day;\n    navree_datetime::Timestamp::now()\n}\n"
    );

    let rewritten_toml = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(rewritten_toml.contains("navree-datetime = { workspace = true }"));
    assert!(!rewritten_toml.contains("chrono"));

    // Direct `rewrite_source` API works on strings too.
    let direct = upgrade::rewrite_source(
        "use chrono::Utc;\npub fn now() { let _ = Utc::now(); }\n",
        TargetCrate::Direct,
    )
    .unwrap();
    assert_eq!(
        direct.source,
        "use navree_datetime::Timestamp;\npub fn now() { let _ = Timestamp::now(); }\n"
    );
}

#[test]
fn binary_upgrade_command_honors_check_flag() {
    let dir = tempfile::Builder::new()
        .prefix("navree-upgrade-bin-")
        .tempdir()
        .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"amritsar-ledger\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = \"1.5.0\"\nchrono = \"0.4\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src").join("main.rs"),
        "use chrono::{DateTime, Utc};\n\nfn main() {\n    let _now: DateTime<Utc> = Utc::now();\n}\n",
    )
    .unwrap();

    let check_first = Command::new(env!("CARGO_BIN_EXE_navree"))
        .args(["upgrade", "--check"])
        .current_dir(dir.path())
        .output()
        .expect("navree runs");
    assert_eq!(check_first.status.code(), Some(1));
    let out = String::from_utf8_lossy(&check_first.stdout);
    assert!(out.contains("would update Cargo.toml"), "{out}");
    assert!(out.contains("would update src/main.rs"), "{out}");

    let apply = Command::new(env!("CARGO_BIN_EXE_navree"))
        .arg("upgrade")
        .current_dir(dir.path())
        .output()
        .expect("navree runs");
    assert!(apply.status.success());

    let check_second = Command::new(env!("CARGO_BIN_EXE_navree"))
        .args(["upgrade", "--check"])
        .current_dir(dir.path())
        .output()
        .expect("navree runs");
    assert!(check_second.status.success());
}
