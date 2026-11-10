//! `without_overlapping`: a task still running blocks its next ticks.

use std::sync::Arc;

use chrono::TimeDelta;
use navree_db::{Db, Executor, Statement};
use navree_kernel::{AppContext, BoxError};
use navree_schedule::{RunStatus, SkipReason};
use tokio::sync::{Semaphore, mpsc};

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{Tasks, TestDb, at, clock, scheduler, within};

/// A task body that reports when it starts and then waits to be released.
#[derive(Clone)]
struct Gate {
    started: mpsc::UnboundedSender<()>,
    release: Arc<Semaphore>,
}

impl Gate {
    fn new() -> (Self, mpsc::UnboundedReceiver<()>) {
        let (started, rx) = mpsc::unbounded_channel();
        let gate = Self {
            started,
            release: Arc::new(Semaphore::new(0)),
        };
        (gate, rx)
    }

    fn open(&self) {
        self.release.add_permits(1_000);
    }

    fn task(
        &self,
    ) -> impl Fn(AppContext) -> navree_kernel::BoxFuture<'static, Result<(), BoxError>>
    + Clone
    + Send
    + Sync
    + 'static {
        let gate = self.clone();
        move |_ctx| {
            let gate = gate.clone();
            Box::pin(async move {
                let _ = gate.started.send(());
                let _permit = gate.release.acquire().await?;
                Ok(())
            })
        }
    }
}

async fn without_overlapping_skips_while_running(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let (gate, mut started) = Gate::new();
    let body = gate.task();
    let app = db
        .boot(
            &clock("2026-10-25T10:00:00Z"),
            Tasks::new(move |s| {
                s.task("import", body.clone()).without_overlapping();
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;

    let first = tokio::spawn({
        let scheduler = scheduler.clone();
        async move { scheduler.tick(at("2026-10-25T10:00:00Z")).await }
    });
    within(started.recv()).await.expect("the first run starts");

    let second = within(scheduler.tick(at("2026-10-25T10:01:00Z")))
        .await
        .unwrap();
    assert_eq!(
        second[0].status,
        RunStatus::Skipped(SkipReason::Overlapping)
    );

    gate.open();
    let first = within(first).await.unwrap().unwrap();
    assert_eq!(first[0].status, RunStatus::Succeeded);

    let third = within(scheduler.tick(at("2026-10-25T10:02:00Z")))
        .await
        .unwrap();
    assert_eq!(third[0].status, RunStatus::Succeeded);
    started.recv().await.expect("the third run started");
    assert!(
        started.try_recv().is_err(),
        "the skipped tick never started"
    );
}

async fn without_overlapping_sees_runs_of_other_schedulers(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let clock = clock("2026-10-25T10:00:00Z");
    let (gate, mut started) = Gate::new();
    let (body_one, body_two) = (gate.task(), gate.task());
    let one = db
        .boot(
            &clock,
            Tasks::new(move |s| {
                s.task("import", body_one.clone()).without_overlapping();
            }),
        )
        .await;
    let two = db
        .boot(
            &clock,
            Tasks::new(move |s| {
                s.task("import", body_two.clone()).without_overlapping();
            }),
        )
        .await;

    let first = tokio::spawn({
        let scheduler = scheduler(&one).await;
        async move { scheduler.tick(at("2026-10-25T10:00:00Z")).await }
    });
    within(started.recv()).await.expect("the first run starts");

    let other = within(scheduler(&two).await.tick(at("2026-10-25T10:01:00Z")))
        .await
        .unwrap();
    assert_eq!(other[0].status, RunStatus::Skipped(SkipReason::Overlapping));

    gate.open();
    within(first).await.unwrap().unwrap();
}

async fn a_running_row_older_than_a_day_does_not_block(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let (gate, mut started) = Gate::new();
    gate.open();
    let body = gate.task();
    let app = db
        .boot(
            &clock("2026-10-25T10:00:00Z"),
            Tasks::new(move |s| {
                s.task("import", body.clone()).without_overlapping();
            }),
        )
        .await;

    // A run from a process that crashed 25 hours ago.
    let crashed = at("2026-10-25T10:00:00Z") - TimeDelta::hours(25);
    let db_handle = app.ctx().get::<Db>().await.unwrap();
    db_handle
        .execute(&Statement::portable(
            "INSERT INTO schedule_runs (task, tick, started_at, status) VALUES (?, ?, ?, ?)",
            [
                "import".into(),
                crashed.into(),
                crashed.into(),
                "running".into(),
            ],
        ))
        .await
        .unwrap();

    let outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T10:00:00Z"))
        .await
        .unwrap();
    assert_eq!(outcomes[0].status, RunStatus::Succeeded);
    started.recv().await.expect("it ran");
}

async fn tasks_may_overlap_by_default(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let (gate, mut started) = Gate::new();
    let body = gate.task();
    let app = db
        .boot(
            &clock("2026-10-25T10:00:00Z"),
            Tasks::new(move |s| {
                s.task("report", body.clone());
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;

    let ticks: Vec<_> = ["2026-10-25T10:00:00Z", "2026-10-25T10:01:00Z"]
        .into_iter()
        .map(|time| {
            let scheduler = scheduler.clone();
            tokio::spawn(async move { scheduler.tick(at(time)).await })
        })
        .collect();
    within(started.recv()).await.expect("the first run starts");
    within(started.recv())
        .await
        .expect("the second run starts while the first runs");

    gate.open();
    for tick in ticks {
        assert_eq!(
            within(tick).await.unwrap().unwrap()[0].status,
            RunStatus::Succeeded
        );
    }
}

on_every_server!(
    "navree_schedule";
    without_overlapping_skips_while_running,
    without_overlapping_sees_runs_of_other_schedulers,
    a_running_row_older_than_a_day_does_not_block,
    tasks_may_overlap_by_default,
);
