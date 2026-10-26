//! Harbor's errors.

use std::path::PathBuf;

/// A Harbor result.
pub type Result<T, E = HarborError> = std::result::Result<T, E>;

/// Everything that can go wrong in Harbor. Messages say what to do next.
#[derive(Debug, thiserror::Error)]
pub enum HarborError {
    /// A file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// `harbor.toml` (or `harbor.<env>.toml`) does not exist.
    #[error("{} not found; run `navree harbor:init` in your app's directory", path.display())]
    NoConfig {
        /// The expected file.
        path: PathBuf,
    },
    /// `harbor.toml` is not valid TOML or has unknown keys.
    #[error("{}: {message}", path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// What the parser said.
        message: String,
    },
    /// `harbor.toml` parses but describes something Harbor cannot run.
    #[error("{} is invalid:\n  - {}", path.display(), problems.join("\n  - "))]
    Invalid {
        /// The file.
        path: PathBuf,
        /// One line per problem.
        problems: Vec<String>,
    },
    /// No secrets key: neither the key file nor `NAVREE_HARBOR_KEY`.
    #[error(
        "no secrets key: {} does not exist and NAVREE_HARBOR_KEY is not set (run `navree harbor:init`, or get the key from your team)",
        path.display()
    )]
    MissingKey {
        /// The expected key file.
        path: PathBuf,
    },
    /// The secrets key is not `base64:<32 bytes>`.
    #[error("the secrets key is invalid: {0}")]
    BadKey(String),
    /// A secret did not decrypt with the key (wrong key, edited or moved
    /// value).
    #[error(
        "secret `{name}` cannot be decrypted with this key (a different harbor.key, or the value was edited or copied from another name)"
    )]
    Decrypt {
        /// The secret's name.
        name: String,
    },
    /// A secret that is not in `[secrets]`.
    #[error("no secret named `{0}` in [secrets]")]
    UnknownSecret(String),
    /// A server that is not in `[servers]`.
    #[error("no server named `{0}` in [servers]")]
    UnknownServer(String),
    /// A local program (ssh, cargo, docker, git) could not be started.
    #[error("cannot run `{program}`: {source} (is it installed and on PATH?)")]
    Spawn {
        /// The program.
        program: String,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// A local program failed.
    #[error("`{command}` failed{}:\n{output}", code.map(|c| format!(" with exit code {c}")).unwrap_or_default())]
    Local {
        /// The command line, without secrets.
        command: String,
        /// Its exit code.
        code: Option<i32>,
        /// Its last output lines.
        output: String,
    },
    /// A step failed on a server.
    #[error("{step} failed on {server}{}:\n{output}", code.map(|c| format!(" (exit code {c})")).unwrap_or_default())]
    Remote {
        /// The server's name.
        server: String,
        /// The step (`upload`, `migrate`, `firewall`, ...).
        step: String,
        /// The exit code of `ssh` (255: the connection failed).
        code: Option<i32>,
        /// The last output lines.
        output: String,
    },
    /// A new release did not pass its health check.
    #[error("{0}")]
    Unhealthy(Box<Unhealthy>),
    /// Anything else, with a message that says what to do.
    #[error("{0}")]
    Message(String),
}

/// A release that did not pass its health check.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{server}: GET {path} on {address} did not answer 200 within {timeout_secs}s (last status: {status}); the previous release goes on serving.\nLast log lines:\n{logs}"
)]
pub struct Unhealthy {
    /// The server's name.
    pub server: String,
    /// The health check path.
    pub path: String,
    /// Where it was requested.
    pub address: String,
    /// `deploy.health_timeout_secs`.
    pub timeout_secs: u64,
    /// The last HTTP status (`000` when nothing answered).
    pub status: String,
    /// The process's last log lines.
    pub logs: String,
}

impl HarborError {
    /// An [`Io`](HarborError::Io) error for `path`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// A [`Message`](HarborError::Message) error.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

/// The last `max` lines of `text`, trimmed.
pub(crate) fn tail(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let start = lines.len().saturating_sub(max);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_lists_every_problem() {
        let error = HarborError::Invalid {
            path: "harbor.toml".into(),
            problems: vec!["a".into(), "b".into()],
        };
        assert_eq!(error.to_string(), "harbor.toml is invalid:\n  - a\n  - b");
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        assert_eq!(tail("1\n2\n3\n", 2), "2\n3");
        assert_eq!(tail("1", 5), "1");
        assert_eq!(tail("", 5), "");
    }

    #[test]
    fn remote_errors_name_the_server_and_step() {
        let error = HarborError::Remote {
            server: "app-1".into(),
            step: "migrate".into(),
            code: Some(1),
            output: "boom".into(),
        };
        assert_eq!(
            error.to_string(),
            "migrate failed on app-1 (exit code 1):\nboom"
        );
    }
}
