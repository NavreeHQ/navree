//! Schedules on local clocks: `in_zone`, `in_default_zone`, and
//! `per_workspace_local` (once per workspace, at its own local time).

use std::sync::{Arc, Mutex};

use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{Job, JobContext, ZoneLevel, ZoneOwner};
use navree_kernel::BoxError;
use navree_schedule::{LocalRun, RunStatus, ScheduleError, SkipReason};
use navree_time::{TimeModule, ZonesFake};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::support::{Counter, Tasks, TestDb, at, clock, scheduler};

fn ticks(outcomes: &[navree_schedule::RunOutcome]) -> Vec<String> {
    outcomes.iter().map(|o| o.task.clone()).collect()
}

#[tokio::test]
async fn in_zone_runs_on_that_zones_clock() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("digest", task.task())
                    .daily_at("09:00")
                    .in_zone("Asia/Calcutta");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;
    assert!(scheduler.tick(at("2026-10-26T09:00:00Z")).await.unwrap().is_empty());
    let due = scheduler.tick(at("2026-10-26T03:30:00Z")).await.unwrap();
    assert_eq!(ticks(&due), ["digest"]);
    assert_eq!(due[0].status, RunStatus::Succeeded);
    assert_eq!(runs.get(), 1);
    let listed = scheduler.list(at("2026-10-26T04:00:00Z"));
    assert_eq!(listed[0].zone, "Asia/Kolkata");
    assert_eq!(listed[0].next_run, Some(at("2026-10-27T03:30:00Z")));
}

#[tokio::test]
async fn local_times_follow_daylight_saving_time() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-03-01T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("standup", task.task())
                    .daily_at("09:00")
                    .in_zone("America/New_York");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;
    // Friday 6 March (EST) and Monday 9 March (EDT).
    assert_eq!(scheduler.tick(at("2026-03-06T14:00:00Z")).await.unwrap().len(), 1);
    assert!(scheduler.tick(at("2026-03-09T14:00:00Z")).await.unwrap().is_empty());
    assert_eq!(scheduler.tick(at("2026-03-09T13:00:00Z")).await.unwrap().len(), 1);
    assert_eq!(runs.get(), 2);
}

#[tokio::test]
async fn a_time_skipped_by_dst_runs_right_after_the_gap() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-03-08T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("nightly", task.task())
                    .daily_at("02:30")
                    .in_zone("America/New_York");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;
    // 8 March 2026: 01:59 EST (06:59Z) is followed by 03:00 EDT (07:00Z).
    let mut minute = at("2026-03-08T05:00:00Z");
    let mut due_at = Vec::new();
    while minute < at("2026-03-08T09:00:00Z") {
        if !scheduler.tick(minute).await.unwrap().is_empty() {
            due_at.push(minute);
        }
        minute += chrono::TimeDelta::minutes(1);
    }
    assert_eq!(due_at, [at("2026-03-08T07:00:00Z")]);
    assert_eq!(runs.get(), 1);
}

#[tokio::test]
async fn a_time_repeated_by_dst_runs_once() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-11-01T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("nightly", task.task())
                    .daily_at("01:30")
                    .in_zone("America/New_York");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;
    // 1 November 2026: 01:30 EDT (05:30Z), then 01:30 EST (06:30Z).
    assert_eq!(scheduler.tick(at("2026-11-01T05:30:00Z")).await.unwrap().len(), 1);
    assert!(scheduler.tick(at("2026-11-01T06:30:00Z")).await.unwrap().is_empty());
    assert_eq!(runs.get(), 1);
}

#[tokio::test]
async fn an_unknown_zone_fails_the_boot() {
    let db = TestDb::new();
    let error = db
        .app(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(|s| {
                s.task("digest", |_| async { Ok(()) })
                    .daily_at("09:00")
                    .in_zone("Mars/Base");
            }),
        )
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Mars/Base"), "{error}");
    assert!(error.contains("digest"), "{error}");
}

#[tokio::test]
async fn the_default_zone_comes_from_time_module() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .app(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("digest", task.task())
                    .daily_at("09:00")
                    .in_default_zone();
            }),
        )
        .module(ConfigModule::for_root(
            ConfigSources::new().set("time.default_zone", "Asia/Dubai"),
        ))
        .module(TimeModule::from_config())
        .boot()
        .await
        .unwrap();
    let dubai = scheduler(&app).await;
    assert!(dubai.tick(at("2026-10-26T09:00:00Z")).await.unwrap().is_empty());
    assert_eq!(dubai.tick(at("2026-10-26T05:00:00Z")).await.unwrap().len(), 1);
    assert_eq!(dubai.list(at("2026-10-26T06:00:00Z"))[0].zone, "Asia/Dubai");

    // Without TimeModule the default zone is UTC.
    let db = TestDb::new();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("digest", task.task())
                    .daily_at("09:00")
                    .in_default_zone();
            }),
        )
        .await;
    let utc_scheduler = scheduler(&app).await;
    assert_eq!(
        utc_scheduler
            .tick(at("2026-10-26T09:00:00Z"))
            .await
            .unwrap()
            .len(),
        1
    );
}

struct Workspaces {
    fake: ZonesFake,
    kolkata: Uuid,
    new_york: Uuid,
    plain: Uuid,
}

fn workspaces() -> Workspaces {
    let (kolkata, new_york, plain) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let fake = ZonesFake::new(ZoneLevel::Workspace)
        .owner(ZoneOwner::new(kolkata).zone("Asia/Kolkata"))
        .owner(ZoneOwner::new(new_york).country("US"))
        .owner(ZoneOwner::new(plain));
    Workspaces {
        fake,
        kolkata,
        new_york,
        plain,
    }
}

/// Records the owner and zone of every run.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(Option<Uuid>, String)>>>);

impl Seen {
    fn all(&self) -> Vec<(Option<Uuid>, String)> {
        self.0.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn per_workspace_local_runs_each_workspace_at_its_own_nine() {
    let db = TestDb::new();
    let w = workspaces();
    let seen = Seen::default();
    let record = seen.clone();
    let app = db
        .app(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(move |s| {
                let record = record.clone();
                s.task("digest", move |_ctx| {
                    let record = record.clone();
                    async move {
                        let run = LocalRun::current().expect("inside a scheduled run");
                        record
                            .0
                            .lock()
                            .unwrap()
                            .push((run.owner, run.zone.name().to_owned()));
                        Ok::<(), BoxError>(())
                    }
                })
                .daily_at("09:00")
                .per_workspace_local();
            }),
        )
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(TimeModule::from_config())
        .module(w.fake.module())
        .boot()
        .await
        .unwrap();
    let scheduler = scheduler(&app).await;

    let kolkata = scheduler.tick(at("2026-10-26T03:30:00Z")).await.unwrap();
    assert_eq!(ticks(&kolkata), [format!("digest@{}", w.kolkata)]);
    let utc = scheduler.tick(at("2026-10-26T09:00:00Z")).await.unwrap();
    assert_eq!(ticks(&utc), [format!("digest@{}", w.plain)]);
    let new_york = scheduler.tick(at("2026-10-26T13:00:00Z")).await.unwrap();
    assert_eq!(ticks(&new_york), [format!("digest@{}", w.new_york)]);
    assert_eq!(
        seen.all(),
        [
            (Some(w.kolkata), "Asia/Kolkata".to_owned()),
            (Some(w.plain), "UTC".to_owned()),
            (Some(w.new_york), "America/New_York".to_owned()),
        ]
    );

    // Each workspace's tick is claimed on its own.
    let again = scheduler.tick(at("2026-10-26T03:30:00Z")).await.unwrap();
    assert_eq!(again[0].status, RunStatus::Skipped(SkipReason::AlreadyClaimed));

    // A workspace added later joins at its next local nine.
    let later = Uuid::now_v7();
    w.fake.set(ZoneOwner::new(later).zone("Asia/Dubai"));
    let dubai = scheduler.tick(at("2026-10-27T05:00:00Z")).await.unwrap();
    assert_eq!(ticks(&dubai), [format!("digest@{later}")]);

    let listed = scheduler.list(at("2026-10-26T00:00:00Z"));
    assert_eq!(listed[0].zone, "per workspace (local)");
    assert_eq!(listed[0].next_run, None);
}

/// A digest for one workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Digest {
    workspace: Option<Uuid>,
}

static DIGESTS: Mutex<Vec<Option<Uuid>>> = Mutex::new(Vec::new());

impl Job for Digest {
    const NAME: &'static str = "test.zones.digest";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        DIGESTS.lock().unwrap().push(self.workspace);
        Ok(())
    }
}

#[tokio::test]
async fn jobs_are_built_per_workspace() {
    let db = TestDb::new();
    let w = workspaces();
    let app = db
        .app(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(|s| {
                s.job_with(|run: &LocalRun| Digest {
                    workspace: run.owner,
                })
                .daily_at("09:00")
                .per_workspace_local();
            }),
        )
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(TimeModule::from_config())
        .module(w.fake.module())
        .boot()
        .await
        .unwrap();
    let scheduler = scheduler(&app).await;
    let due = scheduler.tick(at("2026-10-26T03:30:00Z")).await.unwrap();
    assert_eq!(ticks(&due), [format!("test.zones.digest@{}", w.kolkata)]);
    assert_eq!(DIGESTS.lock().unwrap().clone(), [Some(w.kolkata)]);
}

#[tokio::test]
async fn utc_tasks_see_a_local_run_without_owner() {
    let db = TestDb::new();
    let seen = Seen::default();
    let record = seen.clone();
    let app = db
        .boot(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(move |s| {
                let record = record.clone();
                s.task("sweep", move |_ctx| {
                    let record = record.clone();
                    async move {
                        let run = LocalRun::current().expect("inside a scheduled run");
                        assert_eq!(run.tick, run.local.and_utc());
                        record
                            .0
                            .lock()
                            .unwrap()
                            .push((run.owner, run.zone.name().to_owned()));
                        Ok::<(), BoxError>(())
                    }
                })
                .hourly();
            }),
        )
        .await;
    scheduler(&app)
        .await
        .tick(at("2026-10-26T10:00:00Z"))
        .await
        .unwrap();
    assert_eq!(seen.all(), [(None, "UTC".to_owned())]);
    assert!(LocalRun::current().is_none(), "only inside a run");
}

#[tokio::test]
async fn per_workspace_local_needs_time_module() {
    let db = TestDb::new();
    let error = db
        .app(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(|s| {
                s.task("digest", |_| async { Ok(()) })
                    .daily_at("09:00")
                    .per_workspace_local();
            }),
        )
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("TimeModule"), "{error}");
}

#[tokio::test]
async fn per_owner_task_names_leave_room_for_the_owner() {
    let db = TestDb::new();
    let long = "x".repeat(160);
    let error = db
        .app(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(move |s| {
                s.task(long.clone(), |_| async { Ok(()) })
                    .daily_at("09:00")
                    .per_workspace_local();
            }),
        )
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(TimeModule::from_config())
        .boot()
        .await
        .unwrap_err();
    let text = error.to_string();
    assert!(text.contains("154"), "{text}");
    let _ = ScheduleError::AppDropped; // the error type stays public
}

#[tokio::test]
async fn a_counter_still_counts_runs_in_zones() {
    // Two tasks on different clocks due at the same instant both run.
    let db = TestDb::new();
    let runs = Counter::default();
    let (a, b) = (runs.clone(), runs.clone());
    let app = db
        .boot(
            &clock("2026-10-26T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("paris", a.task()).daily_at("10:00").in_zone("Europe/Paris");
                s.task("utc", b.task()).daily_at("09:00");
            }),
        )
        .await;
    let due = scheduler(&app)
        .await
        .tick(at("2026-10-26T09:00:00Z"))
        .await
        .unwrap();
    assert_eq!(due.len(), 2);
    assert_eq!(runs.get(), 2);
}
