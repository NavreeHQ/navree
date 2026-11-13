//! `cargo xtask release <command>` and `cargo xtask native-models`: see the
//! library's documentation.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use xtask::metadata::Workspace;
use xtask::publish::{Flags, Outcome};
use xtask::{check, native_models, order, prepare, publish, stage, time};

const USAGE: &str = "\
cargo xtask release <command>
cargo xtask native-models [--check]

native-models [--check]
    Writes the Kotlin and Swift models of Navree Native's protocol from
    crates/navree-native/schema/navree-native.schema.json; with --check,
    writes nothing and fails when a written model is stale.

Release commands:
  prepare <version> [--date YYYY-MM-DD]
      Sets <version> in the workspace, its exact internal requirements, the
      npm packages and Cargo.lock, and adds its CHANGELOG.md section.
  check
      Every version equal, every internal requirement exact, a publish order
      exists, the changelog has the version, Cargo.lock is current.
  order
      Prints the publish order, layer by layer.
  crates
      Prints each published crate and its folder, in publish order (one
      `name path` line each), for scripts.
  stage [--clean]
      Copies the skills, the docs and the UI catalog into navree-navigator
      (or removes the copies).
  publish [--registry <name>] [--index <url>] [--dry-run] [--package <name>]...
          [--limit <n>] [--per-token <n>] [--trusted-publishing]
          [--skip-verify] [--allow-dirty]
      Publishes every crate the registry does not have at the version, in
      dependency order. Run it again after a stop: it resumes.
";

/// The workspace root: this crate's parent folder.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is in the workspace root")
        .to_owned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["release", "prepare", rest @ ..] => cmd_prepare(rest),
        ["release", "check"] => cmd_check(),
        ["release", "order"] => cmd_order(),
        ["release", "crates"] => cmd_crates(),
        ["release", "stage"] => cmd_stage(false),
        ["release", "stage", "--clean"] => cmd_stage(true),
        ["release", "publish", rest @ ..] => cmd_publish(rest),
        ["native-models"] => cmd_native_models(false),
        ["native-models", "--check"] => cmd_native_models(true),
        ["help" | "--help" | "-h"] | ["release"] | ["release", "help" | "--help" | "-h"] | [] => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(format!("unknown command: {}\n\n{USAGE}", args.join(" "))),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_prepare(args: &[&str]) -> Result<ExitCode, String> {
    let mut version = None;
    let mut date = time::date_utc(SystemTime::now());
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--date" => date = value(&mut args, "--date")?.to_owned(),
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            given if version.is_none() => version = Some(given.trim_start_matches('v').to_owned()),
            extra => return Err(format!("unexpected argument {extra}")),
        }
    }
    let version = version.ok_or("release prepare needs a version: release prepare 1.2.3")?;
    let changed = prepare::prepare(&root(), &version, &date)?;
    println!("set {version} in:");
    for file in changed {
        println!("  {}", file.strip_prefix(root()).unwrap_or(&file).display());
    }
    println!(
        "edit the CHANGELOG.md section and the matching section of docs/prologue/release-notes.mdx, then run `cargo xtask release check`"
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_check() -> Result<ExitCode, String> {
    let report = check::run(&root())?;
    if report.problems.is_empty() {
        println!(
            "release {}: {} crates at {}, every internal requirement exact, publish order in {} layers",
            report.version, report.crates, report.version, report.layers
        );
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "release {}: {} problems",
            report.version,
            report.problems.len()
        );
        for problem in &report.problems {
            eprintln!("- {problem}");
        }
        Ok(ExitCode::FAILURE)
    }
}

fn cmd_order() -> Result<ExitCode, String> {
    let workspace = Workspace::load(&root())?;
    let publishable: Vec<String> = workspace.publishable().map(|p| p.name.clone()).collect();
    let layers = order::layers(&workspace, &publishable).map_err(|c| order::describe_cycles(&c))?;
    let mut n = 0;
    for (depth, layer) in layers.iter().enumerate() {
        println!("layer {}:", depth + 1);
        for name in layer {
            n += 1;
            println!("  {n:>3} {name}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_crates() -> Result<ExitCode, String> {
    let root = root();
    let workspace = Workspace::load(&root)?;
    let publishable: Vec<String> = workspace.publishable().map(|p| p.name.clone()).collect();
    for name in order::publish_order(&workspace, &publishable)? {
        let package = workspace.member(&name).expect("a member");
        let dir = package
            .dir()
            .strip_prefix(&workspace.root)
            .unwrap_or(package.dir());
        println!("{name} {}", dir.display().to_string().replace('\\', "/"));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_stage(clean: bool) -> Result<ExitCode, String> {
    if clean {
        stage::clean(&root())?;
        println!("removed the staged copies from {}", stage::NAVIGATOR);
    } else {
        let staged = stage::stage(&root())?;
        println!(
            "staged {} skill files, {} docs pages and the UI catalog in {}",
            staged.skill_files,
            staged.doc_pages,
            stage::NAVIGATOR
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_publish(args: &[&str]) -> Result<ExitCode, String> {
    let mut flags = Flags::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--registry" => flags.registry = Some(value(&mut args, "--registry")?.to_owned()),
            "--index" => flags.index = Some(value(&mut args, "--index")?.to_owned()),
            "--dry-run" => flags.dry_run = true,
            "--package" | "-p" => flags
                .packages
                .push(value(&mut args, "--package")?.to_owned()),
            "--limit" => flags.limit = Some(number(value(&mut args, "--limit")?)?),
            "--per-token" => flags.per_token = Some(number(value(&mut args, "--per-token")?)?),
            "--trusted-publishing" => flags.trusted_publishing = true,
            "--skip-verify" => flags.skip_verify = true,
            "--allow-dirty" => flags.allow_dirty = true,
            other => return Err(format!("unknown flag {other}")),
        }
    }
    match publish::run(&root(), &flags)? {
        Outcome::Done(report) => {
            if flags.dry_run {
                println!(
                    "dry run: {} on the registry, {} to publish",
                    report.already.len(),
                    report.remaining.len()
                );
            } else {
                println!(
                    "done: {} published now, {} on the registry before",
                    report.published.len(),
                    report.already.len()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Paused(report) => {
            println!(
                "paused at the limit: {} published now, {} left ({}); run the same command again to go on",
                report.published.len(),
                report.remaining.len(),
                report.remaining.join(", ")
            );
            // Distinct from a failure, so a workflow can loop on it.
            Ok(ExitCode::from(2))
        }
    }
}

fn cmd_native_models(check: bool) -> Result<ExitCode, String> {
    let root = root();
    let stale = native_models::run(&root, check)?;
    if !check {
        println!(
            "wrote {} and {} from {}",
            native_models::KOTLIN,
            native_models::SWIFT,
            native_models::SCHEMA
        );
        return Ok(ExitCode::SUCCESS);
    }
    if stale.is_empty() {
        println!(
            "the native models are what {} describes",
            native_models::SCHEMA
        );
        return Ok(ExitCode::SUCCESS);
    }
    for path in &stale {
        eprintln!(
            "- {} is stale",
            path.strip_prefix(&root).unwrap_or(path).display()
        );
    }
    eprintln!("write them again with `cargo xtask native-models`");
    Ok(ExitCode::FAILURE)
}

fn value<'a>(args: &mut std::slice::Iter<'_, &'a str>, flag: &str) -> Result<&'a str, String> {
    args.next()
        .copied()
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn number(text: &str) -> Result<usize, String> {
    text.parse().map_err(|_| format!("{text} is not a number"))
}
