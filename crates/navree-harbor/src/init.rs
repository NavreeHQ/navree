//! `navree harbor:init`: writes `harbor.toml` and its key, and keeps the
//! key out of Git.

use std::path::Path;

use serde_json::json;

use crate::config::Environment;
use crate::error::{HarborError, Result};
use crate::project::{package_name, write_private};
use crate::render;
use crate::secrets::SecretKey;

/// What `harbor:init` did, one line per file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitReport {
    /// `(verb, file, note)`: `created harbor.toml`, `updated .gitignore`.
    pub lines: Vec<(String, String, String)>,
}

impl InitReport {
    fn push(&mut self, verb: &str, file: &str, note: &str) {
        self.lines
            .push((verb.to_owned(), file.to_owned(), note.to_owned()));
    }

    /// `  created  harbor.toml`, aligned like the docs.
    pub fn render(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|(verb, file, note)| {
                if note.is_empty() {
                    format!("  {verb:<8} {file}")
                } else {
                    format!("  {verb:<8} {file:<18} # {note}")
                }
            })
            .collect()
    }
}

/// The `.gitignore` line that keeps every Harbor key out of Git.
pub const IGNORE_LINE: &str = "harbor*.key";

/// Writes the environment's `harbor.toml` and key in `dir`.
pub fn init(dir: &Path, env: &Environment) -> Result<InitReport> {
    let app = package_name(dir).ok_or_else(|| {
        HarborError::msg(format!(
            "{} has no Cargo.toml with a [package]; run harbor:init in your app's directory",
            dir.display()
        ))
    })?;
    let config_file = env.config_file();
    let key_file = env.key_file();
    let config_path = dir.join(&config_file);
    if config_path.exists() {
        return Err(HarborError::msg(format!("{config_file} already exists")));
    }
    let mut report = InitReport::default();
    let toml = render::render(
        "init/harbor.toml",
        json!({
            "app": app,
            "domain": format!("{app}.example.com"),
            "env_name": env.name(),
            "env_flag": env.flag(),
            "key_file": key_file,
        }),
    )?;
    std::fs::write(&config_path, toml).map_err(|e| HarborError::io(&config_path, e))?;
    report.push("created", &config_file, "");

    let key_path = dir.join(&key_file);
    if key_path.exists() {
        report.push("kept", &key_file, "the existing key");
    } else {
        write_private(&key_path, &format!("{}\n", SecretKey::generate().encode()))?;
        report.push("created", &key_file, "the secrets key: never commit it");
    }

    let ignore_path = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&ignore_path).unwrap_or_default();
    let covered = existing
        .lines()
        .any(|l| matches!(l.trim(), IGNORE_LINE | "/harbor*.key") || l.trim() == key_file);
    if !covered {
        let mut text = existing.clone();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("# Harbor's secrets keys (navree harbor:init)\n");
        text.push_str(IGNORE_LINE);
        text.push('\n');
        std::fs::write(&ignore_path, text).map_err(|e| HarborError::io(&ignore_path, e))?;
        let verb = if existing.is_empty() {
            "created"
        } else {
            "updated"
        };
        report.push(verb, ".gitignore", &format!("ignores {key_file}"));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    fn app_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"crm-api\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn init_writes_the_file_the_key_and_the_ignore_line() {
        let dir = app_dir();
        std::fs::write(dir.path().join(".gitignore"), "/target").unwrap();
        let report = init(dir.path(), &Environment::production()).unwrap();
        assert_eq!(
            report.render(),
            [
                "  created  harbor.toml",
                "  created  harbor.key         # the secrets key: never commit it",
                "  updated  .gitignore         # ignores harbor.key",
            ]
        );
        let ignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(ignore.starts_with("/target\n"), "{ignore}");
        assert!(ignore.ends_with("harbor*.key\n"), "{ignore}");
        // The file is valid and the key works.
        let project = Project::load(dir.path(), Environment::production()).unwrap();
        assert_eq!(project.config.app, "crm-api");
        assert_eq!(
            project.config.domain.as_deref(),
            Some("crm-api.example.com")
        );
        assert!(project.key(None).is_ok());
        // Not twice.
        assert!(init(dir.path(), &Environment::production()).is_err());
    }

    #[test]
    fn another_environment_gets_its_own_files() {
        let dir = app_dir();
        init(dir.path(), &Environment::production()).unwrap();
        let staging = Environment::named("staging").unwrap();
        let report = init(dir.path(), &staging).unwrap();
        assert_eq!(
            report.render(),
            [
                "  created  harbor.staging.toml",
                "  created  harbor.staging.key # the secrets key: never commit it",
            ]
        );
        let text = std::fs::read_to_string(dir.path().join("harbor.staging.toml")).unwrap();
        assert!(text.contains("navree deploy --env staging"), "{text}");
        assert!(
            text.contains("harbor:secrets set <NAME> --env staging"),
            "{text}"
        );
        assert!(Project::load(dir.path(), staging).is_ok());
    }

    #[test]
    fn init_needs_a_package() {
        let dir = tempfile::tempdir().unwrap();
        assert!(init(dir.path(), &Environment::production()).is_err());
    }
}
