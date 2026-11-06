//! The starter kits `navree new --kit` writes build against the workspace's
//! navree, and their own tests and lints pass.
//!
//! The Rust tests are ignored by default: each builds the whole framework in
//! a fresh project (minutes). They run `cargo test`, clippy, the app's
//! `check`, then `conformity:check` and the OpenAPI drift check (`saas`,
//! `api`) or `micro:patterns` (`micro`). Run them with
//!
//! ```text
//! CARGO_TARGET_DIR=target/builds/cli cargo test -p navree-cli --test generated_kits -- --ignored
//! ```
//!
//! The SaaS kit's front end is checked by `saas_kit_web_installs_lints_tests_and_builds`,
//! which runs only with `NAVREE_TEST_KIT_WEB=1` (it needs Node.js 22 and
//! pnpm, and downloads the packages). The web app links the Navree packages
//! (`@navree/next`, `@navree/client`) from the checkout, so the test installs
//! and builds `packages/js` first. Then in `web/`: `pnpm install` (not frozen:
//! the shipped lockfile pins the other packages and the first install adds the
//! two links), the shipped `lib/api/schema.d.ts` against what
//! `navree openapi:client` writes for `../openapi.json`, then `pnpm lint`,
//! `pnpm typecheck`, `pnpm test` and `pnpm build`.
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

/// The feature the commands that boot an app turn on: the kits compile only
/// the `postgres` driver, and `cargo run --features navree/sqlite` adds
/// SQLite for a run on a scratch file (their tests have SQLite through
/// `[dev-dependencies]`).
const SQLITE: &str = "navree/sqlite";

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

/// The `navree` facade crate the projects depend on.
fn facade() -> PathBuf {
    std::env::var_os("NAVREE_FACADE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("crates").join("navree"))
}

/// Writes `kit` as app `name` into a temporary directory, with the
/// workspace's `Cargo.lock` so dependency versions match.
fn generate(kit: Kit, name: &str) -> (tempfile::TempDir, PathBuf) {
    let facade = facade();
    let parent = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
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
    let mut run = vec!["run", "--quiet", "--features", SQLITE, "--"];
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
        && cargo(
            root,
            &["run", "--quiet", "--features", SQLITE, "--", "check"],
        )
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
            "--features",
            SQLITE,
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
    let (parent, root) = generate(Kit::Api, "agra-api");
    // Every route is documented, and the committed contract is current.
    let ok = rust_checks(&root, &["conformity:check"]) && openapi_is_current(&root);
    finish(parent, ok, "api kit");
}

#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn saas_kit_api_compiles_and_its_tests_pass() {
    let (parent, root) = generate(Kit::Saas, "agra");
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
    let (parent, root) = generate(Kit::Saas, "agra");
    let web = root.join("web");
    // The packages the web app links are installed and built where it links them from.
    let packages = PathBuf::from(
        NavreeSource::Path(facade())
            .js_dir()
            .expect("a local checkout has packages/js"),
    );
    // The shipped lockfile pins the other packages and installs as it is; the first
    // install adds the links to the two packages (so it is not frozen). The shipped
    // types are what `navree openapi:client` makes of the shipped `openapi.json`.
    let ok = pnpm(&packages, &["install", "--frozen-lockfile"])
        && pnpm(&packages, &["build"])
        && pnpm(&web, &["install", "--no-frozen-lockfile"])
        && {
            let read = |path: &Path| std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
            let generated = navree_cli::openapi_client::generate(&read(&root.join("openapi.json")))
                .expect("the shipped openapi.json is a document the generator reads")
                .schema;
            let same = generated == read(&web.join("lib/api/schema.d.ts"));
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

/// Writes the minimal app with `--ui navree` as app `name`, with the
/// workspace's `Cargo.lock` so dependency versions match.
fn generate_navree_ui(name: &str) -> (tempfile::TempDir, PathBuf) {
    let parent = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let app = NewApp {
        name: AppName::parse(name).unwrap(),
        ui: Ui::Navree,
        kit: None,
        database: Database::default(),
        navree: NavreeSource::Path(facade()),
    };
    let root = create_app(parent.path(), &app, Timestamp::now()).unwrap();
    std::fs::copy(workspace().join("Cargo.lock"), root.join("Cargo.lock")).unwrap();
    (parent, root)
}

/// The app with screens drawn by Navree UI compiles, passes clippy and its own
/// tests (the whole journey over HTTP, `tests/journey.rs`), its module graph
/// resolves, and every route is documented or allowed by `api.undocumented`.
#[test]
#[ignore = "builds the whole framework in a new project; run with --ignored"]
fn navree_ui_kit_compiles_and_its_tests_pass() {
    let (parent, root) = generate_navree_ui("platform");
    let ok = rust_checks(&root, &["conformity:check"]);
    finish(parent, ok, "navree ui kit");
}

/// A free port on the loopback address.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("its address")
        .port()
}

/// The status line's code of `GET path` from the app on `port`, or `None` while it
/// does not answer. Plain HTTP over a socket: no client library is needed.
fn status_of(port: u16, path: &str) -> Option<u16> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).ok()?;
    String::from_utf8_lossy(&bytes)
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// The app process this test started. Dropping it stops that process, by the
/// handle `spawn` gave: nothing else is touched.
struct Running {
    child: std::process::Child,
}

impl Drop for Running {
    fn drop(&mut self) {
        eprintln!("stopping the app (process {})", self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the built app on `port` with SQLite and `APP_*` settings, and waits
/// until `/health/live` answers 200.
fn start_app(root: &Path, binary: &Path, port: u16, key: &str) -> Option<Running> {
    let child = Command::new(binary)
        .current_dir(root)
        .env("APP_KEY", key)
        .env("APP_HTTP__ADDR", format!("127.0.0.1:{port}"))
        .env("APP_DATABASE__URL", "sqlite://storage/browser.db?mode=rwc")
        .env("APP_MAIL__TRANSPORT", "log")
        .env("APP_LOGBOOK__ENABLED", "false")
        // The browser tests send every request from one address, and keep
        // event streams open from it.
        .env("APP_HTTP__PER_IP_RATE_LIMIT", "100000000")
        .env("APP_HTTP__SERVER__MAX_CONNECTIONS_PER_IP", "10000")
        .spawn()
        .expect("the app starts");
    eprintln!("started the app on port {port} (process {})", child.id());
    let mut running = Running { child };
    for _ in 0..240 {
        if let Ok(Some(status)) = running.child.try_wait() {
            eprintln!("the app stopped at once: {status}");
            return None;
        }
        if status_of(port, "/health/live") == Some(200) {
            return Some(running);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    eprintln!("the app did not answer /health/live in two minutes");
    None
}

/// Builds the app, starts it, runs the browser journey against it and stops it.
fn journey_in_a_browser(root: &Path) -> bool {
    let e2e = workspace().join("e2e").join("ui");
    if !e2e.join("node_modules").join("playwright").is_dir() {
        eprintln!(
            "Playwright is not installed: run `npm ci` and `npx playwright install chromium` in {}",
            e2e.display()
        );
        return false;
    }
    std::fs::create_dir_all(root.join("storage")).unwrap();
    if !cargo(root, &["build", "--quiet", "--features", SQLITE]) {
        return false;
    }
    let binary = target()
        .join("debug")
        .join(format!("platform{}", std::env::consts::EXE_SUFFIX));
    // A key for the session cookie: the app prints a new one.
    let printed = Command::new(&binary)
        .arg("key:generate")
        .current_dir(root)
        .env("APP_DATABASE__URL", "sqlite://storage/browser.db?mode=rwc")
        .env("APP_MAIL__TRANSPORT", "log")
        .output()
        .expect("the app runs");
    let key = String::from_utf8_lossy(&printed.stdout)
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("base64:"))
        .map(str::to_owned);
    let Some(key) = key else {
        eprintln!(
            "key:generate printed no key:\n{}{}",
            String::from_utf8_lossy(&printed.stdout),
            String::from_utf8_lossy(&printed.stderr)
        );
        return false;
    };
    let port = free_port();
    let Some(running) = start_app(root, &binary, port, &key) else {
        return false;
    };
    let status = Command::new("node")
        .args(["--test", "--test-concurrency=1", "tests/platform.test.mjs"])
        .current_dir(&e2e)
        .env("PLATFORM_URL", format!("http://127.0.0.1:{port}"))
        .status();
    drop(running);
    status.is_ok_and(|status| status.success())
}

/// The screens in real browsers: sign-in, the customers table with search,
/// filters and sort, the form, a second window that sees the first one's change
/// live, and sign-out (`e2e/ui/tests/platform.test.mjs`). It builds the app,
/// starts it on a free port with SQLite, runs the browser tests against it and
/// stops the one process it started. It runs only with `NAVREE_TEST_KIT_WEB=1`,
/// and needs Node.js 22 with `npm ci` and `npx playwright install chromium`
/// run in `e2e/ui` once; `PLATFORM_BROWSERS` (`chromium,firefox,webkit`) picks
/// the engines.
#[test]
#[ignore = "builds the app and drives it in browsers; needs NAVREE_TEST_KIT_WEB=1"]
fn navree_ui_kit_journey_in_a_browser() {
    if std::env::var_os("NAVREE_TEST_KIT_WEB").is_none() {
        eprintln!(
            "SKIPPED: set NAVREE_TEST_KIT_WEB=1 to build the Navree UI kit and drive it in browsers"
        );
        return;
    }
    let (parent, root) = generate_navree_ui("platform");
    let ok = journey_in_a_browser(&root);
    finish(parent, ok, "navree ui kit's browser journey");
}
