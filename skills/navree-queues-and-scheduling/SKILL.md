---
name: navree-queues-and-scheduling
description: "Background work in Navree: `Job` types, registering and dispatching jobs, job options (retries, backoff, timeouts, unique jobs), jobs inside transactions, `queue:work` and failed jobs, `QueueFake`, and cron-style schedules with `m.schedule` run by `schedule:work` or `schedule:run`. Use when moving slow work out of a request, adding a job or worker, retrying or inspecting failed jobs, running something every minute, hour or day, or testing jobs and schedules."
---

# Queues and scheduling

`QueueModule` stores jobs (database driver by default: tables `jobs` and
`failed_jobs`; `memory` for tests); a `queue:work` process runs them with
retries. `ScheduleModule` runs registered commands, jobs and functions on
cron schedules (UTC by default, or a zone, or each workspace's local time),
once per tick across all servers sharing the database.
A new app has `QueueModule`; add `ScheduleModule::for_root()` to the root
for schedules.

## Steps: a job

1. `navree make:job posts publish_post` writes
   `src/modules/posts/jobs/publish_post.rs` (job name `posts.publish_post`)
   and registers it at the module's `// navree:wire` marker
   (`m.job::<PublishPost>()`).
2. Keep the payload small: ids, not records; never secrets.
3. Write `handle`: resolve services from `ctx.app`, open a transaction for
   writes, make it safe to run twice (it can be retried).
4. Dispatch it from a service with an injected `Arc<Queue>`.
5. Test it with `QueueFake` (dispatched?) and with a real queue plus
   `run_pending` (what it does).

## Job template

```rust
use navree::contracts::{Backoff, Job, JobContext, JobOptions};
use navree::db::Db;
use navree::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportContacts {
    pub company_id: Uuid,
    pub requested_by: Uuid,
}

impl Job for ExportContacts {
    const NAME: &'static str = "exports.contacts"; // stable: workers find the handler by it

    fn options() -> JobOptions {                    // optional; defaults shown in the table below
        JobOptions {
            queue: "exports".into(),
            max_attempts: 5,
            backoff: Backoff::Fixed { secs: 30 },
            timeout_secs: 300,
            unique_for_secs: Some(600),             // one pending job per identical payload
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let db = ctx.app.get::<Db>().await?;
        db.transaction(|uow| async move {
            let tx = uow.tx().await?;
            // ... write through &*tx or Repo::<E>::insert(&uow, ..)
            Ok(())
        })
        .await?;
        Ok(())
    }
}
```

| Option | Default |
|---|---|
| `queue` | `default` (empty means `queue.default_queue`) |
| `max_attempts` | 3, then the job moves to `failed_jobs` |
| `backoff` | `Backoff::Exponential { base_secs: 1, max_secs: 300 }` |
| `timeout_secs` | 120 per attempt (0 = none) |
| `unique_for_secs` | `None` |

A job doesn't run in a unit of work: open one (`db.transaction`, or
`db.uow(Some(user), None)` to act as a user; see `navree-database`).

## Register and dispatch

```rust
#[module(providers = [ExportService], exports = [ExportService], register = Self::wire)]
pub struct ExportsModule;

impl ExportsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.job::<ExportContacts>(); // dispatching an unregistered job fails with QueueError::UnknownJob
        // navree:wire
    }
}

#[injectable]
pub struct ExportService {
    queue: Arc<navree::queue::Queue>,
}

impl ExportService {
    pub async fn request(&self, company_id: Uuid, requested_by: Uuid) -> Result<String, navree::queue::QueueError> {
        self.queue.dispatch(ExportContacts { company_id, requested_by }).await // the job id
    }
}
```

- `queue.later(job, Duration::from_secs(3600))` delays it (by the app's
  `dyn Clock` when one is registered).
- Inside a request (or `Db::transaction`), a dispatched job joins the unit
  of work: it is queued only if the request commits (status < 400), so
  workers never see uncommitted data. `dispatch_in(&uow, job)` names the
  unit of work explicitly.
- Code that shouldn't depend on navree-queue injects
  `Arc<dyn navree::contracts::Dispatcher>` (`dispatch`, `dispatch_later`).
- Queued mail, queued listeners and queued notifications are jobs on the
  same queue.

## Run workers

```sh
navree queue:work                          # until Ctrl-C / SIGTERM (finishes running jobs)
navree queue:work --queue exports,default  # these queues, highest priority first
navree queue:work --stop-when-empty        # or --once, --max-jobs 500
navree queue:failed                        # list failed jobs
navree queue:retry <id>|all                # put them back
navree queue:flush                         # delete them
```

```toml
[queue]
driver = "database"        # or "memory" (same process only: tests)
workers = 4                # concurrent jobs per queue:work process
queues = ["exports", "default"]  # what workers process; empty = default_queue
```

Run workers as their own processes from the same binary and configuration
(`./app queue:work` under systemd); restart them after every deploy. The
`memory` driver never reaches a separate `queue:work` process.

## Schedules

```rust
fn wire(&self, m: &mut ModuleBuilder) {
    m.command(clear_command());
    m.schedule(|s| {
        s.command("exports:clear", &[]).daily_at("04:00").without_overlapping();
        s.job(RefreshStats).every_fifteen_minutes();        // the job type must be Clone; registered for you
        s.task("exports.prune", prune_exports).cron("0 6 * * MON-FRI");
        s.task("exports.ping", |ctx| async move {
            let _db = ctx.get::<navree::db::Db>().await?;
            Ok(())
        })
        .hourly();
    });
}

async fn prune_exports(ctx: AppContext) -> Result<(), BoxError> {
    // ...
    Ok(())
}
```

Frequencies: `every_minute()` (default), `every_five_minutes()`,
`every_fifteen_minutes()`, `hourly()`, `hourly_at(17)`, `daily()`,
`daily_at("08:00")`, `weekly()`, `cron("*/10 9-17 * * MON-FRI")` (five
fields). Times are **UTC** unless the task says otherwise (needs
`TimeModule`, feature `time`):

```rust
s.command("reports:close-day", &[]).daily_at("02:00").in_zone("Europe/London");
s.command("crm:task-reminders", &[]).daily_at("08:00").in_default_zone(); // time.default_zone
s.job_with(|run: &LocalRun| SendDigest { workspace_id: run.owner.unwrap_or_default() })
    .daily_at("09:00")
    .per_workspace_local(); // one run per workspace, at 09:00 on its clock
```

Per-workspace runs are claimed as `<task>@<workspace id>` (names at most
154 bytes); `LocalRun::current()` gives the owner and zone inside a run.
Task names must be unique (`.name("..")` renames). A failed task is logged
and not retried; schedule a job when it must be retried. See the
`navree-time-zones` skill.

```sh
navree schedule:work       # foreground, ticks every minute (process manager)
navree schedule:run        # or call this from cron every minute
navree schedule:list       # tasks, expressions, next runs
```

## Test

```rust
use navree::prelude::*;
use navree::queue::QueueFake;
use navree::testing::TestApp;
use uuid::Uuid;

use super::{ExportContacts, ExportService, ExportsModule};

#[tokio::test]
async fn requesting_an_export_queues_it() {
    let fake = QueueFake::new();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(ExportsModule)
        .override_value(Arc::new(fake.queue()))
        .build()
        .await;
    let company = Uuid::now_v7();
    app.get::<ExportService>().await.request(company, Uuid::now_v7()).await.unwrap();
    fake.assert_dispatched::<ExportContacts>(|job| job.company_id == company);
    fake.assert_dispatched_times::<ExportContacts>(1);
    app.shutdown().await.unwrap();
}
```

To run jobs for real, keep the queue and call
`app.get::<Queue>().await.run_pending().await` (returns `WorkStats` with
`succeeded`, `failed`, ...; it processes `queue.queues`, so set
`.config("queue.queues", json!(["exports", "default"]))` for other queues).
For schedules, override `dyn Clock` with a `FixedClock`, then
`app.get::<navree::schedule::Scheduler>().await.tick(time).await` returns a
`RunOutcome` per due task (`RunStatus::Succeeded`, `Skipped(..)`, `Failed(..)`).

## Common mistakes

- Forgetting `m.job::<J>()`: the fake accepts any job, production refuses
  it. Keep one test on the real queue.
- Large or secret payloads (they sit in the `jobs` table as JSON).
- Renaming `NAME` (or a queued listener's type) while jobs are waiting:
  they have no handler and fail.
- Local times in schedules: use `in_zone(..)` or `per_workspace_local()`;
  converting to UTC by hand breaks twice a year with DST.
- `without_overlapping` on a job task doesn't stop two jobs running on
  workers: make the job unique.

## Docs

- https://navree.com/docs/digging-deeper/queues/
- https://navree.com/docs/digging-deeper/scheduling/
