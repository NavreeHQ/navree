//! Asking the app binary: Navigator builds the app with `cargo build` (the
//! debug profile, the app's own target directory) and runs it with
//! `introspect --json` (the module graph, providers and commands, nothing
//! constructed), `routes --json` and `migrate:status --json`.
//!
//! Answers are cached per binary: when `cargo build` leaves the binary as it
//! was (no source changed), the previous answers are reused. Every process
//! goes through a [`Runner`], so tests use [`FakeRunner`].

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::project::Project;

/// How long a build may take.
const BUILD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// How long the app binary may take to answer.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// What a finished process printed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Whether it exited with status 0.
    pub success: bool,
    /// Its standard output.
    pub stdout: String,
    /// Its standard error.
    pub stderr: String,
}

/// One process to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessCall {
    /// The program (`cargo`, or the app binary's path).
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// The working directory.
    pub dir: PathBuf,
    /// Variables added to the inherited environment.
    pub env: Vec<(String, String)>,
}

/// Runs processes: [`ProcessRunner`] for real, [`FakeRunner`] in tests.
pub trait Runner: Send + Sync {
    /// Runs `call` to completion (stdin closed, output captured), or fails
    /// after `timeout`.
    fn run(&self, call: &ProcessCall, timeout: Duration) -> std::io::Result<ProcessOutput>;
}

/// Runs real processes.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, call: &ProcessCall, timeout: Duration) -> std::io::Result<ProcessOutput> {
        let mut child = Command::new(&call.program)
            .args(&call.args)
            .current_dir(&call.dir)
            .envs(call.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let out = std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stdout.read_to_end(&mut text);
            text
        });
        let err = std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stderr.read_to_end(&mut text);
            text
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "{} did not finish within {} s",
                        call.program.display(),
                        timeout.as_secs()
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        Ok(ProcessOutput {
            success: status.success(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }
}

/// A [`Runner`] that answers from a script: the first rule whose program
/// file name and arguments match wins. Records every call.
#[derive(Default)]
pub struct FakeRunner {
    rules: Mutex<Vec<(String, Vec<String>, ProcessOutput)>>,
    calls: Mutex<Vec<ProcessCall>>,
}

impl FakeRunner {
    /// An empty script (every call fails to spawn).
    pub fn new() -> Arc<FakeRunner> {
        Arc::new(FakeRunner::default())
    }

    /// Answers calls of `program` (its file stem: `cargo`, `crm-api`)
    /// whose arguments start with `args`.
    pub fn on(&self, program: &str, args: &[&str], output: ProcessOutput) -> &Self {
        self.rules.lock().expect("rules").push((
            program.to_owned(),
            args.iter().map(|a| (*a).to_owned()).collect(),
            output,
        ));
        self
    }

    /// The calls made so far.
    pub fn calls(&self) -> Vec<ProcessCall> {
        self.calls.lock().expect("calls").clone()
    }
}

impl std::fmt::Debug for FakeRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeRunner")
            .field("calls", &self.calls().len())
            .finish_non_exhaustive()
    }
}

impl Runner for FakeRunner {
    fn run(&self, call: &ProcessCall, _timeout: Duration) -> std::io::Result<ProcessOutput> {
        self.calls.lock().expect("calls").push(call.clone());
        let stem = call
            .program
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rules = self.rules.lock().expect("rules");
        rules
            .iter()
            .find(|(program, args, _)| *program == stem && call.args.starts_with(args))
            .map(|(_, _, output)| output.clone())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("FakeRunner: no rule for {stem} {:?}", call.args),
                )
            })
    }
}

/// Why the app could not answer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    /// The app doesn't compile.
    #[error("The app doesn't build. The compiler's first error:\n\n{first_error}")]
    BuildFailed {
        /// The first error, as the compiler rendered it.
        first_error: String,
    },
    /// `--no-build` and no binary built yet.
    #[error(
        "Navigator runs with --no-build and there is no built binary at {}. Build the app (`cargo build`), or \
         start Navigator without --no-build.",
        .path.display()
    )]
    NotBuilt {
        /// Where the binary would be.
        path: PathBuf,
    },
    /// Cargo could not be run or did not describe the package.
    #[error("{0}")]
    Cargo(String),
    /// The binary failed.
    #[error("`{command}` failed:\n\n{output}")]
    Run {
        /// The command line.
        command: String,
        /// What it printed on stderr (or stdout).
        output: String,
    },
    /// The binary answered something that isn't the expected JSON.
    #[error("`{command}` did not print the expected JSON: {message}")]
    Output {
        /// The command line.
        command: String,
        /// What was wrong.
        message: String,
    },
}

/// What identifies one build of the binary.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Stamp {
            path: path.to_path_buf(),
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// The package's binary target and the target directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryTarget {
    /// The binary's name (`crm-api`).
    pub name: String,
    /// Where `cargo build` puts it in the debug profile.
    pub debug_path: PathBuf,
}

/// Asks the app binary, building it first unless `--no-build`.
pub struct AppProbe {
    project: Project,
    runner: Arc<dyn Runner>,
    no_build: bool,
    /// Variables added to the binary's environment (Dock's settings).
    env: Vec<(String, String)>,
    answers: Mutex<HashMap<String, (Stamp, Value)>>,
    target: Mutex<Option<BinaryTarget>>,
}

impl std::fmt::Debug for AppProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppProbe")
            .field("project", &self.project)
            .field("no_build", &self.no_build)
            .finish_non_exhaustive()
    }
}

/// The environment the app binary runs with, on top of the inherited one:
/// no migrations at boot, and only errors logged, so asking never changes
/// the database and adds nothing to the log file but errors.
pub const PROBE_ENV: [(&str, &str); 2] = [
    ("APP_DATABASE__MIGRATE_ON_BOOT", "false"),
    ("APP_LOG__LEVEL", "error"),
];

impl AppProbe {
    /// A probe of `project`.
    pub fn new(
        project: Project,
        runner: Arc<dyn Runner>,
        no_build: bool,
        env: Vec<(String, String)>,
    ) -> AppProbe {
        AppProbe {
            project,
            runner,
            no_build,
            env,
            answers: Mutex::new(HashMap::new()),
            target: Mutex::new(None),
        }
    }

    fn cargo(&self, args: &[&str], timeout: Duration) -> Result<ProcessOutput, AppError> {
        let call = ProcessCall {
            program: PathBuf::from("cargo"),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            dir: self.project.root.clone(),
            env: Vec::new(),
        };
        self.runner.run(&call, timeout).map_err(|e| {
            AppError::Cargo(format!(
                "cannot run cargo (is it installed and on PATH?): {e}"
            ))
        })
    }

    fn manifest(&self) -> String {
        self.project.root.join("Cargo.toml").display().to_string()
    }

    /// The package's binary (from `cargo metadata --no-deps`): the one
    /// named after the package, else `default-run`, else the first.
    pub fn binary_target(&self) -> Result<BinaryTarget, AppError> {
        if let Some(target) = self.target.lock().expect("target").clone() {
            return Ok(target);
        }
        let manifest = self.manifest();
        let output = self.cargo(
            &[
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                &manifest,
            ],
            RUN_TIMEOUT,
        )?;
        if !output.success {
            return Err(AppError::Cargo(format!(
                "cargo metadata failed:\n\n{}",
                output.stderr.trim()
            )));
        }
        let metadata: Value = serde_json::from_str(&output.stdout)
            .map_err(|e| AppError::Cargo(format!("cargo metadata printed invalid JSON: {e}")))?;
        let package = metadata["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["name"] == self.project.package.as_str())
            .ok_or_else(|| {
                AppError::Cargo(format!(
                    "cargo metadata doesn't list the package {}",
                    self.project.package
                ))
            })?;
        let bins: Vec<&str> = package["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                t["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|k| k == "bin"))
            })
            .filter_map(|t| t["name"].as_str())
            .collect();
        let name = bins
            .iter()
            .find(|b| **b == self.project.package)
            .or_else(|| {
                package["default_run"]
                    .as_str()
                    .and_then(|run| bins.iter().find(|b| **b == run))
            })
            .or_else(|| bins.first())
            .ok_or_else(|| {
                AppError::Cargo(format!(
                    "the package {} has no binary, so there is no app to ask",
                    self.project.package
                ))
            })?
            .to_string();
        let target_dir = metadata["target_directory"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.project.root.join("target"));
        let target = BinaryTarget {
            debug_path: target_dir
                .join("debug")
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
            name,
        };
        *self.target.lock().expect("target") = Some(target.clone());
        Ok(target)
    }

    /// The app binary: built now (unless `--no-build`), or the last build.
    pub fn binary(&self) -> Result<PathBuf, AppError> {
        let target = self.binary_target()?;
        if self.no_build {
            return if target.debug_path.is_file() {
                Ok(target.debug_path)
            } else {
                Err(AppError::NotBuilt {
                    path: target.debug_path,
                })
            };
        }
        let manifest = self.manifest();
        let output = self.cargo(
            &[
                "build",
                "--message-format=json",
                "--manifest-path",
                &manifest,
                "--bin",
                &target.name,
            ],
            BUILD_TIMEOUT,
        )?;
        parse_build(&output, &target.name).map(|path| path.unwrap_or(target.debug_path))
    }

    /// Runs the binary with `args` and returns the JSON value it printed
    /// that `expect` accepts; cached while the binary is unchanged.
    fn ask(&self, args: &[&str], expect: fn(&Value) -> bool) -> Result<Value, AppError> {
        let binary = self.binary()?;
        let key = args.join(" ");
        let stamp = Stamp::of(&binary);
        if let Some(stamp) = &stamp
            && let Some((cached, value)) = self.answers.lock().expect("answers").get(&key)
            && cached == stamp
        {
            return Ok(value.clone());
        }
        let mut env = self.env.clone();
        env.extend(
            PROBE_ENV
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        let call = ProcessCall {
            program: binary.clone(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            dir: self.project.root.clone(),
            env,
        };
        let command = format!(
            "{} {key}",
            self.binary_target().map(|t| t.name).unwrap_or_default()
        );
        let output = self
            .runner
            .run(&call, RUN_TIMEOUT)
            .map_err(|e| AppError::Run {
                command: command.clone(),
                output: e.to_string(),
            })?;
        if !output.success {
            let printed = if output.stderr.trim().is_empty() {
                output.stdout.trim()
            } else {
                output.stderr.trim()
            };
            return Err(AppError::Run {
                command,
                output: printed.to_owned(),
            });
        }
        let value = find_json(&output.stdout, expect).ok_or_else(|| AppError::Output {
            command: command.clone(),
            message: format!(
                "got:\n{}",
                output.stdout.chars().take(2000).collect::<String>()
            ),
        })?;
        if let Some(stamp) = stamp {
            self.answers
                .lock()
                .expect("answers")
                .insert(key, (stamp, value.clone()));
        }
        Ok(value)
    }

    /// `introspect --json`: the module graph, providers and commands.
    pub fn introspect(&self) -> Result<Value, AppError> {
        self.ask(&["introspect", "--json"], |v| {
            v.get("modules").is_some_and(Value::is_array)
        })
    }

    /// `routes --json`: the route table (the app boots without serving).
    pub fn routes(&self) -> Result<Value, AppError> {
        self.ask(&["routes", "--json"], Value::is_array)
    }

    /// `migrate:status --json`: every registered migration with its state.
    pub fn migration_status(&self) -> Result<Value, AppError> {
        self.ask(&["migrate:status", "--json"], Value::is_array)
    }

    /// `rustc --version` in the app's directory (its toolchain file applies).
    pub fn rustc_version(&self) -> Option<String> {
        let call = ProcessCall {
            program: PathBuf::from("rustc"),
            args: vec!["--version".to_owned()],
            dir: self.project.root.clone(),
            env: Vec::new(),
        };
        let output = self.runner.run(&call, RUN_TIMEOUT).ok()?;
        output.success.then(|| output.stdout.trim().to_owned())
    }
}

/// The binary's path from `cargo build --message-format=json` output, or
/// the compiler's first error.
fn parse_build(output: &ProcessOutput, bin: &str) -> Result<Option<PathBuf>, AppError> {
    let mut first_error: Option<String> = None;
    let mut executable: Option<PathBuf> = None;
    let mut finished: Option<bool> = None;
    for line in output.stdout.lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match message["reason"].as_str() {
            Some("compiler-message")
                if first_error.is_none() && message["message"]["level"] == "error" =>
            {
                first_error = message["message"]["rendered"]
                    .as_str()
                    .or_else(|| message["message"]["message"].as_str())
                    .map(|s| s.trim_end().to_owned());
            }
            Some("compiler-artifact")
                if message["target"]["name"] == bin
                    && message["target"]["kind"]
                        .as_array()
                        .is_some_and(|k| k.iter().any(|k| k == "bin")) =>
            {
                executable = message["executable"].as_str().map(PathBuf::from);
            }
            Some("build-finished") => finished = message["success"].as_bool(),
            _ => {}
        }
    }
    if finished == Some(true) || (finished.is_none() && output.success) {
        return Ok(executable);
    }
    let first_error = first_error.unwrap_or_else(|| {
        let lines: Vec<&str> = output
            .stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        lines[lines.len().saturating_sub(20)..].join("\n")
    });
    Err(AppError::BuildFailed { first_error })
}

/// The first JSON value in `stdout` that `expect` accepts, skipping log
/// lines printed around it.
fn find_json(stdout: &str, expect: fn(&Value) -> bool) -> Option<Value> {
    let mut offset = 0;
    for line in stdout.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            let start = offset + (line.len() - trimmed.len());
            let mut values =
                serde_json::Deserializer::from_str(&stdout[start..]).into_iter::<Value>();
            if let Some(Ok(value)) = values.next()
                && expect(&value)
            {
                return Some(value);
            }
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn ok(stdout: &str) -> ProcessOutput {
        ProcessOutput {
            success: true,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    fn metadata(target_dir: &Path) -> String {
        serde_json::json!({
            "packages": [{
                "name": "crm",
                "default_run": null,
                "targets": [
                    {"name": "crm", "kind": ["lib"]},
                    {"name": "seed", "kind": ["bin"]},
                    {"name": "crm", "kind": ["bin"]}
                ]
            }],
            "target_directory": target_dir,
        })
        .to_string()
    }

    fn project(root: &Path) -> Project {
        Project {
            root: root.to_path_buf(),
            package: "crm".to_owned(),
            lockfile: None,
        }
    }

    fn build_output(executable: &Path) -> String {
        [
            serde_json::json!({"reason": "compiler-artifact", "target": {"name": "crm", "kind": ["lib"]}, "executable": null}),
            serde_json::json!({"reason": "compiler-artifact", "target": {"name": "crm", "kind": ["bin"]}, "executable": executable, "fresh": true}),
            serde_json::json!({"reason": "build-finished", "success": true}),
        ]
        .map(|v| v.to_string())
        .join("\n")
    }

    #[test]
    fn the_binary_is_built_and_its_answers_are_cached() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let binary = dir.path().join("crm.exe");
        std::fs::write(&binary, "binary").unwrap();
        let runner = FakeRunner::new();
        runner
            .on("cargo", &["metadata"], ok(&metadata(dir.path())))
            .on("cargo", &["build"], ok(&build_output(&binary)))
            .on(
                "crm",
                &["introspect", "--json"],
                ok("2026-10-27T09:00:00Z  INFO boot noise\n{\"navree\":\"0.1.0\",\"modules\":[]}\n"),
            );
        let probe = AppProbe::new(
            project(dir.path()),
            runner.clone(),
            false,
            vec![("APP_X".into(), "1".into())],
        );
        let answer = probe.introspect().unwrap();
        assert_eq!(answer["navree"], "0.1.0");
        let again = probe.introspect().unwrap();
        assert_eq!(again, answer);

        let calls = runner.calls();
        let runs: Vec<&ProcessCall> = calls.iter().filter(|c| c.program == binary).collect();
        assert_eq!(runs.len(), 1, "the unchanged binary is not asked twice");
        assert_eq!(runs[0].dir, dir.path());
        assert!(runs[0].env.contains(&("APP_X".to_owned(), "1".to_owned())));
        assert!(runs[0].env.contains(&(
            "APP_DATABASE__MIGRATE_ON_BOOT".to_owned(),
            "false".to_owned()
        )));
        let build = calls
            .iter()
            .find(|c| c.args.first().map(String::as_str) == Some("build"))
            .unwrap();
        assert!(
            build
                .args
                .ends_with(&["--bin".to_owned(), "crm".to_owned()]),
            "{:?}",
            build.args
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.args.first().map(String::as_str) == Some("metadata"))
                .count(),
            1,
            "cargo metadata runs once"
        );
    }

    #[test]
    fn a_failed_build_returns_the_first_compiler_error() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let runner = FakeRunner::new();
        let messages = [
            serde_json::json!({"reason": "compiler-message", "message": {"level": "warning", "rendered": "warning: unused"}}),
            serde_json::json!({"reason": "compiler-message", "message": {"level": "error", "rendered": "error[E0425]: cannot find value `deal` in this scope\n --> src/deals.rs:4:5\n"}}),
            serde_json::json!({"reason": "compiler-message", "message": {"level": "error", "rendered": "error: aborting"}}),
            serde_json::json!({"reason": "build-finished", "success": false}),
        ]
        .map(|v| v.to_string())
        .join("\n");
        runner
            .on("cargo", &["metadata"], ok(&metadata(dir.path())))
            .on(
                "cargo",
                &["build"],
                ProcessOutput {
                    success: false,
                    stdout: messages,
                    stderr: String::new(),
                },
            );
        let probe = AppProbe::new(project(dir.path()), runner, false, Vec::new());
        let error = probe.routes().unwrap_err();
        assert_eq!(
            error,
            AppError::BuildFailed {
                first_error:
                    "error[E0425]: cannot find value `deal` in this scope\n --> src/deals.rs:4:5"
                        .to_owned()
            }
        );
        assert!(error.to_string().starts_with("The app doesn't build."));
    }

    #[test]
    fn no_build_uses_the_last_binary_or_says_there_is_none() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let runner = FakeRunner::new();
        runner.on("cargo", &["metadata"], ok(&metadata(dir.path())));
        let probe = AppProbe::new(project(dir.path()), runner.clone(), true, Vec::new());
        let expected = dir
            .path()
            .join("debug")
            .join(format!("crm{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(
            probe.binary(),
            Err(AppError::NotBuilt {
                path: expected.clone()
            })
        );
        std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
        std::fs::write(&expected, "binary").unwrap();
        assert_eq!(probe.binary(), Ok(expected));
        assert!(
            runner
                .calls()
                .iter()
                .all(|c| c.args.first().map(String::as_str) != Some("build"))
        );
    }

    #[test]
    fn a_failing_command_reports_what_it_printed() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let binary = dir.path().join("crm");
        std::fs::write(&binary, "binary").unwrap();
        let runner = FakeRunner::new();
        runner
            .on("cargo", &["metadata"], ok(&metadata(dir.path())))
            .on("cargo", &["build"], ok(&build_output(&binary)))
            .on(
                "crm",
                &["routes"],
                ProcessOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: "Error: cannot connect to the database".to_owned(),
                },
            );
        let probe = AppProbe::new(project(dir.path()), runner, false, Vec::new());
        let error = probe.routes().unwrap_err();
        assert!(
            matches!(&error, AppError::Run { output, .. } if output.contains("cannot connect")),
            "{error}"
        );
    }

    #[test]
    fn json_is_found_among_log_lines() {
        let printed =
            "{\"timestamp\":\"x\",\"level\":\"INFO\"}\n[\n  {\"method\": \"GET\"}\n]\nlater\n";
        assert_eq!(
            find_json(printed, Value::is_array),
            Some(serde_json::json!([{"method": "GET"}]))
        );
        assert_eq!(find_json("no json", Value::is_array), None);
    }

    #[test]
    fn the_package_needs_a_binary() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let runner = FakeRunner::new();
        runner.on(
            "cargo",
            &["metadata"],
            ok(&serde_json::json!({"packages": [{"name": "crm", "targets": [{"name": "crm", "kind": ["lib"]}]}]}).to_string()),
        );
        let probe = AppProbe::new(project(dir.path()), runner, false, Vec::new());
        assert!(
            matches!(probe.binary_target(), Err(AppError::Cargo(m)) if m.contains("no binary"))
        );
    }
}
