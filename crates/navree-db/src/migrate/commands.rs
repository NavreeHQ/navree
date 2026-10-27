//! The migration commands: `migrate`, `migrate:install`, `migrate:rollback`,
//! `migrate:reset`, `migrate:refresh`, `migrate:fresh`, `migrate:status`
//! and `schema:dump`.

use std::collections::HashMap;
use std::path::PathBuf;

use navree_config::Config;
use navree_kernel::{AppContext, BoxError};

use super::Migrator;
use super::report::{FreshOptions, MigrateOptions, MigrationReport, RollbackOptions};
use crate::seed::run_seeders;

/// What an option of a command takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Takes {
    Nothing,
    Number,
    Path,
}

/// The options a command accepts, for parsing and for messages.
struct Spec {
    command: &'static str,
    options: &'static [(&'static str, Takes)],
}

/// Parsed options: flags present, and values given.
#[derive(Default)]
struct Parsed {
    flags: Vec<&'static str>,
    values: HashMap<&'static str, String>,
}

impl Parsed {
    fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }

    fn number(&self, name: &str) -> Result<Option<u32>, BoxError> {
        self.values
            .get(name)
            .map(|value| {
                value.parse::<u32>().ok().filter(|n| *n > 0).ok_or_else(|| {
                    BoxError::from(format!(
                        "`--{name}` takes a positive whole number, not `{value}`"
                    ))
                })
            })
            .transpose()
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.values.get(name).map(PathBuf::from)
    }
}

impl Spec {
    fn usage(&self) -> String {
        self.options
            .iter()
            .map(|(name, takes)| match takes {
                Takes::Nothing => format!("--{name}"),
                Takes::Number => format!("--{name} <n>"),
                Takes::Path => format!("--{name} <path>"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `--flag`, `--option value` and `--option=value`.
    fn parse(&self, args: &[String]) -> Result<Parsed, BoxError> {
        let mut parsed = Parsed::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let Some(option) = arg.strip_prefix("--") else {
                return Err(format!(
                    "{} takes no arguments (got `{arg}`); it takes {}",
                    self.command,
                    self.usage()
                )
                .into());
            };
            let (name, inline) = match option.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (option, None),
            };
            let Some((name, takes)) = self.options.iter().find(|(known, _)| *known == name) else {
                return Err(format!(
                    "unknown option `--{name}` for {}; it takes {}",
                    self.command,
                    self.usage()
                )
                .into());
            };
            match takes {
                Takes::Nothing => {
                    if inline.is_some() {
                        return Err(format!("`--{name}` takes no value").into());
                    }
                    parsed.flags.push(name);
                }
                Takes::Number | Takes::Path => {
                    let value = match inline {
                        Some(value) => value,
                        None => args
                            .next()
                            .cloned()
                            .ok_or_else(|| format!("`--{name}` needs a value"))?,
                    };
                    parsed.values.insert(name, value);
                }
            }
        }
        Ok(parsed)
    }
}

const MIGRATE: Spec = Spec {
    command: "migrate",
    options: &[
        ("pretend", Takes::Nothing),
        ("step", Takes::Nothing),
        ("seed", Takes::Nothing),
        ("schema-path", Takes::Path),
        ("force", Takes::Nothing),
    ],
};
const INSTALL: Spec = Spec {
    command: "migrate:install",
    options: &[],
};
const ROLLBACK: Spec = Spec {
    command: "migrate:rollback",
    options: &[
        ("step", Takes::Number),
        ("batch", Takes::Number),
        ("pretend", Takes::Nothing),
        ("force", Takes::Nothing),
    ],
};
const RESET: Spec = Spec {
    command: "migrate:reset",
    options: &[("pretend", Takes::Nothing), ("force", Takes::Nothing)],
};
const REFRESH: Spec = Spec {
    command: "migrate:refresh",
    options: &[
        ("step", Takes::Number),
        ("seed", Takes::Nothing),
        ("force", Takes::Nothing),
    ],
};
const FRESH: Spec = Spec {
    command: "migrate:fresh",
    options: &[
        ("seed", Takes::Nothing),
        ("drop-views", Takes::Nothing),
        ("drop-types", Takes::Nothing),
        ("step", Takes::Nothing),
        ("schema-path", Takes::Path),
        ("force", Takes::Nothing),
    ],
};
const STATUS: Spec = Spec {
    command: "migrate:status",
    options: &[("pending", Takes::Nothing), ("json", Takes::Nothing)],
};
const DUMP: Spec = Spec {
    command: "schema:dump",
    options: &[("path", Takes::Path)],
};

/// The app's environment: top-level `env` (`APP_ENV`), else `app.env`.
pub(crate) async fn environment(ctx: &AppContext) -> Option<String> {
    let config = ctx.try_get::<Config>().await?;
    config
        .get_string("env")
        .or_else(|| config.get_string("app.env"))
}

/// Refuses a destructive command in production unless `--force` is given.
async fn guard(ctx: &AppContext, spec: &Spec, parsed: &Parsed) -> Result<(), BoxError> {
    if parsed.flag("force") {
        return Ok(());
    }
    match environment(ctx).await {
        Some(env) if env.eq_ignore_ascii_case("production") => Err(format!(
            "{} would change or drop tables and this app runs in production (`env = \"{env}\"`); pass --force to run it anyway",
            spec.command
        )
        .into()),
        _ => Ok(()),
    }
}

/// Prints what a run did: each migration with its time and notices, and
/// with `statements` (pretending) the SQL.
fn print_report(report: &MigrationReport, verb: &str, statements: bool, empty: &str) {
    if let Some(dump) = &report.loaded_dump {
        println!(
            "Loaded the schema dump {} ({} statements).",
            dump.path.display(),
            dump.statements
        );
    }
    if report.migrations.is_empty() {
        println!("{empty}");
    }
    for run in &report.migrations {
        if statements {
            println!("{}:", run.label());
            if run.statements.is_empty() {
                println!("  (no statements)");
            }
            for statement in &run.statements {
                println!("  {statement};");
            }
        } else {
            println!("{verb}: {} ({} ms)", run.label(), run.duration.as_millis());
        }
        for notice in &run.notices {
            println!("  notice: {notice}");
        }
    }
}

pub(crate) async fn migrate(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = MIGRATE.parse(&args)?;
    let options = MigrateOptions {
        pretend: parsed.flag("pretend"),
        step: parsed.flag("step"),
        schema_path: parsed.path("schema-path"),
    };
    let report = Migrator::run(&ctx, &options).await?;
    print_report(&report, "Migrated", options.pretend, "Nothing to migrate.");
    if parsed.flag("seed") && !options.pretend {
        seed(&ctx).await?;
    }
    Ok(())
}

pub(crate) async fn install(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    INSTALL.parse(&args)?;
    Migrator::install(&ctx).await?;
    println!("The migrations table is ready.");
    Ok(())
}

pub(crate) async fn rollback(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = ROLLBACK.parse(&args)?;
    let step = parsed.number("step")?;
    let batch = parsed.number("batch")?;
    if step.is_some() && batch.is_some() {
        return Err("migrate:rollback takes --step or --batch, not both".into());
    }
    let pretend = parsed.flag("pretend");
    if !pretend {
        guard(&ctx, &ROLLBACK, &parsed).await?;
    }
    let options = RollbackOptions {
        step,
        batch: batch.map(i64::from),
        pretend,
    };
    let report = Migrator::rollback_with(&ctx, &options).await?;
    print_report(&report, "Rolled back", pretend, "Nothing to roll back.");
    Ok(())
}

pub(crate) async fn reset(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = RESET.parse(&args)?;
    let pretend = parsed.flag("pretend");
    if !pretend {
        guard(&ctx, &RESET, &parsed).await?;
    }
    let report = Migrator::reset(&ctx, pretend).await?;
    print_report(&report, "Rolled back", pretend, "Nothing to roll back.");
    Ok(())
}

pub(crate) async fn refresh(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = REFRESH.parse(&args)?;
    guard(&ctx, &REFRESH, &parsed).await?;
    let refreshed = Migrator::refresh(&ctx, parsed.number("step")?).await?;
    print_report(
        &refreshed.reverted,
        "Rolled back",
        false,
        "Nothing to roll back.",
    );
    print_report(
        &refreshed.migrated,
        "Migrated",
        false,
        "Nothing to migrate.",
    );
    if parsed.flag("seed") {
        seed(&ctx).await?;
    }
    Ok(())
}

pub(crate) async fn fresh(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = FRESH.parse(&args)?;
    guard(&ctx, &FRESH, &parsed).await?;
    let options = FreshOptions {
        drop_views: parsed.flag("drop-views"),
        drop_types: parsed.flag("drop-types"),
        migrate: MigrateOptions {
            step: parsed.flag("step"),
            schema_path: parsed.path("schema-path"),
            ..MigrateOptions::default()
        },
    };
    let report = Migrator::fresh_with(&ctx, &options).await?;
    println!(
        "Dropped all tables{}{}.",
        if options.drop_views { ", views" } else { "" },
        if options.drop_types { " and types" } else { "" }
    );
    print_report(&report, "Migrated", false, "Nothing to migrate.");
    if parsed.flag("seed") {
        seed(&ctx).await?;
    }
    Ok(())
}

pub(crate) async fn status(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = STATUS.parse(&args)?;
    let pending_only = parsed.flag("pending");
    let status = Migrator::status(&ctx).await?;
    let shown: Vec<_> = status
        .iter()
        .filter(|entry| !pending_only || !entry.applied)
        .collect();
    if parsed.flag("json") {
        // `[{"module", "name", "applied", "batch", "applied_at"}]`, for tools
        // (Navigator's `describe_schema`).
        println!("{}", serde_json::to_string_pretty(&shown)?);
        return Ok(());
    }
    if status.is_empty() {
        println!("No migrations are registered.");
        return Ok(());
    }
    if shown.is_empty() {
        println!("No pending migrations.");
        return Ok(());
    }
    println!("{:<7} {:<8} Migration", "Batch", "Status");
    for entry in shown {
        let batch = entry.batch.map(|b| b.to_string()).unwrap_or_default();
        let state = if entry.applied { "Ran" } else { "Pending" };
        println!("{batch:<7} {state:<8} {}/{}", entry.module, entry.name);
    }
    Ok(())
}

pub(crate) async fn dump(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = DUMP.parse(&args)?;
    let path = Migrator::dump(&ctx, parsed.path("path").as_deref()).await?;
    println!(
        "Dumped the schema to {}. `navree migrate` loads it into an empty database.",
        path.display()
    );
    Ok(())
}

async fn seed(ctx: &AppContext) -> Result<(), BoxError> {
    let ran = run_seeders(ctx, &[]).await?;
    if ran.is_empty() {
        println!("No seeders are registered.");
    }
    for name in ran {
        println!("Seeded: {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn options_parse_with_spaces_or_equals() {
        let parsed = ROLLBACK
            .parse(&args(&["--step", "3", "--pretend"]))
            .unwrap();
        assert_eq!(parsed.number("step").unwrap(), Some(3));
        assert!(parsed.flag("pretend"));
        let parsed = ROLLBACK.parse(&args(&["--batch=2"])).unwrap();
        assert_eq!(parsed.number("batch").unwrap(), Some(2));
        let parsed = DUMP.parse(&args(&["--path=out/schema.sql"])).unwrap();
        assert_eq!(parsed.path("path"), Some(PathBuf::from("out/schema.sql")));
    }

    #[test]
    fn mistakes_are_explained() {
        let message = |spec: &Spec, list: &[&str]| match spec.parse(&args(list)) {
            Ok(_) => String::new(),
            Err(error) => error.to_string(),
        };
        assert_eq!(
            message(&STATUS, &["--all"]),
            "unknown option `--all` for migrate:status; it takes --pending, --json"
        );
        assert_eq!(
            message(&MIGRATE, &["now"]),
            "migrate takes no arguments (got `now`); it takes --pretend, --step, --seed, --schema-path <path>, --force"
        );
        assert_eq!(message(&ROLLBACK, &["--step"]), "`--step` needs a value");
        assert_eq!(
            message(&MIGRATE, &["--seed=yes"]),
            "`--seed` takes no value"
        );
        let zero = ROLLBACK.parse(&args(&["--step", "0"])).unwrap();
        assert_eq!(
            zero.number("step").unwrap_err().to_string(),
            "`--step` takes a positive whole number, not `0`"
        );
    }
}
