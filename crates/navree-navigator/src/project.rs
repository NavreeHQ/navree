//! The app Navigator serves: its directory, package, Navree version and
//! configuration.

use std::path::{Path, PathBuf};

use navree_config::{Config, ConfigError, ConfigSources};

/// The configuration files `app_info` reports when they exist.
pub const CONFIG_FILES: [&str; 4] = [".env", ".env.example", "config/app.toml", "dock.toml"];

/// The app: the nearest Cargo package from the starting directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// The package directory (holds its `Cargo.toml`).
    pub root: PathBuf,
    /// The package name.
    pub package: String,
    /// The `Cargo.lock` that pins its dependencies (here or in a workspace
    /// root above), if any.
    pub lockfile: Option<PathBuf>,
}

/// Why no app was found.
#[derive(Debug, thiserror::Error)]
#[error(
    "{} is not inside a Rust package (no Cargo.toml with [package] here or above). Start Navigator in your \
     app's directory, or pass `navree mcp --path <app>`.",
    .start.display()
)]
pub struct NotAnApp {
    /// Where the search started.
    pub start: PathBuf,
}

impl Project {
    /// The nearest directory from `start` up whose `Cargo.toml` has a
    /// `[package]` table.
    pub fn find(start: &Path) -> Result<Project, NotAnApp> {
        let start = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
        for dir in start.ancestors() {
            let Ok(manifest) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
                continue;
            };
            let Ok(parsed) = manifest.parse::<toml::Table>() else {
                continue;
            };
            let Some(name) = parsed
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
            else {
                continue;
            };
            let lockfile = dir
                .ancestors()
                .map(|d| d.join("Cargo.lock"))
                .find(|lock| lock.is_file());
            return Ok(Project {
                root: dir.to_path_buf(),
                package: name.to_owned(),
                lockfile,
            });
        }
        Err(NotAnApp { start })
    }

    /// The `navree` version in the lockfile (the highest, if several).
    pub fn navree_version(&self) -> Option<String> {
        let lock = std::fs::read_to_string(self.lockfile.as_ref()?).ok()?;
        navree_version_in(&lock)
    }

    /// The configuration files that exist, relative to the app.
    pub fn config_files(&self) -> Vec<String> {
        CONFIG_FILES
            .iter()
            .filter(|f| self.root.join(f).is_file())
            .map(|f| (*f).to_owned())
            .collect()
    }

    /// The sources the app reads its configuration from, from its
    /// directory: `config/app.toml`, `.env`, then `APP_*` environment
    /// variables, plus the settings of its `dock.toml` services that the
    /// environment doesn't set (what `navree dev` passes to the app).
    pub fn config_sources(&self) -> ConfigSources {
        ConfigSources::defaults_in(&self.root).env_vars("APP_", self.dock_env())
    }

    /// The settings of the app's `dock.toml` services that the process
    /// environment doesn't already set (none without a `dock.toml`).
    pub fn dock_env(&self) -> Vec<(String, String)> {
        dock_env(&self.root)
    }

    /// The app's configuration, as the app sees it.
    pub fn config(&self) -> Result<Config, ConfigError> {
        Config::load(&self.config_sources())
    }

    /// The facade features the app's `Cargo.toml` turns on for `navree`
    /// (from the workspace's `[workspace.dependencies]` too), with
    /// `default` when default features are on.
    pub fn declared_features(&self) -> Vec<String> {
        let read = |path: PathBuf| -> Option<toml::Table> {
            std::fs::read_to_string(path).ok()?.parse().ok()
        };
        let Some(manifest) = read(self.root.join("Cargo.toml")) else {
            return Vec::new();
        };
        let Some(dependency) = manifest
            .get("dependencies")
            .and_then(|d| d.get("navree"))
            .cloned()
        else {
            return Vec::new();
        };
        let mut specs = vec![dependency.clone()];
        if dependency.get("workspace").and_then(|w| w.as_bool()) == Some(true)
            && let Some(workspace) = self
                .root
                .ancestors()
                .skip(1)
                .filter_map(|d| read(d.join("Cargo.toml")))
                .find(|t| t.contains_key("workspace"))
            && let Some(inherited) = workspace
                .get("workspace")
                .and_then(|w| w.get("dependencies"))
                .and_then(|d| d.get("navree"))
        {
            specs.push(inherited.clone());
        }
        let mut features = Vec::new();
        let mut defaults = true;
        for spec in &specs {
            if spec.get("default-features").and_then(|d| d.as_bool()) == Some(false) {
                defaults = false;
            }
            if let Some(list) = spec.get("features").and_then(|f| f.as_array()) {
                features.extend(list.iter().filter_map(|f| f.as_str()).map(str::to_owned));
            }
        }
        if defaults {
            features.insert(0, "default".to_owned());
        }
        features.dedup();
        features
    }
}

/// The settings of the app's `dock.toml` services that the process
/// environment doesn't already set.
fn dock_env(root: &Path) -> Vec<(String, String)> {
    navree_dock::app_env(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _)| std::env::var_os(name).is_none())
        .collect()
}

/// The highest `navree` version pinned in a `Cargo.lock`.
pub fn navree_version_in(lock: &str) -> Option<String> {
    let parsed: toml::Table = lock.parse().ok()?;
    let mut versions: Vec<String> = parsed
        .get("package")?
        .as_array()?
        .iter()
        .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some("navree"))
        .filter_map(|p| p.get("version").and_then(|v| v.as_str()).map(str::to_owned))
        .collect();
    versions.sort_by_key(|v| {
        v.split(['.', '-'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    });
    versions.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn the_nearest_package_is_the_app() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"apps/crm\"]\n",
        );
        write(&dir.path().join("Cargo.lock"), "version = 4\n");
        let app = dir.path().join("apps/crm");
        write(
            &app.join("Cargo.toml"),
            "[package]\nname = \"crm\"\nversion = \"0.1.0\"\n",
        );
        let project = Project::find(&app.join("src")).unwrap();
        assert_eq!(project.root, app);
        assert_eq!(project.package, "crm");
        assert_eq!(project.lockfile, Some(dir.path().join("Cargo.lock")));
        assert!(Project::find(dir.path()).is_err());
    }

    #[test]
    fn the_navree_version_comes_from_the_lockfile() {
        let lock = r#"
version = 4

[[package]]
name = "navree"
version = "0.9.2"

[[package]]
name = "navree"
version = "0.10.0"

[[package]]
name = "navree-kernel"
version = "0.11.0"
"#;
        assert_eq!(navree_version_in(lock).as_deref(), Some("0.10.0"));
        assert_eq!(navree_version_in("version = 4\n"), None);
    }

    #[test]
    fn declared_features_include_the_workspace_ones() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[workspace]\n[workspace.dependencies]\nnavree = { version = \"0.1\", features = [\"postgres\"] }\n",
        );
        let app = dir.path().join("app");
        write(
            &app.join("Cargo.toml"),
            "[package]\nname = \"app\"\n[dependencies]\nnavree = { workspace = true, features = [\"social\"] }\n",
        );
        let project = Project::find(&app).unwrap();
        assert_eq!(
            project.declared_features(),
            ["default", "social", "postgres"]
        );

        write(
            &app.join("Cargo.toml"),
            "[package]\nname = \"app\"\n[dependencies]\nnavree = { version = \"0.1\", default-features = false, features = [\"http\"] }\n",
        );
        assert_eq!(project.declared_features(), ["http"]);
    }

    #[test]
    fn configuration_is_read_from_the_app_directory() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        write(
            &dir.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\n",
        );
        write(
            &dir.path().join("config/app.toml"),
            "[database]\nurl = \"sqlite://storage/app.db?mode=rwc\"\n[mail]\npassword = \"hunter2\"\n",
        );
        write(&dir.path().join(".env"), "APP_KEY=base64:abc\n");
        let project = Project::find(dir.path()).unwrap();
        let config = project.config().unwrap();
        assert_eq!(
            config.get_string("database.url").as_deref(),
            Some("sqlite://storage/app.db?mode=rwc")
        );
        assert_eq!(config.redacted()["mail"]["password"], "[redacted]");
        assert_eq!(config.redacted()["key"], "[redacted]");
        assert_eq!(project.config_files(), [".env", "config/app.toml"]);
    }
}
