//! [`ScheduleModule`].

use std::sync::Arc;

use navree_config::Config;
use navree_contracts::{Clock, SystemClock};
use navree_db::navree_schema::SchemaMigration;
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt};
use navree_kernel::{
    AppContext, BoxError, CommandSpec, Factory, Module, ModuleBuilder, ModuleId, ProviderKey,
    Resolver,
};
use navree_telemetry::{Telemetry, TracePolicy, TracePolicySection};
use navree_time::Zones;

use crate::store::{ConvertScheduleRunsToHypertable, CreateScheduleRuns, MIGRATION_MODULE};
use crate::{RunStatus, Scheduler, SkipReason, TaskInfo};

/// The scheduler module. Global; provides and exports [`Scheduler`], built
/// at boot from every module's [`Schedule`](crate::Schedule) (invalid
/// frequencies, duplicate names and unknown commands fail the boot).
///
/// Needs `DatabaseModule` (it contributes the `schedule` migration set,
/// creating `schedule_runs`) and uses the app's `dyn Clock` when one is
/// registered. Registers the commands `schedule:run`, `schedule:work` and
/// `schedule:list`.
///
/// Feature modules may import it by type (`m.import(ScheduleModule)`); the
/// app root's `ScheduleModule::for_root()` is the same module.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScheduleModule;

impl ScheduleModule {
    /// The module, configured once in the app root (it has no options; a
    /// plain `ScheduleModule` import does the same).
    pub fn for_root() -> impl Module {
        ConfiguredScheduleModule
    }
}

impl Module for ScheduleModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m);
    }
}

/// `ScheduleModule::for_root()`: same module id, configured.
struct ConfiguredScheduleModule;

impl Module for ConfiguredScheduleModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<ScheduleModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m);
    }
}

fn register(m: &mut ModuleBuilder) {
    m.global()
        .migrations(
            MigrationSet::new(MIGRATION_MODULE)
                .migration(SchemaMigration::new(
                    CreateScheduleRuns::NAME,
                    CreateScheduleRuns,
                ))
                .migration(SchemaMigration::new(
                    ConvertScheduleRunsToHypertable::NAME,
                    ConvertScheduleRunsToHypertable,
                )),
        )
        .factory(
            Factory::<Scheduler>::new(|r: Resolver| async move {
                let db = r.get::<Db>().await?;
                let app = r.app();
                let clock = match app.try_get::<dyn Clock>().await {
                    Some(clock) => clock,
                    None => Arc::new(SystemClock),
                };
                let telemetry = Telemetry::from_app(&app).await;
                let policy = match app.try_get::<Config>().await {
                    Some(config) => TracePolicy::from_section(
                        config.section_or_default::<TracePolicySection>(TracePolicy::SECTION)?,
                        |name| std::env::var(name).ok(),
                    )?,
                    None => TracePolicy::default(),
                };
                let zones = app.try_get::<Zones>().await;
                Ok(Arc::new(
                    Scheduler::new(&app, db, clock, zones)?.instrumented(telemetry, policy),
                ))
            })
            .deps(vec![ProviderKey::of::<Db>()]),
        )
        .exports::<Scheduler>()
        .command(CommandSpec::new(
            "schedule:run",
            "Run the scheduled tasks due this minute (call it every minute from cron)",
            run_command,
        ))
        .command(CommandSpec::new(
            "schedule:work",
            "Run the scheduler in the foreground, ticking every minute until stopped",
            work_command,
        ))
        .command(CommandSpec::new(
            "schedule:list",
            "List the scheduled tasks and their next run",
            list_command,
        ));
}

async fn run_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let scheduler = ctx.get::<Scheduler>().await?;
    let outcomes = scheduler.tick(scheduler.now()).await?;
    if outcomes.is_empty() {
        println!("No scheduled tasks are due.");
    }
    let mut failed = Vec::new();
    for outcome in &outcomes {
        match &outcome.status {
            RunStatus::Succeeded => println!("Ran: {}", outcome.task),
            RunStatus::Failed(error) => {
                println!("Failed: {} ({error})", outcome.task);
                failed.push(outcome.task.as_str());
            }
            RunStatus::Skipped(SkipReason::AlreadyClaimed) => {
                println!("Skipped: {} (already run for this minute)", outcome.task);
            }
            RunStatus::Skipped(SkipReason::Overlapping) => {
                println!("Skipped: {} (still running)", outcome.task);
            }
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("scheduled tasks failed: {}", failed.join(", ")).into())
    }
}

async fn work_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let scheduler = ctx.get::<Scheduler>().await?;
    let tasks = scheduler.list(scheduler.now()).len();
    tracing::info!(target: "navree::schedule", tasks, "scheduler started");
    scheduler.work(ctx.shutdown_token()).await?;
    tracing::info!(target: "navree::schedule", "scheduler stopped");
    Ok(())
}

async fn list_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let scheduler = ctx.get::<Scheduler>().await?;
    print!("{}", list_table(&scheduler.list(scheduler.now())));
    Ok(())
}

/// The `schedule:list` table.
fn list_table(tasks: &[TaskInfo]) -> String {
    if tasks.is_empty() {
        return "No tasks are scheduled.\n".to_owned();
    }
    let rows: Vec<[String; 4]> = tasks
        .iter()
        .map(|t| {
            let next = match t.next_run {
                Some(next) => next.format("%Y-%m-%d %H:%M UTC").to_string(),
                None if t.zone.starts_with("per ") => "per owner".to_owned(),
                None => "never".to_owned(),
            };
            let name = if t.without_overlapping {
                format!("{} (without overlapping)", t.name)
            } else {
                t.name.clone()
            };
            [t.expr.clone(), t.zone.clone(), next, name]
        })
        .collect();
    let expr_width = rows.iter().map(|r| r[0].len()).max().unwrap_or(0).max(10);
    let zone_width = rows.iter().map(|r| r[1].len()).max().unwrap_or(0).max(4);
    let next_width = rows.iter().map(|r| r[2].len()).max().unwrap_or(0).max(8);
    let mut out = format!(
        "{:<expr_width$}  {:<zone_width$}  {:<next_width$}  Task\n",
        "Expression", "Zone", "Next run"
    );
    for [expr, zone, next, name] in rows {
        out.push_str(&format!(
            "{expr:<expr_width$}  {zone:<zone_width$}  {next:<next_width$}  {name}\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::*;

    #[test]
    fn list_table_aligns_columns() {
        let next = DateTime::parse_from_rfc3339("2026-10-26T02:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let table = list_table(&[
            TaskInfo {
                name: "report".to_owned(),
                module: "ReportsModule",
                expr: "30 2 * * *".to_owned(),
                next_run: Some(next),
                without_overlapping: true,
                zone: "UTC".to_owned(),
            },
            TaskInfo {
                name: "sync".to_owned(),
                module: "ReportsModule",
                expr: "*/5 * * * *".to_owned(),
                next_run: None,
                without_overlapping: false,
                zone: "Asia/Kolkata".to_owned(),
            },
            TaskInfo {
                name: "digest".to_owned(),
                module: "ReportsModule",
                expr: "0 9 * * *".to_owned(),
                next_run: None,
                without_overlapping: false,
                zone: "per workspace (local)".to_owned(),
            },
        ]);
        assert_eq!(
            table,
            "Expression   Zone                   Next run              Task\n\
             30 2 * * *   UTC                    2026-10-26 02:30 UTC  report (without overlapping)\n\
             */5 * * * *  Asia/Kolkata           never                 sync\n\
             0 9 * * *    per workspace (local)  per owner             digest\n"
        );
        assert_eq!(list_table(&[]), "No tasks are scheduled.\n");
    }
}
