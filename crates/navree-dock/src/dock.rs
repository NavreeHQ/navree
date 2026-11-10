//! [`Dock`]: runs an app's `dock.toml` with Docker or Podman.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::compose::{self, COMPOSE_FILE, PROJECT_LABEL};
use crate::config::{DOCK_FILE, DockFile};
use crate::dotenv::write_dotenv;
use crate::error::{DockError, PortConflict};
use crate::exec::{Cmd, CmdOutput, Executor};
use crate::runtime::Runtime;
use crate::service::{HOST, Readiness, ServiceSpec, Shell};

/// How long `navree dock up` waits for a service's readiness check.
pub const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// How often readiness is checked.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long the runtime has to answer the first command; a runtime whose
/// daemon or machine is not running hangs instead of failing.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// The limit for other commands Dock waits for (pulls run attached and
/// have none).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(180);

/// The limit for one readiness probe run in a container.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// A container of this project, as the runtime lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Container {
    state: String,
    image: String,
    ports: String,
}

/// One row of `navree dock status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceStatus {
    /// The service.
    pub service: String,
    /// `running`, `stopped`, or another state the runtime reports
    /// (`created`, `restarting`, `paused`, ...).
    pub state: String,
    /// The image its container runs, when it has one.
    pub image: Option<String>,
    /// Its addresses on 127.0.0.1, when it is running.
    pub addresses: Vec<String>,
}

/// An app's local services: its `dock.toml`, the runtime that runs them,
/// and the [`Executor`] that talks to it.
#[derive(Debug)]
pub struct Dock<E: Executor> {
    root: PathBuf,
    file: DockFile,
    runtime: Runtime,
    exec: E,
}

impl<E: Executor> Dock<E> {
    /// Reads `<root>/dock.toml` and picks the runtime.
    pub fn open(root: impl Into<PathBuf>, exec: E) -> Result<Self, DockError> {
        let root = root.into();
        let file = DockFile::load(&root)?;
        Self::new(root, file, exec)
    }

    /// `file` for the app at `root`; picks the runtime ([`RuntimeChoice::resolve`](crate::RuntimeChoice::resolve)).
    pub fn new(root: impl Into<PathBuf>, file: DockFile, exec: E) -> Result<Self, DockError> {
        let runtime = file.runtime.resolve(&exec)?;
        Ok(Dock {
            root: root.into(),
            file,
            runtime,
            exec,
        })
    }

    /// The parsed `dock.toml`.
    pub fn file(&self) -> &DockFile {
        &self.file
    }

    /// The runtime in use.
    pub fn runtime(&self) -> Runtime {
        self.runtime
    }

    /// The executor.
    pub fn executor(&self) -> &E {
        &self.exec
    }

    /// The app's directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Starts the services named (every service when `names` is empty) and
    /// waits until each is ready; then writes the settings into `.env`
    /// (when `write_env` is on). What it does is printed to `out`.
    ///
    /// Before starting anything it checks the ports: a port that another
    /// program or container holds fails with [`DockError::PortsTaken`].
    /// Missing images are pulled first (with the runtime's own progress
    /// output). A service that is not ready within [`READY_TIMEOUT`] fails
    /// with its last log lines.
    pub fn up(&self, names: &[String], out: &mut dyn Write) -> Result<(), DockError> {
        let selected = self.file.select(names)?;
        if selected.is_empty() {
            return Err(DockError::config(
                "no services yet; add one with `navree dock:add <service>`",
            ));
        }
        compose::write(&self.root, &self.file)?;
        say(
            out,
            format_args!(
                "Starting {} services with {}",
                self.file.project, self.runtime
            ),
        );

        let containers = self.containers()?;
        let already_running: Vec<bool> = selected
            .iter()
            .map(|spec| is_current(spec, containers.get(&spec.container)))
            .collect();
        self.check_ports(&selected, &containers)?;
        self.pull_missing(&selected, out)?;

        let mut up = vec!["up".to_owned(), "--detach".to_owned()];
        up.extend(selected.iter().map(|spec| spec.name().to_owned()));
        self.run_compose(&up)?;

        let ready_after = self.wait_until_ready(&selected)?;
        for spec in &selected {
            for step in spec.setup() {
                let cmd = self.exec_in(&spec.container, &step.env, &step.args, false);
                self.run_checked(&cmd.timeout(COMMAND_TIMEOUT))?;
            }
        }

        let images: Vec<&str> = selected.iter().map(|spec| spec.image.as_str()).collect();
        let addresses: Vec<String> = selected
            .iter()
            .map(|spec| format!("{HOST}:{}", spec.port()))
            .collect();
        let name_width = width(selected.iter().map(|spec| spec.name()));
        let image_width = width(images.iter().copied());
        let address_width = width(addresses.iter().map(String::as_str));
        for (index, spec) in selected.iter().enumerate() {
            let state = if already_running[index] {
                "already running".to_owned()
            } else {
                format!("ready in {:.1} s", ready_after[index].as_secs_f64())
            };
            say(
                out,
                format_args!(
                    "  {:<name_width$}  {:<image_width$}  {:<address_width$}  {state}",
                    spec.name(),
                    images[index],
                    addresses[index],
                ),
            );
        }
        for spec in &selected {
            if let Some(ui) = spec.web_ui() {
                say(out, format_args!("{ui}"));
            }
        }
        if self.file.write_env {
            self.write_env(out)?;
        }
        Ok(())
    }

    /// Stops and removes the containers of the services named, or of every
    /// service when `names` is empty; with `volumes`, deletes their data too.
    pub fn down(
        &self,
        names: &[String],
        volumes: bool,
        out: &mut dyn Write,
    ) -> Result<(), DockError> {
        let selected = self.file.select(names)?;
        let containers = self.containers()?;
        if names.is_empty() {
            compose::write(&self.root, &self.file)?;
            let mut args = vec!["down".to_owned(), "--remove-orphans".to_owned()];
            if volumes {
                args.push("--volumes".to_owned());
            }
            self.run_compose(&args)?;
            if volumes {
                say(
                    out,
                    format_args!(
                        "Stopped {} services and deleted their data.",
                        self.file.project
                    ),
                );
            } else {
                say(out, format_args!("Stopped {} services.", self.file.project));
            }
            return Ok(());
        }
        let name_width = width(selected.iter().map(|spec| spec.name()));
        for spec in selected {
            self.remove(spec, containers.contains_key(&spec.container), volumes)?;
            let done = if volumes {
                "stopped, data deleted"
            } else {
                "stopped"
            };
            say(out, format_args!("  {:<name_width$}  {done}", spec.name()));
        }
        Ok(())
    }

    /// Stops a service's container and, with `volumes`, deletes its data.
    /// Used by `navree dock down <service>` and `navree dock:remove`.
    pub fn remove_container(&self, name: &str, volumes: bool) -> Result<(), DockError> {
        let spec = self.file.service(name)?;
        let containers = self.containers()?;
        self.remove(spec, containers.contains_key(&spec.container), volumes)
    }

    fn remove(&self, spec: &ServiceSpec, exists: bool, volumes: bool) -> Result<(), DockError> {
        if exists {
            let cmd = self.cmd(["rm", "--force", spec.container.as_str()]);
            self.run_checked(&cmd.timeout(COMMAND_TIMEOUT))?;
        }
        if volumes {
            let cmd = self
                .cmd(["volume", "rm", "--force", spec.volume.as_str()])
                .timeout(COMMAND_TIMEOUT);
            let output = self.exec.run(&cmd)?;
            if !output.success() && !output.stderr.to_lowercase().contains("no such volume") {
                output.check(&cmd)?;
            }
        }
        Ok(())
    }

    /// The state, image and addresses of every service in `dock.toml`.
    pub fn status(&self) -> Result<Vec<ServiceStatus>, DockError> {
        let containers = self.containers()?;
        Ok(self
            .file
            .services
            .iter()
            .map(|spec| match containers.get(&spec.container) {
                Some(container) => {
                    let state = match container.state.as_str() {
                        "exited" | "dead" | "stopped" => "stopped".to_owned(),
                        other => other.to_owned(),
                    };
                    let addresses = if state == "running" {
                        spec.addresses()
                    } else {
                        Vec::new()
                    };
                    ServiceStatus {
                        service: spec.name().to_owned(),
                        image: (state == "running").then(|| container.image.clone()),
                        state,
                        addresses,
                    }
                }
                None => ServiceStatus {
                    service: spec.name().to_owned(),
                    state: "stopped".to_owned(),
                    image: None,
                    addresses: Vec::new(),
                },
            })
            .collect())
    }

    /// Shows a service's output (`--follow` keeps streaming); returns the
    /// runtime's exit code.
    pub fn logs(&self, name: &str, follow: bool) -> Result<i32, DockError> {
        let spec = self.file.service(name)?;
        let mut args = vec!["logs"];
        if follow {
            args.push("--follow");
        }
        args.push(&spec.container);
        self.exec.run_attached(&self.cmd(args))
    }

    /// Opens the service's own client (`psql`, `redis-cli`, the `nats` CLI
    /// in a nats-box container) or a shell in its container; returns the
    /// exit code.
    pub fn shell(&self, name: &str) -> Result<i32, DockError> {
        let spec = self.file.service(name)?;
        let cmd = match spec.shell() {
            Shell::Exec { env, args } => self.exec_in(&spec.container, &env, &args, true),
            Shell::Run { image, env, args } => {
                let mut words = vec![
                    "run".to_owned(),
                    "--rm".to_owned(),
                    "-it".to_owned(),
                    "--network".to_owned(),
                    self.file.network(),
                ];
                for (key, value) in env {
                    words.push("-e".to_owned());
                    words.push(format!("{key}={value}"));
                }
                words.push(image.to_owned());
                words.extend(args);
                self.cmd(words)
            }
        };
        self.exec.run_attached(&cmd)
    }

    /// `<runtime> <args>`.
    fn cmd<I>(&self, args: I) -> Cmd
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Cmd::new(self.runtime.program(), args)
    }

    /// `<runtime> exec [-it] [-e K=V]... <container> <args>`.
    fn exec_in(
        &self,
        container: &str,
        env: &[(String, String)],
        args: &[String],
        tty: bool,
    ) -> Cmd {
        let mut words = vec!["exec".to_owned()];
        if tty {
            words.push("-it".to_owned());
        }
        for (key, value) in env {
            words.push("-e".to_owned());
            words.push(format!("{key}={value}"));
        }
        words.push(container.to_owned());
        words.extend(args.iter().cloned());
        self.cmd(words)
    }

    fn run_checked(&self, cmd: &Cmd) -> Result<CmdOutput, DockError> {
        self.exec.run(cmd)?.check(cmd)
    }

    /// `<runtime> compose --project-name <p> --file <file> <args>`.
    fn run_compose(&self, args: &[String]) -> Result<CmdOutput, DockError> {
        let mut words = vec![
            "compose".to_owned(),
            "--project-name".to_owned(),
            self.file.project.clone(),
            "--file".to_owned(),
            COMPOSE_FILE.to_owned(),
        ];
        words.extend(args.iter().cloned());
        let cmd = self.cmd(words).timeout(COMMAND_TIMEOUT);
        let output = self.exec.run(&cmd)?;
        if !output.success() && compose_missing(&output) {
            return Err(DockError::ComposeMissing(match self.runtime {
                Runtime::Docker => {
                    "`docker compose` is not available; install Docker Compose v2 (Docker Desktop includes it)".to_owned()
                }
                Runtime::Podman => {
                    "`podman compose` needs a compose provider; install podman-compose or docker-compose".to_owned()
                }
            }));
        }
        output.check(&cmd)
    }

    /// This project's containers by name, from `ps`. It is the first call
    /// to the runtime, so a hang becomes [`DockError::NotResponding`].
    fn containers(&self) -> Result<BTreeMap<String, Container>, DockError> {
        let cmd = self
            .cmd([
                "ps".to_owned(),
                "--all".to_owned(),
                "--filter".to_owned(),
                format!("label={PROJECT_LABEL}={}", self.file.project),
                "--format".to_owned(),
                "{{.Names}}|{{.State}}|{{.Image}}|{{.Ports}}".to_owned(),
            ])
            .timeout(ANSWER_TIMEOUT);
        let output = match self.exec.run(&cmd) {
            Err(DockError::Timeout { .. }) => {
                return Err(DockError::NotResponding {
                    runtime: self.runtime,
                    seconds: ANSWER_TIMEOUT.as_secs(),
                });
            }
            other => other?.check(&cmd)?,
        };
        Ok(output
            .stdout
            .lines()
            .filter_map(|line| {
                let mut fields = line.trim().splitn(4, '|');
                let name = fields.next()?.trim();
                if name.is_empty() {
                    return None;
                }
                let container = Container {
                    state: fields.next().unwrap_or_default().trim().to_lowercase(),
                    image: fields.next().unwrap_or_default().trim().to_owned(),
                    ports: fields.next().unwrap_or_default().trim().to_owned(),
                };
                Some((name.to_owned(), container))
            })
            .collect())
    }

    /// Fails when a port a service needs is taken by something other than
    /// that service's own running container.
    fn check_ports(
        &self,
        selected: &[&ServiceSpec],
        containers: &BTreeMap<String, Container>,
    ) -> Result<(), DockError> {
        let mut conflicts = Vec::new();
        for spec in selected {
            let own = containers
                .get(&spec.container)
                .filter(|c| c.state == "running");
            for port in &spec.ports {
                let held_by_us = own.is_some_and(|c| publishes(&c.ports, port.host));
                if !held_by_us && self.exec.port_taken(port.host) {
                    conflicts.push(PortConflict {
                        service: spec.name().to_owned(),
                        key: port.key,
                        port: port.host,
                        holder: None,
                    });
                }
            }
        }
        if conflicts.is_empty() {
            return Ok(());
        }
        // Name the containers that hold them, when containers do.
        let cmd = self
            .cmd(["ps", "--format", "{{.Names}}|{{.Ports}}"])
            .timeout(COMMAND_TIMEOUT);
        if let Ok(output) = self.exec.run(&cmd)
            && output.success()
        {
            for conflict in &mut conflicts {
                conflict.holder = output.stdout.lines().find_map(|line| {
                    let (name, ports) = line.split_once('|')?;
                    publishes(ports, conflict.port).then(|| name.trim().to_owned())
                });
            }
        }
        Err(DockError::PortsTaken(conflicts))
    }

    /// Pulls the images that are not on the machine yet, showing the
    /// runtime's progress.
    fn pull_missing(
        &self,
        selected: &[&ServiceSpec],
        out: &mut dyn Write,
    ) -> Result<(), DockError> {
        for spec in selected {
            let inspect = self
                .cmd([
                    "image",
                    "inspect",
                    "--format",
                    "{{.Id}}",
                    spec.image.as_str(),
                ])
                .timeout(COMMAND_TIMEOUT);
            if self.exec.run(&inspect)?.success() {
                continue;
            }
            say(out, format_args!("  pulling  {}", spec.image));
            let pull = self.cmd(["pull", spec.image.as_str()]);
            let code = self.exec.run_attached(&pull)?;
            if code != 0 {
                return Err(DockError::CommandFailed {
                    command: pull.to_string(),
                    code,
                    output: String::new(),
                });
            }
        }
        Ok(())
    }

    /// Checks every service until all are ready; returns how long each
    /// took, in `selected`'s order.
    fn wait_until_ready(&self, selected: &[&ServiceSpec]) -> Result<Vec<Duration>, DockError> {
        let started = self.exec.now();
        let mut ready_after: Vec<Option<Duration>> = vec![None; selected.len()];
        loop {
            for (index, spec) in selected.iter().enumerate() {
                if ready_after[index].is_none() && self.is_ready(spec)? {
                    ready_after[index] = Some(self.exec.now().saturating_sub(started));
                }
            }
            if let Some(index) = ready_after.iter().position(Option::is_none) {
                if self.exec.now().saturating_sub(started) >= READY_TIMEOUT {
                    let spec = selected[index];
                    return Err(DockError::NotReady {
                        service: spec.name().to_owned(),
                        seconds: READY_TIMEOUT.as_secs(),
                        logs: self.last_logs(spec),
                    });
                }
                self.exec.sleep(POLL_INTERVAL);
            } else {
                return Ok(ready_after
                    .into_iter()
                    .map(Option::unwrap_or_default)
                    .collect());
            }
        }
    }

    fn is_ready(&self, spec: &ServiceSpec) -> Result<bool, DockError> {
        match spec.readiness() {
            Readiness::Exec { args, expect } => {
                let cmd = self
                    .exec_in(&spec.container, &[], &args, false)
                    .timeout(PROBE_TIMEOUT);
                match self.exec.run(&cmd) {
                    Ok(output) => Ok(output.success()
                        && expect.is_none_or(|expected| output.stdout.trim() == expected)),
                    Err(DockError::Timeout { .. }) => Ok(false),
                    Err(error) => Err(error),
                }
            }
            Readiness::Http { port, path } => Ok(self.exec.http_status(port, path) == Some(200)),
        }
    }

    /// The last 20 lines a container printed, for errors.
    fn last_logs(&self, spec: &ServiceSpec) -> String {
        let cmd = self
            .cmd(["logs", "--tail", "20", spec.container.as_str()])
            .timeout(COMMAND_TIMEOUT);
        match self.exec.run(&cmd) {
            Ok(output) => format!("{}{}", output.stdout, output.stderr),
            Err(_) => String::new(),
        }
    }

    /// Writes the settings into `.env` and says what changed.
    fn write_env(&self, out: &mut dyn Write) -> Result<(), DockError> {
        let settings = self.file.env();
        let update = write_dotenv(&self.root, &settings)?;
        let names: Vec<&str> = settings.iter().map(|(name, _)| name.as_str()).collect();
        let count = match names.len() {
            1 => "1 setting".to_owned(),
            n => format!("{n} settings"),
        };
        if update.changed {
            say(
                out,
                format_args!("Wrote {count} to .env: {}.", names.join(", ")),
            );
        } else {
            say(out, format_args!(".env already has the {count}."));
        }
        for name in update.overridden {
            say(
                out,
                format_args!(
                    "Note: .env sets {name} again below the dock block, and that value wins."
                ),
            );
        }
        Ok(())
    }
}

/// The settings `navree dev` and forwarded commands pass to the app: those
/// of `<root>/dock.toml`, or none when the app has no `dock.toml`.
pub fn app_env(root: &Path) -> Result<Vec<(String, String)>, DockError> {
    if !root.join(DOCK_FILE).is_file() {
        return Ok(Vec::new());
    }
    Ok(DockFile::load(root)?.env())
}

/// `navree dock status`'s table.
pub fn status_table(statuses: &[ServiceStatus]) -> String {
    let images: Vec<&str> = statuses
        .iter()
        .map(|s| s.image.as_deref().unwrap_or_default())
        .collect();
    let service_width = width(
        statuses
            .iter()
            .map(|s| s.service.as_str())
            .chain(["Service"]),
    );
    let state_width = width(statuses.iter().map(|s| s.state.as_str()).chain(["State"]));
    let image_width = width(images.iter().copied().chain(["Image"]));
    let mut rows = vec![format!(
        "{:<service_width$}  {:<state_width$}  {:<image_width$}  Address",
        "Service", "State", "Image"
    )];
    for (status, image) in statuses.iter().zip(images) {
        let row = format!(
            "{:<service_width$}  {:<state_width$}  {:<image_width$}  {}",
            status.service,
            status.state,
            image,
            status.addresses.join(", ")
        );
        rows.push(row.trim_end().to_owned());
    }
    let mut table = rows.join("\n");
    table.push('\n');
    table
}

/// Whether the container is running the service as `dock.toml` describes
/// it (same image, same ports), so `up` leaves it alone.
fn is_current(spec: &ServiceSpec, container: Option<&Container>) -> bool {
    container.is_some_and(|c| {
        c.state == "running"
            && c.image == spec.image
            && spec.ports.iter().all(|port| {
                c.ports
                    .contains(&format!("{HOST}:{}->{}/", port.host, port.container))
            })
    })
}

/// Whether a `ps` ports column (`127.0.0.1:5432->5432/tcp, ...`) publishes `port`.
fn publishes(ports: &str, port: u16) -> bool {
    ports.contains(&format!(":{port}->"))
}

/// Whether a failed compose command failed because compose is missing.
fn compose_missing(output: &CmdOutput) -> bool {
    let text = format!("{}\n{}", output.stderr, output.stdout).to_lowercase();
    text.contains("is not a docker command")
        || text.contains("unknown command \"compose\"")
        || text.contains("unknown shorthand flag") && text.contains("compose")
        || text.contains("compose provider")
}

fn width<'a>(items: impl Iterator<Item = &'a str>) -> usize {
    items.map(str::len).max().unwrap_or(0)
}

/// Prints a line; a closed stdout is not an error worth failing for.
fn say(out: &mut dyn Write, line: std::fmt::Arguments<'_>) {
    let _ = writeln!(out, "{line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_columns() {
        let ports = "127.0.0.1:1025->1025/tcp, 127.0.0.1:8025->8025/tcp";
        assert!(publishes(ports, 1025) && publishes(ports, 8025));
        assert!(!publishes(ports, 25) && !publishes(ports, 802));
        assert!(publishes("0.0.0.0:5432->5432/tcp, :::5432->5432/tcp", 5432));
    }

    #[test]
    fn compose_missing_is_recognised() {
        assert!(compose_missing(&CmdOutput::failed(
            1,
            "docker: 'compose' is not a docker command."
        )));
        assert!(compose_missing(&CmdOutput::failed(
            125,
            "Error: looking up compose provider failed\n7 errors occurred"
        )));
        assert!(!compose_missing(&CmdOutput::failed(
            1,
            "no such service: x"
        )));
    }

    #[test]
    fn the_status_table_aligns_its_columns() {
        let table = status_table(&[
            ServiceStatus {
                service: "postgres".into(),
                state: "running".into(),
                image: Some("postgres:17".into()),
                addresses: vec!["127.0.0.1:5432".into()],
            },
            ServiceStatus {
                service: "meilisearch".into(),
                state: "stopped".into(),
                image: None,
                addresses: vec![],
            },
        ]);
        assert_eq!(
            table,
            "Service      State    Image        Address\npostgres     running  postgres:17  127.0.0.1:5432\nmeilisearch  stopped\n"
        );
    }
}
