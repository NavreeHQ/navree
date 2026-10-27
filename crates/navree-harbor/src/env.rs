//! The environment files on a server.
//!
//! `/etc/<app>/env` is written on every deploy from three sources, later over
//! earlier: the settings Harbor knows, `[env]`, and the decrypted
//! `[secrets]`. Values are single-quoted the way both systemd's
//! `EnvironmentFile=` and a shell (`set -a; . /etc/<app>/env`) read them.

use std::collections::BTreeMap;

use crate::config::{HarborConfig, Role, Server};

/// An ordered list of `NAME=value` settings; setting a name again replaces
/// its value in place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvFile {
    entries: Vec<(String, String)>,
}

impl EnvFile {
    /// An empty file.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `name` to `value`.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let (name, value) = (name.into(), value.into());
        match self.entries.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((name, value)),
        }
    }

    /// The value of `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The settings, in order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    /// How many settings it holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The file: a header line, then `NAME='value'` lines.
    pub fn render(&self, header: &str) -> String {
        let mut out = format!("# {header}\n");
        for (name, value) in &self.entries {
            out.push_str(name);
            out.push('=');
            out.push_str(&quote(value));
            out.push('\n');
        }
        out
    }

    /// `NAME=value` lines without quoting, as `docker run --env-file` reads
    /// them (`harbor:secrets export`). Fails on a value with a line break,
    /// which that format cannot hold.
    pub fn render_plain(&self) -> Result<String, String> {
        let mut out = String::new();
        for (name, value) in &self.entries {
            if value.contains(['\n', '\r']) {
                return Err(name.clone());
            }
            out.push_str(&format!("{name}={value}\n"));
        }
        Ok(out)
    }
}

/// Single-quotes `value` for a shell or systemd: `it's` becomes
/// `'it'\''s'`.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// The URL workers and the scheduler on `server` relay broadcasts to:
/// Caddy's internal entry on the server itself, or on the first `web`
/// server's private address. `None` when no web server is reachable.
pub fn relay_url(config: &HarborConfig, server: &Server) -> Option<String> {
    let port = config.deploy.internal_port;
    if server.has(Role::Web) {
        return Some(format!("http://127.0.0.1:{port}"));
    }
    config
        .servers_with(Role::Web)
        .find_map(|web| web.private_ip.as_ref())
        .map(|ip| format!("http://{ip}:{port}"))
}

/// The settings Harbor knows for `server` and `release`.
pub fn harbor_settings(config: &HarborConfig, server: &Server, release: &str) -> EnvFile {
    let mut env = EnvFile::new();
    let mut proxies = vec!["127.0.0.1".to_owned(), "::1".to_owned()];
    proxies.extend(config.deploy.trusted_proxies.iter().cloned());
    env.set("APP_HTTP__TRUSTED_PROXIES", proxies.join(","));
    env.set("APP_DATABASE__MIGRATE_ON_BOOT", "false");
    if let Some(url) = relay_url(config, server) {
        env.set("APP_REALTIME__RELAY_URL", url);
    }
    env.set("APP_LIGHTHOUSE__RELEASE", release);
    env
}

/// What `/etc/<app>/env` holds on `server`, and how many settings came
/// from Harbor and `[env]` (the first number) and from `[secrets]` (the
/// second).
pub fn server_env(
    config: &HarborConfig,
    server: &Server,
    release: &str,
    secrets: &BTreeMap<String, String>,
) -> (EnvFile, usize, usize) {
    let mut env = harbor_settings(config, server, release);
    for (name, value) in &config.env {
        env.set(name, value);
    }
    let settings = env.len();
    for (name, value) in secrets {
        env.set(name, value);
    }
    (env, settings, secrets.len())
}

/// `/etc/<app>/<slot>.env`: where the slot listens and the release it runs.
pub fn slot_env(port: u16, release: &str) -> EnvFile {
    let mut env = EnvFile::new();
    env.set("APP_HTTP__ADDR", format!("127.0.0.1:{port}"));
    env.set("APP_LIGHTHOUSE__RELEASE", release);
    env
}

/// `/etc/<app>/frontend-<slot>.env`: the front end's port, and the API slot
/// of the same release it calls.
pub fn frontend_env(port: u16, api_port: u16) -> EnvFile {
    let mut env = EnvFile::new();
    env.set("PORT", port.to_string());
    env.set("HOSTNAME", "127.0.0.1");
    env.set("NODE_ENV", "production");
    env.set("NAVREE_API_URL", format!("http://127.0.0.1:{api_port}"));
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> HarborConfig {
        HarborConfig::parse(text, std::path::Path::new("harbor.toml")).unwrap()
    }

    #[test]
    fn quoting_survives_any_value() {
        assert_eq!(quote("plain"), "'plain'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote("$HOME `x` \"y\" \\z"), "'$HOME `x` \"y\" \\z'");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn later_sources_win_in_place() {
        let mut env = EnvFile::new();
        env.set("A", "1");
        env.set("B", "2");
        env.set("A", "3");
        assert_eq!(
            env.entries(),
            [
                ("A".to_owned(), "3".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
        assert_eq!(env.get("A"), Some("3"));
        assert_eq!(env.render("header"), "# header\nA='3'\nB='2'\n");
    }

    #[test]
    fn plain_rendering_refuses_line_breaks() {
        let mut env = EnvFile::new();
        env.set("A", "x y");
        assert_eq!(env.render_plain().unwrap(), "A=x y\n");
        env.set("PEM", "line1\nline2");
        assert_eq!(env.render_plain().unwrap_err(), "PEM");
    }

    #[test]
    fn the_three_sources_in_order() {
        let config = config(
            r#"
app = "crm-api"
domain = "crm.example.com"
[deploy]
trusted_proxies = ["10.0.0.0/24"]
[servers.app-1]
host = "203.0.113.10"
[env]
APP_LOG__LEVEL = "info"
APP_DATABASE__MIGRATE_ON_BOOT = "true"
"#,
        );
        let secrets = BTreeMap::from([("APP_KEY".to_owned(), "base64:k".to_owned())]);
        let (env, settings, secret_count) = server_env(
            &config,
            &config.servers[0],
            "20261026-091512-3f2a9c1",
            &secrets,
        );
        assert_eq!((settings, secret_count), (5, 1));
        insta::assert_snapshot!(env.render("Written by navree deploy."), @r"
        # Written by navree deploy.
        APP_HTTP__TRUSTED_PROXIES='127.0.0.1,::1,10.0.0.0/24'
        APP_DATABASE__MIGRATE_ON_BOOT='true'
        APP_REALTIME__RELAY_URL='http://127.0.0.1:8079'
        APP_LIGHTHOUSE__RELEASE='20261026-091512-3f2a9c1'
        APP_LOG__LEVEL='info'
        APP_KEY='base64:k'
        ");
    }

    #[test]
    fn workers_relay_to_a_web_server_on_the_private_network() {
        let config = config(
            r#"
app = "a"
domain = "a.example.com"
[servers.web-1]
host = "203.0.113.11"
private_ip = "10.0.0.11"
roles = ["web"]
[servers.worker-1]
host = "203.0.113.21"
private_ip = "10.0.0.21"
roles = ["worker"]
"#,
        );
        assert_eq!(
            relay_url(&config, &config.servers[1]).as_deref(),
            Some("http://10.0.0.11:8079")
        );
        let lonely =
            super::tests::config("app = \"a\"\n[servers.w]\nhost = \"x\"\nroles = [\"worker\"]\n");
        assert_eq!(relay_url(&lonely, &lonely.servers[0]), None);
    }

    #[test]
    fn slot_files() {
        assert_eq!(
            slot_env(8082, "r1").render("slot"),
            "# slot\nAPP_HTTP__ADDR='127.0.0.1:8082'\nAPP_LIGHTHOUSE__RELEASE='r1'\n"
        );
        let env = frontend_env(3002, 8082);
        assert_eq!(env.get("PORT"), Some("3002"));
        assert_eq!(env.get("NAVREE_API_URL"), Some("http://127.0.0.1:8082"));
    }
}
