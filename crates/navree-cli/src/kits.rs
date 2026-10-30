//! Starter kits: `navree new <name> --kit saas|api|micro` writes a complete
//! application instead of the minimal app.
//!
//! The kits live in `crates/navree-cli/templates/kits/` and are embedded in
//! the binary by `build.rs`:
//!
//! | Directory | Used by | What it holds |
//! |---|---|---|
//! | `api/` | `saas`, `api` | the API: accounts, settings, projects, admin, seeds, tests, Dock, Harbor and CI files |
//! | `web/` | `saas` | the Next.js and shadcn/ui front end with its backend-for-frontend |
//! | `micro/` | `micro` | a Cargo workspace: a service crate and its contracts crate |
//!
//! A file ending in `.j2` is a MiniJinja template rendered with the kit's
//! context ([`KitContext`]); every other file is copied as it is. In paths,
//! a component starting with `dot.` becomes a dotfile (`dot.gitignore` is
//! `.gitignore`, so the templates never act as git or tool configuration in
//! this repository), `__package__` is the package name and `mNOW` is the
//! migration timestamp prefix (`m20261027_101500`).

use serde::Serialize;

use crate::clock::Timestamp;
use crate::new_app::{Database, NewApp};
use crate::scaffold::FileSet;
use crate::templates::{TemplateError, render};

include!(concat!(env!("OUT_DIR"), "/kit_files.rs"));

/// The version of the kits this CLI writes, recorded in the app's
/// `Cargo.toml` as `[package.metadata.navree] kit_version`.
pub const KIT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A starter kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum Kit {
    /// A multi-tenant SaaS: the API (accounts, two-factor, workspaces,
    /// billing, social sign-in, the Helm admin panel) and a Next.js and
    /// shadcn/ui front end in `web/` behind a backend-for-frontend.
    Saas,
    /// The SaaS kit's API without the front end, with OpenAPI for typed
    /// clients.
    Api,
    /// A microservice: message and event handlers on NATS, health checks,
    /// and a contracts crate for its clients.
    Micro,
}

impl Kit {
    /// Every kit.
    pub const ALL: [Kit; 3] = [Kit::Saas, Kit::Api, Kit::Micro];

    /// The kit's name, as `--kit` takes it.
    pub fn name(self) -> &'static str {
        match self {
            Kit::Saas => "saas",
            Kit::Api => "api",
            Kit::Micro => "micro",
        }
    }

    /// The template directories the kit is made of, in order, each with
    /// the directory of the app it is written to (`""` for the root).
    fn parts(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Kit::Saas => &[("api", ""), ("web", "web/")],
            Kit::Api => &[("api", "")],
            Kit::Micro => &[("micro", "")],
        }
    }

    /// The navree features the kit's app turns on. `saas` and `api` add
    /// them to the default `full`; the `micro` workspace declares navree
    /// without default features, so its service names `full` too.
    pub fn navree_features(self) -> &'static [&'static str] {
        match self {
            Kit::Saas | Kit::Api => &["billing", "social", "postgres"],
            Kit::Micro => &["full", "micro", "micro-nats", "postgres"],
        }
    }

    /// Whether the kit has a front end in `web/`.
    pub fn has_web(self) -> bool {
        self == Kit::Saas
    }
}

impl std::fmt::Display for Kit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What the kit templates are rendered with.
#[derive(Debug, Clone, Serialize)]
pub struct KitContext {
    /// `saas`, `api` or `micro`.
    pub kit: &'static str,
    /// [`KIT_VERSION`].
    pub kit_version: &'static str,
    /// Whether the app has the `web/` front end.
    pub web: bool,
    /// The package name (`acme-cloud`).
    pub package: String,
    /// The crate name (`acme_cloud`).
    pub crate_name: String,
    /// The display name (`Acme cloud`).
    pub title: String,
    /// The right-hand side of `navree = ...` in `Cargo.toml`, with the
    /// kit's features (`saas`, `api`).
    pub navree_dependency: String,
    /// The right-hand side of `navree = ...` in `[dev-dependencies]`: the
    /// `sqlite` driver, which the kit's tests run on (each test boots the
    /// app on a fresh SQLite file); the app itself compiles no SQLite.
    pub navree_test_dependency: String,
    /// The right-hand side of `navree = ...` in a workspace's
    /// `[workspace.dependencies]`: default features off (`micro`).
    pub navree_workspace_dependency: String,
    /// The kit's navree features as TOML strings: `"full", "micro", ...`.
    pub navree_features: String,
    /// The prefix of the kit's migration names (`m20261027_101500`).
    pub migration_prefix: String,
    /// The contracts crate's package name (`billing-contracts`; micro).
    pub contracts_package: String,
    /// The contracts crate's crate name (`billing_contracts`; micro).
    pub contracts_crate: String,
    /// `TimescaleDB` or `PostgreSQL`: what `[services.postgres]` runs.
    pub database_name: &'static str,
    /// The image of the CI workflow's database service, the one Dock runs
    /// locally (`timescale/timescaledb:2.31.0-pg17`, or `postgres:17` with
    /// `--database postgres`).
    pub postgres_image: String,
}

impl KitContext {
    /// The context of `app` created with `kit` at `now`.
    pub fn new(app: &NewApp, kit: Kit, now: Timestamp) -> Self {
        let package = app.name.package().to_owned();
        let crate_name = app.name.crate_name();
        let timescale = app.database != Database::Postgres;
        KitContext {
            kit: kit.name(),
            kit_version: KIT_VERSION,
            web: kit.has_web(),
            title: app.name.title(),
            navree_dependency: app.navree.dependency_with_features(kit.navree_features()),
            navree_test_dependency: app.navree.dependency_with_features(&["sqlite"]),
            navree_workspace_dependency: app.navree.dependency_without_default_features(),
            navree_features: kit
                .navree_features()
                .iter()
                .map(|feature| format!("{feature:?}"))
                .collect::<Vec<_>>()
                .join(", "),
            migration_prefix: now.migration_prefix(),
            contracts_package: format!("{package}-contracts"),
            contracts_crate: format!("{crate_name}_contracts"),
            database_name: if timescale {
                "TimescaleDB"
            } else {
                "PostgreSQL"
            },
            postgres_image: if timescale {
                format!(
                    "{}:{}-pg{}",
                    navree_dock::TIMESCALEDB_IMAGE,
                    navree_dock::TIMESCALEDB_VERSION,
                    navree_dock::POSTGRES_VERSION
                )
            } else {
                format!("postgres:{}", navree_dock::POSTGRES_VERSION)
            },
            package,
            crate_name,
        }
    }
}

/// The embedded files of template directory `part` (`api`, `web` or
/// `micro`), as (path inside it, contents).
pub fn embedded(part: &str) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
    KIT_FILES.iter().filter_map(move |(path, contents)| {
        path.strip_prefix(part)
            .and_then(|rest| rest.strip_prefix('/'))
            .map(|rest| (rest, *contents))
    })
}

/// Every embedded kit template (`.j2`), as (template name, source). The
/// name is the path under `templates/kits` with its `.j2`
/// (`api/Cargo.toml.j2`), so it never collides with the app and `make:*`
/// templates, whose names have no suffix.
pub(crate) fn templates() -> impl Iterator<Item = (&'static str, &'static str)> {
    KIT_FILES
        .iter()
        .filter(|(path, _)| path.ends_with(".j2"))
        .copied()
}

/// The generated path of template file `path`: `.j2` dropped, `dot.`
/// components made dotfiles, `__package__` and `mNOW` replaced.
pub fn output_path(path: &str, ctx: &KitContext) -> String {
    let path = path.strip_suffix(".j2").unwrap_or(path);
    path.split('/')
        .map(|part| {
            let part = match part.strip_prefix("dot.") {
                Some(rest) => format!(".{rest}"),
                None => part.to_owned(),
            };
            part.replace("__package__", &ctx.package)
                .replace("mNOW", &ctx.migration_prefix)
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The files of `kit` for `app`, relative to the app's directory.
pub fn kit_files(app: &NewApp, kit: Kit, now: Timestamp) -> Result<FileSet, TemplateError> {
    let ctx = KitContext::new(app, kit, now);
    let mut files = FileSet::new();
    for (part, into) in kit.parts() {
        for (path, contents) in embedded(part) {
            let target = format!("{into}{}", output_path(path, &ctx));
            let contents = if path.ends_with(".j2") {
                render(&format!("{part}/{path}"), &ctx)?
            } else {
                contents.to_owned()
            };
            // Embedded files keep the checkout's line endings; apps get LF.
            let mut contents = contents.replace("\r\n", "\n");
            // Dock's `postgres` service runs TimescaleDB unless told otherwise.
            if target == "dock.toml" && app.database == Database::Postgres {
                contents = contents.replacen(
                    "[services.postgres]\n",
                    "[services.postgres]\ntimescaledb = false\n",
                    1,
                );
            }
            files.add(target, contents);
        }
    }
    if let Some(env) = files.get(".env.example").map(str::to_owned) {
        files.add(
            ".env",
            with_dock_settings(&env, files.get("dock.toml"), &ctx),
        );
    }
    // The web app's settings for this machine (git-ignored), so `navree dev`
    // starts a front end that reaches the API and its WebSocket server.
    if let Some(env) = files.get("web/.env.example").map(str::to_owned) {
        files.add("web/.env.local", env);
    }
    Ok(files)
}

/// `.env`: `.env.example` plus the settings of the services in `dock.toml`
/// (between `# dock:start` and `# dock:end`, as `navree dock up` writes
/// them), so the app reaches the services Dock starts.
fn with_dock_settings(env: &str, dock: Option<&str>, ctx: &KitContext) -> String {
    let Some(dock) = dock else {
        return env.to_owned();
    };
    match navree_dock::DockFile::parse(dock, &ctx.package) {
        Ok(file) => navree_dock::update_dotenv(env, &file.env()).text,
        Err(_) => env.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::names::AppName;
    use crate::new_app::{NavreeSource, Ui};

    fn ctx() -> KitContext {
        let app = NewApp {
            name: AppName::parse("acme").unwrap(),
            ui: Ui::None,
            kit: Some(Kit::Saas),
            database: Database::default(),
            navree: NavreeSource::Path(PathBuf::from("/opt/navree/crates/navree")),
        };
        KitContext::new(&app, Kit::Saas, Timestamp::from_unix(1_792_938_605))
    }

    #[test]
    fn every_kit_has_embedded_files() {
        for part in ["api", "web", "micro"] {
            assert!(embedded(part).count() > 5, "{part}");
        }
    }

    #[test]
    fn output_paths_drop_j2_and_make_dotfiles() {
        let ctx = ctx();
        assert_eq!(output_path("Cargo.toml.j2", &ctx), "Cargo.toml");
        assert_eq!(output_path("dot.gitignore.j2", &ctx), ".gitignore");
        assert_eq!(
            output_path("dot.github/workflows/ci.yml.j2", &ctx),
            ".github/workflows/ci.yml"
        );
        assert_eq!(output_path("web/dot.env.example", &ctx), "web/.env.example");
        assert_eq!(
            output_path("crates/__package__-contracts/Cargo.toml.j2", &ctx),
            "crates/acme-contracts/Cargo.toml"
        );
        assert_eq!(
            output_path("migrations/mNOW_create_projects_table.rs.j2", &ctx),
            "migrations/m20261025_143005_create_projects_table.rs"
        );
        assert_eq!(
            output_path("app/(auth)/sign-in/page.tsx", &ctx),
            "app/(auth)/sign-in/page.tsx"
        );
    }

    #[test]
    fn kit_names_round_trip() {
        use clap::ValueEnum;
        for kit in Kit::ALL {
            assert_eq!(Kit::from_str(kit.name(), false), Ok(kit));
            assert_eq!(kit.to_string(), kit.name());
        }
    }
}
