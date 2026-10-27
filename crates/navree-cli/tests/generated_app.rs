//! The code `navree new` and `navree make:*` generate compiles against the
//! workspace's navree and its tests pass.
//!
//! Ignored by default: it builds the whole framework in a fresh project
//! (minutes). Run it with
//!
//! ```text
//! CARGO_TARGET_DIR=target/agents/cli cargo test -p navree-cli --test generated_app -- --ignored
//! ```
//!
//! `NAVREE_FACADE` may point at another `navree` facade crate directory (default:
//! the workspace's `crates/navree`). The generated project builds in
//! `$CARGO_TARGET_DIR/generated-app` (default `target/generated-app`), with
//! sccache when it is installed; on failure the project is kept and its path
//! printed.

use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::clock::Timestamp;
use navree_cli::make::{Companions, MakeKind, MakeOptions, make_item, make_model, make_module};
use navree_cli::names::{AppName, Name};
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app};
use navree_cli::resource::make_resource;

/// The workspace root. `std::path::absolute`, not `canonicalize`, which
/// gives verbatim `\\?\C:\..` paths on Windows.
fn workspace() -> PathBuf {
    std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")).unwrap()
}

fn name(s: &str) -> Name {
    Name::parse("name", s).unwrap()
}

/// `cargo <args>` in `dir`, with the outer build's job limit and sccache.
///
/// The app is made for TimescaleDB (the default), so it compiles the
/// `postgres` driver; the commands that boot it (`conformity:check`) run on
/// a SQLite file instead, since no database server runs here.
/// `APP_DATABASE__URL` overrides config/app.toml.
fn cargo(dir: &Path, target: &Path, args: &[&str]) -> bool {
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .env("APP_DATABASE__URL", "sqlite://storage/demo.db?mode=rwc")
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
fn navree_new_output_compiles_against_the_workspace() {
    let workspace = workspace();
    let facade = std::env::var_os("NAVREE_FACADE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("crates").join("navree"));
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
        kit: None,
        database: Database::Timescaledb,
        navree: NavreeSource::Path(facade),
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
        ..MakeOptions::default()
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

    make_resource(&root, "src/modules", &name("posts"), "Post").unwrap();

    // ORM models: one with every companion (its controller checks
    // abilities), one with a controller binding records for signed-in users,
    // and a second factory of the first model.
    make_model(
        &root,
        "src/modules",
        &name("posts"),
        &name("comment"),
        Companions::all(),
        now,
    )
    .unwrap();
    let label = Companions {
        migration: true,
        controller: true,
        seeder: true,
        ..Companions::default()
    };
    make_model(
        &root,
        "src/modules",
        &name("tags"),
        &name("label"),
        label,
        now,
    )
    .unwrap();
    let pivot = Companions::all_for_pivot();
    make_model(
        &root,
        "src/modules",
        &name("posts"),
        &name("post_label"),
        pivot,
        now,
    )
    .unwrap();
    let for_comments = MakeOptions {
        model: Some(name("comment")),
        ..MakeOptions::default()
    };
    make_item(
        &root,
        "src/modules",
        MakeKind::Factory,
        &name("posts"),
        &name("pinned_comment"),
        &for_comments,
        now,
    )
    .unwrap();

    let ok = cargo(&root, &target, &["test", "--quiet"])
        && cargo(&root, &target, &["run", "--quiet", "--", "check"])
        // Generated routes are documented: `navree check` passes.
        && cargo(
            &root,
            &target,
            &["run", "--quiet", "--", "conformity:check"],
        )
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
    if std::env::var_os("NAVREE_KEEP_GENERATED").is_some() {
        eprintln!("kept the generated app at {}", parent.keep().display());
    }
}
