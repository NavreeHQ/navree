//! Running a parsed command line.

use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::time::Duration;

use anyhow::Context;
use heck::ToSnakeCase;

use crate::browser;
use crate::check::{CheckOptions, run_check};
use crate::cli::{Cli, Command, MakeUiArgs};
use crate::clock::Timestamp;
use crate::dev::{self, DevOptions};
use crate::dock;
use crate::forward::{ProcessRunner, forward};
use crate::gate::{GateOptions, default_lock_path, run_gate};
use crate::make::{MakeKind, MakeOptions, MakeReport, make_item, make_model, make_module};
use crate::make_ui::{UiOptions, UiReport, default_module, make_ui_item};
use crate::names::{AppName, Name};
use crate::new_app::{
    NavreeSource, NewApp, Repository, Ui, create_app, created_message, init_repository,
    write_harbor_key,
};
use crate::openapi_client;
use crate::project::{project_root, run_root};
use crate::resource::make_resource;

/// Runs `cli` with `cwd` as the working directory.
pub fn execute(cli: Cli, cwd: &Path) -> anyhow::Result<ExitCode> {
    match &cli.command {
        Command::New(args) => {
            let app = NewApp {
                name: AppName::parse(&args.name)?,
                // A kit brings its own front end, or none; the minimal app
                // draws its screens with Navree UI unless told otherwise.
                ui: args.ui.unwrap_or(if args.kit.is_some() {
                    Ui::None
                } else {
                    Ui::default()
                }),
                kit: args.kit,
                database: args.database,
                navree: NavreeSource::resolve(args.navree_path.as_deref())?,
            };
            let root = create_app(cwd, &app, Timestamp::now())?;
            format_rust(&rust_files(&root));
            write_harbor_key(&root)?;
            let repository = init_repository(&root);
            print!("{}", created_message(&app, &root));
            if repository == Repository::Created {
                println!("\nInitialized a Git repository (nothing committed yet).");
            }
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
            let report = make_resource(&root, &args.path, &module, &args.model)?;
            finish(&root, &report);
            Ok(ExitCode::SUCCESS)
        }
        Command::MakePage(args) => make_ui(cwd, MakeKind::Page, &args.item, args.public),
        Command::MakeForm(args) => make_ui(cwd, MakeKind::Form, args, false),
        Command::MakeTable(args) => make_ui(cwd, MakeKind::Table, args, false),
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
        Command::Mcp(args) => {
            navree_navigator::run_stdio(navree_navigator::NavigatorOptions {
                path: absolute(cwd, args.path.as_deref()),
                no_build: args.no_build,
                allow_remote_database: args.allow_remote_database,
                allow_privileged_database_role: args.allow_privileged_database_role,
                database_url: args.database_url.clone(),
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
        Command::BrowserInstall(args) => {
            let options = browser::InstallOptions::for_this_machine(
                args.chrome_version.as_deref(),
                args.force,
            )?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("cannot start the download")?;
            runtime.block_on(browser::install(&options, &mut std::io::stdout()))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::OpenapiClient(args) => {
            // With `--input` the paths are the current directory's and no app
            // is needed; without it they are the app's, and the app exports
            // its document first.
            let root = if args.input.is_some() {
                cwd.to_path_buf()
            } else {
                run_root_of(cwd)?
            };
            let plan = openapi_client::plan(&args.out, args.input.as_deref());
            std::fs::create_dir_all(root.join(&args.out))
                .with_context(|| format!("cannot create {}", args.out.display()))?;
            if let Some(export) = &plan.export {
                let env = dock::app_env(&root)?;
                println!("  run      {export}");
                let status = export.clone().with_env(env).run(&root).with_context(|| {
                    format!(
                        "cannot run {} (is it installed and on PATH?)",
                        export.program
                    )
                })?;
                if !status.success() {
                    return Ok(status
                        .code()
                        .and_then(|code| u8::try_from(code).ok())
                        .map_or(ExitCode::FAILURE, ExitCode::from));
                }
            }
            let document = root.join(&plan.document);
            let size = std::fs::metadata(&document)
                .with_context(|| format!("cannot read {}", document.display()))?
                .len();
            anyhow::ensure!(
                size <= openapi_client::MAX_DOCUMENT_BYTES,
                "{} is larger than {} bytes",
                document.display(),
                openapi_client::MAX_DOCUMENT_BYTES
            );
            let text = std::fs::read_to_string(&document)
                .with_context(|| format!("cannot read {}", document.display()))?;
            let generated = openapi_client::generate(&text).with_context(|| {
                format!("cannot generate the schema from {}", document.display())
            })?;
            for warning in &generated.warnings {
                eprintln!("  warning  {warning}");
            }
            let schema = root.join(&plan.schema);
            std::fs::write(&schema, &generated.schema)
                .with_context(|| format!("cannot write {}", schema.display()))?;
            if args.input.is_none() {
                println!("  wrote    {}", shown(&root, &root.join(&plan.document)));
            }
            println!("  wrote    {}", shown(&root, &schema));
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
            let (kind, args, flags) = other
                .make_item()
                .expect("every other command is a make:<kind>");
            let module = Name::parse("module", &args.module)?;
            // Models (and what is named after them) may be given as types:
            // `make:model crm Deal`.
            let given = match kind {
                MakeKind::Model | MakeKind::Factory | MakeKind::Seeder => args.name.to_snake_case(),
                _ => args.name.clone(),
            };
            let name = Name::parse(kind.noun(), &given)?;
            let root = app_root(cwd)?;
            let report = if kind == MakeKind::Model {
                make_model(
                    &root,
                    &args.path,
                    &module,
                    &name,
                    flags.companions,
                    Timestamp::now(),
                )?
            } else {
                let options = MakeOptions {
                    event: flags.event.map(|e| Name::parse("event", e)).transpose()?,
                    model: flags
                        .model
                        .map(|m| Name::parse("model", &m.to_snake_case()))
                        .transpose()?,
                    companions: flags.companions,
                };
                make_item(
                    &root,
                    &args.path,
                    kind,
                    &module,
                    &name,
                    &options,
                    Timestamp::now(),
                )?
            };
            finish(&root, &report);
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// `make:page`, `make:form` or `make:table`: the name may be given as
/// `NewDeal`, `new_deal` or `new-deal`, and the module defaults to the app's
/// only one.
fn make_ui(
    cwd: &Path,
    kind: MakeKind,
    args: &MakeUiArgs,
    public: bool,
) -> anyhow::Result<ExitCode> {
    let root = app_root(cwd)?;
    let module = match &args.module {
        Some(module) => Name::parse("module", module)?,
        None => default_module(&root, &args.path)?,
    };
    let name = Name::parse(kind.noun(), &args.name.to_snake_case())?;
    let options = UiOptions {
        public,
        dry_run: args.dry_run,
        force: args.force,
    };
    let report = make_ui_item(&root, &args.path, kind, &module, &name, &options)?;
    finish_ui(&root, &report);
    Ok(ExitCode::SUCCESS)
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

/// Formats the generated Rust files and prints what happened, or with
/// `--dry-run` what would happen, and the files that would be written.
fn finish_ui(root: &Path, report: &UiReport) {
    let made = &report.made;
    if report.dry_run {
        for path in &made.created {
            println!("  would create     {}", shown(root, path));
        }
        for path in &report.overwritten {
            println!("  would replace    {}", shown(root, path));
        }
        for path in &made.updated {
            println!("  would update     {}", shown(root, path));
        }
        for hint in &made.hints {
            println!("  next     {hint}");
        }
        println!("\nNothing was written (--dry-run). The files would be:\n");
        print!("{}", report.files);
        return;
    }
    let rust: Vec<PathBuf> = made
        .created
        .iter()
        .chain(&report.overwritten)
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .cloned()
        .collect();
    format_rust(&rust);
    for path in &made.created {
        println!("  created  {}", shown(root, path));
    }
    for path in &report.overwritten {
        println!("  replaced {}", shown(root, path));
    }
    for path in &made.updated {
        println!("  updated  {}", shown(root, path));
    }
    for hint in &made.hints {
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
