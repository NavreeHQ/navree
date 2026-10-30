//! An app's Harbor files: `harbor.toml` (or `harbor.<env>.toml`), its key,
//! and the secrets operations behind `navree harbor:secrets`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::{Environment, HarborConfig, is_env_name};
use crate::document;
use crate::error::{HarborError, Result};
use crate::secrets::SecretKey;

/// The environment variable that holds the secrets key in CI.
pub const KEY_VAR: &str = "NAVREE_HARBOR_KEY";

/// An app's Harbor configuration, read from its directory.
#[derive(Debug, Clone)]
pub struct Project {
    /// The app's directory (where `harbor.toml` and `Cargo.toml` are).
    pub dir: PathBuf,
    /// The selected environment.
    pub env: Environment,
    /// The parsed configuration, with `app` filled in.
    pub config: HarborConfig,
    text: String,
}

impl Project {
    /// Reads the environment's file in `dir` without validating it (enough
    /// for the secrets commands).
    pub fn open(dir: &Path, env: Environment) -> Result<Project> {
        let path = dir.join(env.config_file());
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(HarborError::NoConfig { path });
            }
            Err(e) => return Err(HarborError::io(path, e)),
        };
        let mut config = HarborConfig::parse(&text, &path)?;
        if config.app.is_empty() {
            config.app = package_name(dir).unwrap_or_default();
        }
        Ok(Project {
            dir: dir.to_path_buf(),
            env,
            config,
            text,
        })
    }

    /// Reads and validates the environment's file in `dir`.
    pub fn load(dir: &Path, env: Environment) -> Result<Project> {
        let project = Self::open(dir, env)?;
        let problems = project.config.validate();
        if problems.is_empty() {
            Ok(project)
        } else {
            Err(HarborError::Invalid {
                path: project.config_path(),
                problems,
            })
        }
    }

    /// The configuration file.
    pub fn config_path(&self) -> PathBuf {
        self.dir.join(self.env.config_file())
    }

    /// The key file.
    pub fn key_path(&self) -> PathBuf {
        self.dir.join(self.env.key_file())
    }

    /// The file's current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The secrets key: `var` (the value of `NAVREE_HARBOR_KEY`) when set,
    /// otherwise the key file.
    pub fn key(&self, var: Option<&str>) -> Result<SecretKey> {
        if let Some(text) = var.filter(|v| !v.trim().is_empty()) {
            return SecretKey::parse(text);
        }
        let path = self.key_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => SecretKey::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(HarborError::MissingKey { path })
            }
            Err(e) => Err(HarborError::io(path, e)),
        }
    }

    /// Every secret, decrypted.
    pub fn decrypt_secrets(&self, key: &SecretKey) -> Result<BTreeMap<String, String>> {
        self.config
            .secrets
            .iter()
            .map(|(name, stored)| Ok((name.clone(), key.decrypt(name, stored)?)))
            .collect()
    }

    /// One secret, decrypted.
    pub fn reveal(&self, key: &SecretKey, name: &str) -> Result<String> {
        let stored = self
            .config
            .secrets
            .get(name)
            .ok_or_else(|| HarborError::UnknownSecret(name.to_owned()))?;
        key.decrypt(name, stored)
    }

    /// Encrypts `value` and stores it as `[secrets] name`.
    pub fn set_secret(&mut self, key: &SecretKey, name: &str, value: &str) -> Result<()> {
        if !is_env_name(name) {
            return Err(HarborError::msg(format!(
                "`{name}` is not an environment variable name (letters, digits and `_`)"
            )));
        }
        let text = document::set_secret(&self.text, name, &key.encrypt(name, value))?;
        self.save(text)
    }

    /// Removes `[secrets] name`.
    pub fn unset_secret(&mut self, name: &str) -> Result<()> {
        let text = document::remove_secret(&self.text, name)?;
        self.save(text)
    }

    /// Pins `server`'s SSH host key.
    pub fn set_host_key(&mut self, server: &str, host_key: &str) -> Result<()> {
        let text = document::set_host_key(&self.text, server, host_key)?;
        self.save(text)
    }

    /// Re-encrypts every secret under `new` and writes the new key file. The
    /// configuration is written first and the key file replaced last, so an
    /// interruption leaves the new key in `<key file>.new`.
    pub fn rotate_key(&mut self, old: &SecretKey, new: &SecretKey) -> Result<usize> {
        let plain = self.decrypt_secrets(old)?;
        let staged = self.key_path().with_extension("key.new");
        write_private(&staged, &format!("{}\n", new.encode()))?;
        let payloads: BTreeMap<String, String> = plain
            .iter()
            .map(|(name, value)| (name.clone(), new.encrypt(name, value)))
            .collect();
        let text = document::replace_secrets(&self.text, &payloads)?;
        self.save(text)?;
        let path = self.key_path();
        std::fs::rename(&staged, &path).map_err(|e| HarborError::io(&path, e))?;
        Ok(plain.len())
    }

    fn save(&mut self, text: String) -> Result<()> {
        let path = self.config_path();
        let mut config = HarborConfig::parse(&text, &path)?;
        if config.app.is_empty() {
            config.app = self.config.app.clone();
        }
        std::fs::write(&path, &text).map_err(|e| HarborError::io(&path, e))?;
        self.config = config;
        self.text = text;
        Ok(())
    }
}

/// The package name in `dir/Cargo.toml`.
pub fn package_name(dir: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let parsed: toml::Table = manifest.parse().ok()?;
    parsed
        .get("package")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

/// Writes `contents` to `path`, readable only by its owner where the
/// platform supports it.
pub fn write_private(path: &Path, contents: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| HarborError::io(path, e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| HarborError::io(path, e))
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents).map_err(|e| HarborError::io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(dir: &Path, harbor: &str) {
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"crm-api\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("harbor.toml"), harbor).unwrap();
    }

    const HARBOR: &str =
        "domain = \"crm.example.com\"\n\n[servers.app-1]\nhost = \"203.0.113.10\"\n";

    #[test]
    fn the_app_defaults_to_the_package() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        app(dir.path(), HARBOR);
        let project = Project::load(dir.path(), Environment::production()).unwrap();
        assert_eq!(project.config.app, "crm-api");
        assert_eq!(project.config_path(), dir.path().join("harbor.toml"));
        assert_eq!(project.key_path(), dir.path().join("harbor.key"));
    }

    #[test]
    fn a_missing_file_says_to_run_init() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let error = Project::load(dir.path(), Environment::named("staging").unwrap()).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("harbor.staging.toml"), "{message}");
        assert!(message.contains("navree harbor:init"), "{message}");
    }

    #[test]
    fn load_validates_and_open_does_not() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        app(dir.path(), "[servers.app-1]\nhost = \"203.0.113.10\"\n");
        assert!(Project::open(dir.path(), Environment::production()).is_ok());
        let error = Project::load(dir.path(), Environment::production()).unwrap_err();
        assert!(
            error.to_string().contains("`domain` is required"),
            "{error}"
        );
    }

    #[test]
    fn secrets_are_set_revealed_and_unset() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        app(dir.path(), HARBOR);
        let key = SecretKey::generate();
        let mut project = Project::load(dir.path(), Environment::production()).unwrap();
        project
            .set_secret(&key, "STRIPE_SECRET", "sk_live_1")
            .unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join("harbor.toml")).unwrap();
        assert!(
            on_disk.contains("STRIPE_SECRET = \"harbor:v1:"),
            "{on_disk}"
        );
        assert!(!on_disk.contains("sk_live_1"));

        let reread = Project::load(dir.path(), Environment::production()).unwrap();
        assert_eq!(reread.reveal(&key, "STRIPE_SECRET").unwrap(), "sk_live_1");
        assert_eq!(
            reread.decrypt_secrets(&key).unwrap(),
            BTreeMap::from([("STRIPE_SECRET".to_owned(), "sk_live_1".to_owned())])
        );
        assert!(matches!(
            reread.reveal(&key, "NOPE"),
            Err(HarborError::UnknownSecret(_))
        ));

        project.unset_secret("STRIPE_SECRET").unwrap();
        assert!(project.config.secrets.is_empty());
        assert!(project.set_secret(&key, "BAD NAME", "x").is_err());
    }

    #[test]
    fn the_key_comes_from_the_variable_or_the_file() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        app(dir.path(), HARBOR);
        let project = Project::load(dir.path(), Environment::production()).unwrap();
        assert!(matches!(
            project.key(None),
            Err(HarborError::MissingKey { .. })
        ));
        let file_key = SecretKey::generate();
        std::fs::write(project.key_path(), format!("{}\n", file_key.encode())).unwrap();
        assert_eq!(project.key(None).unwrap(), file_key);
        assert_eq!(project.key(Some("  ")).unwrap(), file_key);
        let var_key = SecretKey::generate();
        assert_eq!(project.key(Some(&var_key.encode())).unwrap(), var_key);
    }

    #[test]
    fn rotation_re_encrypts_every_secret_and_replaces_the_key() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        app(dir.path(), HARBOR);
        let old = SecretKey::generate();
        std::fs::write(dir.path().join("harbor.key"), old.encode()).unwrap();
        let mut project = Project::load(dir.path(), Environment::production()).unwrap();
        project.set_secret(&old, "A", "one").unwrap();
        project.set_secret(&old, "B", "two").unwrap();
        let new = SecretKey::generate();
        assert_eq!(project.rotate_key(&old, &new).unwrap(), 2);
        assert_eq!(project.key(None).unwrap(), new);
        assert!(!dir.path().join("harbor.key.new").exists());
        let reread = Project::load(dir.path(), Environment::production()).unwrap();
        assert_eq!(reread.reveal(&new, "A").unwrap(), "one");
        assert_eq!(reread.reveal(&new, "B").unwrap(), "two");
        assert!(reread.reveal(&old, "A").is_err());
    }

    #[test]
    fn host_keys_are_saved() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        app(dir.path(), HARBOR);
        let mut project = Project::load(dir.path(), Environment::production()).unwrap();
        project
            .set_host_key("app-1", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJz")
            .unwrap();
        assert_eq!(
            project.config.servers[0].host_key.as_deref(),
            Some("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJz")
        );
    }
}
