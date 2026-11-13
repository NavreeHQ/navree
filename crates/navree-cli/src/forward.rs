//! Forwarding of runtime commands (`migrate`, `queue:work`, `routes`, any
//! `module:command`) to the app binary, which owns them: `navree migrate
//! --seed` runs `cargo run --quiet -- migrate --seed` in the project.

use std::fmt;
use std::path::Path;
use std::process::{Command, ExitStatus};

/// A program and its arguments, ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The program, e.g. `cargo`.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment variables set for it (the app's Dock settings).
    pub env: Vec<(String, String)>,
}

impl Invocation {
    /// `program` with `args`.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
        }
    }

    /// The same invocation with `env` set in its environment.
    pub fn with_env(mut self, env: Vec<(String, String)>) -> Self {
        self.env = env;
        self
    }

    /// The process to spawn, in `dir`, with inherited standard streams.
    pub fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(dir);
        command
    }

    /// Runs it in `dir` and waits for it.
    pub fn run(&self, dir: &Path) -> std::io::Result<ExitStatus> {
        self.command(dir).status()
    }
}

impl fmt::Display for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for arg in &self.args {
            if arg.is_empty() || arg.contains(char::is_whitespace) {
                write!(f, " \"{arg}\"")?;
            } else {
                write!(f, " {arg}")?;
            }
        }
        Ok(())
    }
}

/// Runs invocations. `navree check` and `navree gate` go through it, so
/// their tests can script the programs' results.
pub trait Runner {
    /// Runs `invocation` in `dir` with the terminal's output; whether it
    /// exited successfully.
    fn run(&mut self, invocation: &Invocation, dir: &Path) -> std::io::Result<bool>;

    /// Runs `invocation` in `dir` capturing its standard output (standard
    /// error still reaches the terminal); whether it succeeded, and the
    /// output.
    fn capture(&mut self, invocation: &Invocation, dir: &Path) -> std::io::Result<(bool, String)>;
}

/// The [`Runner`] that starts real processes.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&mut self, invocation: &Invocation, dir: &Path) -> std::io::Result<bool> {
        Ok(invocation.run(dir)?.success())
    }

    fn capture(&mut self, invocation: &Invocation, dir: &Path) -> std::io::Result<(bool, String)> {
        let output = invocation
            .command(dir)
            .stderr(std::process::Stdio::inherit())
            .output()?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ))
    }
}

/// The invocation running runtime command `command` with `args` in the app
/// binary: `cargo run --quiet -- <command> <args>`.
pub fn forward(command: &str, args: &[String]) -> Invocation {
    forward_with(&[], command, args)
}

/// [`forward`], with the app built with these cargo `features`: `cargo run
/// --quiet --features <features> -- <command> <args>`. A command that only
/// exists when a feature is on (`native:coverage`) is run this way.
pub fn forward_with(features: &[String], command: &str, args: &[String]) -> Invocation {
    let mut all = vec!["run".to_owned(), "--quiet".to_owned()];
    if !features.is_empty() {
        all.push("--features".to_owned());
        all.push(features.join(","));
    }
    all.push("--".to_owned());
    all.push(command.to_owned());
    all.extend(args.iter().cloned());
    Invocation::new("cargo", all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_through_cargo_run() {
        let invocation = forward("migrate:fresh", &["--seed".to_owned()]);
        assert_eq!(invocation.program, "cargo");
        assert_eq!(
            invocation.args,
            ["run", "--quiet", "--", "migrate:fresh", "--seed"]
        );
    }

    #[test]
    fn features_are_passed_to_cargo_before_the_command() {
        let invocation = forward_with(
            &["native-preview".to_owned(), "orm".to_owned()],
            "native:coverage",
            &["--json".to_owned()],
        );
        assert_eq!(
            invocation.to_string(),
            "cargo run --quiet --features native-preview,orm -- native:coverage --json"
        );
        assert_eq!(forward_with(&[], "routes", &[]), forward("routes", &[]));
    }

    #[test]
    fn dock_settings_reach_the_process_environment() {
        let invocation = forward("migrate", &[]).with_env(vec![(
            "APP_DATABASE__URL".to_owned(),
            "postgres://navree:navree@127.0.0.1:5432/blog".to_owned(),
        )]);
        let command = invocation.command(Path::new("."));
        let envs: Vec<_> = command.get_envs().collect();
        assert_eq!(
            envs,
            [(
                std::ffi::OsStr::new("APP_DATABASE__URL"),
                Some(std::ffi::OsStr::new(
                    "postgres://navree:navree@127.0.0.1:5432/blog"
                ))
            )]
        );
        assert!(forward("migrate", &[]).env.is_empty());
    }

    #[test]
    fn arguments_pass_through_untouched() {
        let args = ["--queue".to_owned(), "a,b".to_owned(), "--once".to_owned()];
        let invocation = forward("queue:work", &args);
        assert_eq!(
            invocation.args,
            [
                "run",
                "--quiet",
                "--",
                "queue:work",
                "--queue",
                "a,b",
                "--once"
            ]
        );
        assert_eq!(
            invocation.to_string(),
            "cargo run --quiet -- queue:work --queue a,b --once"
        );
    }
}
