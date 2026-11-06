//! `navree deploy`, `navree deploy:rollback` and `navree deploy:releases`:
//! the release state machine.
//!
//! A deploy builds once, uploads to every server, writes the environment,
//! migrates once, then switches the web servers one at a time: start the
//! idle slot, health-check it, point Caddy and `current` at it, and drain
//! the old slot. When a web server's new release fails its health check,
//! the servers already switched go back to the previous release, so every
//! web server always runs the same release. Workers and the scheduler
//! restart once every web server is on the new release; old releases are
//! pruned last.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::config::{HarborConfig, Role};
use crate::env::{frontend_env, server_env, slot_env};
use crate::error::{HarborError, Result, Unhealthy, tail};
use crate::host::{Host, HostState, ReleaseInfo, SlotProcess};
use crate::layout::{Layout, Slot};
use crate::output::{Clock, Progress, elapsed, millis, size};
use crate::render::{self, AppInfo};

/// A release's id and the commit it was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMeta {
    /// `20261026-091512-3f2a9c1`.
    pub id: String,
    /// The full commit hash, or empty outside Git.
    pub commit: String,
    /// The commit's subject line.
    pub subject: String,
}

impl ReleaseMeta {
    /// The release built at `now` from `commit` (hash and subject).
    pub fn new(now: DateTime<Utc>, commit: Option<(String, String)>) -> ReleaseMeta {
        let stamp = now.format("%Y%m%d-%H%M%S").to_string();
        match commit {
            Some((hash, subject)) => {
                let short: String = hash.chars().take(7).collect();
                ReleaseMeta {
                    id: format!("{stamp}-{short}"),
                    commit: hash,
                    subject: subject.replace(['\t', '\n', '\r'], " "),
                }
            }
            None => ReleaseMeta {
                id: stamp,
                commit: String::new(),
                subject: String::new(),
            },
        }
    }

    /// The `.harbor-release` file stored in the release.
    pub fn file(&self) -> String {
        format!(
            "id={}\ncommit={}\nsubject={}\n",
            self.id, self.commit, self.subject
        )
    }
}

/// What the servers run: their architecture and the oldest glibc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// The Rust target triple, e.g. `x86_64-unknown-linux-gnu`.
    pub triple: String,
    /// The container platform, e.g. `linux/amd64`.
    pub platform: String,
    /// The oldest glibc of the servers, e.g. `2.35`.
    pub glibc: Option<String>,
}

impl Target {
    /// The target for servers of `arch`; `triple` overrides the Rust target.
    pub fn new(arch: &str, glibc: Option<String>, triple: Option<&str>) -> Target {
        let platform = match arch {
            "aarch64" | "arm64" => "linux/arm64",
            _ => "linux/amd64",
        };
        Target {
            arch: arch.to_owned(),
            triple: triple
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{arch}-unknown-linux-gnu")),
            platform: platform.to_owned(),
            glibc,
        }
    }
}

/// A built release archive.
#[derive(Debug)]
pub struct Artifact {
    /// The `.tar.gz` to upload.
    pub archive: PathBuf,
    /// Its size in bytes.
    pub size: u64,
    /// `crm-api for x86_64-unknown-linux-gnu, in a container`.
    pub summary: String,
    /// Keeps a temporary directory alive while the archive is uploaded.
    pub keep: Option<tempfile::TempDir>,
}

/// Builds the release archive.
pub trait Build {
    /// Builds `release` for `target`.
    fn build(&self, release: &ReleaseMeta, target: &Target) -> Result<Artifact>;
}

/// What `navree deploy` was asked to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeployOptions {
    /// Skip the migrations (`--no-migrate`).
    pub no_migrate: bool,
    /// Turn on maintenance mode around the migrations (`--maintenance`).
    pub maintenance: bool,
    /// Deploy a release already uploaded (`--release <id>`).
    pub release: Option<String>,
}

/// Runs deploys and rollbacks on a set of servers.
pub struct Deployer<'a> {
    /// The configuration.
    pub config: &'a HarborConfig,
    /// The environment's name, for output.
    pub env_name: &'a str,
    /// What Harbor knows about the app.
    pub info: &'a AppInfo,
    /// The servers that run the app, in `harbor.toml` order.
    pub hosts: Vec<Arc<dyn Host>>,
    /// Where progress lines go.
    pub progress: &'a dyn Progress,
    /// The clock.
    pub clock: &'a dyn Clock,
}

/// How one web server was switched, to undo it.
struct Switched {
    host: Arc<dyn Host>,
    state: HostState,
    slot: Slot,
}

impl Deployer<'_> {
    fn layout(&self) -> Layout {
        Layout::new(self.config)
    }

    fn servers_label(&self) -> String {
        let n = self.hosts.len();
        format!("{n} server{}", if n == 1 { "" } else { "s" })
    }

    /// Reads every server's state, in parallel.
    fn inspect(&self) -> Result<Vec<HostState>> {
        let states = std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .hosts
                .iter()
                .map(|host| scope.spawn(move || host.inspect()))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("inspect thread panicked"))
                .collect::<Vec<_>>()
        });
        let states: Vec<HostState> = states.into_iter().collect::<Result<_>>()?;
        for (host, state) in self.hosts.iter().zip(&states) {
            if !state.provisioned {
                let name = &host.server().name;
                return Err(HarborError::msg(format!(
                    "{name} is not provisioned yet: run `navree harbor:provision {name}` first"
                )));
            }
        }
        Ok(states)
    }

    /// The servers' common target.
    fn target(&self, states: &[HostState]) -> Result<Target> {
        let arches: Vec<&str> = states.iter().map(|s| s.arch.as_str()).collect();
        let arch = arches.first().copied().unwrap_or("x86_64");
        if let Some(other) = arches.iter().find(|a| **a != arch) {
            return Err(HarborError::msg(format!(
                "the servers mix architectures ({arch} and {other}); one release binary can't run on both"
            )));
        }
        let glibc = states
            .iter()
            .filter_map(|s| s.glibc.clone())
            .min_by(|a, b| version_cmp(a, b));
        Ok(Target::new(
            arch,
            glibc,
            self.config.build.target.as_deref(),
        ))
    }

    /// `navree deploy`: returns the id of the release now live.
    pub fn deploy(
        &self,
        build: &dyn Build,
        release: &ReleaseMeta,
        secrets: &BTreeMap<String, String>,
        options: &DeployOptions,
    ) -> Result<String> {
        let started = self.clock.now();
        self.progress.line(&format!(
            "Deploying {} to {} ({})",
            self.config.app,
            self.env_name,
            self.servers_label()
        ));
        let states = self.inspect()?;

        // Build and upload, or reuse an uploaded release.
        let id = match &options.release {
            Some(id) => {
                for (host, state) in self.hosts.iter().zip(&states) {
                    if !state.has_release(id) {
                        return Err(HarborError::msg(format!(
                            "release {id} is not on {}; `navree deploy:releases` lists them",
                            host.server().name
                        )));
                    }
                }
                id.clone()
            }
            None => {
                let target = self.target(&states)?;
                let build_started = self.clock.now();
                let artifact = build.build(release, &target)?;
                self.progress.step(
                    "build",
                    &format!(
                        "{} ({})",
                        artifact.summary,
                        elapsed(self.clock, build_started)
                    ),
                );
                self.upload(&release.id, &artifact)?;
                release.id.clone()
            }
        };

        // Environment.
        for host in &self.hosts {
            let (env, settings, secret_count) =
                server_env(self.config, host.server(), &id, secrets);
            host.write_env(&env)?;
            self.progress.server_step(
                "env",
                &host.server().name,
                &format!(
                    "{settings} settings, {secret_count} secret{}",
                    if secret_count == 1 { "" } else { "s" }
                ),
            );
        }

        // The configuration the release boots with, checked where it runs and
        // before anything changes: a section that does not read, a key
        // nothing reads, or, in production, an unsafe setting stops the
        // deploy here.
        let web: Vec<usize> = (0..self.hosts.len())
            .filter(|&i| self.hosts[i].server().has(Role::Web))
            .collect();
        {
            let host = &self.hosts[web.first().copied().unwrap_or(0)];
            self.app_command(host, "config:check", Some(&id))?;
            self.progress
                .server_step("config", &host.server().name, "configuration checked");
        }

        // Migrations, once.
        let migrate = self.config.deploy.migrate && !options.no_migrate;
        let maintenance = options.maintenance && migrate;
        if maintenance {
            for &i in &web {
                if states[i].current.is_some() {
                    self.app_command(&self.hosts[i], "down", None)?;
                    self.progress.server_step(
                        "down",
                        &self.hosts[i].server().name,
                        "maintenance mode on",
                    );
                }
            }
        }
        if migrate {
            let index = web.first().copied().unwrap_or(0);
            let host = &self.hosts[index];
            let migrate_started = self.clock.now();
            let output = match self.app_command(host, "migrate", Some(&id)) {
                Ok(output) => output,
                Err(error) => {
                    if maintenance {
                        self.maintenance_off(&web, None);
                    }
                    return Err(error);
                }
            };
            self.progress.server_step(
                "migrate",
                &host.server().name,
                &format!(
                    "{} ({})",
                    migrate_summary(&output),
                    elapsed(self.clock, migrate_started)
                ),
            );
        }

        // Web servers, one at a time.
        let mut switched: Vec<Switched> = Vec::new();
        for &i in &web {
            match self.activate(&self.hosts[i], &states[i], &id) {
                Ok(slot) => switched.push(Switched {
                    host: self.hosts[i].clone(),
                    state: states[i].clone(),
                    slot,
                }),
                Err(error) => {
                    self.revert(&switched);
                    if maintenance {
                        self.maintenance_off(&web, None);
                    }
                    return Err(error);
                }
            }
        }
        if maintenance {
            self.maintenance_off(&web, Some(&id));
        }

        // The other servers, then the workers and the scheduler.
        for host in self.hosts.iter().filter(|h| !h.server().has(Role::Web)) {
            host.set_current(&id)?;
            self.progress
                .server_step("switch", &host.server().name, &format!("current -> {id}"));
        }
        self.restart_processes()?;
        self.prune()?;
        self.progress.line(&format!(
            "Released {id} in {}.",
            elapsed(self.clock, started)
        ));
        Ok(id)
    }

    /// Uploads the archive to every server in parallel.
    fn upload(&self, id: &str, artifact: &Artifact) -> Result<()> {
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .hosts
                .iter()
                .map(|host| {
                    scope.spawn(move || {
                        let started = self.clock.now();
                        host.upload(id, &artifact.archive)
                            .map(|()| elapsed(self.clock, started))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("upload thread panicked"))
                .collect::<Vec<_>>()
        });
        for (host, result) in self.hosts.iter().zip(results) {
            let took = result?;
            self.progress.server_step(
                "upload",
                &host.server().name,
                &format!("releases/{id}, {} ({took})", size(artifact.size)),
            );
        }
        Ok(())
    }

    /// Runs one app command and returns its output.
    fn app_command(
        &self,
        host: &Arc<dyn Host>,
        command: &str,
        release: Option<&str>,
    ) -> Result<String> {
        let output = host.run_app(command, release, &[command.to_owned()])?;
        Ok(output.combined())
    }

    /// Turns maintenance mode off on every web server (best effort when
    /// `release` is `None`, after a failure).
    fn maintenance_off(&self, web: &[usize], release: Option<&str>) {
        for &i in web {
            let host = &self.hosts[i];
            match self.app_command(host, "up", release) {
                Ok(_) => {
                    self.progress
                        .server_step("up", &host.server().name, "maintenance mode off")
                }
                Err(error) => self.progress.server_step(
                    "up",
                    &host.server().name,
                    &format!("could not turn maintenance mode off: {error}"),
                ),
            }
        }
    }

    /// Starts `release` in the idle slot, checks it, switches to it and
    /// drains the old slot. Returns the slot now live.
    fn activate(&self, host: &Arc<dyn Host>, state: &HostState, release: &str) -> Result<Slot> {
        let layout = self.layout();
        let name = host.server().name.clone();
        let idle = state.live.map(Slot::other).unwrap_or(Slot::Blue);
        let port = self.config.deploy.ports[idle.index()];
        let mut started_units = vec![layout.web_unit(idle)];

        host.start(SlotProcess::Web, idle, release, &slot_env(port, release))?;
        self.progress
            .server_step("start", &name, &format!("{idle} on 127.0.0.1:{port}"));
        let path = &self.config.deploy.health_check;
        self.check(host, port, path, &layout.web_unit(idle), &started_units)?;

        if let Some(frontend) = &self.info.frontend {
            let fport = frontend.ports[idle.index()];
            started_units.push(layout.frontend_unit(idle));
            if let Err(error) = host.start(
                SlotProcess::Frontend,
                idle,
                release,
                &frontend_env(fport, port),
            ) {
                let _ = host.systemctl("stop", "stop", &started_units);
                return Err(error);
            }
            self.progress.server_step(
                "start",
                &name,
                &format!("front end {idle} on 127.0.0.1:{fport}"),
            );
            self.check(
                host,
                fport,
                &frontend.health_check,
                &layout.frontend_unit(idle),
                &started_units,
            )?;
        }

        let site = render::site(self.config, host.server(), self.info, idle)?;
        let units_of = |slot: Slot| {
            let mut units = vec![layout.web_unit(slot)];
            if self.info.frontend.is_some() {
                units.push(layout.frontend_unit(slot));
            }
            units
        };
        let disable = state.live.map(units_of).unwrap_or_default();
        host.switch(idle, release, &site, &units_of(idle), &disable)?;
        self.progress.server_step(
            "switch",
            &name,
            &format!("Caddy -> {idle}, current -> {release}"),
        );
        if let Some(old) = state.live {
            host.systemctl("drain", "stop", &units_of(old))?;
            self.progress.server_step(
                "drain",
                &name,
                &format!("{old} stopped after its requests finished"),
            );
        }
        Ok(idle)
    }

    /// Health-checks `port`; on failure prints nothing, stops `units` and
    /// returns [`HarborError::Unhealthy`] with `unit`'s last log lines.
    fn check(
        &self,
        host: &Arc<dyn Host>,
        port: u16,
        path: &str,
        unit: &str,
        units: &[String],
    ) -> Result<()> {
        let name = &host.server().name;
        let timeout = self.config.deploy.health_timeout_secs;
        let health = host.health(port, path, timeout)?;
        if health.ok() {
            self.progress.server_step(
                "health",
                name,
                &format!("GET {path} 200 after {}", millis(health.millis)),
            );
            return Ok(());
        }
        let logs = host.logs(unit, 20).unwrap_or_default();
        let _ = host.systemctl("stop", "stop", units);
        self.progress.server_step(
            "health",
            name,
            &format!("GET {path} {} after {timeout}s: stopped", health.status),
        );
        Err(HarborError::Unhealthy(Box::new(Unhealthy {
            server: name.clone(),
            path: path.to_owned(),
            address: format!("127.0.0.1:{port}"),
            timeout_secs: timeout,
            status: health.status,
            logs: tail(&logs, 20),
        })))
    }

    /// Switches web servers already moved to the new release back to the
    /// release they ran before.
    fn revert(&self, switched: &[Switched]) {
        for entry in switched.iter().rev() {
            let name = &entry.host.server().name;
            let Some(previous) = entry.state.current.clone() else {
                self.progress.server_step(
                    "revert",
                    name,
                    "no previous release to go back to; it stays on the new one",
                );
                continue;
            };
            let now = HostState {
                live: Some(entry.slot),
                current: None,
                ..entry.state.clone()
            };
            match self.activate(&entry.host, &now, &previous) {
                Ok(_) => self
                    .progress
                    .server_step("revert", name, &format!("back on {previous}")),
                Err(error) => self.progress.server_step(
                    "revert",
                    name,
                    &format!("could not go back to {previous}: {error}"),
                ),
            }
        }
    }

    /// Restarts the workers and the scheduler on the new release.
    fn restart_processes(&self) -> Result<()> {
        let layout = self.layout();
        for host in &self.hosts {
            let server = host.server();
            let mut units = Vec::new();
            let mut shown = Vec::new();
            if server.has(Role::Worker) {
                let n = self.config.processes.worker.instances;
                units.extend((1..=n).map(|i| layout.worker_unit(i)));
                shown.push(format!("worker x{n}"));
            }
            if server.has(Role::Scheduler) {
                units.push(layout.scheduler_unit());
                shown.push("scheduler".to_owned());
            }
            if units.is_empty() {
                continue;
            }
            host.systemctl("restart", "restart", &units)?;
            self.progress
                .server_step("restart", &server.name, &shown.join(", "));
        }
        Ok(())
    }

    /// Removes old releases on every server.
    fn prune(&self) -> Result<()> {
        for host in &self.hosts {
            let removed = host.prune(self.config.deploy.keep_releases)?;
            let n = removed.len();
            let detail = match n {
                0 => "nothing to remove".to_owned(),
                1 => "1 old release removed".to_owned(),
                n => format!("{n} old releases removed"),
            };
            self.progress
                .server_step("prune", &host.server().name, &detail);
        }
        Ok(())
    }

    /// `navree deploy:rollback [--to <id>]`: switches back to the release
    /// before the current one (or `to`). Returns the release now live.
    pub fn rollback(&self, to: Option<&str>) -> Result<String> {
        let started = self.clock.now();
        let states = self.inspect()?;
        let first = states
            .first()
            .ok_or_else(|| HarborError::msg("no servers to roll back"))?;
        let target = match to {
            Some(id) => id.to_owned(),
            None => first
                .previous_release()
                .map(|r| r.id.clone())
                .ok_or_else(|| {
                    HarborError::msg(format!(
                        "{} has no release before the current one to go back to",
                        self.hosts[0].server().name
                    ))
                })?,
        };
        for (host, state) in self.hosts.iter().zip(&states) {
            if !state.has_release(&target) {
                return Err(HarborError::msg(format!(
                    "release {target} is not on {}; `navree deploy:releases` lists them",
                    host.server().name
                )));
            }
        }
        let names: Vec<&str> = self
            .hosts
            .iter()
            .map(|h| h.server().name.as_str())
            .collect();
        self.progress.line(&format!(
            "Rolling back {} on {} to {target}",
            self.config.app,
            names.join(", ")
        ));
        let mut switched: Vec<Switched> = Vec::new();
        for (i, host) in self.hosts.iter().enumerate() {
            if !host.server().has(Role::Web) {
                continue;
            }
            match self.activate(host, &states[i], &target) {
                Ok(slot) => switched.push(Switched {
                    host: host.clone(),
                    state: states[i].clone(),
                    slot,
                }),
                Err(error) => {
                    self.revert(&switched);
                    return Err(error);
                }
            }
        }
        for host in self.hosts.iter().filter(|h| !h.server().has(Role::Web)) {
            host.set_current(&target)?;
            self.progress.server_step(
                "switch",
                &host.server().name,
                &format!("current -> {target}"),
            );
        }
        self.restart_processes()?;
        self.progress
            .line(&format!("Rolled back in {}.", elapsed(self.clock, started)));
        Ok(target)
    }

    /// `navree deploy:releases`: the releases on the first server, newest
    /// first, with the current one marked.
    pub fn releases(&self) -> Result<Vec<(ReleaseInfo, bool)>> {
        let host = self
            .hosts
            .first()
            .ok_or_else(|| HarborError::msg("no servers"))?;
        let state = host.inspect()?;
        let current = state.current.clone();
        let mut releases: Vec<(ReleaseInfo, bool)> = state
            .releases
            .into_iter()
            .map(|r| {
                let live = current.as_deref() == Some(r.id.as_str());
                (r, live)
            })
            .collect();
        releases.reverse();
        Ok(releases)
    }
}

/// `2 migrations ran` from the output of the app's `migrate` command (one
/// `Migrated: <module>/<name>` line each, or `Nothing to migrate.`).
pub fn migrate_summary(output: &str) -> String {
    let ran = output
        .lines()
        .filter(|l| l.trim_start().starts_with("Migrated:"))
        .count();
    match ran {
        0 if output.contains("Nothing to migrate") => "nothing to migrate".to_owned(),
        0 => {
            let last = tail(output, 1);
            if last.trim().is_empty() {
                "done".to_owned()
            } else {
                last.trim().to_owned()
            }
        }
        1 => "1 migration ran".to_owned(),
        n => format!("{n} migrations ran"),
    }
}

/// Compares versions like `2.35` and `2.39` numerically.
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u32> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parts(a).cmp(&parts(b))
}

/// The lines of `navree deploy:releases`.
pub fn format_releases(releases: &[(ReleaseInfo, bool)]) -> Vec<String> {
    releases
        .iter()
        .map(|(r, current)| {
            format!(
                "{}  {:<8} {}",
                r.id,
                if *current { "current" } else { "" },
                r.subject
            )
            .trim_end()
            .to_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_ids_are_the_time_and_the_short_commit() {
        let now = crate::output::FixedClock::example().0;
        let meta = ReleaseMeta::new(
            now,
            Some((
                "3f2a9c1d0e5b".to_owned(),
                "feat: stage\tprobabilities".to_owned(),
            )),
        );
        assert_eq!(meta.id, "20261026-091512-3f2a9c1");
        assert_eq!(meta.subject, "feat: stage probabilities");
        assert_eq!(
            meta.file(),
            "id=20261026-091512-3f2a9c1\ncommit=3f2a9c1d0e5b\nsubject=feat: stage probabilities\n"
        );
        assert_eq!(ReleaseMeta::new(now, None).id, "20261026-091512");
    }

    #[test]
    fn targets() {
        let x86 = Target::new("x86_64", Some("2.35".into()), None);
        assert_eq!(x86.triple, "x86_64-unknown-linux-gnu");
        assert_eq!(x86.platform, "linux/amd64");
        let arm = Target::new("aarch64", None, None);
        assert_eq!(arm.triple, "aarch64-unknown-linux-gnu");
        assert_eq!(arm.platform, "linux/arm64");
        let musl = Target::new("x86_64", None, Some("x86_64-unknown-linux-musl"));
        assert_eq!(musl.triple, "x86_64-unknown-linux-musl");
    }

    #[test]
    fn migrations_are_counted() {
        assert_eq!(
            migrate_summary("Migrated: deals/m1\nMigrated: deals/m2\n"),
            "2 migrations ran"
        );
        assert_eq!(
            migrate_summary("{\"level\":\"INFO\"}\nMigrated: deals/m1\n"),
            "1 migration ran"
        );
        assert_eq!(
            migrate_summary("Nothing to migrate.\n"),
            "nothing to migrate"
        );
        assert_eq!(migrate_summary("all good\n"), "all good");
        assert_eq!(migrate_summary(""), "done");
    }

    #[test]
    fn versions_compare_numerically() {
        use std::cmp::Ordering;
        assert_eq!(version_cmp("2.35", "2.39"), Ordering::Less);
        assert_eq!(version_cmp("2.9", "2.10"), Ordering::Less);
        assert_eq!(version_cmp("2.39", "2.39"), Ordering::Equal);
    }

    #[test]
    fn the_releases_listing() {
        let info = |id: &str, subject: &str| ReleaseInfo {
            id: id.into(),
            commit: String::new(),
            subject: subject.into(),
        };
        let lines = format_releases(&[
            (
                info("20261026-091512-3f2a9c1", "feat: stage probabilities"),
                true,
            ),
            (
                info("20261025-163003-9b17e44", "fix: deal board ordering"),
                false,
            ),
            (info("20261024-110741", ""), false),
        ]);
        assert_eq!(
            lines,
            [
                "20261026-091512-3f2a9c1  current  feat: stage probabilities",
                "20261025-163003-9b17e44           fix: deal board ordering",
                "20261024-110741",
            ]
        );
    }
}
