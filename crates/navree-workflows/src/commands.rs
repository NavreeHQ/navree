//! The commands `workflows:list`, `workflows:show`, `workflows:cancel`,
//! `workflows:retry`, `workflows:tick`, `workflows:prune`,
//! `automations:list` and `automations:catalog`.

use std::fmt::Write as _;

use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::automation::{Automation, AutomationQuery, Automations, BlockInfo};
use crate::{RunDetails, RunQuery, RunStatus, WorkflowRun, Workflows, WorkflowsOptions};

/// Every command of the workflows module.
pub(crate) fn commands() -> [CommandSpec; 8] {
    [
        CommandSpec::new(
            "workflows:list",
            "List workflow runs, newest first ([--workflow <name>] [--status <s,..>] [--limit N])",
            list_command,
        ),
        CommandSpec::new(
            "workflows:show",
            "Show a workflow run with its steps (<run-id>)",
            show_command,
        ),
        CommandSpec::new(
            "workflows:cancel",
            "Cancel an unfinished workflow run (<run-id>)",
            cancel_command,
        ),
        CommandSpec::new(
            "workflows:retry",
            "Run a failed workflow run again from its failed step (<run-id>)",
            retry_command,
        ),
        CommandSpec::new(
            "workflows:tick",
            "Wake the runs that are due now (timers, timeouts, lost executions)",
            tick_command,
        ),
        CommandSpec::new(
            "workflows:prune",
            "Delete runs that finished more than N days ago ([--days N], default workflows.prune_after_days)",
            prune_command,
        ),
        CommandSpec::new(
            "automations:list",
            "List automations with their status and published version",
            automations_command,
        ),
        CommandSpec::new(
            "automations:catalog",
            "List the registered triggers, conditions and actions",
            catalog_command,
        ),
    ]
}

/// The value after `--name` (or in `--name=value`).
fn flag(args: &[String], name: &str) -> Result<Option<String>, BoxError> {
    let long = format!("--{name}");
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if *arg == long {
            return iter
                .next()
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("`{long}` needs a value").into());
        }
        if let Some(value) = arg.strip_prefix(&format!("{long}=")) {
            return Ok(Some(value.to_owned()));
        }
    }
    Ok(None)
}

fn run_id(args: &[String]) -> Result<Uuid, BoxError> {
    let text = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("give the run id")?;
    Uuid::parse_str(text).map_err(|_| format!("`{text}` is not a run id").into())
}

fn time(at: chrono::DateTime<chrono::Utc>) -> String {
    at.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// One line per run.
pub(crate) fn format_runs(runs: &[WorkflowRun]) -> String {
    if runs.is_empty() {
        return "No workflow runs.\n".to_owned();
    }
    let mut out = String::new();
    for run in runs {
        let _ = write!(
            out,
            "{}  {:<9}  {} v{}  started {}",
            run.id,
            run.status,
            run.workflow,
            run.version,
            time(run.created_at)
        );
        if let Some(at) = run.wake_at {
            let _ = write!(out, "  wakes {}", time(at));
        }
        if let Some(error) = &run.error {
            let _ = write!(out, "  error: {error}");
        }
        out.push('\n');
    }
    out
}

/// A run and its steps.
pub(crate) fn format_details(details: &RunDetails) -> String {
    let run = &details.run;
    let mut out = format!(
        "Run {}\n  workflow  {} v{}\n  status    {}\n  started   {}\n",
        run.id,
        run.workflow,
        run.version,
        run.status,
        time(run.created_at)
    );
    if let Some(at) = run.wake_at {
        let _ = writeln!(out, "  wakes     {}", time(at));
    }
    if let Some(at) = run.finished_at {
        let _ = writeln!(out, "  finished  {}", time(at));
    }
    if let Some(error) = &run.error {
        let _ = writeln!(out, "  error     {error}");
    }
    if details.steps.is_empty() {
        out.push_str("  no steps yet\n");
    } else {
        out.push_str("  steps:\n");
        for step in &details.steps {
            let _ = write!(
                out,
                "    {:>3}. {:<30} {:<6} {:<9}",
                step.position,
                step.key,
                step.kind.as_str(),
                step.status.as_str()
            );
            if step.attempts > 1 {
                let _ = write!(out, " attempts {}", step.attempts);
            }
            if let Some(error) = &step.error {
                let _ = write!(out, " error: {error}");
            }
            out.push('\n');
        }
    }
    out
}

/// One line per automation.
pub(crate) fn format_automations(automations: &[Automation]) -> String {
    if automations.is_empty() {
        return "No automations.\n".to_owned();
    }
    let mut out = String::new();
    for automation in automations {
        let version = automation
            .version
            .map_or_else(|| "unpublished".to_owned(), |v| format!("v{v}"));
        let _ = writeln!(
            out,
            "{}  {:<7}  {:<11}  {}  (trigger {})",
            automation.id,
            automation.status,
            version,
            automation.name,
            automation.definition.trigger.block_type
        );
    }
    out
}

/// The catalog, one block per line.
pub(crate) fn format_catalog(
    triggers: &[BlockInfo],
    conditions: &[BlockInfo],
    actions: &[BlockInfo],
) -> String {
    let mut out = String::new();
    for (title, blocks) in [
        ("Triggers", triggers),
        ("Conditions", conditions),
        ("Actions", actions),
    ] {
        let _ = writeln!(out, "{title}:");
        if blocks.is_empty() {
            out.push_str("  (none)\n");
        }
        for block in blocks {
            let _ = write!(out, "  {:<30} {}", block.block_type, block.label);
            if let Some(event) = &block.event {
                let _ = write!(out, "  (event {event})");
            }
            out.push('\n');
        }
    }
    out
}

async fn list_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let workflows = ctx.get::<Workflows>().await?;
    let mut query = RunQuery::new();
    if let Some(workflow) = flag(&args, "workflow")? {
        query = query.workflow(workflow);
    }
    if let Some(statuses) = flag(&args, "status")? {
        for status in statuses.split(',').filter(|s| !s.is_empty()) {
            query = query.status(status.trim().parse::<RunStatus>()?);
        }
    }
    if let Some(limit) = flag(&args, "limit")? {
        query = query.limit(
            limit
                .parse()
                .map_err(|_| format!("`{limit}` is not a number"))?,
        );
    }
    print!("{}", format_runs(&workflows.runs(&query).await?));
    Ok(())
}

async fn show_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let workflows = ctx.get::<Workflows>().await?;
    let details = workflows.details(run_id(&args)?).await?;
    print!("{}", format_details(&details));
    Ok(())
}

async fn cancel_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let workflows = ctx.get::<Workflows>().await?;
    let run = workflows.cancel(run_id(&args)?).await?;
    println!("Cancelled workflow run {} ({}).", run.id, run.workflow);
    Ok(())
}

async fn retry_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let workflows = ctx.get::<Workflows>().await?;
    let run = workflows.retry(run_id(&args)?).await?;
    println!("Workflow run {} ({}) runs again.", run.id, run.workflow);
    Ok(())
}

async fn tick_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let woken = ctx.get::<Workflows>().await?.tick().await?;
    println!("Woke {woken} workflow run(s).");
    Ok(())
}

async fn prune_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let options = ctx.get::<WorkflowsOptions>().await?;
    let days = match flag(&args, "days")? {
        Some(days) => days
            .parse::<u32>()
            .map_err(|_| format!("`{days}` is not a number of days"))?,
        None => options.prune_after_days,
    };
    if days == 0 {
        println!("Pruning is off (workflows.prune_after_days = 0).");
        return Ok(());
    }
    let workflows = ctx.get::<Workflows>().await?;
    let before = workflows.now() - chrono::TimeDelta::days(i64::from(days));
    let pruned = workflows.prune(before).await?;
    println!("Pruned {pruned} workflow run(s) finished more than {days} day(s) ago.");
    Ok(())
}

async fn automations_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let automations = ctx.get::<Automations>().await?;
    let list = automations
        .list(&AutomationQuery {
            limit: 1_000,
            ..AutomationQuery::default()
        })
        .await?;
    print!("{}", format_automations(&list));
    Ok(())
}

async fn catalog_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let automations = ctx.get::<Automations>().await?;
    let catalog = automations.catalog();
    print!(
        "{}",
        format_catalog(
            &catalog.triggers(),
            &catalog.conditions(),
            &catalog.actions()
        )
    );
    Ok(())
}
