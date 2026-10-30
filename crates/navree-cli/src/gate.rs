//! `navree gate`: every check a change must pass before it is pushed, in
//! one command: `cargo fmt --check`, clippy with warnings as errors, the
//! tests, `navree check`, and the front end's typecheck, tests and build
//! (`web/`, when it has a `package.json`). Steps run one after another under
//! a machine-wide lock, so two gates never compete for the machine, and each
//! step's time is reported.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::Value;

use crate::check::{CheckOptions, run_check};
use crate::dev::{node_program, package_manager};
use crate::forward::{Invocation, Runner};

/// Every step name, in the order the steps run.
pub const STEP_NAMES: [&str; 7] = [
    "fmt",
    "clippy",
    "test",
    "check",
    "web:typecheck",
    "web:test",
    "web:build",
];

/// What `navree gate` runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateOptions {
    /// Only these steps (names, or `web` for every `web:` step).
    pub only: Vec<String>,
    /// Leave these steps out (names, or `web`).
    pub skip: Vec<String>,
    /// Run every step even after one fails.
    pub keep_going: bool,
    /// The machine lock file; `None` runs without the lock.
    pub lock: Option<PathBuf>,
    /// The front end's directory (default `web/` in the app).
    pub web: Option<PathBuf>,
    /// Environment variables for the app (its Dock settings).
    pub env: Vec<(String, String)>,
}

/// The lock file every gate on this machine (for this user) shares:
/// `navree-gate.lock` in the temporary directory.
pub fn default_lock_path() -> PathBuf {
    std::env::temp_dir().join("navree-gate.lock")
}

/// What a step runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// A program in a directory.
    Program(Invocation, PathBuf),
    /// `navree check`.
    Check,
}

/// One step of the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// `fmt`, `clippy`, `test`, `check`, `web:typecheck`, `web:test` or
    /// `web:build`.
    pub name: &'static str,
    /// What it runs.
    pub kind: StepKind,
}

impl Step {
    /// The command line, for the report.
    pub fn command_line(&self) -> String {
        match &self.kind {
            StepKind::Program(invocation, _) => invocation.to_string(),
            StepKind::Check => "navree check".to_owned(),
        }
    }
}

/// Whether `selector` (a step name, or `web` for every `web:` step) picks
/// step `name`.
fn selects(selector: &str, name: &str) -> bool {
    selector == name
        || name
            .strip_prefix(selector)
            .is_some_and(|rest| rest.starts_with(':'))
}

/// The steps for the app in `root`, after `only` and `skip`.
///
/// # Errors
///
/// When `only` or `skip` names no step, or `package.json` cannot be read.
pub fn steps(root: &Path, options: &GateOptions) -> anyhow::Result<Vec<Step>> {
    for selector in options.only.iter().chain(&options.skip) {
        if !STEP_NAMES.iter().any(|name| selects(selector, name)) {
            anyhow::bail!(
                "unknown gate step `{selector}`; the steps are {} (`web` selects every web step)",
                STEP_NAMES.join(", ")
            );
        }
    }
    let workspace = std::fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|manifest| manifest.parse::<toml::Table>().ok())
        .is_some_and(|manifest| manifest.contains_key("workspace"));
    let cargo = |args: &[&str]| {
        let mut all: Vec<String> = Vec::new();
        for arg in args {
            all.push((*arg).to_owned());
            if workspace && (*arg == "clippy" || *arg == "test") {
                all.push("--workspace".to_owned());
            }
        }
        StepKind::Program(
            Invocation::new("cargo", all).with_env(options.env.clone()),
            root.to_path_buf(),
        )
    };
    let mut steps = vec![
        Step {
            name: "fmt",
            kind: cargo(&["fmt", "--all", "--", "--check"]),
        },
        Step {
            name: "clippy",
            kind: cargo(&["clippy", "--all-targets", "--", "-D", "warnings"]),
        },
        Step {
            name: "test",
            kind: cargo(&["test"]),
        },
        Step {
            name: "check",
            kind: StepKind::Check,
        },
    ];
    let web = options.web.clone().unwrap_or_else(|| root.join("web"));
    let web = if web.is_absolute() {
        web
    } else {
        root.join(web)
    };
    steps.extend(web_steps(&web)?);
    Ok(steps
        .into_iter()
        .filter(|step| {
            (options.only.is_empty() || options.only.iter().any(|s| selects(s, step.name)))
                && !options.skip.iter().any(|s| selects(s, step.name))
        })
        .collect())
}

/// The front end's steps: its `typecheck`, `test` and `build` scripts, with
/// `tsc --noEmit` for a TypeScript project without a `typecheck` script.
fn web_steps(web: &Path) -> anyhow::Result<Vec<Step>> {
    let manifest = web.join("package.json");
    if !manifest.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&manifest)
        .with_context(|| format!("cannot read {}", manifest.display()))?;
    let package: Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", manifest.display()))?;
    let has_script = |name: &str| package["scripts"].get(name).is_some_and(Value::is_string);
    let manager = package_manager(web);
    let program = |args: &[&str]| {
        StepKind::Program(
            Invocation::new(node_program(manager), args.iter().copied()),
            web.to_path_buf(),
        )
    };
    let mut steps = Vec::new();
    if has_script("typecheck") {
        steps.push(Step {
            name: "web:typecheck",
            kind: program(&["run", "typecheck"]),
        });
    } else if web.join("tsconfig.json").is_file() {
        let exec: &[&str] = match manager {
            "npm" => &["exec", "--", "tsc", "--noEmit"],
            _ => &["exec", "tsc", "--noEmit"],
        };
        steps.push(Step {
            name: "web:typecheck",
            kind: program(exec),
        });
    }
    if has_script("test") {
        steps.push(Step {
            name: "web:test",
            kind: program(&["run", "test"]),
        });
    }
    if has_script("build") {
        steps.push(Step {
            name: "web:build",
            kind: program(&["run", "build"]),
        });
    }
    Ok(steps)
}

/// How a step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// It succeeded.
    Passed,
    /// It failed.
    Failed,
    /// An earlier step failed, so it did not run.
    NotRun,
}

/// One step's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    /// The step.
    pub step: Step,
    /// How it ended.
    pub status: StepStatus,
    /// How long it took (`None` when it did not run).
    pub elapsed: Option<Duration>,
}

/// The gate's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReport {
    /// Every step, in order.
    pub results: Vec<StepResult>,
    /// The whole run.
    pub elapsed: Duration,
}

impl GateReport {
    /// Whether every step that ran passed.
    pub fn passed(&self) -> bool {
        self.results.iter().all(|r| r.status != StepStatus::Failed)
    }

    /// The report as text: one line per step with its time, then the verdict.
    pub fn render(&self) -> String {
        let width = self
            .results
            .iter()
            .map(|r| r.step.name.len())
            .max()
            .unwrap_or(0);
        let mut out = String::from("Gate\n");
        for result in &self.results {
            let (label, time) = match (result.status, result.elapsed) {
                (StepStatus::Passed, Some(t)) => ("ok", duration(t)),
                (StepStatus::Failed, Some(t)) => ("FAIL", duration(t)),
                _ => ("-", String::new()),
            };
            out.push_str(&format!(
                "  {label:<6}{:<width$}  {time:>7}  {}{}\n",
                result.step.name,
                result.step.command_line(),
                if result.status == StepStatus::NotRun {
                    "  (not run)"
                } else {
                    ""
                }
            ));
        }
        let failed: Vec<&str> = self
            .results
            .iter()
            .filter(|r| r.status == StepStatus::Failed)
            .map(|r| r.step.name)
            .collect();
        if failed.is_empty() {
            out.push_str(&format!("gate passed in {}\n", duration(self.elapsed)));
        } else {
            out.push_str(&format!(
                "gate failed in {}: {}\n",
                duration(self.elapsed),
                failed.join(", ")
            ));
        }
        out
    }
}

/// `0.4s`, `38.2s`, `1m 42s`.
pub fn duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        let whole = d.as_secs();
        format!("{}m {:02}s", whole / 60, whole % 60)
    }
}

/// The held machine lock; released when dropped.
#[derive(Debug)]
pub struct GateLock {
    _file: File,
    owner: PathBuf,
}

impl Drop for GateLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.owner);
    }
}

/// The file naming the gate that holds the lock (read while waiting; the
/// lock file itself cannot be read while locked on Windows).
fn owner_path(lock: &Path) -> PathBuf {
    let mut name = lock.as_os_str().to_owned();
    name.push(".owner");
    PathBuf::from(name)
}

/// Takes the lock at `path` if it is free.
///
/// # Errors
///
/// When the lock file cannot be opened or locked for another reason than
/// another holder.
pub fn try_lock(path: &Path, root: &Path) -> anyhow::Result<Option<GateLock>> {
    let file = open_lock(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(held(file, path, root))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => {
            Err(error).with_context(|| format!("cannot lock {}", path.display()))
        }
    }
}

/// Takes the lock at `path`, waiting (and saying so on `out`) while another
/// gate holds it.
///
/// # Errors
///
/// When the lock file cannot be opened or locked.
pub fn lock(path: &Path, root: &Path, out: &mut dyn Write) -> anyhow::Result<GateLock> {
    if let Some(lock) = try_lock(path, root)? {
        return Ok(lock);
    }
    let holder = std::fs::read_to_string(owner_path(path)).unwrap_or_default();
    writeln!(
        out,
        "Waiting for another navree gate on this machine{} (lock: {})...",
        if holder.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", holder.trim())
        },
        path.display()
    )?;
    let file = open_lock(path)?;
    file.lock()
        .with_context(|| format!("cannot lock {}", path.display()))?;
    Ok(held(file, path, root))
}

fn open_lock(path: &Path) -> anyhow::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("cannot open the gate lock {}", path.display()))
}

fn held(file: File, path: &Path, root: &Path) -> GateLock {
    let owner = owner_path(path);
    let _ = std::fs::write(
        &owner,
        format!("process {} in {}", std::process::id(), root.display()),
    );
    GateLock { _file: file, owner }
}

/// Runs the gate for the app in `root`: takes the lock, runs the steps
/// (stopping at the first failure unless `keep_going`) and writes progress
/// and the report to `out`.
///
/// # Errors
///
/// When the steps cannot be planned, the lock cannot be taken or a program
/// cannot be started.
pub fn run_gate(
    root: &Path,
    options: &GateOptions,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> anyhow::Result<GateReport> {
    let steps = steps(root, options)?;
    let _lock = match &options.lock {
        Some(path) => Some(lock(path, root, out)?),
        None => None,
    };
    let started = Instant::now();
    let mut results = Vec::new();
    let mut stopped = false;
    let total = steps.len();
    for (index, step) in steps.into_iter().enumerate() {
        if stopped {
            results.push(StepResult {
                step,
                status: StepStatus::NotRun,
                elapsed: None,
            });
            continue;
        }
        writeln!(
            out,
            "[{}/{total}] {}: {}",
            index + 1,
            step.name,
            step.command_line()
        )?;
        out.flush()?;
        let begun = Instant::now();
        let passed = match &step.kind {
            StepKind::Program(invocation, dir) => runner
                .run(invocation, dir)
                .with_context(|| format!("cannot run `{invocation}`"))?,
            StepKind::Check => run_check(
                root,
                &CheckOptions {
                    env: options.env.clone(),
                    ..CheckOptions::default()
                },
                runner,
                out,
            )?,
        };
        let status = if passed {
            StepStatus::Passed
        } else {
            stopped = !options.keep_going;
            StepStatus::Failed
        };
        results.push(StepResult {
            step,
            status,
            elapsed: Some(begun.elapsed()),
        });
    }
    let report = GateReport {
        results,
        elapsed: started.elapsed(),
    };
    write!(out, "{}", report.render())?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::tests::{FakeRunner, HELP};

    fn app(manifest: &str) -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
        dir
    }

    fn names(steps: &[Step]) -> Vec<&'static str> {
        steps.iter().map(|s| s.name).collect()
    }

    const PACKAGE: &str = "[package]\nname = \"blog\"\nversion = \"0.1.0\"\n\n[workspace]\n";

    #[test]
    fn the_rust_steps_cover_the_whole_workspace() {
        let dir = app(PACKAGE);
        let steps = steps(dir.path(), &GateOptions::default()).unwrap();
        assert_eq!(names(&steps), ["fmt", "clippy", "test", "check"]);
        let lines: Vec<String> = steps.iter().map(Step::command_line).collect();
        assert_eq!(
            lines,
            [
                "cargo fmt --all -- --check",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "cargo test --workspace",
                "navree check",
            ]
        );
        // A package inside another workspace tests itself only.
        let member = app("[package]\nname = \"blog\"\n");
        let steps = super::steps(member.path(), &GateOptions::default()).unwrap();
        assert_eq!(steps[2].command_line(), "cargo test");
    }

    #[test]
    fn the_front_end_steps_follow_its_scripts() {
        let dir = app(PACKAGE);
        let web = dir.path().join("web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("package.json"),
            r#"{"scripts": {"typecheck": "tsc --noEmit", "test": "vitest run", "build": "next build", "dev": "next dev"}}"#,
        )
        .unwrap();
        std::fs::write(web.join("pnpm-lock.yaml"), "").unwrap();
        let steps = steps(dir.path(), &GateOptions::default()).unwrap();
        assert_eq!(
            names(&steps),
            [
                "fmt",
                "clippy",
                "test",
                "check",
                "web:typecheck",
                "web:test",
                "web:build"
            ]
        );
        let pnpm = node_program("pnpm");
        assert_eq!(steps[4].command_line(), format!("{pnpm} run typecheck"));
        assert_eq!(steps[6].command_line(), format!("{pnpm} run build"));
        let StepKind::Program(_, dir_of) = &steps[5].kind else {
            panic!("a program");
        };
        assert_eq!(dir_of, &web);

        // No typecheck script: tsc through the package manager.
        std::fs::write(web.join("package.json"), r#"{"scripts": {}}"#).unwrap();
        std::fs::remove_file(web.join("pnpm-lock.yaml")).unwrap();
        std::fs::write(web.join("tsconfig.json"), "{}").unwrap();
        let steps = super::steps(dir.path(), &GateOptions::default()).unwrap();
        assert_eq!(names(&steps)[4..], ["web:typecheck"]);
        assert_eq!(
            steps[4].command_line(),
            format!("{} exec -- tsc --noEmit", node_program("npm"))
        );
    }

    #[test]
    fn only_and_skip_select_steps() {
        let dir = app(PACKAGE);
        let web = dir.path().join("frontend");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("package.json"),
            r#"{"scripts": {"test": "vitest", "build": "vite build"}}"#,
        )
        .unwrap();
        let options = GateOptions {
            web: Some(PathBuf::from("frontend")),
            skip: vec!["clippy".into(), "web:test".into()],
            ..GateOptions::default()
        };
        assert_eq!(
            names(&steps(dir.path(), &options).unwrap()),
            ["fmt", "test", "check", "web:build"]
        );
        let options = GateOptions {
            web: Some(PathBuf::from("frontend")),
            only: vec!["web".into(), "fmt".into()],
            ..GateOptions::default()
        };
        assert_eq!(
            names(&steps(dir.path(), &options).unwrap()),
            ["fmt", "web:test", "web:build"]
        );
        let error = steps(
            dir.path(),
            &GateOptions {
                only: vec!["lint".into()],
                ..GateOptions::default()
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().starts_with("unknown gate step `lint`"),
            "{error}"
        );
    }

    fn gate(options: &GateOptions, runner: &mut FakeRunner) -> (GateReport, String) {
        let dir = app(PACKAGE);
        let mut out = Vec::new();
        let report = run_gate(dir.path(), options, runner, &mut out).unwrap();
        (report, String::from_utf8(out).unwrap())
    }

    #[test]
    fn every_step_runs_in_order_with_its_time() {
        let mut runner = FakeRunner::default().answer("cargo run --quiet -- help", true, HELP);
        let (report, out) = gate(&GateOptions::default(), &mut runner);
        assert!(report.passed());
        assert_eq!(
            runner.ran,
            [
                "cargo fmt --all -- --check",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "cargo test --workspace",
                "cargo run --quiet -- check",
                "cargo run --quiet -- help",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .chain(std::iter::once(runner.ran[5].clone()))
            .collect::<Vec<_>>()
        );
        assert!(runner.ran[5].starts_with("cargo run --quiet -- conformity:check --root "));
        assert!(
            out.contains("[1/4] fmt: cargo fmt --all -- --check\n"),
            "{out}"
        );
        assert!(out.contains("\nGate\n  ok    fmt   "), "{out}");
        assert!(out.contains("s  navree check\n"), "{out}");
        assert!(out.contains("gate passed in "), "{out}");
        assert!(report.results.iter().all(|r| r.elapsed.is_some()));
    }

    #[test]
    fn a_failure_stops_the_gate_unless_told_to_keep_going() {
        let mut runner = FakeRunner::default().answer("cargo clippy", false, "");
        let (report, out) = gate(&GateOptions::default(), &mut runner);
        assert!(!report.passed());
        assert_eq!(runner.ran.len(), 2);
        let statuses: Vec<_> = report.results.iter().map(|r| r.status).collect();
        assert_eq!(
            statuses,
            [
                StepStatus::Passed,
                StepStatus::Failed,
                StepStatus::NotRun,
                StepStatus::NotRun
            ]
        );
        assert!(out.contains("cargo test --workspace  (not run)\n"), "{out}");
        assert!(out.contains("gate failed in "), "{out}");
        assert!(out.ends_with(": clippy\n"), "{out}");

        let mut runner = FakeRunner::default()
            .answer("cargo clippy", false, "")
            .answer("cargo run --quiet -- help", true, HELP);
        let options = GateOptions {
            keep_going: true,
            ..GateOptions::default()
        };
        let (report, _) = gate(&options, &mut runner);
        assert!(!report.passed());
        assert_eq!(runner.ran.len(), 6);
    }

    #[test]
    fn the_machine_lock_admits_one_gate_at_a_time() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let path = dir.path().join("locks").join("navree-gate.lock");
        let held = try_lock(&path, Path::new("app-one"))
            .unwrap()
            .expect("free");
        assert_eq!(
            std::fs::read_to_string(owner_path(&path)).unwrap(),
            format!("process {} in app-one", std::process::id())
        );
        assert!(try_lock(&path, Path::new("app-two")).unwrap().is_none());
        drop(held);
        assert!(!owner_path(&path).exists());
        let again = try_lock(&path, Path::new("app-two")).unwrap();
        assert!(again.is_some());
    }

    #[test]
    fn the_gate_takes_the_lock() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let path = dir.path().join("navree-gate.lock");
        let mut runner = FakeRunner::default();
        let options = GateOptions {
            only: vec!["fmt".into()],
            lock: Some(path.clone()),
            ..GateOptions::default()
        };
        let (report, _) = gate(&options, &mut runner);
        assert!(report.passed());
        // Released afterwards.
        assert!(try_lock(&path, Path::new("next")).unwrap().is_some());
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(Duration::from_millis(400)), "0.4s");
        assert_eq!(duration(Duration::from_millis(38_240)), "38.2s");
        assert_eq!(duration(Duration::from_secs(102)), "1m 42s");
    }
}
