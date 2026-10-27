//! The starter kits `navree new --kit` writes build against the workspace's
//! navree, and their own tests and lints pass.
//!
//! The Rust tests are ignored by default: each builds the whole framework in
//! a fresh project (minutes). They run `cargo test`, clippy, the app's
//! `check`, then `conformity:check` and the OpenAPI drift check (`saas`,
//! `api`) or `micro:patterns` (`micro`). Run them with
//!
//! ```text
//! CARGO_TARGET_DIR=target/agents/cli cargo test -p navree-cli --test generated_kits -- --ignored
//! ```
//!
//! The SaaS kit's front end is checked by `saas_kit_web_installs_lints_tests_and_builds`,
//! which runs only with `NAVREE_TEST_KIT_WEB=1` (it needs Node.js 22 and
//! pnpm, and downloads the packages): `pnpm install --frozen-lockfile`, the
//! shipped `lib/api/schema.d.ts` against `openapi-typescript ../openapi.json`,
//! then `pnpm lint`, `pnpm typecheck`, `pnpm test` and `pnpm build` in `web/`.
//!
//! `NAVREE_FACADE` may point at another `navree` facade crate directory
//! (default: the workspace's `crates/navree`). The projects build in
//! `$CARGO_TARGET_DIR/generated-kits` (default `target/generated-kits`),
//! with sccache when it is installed; on failure the project is kept and its
//! path printed. `NAVREE_KEEP_GENERATED=1` keeps it on success too, and
//! `NAVREE_KITS_TARGET_DIR` moves the target directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use navree_cli::clock::Timestamp;
use navree_cli::kits::Kit;
use navree_cli::names::AppName;
use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, create_app};

/// The workspace root. `std::path::absolute`, not `canonicalize`, which
/// gives verbatim `\\?\C:\..` paths on Windows.
fn workspace() -> PathBuf {
    std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")).unwrap()
}

/// Where the generated projects build: one target directory for every kit,
/// so the framework compiles once. `NAVREE_KITS_TARGET_DIR` puts it
/// elsewhere (another disk).
fn target() -> PathBuf {
    if let Some(dir) = std::env::var_os("NAVREE_KITS_TARGET_DIR") {
        return PathBuf::from(dir);
    }
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
        .env(
            "APP_DATABASE__URL",
            "sqlite://storage/kit-check.db?mode=rwc",
        )
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
        database: Database::default(),
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

/// `cargo test` and clippy over the whole project, then the app's `check`
/// (the module graph resolves) and `extra`, an app command.
fn rust_checks(root: &Path, extra: &[&str]) -> bool {
    std::fs::create_dir_all(root.join("storage")).unwrap();
    let mut run = vec!["run", "--quiet", "--"];
    run.extend_from_slice(extra);
    cargo(root, &["test", "--workspace", "--quiet"])
        && cargo(
            root,
            &[
                "clippy",
                "--workspace",
                "--quiet",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        )
        && cargo(root, &["run", "--quiet", "--", "check"])
        && cargo(root, &run)
}

/// Whether the committed `openapi.json` is what the app exports now (the
/// CI workflow's drift check).
fn openapi_is_current(root: &Path) -> bool {
    let committed =
        std::fs::read_to_string(root.join("openapi.json")).expect("the kit ships openapi.json");
    cargo(
        root,
        &[
            "run",
            "--quiet",
            "--",
            "openapi:export",
            "openapi.fresh.json",
        ],
    ) && {
        let fresh = std::fs::read_to_string(root.join("openapi.fresh.json")).unwrap();
        let same = fresh.replace("\r\n", "\n") == committed;
        if !same {
            eprintln!(
                "openapi.json is stale: copy openapi.fresh.json over crates/navree-cli/templates/kits/api/openapi.json.j2 (with the title as `{{{{ title }}}} API`) and regenerate web/lib/api/schema.d.ts"
            );
        }
        same
    }
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn api_kit_compiles_and_its_tests_pass() {
    let (parent, root) = generate(Kit::Api, "acme-api");
    // Every route is documented, and the committed contract is current.
    let ok = rust_checks(&root, &["conformity:check"]) && openapi_is_current(&root);
    finish(parent, ok, "api kit");
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn saas_kit_api_compiles_and_its_tests_pass() {
    let (parent, root) = generate(Kit::Saas, "acme");
    let ok = rust_checks(&root, &["conformity:check"]) && openapi_is_current(&root);
    finish(parent, ok, "saas kit");
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn micro_kit_compiles_and_its_tests_pass() {
    let (parent, root) = generate(Kit::Micro, "billing");
    // A workspace whose default member is the service: `cargo run` runs it,
    // and it lists its message and event patterns.
    let ok = rust_checks(&root, &["micro:patterns"]);
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
    // The committed lockfile installs as it is, and the shipped types are
    // what openapi-typescript makes of the shipped `openapi.json`.
    let ok = pnpm(&web, &["install", "--frozen-lockfile"])
        && pnpm(
            &web,
            &[
                "exec",
                "openapi-typescript",
                "../openapi.json",
                "-o",
                "lib/api/schema.check.d.ts",
            ],
        )
        && {
            let read = |path: &str| {
                std::fs::read_to_string(web.join(path))
                    .unwrap()
                    .replace("\r\n", "\n")
            };
            let same = read("lib/api/schema.check.d.ts") == read("lib/api/schema.d.ts");
            std::fs::remove_file(web.join("lib/api/schema.check.d.ts")).unwrap();
            if !same {
                eprintln!(
                    "web/lib/api/schema.d.ts does not match openapi.json: run `pnpm gen:api`"
                );
            }
            same
        }
        && pnpm(&web, &["lint"])
        && pnpm(&web, &["typecheck"])
        && pnpm(&web, &["test"])
        && pnpm(&web, &["build"]);
    finish(parent, ok, "saas kit's web app");
}
