---
name: navree-fleet
description: "Queue workers under Fleet with navree-fleet (facade feature `fleet`, in `full`): `FleetModule` and the `fleet` config section (supervisors with queues, `balance = auto|simple|off`, min/max processes, concurrency, recycling limits `max_jobs`/`max_memory_mb`/`max_time_secs`, `fleet.only`, `fleet.long_wait`, snapshots and trimming), the `fleet` master command and its `fleet:work` child processes, `fleet:status`/`fleet:pause`/`fleet:continue`/`fleet:terminate`/`fleet:forget`, the `Fleet` service (pause/resume, masters, status, workload, long_waits, failed jobs retry/retry_all/forget, pending and completed jobs, metrics), the `/_fleet` dashboard and its API behind the `Fleet` abilities `view`/`operate` (`fleet.view`, `fleet.operate`), the `fleet_*` gauges on `/metrics`, `LongWaitDetected`, and testing with `FakeSpawner`, `InProcessSpawner` and `next_allocation`. Use when running queue workers in production, balancing them across queues, pausing workers, adding the queue dashboard, alerting on queue backlogs, or debugging workers that do not start, restart in a loop, or ignore a pause."
---

# Fleet

`navree-fleet` supervises queue workers. `<app> fleet` starts a **master**
that runs no jobs: it starts worker processes (the same binary, command
`fleet:work`) per **supervisor**, balances them across queues, pauses,
recycles and restarts them, stores what they ran, and raises long-wait
alerts. `FleetModule` also serves a dashboard at `/_fleet`. It is
`navree::fleet` (facade feature `fleet`, in `full`). When in doubt read
`crates/navree-fleet/src` and its tests: `tests/fleet/` (service, master
with `FakeSpawner`, in-process workers, dashboard, commands, gauges) and
`tests/processes.rs` (real worker processes).

## Steps: run workers under Fleet

1. Add the module after `QueueModule` (database driver; `fleet` refuses
   `queue.driver = "memory"`):

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(QueueModule::from_config())
    .module(AuthModule::from_config())                       // the dashboard signs in through it
    .module(AccessModule::for_root(AccessOptions::default())) // the dashboard checks abilities
    .module(FleetModule::from_config())                      // section `fleet`
```

   No `AuthModule`/`AccessModule`? Set `fleet.dashboard = false` or the
   boot fails (the dashboard routes declare access).

2. Run `navree migrate` (migration set `fleet`, after `queue`: tables
   `fleet_masters`, `fleet_state`, `fleet_jobs`, `fleet_metrics`; on
   TimescaleDB the last two are hypertables, an hour and a day per chunk,
   and trimming drops whole chunks; on PostgreSQL, MySQL and SQLite they are
   regular tables with a time index and trimming deletes the old rows).

3. Describe supervisors (optional: without any, one supervisor `default`
   works `queue.queues` or `queue.default_queue`):

```toml
[fleet.supervisors.default]
queues = ["default", "mail", "notifications"]   # priority order; required
balance = "auto"        # auto (default) | simple | off
min_processes = 1       # per queue (auto)
max_processes = 8       # whole supervisor (default 4)
concurrency = 4         # jobs per process (default queue.workers)
max_jobs = 1000         # recycle after N jobs (0: never)
max_memory_mb = 256     # recycle above N MB (0: never)
max_time_secs = 3600    # recycle after N s (0: never)

[fleet.supervisors.exports]
queues = ["exports"]
balance = "off"
max_processes = 2

[fleet.long_wait]       # seconds before a long-wait alert, per queue
mail = 30
```

4. Run it: `navree fleet` locally, `./my-app fleet` in production under a
   process manager (systemd `KillMode=mixed`, `TimeoutStopSec` longer than
   the longest job). Replace your `queue:work` processes with it.
5. After a deploy: restart the unit or run `fleet:terminate` (every master
   stops gracefully; one started afterwards keeps running).
6. Check: `fleet:status` (exits 1 when no master reported in 30 s).

## The rules the code enforces

- `auto`: `min_processes × queues ≤ max_processes`; `simple`:
  `max_processes ≥ queues`. Breaking one fails the boot naming
  `fleet.supervisors.<name>`. `fleet.only` must name supervisors.
- `auto` rebalances every `fleet.balance_cooldown_secs` (3): workload =
  (waiting + running jobs) × average runtime of the last 5 minutes (1 s per
  job when none ran); spare processes go proportionally to workload, never
  more than `ceil(active jobs / concurrency)` per queue; each queue moves by
  at most `fleet.balance_max_shift` (1) per step. An idle queue keeps
  `min_processes`.
- `off`: one group of `max_processes` processes working every queue
  (process queue key `exports,default`).
- A worker at a limit finishes its jobs, exits and is replaced at once. A
  crash or a failed spawn restarts after 1 s, doubling to 60 s; five
  failures in a minute log an error under `navree::fleet`.
- Pause is stored in the database (`fleet_state`), seen by every master
  within a tick (1 s); paused workers finish running jobs and take no more.
- Env overrides: `APP_FLEET__SUPERVISORS__DEFAULT__MAX_PROCESSES=16`,
  `APP_FLEET__ONLY=exports`, lists as comma-separated strings.

## The Fleet service

```rust
use navree::fleet::{Fleet, MetricsPeriod};

let fleet = ctx.get::<Fleet>().await?;
fleet.pause(Some("exports")).await?;       // None: every supervisor
fleet.resume(None).await?;
let status = fleet.status().await?;        // Running | Paused | Inactive
let workload = fleet.workload().await?;    // per queue: pending, oldest_wait_secs, avg_runtime_ms, workload_secs, processes, jobs_per_minute
let late = fleet.long_waits().await?;      // Vec<LongWaitDetected>
let failed = fleet.failed_jobs(Some("contacts.export")).await?;
fleet.retry(&failed[0].id).await?;         // FleetError::FailedJobNotFound when gone
fleet.retry_all(Some("contacts.export")).await?;
fleet.forget(&id).await?;
let charts = fleet.metrics(MetricsPeriod::Hour).await?;
```

## The dashboard

- `GET /_fleet` (page) and `/_fleet/assets/fleet-<content hash>.{js,css}` are
  public and hold no data. The page signs in through
  `{http.api_prefix}{auth.routes_prefix}/login` (navree-auth, 2FA too) or
  a pasted token, and calls `/_fleet/api/*` with `Authorization: Bearer`.
- Abilities on subject `Fleet` (prefix `fleet`): `view` for every `GET`
  (`overview`, `masters`, `jobs`, `jobs/{id}`, `failed`, `failed/{id}`,
  `metrics?period=hour|day|week`), `operate` for `POST failed/{id}/retry`,
  `POST failed/retry?search=`, `DELETE failed/{id}`, `POST pause`,
  `POST continue` (body `{"supervisor": "x"}` or none).
  Grant with `navree access:grant manager fleet.view`, or in code
  `rules.can("view", "Fleet")`.
- Payload fields matching `log.redact` fragments show `[redacted]`.
- `fleet.path` moves it; `fleet.dashboard = false` removes it.

## Long-wait alerts

```rust
#[injectable]
pub struct PageOnCall { /* ... */ }

impl Listener<LongWaitDetected> for PageOnCall {
    fn handle<'a>(&'a self, e: &'a LongWaitDetected, _ctx: &'a AppContext)
        -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move { /* e.queue, e.wait_secs, e.threshold_secs */ Ok(()) })
    }
}
// in wire: m.listen::<LongWaitDetected, PageOnCall>();
```

A running master emits it (needs `EventsModule`) at most once per queue per
`fleet.long_wait_cooldown_minutes` (15) across all masters.

## Testing

- What Fleet decides: `TestApp` with `.temp_database()` (a SQLite file: the
  test crate's `navree` dev-dependency needs the `sqlite` feature, which
  `navree new` writes), `.config("fleet.dashboard", false)`, a `FixedClock`
  override, dispatch jobs, then `fleet.long_waits()` / `fleet.workload()`.
- The balancer: `next_allocation(&supervisor, &current, &workload, shift)`
  with `QueueWorkload::new(queue, waiting, running, oldest_wait_secs,
  avg_runtime_ms)` and `Supervisor::resolve(name, &SupervisorOptions, &queue_options)`.
- A master without processes:

```rust
use navree::fleet::testing::FakeSpawner;
use navree::fleet::{ExitReason, Master, MasterSettings, WorkerCommand};

let spawner = Arc::new(FakeSpawner::new());
let master = Master::from_app(app.ctx()).await?
    .with_spawner(spawner.clone())
    .with_settings(MasterSettings { tick: Duration::from_millis(20), ..MasterSettings::default() });
let stop = CancellationToken::new();
let task = tokio::spawn(master.run(stop.clone()));
// spawner.running_specs(), spawner.commands(i), spawner.exit(i, ExitReason::MaxJobs),
// spawner.report(i, WorkerReport::Heartbeat { jobs: 3, memory_mb: Some(40) })
stop.cancel();
let summary = task.await??; // started, recycled, crashed, jobs
```

- Real workers inside the test app: `.with_spawner(Arc::new(InProcessSpawner::new(app.ctx().clone())))`.

## Gotchas

- Workers are child processes of `std::env::current_exe()` with
  `fleet:work ...`; they read config the normal way (`.env`,
  `config/app.toml`, `APP_*`), so run `fleet` from the app's working
  directory. `ProcessSpawner::env(k, v)` adds variables.
- A worker prints its reports as `@fleet {json}` lines on stdout; keep
  your own stdout lines from starting with `@fleet `.
- Telemetry is turned on by `FleetModule` (its `JobRecorder` sink receives
  `JobProcessed`/`JobFailed`); that is how workers report attempts.
- `fleet_jobs` timestamps come from telemetry (real time), not a
  `FixedClock`.
- `/metrics` gauges come from a navree-observe `MetricsCollector`; they
  need `ObserveModule` with metrics on.
