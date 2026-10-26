//! `navree harbor:provision`: prepares Ubuntu servers for their roles.
//!
//! Each area (base system, users, SSH, firewall, PostgreSQL, Redis, Caddy,
//! Node.js, systemd) is one bash script, run over SSH as
//! `ssh.provision_user`. Every script is idempotent: files are written only
//! when their contents differ, packages installed only when missing, and
//! services reloaded only when something changed, so running it again
//! brings a server in line with `harbor.toml`.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;

use crate::config::{HarborConfig, Role, Server};
use crate::error::{HarborError, Result};
use crate::exec::Executor;
use crate::layout::{Layout, Slot};
use crate::output::{Clock, Progress, elapsed};
use crate::project::Project;
use crate::render::{self, AppInfo, context, heredoc_safe, with};
use crate::secrets::{SecretKey, generate_password};
use crate::ssh::{Identity, Ssh};

/// The secret holding the database URL.
pub const DATABASE_URL: &str = "APP_DATABASE__URL";
/// The secret holding the Redis URL (the name Dock uses locally too).
pub const REDIS_URL: &str = "REDIS_URL";

/// What the `facts` script reports about a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// `ubuntu`.
    pub id: String,
    /// `24.04`.
    pub version: String,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// Total memory.
    pub memory_mb: u64,
}

impl Facts {
    /// Parses the `facts` script's `key=value` lines.
    pub fn parse(stdout: &str) -> Facts {
        let values = key_values(stdout);
        let get = |k: &str| values.get(k).cloned().unwrap_or_default();
        Facts {
            id: get("id"),
            version: get("version"),
            arch: get("arch"),
            memory_mb: get("memory_mb").parse().unwrap_or(0),
        }
    }

    /// `Ubuntu 24.04, x86_64`.
    pub fn describe(&self) -> String {
        let name = if self.id == "ubuntu" {
            "Ubuntu"
        } else {
            &self.id
        };
        format!("{name} {}, {}", self.version, self.arch)
    }

    /// Fails unless the server runs Ubuntu 22.04 or later on x86-64 or
    /// ARM64.
    pub fn check(&self, server: &str) -> Result<()> {
        let major: u32 = self
            .version
            .split('.')
            .next()
            .and_then(|m| m.parse().ok())
            .unwrap_or(0);
        if self.id != "ubuntu" || major < 22 {
            return Err(HarborError::msg(format!(
                "{server} runs {} {}: Harbor provisions Ubuntu 22.04 or 24.04 LTS",
                self.id, self.version
            )));
        }
        if !matches!(self.arch.as_str(), "x86_64" | "aarch64") {
            return Err(HarborError::msg(format!(
                "{server} is {}: Harbor supports x86_64 and aarch64 servers",
                self.arch
            )));
        }
        Ok(())
    }

    /// The swap file Harbor adds, in MB: 2 GB below 4 GB of memory.
    pub fn swap_mb(&self) -> u64 {
        if self.memory_mb > 0 && self.memory_mb < 4000 {
            2048
        } else {
            0
        }
    }
}

/// `key=value` lines into a map (the last value of a key wins).
pub fn key_values(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// The generated passwords of the data services on a server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Passwords {
    /// The app's PostgreSQL user.
    pub postgres: Option<String>,
    /// Redis's `requirepass`.
    pub redis: Option<String>,
}

/// One provisioning script and the line it prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// `base`, `users`, `ssh`, ...
    pub area: &'static str,
    /// What it sets up, for the progress line.
    pub summary: String,
    /// The bash script.
    pub script: String,
}

/// The `facts` script.
pub fn facts_script(config: &HarborConfig, server: &Server, info: &AppInfo) -> Result<String> {
    render::render("provision/facts.sh", context(config, server, info))
}

/// The database URL every server's processes use.
pub fn database_url(config: &HarborConfig, password: &str) -> Option<String> {
    let db = config.postgres_server()?;
    let name = config.database();
    let (host, query) = if config.only_local_clients(db) {
        ("127.0.0.1".to_owned(), "")
    } else if let Some(ip) = &db.private_ip {
        (ip.clone(), "")
    } else {
        (db.host.clone(), "?sslmode=require")
    };
    Some(format!(
        "postgres://{name}:{password}@{host}:5432/{name}{query}"
    ))
}

/// The Redis URL every server's processes use.
pub fn redis_url(config: &HarborConfig, password: &str) -> Option<String> {
    let redis = config.redis_server()?;
    let host = if config.only_local_clients(redis) {
        "127.0.0.1".to_owned()
    } else {
        redis.internal_address().to_owned()
    };
    Some(format!("redis://:{password}@{host}:6379/0"))
}

/// The password in a `scheme://user:password@host` URL.
pub fn password_in_url(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let credentials = rest.split_once('@')?.0;
    let password = credentials.split_once(':')?.1;
    (!password.is_empty() && password.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| password.to_owned())
}

/// Every provisioning step for `server`, in order.
pub fn plan(
    config: &HarborConfig,
    server: &Server,
    info: &AppInfo,
    facts: &Facts,
    passwords: &Passwords,
    authorized_keys: &[String],
) -> Result<Vec<Step>> {
    let layout = Layout::new(config);
    let base = context(config, server, info);
    let file = |name: &str, extra: serde_json::Value| -> Result<String> {
        let content = render::render(&format!("files/{name}"), with(base.clone(), extra))?;
        heredoc_safe(name, &content)?;
        Ok(content)
    };
    let conf = |name: &str, extra: serde_json::Value| -> Result<String> {
        let mut extra = extra;
        extra["name"] = json!(name);
        let content = render::render("files/server.conf", with(base.clone(), extra))?;
        heredoc_safe(name, &content)?;
        Ok(content)
    };
    let script = |name: &str, files: serde_json::Value, extra: serde_json::Value| {
        let mut context = with(base.clone(), extra);
        context["files"] = files;
        render::render(&format!("provision/{name}.sh"), context)
    };
    let mut steps = Vec::new();

    // Base system.
    let swap_mb = facts.swap_mb();
    steps.push(Step {
        area: "base",
        summary: if swap_mb > 0 {
            format!(
                "updates, unattended upgrades, chrony, {} GB swap, UTC",
                swap_mb / 1024
            )
        } else {
            "updates, unattended upgrades, chrony, UTC".to_owned()
        },
        script: script(
            "base",
            json!({
                "auto_upgrades": conf("auto-upgrades", json!({}))?,
                "journald": conf("journald", json!({}))?,
            }),
            json!({ "swap_mb": swap_mb }),
        )?,
    });

    // Users.
    let mut keys = authorized_keys.join("\n");
    keys.push('\n');
    heredoc_safe("ssh.authorized_keys", &keys)?;
    let sudoers = file(
        "sudoers",
        json!({
            "alias": render::sudo_alias(&config.app),
            "sudo_commands": render::sudo_commands(config, server, info),
        }),
    )?;
    steps.push(Step {
        area: "users",
        summary: format!(
            "{}, with {} SSH key{}",
            layout.user,
            authorized_keys.len(),
            if authorized_keys.len() == 1 { "" } else { "s" }
        ),
        script: script(
            "users",
            json!({ "authorized_keys": keys, "sudoers": sudoers }),
            json!({}),
        )?,
    });

    // SSH.
    steps.push(Step {
        area: "ssh",
        summary: format!(
            "no passwords, root by key only, host key {}",
            if server.host_key.is_some() {
                "checked"
            } else {
                "recorded"
            }
        ),
        script: script(
            "ssh",
            json!({ "sshd": conf("sshd", json!({}))? }),
            json!({}),
        )?,
    });

    // Firewall.
    let rules = render::firewall_rules(config, server);
    steps.push(Step {
        area: "firewall",
        summary: format!("ufw: {}; fail2ban", describe_rules(&rules)),
        script: script(
            "firewall",
            json!({ "fail2ban": conf("fail2ban", json!({}))? }),
            json!({ "firewall_rules": rules }),
        )?,
    });

    // PostgreSQL.
    if server.has(Role::Postgres) {
        let password = passwords
            .postgres
            .clone()
            .ok_or_else(|| HarborError::msg("no PostgreSQL password"))?;
        let clients: Vec<serde_json::Value> = config
            .servers
            .iter()
            .filter(|s| s.name != server.name && s.runs_app())
            .map(|s| {
                let private = server.private_ip.is_some();
                let address = if private {
                    s.private_ip.clone().unwrap_or_else(|| s.host.clone())
                } else {
                    s.host.clone()
                };
                let bits = if address.contains(':') { 128 } else { 32 };
                json!({
                    "kind": if private { "host   " } else { "hostssl" },
                    "address": address,
                    "bits": bits,
                })
            })
            .collect();
        let listen = if clients.is_empty() {
            "localhost".to_owned()
        } else if let Some(ip) = &server.private_ip {
            format!("localhost,{ip}")
        } else {
            "*".to_owned()
        };
        let postgres = json!({
            "version": config.postgres.version,
            "password": password,
            "listen": listen,
            "clients": clients,
        });
        let extra = json!({ "postgres": postgres });
        steps.push(Step {
            area: "postgres",
            summary: format!(
                "PostgreSQL {}, database {}",
                config.postgres.version,
                config.database()
            ),
            script: script(
                "postgres",
                json!({
                    "postgres": conf("postgres", extra.clone())?,
                    "pg_hba": conf("pg_hba", extra.clone())?,
                    "backup_script": file("backup.sh", json!({}))?,
                    "backup_service": file("backup.service", json!({}))?,
                    "backup_timer": file("backup.timer", json!({}))?,
                }),
                extra,
            )?,
        });
    }

    // Redis.
    if server.has(Role::Redis) {
        let password = passwords
            .redis
            .clone()
            .ok_or_else(|| HarborError::msg("no Redis password"))?;
        let remote = !config.only_local_clients(server);
        // Plain addresses only: the `-` (optional address) prefix needs
        // Redis 6.2, and Ubuntu 22.04 ships 6.0. The app connects to
        // 127.0.0.1, so IPv6 loopback is not needed.
        let mut addresses = vec!["127.0.0.1".to_owned()];
        if remote && let Some(ip) = &server.private_ip {
            addresses.push(ip.clone());
        }
        let redis = json!({
            "password": password,
            "bind": addresses.join(" "),
            "maxmemory_mb": config.redis.maxmemory_mb,
        });
        steps.push(Step {
            area: "redis",
            summary: format!(
                "Redis on {}, {} MB",
                addresses.join(" and "),
                config.redis.maxmemory_mb
            ),
            script: script(
                "redis",
                json!({ "redis": conf("redis", json!({ "redis": redis }))? }),
                json!({}),
            )?,
        });
    }

    // Caddy.
    if server.has(Role::Web) {
        let mut sites = vec![config.domain.clone().unwrap_or_default()];
        sites.extend(config.api_domain.clone());
        let site_blue = render::site(config, server, info, Slot::Blue)?;
        let site_green = render::site(config, server, info, Slot::Green)?;
        heredoc_safe("the Caddy site", &site_blue)?;
        steps.push(Step {
            area: "caddy",
            summary: format!("Caddy 2, site {}", sites.join(", ")),
            script: script(
                "caddy",
                json!({
                    "caddyfile": file("Caddyfile", json!({}))?,
                    "site_blue": site_blue,
                    "site_green": site_green,
                }),
                json!({}),
            )?,
        });
        if let Some(frontend) = &info.frontend {
            steps.push(Step {
                area: "node",
                summary: format!("Node.js {}, {}", frontend.node_version, frontend.manager),
                script: script("node", json!({}), json!({}))?,
            });
        }
    }

    // systemd.
    let mut process_units = Vec::new();
    let mut shown = Vec::new();
    if server.has(Role::Web) {
        shown.push(format!("{}, @green", layout.web_unit(Slot::Blue)));
        if info.frontend.is_some() {
            shown.push(format!("{}, @green", layout.frontend_unit(Slot::Blue)));
        }
    }
    if server.has(Role::Worker) {
        let instances = config.processes.worker.instances;
        let mut names: Vec<String> = (1..=instances).map(|i| format!("@{i}")).collect();
        names[0] = layout.worker_unit(1);
        shown.push(names.join(", "));
        process_units.extend((1..=instances).map(|i| layout.worker_unit(i)));
    }
    if server.has(Role::Scheduler) {
        shown.push(layout.scheduler_unit());
        process_units.push(layout.scheduler_unit());
    }
    if server.has(Role::Postgres) {
        shown.push(format!("{}.timer", layout.backup_unit()));
    }
    let mut files = json!({});
    if server.has(Role::Web) {
        files["web_unit"] = json!(file("web.service", json!({}))?);
        if info.frontend.is_some() {
            files["frontend_unit"] = json!(file("frontend.service", json!({}))?);
        }
    }
    if server.has(Role::Worker) {
        files["worker_unit"] = json!(file("worker.service", json!({}))?);
    }
    if server.has(Role::Scheduler) {
        files["scheduler_unit"] = json!(file("scheduler.service", json!({}))?);
    }
    steps.push(Step {
        area: "systemd",
        summary: if shown.is_empty() {
            "no app processes".to_owned()
        } else {
            shown.join(", ")
        },
        script: script("systemd", files, json!({ "process_units": process_units }))?,
    });
    Ok(steps)
}

/// `22/tcp, 80/tcp, 443/tcp, 443/udp, 5432/tcp from 2 servers`.
fn describe_rules(rules: &[String]) -> String {
    let mut open = Vec::new();
    let mut from: BTreeMap<String, usize> = BTreeMap::new();
    for rule in rules {
        let words: Vec<&str> = rule.split_whitespace().collect();
        match words.as_slice() {
            [
                "allow",
                "from",
                _,
                "to",
                "any",
                "port",
                port,
                "proto",
                proto,
                ..,
            ] => {
                *from.entry(format!("{port}/{proto}")).or_default() += 1;
            }
            ["allow", port, ..] => open.push((*port).to_owned()),
            _ => {}
        }
    }
    for (port, count) in from {
        open.push(format!(
            "{port} from {count} server{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    open.join(", ")
}

/// Runs `harbor:provision`.
pub struct Provisioner<'a> {
    /// Runs `ssh`.
    pub exec: Arc<dyn Executor>,
    /// The SSH key.
    pub identity: Identity,
    /// Where progress lines go.
    pub progress: &'a dyn Progress,
    /// The clock, for durations.
    pub clock: &'a dyn Clock,
}

impl Provisioner<'_> {
    /// Provisions every server, or only `only`. Pins new host keys and
    /// stores the database and Redis URLs in `[secrets]` (which needs the
    /// secrets `key` when a server has the `postgres` or `redis` role).
    pub fn run(
        &self,
        project: &mut Project,
        key: Option<&SecretKey>,
        only: Option<&str>,
    ) -> Result<()> {
        let names: Vec<String> = match only {
            Some(name) => vec![project.config.server(name)?.name.clone()],
            None => project
                .config
                .servers
                .iter()
                .map(|s| s.name.clone())
                .collect(),
        };
        let authorized_keys = match &project.config.ssh.authorized_keys {
            Some(keys) if !keys.is_empty() => keys.clone(),
            _ => vec![self.identity.public_key().ok_or_else(|| {
                HarborError::msg(
                    "no public key for the deploy user: set ssh.authorized_keys in harbor.toml, or keep the public half of ssh.key next to it (<key>.pub)",
                )
            })?],
        };
        let info = AppInfo::detect(&project.config, &project.dir);
        for name in names {
            self.server(project, key, &name, &info, &authorized_keys)?;
        }
        Ok(())
    }

    fn server(
        &self,
        project: &mut Project,
        key: Option<&SecretKey>,
        name: &str,
        info: &AppInfo,
        authorized_keys: &[String],
    ) -> Result<()> {
        let started = self.clock.now();
        let server = project.config.server(name)?.clone();
        let data = server.has(Role::Postgres) || server.has(Role::Redis);
        let key = match key {
            Some(key) => Some(key),
            None if data => {
                return Err(HarborError::MissingKey {
                    path: project.key_path(),
                });
            }
            None => None,
        };
        let user = project.config.ssh.provision_user.clone();
        let ssh = Ssh::new(
            self.exec.clone(),
            &server,
            &user,
            project.config.ssh.port,
            self.identity.clone(),
        )?
        .with_sudo(user != "root");

        let output = ssh.script("facts", &facts_script(&project.config, &server, info)?)?;
        let facts = Facts::parse(&output.stdout);
        if let Some(host_key) = ssh.learned_host_key() {
            project.set_host_key(name, &host_key)?;
        }
        facts.check(name)?;
        self.progress.line(&format!(
            "Provisioning {name} ({}, {})",
            server.host,
            facts.describe()
        ));

        let secrets = match key {
            Some(key) => project.decrypt_secrets(key)?,
            None => BTreeMap::new(),
        };
        let reuse = |secret: &str| secrets.get(secret).and_then(|url| password_in_url(url));
        let passwords = Passwords {
            postgres: server
                .has(Role::Postgres)
                .then(|| reuse(DATABASE_URL).unwrap_or_else(|| generate_password(32))),
            redis: server
                .has(Role::Redis)
                .then(|| reuse(REDIS_URL).unwrap_or_else(|| generate_password(32))),
        };
        let steps = plan(
            &project.config,
            &server,
            info,
            &facts,
            &passwords,
            authorized_keys,
        )?;
        for step in steps {
            ssh.script(step.area, &step.script)?;
            let mut summary = step.summary;
            let saved = match step.area {
                "postgres" => passwords
                    .postgres
                    .as_deref()
                    .and_then(|p| database_url(&project.config, p))
                    .map(|url| (DATABASE_URL, url)),
                "redis" => passwords
                    .redis
                    .as_deref()
                    .and_then(|p| redis_url(&project.config, p))
                    .map(|url| (REDIS_URL, url)),
                _ => None,
            };
            if let (Some((secret, url)), Some(key)) = (saved, key) {
                if secrets.get(secret) == Some(&url) {
                    summary.push_str(&format!("; URL in [secrets] as {secret}"));
                } else {
                    project.set_secret(key, secret, &url)?;
                    summary.push_str(&format!("; URL saved to [secrets] as {secret}"));
                }
            }
            self.progress.step(step.area, &summary);
        }
        self.progress.line(&format!(
            "Provisioned {name} in {}. Run `navree deploy{}` next.",
            elapsed(self.clock, started),
            project.env.flag()
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> HarborConfig {
        HarborConfig::parse(text, std::path::Path::new("harbor.toml")).unwrap()
    }

    #[test]
    fn facts_are_parsed_and_checked() {
        let facts = Facts::parse("id=ubuntu\nversion=24.04\narch=x86_64\nmemory_mb=1987\n");
        assert_eq!(facts.describe(), "Ubuntu 24.04, x86_64");
        assert!(facts.check("app-1").is_ok());
        assert_eq!(facts.swap_mb(), 2048);
        let big = Facts {
            memory_mb: 8000,
            ..facts.clone()
        };
        assert_eq!(big.swap_mb(), 0);
        let debian = Facts::parse("id=debian\nversion=12\narch=x86_64\n");
        let error = debian.check("app-1").unwrap_err().to_string();
        assert!(error.contains("Ubuntu 22.04 or 24.04"), "{error}");
        let old = Facts::parse("id=ubuntu\nversion=20.04\narch=x86_64\n");
        assert!(old.check("app-1").is_err());
        let arm = Facts::parse("id=ubuntu\nversion=22.04\narch=aarch64\n");
        assert!(arm.check("app-1").is_ok());
        let odd = Facts::parse("id=ubuntu\nversion=24.04\narch=riscv64\n");
        assert!(odd.check("app-1").is_err());
    }

    #[test]
    fn urls_point_at_the_data_server() {
        let single = config(
            "app = \"crm-api\"\ndomain = \"a.example.com\"\n[servers.a]\nhost = \"203.0.113.10\"\nroles = [\"web\", \"postgres\", \"redis\"]\n",
        );
        assert_eq!(
            database_url(&single, "pw").as_deref(),
            Some("postgres://crm_api:pw@127.0.0.1:5432/crm_api")
        );
        assert_eq!(
            redis_url(&single, "pw").as_deref(),
            Some("redis://:pw@127.0.0.1:6379/0")
        );
        let split = config(
            r#"
app = "crm-api"
domain = "a.example.com"
[servers.web]
host = "203.0.113.11"
private_ip = "10.0.0.11"
roles = ["web"]
[servers.data]
host = "203.0.113.31"
private_ip = "10.0.0.31"
roles = ["postgres", "redis"]
"#,
        );
        assert_eq!(
            database_url(&split, "pw").as_deref(),
            Some("postgres://crm_api:pw@10.0.0.31:5432/crm_api")
        );
        assert_eq!(
            redis_url(&split, "pw").as_deref(),
            Some("redis://:pw@10.0.0.31:6379/0")
        );
        let public = config(
            r#"
app = "crm-api"
domain = "a.example.com"
[servers.web]
host = "203.0.113.11"
roles = ["web"]
[servers.data]
host = "203.0.113.31"
roles = ["postgres"]
"#,
        );
        assert_eq!(
            database_url(&public, "pw").as_deref(),
            Some("postgres://crm_api:pw@203.0.113.31:5432/crm_api?sslmode=require")
        );
        assert_eq!(redis_url(&public, "pw"), None);
    }

    #[test]
    fn passwords_are_read_back_from_urls() {
        assert_eq!(
            password_in_url("postgres://crm_api:Abc123@127.0.0.1:5432/crm_api").as_deref(),
            Some("Abc123")
        );
        assert_eq!(
            password_in_url("redis://:Xyz9@10.0.0.31:6379/0").as_deref(),
            Some("Xyz9")
        );
        assert_eq!(password_in_url("postgres://user@host/db"), None);
        assert_eq!(password_in_url("postgres://u:p%40ss@host/db"), None);
        assert_eq!(password_in_url("nonsense"), None);
    }

    #[test]
    fn firewall_rules_are_summarised() {
        let rules = [
            "allow 22/tcp comment 'harbor'".to_owned(),
            "allow 80/tcp comment 'harbor'".to_owned(),
            "allow from 10.0.0.11 to any port 5432 proto tcp comment 'harbor'".to_owned(),
            "allow from 10.0.0.12 to any port 5432 proto tcp comment 'harbor'".to_owned(),
            "allow from 10.0.0.12 to any port 6379 proto tcp comment 'harbor'".to_owned(),
        ];
        assert_eq!(
            describe_rules(&rules),
            "22/tcp, 80/tcp, 5432/tcp from 2 servers, 6379/tcp from 1 server"
        );
    }
}
