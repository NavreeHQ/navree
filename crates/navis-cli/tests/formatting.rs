//! Generated Rust is already rustfmt-formatted for typical names, so apps
//! look right even where `navis` cannot run `rustfmt` afterwards. (Migration
//! sets are the exception: they stay one call per line so `make:migration`
//! can extend them.) Skipped when `rustfmt` is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use navis_cli::clock::Timestamp;
use navis_cli::make::{MakeKind, MakeOptions, make_item, make_module};
use navis_cli::names::{AppName, Name};
use navis_cli::new_app::{NavisSource, NewApp, Ui, create_app};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn name(s: &str) -> Name {
    Name::parse("name", s).unwrap()
}

#[test]
fn generated_rust_is_rustfmt_clean() {
    if Command::new("rustfmt").arg("--version").output().is_err() {
        eprintln!("rustfmt is not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let now = Timestamp::from_unix(1_792_938_605);
    let app = NewApp {
        name: AppName::parse("blog").unwrap(),
        ui: Ui::None,
        navis: NavisSource::Path(PathBuf::from("/opt/navis/crates/navis")),
    };
    let root = create_app(dir.path(), &app, now).unwrap();
    make_module(&root, "src/modules", &name("posts"), true, now).unwrap();
    make_module(&root, "src/modules", &name("tags"), false, now).unwrap();
    for kind in MakeKind::ALL {
        let item = match kind {
            MakeKind::Migration => "create_drafts_table",
            _ => "draft",
        };
        make_item(
            &root,
            "src/modules",
            kind,
            &name("posts"),
            &name(item),
            &MakeOptions::default(),
            now,
        )
        .unwrap();
    }
    make_item(
        &root,
        "src/modules",
        MakeKind::Event,
        &name("tags"),
        &name("tag_added"),
        &MakeOptions::default(),
        now,
    )
    .unwrap();
    let with_event = MakeOptions {
        event: Some(name("tag_added")),
    };
    make_item(
        &root,
        "src/modules",
        MakeKind::Listener,
        &name("tags"),
        &name("count_tags"),
        &with_event,
        now,
    )
    .unwrap();

    let mut files = Vec::new();
    rust_files(&root, &mut files);
    let mut unformatted = Vec::new();
    for file in files {
        let output = Command::new("rustfmt")
            .args([
                "--edition",
                "2024",
                "--check",
                "--config",
                "skip_children=true",
            ])
            .arg(&file)
            .output()
            .unwrap();
        let migration_set = file.ends_with(Path::new("migrations").join("mod.rs"));
        if !output.status.success() && !migration_set {
            unformatted.push(format!(
                "{}\n{}",
                file.strip_prefix(&root).unwrap().display(),
                String::from_utf8_lossy(&output.stdout)
            ));
        }
    }
    assert!(
        unformatted.is_empty(),
        "not rustfmt-clean:\n{}",
        unformatted.join("\n")
    );
}
