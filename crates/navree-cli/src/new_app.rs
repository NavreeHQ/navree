//! `navree new <name>`: a new Navree app, ready to run.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

use crate::clock::Timestamp;
use crate::kits::{Kit, kit_files};
use crate::names::AppName;
use crate::scaffold::FileSet;
use crate::templates::{TemplateError, render};

/// The front end created next to the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Ui {
    /// A Next.js app in `web/` that talks to the API through its own server
    /// (backend-for-frontend), keeping the tokens in httpOnly cookies.
    Next,
    /// API only.
    #[default]
    None,
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
    /// The version requirement used when navree is not found locally.
    pub const DEFAULT_VERSION: &'static str = "0.1.0";

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
    /// top of the default ones): `{ path = "..", features = ["billing"] }`.
    pub fn dependency_with_features(&self, features: &[&str]) -> String {
        if features.is_empty() {
            return self.dependency();
        }
        let features = features
            .iter()
            .map(|f| toml_string(f))
            .collect::<Vec<_>>()
            .join(", ");
        match self {
            NavreeSource::Path(path) => format!(
                "{{ path = {}, features = [{features}] }}",
                path_value(path)
            ),
            NavreeSource::Version(version) => format!(
                "{{ version = {}, features = [{features}] }}",
                toml_string(version)
            ),
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
    /// Where `navree` comes from.
    pub navree: NavreeSource,
}

#[derive(Serialize)]
struct AppContext {
    package: String,
    crate_name: String,
    title: String,
    navree_dependency: String,
    users_migration: String,
    ui_next: bool,
}

/// The files of the app, relative to its directory: the kit's when it has
/// one ([`kit_files`]), else the minimal app with an optional front end.
pub fn app_files(app: &NewApp, now: Timestamp) -> Result<FileSet, TemplateError> {
    if let Some(kit) = app.kit {
        return kit_files(app, kit, now);
    }
    let ctx = AppContext {
        package: app.name.package().to_owned(),
        crate_name: app.name.crate_name(),
        title: app.name.title(),
        navree_dependency: app.navree.dependency(),
        users_migration: format!("{}_create_user_profiles_table", now.migration_prefix()),
        ui_next: app.ui == Ui::Next,
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
    if ctx.ui_next {
        for (template, path) in [
            ("web/package.json", "web/package.json"),
            ("web/tsconfig.json", "web/tsconfig.json"),
            ("web/next.config.ts", "web/next.config.ts"),
            ("web/env.example", "web/.env.example"),
            ("web/README.md", "web/README.md"),
            ("web/lib/bff.ts", "web/lib/bff.ts"),
            ("web/app/layout.tsx", "web/app/layout.tsx"),
            ("web/app/page.tsx", "web/app/page.tsx"),
            ("web/app/login/page.tsx", "web/app/login/page.tsx"),
            (
                "web/app/api/auth/login/route.ts",
                "web/app/api/auth/login/route.ts",
            ),
            (
                "web/app/api/auth/logout/route.ts",
                "web/app/api/auth/logout/route.ts",
            ),
            (
                "web/app/api/proxy/route.ts",
                "web/app/api/[...path]/route.ts",
            ),
        ] {
            files.add(path, render(template, &ctx)?);
        }
    }
    Ok(files)
}

/// What `navree new` prints once the app is written to `root`: where it
/// is, and the next steps for its kit.
pub fn created_message(app: &NewApp, root: &Path) -> String {
    let package = app.name.package();
    let mut out = match app.kit {
        Some(kit) => format!("Created {package} in {} (kit: {kit})\n", root.display()),
        None => format!("Created {package} in {}\n", root.display()),
    };
    if let NavreeSource::Path(path) = &app.navree {
        out.push_str(&format!("It depends on navree at {}.\n", path.display()));
    }
    out.push_str("\nNext steps:\n");
    let steps: &[(&str, &str)] = match app.kit {
        Some(Kit::Saas) => &[
            ("navree dock up", "PostgreSQL and Mailpit"),
            ("navree key:generate", "then set APP_KEY in .env"),
            ("navree migrate:fresh --seed", "tables and a demo workspace"),
            ("pnpm --dir web install", "the web app's packages"),
            ("navree dev", "API on :8080, web app on :3000"),
        ],
        Some(Kit::Api) => &[
            ("navree dock up", "PostgreSQL and Mailpit"),
            ("navree key:generate", "then set APP_KEY in .env"),
            ("navree migrate:fresh --seed", "tables and a demo workspace"),
            ("navree dev", "API on :8080, reference at /api/docs"),
        ],
        Some(Kit::Micro) => &[
            ("navree dock up", "PostgreSQL and NATS"),
            ("navree key:generate", "then set APP_KEY in .env"),
            ("navree migrate", "the invoices table"),
            ("navree dev", "handlers on NATS, health on :8080"),
        ],
        None if app.ui == Ui::Next => &[
            ("navree key:generate", "then set APP_KEY in .env"),
            ("npm --prefix web install", "the front end; `navree dev` starts it"),
            ("navree dev", "http://127.0.0.1:8080/api/docs"),
        ],
        None => &[
            ("navree key:generate", "then set APP_KEY in .env"),
            ("navree dev", "http://127.0.0.1:8080/api/docs"),
        ],
    };
    out.push_str(&format!("  cd {package}\n"));
    for (command, comment) in steps {
        out.push_str(&format!("  {command:<30} # {comment}\n"));
    }
    out
}

/// Creates the app in `parent/<name>` and returns its directory. The
/// directory must not exist or be empty.
pub fn create_app(parent: &Path, app: &NewApp, now: Timestamp) -> anyhow::Result<PathBuf> {
    let root = parent.join(app.name.package());
    if root.exists() {
        let mut entries =
            std::fs::read_dir(&root).with_context(|| format!("cannot read {}", root.display()))?;
        if entries.next().is_some() {
            bail!("{} already exists and is not empty", root.display());
        }
    }
    app_files(app, now)?.write(&root)?;
    Ok(root)
}
