//! SSH through the system `ssh` binary, with the server's host key pinned.
//!
//! Every connection uses a known-hosts file of its own holding only the key
//! pinned in `harbor.toml` (under the alias `harbor-<server>`), with
//! `StrictHostKeyChecking=yes`. Before a key is pinned (the first
//! `harbor:provision`), the connection accepts the key the server offers
//! and [`Ssh::learned_host_key`] reads it back so it can be recorded.
//! Scripts go to `bash -s` on standard input, never on the command line, so
//! secrets in them don't show up in process lists.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempPath;

use crate::config::Server;
use crate::env::quote;
use crate::error::{HarborError, Result, tail};
use crate::exec::{Cmd, Executor, Output, slash_path};

/// The environment variable holding a private key (CI).
pub const SSH_KEY_VAR: &str = "NAVREE_HARBOR_SSH_KEY";

/// The private key SSH uses: a file, or none (the agent and SSH's
/// defaults).
#[derive(Debug, Clone, Default)]
pub struct Identity {
    path: Option<PathBuf>,
    // Keeps a key written from NAVREE_HARBOR_SSH_KEY alive.
    _temp: Option<Arc<TempPath>>,
}

impl Identity {
    /// No key file: SSH's agent and default keys.
    pub fn agent() -> Self {
        Self::default()
    }

    /// The key at `path`.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            _temp: None,
        }
    }

    /// `NAVREE_HARBOR_SSH_KEY` (the key itself) when set; otherwise
    /// `ssh.key` (`~` is the home directory), which must exist; otherwise
    /// `~/.ssh/id_ed25519` when it exists; otherwise the agent.
    pub fn resolve(
        configured: Option<&str>,
        key_var: Option<&str>,
        home: Option<&Path>,
    ) -> Result<Identity> {
        if let Some(contents) = key_var.filter(|v| !v.trim().is_empty()) {
            let mut text = contents.replace("\r\n", "\n").trim().to_owned();
            text.push('\n');
            let file = tempfile::Builder::new()
                .prefix("navree-harbor-key-")
                .tempfile()
                .map_err(|e| HarborError::io(std::env::temp_dir(), e))?;
            let path = file.into_temp_path();
            crate::project::write_private(&path, &text)?;
            return Ok(Identity {
                path: Some(path.to_path_buf()),
                _temp: Some(Arc::new(path)),
            });
        }
        let expand = |p: &str| match (p.strip_prefix("~/"), home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(p),
        };
        if let Some(configured) = configured {
            let path = expand(configured);
            if !path.is_file() {
                return Err(HarborError::msg(format!(
                    "ssh.key is {} but there is no such file",
                    path.display()
                )));
            }
            return Ok(Identity::file(path));
        }
        match home.map(|h| h.join(".ssh").join("id_ed25519")) {
            Some(path) if path.is_file() => Ok(Identity::file(path)),
            _ => Ok(Identity::agent()),
        }
    }

    /// The key file, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The public half (`<key>.pub`), when it sits next to the key.
    pub fn public_key(&self) -> Option<String> {
        let path = self.path.as_ref()?;
        let mut public = path.as_os_str().to_owned();
        public.push(".pub");
        std::fs::read_to_string(PathBuf::from(public))
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    }
}

/// A connection to one server as one user.
pub struct Ssh {
    exec: Arc<dyn Executor>,
    server: String,
    host: String,
    user: String,
    port: u16,
    identity: Identity,
    pinned: bool,
    known_hosts: TempPath,
    sudo: bool,
}

impl std::fmt::Debug for Ssh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ssh")
            .field("server", &self.server)
            .field("destination", &self.destination())
            .field("port", &self.port)
            .finish()
    }
}

impl Ssh {
    /// A connection to `server` as `user` on `port`.
    pub fn new(
        exec: Arc<dyn Executor>,
        server: &Server,
        user: &str,
        port: u16,
        identity: Identity,
    ) -> Result<Ssh> {
        let known_hosts = known_hosts_file()?;
        let pinned = server.host_key.is_some();
        if let Some(key) = &server.host_key {
            let line = format!("{} {}\n", alias(&server.name), key.trim());
            std::fs::write(&known_hosts, line).map_err(|e| HarborError::io(&*known_hosts, e))?;
        }
        Ok(Ssh {
            exec,
            server: server.name.clone(),
            host: server.host.clone(),
            user: user.to_owned(),
            port,
            identity,
            pinned,
            known_hosts,
            sudo: false,
        })
    }

    /// Runs scripts with `sudo -n` (provisioning as a user other than
    /// root).
    pub fn with_sudo(mut self, sudo: bool) -> Self {
        self.sudo = sudo;
        self
    }

    /// The server's name.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// The options shared by every connection.
    fn options(&self) -> Vec<String> {
        let mut args = vec![
            "-p".to_owned(),
            self.port.to_string(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=15".to_owned(),
            "-o".to_owned(),
            "ServerAliveInterval=15".to_owned(),
            "-o".to_owned(),
            format!("HostKeyAlias={}", alias(&self.server)),
            "-o".to_owned(),
            known_hosts_option(&self.known_hosts),
            "-o".to_owned(),
            "HashKnownHosts=no".to_owned(),
            "-o".to_owned(),
            format!(
                "StrictHostKeyChecking={}",
                if self.pinned { "yes" } else { "accept-new" }
            ),
            "-o".to_owned(),
            "LogLevel=ERROR".to_owned(),
        ];
        if let Some(key) = self.identity.path() {
            args.extend([
                "-i".to_owned(),
                slash_path(key),
                "-o".to_owned(),
                "IdentitiesOnly=yes".to_owned(),
            ]);
        }
        args
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// The `ssh` command running `remote` (one shell command line).
    pub fn command(&self, remote: &str) -> Cmd {
        Cmd::new("ssh")
            .args(self.options())
            .arg(self.destination())
            .arg(remote)
    }

    /// Runs `script` with bash (`set -euo pipefail` first) and returns its
    /// output; a failure is a [`HarborError::Remote`] naming `step`.
    pub fn script(&self, step: &str, script: &str) -> Result<Output> {
        let shell = if self.sudo {
            "sudo -n bash -s"
        } else {
            "bash -s"
        };
        let cmd = self.command(shell).stdin(wrap_script(script));
        let output = self.exec.run(&cmd)?;
        if output.success() {
            Ok(output)
        } else {
            Err(self.failure(step, &output))
        }
    }

    /// A [`HarborError::Remote`] for `step` from `output`.
    pub fn failure(&self, step: &str, output: &Output) -> HarborError {
        let mut text = tail(&output.combined(), 25);
        if output.code == Some(255) && text.is_empty() {
            text = format!(
                "cannot connect to {}@{}:{}",
                self.user, self.host, self.port
            );
        }
        HarborError::Remote {
            server: self.server.clone(),
            step: step.to_owned(),
            code: output.code,
            output: text,
        }
    }

    /// Streams the local file `local` to `remote` on the server.
    pub fn upload(&self, local: &Path, remote: &str) -> Result<()> {
        let cmd = self
            .command(&format!("cat > {}", quote(remote)))
            .stdin_file(local);
        let output = self.exec.run(&cmd)?;
        if output.success() {
            Ok(())
        } else {
            Err(self.failure("upload", &output))
        }
    }

    /// Copies `remote` from the server to the local file `local`.
    pub fn download(&self, remote: &str, local: &Path) -> Result<()> {
        let cmd = self
            .command(&format!("cat {}", quote(remote)))
            .stdout_to(local);
        let output = self.exec.run(&cmd)?;
        if output.success() {
            Ok(())
        } else {
            Err(self.failure("download", &output))
        }
    }

    /// Runs `remote` (or a login shell) on a terminal and returns its exit
    /// code.
    pub fn interactive(&self, remote: Option<&str>) -> Result<Option<i32>> {
        let mut cmd = Cmd::new("ssh").arg("-t").args(self.options());
        cmd = cmd.arg(self.destination());
        if let Some(remote) = remote {
            cmd = cmd.arg(remote);
        }
        self.exec.interactive(&cmd)
    }

    /// The host key the server presented on the first connection, when it
    /// was not pinned yet: `ssh-ed25519 AAAA...`.
    pub fn learned_host_key(&self) -> Option<String> {
        if self.pinned {
            return None;
        }
        let text = std::fs::read_to_string(&self.known_hosts).ok()?;
        text.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            let (_host, kind, key) = (parts.next()?, parts.next()?, parts.next()?);
            Some(format!("{kind} {key}"))
        })
    }

    /// Where the known-hosts file is (tests write a learned key there).
    pub fn known_hosts_path(&self) -> &Path {
        &self.known_hosts
    }
}

/// A new empty file for the connection's known hosts. SSH splits a
/// `UserKnownHostsFile` value at spaces, so a path with one needs quotes in
/// the option, and on Windows the quotes do not survive: Rust escapes them
/// as `\"`, which the `ssh.exe` of Git for Windows (an MSYS program) does
/// not unescape ("command-line line 0: invalid quotes"). So the file goes
/// in a directory whose path has no whitespace when there is one.
fn known_hosts_file() -> Result<TempPath> {
    let mut dirs = vec![std::env::temp_dir()];
    if cfg!(windows) {
        dirs.extend(std::env::var_os("ProgramData").map(PathBuf::from));
        dirs.push(PathBuf::from("C:/Users/Public"));
    }
    let plain = |dir: &PathBuf| !slash_path(dir).contains(char::is_whitespace);
    // The first directory that works and has a plain path; else the first
    // that works (the option is then quoted).
    let mut fallback = None;
    for dir in dirs.iter().filter(|dir| plain(dir)).chain(dirs.iter()) {
        let made = tempfile::Builder::new()
            .prefix("navree-harbor-known-hosts-")
            .tempfile_in(dir);
        if let Ok(file) = made {
            let path = file.into_temp_path();
            if plain(&path.to_path_buf()) {
                return Ok(path);
            }
            fallback.get_or_insert(path);
        }
    }
    fallback.ok_or_else(|| {
        HarborError::io(
            std::env::temp_dir(),
            std::io::Error::other("cannot create a known-hosts file"),
        )
    })
}

/// The `-o` value naming the known-hosts file: the path with `/`
/// separators, and quotes only when it has whitespace.
fn known_hosts_option(path: &Path) -> String {
    let path = slash_path(path);
    if path.contains(char::is_whitespace) {
        format!("UserKnownHostsFile=\"{path}\"")
    } else {
        format!("UserKnownHostsFile={path}")
    }
}

/// What `bash -s` reads: strict mode, then the script in one brace group
/// with standard input from `/dev/null`. Bash parses the whole group before
/// running it, so no command in the script (apt-get, psql) can read the rest
/// of the script from the SSH channel.
pub fn wrap_script(script: &str) -> String {
    let newline = if script.ends_with('\n') { "" } else { "\n" };
    format!("set -euo pipefail\n{{\n{script}{newline}}} < /dev/null\n")
}

/// The host-key alias of a server: `harbor-<name>`.
pub fn alias(server: &str) -> String {
    format!("harbor-{server}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Role;
    use crate::exec::FakeExecutor;

    fn server(host_key: Option<&str>) -> Server {
        Server {
            name: "app-1".into(),
            host: "203.0.113.10".into(),
            private_ip: None,
            roles: vec![Role::Web],
            host_key: host_key.map(str::to_owned),
        }
    }

    fn ssh(fake: &FakeExecutor, host_key: Option<&str>, identity: Identity) -> Ssh {
        Ssh::new(
            Arc::new(fake.clone()),
            &server(host_key),
            "navree",
            22,
            identity,
        )
        .unwrap()
    }

    #[test]
    fn the_known_hosts_option_has_no_quotes_unless_the_path_has_spaces() {
        assert_eq!(
            known_hosts_option(Path::new(r"C:\Users\harpreet\AppData\Local\Temp\kh-1")),
            "UserKnownHostsFile=C:/Users/harpreet/AppData/Local/Temp/kh-1"
        );
        assert_eq!(
            known_hosts_option(Path::new("/tmp/a b/kh-1")),
            "UserKnownHostsFile=\"/tmp/a b/kh-1\""
        );
    }

    #[test]
    fn connections_pass_no_quote_characters_to_ssh() {
        // Quotes inside an argument break the `ssh.exe` of Git for Windows
        // ("command-line line 0: invalid quotes"): with a plain temp
        // directory the options carry none.
        if std::env::temp_dir().to_string_lossy().contains(' ') && !cfg!(windows) {
            return;
        }
        let fake = FakeExecutor::default();
        let ssh = ssh(&fake, Some("ssh-ed25519 AAAAPINNED"), Identity::agent());
        ssh.script("status", "uptime").unwrap();
        let args = &fake.calls()[0].args;
        let known_hosts = args
            .iter()
            .find(|a| a.starts_with("UserKnownHostsFile="))
            .unwrap();
        assert!(!known_hosts.contains('"'), "{known_hosts}");
        assert!(args.iter().all(|a| !a.contains('"')), "{args:?}");
        assert!(!slash_path(ssh.known_hosts_path()).contains(char::is_whitespace));
    }

    #[test]
    fn pinned_keys_are_checked_strictly() {
        let fake = FakeExecutor::default();
        let ssh = ssh(&fake, Some("ssh-ed25519 AAAAPINNED"), Identity::agent());
        let pinned = std::fs::read_to_string(ssh.known_hosts_path()).unwrap();
        assert_eq!(pinned, "harbor-app-1 ssh-ed25519 AAAAPINNED\n");
        ssh.script("status", "uptime").unwrap();
        let call = &fake.calls()[0];
        assert_eq!(call.program, "ssh");
        assert!(call.args.contains(&"StrictHostKeyChecking=yes".to_owned()));
        assert!(call.args.contains(&"HostKeyAlias=harbor-app-1".to_owned()));
        assert!(call.args.contains(&"BatchMode=yes".to_owned()));
        assert_eq!(
            call.args[call.args.len() - 2..],
            ["navree@203.0.113.10", "bash -s"]
        );
        assert_eq!(
            call.stdin_text(),
            "set -euo pipefail\n{\nuptime\n} < /dev/null\n"
        );
        assert_eq!(ssh.learned_host_key(), None);
    }

    #[test]
    fn unpinned_servers_accept_and_report_their_key() {
        let fake = FakeExecutor::default();
        let ssh = ssh(&fake, None, Identity::agent());
        ssh.script("facts", "true").unwrap();
        assert!(
            fake.calls()[0]
                .args
                .contains(&"StrictHostKeyChecking=accept-new".to_owned())
        );
        assert_eq!(ssh.learned_host_key(), None);
        std::fs::write(ssh.known_hosts_path(), "harbor-app-1 ssh-ed25519 AAAANEW\n").unwrap();
        assert_eq!(
            ssh.learned_host_key().as_deref(),
            Some("ssh-ed25519 AAAANEW")
        );
    }

    #[test]
    fn failures_name_the_server_and_step() {
        let fake = FakeExecutor::new(|_| Output::failed(255, ""));
        let ssh = ssh(&fake, None, Identity::agent());
        let error = ssh.script("base", "true").unwrap_err();
        assert_eq!(
            error.to_string(),
            "base failed on app-1 (exit code 255):\ncannot connect to navree@203.0.113.10:22"
        );
    }

    #[test]
    fn sudo_scripts_and_uploads() {
        let fake = FakeExecutor::default();
        let ssh = ssh(&fake, None, Identity::agent()).with_sudo(true);
        ssh.script("base", "true").unwrap();
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let archive = dir.path().join("r.tar.gz");
        std::fs::write(&archive, b"archive").unwrap();
        ssh.upload(&archive, "/srv/app/releases/r.tar.gz").unwrap();
        ssh.download("/var/backups/x.dump", &dir.path().join("x.dump"))
            .unwrap();
        let calls = fake.calls();
        assert_eq!(calls[0].args.last().unwrap(), "sudo -n bash -s");
        assert_eq!(
            calls[1].args.last().unwrap(),
            "cat > '/srv/app/releases/r.tar.gz'"
        );
        assert_eq!(calls[1].stdin_text(), "archive");
        assert_eq!(calls[2].args.last().unwrap(), "cat '/var/backups/x.dump'");
        assert!(dir.path().join("x.dump").exists());
    }

    #[test]
    fn identities() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        // Nothing configured, no default key: the agent.
        let identity = Identity::resolve(None, None, Some(dir.path())).unwrap();
        assert_eq!(identity.path(), None);
        // The default key, with its public half.
        std::fs::create_dir_all(dir.path().join(".ssh")).unwrap();
        std::fs::write(dir.path().join(".ssh/id_ed25519"), "PRIVATE").unwrap();
        std::fs::write(
            dir.path().join(".ssh/id_ed25519.pub"),
            "ssh-ed25519 AAAA me\n",
        )
        .unwrap();
        let identity = Identity::resolve(None, None, Some(dir.path())).unwrap();
        assert_eq!(
            identity.path(),
            Some(dir.path().join(".ssh/id_ed25519").as_path())
        );
        assert_eq!(
            identity.public_key().as_deref(),
            Some("ssh-ed25519 AAAA me")
        );
        // A configured key must exist.
        assert!(Identity::resolve(Some("~/.ssh/missing"), None, Some(dir.path())).is_err());
        let identity =
            Identity::resolve(Some("~/.ssh/id_ed25519"), None, Some(dir.path())).unwrap();
        assert!(identity.path().unwrap().ends_with("id_ed25519"));
        // The variable wins, written to a private temporary file.
        let identity = Identity::resolve(
            Some("~/.ssh/missing"),
            Some("-----BEGIN KEY-----\r\nabc\r\n-----END KEY-----"),
            Some(dir.path()),
        )
        .unwrap();
        let path = identity.path().unwrap().to_path_buf();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "-----BEGIN KEY-----\nabc\n-----END KEY-----\n"
        );
        drop(identity);
        assert!(!path.exists());
    }

    #[test]
    fn a_key_file_is_passed_with_identities_only() {
        let fake = FakeExecutor::default();
        let ssh = ssh(&fake, None, Identity::file("C:\\keys\\ci"));
        ssh.interactive(None).unwrap();
        let call = &fake.calls()[0];
        assert_eq!(call.args[0], "-t");
        let joined = call.args.join(" ");
        assert!(
            joined.contains("-i C:/keys/ci -o IdentitiesOnly=yes"),
            "{joined}"
        );
        assert_eq!(call.args.last().unwrap(), "navree@203.0.113.10");
    }
}
