//! Dock's errors. Each message says what went wrong and what to do next.

use std::fmt;
use std::path::PathBuf;

use crate::runtime::Runtime;

/// A host port another program or container already holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortConflict {
    /// The service that wants the port.
    pub service: String,
    /// Its `dock.toml` key: `port`, `ui_port`, `console_port` or `monitor_port`.
    pub key: &'static str,
    /// The port on 127.0.0.1.
    pub port: u16,
    /// The container that publishes it, when one does.
    pub holder: Option<String>,
}

impl fmt::Display for PortConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let by = match &self.holder {
            Some(container) => format!("container {container}"),
            None => "another program".to_owned(),
        };
        write!(
            f,
            "port {} ({}) is taken by {by}; set `{}` in [services.{}] of dock.toml",
            self.port, self.service, self.key, self.service
        )
    }
}

/// Why a Dock operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DockError {
    /// `dock.toml` is not valid.
    #[error("dock.toml: {0}")]
    Config(String),

    /// The app has no `dock.toml`.
    #[error("no dock.toml in {}; add a service with `navree dock:add <service>`", .0.display())]
    NoDockFile(PathBuf),

    /// A name that is not one of Dock's services.
    #[error(
        "unknown service `{0}`; Dock runs postgres, redis, meilisearch, mailpit, minio and nats"
    )]
    UnknownService(String),

    /// A service that `dock.toml` does not list.
    #[error("{0} is not in dock.toml; add it with `navree dock:add {0}`")]
    NotInDockFile(String),

    /// Neither `docker` nor `podman` is installed.
    #[error("Dock needs Docker or Podman, and neither `docker` nor `podman` is on PATH")]
    NoRuntime,

    /// `NAVREE_DOCK_RUNTIME` holds something else than a runtime.
    #[error("NAVREE_DOCK_RUNTIME must be `docker`, `podman` or `auto`, not `{0}`")]
    InvalidRuntimeVariable(String),

    /// The chosen runtime is not installed.
    #[error("Dock is set to use {0}, but `{0}` is not on PATH")]
    RuntimeMissing(Runtime),

    /// The runtime did not answer in time (its daemon or machine is not running).
    #[error(
        "{runtime} did not answer within {seconds} s; is it running? (start Docker Desktop or the Docker service, or `podman machine start`)"
    )]
    NotResponding {
        /// The runtime.
        runtime: Runtime,
        /// How long Dock waited.
        seconds: u64,
    },

    /// `docker compose` or `podman compose` is not available.
    #[error("{0}")]
    ComposeMissing(String),

    /// A command could not be started.
    #[error("cannot run `{command}`: {source}")]
    Spawn {
        /// The command line.
        command: String,
        /// Why.
        source: std::io::Error,
    },

    /// A command ran out of time and was stopped.
    #[error("`{command}` did not finish within {seconds} s")]
    Timeout {
        /// The command line.
        command: String,
        /// The time limit.
        seconds: u64,
    },

    /// A command failed.
    #[error("`{command}` failed (exit code {code}){}", tail(.output))]
    CommandFailed {
        /// The command line.
        command: String,
        /// Its exit code (-1 when it was killed by a signal).
        code: i32,
        /// What it printed on stderr (or stdout when stderr was empty).
        output: String,
    },

    /// Ports a service needs are taken.
    #[error("{}", ports(.0))]
    PortsTaken(Vec<PortConflict>),

    /// A service did not pass its readiness check in time.
    #[error("{service} was not ready after {seconds} s{}", log_tail(.logs))]
    NotReady {
        /// The service.
        service: String,
        /// The time limit.
        seconds: u64,
        /// Its last log lines.
        logs: String,
    },

    /// A file could not be read or written.
    #[error("{}: {source}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

impl DockError {
    /// A [`DockError::Config`].
    pub(crate) fn config(message: impl Into<String>) -> Self {
        DockError::Config(message.into())
    }

    /// A [`DockError::Io`] for `path`.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        DockError::Io {
            path: path.into(),
            source,
        }
    }
}

fn tail(output: &str) -> String {
    let output = output.trim();
    if output.is_empty() {
        String::new()
    } else {
        format!(":\n{output}")
    }
}

fn ports(conflicts: &[PortConflict]) -> String {
    let lines: Vec<String> = conflicts.iter().map(ToString::to_string).collect();
    format!(
        "{}\nNothing was started. Another app's services may be running: give this app other ports in dock.toml.",
        lines.join("\n")
    )
}

fn log_tail(logs: &str) -> String {
    let logs = logs.trim();
    if logs.is_empty() {
        "; it printed nothing".to_owned()
    } else {
        format!(". Its last log lines:\n{logs}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_conflicts_name_the_holder_and_the_key_to_change() {
        let error = DockError::PortsTaken(vec![
            PortConflict {
                service: "postgres".into(),
                key: "port",
                port: 5432,
                holder: Some("shop-postgres".into()),
            },
            PortConflict {
                service: "mailpit".into(),
                key: "ui_port",
                port: 8025,
                holder: None,
            },
        ]);
        let message = error.to_string();
        assert!(
            message.contains(
                "port 5432 (postgres) is taken by container shop-postgres; set `port` in [services.postgres] of dock.toml"
            ),
            "{message}"
        );
        assert!(
            message.contains("port 8025 (mailpit) is taken by another program; set `ui_port`"),
            "{message}"
        );
        assert!(message.contains("Nothing was started"), "{message}");
    }

    #[test]
    fn failed_commands_show_their_output() {
        let error = DockError::CommandFailed {
            command: "docker pull redis:7".into(),
            code: 1,
            output: "  manifest unknown \n".into(),
        };
        assert_eq!(
            error.to_string(),
            "`docker pull redis:7` failed (exit code 1):\nmanifest unknown"
        );
        let silent = DockError::CommandFailed {
            command: "docker pull redis:7".into(),
            code: 1,
            output: String::new(),
        };
        assert_eq!(
            silent.to_string(),
            "`docker pull redis:7` failed (exit code 1)"
        );
    }

    #[test]
    fn not_ready_shows_the_last_log_lines() {
        let error = DockError::NotReady {
            service: "postgres".into(),
            seconds: 60,
            logs: "FATAL: data directory has wrong ownership\n".into(),
        };
        assert_eq!(
            error.to_string(),
            "postgres was not ready after 60 s. Its last log lines:\nFATAL: data directory has wrong ownership"
        );
    }
}
