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
        let dir = tempfile::tempdir().unwrap();
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
    }
}
