//! Forwarding of runtime commands (`migrate`, `queue:work`, `routes`, any
//! `module:command`) to the app binary, which owns them: `navis migrate
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
        }
    }

    /// The process to spawn, in `dir`, with inherited standard streams.
    pub fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).current_dir(dir);
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

/// The invocation running runtime command `command` with `args` in the app
/// binary: `cargo run --quiet -- <command> <args>`.
pub fn forward(command: &str, args: &[String]) -> Invocation {
    let mut all = vec![
        "run".to_owned(),
        "--quiet".to_owned(),
        "--".to_owned(),
        command.to_owned(),
    ];
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
