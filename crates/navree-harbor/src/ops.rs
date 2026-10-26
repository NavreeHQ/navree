//! Working with servers: `harbor:status`, `harbor:logs`, `harbor:run`,
//! `harbor:ssh` and `harbor:backup`, all over SSH as the deploy user.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use crate::config::{HarborConfig, Role, Server};
use crate::env::quote;
use crate::error::{HarborError, Result};
use crate::exec::Executor;
use crate::host::{HostState, SshHost};
use crate::layout::{Layout, Slot};
use crate::provision::key_values;
use crate::render::{self, AppInfo, context, with};
use crate::ssh::{Identity, Ssh};

/// Opens SSH connections to an app's servers as the deploy user.
#[derive(Debug, Clone)]
pub struct Connector {
    /// Runs `ssh`.
    pub exec: Arc<dyn Executor>,
    /// The private key.
    pub identity: Identity,
}

impl Connector {
    /// A connection to `server` as `ssh.user`. The server's host key must
    /// be pinned (`harbor:provision` records it).
    pub fn ssh(&self, config: &HarborConfig, server: &Server) -> Result<Ssh> {
        if server.host_key.is_none() {
            return Err(HarborError::msg(format!(
                "servers.{0}.host_key is not set: run `navree harbor:provision {0}` to record it",
                server.name
            )));
        }
        Ssh::new(
            self.exec.clone(),
            server,
            &config.ssh.user,
            config.ssh.port,
            self.identity.clone(),
        )
    }

    /// A deploy [`Host`](crate::host::Host) for `server`.
    pub fn host(&self, config: &HarborConfig, server: &Server, info: &AppInfo) -> Result<SshHost> {
        Ok(SshHost::new(
            self.ssh(config, server)?,
            server.clone(),
            config.clone(),
            info.clone(),
        ))
    }
}

/// The servers a command runs on: `only`, or every server with an app
/// role.
pub fn app_servers<'a>(config: &'a HarborConfig, only: Option<&str>) -> Result<Vec<&'a Server>> {
    match only {
        Some(name) => Ok(vec![config.server(name)?]),
        None => Ok(config.servers.iter().filter(|s| s.runs_app()).collect()),
    }
}

/// The server `harbor:run` uses: `only`, else the first `web` server, else
/// the first server that runs the app.
pub fn command_server<'a>(config: &'a HarborConfig, only: Option<&str>) -> Result<&'a Server> {
    if let Some(name) = only {
        return config.server(name);
    }
    config
        .servers_with(Role::Web)
        .next()
        .or_else(|| config.servers.iter().find(|s| s.runs_app()))
        .ok_or_else(|| HarborError::msg("no server runs the app"))
}

/// The units `harbor:status` reports on `server`.
fn status_units(config: &HarborConfig, server: &Server, info: &AppInfo) -> Vec<String> {
    let layout = Layout::new(config);
    let mut units = Vec::new();
    if server.has(Role::Web) {
        for slot in Slot::BOTH {
            units.push(layout.web_unit(slot));
            if info.frontend.is_some() {
                units.push(layout.frontend_unit(slot));
            }
        }
        units.push("caddy".to_owned());
    }
    if server.has(Role::Worker) {
        units.extend((1..=config.processes.worker.instances).map(|i| layout.worker_unit(i)));
    }
    if server.has(Role::Scheduler) {
        units.push(layout.scheduler_unit());
    }
    if server.has(Role::Postgres) {
        units.push("postgresql".to_owned());
    }
    if server.has(Role::Redis) {
        units.push("redis-server".to_owned());
    }
    units
}

/// The `status` script for `server`.
pub fn status_script(config: &HarborConfig, server: &Server, info: &AppInfo) -> Result<String> {
    render::render(
        "deploy/status.sh",
        with(
            context(config, server, info),
            json!({ "units": status_units(config, server, info) }),
        ),
    )
}

/// The `harbor:status` block of one server, from its `status` output.
pub fn format_status(
    config: &HarborConfig,
    server: &Server,
    info: &AppInfo,
    stdout: &str,
) -> Vec<String> {
    let values = key_values(stdout);
    let layout = Layout::new(config);
    let active: std::collections::BTreeMap<String, String> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("unit="))
        .filter_map(|rest| rest.split_once(' '))
        .map(|(unit, state)| (unit.to_owned(), state.trim().to_owned()))
        .collect();
    let is_active = |unit: &str| active.get(unit).is_some_and(|s| s == "active");
    let roles: Vec<&str> = server.roles.iter().map(|r| r.as_str()).collect();
    let mut lines = vec![format!(
        "{}  {}  {}",
        server.name,
        server.host,
        roles.join(", ")
    )];
    let row = |label: &str, text: String| format!("  {label:<10} {text}");
    if server.runs_app() {
        let release = match values.get("current") {
            Some(current) => {
                let older = values
                    .get("releases")
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0)
                    .saturating_sub(1);
                format!("{current} (current), {older} older")
            }
            None => "none yet: run `navree deploy`".to_owned(),
        };
        lines.push(row("release", release));
    }
    if server.has(Role::Web) {
        let live = values.get("live").and_then(|l| Slot::parse(l));
        let text = match live {
            Some(slot) => {
                let port = config.deploy.ports[slot.index()];
                let state = if is_active(&layout.web_unit(slot)) {
                    "running"
                } else {
                    "NOT running"
                };
                format!("{slot}, {state} on 127.0.0.1:{port}")
            }
            None => "no live slot yet".to_owned(),
        };
        lines.push(row("web", text));
        if info.frontend.is_some()
            && let Some(slot) = live
        {
            let state = if is_active(&layout.frontend_unit(slot)) {
                "running"
            } else {
                "NOT running"
            };
            lines.push(row("front end", format!("{slot}, {state}")));
        }
        lines.push(row(
            "caddy",
            if is_active("caddy") {
                "running"
            } else {
                "NOT running"
            }
            .to_owned(),
        ));
    }
    if server.has(Role::Worker) {
        let n = config.processes.worker.instances;
        let running = (1..=n)
            .filter(|i| is_active(&layout.worker_unit(*i)))
            .count();
        lines.push(row("worker", format!("{running} of {n} running")));
    }
    if server.has(Role::Scheduler) {
        lines.push(row(
            "scheduler",
            if is_active(&layout.scheduler_unit()) {
                "running"
            } else {
                "NOT running"
            }
            .to_owned(),
        ));
    }
    for (role, unit, label) in [
        (Role::Postgres, "postgresql", "postgres"),
        (Role::Redis, "redis-server", "redis"),
    ] {
        if server.has(role) {
            lines.push(row(
                label,
                if is_active(unit) {
                    "running"
                } else {
                    "NOT running"
                }
                .to_owned(),
            ));
        }
    }
    let mut system = Vec::new();
    if let Some(disk) = values.get("disk") {
        let mut parts = disk.split_whitespace();
        if let (Some(used), Some(blocks)) = (parts.next(), parts.next()) {
            // `df -P` counts 1024-byte blocks.
            let gb = blocks.parse::<u64>().unwrap_or(0) * 1024 / 1_000_000_000;
            system.push(format!("disk {used} of {gb} GB"));
        }
    }
    if let Some(memory) = values.get("memory") {
        let numbers: Vec<u64> = memory
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        if let [total, available] = numbers[..]
            && total > 0
        {
            let used = (total - available.min(total)) * 100 / total;
            let gb = (total as f64 / 1_000_000.0).round() as u64;
            system.push(format!("memory {used}% of {gb} GB"));
        }
    }
    if let Some(load) = values.get("load") {
        system.push(format!("load {load}"));
    }
    if !system.is_empty() {
        lines.push(row("system", system.join(", ")));
    }
    if values.get("reboot").map(String::as_str) == Some("yes") {
        lines.push(row(
            "reboot",
            "required by installed updates; reboot when convenient".to_owned(),
        ));
    }
    lines
}

/// The `journalctl` command line of `harbor:logs`.
pub fn logs_command(
    config: &HarborConfig,
    process: Option<&str>,
    lines: u32,
    follow: bool,
) -> Result<String> {
    let pattern = match process {
        None => format!("{}-*", config.app),
        Some("web") => format!("{}-web@*", config.app),
        Some("frontend") => format!("{}-frontend@*", config.app),
        Some("worker") => format!("{}-worker@*", config.app),
        Some("scheduler") => format!("{}-scheduler", config.app),
        Some(other) => {
            return Err(HarborError::msg(format!(
                "--process takes web, frontend, worker or scheduler (got `{other}`)"
            )));
        }
    };
    let mut command = format!(
        "journalctl --unit {} --lines {lines} --no-pager --output short-iso",
        quote(&pattern)
    );
    if follow {
        command.push_str(" --follow");
    }
    Ok(command)
}

/// Merges the log lines of several servers by their timestamp
/// (`short-iso` lines start with one), keeping each server's order.
pub fn merge_logs(per_server: &[(String, String)]) -> Vec<String> {
    let mut lines: Vec<(String, usize, usize, String)> = Vec::new();
    for (index, (_server, text)) in per_server.iter().enumerate() {
        for (n, line) in text.lines().enumerate() {
            if line.starts_with("-- ") || line.trim().is_empty() {
                continue;
            }
            let stamp = line.split_whitespace().next().unwrap_or("").to_owned();
            lines.push((stamp, index, n, line.to_owned()));
        }
    }
    lines.sort();
    lines.into_iter().map(|(_, _, _, line)| line).collect()
}

/// The remote command of `harbor:run`: the app command from `current`,
/// with the app's environment.
pub fn run_command(config: &HarborConfig, args: &[String]) -> String {
    let layout = Layout::new(config);
    let quoted: Vec<String> = args.iter().map(|a| quote(a)).collect();
    format!(
        "cd {} && set -a && . {} && set +a && exec ./{} {}",
        layout.current(),
        layout.env_file(),
        layout.app,
        quoted.join(" ")
    )
}

/// Runs the PostgreSQL backup now on the `postgres` server; returns the
/// file it wrote there.
pub fn backup(ssh: &Ssh, config: &HarborConfig, server: &Server, info: &AppInfo) -> Result<String> {
    let layout = Layout::new(config);
    let script = render::render(
        "deploy/backup.sh",
        with(
            context(config, server, info),
            json!({ "unit": format!("{}.service", layout.backup_unit()) }),
        ),
    )?;
    let output = ssh.script("backup", &script)?;
    key_values(&output.stdout)
        .get("file")
        .filter(|f| !f.is_empty())
        .cloned()
        .ok_or_else(|| HarborError::msg("the backup ran but wrote no file"))
}

/// Where `--download` saves a backup: its file name in `dir`.
pub fn download_path(dir: &Path, remote: &str) -> PathBuf {
    dir.join(remote.rsplit('/').next().unwrap_or(remote))
}

/// Whether `state` describes a server with a live release.
pub fn has_release(state: &HostState) -> bool {
    state.current.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> HarborConfig {
        HarborConfig::parse(text, Path::new("harbor.toml")).unwrap()
    }

    const SINGLE: &str = "app = \"crm-api\"\ndomain = \"crm.example.com\"\n[processes]\nworker = { instances = 2 }\n[servers.app-1]\nhost = \"203.0.113.10\"\nroles = [\"web\", \"worker\", \"scheduler\", \"postgres\", \"redis\"]\n";

    #[test]
    fn the_status_block() {
        let config = config(SINGLE);
        let server = &config.servers[0];
        let stdout = "live=green\ncurrent=20261026-091512-3f2a9c1\nreleases=5\nunit=crm-api-web@blue inactive\nunit=crm-api-web@green active\nunit=caddy active\nunit=crm-api-worker@1 active\nunit=crm-api-worker@2 active\nunit=crm-api-scheduler active\nunit=postgresql active\nunit=redis-server active\ndisk=18% 83886080\nmemory=4000000 2360000\nload=0.12\n";
        let lines = format_status(&config, server, &AppInfo::default(), stdout);
        insta::assert_snapshot!(lines.join("\n"), @r"
        app-1  203.0.113.10  web, worker, scheduler, postgres, redis
          release    20261026-091512-3f2a9c1 (current), 4 older
          web        green, running on 127.0.0.1:8082
          caddy      running
          worker     2 of 2 running
          scheduler  running
          postgres   running
          redis      running
          system     disk 18% of 85 GB, memory 41% of 4 GB, load 0.12
        ");
        let fresh = format_status(&config, server, &AppInfo::default(), "reboot=yes\n");
        assert!(fresh.contains(&"  release    none yet: run `navree deploy`".to_owned()));
        assert!(fresh.contains(&"  web        no live slot yet".to_owned()));
        assert!(fresh.iter().any(|l| l.starts_with("  reboot")));
        let script = status_script(&config, server, &AppInfo::default()).unwrap();
        assert!(
            script.contains("systemctl is-active crm-api-worker@2"),
            "{script}"
        );
    }

    #[test]
    fn log_commands() {
        let config = config(SINGLE);
        assert_eq!(
            logs_command(&config, None, 200, false).unwrap(),
            "journalctl --unit 'crm-api-*' --lines 200 --no-pager --output short-iso"
        );
        assert_eq!(
            logs_command(&config, Some("web"), 50, true).unwrap(),
            "journalctl --unit 'crm-api-web@*' --lines 50 --no-pager --output short-iso --follow"
        );
        assert!(logs_command(&config, Some("db"), 50, false).is_err());
    }

    #[test]
    fn logs_merge_by_time() {
        let merged = merge_logs(&[
            (
                "web-1".into(),
                "-- Logs begin --\n2026-10-26T09:15:12+0000 web-1 crm-api[1]: a\n2026-10-26T09:15:14+0000 web-1 crm-api[1]: c\n".into(),
            ),
            (
                "web-2".into(),
                "2026-10-26T09:15:13+0000 web-2 crm-api[2]: b\n".into(),
            ),
        ]);
        assert_eq!(
            merged,
            [
                "2026-10-26T09:15:12+0000 web-1 crm-api[1]: a",
                "2026-10-26T09:15:13+0000 web-2 crm-api[2]: b",
                "2026-10-26T09:15:14+0000 web-1 crm-api[1]: c",
            ]
        );
    }

    #[test]
    fn app_commands_run_from_current_with_the_environment() {
        let config = config(SINGLE);
        assert_eq!(
            run_command(&config, &["queue:retry".into(), "all".into()]),
            "cd /srv/crm-api/current && set -a && . /etc/crm-api/env && set +a && exec ./crm-api 'queue:retry' 'all'"
        );
        assert_eq!(command_server(&config, None).unwrap().name, "app-1");
        assert!(command_server(&config, Some("nope")).is_err());
    }

    #[test]
    fn deploy_connections_need_a_pinned_key() {
        let config = config(SINGLE);
        let connector = Connector {
            exec: Arc::new(crate::exec::FakeExecutor::default()),
            identity: Identity::agent(),
        };
        let error = connector.ssh(&config, &config.servers[0]).unwrap_err();
        assert!(
            error.to_string().contains("harbor:provision app-1"),
            "{error}"
        );
        assert_eq!(
            download_path(
                Path::new("."),
                "/var/backups/postgresql/crm-api/crm_api-1.dump"
            ),
            Path::new(".").join("crm_api-1.dump")
        );
    }
}
