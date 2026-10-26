//! `navis new <name>`: a new Navis app (Laravel's `laravel new`).

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Serialize;

use crate::clock::Timestamp;
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

/// Where the app gets the `navis` crate from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavisSource {
    /// A local checkout: the directory of the `navis` facade crate.
    Path(PathBuf),
    /// A published version requirement.
    Version(String),
}

impl NavisSource {
    /// The version requirement used when navis is not found locally.
    pub const DEFAULT_VERSION: &'static str = "0.1.0";

    /// Resolves `--navis-path`: the navis repository (its `crates/navis` is
    /// used) or the facade crate itself. Without it, the repository this
    /// `navis` binary was built from, when it still exists; else the
    /// published version.
    pub fn resolve(flag: Option<&Path>) -> anyhow::Result<Self> {
        match flag {
            Some(path) => {
                let path = std::path::absolute(path)
                    .with_context(|| format!("cannot resolve {}", path.display()))?;
                facade_dir(&path).map(NavisSource::Path).with_context(|| {
                    format!(
                        "--navis-path {}: expected the navis repository (with crates/navis) or the navis crate itself",
                        path.display()
                    )
                })
            }
            None => {
                let built_from = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("navis");
                Ok(match facade_dir(&built_from) {
                    Some(dir) => NavisSource::Path(std::path::absolute(&dir).unwrap_or(dir)),
                    None => NavisSource::Version(Self::DEFAULT_VERSION.to_owned()),
                })
            }
        }
    }

    /// The right-hand side of `navis = ...` in `Cargo.toml`.
    pub fn dependency(&self) -> String {
        match self {
            NavisSource::Path(path) => {
                let path = plain_path(&path.to_string_lossy()).replace('\\', "/");
                format!("{{ path = {} }}", toml_string(&path))
            }
            NavisSource::Version(version) => toml_string(version),
        }
    }
}

/// `path` itself when it is the `navis` package, or its `crates/navis`.
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
                    .map(|name| name == "navis")
            })
            .unwrap_or(false)
    };
    let nested = path.join("crates").join("navis");
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

/// What `navis new` creates.
#[derive(Debug, Clone)]
pub struct NewApp {
    /// The package name.
    pub name: AppName,
    /// The front end.
    pub ui: Ui,
    /// Where `navis` comes from.
    pub navis: NavisSource,
}

#[derive(Serialize)]
struct AppContext {
    package: String,
    crate_name: String,
    title: String,
    navis_dependency: String,
    users_migration: String,
    ui_next: bool,
}

/// The files of the app, relative to its directory.
pub fn app_files(app: &NewApp, now: Timestamp) -> Result<FileSet, TemplateError> {
    let ctx = AppContext {
        package: app.name.package().to_owned(),
        crate_name: app.name.crate_name(),
        title: app.name.title(),
        navis_dependency: app.navis.dependency(),
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
