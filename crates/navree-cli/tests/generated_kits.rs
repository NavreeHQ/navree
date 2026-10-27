//! The starter kits `navree new --kit` writes build against the workspace's
//! navree, and their own tests and lints pass.
//!
//! The Rust tests are ignored by default: each builds the whole framework in
//! a fresh project (minutes). Run them with
//!
//! ```text
//! CARGO_TARGET_DIR=target/agents/cli cargo test -p navree-cli --test generated_kits -- --ignored
//! ```
//!
//! The SaaS kit's front end is checked by `saas_kit_web_installs_lints_tests_and_builds`,
//! which runs only with `NAVREE_TEST_KIT_WEB=1` (it needs Node.js 22 and
//! pnpm, and downloads the packages): `pnpm install`, `pnpm lint`,
//! `pnpm typecheck`, `pnpm test` and `pnpm build` in `web/`.
//!
//! `NAVREE_FACADE` may point at another `navree` facade crate directory
//! (default: the workspace's `crates/navree`). The projects build in
//! `$CARGO_TARGET_DIR/generated-kits` (default `target/generated-kits`),
//! with sccache when it is installed; on failure the project is kept and its
//! path printed. `NAVREE_KEEP_GENERATED=1` keeps it on success too.

use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::clock::Timestamp;
use navree_cli::kits::Kit;
use navree_cli::names::AppName;
use navree_cli::new_app::{NavreeSource, NewApp, Ui, create_app};

/// The workspace root. `std::path::absolute`, not `canonicalize`, which
/// gives verbatim `\\?\C:\..` paths on Windows.
fn workspace() -> PathBuf {
    std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")).unwrap()
}

/// Where the generated projects build: one target directory for every kit,
/// so the framework compiles once.
fn target() -> PathBuf {
    let workspace = workspace();
    match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => workspace.join(dir),
        None => workspace.join("target"),
    }
    .join("generated-kits")
}

/// `cargo <args>` in `dir`, with the outer build's job limit and sccache.
/// What the app runs (`cargo run -- check`, ...) boots without the
/// services Dock would start: a SQLite file instead of PostgreSQL, and the
/// in-process message transport instead of NATS (environment variables
/// override `.env`).
fn cargo(dir: &Path, args: &[&str]) -> bool {
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target())
        .env("APP_DATABASE__URL", "sqlite://storage/kit-check.db?mode=rwc")
        .env("APP_MICRO__TRANSPORT", "memory")
        .env("APP_MAIL__TRANSPORT", "log")
        .env_remove("RUSTFLAGS");
    let sccache = Command::new("sccache").arg("--version").output();
    if sccache.is_ok_and(|o| o.status.success()) {
        command.env("RUSTC_WRAPPER", "sccache");
    }
    eprintln!("running cargo {} in {}", args.join(" "), dir.display());
    command.status().expect("cargo runs").success()
}

/// Writes `kit` as app `name` into a temporary directory, with the
/// workspace's `Cargo.lock` so dependency versions match.
fn generate(kit: Kit, name: &str) -> (tempfile::TempDir, PathBuf) {
    let facade = std::env::var_os("NAVREE_FACADE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("crates").join("navree"));
    let parent = tempfile::tempdir().unwrap();
    let app = NewApp {
        name: AppName::parse(name).unwrap(),
        ui: Ui::None,
        kit: Some(kit),
        navree: NavreeSource::Path(facade),
    };
    let root = create_app(parent.path(), &app, Timestamp::now()).unwrap();
    std::fs::copy(workspace().join("Cargo.lock"), root.join("Cargo.lock")).unwrap();
    (parent, root)
}

/// Keeps the project when `ok` is false (and panics) or when
/// `NAVREE_KEEP_GENERATED` is set.
fn finish(parent: tempfile::TempDir, ok: bool, what: &str) {
    if !ok {
        let kept = parent.keep();
        panic!("the {what} failed; kept at {}", kept.display());
    }
    if std::env::var_os("NAVREE_KEEP_GENERATED").is_some() {
        eprintln!("kept the {what} at {}", parent.keep().display());
    }
}

/// `cargo test`, then the app's `check` (the module graph resolves) and
/// `conformity:check` (every route is documented), then clippy. A `micro`
/// kit is a workspace whose default member is the service, so `cargo run`
/// runs the service there too.
fn rust_checks(root: &Path) -> bool {
    std::fs::create_dir_all(root.join("storage")).unwrap();
    cargo(root, &["test", "--quiet"])
        && cargo(root, &["run", "--quiet", "--", "check"])
        && cargo(root, &["run", "--quiet", "--", "conformity:check"])
        && cargo(
            root,
            &["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"],
        )
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn api_kit_compiles_and_its_tests_pass() {
    let (parent, root) = generate(Kit::Api, "acme-api");
    let ok = rust_checks(&root)
        && cargo(
            &root,
            &["run", "--quiet", "--", "openapi:export", "openapi.json"],
        )
        && root.join("openapi.json").is_file();
    finish(parent, ok, "api kit");
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn saas_kit_api_compiles_and_its_tests_pass() {
    let (parent, root) = generate(Kit::Saas, "acme");
    let ok = rust_checks(&root);
    finish(parent, ok, "saas kit");
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn micro_kit_compiles_and_its_tests_pass() {
    let (parent, root) = generate(Kit::Micro, "billing");
    let ok = rust_checks(&root);
    finish(parent, ok, "micro kit");
}

/// `pnpm <args>` in `dir`.
fn pnpm(dir: &Path, args: &[&str]) -> bool {
    let program = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };
    eprintln!("running pnpm {} in {}", args.join(" "), dir.display());
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("NEXT_TELEMETRY_DISABLED", "1")
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn saas_kit_web_installs_lints_tests_and_builds() {
    if std::env::var_os("NAVREE_TEST_KIT_WEB").is_none() {
        eprintln!("SKIPPED: set NAVREE_TEST_KIT_WEB=1 to install and build the SaaS kit's web app");
        return;
    }
    let (parent, root) = generate(Kit::Saas, "acme");
    let web = root.join("web");
    let ok = pnpm(&web, &["install"])
        && pnpm(&web, &["lint"])
        && pnpm(&web, &["typecheck"])
        && pnpm(&web, &["test"])
        && pnpm(&web, &["build"]);
    finish(parent, ok, "saas kit's web app");
}
