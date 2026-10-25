//! The code `keel new` and `keel make:*` generate compiles against the
//! workspace's keel and its tests pass.
//!
//! Ignored by default: it builds the whole framework in a fresh project
//! (minutes). Run it with
//!
//! ```text
//! CARGO_TARGET_DIR=target/agents/cli cargo test -p keel-cli --test generated_app -- --ignored
//! ```
//!
//! `KEEL_FACADE` may point at another `keel` facade crate directory (default:
//! the workspace's `crates/keel`). The generated project builds in
//! `$CARGO_TARGET_DIR/generated-app` (default `target/generated-app`), with
//! sccache when it is installed; on failure the project is kept and its path
//! printed.

use std::path::{Path, PathBuf};
use std::process::Command;

use keel_cli::clock::Timestamp;
use keel_cli::make::{MakeKind, MakeOptions, make_item, make_module};
use keel_cli::names::{AppName, Name};
use keel_cli::new_app::{KeelSource, NewApp, Ui, create_app};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn name(s: &str) -> Name {
    Name::parse("name", s).unwrap()
}

/// `cargo <args>` in `dir`, with the outer build's job limit and sccache.
fn cargo(dir: &Path, target: &Path, args: &[&str]) -> bool {
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .env_remove("RUSTFLAGS");
    let sccache = Command::new("sccache").arg("--version").output();
    if sccache.is_ok_and(|o| o.status.success()) {
        command.env("RUSTC_WRAPPER", "sccache");
    }
    eprintln!("running cargo {} in {}", args.join(" "), dir.display());
    command.status().expect("cargo runs").success()
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn keel_new_output_compiles_against_the_workspace() {
    let workspace = workspace();
    let facade = std::env::var_os("KEEL_FACADE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("crates").join("keel"));
    let target = match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => workspace.join(dir),
        None => workspace.join("target"),
    }
    .join("generated-app");

    let parent = tempfile::tempdir().unwrap();
    let now = Timestamp::now();
    let app = NewApp {
        name: AppName::parse("demo").unwrap(),
        ui: Ui::None,
        keel: KeelSource::Path(facade),
    };
    let root = create_app(parent.path(), &app, now).unwrap();
    // Pin dependency versions to the workspace's.
    std::fs::copy(workspace.join("Cargo.lock"), root.join("Cargo.lock")).unwrap();

    make_module(&root, "src/modules", &name("posts"), true, now).unwrap();
    make_module(&root, "src/modules", &name("tags"), false, now).unwrap();
    let none = MakeOptions::default();
    for (kind, item) in [
        (MakeKind::Controller, "admin"),
        (MakeKind::Service, "pricing"),
        (MakeKind::Entity, "draft"),
        (MakeKind::Migration, "create_drafts_table"),
        (MakeKind::Job, "publish_post"),
        (MakeKind::Event, "post_published"),
        (MakeKind::Listener, "log_everything"),
        (MakeKind::Mail, "post_digest"),
        (MakeKind::Notification, "post_liked"),
        (MakeKind::Policy, "post"),
        (MakeKind::Command, "prune_drafts"),
        (MakeKind::Test, "publishing"),
    ] {
        make_item(
            &root,
            "src/modules",
            kind,
            &name("posts"),
            &name(item),
            &none,
            now,
        )
        .unwrap();
    }
    let with_event = MakeOptions {
        event: Some(name("post_published")),
    };
    make_item(
        &root,
        "src/modules",
        MakeKind::Listener,
        &name("posts"),
        &name("notify_author"),
        &with_event,
        now,
    )
    .unwrap();
    make_item(
        &root,
        "src/modules",
        MakeKind::Migration,
        &name("tags"),
        &name("add_color_to_tags"),
        &none,
        now,
    )
    .unwrap();

    let ok = cargo(&root, &target, &["test", "--quiet"])
        && cargo(&root, &target, &["run", "--quiet", "--", "check"])
        && cargo(
            &root,
            &target,
            &["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"],
        );
    if !ok {
        let kept = parent.keep();
        panic!(
            "the generated app failed to build, test or lint; kept at {}",
            kept.display()
        );
    }
    if std::env::var_os("KEEL_KEEP_GENERATED").is_some() {
        eprintln!("kept the generated app at {}", parent.keep().display());
    }
}
