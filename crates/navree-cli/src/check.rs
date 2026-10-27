//! `navree check`: the app's module graph (the binary's built-in `check`,
//! which constructs nothing), then its conformity checks
//! (`conformity:check`, which boots the app once without serving and runs
//! every check its modules contribute: routes documented, entities
//! classified, workspace entities reached through `WorkspaceRepo`,
//! translations complete, migrations linted).

use std::io::Write;
use std::path::Path;

use crate::forward::{Invocation, Runner, forward};

/// The app command that runs the conformity checks.
pub const CONFORMITY_COMMAND: &str = "conformity:check";

/// What `navree check` does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Print the module graph only (`check --graph`), nothing else.
    pub graph: bool,
    /// Print the conformity report as JSON.
    pub json: bool,
    /// Only these checks (names or prefixes: `workspace`, `api.docs`).
    pub only: Vec<String>,
    /// Environment variables for the app (its Dock settings).
    pub env: Vec<(String, String)>,
}

impl CheckOptions {
    fn app(&self, command: &str, args: Vec<String>) -> Invocation {
        forward(command, &args).with_env(self.env.clone())
    }
}

/// Runs `navree check` for the app in `root`; whether everything passed.
/// Status lines go to `out`; the app's own output reaches the terminal.
pub fn run_check(
    root: &Path,
    options: &CheckOptions,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> anyhow::Result<bool> {
    if options.graph {
        return Ok(runner.run(&options.app("check", vec!["--graph".into()]), root)?);
    }
    let graph = options.app("check", Vec::new());
    let graph_ok = if options.json {
        // Keep standard output for the JSON report.
        runner.capture(&graph, root)?.0
    } else {
        runner.run(&graph, root)?
    };
    if !graph_ok {
        writeln!(out, "navree check: the module graph has errors")?;
        return Ok(false);
    }
    let (help_ok, help) = runner.capture(&options.app("help", Vec::new()), root)?;
    if !help_ok {
        writeln!(out, "navree check: cannot list the app's commands")?;
        return Ok(false);
    }
    if !app_commands(&help).iter().any(|c| c == CONFORMITY_COMMAND) {
        if !options.json {
            writeln!(
                out,
                "No conformity checks: no module of the app contributes one (ApiModule and WorkspaceModule do)."
            )?;
        }
        return Ok(true);
    }
    let mut args = vec!["--root".to_owned(), root.display().to_string()];
    if options.json {
        args.push("--json".to_owned());
    }
    if !options.only.is_empty() {
        args.push("--only".to_owned());
        args.push(options.only.join(","));
    }
    Ok(runner.run(&options.app(CONFORMITY_COMMAND, args), root)?)
}

/// The app's command names from its `help` output (the lines under
/// `Commands:`).
pub fn app_commands(help: &str) -> Vec<String> {
    help.lines()
        .skip_while(|line| line.trim() != "Commands:")
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::VecDeque;

    use super::*;

    /// Records invocations and answers from a script: `(program+args
    /// prefix, success, stdout)`; unscripted invocations succeed silently.
    #[derive(Default)]
    pub(crate) struct FakeRunner {
        pub(crate) ran: Vec<String>,
        pub(crate) script: VecDeque<(String, bool, String)>,
    }

    impl FakeRunner {
        pub(crate) fn answer(mut self, prefix: &str, success: bool, stdout: &str) -> Self {
            self.script
                .push_back((prefix.to_owned(), success, stdout.to_owned()));
            self
        }

        fn result(&mut self, invocation: &Invocation) -> (bool, String) {
            let text = invocation.to_string();
            self.ran.push(text.clone());
            match self
                .script
                .iter()
                .position(|(prefix, _, _)| text.starts_with(prefix.as_str()))
            {
                Some(index) => {
                    let (_, success, stdout) = self.script.remove(index).expect("scripted");
                    (success, stdout)
                }
                None => (true, String::new()),
            }
        }
    }

    impl Runner for FakeRunner {
        fn run(&mut self, invocation: &Invocation, _dir: &Path) -> std::io::Result<bool> {
            Ok(self.result(invocation).0)
        }

        fn capture(
            &mut self,
            invocation: &Invocation,
            _dir: &Path,
        ) -> std::io::Result<(bool, String)> {
            Ok(self.result(invocation))
        }
    }

    pub(crate) const HELP: &str = "Usage: <command> [args...]\n\nBuilt-in commands:\n  check [--graph]   Validate the module graph\n  help              List the available commands\n\nCommands:\n  serve             Serve HTTP\n  conformity:check  Run the conformity checks\n  migrate           Run migrations\n";

    fn check(options: &CheckOptions, runner: &mut FakeRunner) -> (bool, String) {
        let mut out = Vec::new();
        let ok = run_check(Path::new("app"), options, runner, &mut out).unwrap();
        (ok, String::from_utf8(out).unwrap())
    }

    #[test]
    fn checks_the_graph_then_runs_the_conformity_checks() {
        let mut runner = FakeRunner::default().answer("cargo run --quiet -- help", true, HELP);
        let (ok, out) = check(&CheckOptions::default(), &mut runner);
        assert!(ok);
        assert_eq!(out, "");
        assert_eq!(
            runner.ran,
            [
                "cargo run --quiet -- check",
                "cargo run --quiet -- help",
                "cargo run --quiet -- conformity:check --root app",
            ]
        );
    }

    #[test]
    fn json_and_only_reach_the_conformity_command() {
        let mut runner = FakeRunner::default().answer("cargo run --quiet -- help", true, HELP);
        let options = CheckOptions {
            json: true,
            only: vec!["workspace".into(), "api.docs".into()],
            ..CheckOptions::default()
        };
        let (ok, _) = check(&options, &mut runner);
        assert!(ok);
        assert_eq!(
            runner.ran[2],
            "cargo run --quiet -- conformity:check --root app --json --only workspace,api.docs"
        );
    }

    #[test]
    fn a_failing_check_fails() {
        let mut runner = FakeRunner::default()
            .answer("cargo run --quiet -- help", true, HELP)
            .answer("cargo run --quiet -- conformity:check", false, "");
        assert!(!check(&CheckOptions::default(), &mut runner).0);
    }

    #[test]
    fn a_broken_graph_stops_before_booting() {
        let mut runner = FakeRunner::default().answer("cargo run --quiet -- check", false, "");
        let (ok, out) = check(&CheckOptions::default(), &mut runner);
        assert!(!ok);
        assert_eq!(out, "navree check: the module graph has errors\n");
        assert_eq!(runner.ran, ["cargo run --quiet -- check"]);
    }

    #[test]
    fn an_app_without_checks_passes_with_a_note() {
        let help = "Usage: <command>\n\nBuilt-in commands:\n  check  x\n\nCommands:\n  serve  Serve HTTP\n";
        let mut runner = FakeRunner::default().answer("cargo run --quiet -- help", true, help);
        let (ok, out) = check(&CheckOptions::default(), &mut runner);
        assert!(ok);
        assert!(out.starts_with("No conformity checks"), "{out}");
        assert_eq!(runner.ran.len(), 2);
    }

    #[test]
    fn graph_prints_the_module_tree_only() {
        let mut runner = FakeRunner::default();
        let options = CheckOptions {
            graph: true,
            ..CheckOptions::default()
        };
        assert!(check(&options, &mut runner).0);
        assert_eq!(runner.ran, ["cargo run --quiet -- check --graph"]);
    }

    #[test]
    fn dock_settings_reach_the_app() {
        let mut runner = FakeRunner::default();
        let options = CheckOptions {
            graph: true,
            env: vec![("APP_DATABASE__URL".into(), "postgres://x".into())],
            ..CheckOptions::default()
        };
        let invocation = options.app("check", vec![]);
        assert_eq!(invocation.env, options.env);
        check(&options, &mut runner);
    }

    #[test]
    fn commands_are_read_from_the_help_output() {
        assert_eq!(app_commands(HELP), ["serve", "conformity:check", "migrate"]);
        assert!(app_commands("Usage\n").is_empty());
    }
}
