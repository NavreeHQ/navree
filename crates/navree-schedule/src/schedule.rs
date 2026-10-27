//! [`Schedule`], [`TaskBuilder`] and [`ModuleBuilderScheduleExt`]: how modules declare tasks.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use navree_contracts::{Dispatcher, Job, JobContext, JobRegistration, ZoneLevel};
use navree_kernel::{AppContext, BoxError, BoxFuture, ModuleBuilder};

use crate::LocalRun;

/// The closure behind a function or job task: the app and the run id
/// (`<task>@<tick>`, used as the job id when a job runs inline).
pub(crate) type TaskFn =
    Arc<dyn Fn(AppContext, String) -> BoxFuture<'static, Result<(), BoxError>> + Send + Sync>;

/// What a task runs.
#[derive(Clone)]
pub(crate) enum TaskKind {
    /// A function (`Schedule::task`) or a job dispatch (`Schedule::job`).
    Run(TaskFn),
    /// A registered CLI command (`Schedule::command`).
    Command {
        /// The command name.
        name: String,
        /// Its arguments.
        args: Vec<String>,
    },
}

/// The tasks one module schedules, declared in
/// [`ModuleBuilderScheduleExt::schedule`] and contributed to the app.
///
/// ```
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_schedule::ModuleBuilderScheduleExt;
///
/// struct ReportsModule;
///
/// impl Module for ReportsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.schedule(|s| {
///             s.command("audit:prune", &[]).daily_at("03:00").without_overlapping();
///             s.task("reports.refresh", |_ctx| async { Ok(()) }).every_fifteen_minutes();
///         });
///     }
/// }
/// ```
#[derive(Clone, Default)]
pub struct Schedule {
    pub(crate) tasks: Vec<TaskBuilder>,
    /// The job types of `job` tasks, contributed for the queue's workers.
    pub(crate) jobs: Vec<JobRegistration>,
}

impl Schedule {
    /// Schedules job `J`: each due tick dispatches a clone of `job` through
    /// the app's `dyn Dispatcher` (navree-queue), or runs it inline when no
    /// dispatcher is registered. The task is named after [`Job::NAME`].
    ///
    /// [`ModuleBuilderScheduleExt::schedule`] also contributes the job's
    /// [`JobRegistration`], so queue workers can run it without a separate
    /// `m.job::<J>()` (registering it twice is harmless).
    pub fn job<J: Job + Clone>(&mut self, job: J) -> &mut TaskBuilder {
        self.job_with(move |_run: &LocalRun| job.clone())
    }

    /// Schedules job `J`, built for each run by `f` from the run's
    /// [`LocalRun`] (its owner and zone), so a task that runs
    /// [per workspace](TaskBuilder::per_workspace_local) dispatches one job
    /// per workspace: `s.job_with(|run| Digest { workspace_id: run.owner })`.
    /// Otherwise like [`job`](Schedule::job).
    pub fn job_with<J, F>(&mut self, f: F) -> &mut TaskBuilder
    where
        J: Job,
        F: Fn(&LocalRun) -> J + Send + Sync + 'static,
    {
        if !self.jobs.iter().any(|j| j.name == J::NAME) {
            self.jobs.push(JobRegistration::of::<J>());
        }
        let build = Arc::new(f);
        let run: TaskFn = Arc::new(move |app: AppContext, run_id: String| {
            let job = LocalRun::current().map(|run| build(&run));
            Box::pin(async move {
                let job = job.ok_or("a scheduled job was built outside its scheduled run")?;
                match app.try_get::<dyn Dispatcher>().await {
                    Some(dispatcher) => {
                        let id = dispatcher.dispatch(job).await?;
                        tracing::debug!(
                            target: "navree::schedule",
                            job = J::NAME,
                            job_id = %id,
                            "scheduled job dispatched"
                        );
                        Ok(())
                    }
                    None => {
                        job.handle(JobContext {
                            app,
                            job_id: run_id,
                            attempt: 1,
                        })
                        .await
                    }
                }
            })
        });
        self.push(J::NAME.to_owned(), TaskKind::Run(run))
    }

    /// Schedules the async function `f`, called with the app on each due
    /// tick. A closure works as well as a plain `fn`:
    /// `s.task("cleanup", |ctx| async move { ... })`.
    pub fn task<F, Fut>(&mut self, name: impl Into<String>, f: F) -> &mut TaskBuilder
    where
        F: Fn(AppContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), BoxError>> + Send + 'static,
    {
        let run: TaskFn = Arc::new(move |app: AppContext, _run_id: String| Box::pin(f(app)));
        self.push(name.into(), TaskKind::Run(run))
    }

    /// Schedules the registered CLI command `name` with `args`, run
    /// in-process against the app (`s.command("audit:prune", &["--days", "30"])`).
    /// The task is named after the command line; the boot fails when no
    /// module registers the command.
    pub fn command(&mut self, name: &str, args: &[&str]) -> &mut TaskBuilder {
        let task_name = std::iter::once(name)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        let kind = TaskKind::Command {
            name: name.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        self.push(task_name, kind)
    }

    fn push(&mut self, name: String, kind: TaskKind) -> &mut TaskBuilder {
        self.tasks.push(TaskBuilder {
            name,
            kind,
            expr: EVERY_MINUTE.to_owned(),
            invalid: None,
            without_overlapping: false,
            on_one_server: false,
            zone: ZoneChoice::Utc,
        });
        let last = self.tasks.len() - 1;
        &mut self.tasks[last]
    }
}

impl fmt::Debug for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.tasks).finish()
    }
}

const EVERY_MINUTE: &str = "* * * * *";

/// Which clock a task's frequency is read on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ZoneChoice {
    /// UTC (the default).
    Utc,
    /// A named zone, checked at boot.
    Named(String),
    /// The app's default zone (`time.default_zone`), UTC without TimeModule.
    Default,
    /// Each owner's own zone at this level (a run per owner).
    PerOwner(ZoneLevel),
}

/// One scheduled task: when it runs and how. Times are UTC unless
/// [`in_zone`](TaskBuilder::in_zone),
/// [`in_default_zone`](TaskBuilder::in_default_zone) or
/// [`per_workspace_local`](TaskBuilder::per_workspace_local) say otherwise;
/// ticks have minute granularity. Without a frequency a task runs every
/// minute.
///
/// Every task runs at most once per tick across all schedulers sharing the
/// database (the tick is claimed with `INSERT … ON CONFLICT DO NOTHING` on
/// `schedule_runs`).
#[derive(Clone)]
pub struct TaskBuilder {
    pub(crate) name: String,
    pub(crate) kind: TaskKind,
    pub(crate) expr: String,
    /// Why the frequency is invalid (reported when the scheduler boots).
    pub(crate) invalid: Option<String>,
    pub(crate) without_overlapping: bool,
    pub(crate) on_one_server: bool,
    pub(crate) zone: ZoneChoice,
}

impl TaskBuilder {
    /// Runs on a five-field cron expression (`minute hour day-of-month month
    /// day-of-week`, UTC), e.g. `"*/10 9-17 * * MON-FRI"`. Nicknames such as
    /// `@hourly` work too. An invalid expression fails the boot.
    pub fn cron(&mut self, expr: &str) -> &mut Self {
        self.set(expr.trim().to_owned())
    }

    /// Runs every minute (the default).
    pub fn every_minute(&mut self) -> &mut Self {
        self.set(EVERY_MINUTE.to_owned())
    }

    /// Runs every five minutes (`*/5 * * * *`).
    pub fn every_five_minutes(&mut self) -> &mut Self {
        self.set("*/5 * * * *".to_owned())
    }

    /// Runs every fifteen minutes (`*/15 * * * *`).
    pub fn every_fifteen_minutes(&mut self) -> &mut Self {
        self.set("*/15 * * * *".to_owned())
    }

    /// Runs at the start of every hour (`0 * * * *`).
    pub fn hourly(&mut self) -> &mut Self {
        self.set("0 * * * *".to_owned())
    }

    /// Runs every hour at `minute` past (0–59; anything else fails the boot).
    pub fn hourly_at(&mut self, minute: u32) -> &mut Self {
        self.set(format!("{minute} * * * *"))
    }

    /// Runs every day at midnight UTC (`0 0 * * *`).
    pub fn daily(&mut self) -> &mut Self {
        self.set("0 0 * * *".to_owned())
    }

    /// Runs every day at `time`, written `"HH:MM"` (24-hour, UTC). A
    /// malformed time fails the boot.
    pub fn daily_at(&mut self, time: &str) -> &mut Self {
        match parse_hh_mm(time) {
            Some((hour, minute)) => self.set(format!("{minute} {hour} * * *")),
            None => {
                self.expr = format!("daily_at(\"{time}\")");
                self.invalid = Some(format!("`{time}` is not a 24-hour `HH:MM` time"));
                self
            }
        }
    }

    /// Runs every Sunday at midnight UTC (`0 0 * * 0`).
    pub fn weekly(&mut self) -> &mut Self {
        self.set("0 0 * * 0".to_owned())
    }

    /// Skips a tick while an earlier run of this task is still running, on
    /// this scheduler (an in-process lock) or on any other (a `running` row
    /// in `schedule_runs` younger than 24 hours).
    pub fn without_overlapping(&mut self) -> &mut Self {
        self.without_overlapping = true;
        self
    }

    /// Runs each tick on one server only. Every Navree task already does (each
    /// tick is claimed in `schedule_runs`), so this only states the
    /// intent.
    pub fn on_one_server(&mut self) -> &mut Self {
        self.on_one_server = true;
        self
    }

    /// Reads the frequency on `zone`'s clock (an IANA name; legacy aliases
    /// are accepted): `daily_at("09:00").in_zone("Asia/Kolkata")` runs at
    /// 09:00 in Kolkata. An unknown zone fails the boot.
    ///
    /// Across DST changes a local time that is skipped runs at the first
    /// minute after the gap, and one that happens twice runs once (the
    /// first time).
    pub fn in_zone(&mut self, zone: &str) -> &mut Self {
        self.zone = ZoneChoice::Named(zone.to_owned());
        self
    }

    /// Reads the frequency on the app's default zone (`time.default_zone`
    /// of navree-time's `TimeModule`; UTC without it).
    pub fn in_default_zone(&mut self) -> &mut Self {
        self.zone = ZoneChoice::Default;
        self
    }

    /// Runs once per workspace, when the workspace's own clock reaches the
    /// frequency: `daily_at("09:00").per_workspace_local()` runs at 09:00
    /// in each workspace's zone (its zone, else its country's, else its
    /// organization's, else the app's default). Workspaces come from
    /// navree-time's `Zones`, so the app needs `TimeModule` (the boot fails
    /// without it) and a workspace-level zone source (navree-workspace
    /// registers one).
    ///
    /// Each workspace's run is claimed on its own, as
    /// `<task>@<workspace id>` in `schedule_runs`, so the task name may be
    /// at most 154 bytes. Inside the run, [`LocalRun::current`] names the
    /// workspace; jobs built with [`Schedule::job_with`] receive it.
    pub fn per_workspace_local(&mut self) -> &mut Self {
        self.per_local(ZoneLevel::Workspace)
    }

    /// Runs once per user, at each user's local time (users come from a
    /// user-level zone source). See
    /// [`per_workspace_local`](TaskBuilder::per_workspace_local).
    pub fn per_user_local(&mut self) -> &mut Self {
        self.per_local(ZoneLevel::User)
    }

    /// Runs once per owner at `level`, at each owner's local time. See
    /// [`per_workspace_local`](TaskBuilder::per_workspace_local).
    pub fn per_local(&mut self, level: ZoneLevel) -> &mut Self {
        self.zone = ZoneChoice::PerOwner(level);
        self
    }

    /// Renames the task (the name keys `schedule_runs` and must be unique).
    pub fn name(&mut self, name: impl Into<String>) -> &mut Self {
        self.name = name.into();
        self
    }

    fn set(&mut self, expr: String) -> &mut Self {
        self.expr = expr;
        self.invalid = None;
        self
    }
}

impl fmt::Debug for TaskBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.kind {
            TaskKind::Run(_) => "run".to_owned(),
            TaskKind::Command { name, .. } => format!("command `{name}`"),
        };
        f.debug_struct("TaskBuilder")
            .field("name", &self.name)
            .field("kind", &kind)
            .field("expr", &self.expr)
            .field("without_overlapping", &self.without_overlapping)
            .field("on_one_server", &self.on_one_server)
            .field("zone", &self.zone)
            .finish()
    }
}

/// Parses `"HH:MM"` (hour 0–23 with one or two digits, minute 00–59).
fn parse_hh_mm(time: &str) -> Option<(u32, u32)> {
    let (hour, minute) = time.trim().split_once(':')?;
    let number = |s: &str| {
        (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    let hour = number(hour)?;
    let minute = number(minute).filter(|_| minute.len() == 2)?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// Declares scheduled tasks on a [`ModuleBuilder`].
pub trait ModuleBuilderScheduleExt {
    /// Runs `f` on a new [`Schedule`] and contributes it (plus a
    /// [`JobRegistration`] per scheduled job type); the app's
    /// `ScheduleModule` collects every module's schedule at boot.
    ///
    /// ```
    /// # use navree_kernel::ModuleBuilder;
    /// # use navree_schedule::ModuleBuilderScheduleExt;
    /// # fn register(m: &mut ModuleBuilder) {
    /// m.schedule(|s| {
    ///     s.task("reports.refresh", |_ctx| async { Ok(()) }).hourly();
    /// });
    /// # }
    /// ```
    fn schedule(&mut self, f: impl FnOnce(&mut Schedule)) -> &mut Self;
}

impl ModuleBuilderScheduleExt for ModuleBuilder {
    fn schedule(&mut self, f: impl FnOnce(&mut Schedule)) -> &mut Self {
        let mut schedule = Schedule::default();
        f(&mut schedule);
        for job in std::mem::take(&mut schedule.jobs) {
            self.contribute(job);
        }
        self.contribute(schedule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expr_of(build: impl FnOnce(&mut TaskBuilder)) -> String {
        let mut schedule = Schedule::default();
        build(schedule.task("t", |_ctx| async { Ok(()) }));
        schedule.tasks[0].expr.clone()
    }

    #[test]
    fn default_frequency_is_every_minute() {
        assert_eq!(expr_of(|_| {}), "* * * * *");
    }

    #[test]
    fn frequency_helpers_build_cron_expressions() {
        assert_eq!(
            expr_of(|t| {
                t.every_minute();
            }),
            "* * * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.every_five_minutes();
            }),
            "*/5 * * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.every_fifteen_minutes();
            }),
            "*/15 * * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.hourly();
            }),
            "0 * * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.hourly_at(17);
            }),
            "17 * * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.daily();
            }),
            "0 0 * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.daily_at("02:30");
            }),
            "30 2 * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.daily_at("7:05");
            }),
            "5 7 * * *"
        );
        assert_eq!(
            expr_of(|t| {
                t.weekly();
            }),
            "0 0 * * 0"
        );
        assert_eq!(
            expr_of(|t| {
                t.cron("*/10 9-17 * * MON-FRI");
            }),
            "*/10 9-17 * * MON-FRI"
        );
    }

    #[test]
    fn daily_at_rejects_malformed_times() {
        for bad in ["24:00", "12:60", "noon", "12", "12:5:00", ""] {
            let mut schedule = Schedule::default();
            schedule.task("t", |_ctx| async { Ok(()) }).daily_at(bad);
            assert!(
                schedule.tasks[0].invalid.is_some(),
                "`{bad}` should be rejected"
            );
        }
    }

    #[test]
    fn a_later_frequency_replaces_an_invalid_one() {
        let mut schedule = Schedule::default();
        schedule
            .task("t", |_ctx| async { Ok(()) })
            .daily_at("noon")
            .hourly();
        assert!(schedule.tasks[0].invalid.is_none());
        assert_eq!(schedule.tasks[0].expr, "0 * * * *");
    }

    #[test]
    fn names_flags_and_defaults() {
        let mut schedule = Schedule::default();
        schedule
            .task("cleanup", |_ctx| async { Ok(()) })
            .without_overlapping()
            .on_one_server();
        schedule
            .command("audit:prune", &["--days", "30"])
            .name("prune audit");
        schedule.command("cache:clear", &[]);

        let cleanup = &schedule.tasks[0];
        assert_eq!(cleanup.name, "cleanup");
        assert!(cleanup.without_overlapping);
        assert!(cleanup.on_one_server);

        assert_eq!(schedule.tasks[1].name, "prune audit");
        assert!(!schedule.tasks[1].without_overlapping);
        assert_eq!(schedule.tasks[2].name, "cache:clear");
        assert!(matches!(
            &schedule.tasks[1].kind,
            TaskKind::Command { name, args } if name == "audit:prune" && args == &["--days", "30"]
        ));
    }

    #[test]
    fn command_task_name_includes_its_arguments() {
        let mut schedule = Schedule::default();
        schedule.command("audit:prune", &["--days", "30"]);
        assert_eq!(schedule.tasks[0].name, "audit:prune --days 30");
    }

    fn cleanup(_ctx: AppContext) -> BoxFuture<'static, Result<(), BoxError>> {
        Box::pin(async { Ok(()) })
    }

    #[test]
    fn task_accepts_plain_fns_and_fn_pointers() {
        type TaskPtr = fn(AppContext) -> BoxFuture<'static, Result<(), BoxError>>;
        let mut schedule = Schedule::default();
        schedule.task("cleanup", cleanup).hourly();
        schedule.task("pointer", cleanup as TaskPtr).daily();
        assert_eq!(schedule.tasks.len(), 2);
    }
}
