//! Running a parsed command line.

use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::time::Duration;

use anyhow::Context;

use crate::cli::{Cli, Command};
use crate::clock::Timestamp;
use crate::dev::{self, DevOptions};
use crate::dock;
use crate::forward::forward;
use crate::make::{MakeOptions, MakeReport, make_item, make_module};
use crate::names::{AppName, Name};
use crate::new_app::{NavreeSource, NewApp, create_app};
use crate::openapi_client;
use crate::project::project_root;

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
        Command::Dock(command) => dock::run(command, &app_root(cwd)?),
        Command::DockAdd(args) => dock::add(&args.service, &app_root(cwd)?, &mut std::io::stdout()),
        Command::DockRemove(args) => dock::remove(
            &args.service,
            args.volumes,
            &app_root(cwd)?,
            &mut std::io::stdout(),
        ),
        Command::OpenapiClient(args) => {
            let root = app_root(cwd)?;
            let plan = openapi_client::plan(&args.out, args.input.as_deref());
            std::fs::create_dir_all(root.join(&args.out))
                .with_context(|| format!("cannot create {}", args.out.display()))?;
            let env = dock::app_env(&root)?;
            for step in &plan.steps {
                println!("  run      {step}");
                let status = step
                    .clone()
                    .with_env(env.clone())
                    .run(&root)
                    .with_context(|| {
                        format!("cannot run {} (is it installed and on PATH?)", step.program)
                    })?;
                if !status.success() {
                    return Ok(status
                        .code()
                        .and_then(|code| u8::try_from(code).ok())
                        .map_or(ExitCode::FAILURE, ExitCode::from));
                }
            }
            if args.input.is_none() {
                println!("  wrote    {}", shown(&root, &root.join(&plan.document)));
            }
            println!("  wrote    {}", shown(&root, &root.join(&plan.schema)));
            println!(
                "  next     createNavreeClient<paths>() from @navree/client, with `import type {{ paths }} from \"./schema\"`"
            );
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
