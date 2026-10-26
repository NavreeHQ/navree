//! The embedded MiniJinja templates of every file and script Harbor writes,
//! and the context they render with.
//!
//! Templates live in `crates/navree-harbor/templates/<name>.j2`. Blocks trim
//! their own line, the trailing newline is kept, nothing is escaped, and an
//! undefined variable is an error.

use std::path::Path;
use std::sync::OnceLock;

use minijinja::syntax::SyntaxConfig;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Serialize;
use serde_json::{Value, json};

use crate::config::{HarborConfig, Role, Server};
use crate::error::{HarborError, Result};
use crate::layout::{Layout, Slot};

macro_rules! templates {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../templates/", $name, ".j2")))),*]
    };
}

/// `(name, source)` of every template.
const TEMPLATES: &[(&str, &str)] = templates![
    "files/web.service",
    "files/worker.service",
    "files/scheduler.service",
    "files/frontend.service",
    "files/Caddyfile",
    "files/site.caddy",
    "files/sudoers",
    "files/backup.sh",
    "files/backup.service",
    "files/backup.timer",
    "files/server.conf",
    "provision/prelude.sh",
    "provision/facts.sh",
    "provision/base.sh",
    "provision/users.sh",
    "provision/ssh.sh",
    "provision/firewall.sh",
    "provision/postgres.sh",
    "provision/redis.sh",
    "provision/caddy.sh",
    "provision/node.sh",
    "provision/systemd.sh",
    "deploy/inspect.sh",
    "deploy/unpack.sh",
    "deploy/env.sh",
    "deploy/run.sh",
    "deploy/start.sh",
    "deploy/health.sh",
    "deploy/units.sh",
    "deploy/logs.sh",
    "deploy/switch.sh",
    "deploy/current.sh",
    "deploy/prune.sh",
    "deploy/status.sh",
    "deploy/backup.sh",
    "image/Dockerfile",
    "image/dockerignore",
    "image/builder.Dockerfile",
    "ci/github.yml",
    "init/harbor.toml",
];

/// The heredoc delimiter scripts use; no embedded file may hold it as a line.
pub const HEREDOC: &str = "HARBOR_EOF";

fn environment() -> &'static Environment<'static> {
    static ENV: OnceLock<Environment<'static>> = OnceLock::new();
    ENV.get_or_init(|| {
        let mut env = Environment::new();
        env.set_syntax(
            SyntaxConfig::builder()
                .trim_blocks(true)
                .lstrip_blocks(true)
                .keep_trailing_newline(true)
                .build()
                .expect("the default delimiters are valid"),
        );
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_auto_escape_callback(|_| AutoEscape::None);
        for (name, source) in TEMPLATES {
            env.add_template(name, source)
                .unwrap_or_else(|error| panic!("embedded template `{name}` is invalid: {error}"));
        }
        env
    })
}

/// Renders template `name` with `context`.
pub fn render(name: &str, context: impl Serialize) -> Result<String> {
    let error = |e: minijinja::Error| HarborError::msg(format!("cannot render `{name}`: {e:#}"));
    environment()
        .get_template(name)
        .map_err(error)?
        .render(minijinja::value::Serde(&context))
        .map_err(error)
}

/// Fails when `content` has a line that would end a heredoc early.
pub fn heredoc_safe(what: &str, content: &str) -> Result<()> {
    if content.lines().any(|line| line.trim() == HEREDOC) {
        return Err(HarborError::msg(format!(
            "{what} contains a line `{HEREDOC}`, which Harbor's scripts cannot carry"
        )));
    }
    Ok(())
}

/// What Harbor learns about the app from its files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppInfo {
    /// The app serves gRPC (`navree` with the `grpc` feature, or
    /// `navree-grpc`), so Caddy proxies with HTTP/2 (`h2c://`).
    pub h2c: bool,
    /// The front end, with its defaults resolved.
    pub frontend: Option<FrontendInfo>,
}

/// A `[frontend]` with its build and run commands resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrontendInfo {
    /// Its directory, relative to the app.
    pub path: String,
    /// `npm`, `pnpm` or `yarn`, from the lockfile.
    pub manager: String,
    /// How to build it.
    pub build: String,
    /// How to run it.
    pub command: String,
    /// The path that must answer 200 before a switch.
    pub health_check: String,
    /// The blue and green ports.
    pub ports: [u16; 2],
    /// The Node.js major version.
    pub node_version: u32,
}

impl AppInfo {
    /// Reads the app in `dir`: its `Cargo.toml` and the front end's
    /// lockfile.
    pub fn detect(config: &HarborConfig, dir: &Path) -> AppInfo {
        let h2c = std::fs::read_to_string(dir.join("Cargo.toml"))
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
            .is_some_and(|manifest| uses_grpc(&manifest));
        let frontend = config.frontend.as_ref().map(|f| {
            let root = dir.join(&f.path);
            let manager = if root.join("pnpm-lock.yaml").is_file() {
                "pnpm"
            } else if root.join("yarn.lock").is_file() {
                "yarn"
            } else {
                "npm"
            };
            let (build, command) = match manager {
                "pnpm" => ("pnpm install --frozen-lockfile && pnpm build", "pnpm start"),
                "yarn" => ("yarn install --frozen-lockfile && yarn build", "yarn start"),
                _ => ("npm ci && npm run build", "npm start"),
            };
            FrontendInfo {
                path: f.path.clone(),
                manager: manager.to_owned(),
                build: f.build.clone().unwrap_or_else(|| build.to_owned()),
                command: f.command.clone().unwrap_or_else(|| command.to_owned()),
                health_check: f.health_check.clone(),
                ports: f.ports,
                node_version: f.node_version,
            }
        });
        AppInfo { h2c, frontend }
    }
}

fn uses_grpc(manifest: &toml::Table) -> bool {
    let Some(deps) = manifest.get("dependencies").and_then(|d| d.as_table()) else {
        return false;
    };
    deps.contains_key("navree-grpc")
        || deps
            .get("navree")
            .and_then(|n| n.get("features"))
            .and_then(|f| f.as_array())
            .is_some_and(|features| features.iter().any(|f| f.as_str() == Some("grpc")))
}

/// What the systemd units start after on `server`.
fn after(server: &Server) -> String {
    let mut after = vec!["network-online.target"];
    if server.has(Role::Postgres) {
        after.push("postgresql.service");
    }
    if server.has(Role::Redis) {
        after.push("redis-server.service");
    }
    after.join(" ")
}

/// The addresses Caddy's internal entry listens on: loopback, plus the
/// private address when servers without `web` relay through it.
fn internal_bind(config: &HarborConfig, server: &Server) -> Vec<String> {
    let mut bind = vec!["127.0.0.1".to_owned()];
    let remote_relays = config
        .servers
        .iter()
        .any(|s| !s.has(Role::Web) && s.runs_app() && s.private_ip.is_some());
    if remote_relays && let Some(ip) = &server.private_ip {
        bind.push(ip.clone());
    }
    bind
}

/// The context every server-side template renders with.
pub fn context(config: &HarborConfig, server: &Server, info: &AppInfo) -> Value {
    let layout = Layout::new(config);
    let has = |role| server.has(role);
    json!({
        "app": config.app,
        "user": layout.user,
        "base": layout.base,
        "etc": layout.etc,
        "caddy_site": layout.caddy_site(),
        "backup_dir": layout.backup_dir(),
        "database": config.database(),
        "backup_days": config.postgres.backup_days,
        "domain": config.domain,
        "aliases": config.aliases,
        "api_domain": config.api_domain,
        "ports": config.deploy.ports,
        "internal_port": config.deploy.internal_port,
        "internal_bind": internal_bind(config, server),
        "metrics_allow": config.web.metrics_allow,
        "proxy_protocol": config.web.proxy_protocol,
        "trusted_proxies": config.deploy.trusted_proxies,
        "frontend": info.frontend,
        "commands": {
            "web": config.processes.web.command,
            "worker": config.processes.worker.command,
            "scheduler": config.processes.scheduler.command,
        },
        "worker_instances": if has(Role::Worker) { config.processes.worker.instances } else { 0 },
        "roles": {
            "web": has(Role::Web),
            "worker": has(Role::Worker),
            "scheduler": has(Role::Scheduler),
            "postgres": has(Role::Postgres),
            "redis": has(Role::Redis),
        },
        "after": after(server),
        "shared": config.deploy.shared,
        "ssh_port": config.ssh.port,
        "server": {
            "name": server.name,
            "host": server.host,
            "private_ip": server.private_ip,
        },
    })
}

/// `context` with `extra`'s keys added (or replaced).
pub fn with(mut context: Value, extra: Value) -> Value {
    if let (Some(base), Value::Object(extra)) = (context.as_object_mut(), extra) {
        base.extend(extra);
    }
    context
}

/// The app's Caddy site with the live slot `slot`.
pub fn site(config: &HarborConfig, server: &Server, info: &AppInfo, slot: Slot) -> Result<String> {
    let port = config.deploy.ports[slot.index()];
    let scheme = if info.h2c { "h2c://" } else { "" };
    let frontend_port = info.frontend.as_ref().map(|f| f.ports[slot.index()]);
    render(
        "files/site.caddy",
        with(
            context(config, server, info),
            json!({
                "slot": slot.as_str(),
                "upstream": format!("{scheme}127.0.0.1:{port}"),
                "frontend_port": frontend_port,
            }),
        ),
    )
}

/// The commands the deploy user may run as root (sudoers).
pub fn sudo_commands(config: &HarborConfig, server: &Server, info: &AppInfo) -> Vec<String> {
    let layout = Layout::new(config);
    let systemctl = |verb: &str, unit: &str| format!("/usr/bin/systemctl {verb} {unit}");
    let mut commands = Vec::new();
    if server.has(Role::Web) {
        for slot in Slot::BOTH {
            let mut units = vec![layout.web_unit(slot)];
            if info.frontend.is_some() {
                units.push(layout.frontend_unit(slot));
            }
            for unit in units {
                for verb in ["start", "stop", "restart", "enable", "disable"] {
                    commands.push(systemctl(verb, &unit));
                }
            }
        }
        commands.push(systemctl("reload", "caddy"));
    }
    if server.has(Role::Worker) {
        for instance in 1..=config.processes.worker.instances {
            for verb in ["start", "stop", "restart"] {
                commands.push(systemctl(verb, &layout.worker_unit(instance)));
            }
        }
    }
    if server.has(Role::Scheduler) {
        for verb in ["start", "stop", "restart"] {
            commands.push(systemctl(verb, &layout.scheduler_unit()));
        }
    }
    if server.has(Role::Postgres) {
        commands.push(systemctl(
            "start",
            &format!("{}.service", layout.backup_unit()),
        ));
    }
    if commands.is_empty() {
        // sudoers needs at least one command; this one changes nothing.
        commands.push("/usr/bin/systemctl --version".to_owned());
    }
    commands
}

/// The sudoers alias of an app: `HARBOR_CRM_API`.
pub fn sudo_alias(app: &str) -> String {
    format!(
        "HARBOR_{}",
        app.to_ascii_uppercase()
            .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
    )
}

/// The ufw rules (arguments of `ufw`) `server` should have.
pub fn firewall_rules(config: &HarborConfig, server: &Server) -> Vec<String> {
    let tag = "comment 'harbor'";
    let mut rules = vec![format!("allow {}/tcp {tag}", config.ssh.port)];
    if server.has(Role::Web) {
        rules.push(format!("allow 80/tcp {tag}"));
        rules.push(format!("allow 443/tcp {tag}"));
        rules.push(format!("allow 443/udp {tag}"));
    }
    let clients = |data: &Server| -> Vec<String> {
        config
            .servers
            .iter()
            .filter(|s| s.name != data.name && s.runs_app())
            .map(|s| {
                if data.private_ip.is_some() {
                    s.private_ip.clone().unwrap_or_else(|| s.host.clone())
                } else {
                    s.host.clone()
                }
            })
            .collect()
    };
    let mut allow_from = |port: u16, from: Vec<String>| {
        for address in from {
            rules.push(format!(
                "allow from {address} to any port {port} proto tcp {tag}"
            ));
        }
    };
    if server.has(Role::Postgres) {
        allow_from(5432, clients(server));
    }
    if server.has(Role::Redis) {
        allow_from(6379, clients(server));
    }
    if server.has(Role::Web) && internal_bind(config, server).len() > 1 {
        let relays: Vec<String> = config
            .servers
            .iter()
            .filter(|s| !s.has(Role::Web) && s.runs_app())
            .filter_map(|s| s.private_ip.clone())
            .collect();
        allow_from(config.deploy.internal_port, relays);
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_template_parses() {
        for (name, _) in TEMPLATES {
            assert!(environment().get_template(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn unknown_variables_are_errors() {
        let error = render("files/backup.timer", json!({})).unwrap_err();
        assert!(error.to_string().contains("files/backup.timer"), "{error}");
    }

    #[test]
    fn heredoc_delimiters_are_refused() {
        assert!(heredoc_safe("a key", "ssh-ed25519 AAAA\n").is_ok());
        assert!(heredoc_safe("a key", "x\nHARBOR_EOF\ny").is_err());
    }

    #[test]
    fn grpc_is_detected_from_the_manifest() {
        let manifest = |text: &str| text.parse::<toml::Table>().unwrap();
        assert!(uses_grpc(&manifest(
            "[dependencies]\nnavree = { version = \"0.1\", features = [\"grpc\"] }\n"
        )));
        assert!(uses_grpc(&manifest(
            "[dependencies]\nnavree-grpc = \"0.1\"\n"
        )));
        assert!(!uses_grpc(&manifest("[dependencies]\nnavree = \"0.1\"\n")));
        assert!(!uses_grpc(&manifest("[package]\nname = \"x\"\n")));
    }

    #[test]
    fn the_sudo_alias_is_upper_case() {
        assert_eq!(sudo_alias("crm-api"), "HARBOR_CRM_API");
    }
}
