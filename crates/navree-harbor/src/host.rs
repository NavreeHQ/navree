//! What `navree deploy` does on a server, behind the [`Host`] trait:
//! [`SshHost`] runs each operation as a bash script over SSH as the deploy
//! user, and [`crate::fake::FakeHost`] keeps the same state in memory for
//! tests of the release state machine.

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;

use crate::config::{HarborConfig, Server};
use crate::env::{EnvFile, quote};
use crate::error::Result;
use crate::exec::Output;
use crate::layout::{Layout, Slot};
use crate::provision::key_values;
use crate::render::{self, AppInfo, context, heredoc_safe, with};
use crate::ssh::Ssh;

/// A release on a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// `20261026-091512-3f2a9c1`: the UTC time of the deploy and the commit.
    pub id: String,
    /// The full commit hash, when the release was built in a Git checkout.
    pub commit: String,
    /// The commit's subject line.
    pub subject: String,
}

/// What a server looks like before a deploy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostState {
    /// `harbor:provision` has prepared it (the releases directory exists).
    pub provisioned: bool,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// The glibc version, e.g. `2.39`.
    pub glibc: Option<String>,
    /// The slot taking traffic.
    pub live: Option<Slot>,
    /// The release `current` points at.
    pub current: Option<String>,
    /// The release the blue slot points at.
    pub blue: Option<String>,
    /// The release the green slot points at.
    pub green: Option<String>,
    /// Every release on the server, oldest first.
    pub releases: Vec<ReleaseInfo>,
}

impl HostState {
    /// Parses the `inspect` script's output.
    pub fn parse(stdout: &str) -> HostState {
        let values = key_values(stdout);
        let get = |k: &str| values.get(k).filter(|v| !v.is_empty()).cloned();
        let mut releases: Vec<ReleaseInfo> = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("release="))
            .map(|rest| {
                let mut parts = rest.splitn(3, '\t');
                ReleaseInfo {
                    id: parts.next().unwrap_or_default().to_owned(),
                    commit: parts.next().unwrap_or_default().to_owned(),
                    subject: parts.next().unwrap_or_default().to_owned(),
                }
            })
            .filter(|r| !r.id.is_empty())
            .collect();
        releases.sort_by(|a, b| a.id.cmp(&b.id));
        HostState {
            provisioned: get("provisioned").as_deref() == Some("yes"),
            arch: get("arch").unwrap_or_default(),
            glibc: get("glibc"),
            live: get("live").as_deref().and_then(Slot::parse),
            current: get("current"),
            blue: get("blue"),
            green: get("green"),
            releases,
        }
    }

    /// Whether release `id` is on the server.
    pub fn has_release(&self, id: &str) -> bool {
        self.releases.iter().any(|r| r.id == id)
    }

    /// The release before `current` (by id, so by time), for a rollback.
    pub fn previous_release(&self) -> Option<&ReleaseInfo> {
        let current = self.current.as_deref()?;
        self.releases.iter().rev().find(|r| r.id.as_str() < current)
    }
}

/// The result of a health check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    /// The last HTTP status (`200`, `503`), or `000` when nothing answered.
    pub status: String,
    /// How long it took until that status.
    pub millis: u64,
}

impl Health {
    /// Whether the check passed (200).
    pub fn ok(&self) -> bool {
        self.status == "200"
    }
}

/// A process that runs in the blue and green slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotProcess {
    /// The app's web process.
    Web,
    /// The front end.
    Frontend,
}

/// What a deploy does on one server.
pub trait Host: Send + Sync {
    /// The server.
    fn server(&self) -> &Server;

    /// Reads the server's releases, links and live slot.
    fn inspect(&self) -> Result<HostState>;

    /// Uploads the release archive and unpacks it into `releases/<id>`,
    /// linking the shared directories.
    fn upload(&self, release: &str, archive: &Path) -> Result<()>;

    /// Writes `/etc/<app>/env`.
    fn write_env(&self, env: &EnvFile) -> Result<()>;

    /// Runs an app command (`migrate`, `down`, `up`) from `release`, or from
    /// `current` when `None`, with the app's environment.
    fn run_app(&self, step: &str, release: Option<&str>, args: &[String]) -> Result<Output>;

    /// Points `slot` at `release` (for the web process), writes the slot's
    /// environment file and (re)starts the process in it.
    fn start(&self, process: SlotProcess, slot: Slot, release: &str, env: &EnvFile) -> Result<()>;

    /// Requests `path` on `127.0.0.1:<port>` until it answers 200 or
    /// `timeout_secs` pass.
    fn health(&self, port: u16, path: &str, timeout_secs: u64) -> Result<Health>;

    /// The last `lines` log lines of `unit`.
    fn logs(&self, unit: &str, lines: u32) -> Result<String>;

    /// Runs `systemctl <verb>` on each unit (`stop`, `restart`).
    fn systemctl(&self, step: &str, verb: &str, units: &[String]) -> Result<()>;

    /// Points Caddy at `slot` (writing `site`), moves `current` to
    /// `release`, records the live slot, and enables and disables units so
    /// the live slot starts on boot.
    fn switch(
        &self,
        slot: Slot,
        release: &str,
        site: &str,
        enable: &[String],
        disable: &[String],
    ) -> Result<()>;

    /// Moves `current` to `release` (servers without `web`).
    fn set_current(&self, release: &str) -> Result<()>;

    /// Removes releases beyond the newest `keep`, never one a link points
    /// at; returns the removed ids.
    fn prune(&self, keep: u32) -> Result<Vec<String>>;
}

/// A [`Host`] reached over SSH as the deploy user.
#[derive(Debug)]
pub struct SshHost {
    ssh: Ssh,
    server: Server,
    config: HarborConfig,
    info: AppInfo,
}

impl SshHost {
    /// The host for `server`, connected through `ssh`.
    pub fn new(ssh: Ssh, server: Server, config: HarborConfig, info: AppInfo) -> Self {
        Self {
            ssh,
            server,
            config,
            info,
        }
    }

    fn layout(&self) -> Layout {
        Layout::new(&self.config)
    }

    fn script(&self, template: &str, extra: serde_json::Value) -> Result<String> {
        render::render(
            &format!("deploy/{template}.sh"),
            with(context(&self.config, &self.server, &self.info), extra),
        )
    }

    fn run(&self, step: &str, template: &str, extra: serde_json::Value) -> Result<Output> {
        self.ssh.script(step, &self.script(template, extra)?)
    }

    /// The script of an operation, without running it (for snapshots).
    pub fn render(&self, template: &str, extra: serde_json::Value) -> Result<String> {
        self.script(template, extra)
    }
}

/// The env file as base64 lines, so any value survives the heredoc.
fn base64_lines(text: &str) -> String {
    let encoded = STANDARD.encode(text);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

impl Host for SshHost {
    fn server(&self) -> &Server {
        &self.server
    }

    fn inspect(&self) -> Result<HostState> {
        let output = self.run("inspect", "inspect", json!({}))?;
        Ok(HostState::parse(&output.stdout))
    }

    fn upload(&self, release: &str, archive: &Path) -> Result<()> {
        let remote = format!("{}/{release}.tar.gz", self.layout().releases());
        self.ssh.upload(archive, &remote)?;
        self.run("unpack", "unpack", json!({ "release": release }))?;
        Ok(())
    }

    fn write_env(&self, env: &EnvFile) -> Result<()> {
        let layout = self.layout();
        let text = env.render(&format!(
            "{}: written by navree deploy on every release. Change [env] and [secrets] in harbor.toml instead.",
            layout.app
        ));
        self.run(
            "env",
            "env",
            json!({ "env_file": layout.env_file(), "env_base64": base64_lines(&text) }),
        )?;
        Ok(())
    }

    fn run_app(&self, step: &str, release: Option<&str>, args: &[String]) -> Result<Output> {
        let layout = self.layout();
        let dir = match release {
            Some(id) => layout.release_dir(id),
            None => layout.current(),
        };
        let args: Vec<String> = args.iter().map(|a| quote(a)).collect();
        self.run(
            step,
            "run",
            json!({
                "step": step,
                "dir": dir,
                "env_file": layout.env_file(),
                "args": args.join(" "),
            }),
        )
    }

    fn start(&self, process: SlotProcess, slot: Slot, release: &str, env: &EnvFile) -> Result<()> {
        let layout = self.layout();
        let (kind, unit, env_path) = match process {
            SlotProcess::Web => ("web", layout.web_unit(slot), layout.slot_env(slot)),
            SlotProcess::Frontend => (
                "frontend",
                layout.frontend_unit(slot),
                layout.frontend_env(slot),
            ),
        };
        let text = env.render(&format!("{unit}: written by navree deploy."));
        heredoc_safe("the slot's environment", &text)?;
        self.run(
            "start",
            "start",
            json!({
                "kind": kind,
                "slot": slot.as_str(),
                "release": release,
                "unit": unit,
                "env_path": env_path,
                "env": text,
            }),
        )?;
        Ok(())
    }

    fn health(&self, port: u16, path: &str, timeout_secs: u64) -> Result<Health> {
        let output = self.run(
            "health check",
            "health",
            json!({ "port": port, "path": path, "timeout": timeout_secs }),
        )?;
        let values = key_values(&output.stdout);
        Ok(Health {
            status: values
                .get("status")
                .cloned()
                .unwrap_or_else(|| "none".into()),
            millis: values
                .get("millis")
                .and_then(|m| m.parse().ok())
                .unwrap_or(0),
        })
    }

    fn logs(&self, unit: &str, lines: u32) -> Result<String> {
        let output = self.run("logs", "logs", json!({ "unit": unit, "lines": lines }))?;
        Ok(output.stdout)
    }

    fn systemctl(&self, step: &str, verb: &str, units: &[String]) -> Result<()> {
        if units.is_empty() {
            return Ok(());
        }
        self.run(
            step,
            "units",
            json!({ "step": step, "verb": verb, "units": units }),
        )?;
        Ok(())
    }

    fn switch(
        &self,
        slot: Slot,
        release: &str,
        site: &str,
        enable: &[String],
        disable: &[String],
    ) -> Result<()> {
        heredoc_safe("the Caddy site", site)?;
        self.run(
            "switch",
            "switch",
            json!({
                "slot": slot.as_str(),
                "release": release,
                "site": site,
                "enable": enable,
                "disable": disable,
                "env_file": self.layout().env_file(),
            }),
        )?;
        Ok(())
    }

    fn set_current(&self, release: &str) -> Result<()> {
        self.run(
            "switch",
            "current",
            json!({ "release": release, "env_file": self.layout().env_file() }),
        )?;
        Ok(())
    }

    fn prune(&self, keep: u32) -> Result<Vec<String>> {
        let output = self.run("prune", "prune", json!({ "keep": keep }))?;
        Ok(output
            .stdout
            .lines()
            .filter_map(|l| l.strip_prefix("removed="))
            .map(str::to_owned)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inspect_output_is_parsed() {
        let state = HostState::parse(
            "arch=x86_64\nglibc=2.39\nprovisioned=yes\nlive=green\ncurrent=20261026-091512-3f2a9c1\nblue=20261025-163003-9b17e44\ngreen=20261026-091512-3f2a9c1\nrelease=20261026-091512-3f2a9c1\t3f2a9c1abc\tfeat: stage probabilities\nrelease=20261024-110741-52ad0f3\t52ad0f3abc\tchore: dependencies\nrelease=20261025-163003-9b17e44\t\t\n",
        );
        assert!(state.provisioned);
        assert_eq!(state.arch, "x86_64");
        assert_eq!(state.glibc.as_deref(), Some("2.39"));
        assert_eq!(state.live, Some(Slot::Green));
        assert_eq!(state.current.as_deref(), Some("20261026-091512-3f2a9c1"));
        assert_eq!(state.blue.as_deref(), Some("20261025-163003-9b17e44"));
        let ids: Vec<&str> = state.releases.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "20261024-110741-52ad0f3",
                "20261025-163003-9b17e44",
                "20261026-091512-3f2a9c1"
            ]
        );
        assert_eq!(state.releases[2].subject, "feat: stage probabilities");
        assert_eq!(
            state.previous_release().map(|r| r.id.as_str()),
            Some("20261025-163003-9b17e44")
        );
        assert!(state.has_release("20261024-110741-52ad0f3"));
    }

    #[test]
    fn an_unprovisioned_server() {
        let state = HostState::parse("arch=aarch64\nglibc=\nprovisioned=no\n");
        assert!(!state.provisioned);
        assert_eq!(state.glibc, None);
        assert_eq!(state.live, None);
        assert_eq!(state.previous_release(), None);
    }

    #[test]
    fn base64_lines_wrap() {
        let text = "x".repeat(100);
        let lines = base64_lines(&text);
        assert!(lines.lines().all(|l| l.len() <= 76));
        assert_eq!(
            STANDARD.decode(lines.replace('\n', "")).unwrap(),
            text.as_bytes()
        );
    }
}
