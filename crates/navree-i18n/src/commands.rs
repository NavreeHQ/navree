//! The `i18n:*` commands.

use std::fs;

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::{I18n, shipped, tag};

/// The names of the module's commands (the app loads the Fluent files
/// leniently while running one, so they can report problems).
pub(crate) const NAMES: [&str; 3] = ["i18n:check", "i18n:missing", "i18n:publish"];

/// Every command of the localization module.
pub(crate) fn commands() -> [CommandSpec; 3] {
    [
        CommandSpec::new(
            NAMES[0],
            "Parse every Fluent file and list the messages each locale is missing; exits 1 on a problem",
            |ctx, args| async move { run(ctx, args, check).await },
        ),
        CommandSpec::new(
            NAMES[1],
            "List the message ids a locale is missing: i18n:missing <locale>",
            |ctx, args| async move { run(ctx, args, missing).await },
        ),
        CommandSpec::new(
            NAMES[2],
            "Copy the shipped validation messages into i18n.dir: i18n:publish validation [--force]",
            |ctx, args| async move { run(ctx, args, publish).await },
        ),
    ]
}

/// Runs a command against the app's `I18n` and prints its output.
async fn run(
    ctx: AppContext,
    args: Vec<String>,
    command: fn(&I18n, &[String]) -> Result<String, BoxError>,
) -> Result<(), BoxError> {
    let i18n = ctx.get::<I18n>().await?;
    println!("{}", command(&i18n, &args)?);
    Ok(())
}

/// `i18n:check`: the report, and an error (exit status 1) when it has
/// problems.
pub(crate) fn check(i18n: &I18n, args: &[String]) -> Result<String, BoxError> {
    if let Some(arg) = args.first() {
        return Err(format!("i18n:check: unexpected argument `{arg}`").into());
    }
    let report = i18n.report();
    let text = report.lines.join("\n");
    if report.problems == 0 {
        Ok(text)
    } else {
        Err(text.into())
    }
}

/// `i18n:missing <locale>`: one id per line.
pub(crate) fn missing(i18n: &I18n, args: &[String]) -> Result<String, BoxError> {
    let [locale] = args else {
        return Err("usage: i18n:missing <locale>".into());
    };
    let ids = i18n.missing(locale)?;
    if ids.is_empty() {
        let tag = i18n.locale(locale);
        Ok(format!("No messages are missing from {tag}."))
    } else {
        Ok(ids.join("\n"))
    }
}

/// `i18n:publish validation [--force]`: writes
/// `<i18n.dir>/<locale>/validation.ftl` for each supported locale whose
/// language navree-i18n ships messages for, keeping existing files unless
/// `--force`.
pub(crate) fn publish(i18n: &I18n, args: &[String]) -> Result<String, BoxError> {
    let mut force = false;
    let mut package = None;
    for arg in args {
        match arg.as_str() {
            "--force" => force = true,
            other if package.is_none() && !other.starts_with('-') => package = Some(other),
            other => return Err(format!("i18n:publish: unexpected argument `{other}`").into()),
        }
    }
    match package {
        Some("validation") => {}
        Some(other) => {
            return Err(format!(
                "i18n:publish: nothing to publish called `{other}` (available: validation)"
            )
            .into());
        }
        None => return Err("usage: i18n:publish validation [--force]".into()),
    }
    let dir = &i18n.options().dir;
    let mut lines = Vec::new();
    for locale in i18n.locales() {
        let Ok(langid) = tag::parse(&locale) else {
            continue;
        };
        let Some(source) = shipped::validation_for(&langid) else {
            lines.push(format!("{locale}: navree-i18n ships no validation messages in this language"));
            continue;
        };
        let path = dir.join(&locale).join("validation.ftl");
        let shown = path.to_string_lossy().replace('\\', "/");
        if path.exists() && !force {
            lines.push(format!("{shown}: exists, kept (use --force to replace it)"));
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, source)?;
        lines.push(format!("{shown}: written"));
    }
    Ok(lines.join("\n"))
}
