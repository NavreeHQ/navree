//! Running a parsed command line.

use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::time::Duration;

use anyhow::Context;

use crate::check::{CheckOptions, run_check};
use crate::cli::{Cli, Command};
use crate::clock::Timestamp;
use crate::dev::{self, DevOptions};
use crate::dock;
use crate::forward::{ProcessRunner, forward};
use crate::gate::{GateOptions, default_lock_path, run_gate};
use crate::make::{MakeOptions, MakeReport, make_item, make_module};
use crate::names::{AppName, Name};
use crate::new_app::{NavreeSource, NewApp, create_app, created_message};
use crate::project::{project_root, run_root};
use crate::resource::make_resource;

/// Runs `cli` with `cwd` as the working directory.
pub fn execute(cli: Cli, cwd: &Path) -> anyhow::Result<ExitCode> {
    match &cli.command {
        Command::New(args) => {
            let app = NewApp {
                name: AppName::parse(&args.name)?,
                ui: args.ui,
                kit: args.kit,
                navree: NavreeSource::resolve(args.navree_path.as_deref())?,
            };
            let root = create_app(cwd, &app, Timestamp::now())?;
            format_rust(&rust_files(&root));
            print!("{}", created_message(&app, &root));
            Ok(ExitCode::SUCCESS)
        }
        Command::MakeModule(args) => {
            let name = Name::parse("module", &args.name)?;
            let root = app_root(cwd)?;
            let report = make_module(&root, &args.path, &name, args.crud, Timestamp::now())?;
            finish(&root, &report);
            Ok(ExitCode::SUCCESS)
        }
        Command::MakeResource(args) => {
            let module = Name::parse("module", &args.module)?;
            let root = app_root(cwd)?;
            let report = make_resource(&root, &args.path, &module, &args.entity)?;
            finish(&root, &report);
            Ok(ExitCode::SUCCESS)
        }
        Command::Dev(args) => {
            let root = run_root_of(cwd)?;
            dev::run(
                &root,
                &DevOptions {
                    debounce: Duration::from_millis(args.debounce_ms),
                    web: !args.no_web,
                    serve_args: args.serve_args.clone(),
                    env: dock::app_env(&root)?,
                },
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Check(args) => {
            let root = run_root_of(cwd)?;
            let options = CheckOptions {
                graph: args.graph,
                json: args.json,
                only: args.only.clone(),
                env: dock::app_env(&root)?,
            };
            let passed = run_check(&root, &options, &mut ProcessRunner, &mut std::io::stdout())?;
            Ok(exit_code(passed))
        }
        Command::Gate(args) => {
            let root = run_root_of(cwd)?;
            let options = GateOptions {
                only: args.only.clone(),
                skip: args.skip.clone(),
                keep_going: args.keep_going,
                lock: (!args.no_lock).then(default_lock_path),
                web: args.web.clone(),
                env: dock::app_env(&root)?,
            };
            let report = run_gate(&root, &options, &mut ProcessRunner, &mut std::io::stdout())?;
            Ok(exit_code(report.passed()))
        }
        Command::Dock(command) => dock::run(command, &run_root_of(cwd)?),
        Command::DockAdd(args) => {
            dock::add(&args.service, &run_root_of(cwd)?, &mut std::io::stdout())
        }
        Command::DockRemove(args) => dock::remove(
            &args.service,
            args.volumes,
            &run_root_of(cwd)?,
            &mut std::io::stdout(),
        ),
        Command::Harbor(command) => {
            navree_harbor::run(command, &navree_harbor::Context::system(cwd.to_path_buf()))
        }
        Command::Forward(words) => {
            let Some((command, args)) = words.split_first() else {
                anyhow::bail!("no command given");
            };
            // The app's dock.toml services, when it has one.
            let env = match run_root(cwd) {
                Some(root) => dock::app_env(&root)?,
                None => Vec::new(),
            };
            let status = forward(command, args)
                .with_env(env)
                .run(cwd)
                .context("cannot run cargo (is it installed and on PATH?)")?;
            Ok(status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .map_or(ExitCode::FAILURE, ExitCode::from))
        }
        other => {
            let (kind, args, event) = other
                .make_item()
                .expect("every other command is a make:<kind>");
            let module = Name::parse("module", &args.module)?;
            let name = Name::parse(kind.noun(), &args.name)?;
            let options = MakeOptions {
                event: event.map(|e| Name::parse("event", e)).transpose()?,
            };
            let root = app_root(cwd)?;
            let report = make_item(
                &root,
                &args.path,
                kind,
                &module,
                &name,
                &options,
                Timestamp::now(),
            )?;
            finish(&root, &report);
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Success or failure as an exit code.
fn exit_code(passed: bool) -> ExitCode {
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The app containing `cwd`.
fn app_root(cwd: &Path) -> anyhow::Result<PathBuf> {
    project_root(cwd).with_context(|| {
        format!(
            "{} is not inside a Rust package (no Cargo.toml with [package] here or above); run navree in your app",
            cwd.display()
        )
    })
}

/// Where `dev`, `dock`, `check`, `gate` and forwarded commands run for
/// `cwd`: the app, or a workspace whose `default-members` is the app
/// ([`run_root`]), as in the `micro` starter kit.
fn run_root_of(cwd: &Path) -> anyhow::Result<PathBuf> {
    run_root(cwd).with_context(|| {
        format!(
            "{} is not inside a Rust package (no Cargo.toml with [package], or a [workspace] with default-members, here or above); run navree in your app",
            cwd.display()
        )
    })
}

/// `path` relative to `root`, with `/` separators.
fn shown(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Formats the generated Rust files and prints what happened.
fn finish(root: &Path, report: &MakeReport) {
    let rust: Vec<PathBuf> = report
        .created
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .cloned()
        .collect();
    format_rust(&rust);
    for path in &report.created {
        println!("  created  {}", shown(root, path));
    }
    for path in &report.updated {
        println!("  updated  {}", shown(root, path));
    }
    for hint in &report.hints {
        println!("  next     {hint}");
    }
}

/// Every `.rs` file under `root`'s `src/`, `tests/` and `crates/`.
fn rust_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("src"), &mut out);
    walk(&root.join("tests"), &mut out);
    // The crates of a workspace app (the `micro` kit).
    walk(&root.join("crates"), &mut out);
    out
}

/// Runs `rustfmt` on `files` when it is installed, so generated code follows
/// the project's style whatever the names' lengths. Failures are reported,
/// a missing `rustfmt` is not.
fn format_rust(files: &[PathBuf]) {
    if files.is_empty() {
        return;
    }
    let result = std::process::Command::new("rustfmt")
        .args(["--edition", "2024"])
        .args(files)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    if let Ok(output) = result
        && !output.status.success()
    {
        eprintln!(
            "warning: rustfmt could not format the generated files:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
