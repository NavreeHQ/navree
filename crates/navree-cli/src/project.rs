//! Finding the app a command runs in.

use std::path::{Path, PathBuf};

/// The nearest directory, from `start` up, whose `Cargo.toml` has a
/// `[package]` table (a workspace-only manifest is skipped).
pub fn project_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
        let parsed: toml::Table = manifest.parse().ok()?;
        parsed.contains_key("package").then(|| dir.to_path_buf())
    })
}

/// Where `navree dev`, `navree dock` and forwarded commands run: the
/// nearest directory, from `start` up, whose `Cargo.toml` has a `[package]`
/// table or a `[workspace]` with `default-members` (a workspace whose
/// default member is the app, as in the `micro` starter kit, keeps its
/// `config/`, `.env` and `dock.toml` at its root).
pub fn run_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
        let parsed: toml::Table = manifest.parse().ok()?;
        let runs_an_app = parsed.contains_key("package")
            || parsed
                .get("workspace")
                .and_then(|workspace| workspace.get("default-members"))
                .is_some();
        runs_an_app.then(|| dir.to_path_buf())
    })
}

/// The package name in `root/Cargo.toml`.
pub fn package_name(root: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let parsed: toml::Table = manifest.parse().ok()?;
    parsed
        .get("package")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_nearest_package_above() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\n",
        )
        .unwrap();
        let app = dir.path().join("app");
        let deep = app.join("src").join("modules");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = \"blog\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(project_root(&deep), Some(app.clone()));
        assert_eq!(project_root(&app), Some(app.clone()));
        assert_eq!(project_root(dir.path()), None);
        assert_eq!(package_name(&app).as_deref(), Some("blog"));
        // A workspace without default members runs nothing.
        assert_eq!(run_root(dir.path()), None);
        assert_eq!(run_root(&deep), Some(app.clone()));
    }

    #[test]
    fn a_workspace_with_default_members_is_where_commands_run() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\ndefault-members = [\"crates/billing\"]\n",
        )
        .unwrap();
        let service = dir.path().join("crates").join("billing");
        std::fs::create_dir_all(service.join("src")).unwrap();
        std::fs::write(
            service.join("Cargo.toml"),
            "[package]\nname = \"billing\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(run_root(dir.path()), Some(dir.path().to_path_buf()));
        assert_eq!(
            project_root(dir.path()),
            None,
            "no package to generate code in"
        );
        // Inside the service crate, generators and commands use the crate.
        assert_eq!(run_root(&service.join("src")), Some(service.clone()));
        assert_eq!(project_root(&service.join("src")), Some(service));
    }
}
