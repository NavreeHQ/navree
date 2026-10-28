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

/// Writes the environment's `harbor.toml` and key in `dir`. When
/// `harbor.toml` is already there (a starter kit ships one) but its key is
/// not, only the key is written: see [`ensure_key`].
pub fn init(dir: &Path, env: &Environment) -> Result<InitReport> {
    let config_file = env.config_file();
    let key_file = env.key_file();
    let config_path = dir.join(&config_file);
    if config_path.exists() {
        if dir.join(&key_file).exists() {
            return Err(HarborError::msg(format!("{config_file} already exists")));
        }
        return ensure_key(dir, env);
    }
    let app = package_name(dir).ok_or_else(|| {
        HarborError::msg(format!(
            "{} has no Cargo.toml with a [package]; run harbor:init in your app's directory",
            dir.display()
        ))
    })?;
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

    write_key(dir, env, &mut report)?;
    Ok(report)
}

/// Writes the secrets key of an environment whose `harbor.toml` exists (a
/// starter kit ships the file without a key, which `harbor:secrets` then
/// cannot use), and keeps it out of Git. Does nothing to an existing key.
/// Refuses when the file holds encrypted secrets: a new key could not
/// decrypt them, so the original key has to come back.
pub fn ensure_key(dir: &Path, env: &Environment) -> Result<InitReport> {
    let config_file = env.config_file();
    let key_file = env.key_file();
    let config_path = dir.join(&config_file);
    let text =
        std::fs::read_to_string(&config_path).map_err(|e| HarborError::io(&config_path, e))?;
    let mut report = InitReport::default();
    report.push("kept", &config_file, "");
    if !dir.join(&key_file).exists() {
        let encrypted = text
            .parse::<toml::Table>()
            .ok()
            .and_then(|table| {
                table
                    .get("secrets")
                    .and_then(|s| s.as_table().map(|t| !t.is_empty()))
            })
            .unwrap_or(false);
        if encrypted {
            return Err(HarborError::msg(format!(
                "{config_file} has encrypted secrets but {key_file} is missing: put the key back (a new one cannot decrypt them)"
            )));
        }
    }
    write_key(dir, env, &mut report)?;
    Ok(report)
}

/// Writes `env`'s key unless it exists, and the `.gitignore` line covering
/// it, noting both in `report`.
fn write_key(dir: &Path, env: &Environment, report: &mut InitReport) -> Result<()> {
    let key_file = env.key_file();
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
    Ok(())
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
    fn init_adds_the_missing_key_to_a_harbor_toml_a_kit_shipped() {
        let dir = app_dir();
        std::fs::write(
            dir.path().join("harbor.toml"),
            "app = \"crm-api\"\ndomain = \"crm-api.example.com\"\n\n[servers.app-1]\nhost = \"203.0.113.10\"\nroles = [\"web\"]\n",
        )
        .unwrap();
        let report = init(dir.path(), &Environment::production()).unwrap();
        assert_eq!(
            report.render(),
            [
                "  kept     harbor.toml",
                "  created  harbor.key         # the secrets key: never commit it",
                "  created  .gitignore         # ignores harbor.key",
            ]
        );
        // The file is untouched, and `harbor:secrets` has its key.
        assert!(
            std::fs::read_to_string(dir.path().join("harbor.toml"))
                .unwrap()
                .starts_with("app = \"crm-api\"\n")
        );
        let project = Project::load(dir.path(), Environment::production()).unwrap();
        assert!(project.key(None).is_ok());
        // With both files there is nothing left to do.
        let error = init(dir.path(), &Environment::production()).unwrap_err();
        assert_eq!(error.to_string(), "harbor.toml already exists");
    }

    #[test]
    fn a_missing_key_is_not_replaced_when_secrets_need_it() {
        let dir = app_dir();
        std::fs::write(
            dir.path().join("harbor.toml"),
            "app = \"crm-api\"\n\n[secrets]\nAPP_KEY = \"harbor:v1:abc\"\n",
        )
        .unwrap();
        let error = init(dir.path(), &Environment::production()).unwrap_err();
        assert!(
            error.to_string().contains("has encrypted secrets"),
            "{error}"
        );
        assert!(!dir.path().join("harbor.key").exists());
    }

    #[test]
    fn init_needs_a_package() {
        let dir = tempfile::tempdir().unwrap();
        assert!(init(dir.path(), &Environment::production()).is_err());
    }
}
