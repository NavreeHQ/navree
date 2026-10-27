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
use crate::new_app::{NavreeSource, NewApp, create_app};
use crate::project::project_root;
use crate::resource::make_resource;

/// Runs `cli` with `cwd` as the working directory.
pub fn execute(cli: Cli, cwd: &Path) -> anyhow::Result<ExitCode> {
    match &cli.command {
        Command::New(args) => {
            let app = NewApp {
                name: AppName::parse(&args.name)?,
                ui: args.ui,
                navree: NavreeSource::resolve(args.navree_path.as_deref())?,
            };
            let root = create_app(cwd, &app, Timestamp::now())?;
            format_rust(&rust_files(&root));
            print_new_app(&app, &root);
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
            let root = app_root(cwd)?;
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
            let root = app_root(cwd)?;
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
            let root = app_root(cwd)?;
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
        Command::Dock(command) => dock::run(command, &app_root(cwd)?),
        Command::DockAdd(args) => dock::add(&args.service, &app_root(cwd)?, &mut std::io::stdout()),
        Command::DockRemove(args) => dock::remove(
            &args.service,
            args.volumes,
            &app_root(cwd)?,
            &mut std::io::stdout(),
        ),
        Command::Harbor(command) => {
            navree_harbor::run(command, &navree_harbor::Context::system(cwd.to_path_buf()))
        }
        Command::Mcp(args) => {
            navree_navigator::run_stdio(navree_navigator::NavigatorOptions {
                path: absolute(cwd, args.path.as_deref()),
                no_build: args.no_build,
                allow_remote_database: args.allow_remote_database,
                docs_offline: args.docs_offline,
            })?;
            Ok(ExitCode::SUCCESS)
        }
        Command::SkillsInstall(args) => {
            let dir = absolute(cwd, args.path.as_deref());
            let report = navree_navigator::skills::install(&dir)
                .with_context(|| format!("cannot install the skills into {}", dir.display()))?;
            print_skills(&dir, &report);
            Ok(ExitCode::SUCCESS)
        }
        Command::Forward(words) => {
            let Some((command, args)) = words.split_first() else {
                anyhow::bail!("no command given");
            };
            // The app's dock.toml services, when it has one.
            let env = match project_root(cwd) {
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

/// `path` resolved against `cwd`; `cwd` itself when none is given.
fn absolute(cwd: &Path, path: Option<&Path>) -> PathBuf {
    match path {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
        None => cwd.to_path_buf(),
    }
}

/// What `skills:install` did.
fn print_skills(dir: &Path, report: &navree_navigator::skills::InstallReport) {
    let targets: Vec<String> = report.targets.iter().map(|t| shown(dir, t)).collect();
    println!(
        "Installed {} Navree skills into {}.",
        report.installed.len(),
        targets.join(" and ")
    );
    for name in &report.removed {
        println!("  removed  {name} (not a skill of this Navree version)");
    }
    println!("Commit them with your code so every teammate's agent has them.");
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

/// Every `.rs` file under `root`'s `src/` and `tests/`.
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

fn print_new_app(app: &NewApp, root: &Path) {
    println!("Created {} in {}", app.name.package(), root.display());
    if let NavreeSource::Path(path) = &app.navree {
        println!("It depends on navree at {}.", path.display());
    }
    println!();
    println!("Next steps:");
    println!("  cd {}", app.name.package());
    println!("  navree key:generate     # then set APP_KEY in .env");
    println!("  navree dev              # http://127.0.0.1:8080/api/docs");
    if root.join("web").is_dir() {
        println!("  cd web && npm install # the front end; `navree dev` starts it");
    }
}
