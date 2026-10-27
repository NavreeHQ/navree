//! The MongoDB migration commands: `mongo:migrate`,
//! `mongo:migrate:rollback`, `mongo:migrate:reset`, `mongo:migrate:refresh`,
//! `mongo:migrate:fresh` and `mongo:migrate:status`.

use std::collections::HashMap;

use navree_config::Config;
use navree_kernel::{AppContext, BoxError};

use super::{
    MongoMigrateOptions, MongoMigrationReport, MongoMigrator, MongoRollbackOptions,
};

/// What an option takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Takes {
    Nothing,
    Number,
}

/// A command's options, for parsing and for messages.
struct Spec {
    command: &'static str,
    options: &'static [(&'static str, Takes)],
}

/// Parsed options.
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
}

impl Spec {
    fn usage(&self) -> String {
        if self.options.is_empty() {
            return "no options".to_owned();
        }
        self.options
            .iter()
            .map(|(name, takes)| match takes {
                Takes::Nothing => format!("--{name}"),
                Takes::Number => format!("--{name} <n>"),
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
                Takes::Number => {
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
    command: "mongo:migrate",
    options: &[("pretend", Takes::Nothing), ("step", Takes::Nothing)],
};
const ROLLBACK: Spec = Spec {
    command: "mongo:migrate:rollback",
    options: &[
        ("step", Takes::Number),
        ("batch", Takes::Number),
        ("pretend", Takes::Nothing),
        ("force", Takes::Nothing),
    ],
};
const RESET: Spec = Spec {
    command: "mongo:migrate:reset",
    options: &[("pretend", Takes::Nothing), ("force", Takes::Nothing)],
};
const REFRESH: Spec = Spec {
    command: "mongo:migrate:refresh",
    options: &[("step", Takes::Number), ("force", Takes::Nothing)],
};
const FRESH: Spec = Spec {
    command: "mongo:migrate:fresh",
    options: &[("force", Takes::Nothing)],
};
const STATUS: Spec = Spec {
    command: "mongo:migrate:status",
    options: &[("pending", Takes::Nothing)],
};

/// The app's environment: top-level `env` (`APP_ENV`), else `app.env`.
async fn environment(ctx: &AppContext) -> Option<String> {
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
            "{} would change or drop collections and this app runs in production (`env = \"{env}\"`); pass --force to run it anyway",
            spec.command
        )
        .into()),
        _ => Ok(()),
    }
}

/// Prints what a run did.
fn print_report(report: &MongoMigrationReport, verb: &str, statements: bool, empty: &str) {
    if report.migrations.is_empty() {
        println!("{empty}");
    }
    for run in &report.migrations {
        if statements {
            println!("{}:", run.label());
            if run.statements.is_empty() {
                println!("  (no commands)");
            }
            for statement in &run.statements {
                println!("  {statement}");
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
    let options = MongoMigrateOptions {
        pretend: parsed.flag("pretend"),
        step: parsed.flag("step"),
    };
    let report = MongoMigrator::of(&ctx).await?.up(&options).await?;
    print_report(&report, "Migrated", options.pretend, "Nothing to migrate.");
    Ok(())
}

pub(crate) async fn rollback(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = ROLLBACK.parse(&args)?;
    let step = parsed.number("step")?;
    let batch = parsed.number("batch")?;
    if step.is_some() && batch.is_some() {
        return Err("mongo:migrate:rollback takes --step or --batch, not both".into());
    }
    let pretend = parsed.flag("pretend");
    if !pretend {
        guard(&ctx, &ROLLBACK, &parsed).await?;
    }
    let report = MongoMigrator::of(&ctx)
        .await?
        .rollback(&MongoRollbackOptions {
            step,
            batch: batch.map(i64::from),
            pretend,
        })
        .await?;
    print_report(&report, "Rolled back", pretend, "Nothing to roll back.");
    Ok(())
}

pub(crate) async fn reset(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = RESET.parse(&args)?;
    let pretend = parsed.flag("pretend");
    if !pretend {
        guard(&ctx, &RESET, &parsed).await?;
    }
    let report = MongoMigrator::of(&ctx).await?.reset(pretend).await?;
    print_report(&report, "Rolled back", pretend, "Nothing to roll back.");
    Ok(())
}

pub(crate) async fn refresh(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = REFRESH.parse(&args)?;
    guard(&ctx, &REFRESH, &parsed).await?;
    let (reverted, migrated) = MongoMigrator::of(&ctx)
        .await?
        .refresh(parsed.number("step")?)
        .await?;
    print_report(&reverted, "Rolled back", false, "Nothing to roll back.");
    print_report(&migrated, "Migrated", false, "Nothing to migrate.");
    Ok(())
}

pub(crate) async fn fresh(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = FRESH.parse(&args)?;
    guard(&ctx, &FRESH, &parsed).await?;
    let (dropped, report) = MongoMigrator::of(&ctx).await?.fresh().await?;
    println!("Dropped {} collections.", dropped.len());
    print_report(&report, "Migrated", false, "Nothing to migrate.");
    Ok(())
}

pub(crate) async fn status(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let parsed = STATUS.parse(&args)?;
    let pending_only = parsed.flag("pending");
    let status = MongoMigrator::of(&ctx).await?.status().await?;
    if status.is_empty() {
        println!("No MongoDB migrations are registered.");
        return Ok(());
    }
    let shown: Vec<_> = status
        .iter()
        .filter(|entry| !pending_only || !entry.applied)
        .collect();
    if shown.is_empty() {
        println!("No pending MongoDB migrations.");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn options_parse_and_mistakes_are_explained() {
        let parsed = ROLLBACK.parse(&args(&["--step", "2", "--pretend"])).unwrap();
        assert_eq!(parsed.number("step").unwrap(), Some(2));
        assert!(parsed.flag("pretend"));
        assert_eq!(ROLLBACK.parse(&args(&["--batch=3"])).unwrap().number("batch").unwrap(), Some(3));
        let message = |spec: &Spec, list: &[&str]| match spec.parse(&args(list)) {
            Ok(_) => String::new(),
            Err(error) => error.to_string(),
        };
        assert_eq!(
            message(&STATUS, &["--all"]),
            "unknown option `--all` for mongo:migrate:status; it takes --pending"
        );
        assert_eq!(
            message(&MIGRATE, &["now"]),
            "mongo:migrate takes no arguments (got `now`); it takes --pretend, --step"
        );
        assert_eq!(message(&ROLLBACK, &["--step"]), "`--step` needs a value");
        let zero = ROLLBACK.parse(&args(&["--step", "0"])).unwrap();
        assert_eq!(
            zero.number("step").unwrap_err().to_string(),
            "`--step` takes a positive whole number, not `0`"
        );
        assert_eq!(FRESH.usage(), "--force");
    }
}
