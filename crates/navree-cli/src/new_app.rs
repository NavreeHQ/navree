//! `navree new <name>`: a new Navree app, ready to run.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

use crate::clock::Timestamp;
use crate::kits::{Kit, kit_files, ui_files};
use crate::names::AppName;
use crate::scaffold::FileSet;
use crate::templates::{TemplateError, render};

/// The front end created next to the API. Navree UI is the default: a new app
/// without `--ui` and without a kit draws its screens on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Ui {
    /// A Next.js app in `web/` that talks to the API through its own server
    /// (backend-for-frontend), keeping the tokens in httpOnly cookies.
    Next,
    /// Screens drawn on the server with Navree UI, in `src/` next to the API:
    /// sign-in with two-factor authentication, workspaces, a customers table
    /// and forms, a home page with live figures. No `web/` folder and no Node.
    #[default]
    Navree,
    /// API only.
    None,
}

/// The email of the account `--ui navree` apps start with in development.
/// `src/ui/auth.rs` of the Navree UI template holds the same value, and a
/// test keeps the two equal.
pub const DEMO_EMAIL: &str = "harpreet.singh@example.com";

/// Its password: for development only, since the seeder that makes the
/// account never runs when `env` is `production`.
pub const DEMO_PASSWORD: &str = "demo-password-2026";

/// The database a new app is set up for: its `database.url`, the driver
/// feature of its `navree` dependency and, for PostgreSQL, the `dock.toml`
/// that runs the server.
///
/// ```
/// use navree_cli::new_app::Database;
///
/// let database = Database::default();
/// assert_eq!(database, Database::Timescaledb);
/// assert_eq!(database.feature(), Some("postgres"));
/// assert_eq!(
///     database.url("my_shop"),
///     "postgres://navree:navree@127.0.0.1:5432/my_shop"
/// );
/// assert_eq!(Database::Sqlite.url("my_shop"), "sqlite://storage/my_shop.db?mode=rwc");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Database {
    /// TimescaleDB, Navree's default: PostgreSQL with time-series
    /// hypertables, run on this machine by `navree dock up`.
    #[default]
    #[value(alias = "timescale")]
    Timescaledb,
    /// Plain PostgreSQL, run by `navree dock up` too.
    #[value(alias = "postgresql")]
    Postgres,
    /// MySQL 8.0+ or MariaDB 10.6+, on a server of your own.
    #[value(alias = "mariadb")]
    Mysql,
    /// SQLite: a file under `storage/`, with nothing to install.
    Sqlite,
}

impl Database {
    /// The facade feature that compiles the database's driver: `postgres`
    /// for TimescaleDB and PostgreSQL, `mysql` for MySQL and MariaDB, and
    /// `sqlite` for SQLite (no driver is compiled unless its feature is on).
    pub fn feature(self) -> Option<&'static str> {
        match self {
            Database::Timescaledb | Database::Postgres => Some("postgres"),
            Database::Mysql => Some("mysql"),
            Database::Sqlite => Some("sqlite"),
        }
    }

    /// The `database.url` of a new app whose crate is `crate_name`: the
    /// database is named after it. The PostgreSQL URL is the one Dock's
    /// `postgres` service gives the app (user and password `navree`), so
    /// `config/app.toml` and `navree dock up` agree.
    pub fn url(self, crate_name: &str) -> String {
        match self {
            Database::Timescaledb | Database::Postgres => {
                format!("postgres://navree:navree@127.0.0.1:5432/{crate_name}")
            }
            Database::Mysql => format!("mysql://navree:navree@127.0.0.1:3306/{crate_name}"),
            Database::Sqlite => format!("sqlite://storage/{crate_name}.db?mode=rwc"),
        }
    }

    /// Whether `navree dock up` runs the server, so the app gets a
    /// `dock.toml` with `[services.postgres]`.
    pub fn runs_in_dock(self) -> bool {
        matches!(self, Database::Timescaledb | Database::Postgres)
    }

    /// The name templates branch on: `timescaledb`, `postgres`, `mysql` or
    /// `sqlite`.
    fn key(self) -> &'static str {
        match self {
            Database::Timescaledb => "timescaledb",
            Database::Postgres => "postgres",
            Database::Mysql => "mysql",
            Database::Sqlite => "sqlite",
        }
    }
}

/// Where the app gets the `navree` crate from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavreeSource {
    /// A local checkout: the directory of the `navree` facade crate.
    Path(PathBuf),
    /// A published version requirement.
    Version(String),
}

impl NavreeSource {
    /// The version requirement used when navree is not found locally: this
    /// CLI's own version, which is the version of the `navree` crate it was
    /// released with.
    pub const DEFAULT_VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// Resolves `--navree-path`: the navree repository (its `crates/navree` is
    /// used) or the facade crate itself. Without it, the repository this
    /// `navree` binary was built from, when it still exists; else the
    /// published version.
    pub fn resolve(flag: Option<&Path>) -> anyhow::Result<Self> {
        match flag {
            Some(path) => {
                let path = std::path::absolute(path)
                    .with_context(|| format!("cannot resolve {}", path.display()))?;
                facade_dir(&path).map(NavreeSource::Path).with_context(|| {
                    format!(
                        "--navree-path {}: expected the navree repository (with crates/navree) or the navree crate itself",
                        path.display()
                    )
                })
            }
            None => {
                let built_from = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("navree");
                Ok(match facade_dir(&built_from) {
                    Some(dir) => NavreeSource::Path(std::path::absolute(&dir).unwrap_or(dir)),
                    None => NavreeSource::Version(Self::DEFAULT_VERSION.to_owned()),
                })
            }
        }
    }

    /// The right-hand side of `navree = ...` in `Cargo.toml`.
    pub fn dependency(&self) -> String {
        match self {
            NavreeSource::Path(path) => format!("{{ path = {} }}", path_value(path)),
            NavreeSource::Version(version) => toml_string(version),
        }
    }

    /// The right-hand side of `navree = ...` with `features` turned on (on
    /// top of the default ones): `{ path = "..", features = ["postgres"] }`,
    /// or [`dependency`](NavreeSource::dependency) when `features` is empty.
    ///
    /// ```
    /// use navree_cli::new_app::NavreeSource;
    ///
    /// let published = NavreeSource::Version("1.0.0".to_owned());
    /// assert_eq!(
    ///     published.dependency_with_features(&["postgres"]),
    ///     r#"{ version = "1.0.0", features = ["postgres"] }"#
    /// );
    /// assert_eq!(published.dependency_with_features(&[]), r#""1.0.0""#);
    /// ```
    pub fn dependency_with_features(&self, features: &[&str]) -> String {
        if features.is_empty() {
            return self.dependency();
        }
        let features = features
            .iter()
            .map(|feature| toml_string(feature))
            .collect::<Vec<_>>()
            .join(", ");
        match self {
            NavreeSource::Path(path) => {
                format!("{{ path = {}, features = [{features}] }}", path_value(path))
            }
            NavreeSource::Version(version) => format!(
                "{{ version = {}, features = [{features}] }}",
                toml_string(version)
            ),
        }
    }

    /// The version range of the `@navree/*` packages that a front end written
    /// by a published CLI depends on.
    pub const JS_RANGE: &'static str = "^1.0.0";

    /// The directory of the Navree JavaScript packages (`packages/js` of the
    /// checkout), with `/` separators, when `navree` comes from a local
    /// checkout. A front end links `@navree/next`, `@navree/client` and the
    /// rest from there until the packages are published; `None` for a
    /// published version, where the front end asks npm for
    /// [`JS_RANGE`](NavreeSource::JS_RANGE).
    ///
    /// ```
    /// use std::path::PathBuf;
    ///
    /// use navree_cli::new_app::NavreeSource;
    ///
    /// let local = NavreeSource::Path(PathBuf::from("/src/navree/crates/navree"));
    /// assert_eq!(local.js_dir().as_deref(), Some("/src/navree/packages/js"));
    /// assert_eq!(NavreeSource::Version("1.0.0".to_owned()).js_dir(), None);
    /// ```
    pub fn js_dir(&self) -> Option<String> {
        match self {
            NavreeSource::Path(facade) => {
                let checkout = facade.parent()?.parent()?;
                let dir = checkout.join("packages").join("js");
                Some(plain_path(&dir.to_string_lossy()).replace('\\', "/"))
            }
            NavreeSource::Version(_) => None,
        }
    }

    /// The right-hand side of `navree = ...` with the default features
    /// off, for `[workspace.dependencies]`: each member turns on what it
    /// uses (`{ path = "..", default-features = false }`).
    pub fn dependency_without_default_features(&self) -> String {
        match self {
            NavreeSource::Path(path) => format!(
                "{{ path = {}, default-features = false }}",
                path_value(path)
            ),
            NavreeSource::Version(version) => format!(
                "{{ version = {}, default-features = false }}",
                toml_string(version)
            ),
        }
    }
}

/// A local path as a TOML string, with `/` separators.
fn path_value(path: &Path) -> String {
    toml_string(&plain_path(&path.to_string_lossy()).replace('\\', "/"))
}

/// `path` itself when it is the `navree` package, or its `crates/navree`.
fn facade_dir(path: &Path) -> Option<PathBuf> {
    let is_facade = |dir: &Path| {
        std::fs::read_to_string(dir.join("Cargo.toml"))
            .ok()
            .and_then(|manifest| manifest.parse::<toml::Table>().ok())
            .and_then(|manifest| {
                manifest
                    .get("package")?
                    .get("name")?
                    .as_str()
                    .map(|name| name == "navree")
            })
            .unwrap_or(false)
    };
    let nested = path.join("crates").join("navree");
    if is_facade(&nested) {
        Some(nested)
    } else if is_facade(path) {
        Some(path.to_path_buf())
    } else {
        None
    }
}

/// `path` without a Windows verbatim prefix (`\\?\C:\x` is `C:\x`,
/// `\\?\UNC\server\share` is `\\server\share`), as `canonicalize()`
/// returns them and cargo rejects them in a `path` dependency.
fn plain_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

/// A TOML basic string.
fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_owned()).to_string()
}

/// What `navree new` creates.
#[derive(Debug, Clone)]
pub struct NewApp {
    /// The package name.
    pub name: AppName,
    /// The front end (the minimal app only; a kit brings its own).
    pub ui: Ui,
    /// The starter kit, or `None` for the minimal app.
    pub kit: Option<Kit>,
    /// The database (TimescaleDB unless `--database` says otherwise). The
    /// kits run on TimescaleDB or plain PostgreSQL.
    pub database: Database,
    /// Where `navree` comes from.
    pub navree: NavreeSource,
}

#[derive(Serialize)]
struct AppContext {
    package: String,
    crate_name: String,
    title: String,
    navree_dependency: String,
    /// `navree` for `[dev-dependencies]`, with the `sqlite` driver the
    /// tests run on.
    navree_test_dependency: String,
    users_migration: String,
    ui_next: bool,
    /// Whether the app draws its screens with Navree UI (`--ui navree`).
    ui_navree: bool,
    /// The email and password of the demo account (`--ui navree`).
    demo_email: &'static str,
    demo_password: &'static str,
    /// Where the `@navree/*` packages are linked from (`packages/js` of the
    /// checkout, `/` separators); `None` when `navree` is a published version.
    navree_js_dir: Option<String>,
    /// The npm range of the `@navree/*` packages for a published version.
    navree_js_range: &'static str,
    /// `timescaledb`, `postgres`, `mysql` or `sqlite`.
    database: &'static str,
    database_url: String,
    /// Whether the app gets a `dock.toml` running its database.
    dock: bool,
}

/// The files of the app, relative to its directory: the kit's when it has
/// one ([`kit_files`]), else the minimal app with an optional front end.
pub fn app_files(app: &NewApp, now: Timestamp) -> Result<FileSet, TemplateError> {
    if let Some(kit) = app.kit {
        return kit_files(app, kit, now);
    }
    let crate_name = app.name.crate_name();
    let ctx = AppContext {
        package: app.name.package().to_owned(),
        title: app.name.title(),
        navree_test_dependency: app.navree.dependency_with_features(&["sqlite"]),
        navree_dependency: app
            .navree
            .dependency_with_features(&app.database.feature().into_iter().collect::<Vec<_>>()),
        users_migration: format!("{}_create_user_profiles_table", now.migration_prefix()),
        ui_next: app.ui == Ui::Next,
        ui_navree: app.ui == Ui::Navree,
        demo_email: DEMO_EMAIL,
        demo_password: DEMO_PASSWORD,
        navree_js_dir: app.navree.js_dir(),
        navree_js_range: NavreeSource::JS_RANGE,
        database: app.database.key(),
        database_url: app.database.url(&crate_name),
        dock: app.database.runs_in_dock(),
        crate_name,
    };
    let users = "src/modules/users";
    let mut files = FileSet::new();
    let env = render("app/env.example", &ctx)?;
    files.add(".env.example", env.clone());
    files.add(".env", env);
    for (template, path) in [
        ("app/gitignore", ".gitignore".to_owned()),
        ("app/Cargo.toml", "Cargo.toml".to_owned()),
        ("app/README.md", "README.md".to_owned()),
        // Conventions for coding agents; CLAUDE.md imports AGENTS.md.
        ("app/AGENTS.md", "AGENTS.md".to_owned()),
        ("app/CLAUDE.md", "CLAUDE.md".to_owned()),
        ("app/config/app.toml", "config/app.toml".to_owned()),
        ("app/src/main.rs", "src/main.rs".to_owned()),
        ("app/src/lib.rs", "src/lib.rs".to_owned()),
        ("app/src/app.rs", "src/app.rs".to_owned()),
        ("app/src/abilities.rs", "src/abilities.rs".to_owned()),
        ("app/src/modules/mod.rs", "src/modules/mod.rs".to_owned()),
        ("app/src/modules/users/mod.rs", format!("{users}/mod.rs")),
        (
            "app/src/modules/users/controller.rs",
            format!("{users}/controller.rs"),
        ),
        (
            "app/src/modules/users/service.rs",
            format!("{users}/service.rs"),
        ),
        (
            "app/src/modules/users/entity.rs",
            format!("{users}/entity.rs"),
        ),
        ("app/src/modules/users/dto.rs", format!("{users}/dto.rs")),
        (
            "app/src/modules/users/migrations/mod.rs",
            format!("{users}/migrations/mod.rs"),
        ),
        (
            "app/src/modules/users/migrations/create_user_profiles_table.rs",
            format!("{users}/migrations/{}.rs", ctx.users_migration),
        ),
        (
            "app/src/modules/users/tests/mod.rs",
            format!("{users}/tests/mod.rs"),
        ),
        ("app/tests/app.rs", "tests/app.rs".to_owned()),
        ("app/storage/gitignore", "storage/.gitignore".to_owned()),
    ] {
        files.add(path, render(template, &ctx)?);
    }
    if ctx.dock {
        files.add("dock.toml", render("app/dock.toml", &ctx)?);
    }
    if ctx.ui_next {
        for (template, path) in [
            ("web/package.json", "web/package.json"),
            ("web/pnpm-workspace.yaml", "web/pnpm-workspace.yaml"),
            ("web/tsconfig.json", "web/tsconfig.json"),
            ("web/next.config.ts", "web/next.config.ts"),
            ("web/env.example", "web/.env.example"),
            ("web/README.md", "web/README.md"),
            ("web/lib/navree.ts", "web/lib/navree.ts"),
            ("web/proxy.ts", "web/proxy.ts"),
            ("web/app/layout.tsx", "web/app/layout.tsx"),
            ("web/app/page.tsx", "web/app/page.tsx"),
            ("web/app/sign-out.tsx", "web/app/sign-out.tsx"),
            ("web/app/login/page.tsx", "web/app/login/page.tsx"),
            (
                "web/app/api/proxy/route.ts",
                "web/app/api/[...path]/route.ts",
            ),
        ] {
            files.add(path, render(template, &ctx)?);
        }
    }
    if ctx.ui_navree {
        for file in ui_files(app, now)?.iter() {
            files.add(file.path.clone(), file.contents.clone());
        }
    }
    Ok(files)
}

/// What `navree new` prints once the app is written to `root`: where it
/// is, and the next steps for its kit, database and front end.
///
/// ```
/// use std::path::{Path, PathBuf};
///
/// use navree_cli::names::AppName;
/// use navree_cli::new_app::{Database, NavreeSource, NewApp, Ui, created_message};
///
/// let app = NewApp {
///     name: AppName::parse("my-shop").unwrap(),
///     ui: Ui::None,
///     kit: None,
///     database: Database::Timescaledb,
///     navree: NavreeSource::Version("1.0.0".to_owned()),
/// };
/// let message = created_message(&app, Path::new("/src/my-shop"));
/// assert!(message.contains("navree dock up"));
/// ```
pub fn created_message(app: &NewApp, root: &Path) -> String {
    let package = app.name.package();
    let mut out = match app.kit {
        Some(kit) => format!("Created {package} in {} (kit: {kit})\n", root.display()),
        None => format!("Created {package} in {}\n", root.display()),
    };
    if let NavreeSource::Path(path) = &app.navree {
        out.push_str(&format!("It depends on navree at {}.\n", path.display()));
    }
    // Until they are published, the front end links the Navree packages from
    // the checkout, so they are installed and built there once.
    let js_steps = app.navree.js_dir().map(|dir| {
        (
            format!("pnpm --dir {dir} install"),
            format!("pnpm --dir {dir} build"),
        )
    });
    let timescale = app.database == Database::Timescaledb;
    let (steps, width): (Vec<(&str, &str)>, usize) = match app.kit {
        Some(kit) => {
            let services = match (kit, timescale) {
                (Kit::Micro, true) => "TimescaleDB and NATS",
                (Kit::Micro, false) => "PostgreSQL and NATS",
                (_, true) => "TimescaleDB and Mailpit",
                (_, false) => "PostgreSQL and Mailpit",
            };
            let mut steps = vec![("navree dock up", services)];
            match kit {
                Kit::Saas => {
                    steps.push(("navree migrate:fresh --seed", "tables and a demo workspace"));
                    if let Some((install, build)) = &js_steps {
                        steps.push((install.as_str(), "the Navree packages (not on npm yet)"));
                        steps.push((build.as_str(), "once; the web app links them"));
                    }
                    steps.push(("pnpm --dir web install", "the web app's packages"));
                    steps.push(("navree dev", "API on :8080, web app on :3000"));
                }
                Kit::Api => steps.extend_from_slice(&[
                    ("navree migrate:fresh --seed", "tables and a demo workspace"),
                    ("navree dev", "API on :8080, reference at /api/docs"),
                ]),
                Kit::Micro => steps.extend_from_slice(&[
                    ("navree migrate", "the invoices table"),
                    ("navree dev", "handlers on NATS, health on :8080"),
                ]),
            }
            (steps, 30)
        }
        None => {
            let database = match app.database {
                Database::Timescaledb => Some(("navree dock up", "TimescaleDB on 127.0.0.1:5432")),
                Database::Postgres => Some(("navree dock up", "PostgreSQL on 127.0.0.1:5432")),
                Database::Mysql | Database::Sqlite => None,
            };
            let mut steps: Vec<(&str, &str)> = database.into_iter().collect();
            if app.ui == Ui::Next {
                if let Some((install, build)) = &js_steps {
                    steps.push((install.as_str(), "the Navree packages (not on npm yet)"));
                    steps.push((build.as_str(), "once; the front end links them"));
                }
                steps.push((
                    "pnpm --dir web install",
                    "the front end; `navree dev` starts it",
                ));
            }
            if app.ui == Ui::Navree {
                steps.push(("navree dev", "http://127.0.0.1:8080"));
            } else {
                steps.push(("navree dev", "http://127.0.0.1:8080/api/docs"));
            }
            (steps, 26)
        }
    };
    out.push_str("\nNext steps:\n");
    out.push_str(&format!("  cd {package}\n"));
    for (command, comment) in steps {
        out.push_str(&format!("  {command:<width$} # {comment}\n"));
    }
    if app.kit.is_none() && app.ui == Ui::Navree {
        out.push_str(&format!(
            "\nSign in at http://127.0.0.1:8080/sign-in as {DEMO_EMAIL} with the password {DEMO_PASSWORD}.\nThe account exists in development only.\n"
        ));
    }
    if app.kit.is_none() && app.database == Database::Mysql {
        out.push_str(&format!(
            "\nThe app connects to a MySQL or MariaDB server at {}: point `database.url` in config/app.toml (or APP_DATABASE__URL) at yours.\n",
            app.database.url(&app.name.crate_name())
        ));
    }
    out
}

/// What [`init_repository`] found or did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repository {
    /// `git init` made the app a repository of its own.
    Created,
    /// The app is already inside a repository (its own or an enclosing
    /// one).
    Inside,
    /// Git is not available (or could not initialize the directory).
    NoGit,
}

/// Makes the app at `root` a Git repository unless it is inside one
/// already, or Git is missing. Tools that read `.gitignore` need a
/// repository to do it: Tailwind CSS 4 scans every file below the app,
/// `node_modules` included, when there is none. Nothing is committed.
pub fn init_repository(root: &Path) -> Repository {
    use std::process::{Command, Stdio};
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
    };
    match git(&["rev-parse", "--is-inside-work-tree"]) {
        Err(_) => Repository::NoGit,
        Ok(out) if out.status.success() && out.stdout.starts_with(b"true") => Repository::Inside,
        Ok(_) => match git(&["init", "-q"]) {
            Ok(out) if out.status.success() => Repository::Created,
            _ => Repository::NoGit,
        },
    }
}

/// Writes the Harbor secrets key of an app that ships a `harbor.toml` (the
/// starter kits), so `navree harbor:secrets` works right away, and keeps it
/// out of Git. An app without `harbor.toml` is left alone.
pub fn write_harbor_key(root: &Path) -> anyhow::Result<()> {
    if root.join("harbor.toml").is_file() {
        navree_harbor::init::ensure_key(root, &navree_harbor::Environment::production())
            .context("cannot write harbor.key")?;
    }
    Ok(())
}

/// Creates the app in `parent/<name>` and returns its directory. The
/// directory must not exist or be empty.
pub fn create_app(parent: &Path, app: &NewApp, now: Timestamp) -> anyhow::Result<PathBuf> {
    if let Some(kit) = app.kit
        && app.ui == Ui::Navree
    {
        bail!(
            "--ui navree goes with the minimal app: the {kit} kit brings its own front end (or none), so leave out --kit, or leave out --ui navree"
        );
    }
    if let Some(kit) = app.kit
        && app.ui == Ui::Next
        && !kit.has_web()
    {
        bail!(
            "the {kit} kit has no front end: --ui next goes with --kit saas (whose front end is Next.js) or with no kit"
        );
    }
    if let Some(kit) = app.kit
        && !app.database.runs_in_dock()
    {
        bail!(
            "the {kit} kit runs on PostgreSQL: --database timescaledb (the default) or postgres, or no kit for {}",
            app.database.key()
        );
    }
    let root = parent.join(app.name.package());
    if root.exists() {
        let mut entries =
            std::fs::read_dir(&root).with_context(|| format!("cannot read {}", root.display()))?;
        if entries.next().is_some() {
            bail!("{} already exists and is not empty", root.display());
        }
    }
    app_files(app, now)?.write(&root)?;
    // `.env` is this machine's copy: it gets a key of its own. `.env.example`
    // keeps `APP_KEY=` empty.
    crate::app_key::write_app_key(&root)?;
    Ok(root)
}
