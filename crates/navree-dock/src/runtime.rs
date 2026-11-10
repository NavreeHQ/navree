//! Docker or Podman: which container runtime Dock drives.

use std::fmt;
use std::str::FromStr;

use crate::error::DockError;
use crate::exec::Executor;

/// The environment variable that picks the runtime for one person, over
/// `dock.toml`'s `runtime`.
pub const RUNTIME_VARIABLE: &str = "NAVREE_DOCK_RUNTIME";

/// A container runtime with a Docker-compatible command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Runtime {
    /// `docker` (Docker Desktop or Docker Engine) with `docker compose`.
    Docker,
    /// `podman` with `podman compose`.
    Podman,
}

impl Runtime {
    /// The program: `docker` or `podman`.
    pub fn program(self) -> &'static str {
        match self {
            Runtime::Docker => "docker",
            Runtime::Podman => "podman",
        }
    }
}

impl fmt::Display for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.program())
    }
}

/// `dock.toml`'s `runtime`: a runtime, or `auto`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeChoice {
    /// Docker when the `docker` command exists, else Podman.
    #[default]
    Auto,
    /// Always Docker.
    Docker,
    /// Always Podman.
    Podman,
}

impl FromStr for RuntimeChoice {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(RuntimeChoice::Auto),
            "docker" => Ok(RuntimeChoice::Docker),
            "podman" => Ok(RuntimeChoice::Podman),
            other => Err(format!(
                "`runtime` must be \"docker\", \"podman\" or \"auto\", not \"{other}\""
            )),
        }
    }
}

impl RuntimeChoice {
    /// The runtime to use: `NAVREE_DOCK_RUNTIME` when set (it overrides
    /// `dock.toml`), else this choice; `auto` is Docker when the `docker`
    /// command exists, else Podman. The runtime must be on PATH.
    pub fn resolve(self, exec: &dyn Executor) -> Result<Runtime, DockError> {
        let choice = match exec.var(RUNTIME_VARIABLE).filter(|v| !v.trim().is_empty()) {
            Some(value) => value
                .trim()
                .parse::<RuntimeChoice>()
                .map_err(|_| DockError::InvalidRuntimeVariable(value))?,
            None => self,
        };
        let wanted = match choice {
            RuntimeChoice::Docker => Runtime::Docker,
            RuntimeChoice::Podman => Runtime::Podman,
            RuntimeChoice::Auto => {
                return [Runtime::Docker, Runtime::Podman]
                    .into_iter()
                    .find(|runtime| exec.has_program(runtime.program()))
                    .ok_or(DockError::NoRuntime);
            }
        };
        if exec.has_program(wanted.program()) {
            Ok(wanted)
        } else {
            Err(DockError::RuntimeMissing(wanted))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeExecutor;

    #[test]
    fn auto_prefers_docker_then_podman() {
        let both = FakeExecutor::new().with_programs(["docker", "podman"]);
        assert_eq!(RuntimeChoice::Auto.resolve(&both).unwrap(), Runtime::Docker);
        let podman = FakeExecutor::new().with_programs(["podman"]);
        assert_eq!(
            RuntimeChoice::Auto.resolve(&podman).unwrap(),
            Runtime::Podman
        );
        let none = FakeExecutor::new().with_programs::<[&str; 0]>([]);
        assert!(matches!(
            RuntimeChoice::Auto.resolve(&none),
            Err(DockError::NoRuntime)
        ));
    }

    #[test]
    fn the_variable_overrides_the_file() {
        let exec = FakeExecutor::new()
            .with_programs(["docker", "podman"])
            .with_var(RUNTIME_VARIABLE, "podman");
        assert_eq!(
            RuntimeChoice::Docker.resolve(&exec).unwrap(),
            Runtime::Podman
        );
        let auto = FakeExecutor::new()
            .with_programs(["docker", "podman"])
            .with_var(RUNTIME_VARIABLE, "auto");
        assert_eq!(
            RuntimeChoice::Podman.resolve(&auto).unwrap(),
            Runtime::Docker
        );
        let wrong = FakeExecutor::new().with_var(RUNTIME_VARIABLE, "lxc");
        let error = RuntimeChoice::Auto.resolve(&wrong).unwrap_err();
        assert_eq!(
            error.to_string(),
            "NAVREE_DOCK_RUNTIME must be `docker`, `podman` or `auto`, not `lxc`"
        );
    }

    #[test]
    fn a_chosen_runtime_must_be_installed() {
        let exec = FakeExecutor::new().with_programs(["docker"]);
        let error = RuntimeChoice::Podman.resolve(&exec).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Dock is set to use podman, but `podman` is not on PATH"
        );
    }

    #[test]
    fn choices_parse_from_dock_toml() {
        assert_eq!("auto".parse(), Ok(RuntimeChoice::Auto));
        assert_eq!("docker".parse(), Ok(RuntimeChoice::Docker));
        assert_eq!("podman".parse(), Ok(RuntimeChoice::Podman));
        assert!("Docker".parse::<RuntimeChoice>().is_err());
    }
}
