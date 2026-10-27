//! The master's lifecycle with a fake spawner: starting, balancing,
//! pausing, recycling, restarting, terminating and reporting.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use navree_contracts::BoxError;
use navree_events::{EventsModule, Listener, ModuleBuilderEventsExt};
use navree_fleet::testing::FakeSpawner;
use navree_fleet::{
    CompletedJob, ExitReason, FleetError, FleetStatus, Master, MasterSummary, MetricKind,
    MetricsPeriod, WorkerCommand, WorkerReport,
};
use navree_kernel::{
    AppContext, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_queue::{AttemptOutcome, WorkOptions};
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::support::{Harness, SendDigest, builder, eventually, fast};

fn supervisors() -> serde_json::Value {
    json!({
        "default": { "queues": ["default", "mail"], "max_processes": 4 },
        "exports": { "queues": ["exports"], "balance": "off", "max_processes": 2, "concurrency": 1 }
    })
}

/// A running master and its fake workers.
struct Running {
    spawner: Arc<FakeSpawner>,
    stop: CancellationToken,
    task: JoinHandle<Result<MasterSummary, FleetError>>,
    id: String,
}

impl Running {
    async fn start(h: &Harness) -> Self {
        let spawner = Arc::new(FakeSpawner::new());
        let master = Master::from_app(h.app.ctx())
            .await
            .expect("a master")
            .with_spawner(spawner.clone())
            .with_settings(fast());
        let id = master.id().to_owned();
        let stop = CancellationToken::new();
        let task = tokio::spawn(master.run(stop.clone()));
        Self {
            spawner,
            stop,
            task,
            id,
        }
    }

    fn running_queues(&self) -> Vec<String> {
        let mut queues: Vec<String> = self
            .spawner
            .running_specs()
            .iter()
            .map(|spec| spec.queue_key())
            .collect();
        queues.sort();
        queues
    }

    fn count_on(&self, queue: &str) -> usize {
        self.running_queues().iter().filter(|q| *q == queue).count()
    }

    async fn stop(self) -> MasterSummary {
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("the master stops")
            .unwrap()
            .expect("the master ran")
    }
}

#[tokio::test]
async fn a_master_starts_each_supervisors_processes_and_reports_them() {
    let h = Harness::new(builder().config("fleet.supervisors", supervisors())).await;
    let master = Running::start(&h).await;
    eventually("four workers", || async {
        master.spawner.running().len() == 4
    })
    .await;
    assert_eq!(
        master.running_queues(),
        ["default", "exports", "exports", "mail"]
    );
    let exports = master
        .spawner
        .running_specs()
        .into_iter()
        .find(|s| s.supervisor == "exports")
        .unwrap();
    assert_eq!(exports.concurrency, 1);
    assert_eq!(exports.max_jobs, 1000);
    assert_eq!(exports.max_memory_mb, 256);
    assert_eq!(exports.max_time_secs, 3600);
    assert!(!exports.paused);
    let default = master
        .spawner
        .running_specs()
        .into_iter()
        .find(|s| s.supervisor == "default")
        .unwrap();
    assert_eq!(default.concurrency, 4, "queue.workers");

    // Heartbeats: jobs and memory reported by a worker show up.
    let first = master.spawner.running()[0];
    master.spawner.report(
        first,
        WorkerReport::Heartbeat {
            jobs: 7,
            memory_mb: Some(42),
        },
    );
    eventually("a heartbeat with the worker's numbers", || async {
        h.fleet.masters().await.unwrap().first().is_some_and(|m| {
            m.supervisors
                .iter()
                .flat_map(|s| &s.processes)
                .any(|p| p.jobs == 7 && p.memory_mb == Some(42))
        })
    })
    .await;
    let masters = h.fleet.masters().await.unwrap();
    assert_eq!(masters.len(), 1);
    assert_eq!(masters[0].id, master.id);
    assert_eq!(masters[0].name, "worker-1");
    assert_eq!(masters[0].process_count(), 4);
    assert_eq!(h.fleet.status().await.unwrap(), FleetStatus::Running);
    assert_eq!(h.fleet.workload().await.unwrap()[1].processes, 1, "mail");

    let spawner = master.spawner.clone();
    let summary = master.stop().await;
    assert_eq!(summary.started, 4);
    for worker in 0..4 {
        assert_eq!(spawner.commands(worker), [WorkerCommand::Stop]);
    }
    assert!(spawner.running().is_empty());
    assert!(
        h.fleet.masters().await.unwrap().is_empty(),
        "the heartbeat is removed"
    );
}

#[tokio::test]
async fn only_runs_the_named_supervisors() {
    let h = Harness::new(
        builder()
            .config("fleet.supervisors", supervisors())
            .config("fleet.only", "exports"),
    )
    .await;
    let master = Running::start(&h).await;
    eventually("two workers", || async {
        master.spawner.running().len() == 2
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(master.running_queues(), ["exports", "exports"]);
    master.stop().await;
}

#[tokio::test]
async fn the_memory_driver_is_refused() {
    let h = Harness::new(builder().config("queue.driver", "memory")).await;
    let error = Master::from_app(h.app.ctx()).await.unwrap_err().to_string();
    assert!(error.contains("database"), "{error}");
}

#[tokio::test]
async fn a_backlog_moves_processes_to_its_queue_and_back() {
    let h = Harness::new(
        builder()
            .config(
                "fleet.supervisors.default",
                json!({ "queues": ["default", "mail"], "max_processes": 4 }),
            )
            .config("fleet.balance_cooldown_secs", 1),
    )
    .await;
    for i in 0..50 {
        h.queue
            .dispatch(SendDigest {
                user: format!("u{i}"),
                api_token: "t".into(),
            })
            .await
            .unwrap();
    }
    let master = Running::start(&h).await;
    // `mail` gets every spare process: 1 + 2, one step per cooldown.
    eventually("three mail workers", || async {
        master.count_on("mail") == 3
    })
    .await;
    assert_eq!(master.count_on("default"), 1);

    // The backlog drains (a worker of the test runs it): back to one.
    let options = WorkOptions {
        queues: vec!["mail".into()],
        stop_when_empty: true,
        ..WorkOptions::default()
    };
    h.queue.work(options).await.unwrap();
    eventually("one mail worker", || async { master.count_on("mail") == 1 }).await;
    let stopped = (0..master.spawner.spawned().len())
        .filter(|i| master.spawner.commands(*i) == [WorkerCommand::Stop])
        .count();
    assert_eq!(stopped, 2, "the extra workers were stopped");
    master.stop().await;
}

#[tokio::test]
async fn pausing_reaches_running_and_new_workers() {
    let h = Harness::new(builder().config("fleet.supervisors", supervisors())).await;
    let master = Running::start(&h).await;
    eventually("four workers", || async {
        master.spawner.running().len() == 4
    })
    .await;

    h.fleet.pause(Some("exports")).await.unwrap();
    let exports: Vec<usize> = master
        .spawner
        .running()
        .into_iter()
        .filter(|i| master.spawner.spawned()[*i].supervisor == "exports")
        .collect();
    eventually("exports workers paused", || async {
        exports
            .iter()
            .all(|i| master.spawner.commands(*i) == [WorkerCommand::Pause])
    })
    .await;
    eventually("the heartbeat shows the pause", || async {
        h.fleet.masters().await.unwrap()[0]
            .supervisors
            .iter()
            .any(|s| s.name == "exports" && s.paused)
    })
    .await;
    assert_eq!(h.fleet.status().await.unwrap(), FleetStatus::Running);

    // A replacement of a paused supervisor starts paused.
    master.spawner.exit(exports[0], ExitReason::MaxJobs);
    eventually("a replacement", || async {
        master.spawner.spawned().len() == 5
    })
    .await;
    assert!(master.spawner.spawned()[4].paused);

    h.fleet.pause(None).await.unwrap();
    eventually("every supervisor paused", || async {
        h.fleet.status().await.unwrap() == FleetStatus::Paused
    })
    .await;
    h.fleet.resume(None).await.unwrap();
    eventually("continued", || async {
        master
            .spawner
            .commands(exports[1])
            .ends_with(&[WorkerCommand::Continue])
    })
    .await;
    master.stop().await;
}

#[tokio::test]
async fn a_worker_at_a_limit_is_replaced_at_once() {
    let h = Harness::new(builder().config(
        "fleet.supervisors.default",
        json!({ "queues": ["default"], "max_processes": 1 }),
    ))
    .await;
    let master = Running::start(&h).await;
    eventually("one worker", || async {
        master.spawner.running().len() == 1
    })
    .await;
    for reason in [
        ExitReason::MaxJobs,
        ExitReason::MaxMemory,
        ExitReason::MaxTime,
    ] {
        let current = master.spawner.running()[0];
        let started = Instant::now();
        master.spawner.exit(current, reason);
        eventually("a fresh worker", || async {
            master.spawner.running().len() == 1 && master.spawner.running()[0] != current
        })
        .await;
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    let summary = master.stop().await;
    assert_eq!(summary.recycled, 3);
    assert_eq!(summary.started, 4);
    assert_eq!(summary.crashed, 0);
}

#[tokio::test]
async fn a_failing_worker_is_restarted_with_growing_delays() {
    let h = Harness::new(builder().config(
        "fleet.supervisors.default",
        json!({ "queues": ["default"], "max_processes": 1 }),
    ))
    .await;
    let master = Running::start(&h).await;
    eventually("one worker", || async {
        master.spawner.running().len() == 1
    })
    .await;

    // 100 ms, then 200 ms, then 400 ms (the maximum here).
    let mut gaps = Vec::new();
    for _ in 0..4 {
        let current = master.spawner.running()[0];
        let crashed = Instant::now();
        master.spawner.exit(
            current,
            ExitReason::Crashed {
                error: "segfault".into(),
            },
        );
        eventually("a restart", || async {
            master
                .spawner
                .running()
                .first()
                .is_some_and(|i| *i != current)
        })
        .await;
        gaps.push(crashed.elapsed());
    }
    assert!(gaps[0] >= Duration::from_millis(100), "{gaps:?}");
    assert!(gaps[1] >= Duration::from_millis(200), "{gaps:?}");
    assert!(gaps[2] >= Duration::from_millis(400), "{gaps:?}");
    assert!(gaps[3] >= Duration::from_millis(400), "{gaps:?}");
    assert!(
        gaps[3] < Duration::from_millis(800) + Duration::from_secs(1),
        "capped: {gaps:?}"
    );

    // A worker that cannot even start counts as a failure too.
    master.spawner.fail_next(1);
    let current = master.spawner.running()[0];
    master
        .spawner
        .exit(current, ExitReason::Crashed { error: "x".into() });
    eventually("a restart after the failed spawn", || async {
        master.spawner.running().len() == 1
    })
    .await;
    let summary = master.stop().await;
    assert_eq!(summary.crashed, 6);
}

#[tokio::test]
async fn terminate_stops_every_running_master() {
    let h = Harness::new(builder()).await;
    let one = Running::start(&h).await;
    let two = Running::start(&h).await;
    eventually("two masters", || async {
        h.fleet.masters().await.unwrap().len() == 2
    })
    .await;

    h.fleet.terminate().await.unwrap();
    for master in [one, two] {
        let spawner = master.spawner.clone();
        tokio::time::timeout(Duration::from_secs(10), master.task)
            .await
            .expect("terminated")
            .unwrap()
            .unwrap();
        assert!(spawner.running().is_empty());
    }
    assert!(h.fleet.masters().await.unwrap().is_empty());

    // A master started after the terminate keeps running.
    let three = Running::start(&h).await;
    eventually("a worker", || async { three.spawner.running().len() == 1 }).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!three.task.is_finished());
    three.stop().await;
}

#[tokio::test]
async fn reported_jobs_are_stored_and_snapshotted() {
    let h = Harness::new(builder().config("fleet.snapshot_secs", 1)).await;
    let master = Running::start(&h).await;
    eventually("a worker", || async { master.spawner.running().len() == 1 }).await;
    let job = |id: &str, outcome, ms| CompletedJob {
        job_id: id.into(),
        queue: "default".into(),
        job_type: "test.record".into(),
        attempt: 1,
        outcome,
        runtime_ms: ms,
        finished_at: h.fleet.now(),
    };
    master.spawner.report(
        0,
        WorkerReport::Jobs {
            jobs: vec![
                job("a", AttemptOutcome::Succeeded, 100),
                job("b", AttemptOutcome::Failed, 300),
            ],
        },
    );
    eventually("the jobs are stored", || async {
        h.fleet.completed_jobs(None).await.unwrap().len() == 2
    })
    .await;
    eventually("a snapshot", || async {
        h.fleet
            .metrics(MetricsPeriod::Hour)
            .await
            .unwrap()
            .iter()
            .any(|s| s.kind == MetricKind::Job && s.name == "test.record")
    })
    .await;
    let series = h.fleet.metrics(MetricsPeriod::Hour).await.unwrap();
    let job_series = series.iter().find(|s| s.kind == MetricKind::Job).unwrap();
    let processed: u64 = job_series.points.iter().map(|p| p.processed).sum();
    let failed: u64 = job_series.points.iter().map(|p| p.failed).sum();
    assert_eq!((processed, failed), (2, 1));
    assert!(
        series
            .iter()
            .any(|s| s.kind == MetricKind::Queue && s.name == "default")
    );
    assert_eq!(h.fleet.overview().await.unwrap().jobs_past_hour, 2);
    let summary = master.stop().await;
    assert_eq!(summary.jobs, 2);
}

/// Collects the long-wait events.
#[derive(Default)]
struct Inbox(Mutex<Vec<navree_fleet::LongWaitDetected>>);

struct Collector(Arc<Inbox>);

impl Injectable for Collector {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(Collector(r.get::<Inbox>().await?)) })
    }
}

impl Listener<navree_fleet::LongWaitDetected> for Collector {
    fn handle<'a>(
        &'a self,
        event: &'a navree_fleet::LongWaitDetected,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        self.0.0.lock().unwrap().push(event.clone());
        Box::pin(async { Ok(()) })
    }
}

struct OpsModule;

impl Module for OpsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.listen::<navree_fleet::LongWaitDetected, Collector>();
    }
}

#[tokio::test]
async fn long_waits_raise_one_event_per_cooldown_across_masters() {
    let inbox = Arc::new(Inbox::default());
    let (h, clock) = Harness::with_clock(
        builder()
            .config("fleet.long_wait.mail", 60)
            .module(EventsModule)
            .module(OpsModule)
            .override_value(inbox.clone()),
    )
    .await;
    h.queue
        .dispatch(SendDigest {
            user: "ada".into(),
            api_token: "t".into(),
        })
        .await
        .unwrap();
    clock.advance(Duration::from_secs(120));
    let one = Running::start(&h).await;
    let two = Running::start(&h).await;
    eventually("an event", || async { !inbox.0.lock().unwrap().is_empty() }).await;
    // Several heartbeats of both masters later, still one.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let events = inbox.0.lock().unwrap().clone();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].queue, "mail");
    assert_eq!(events[0].wait_secs, 120);
    assert_eq!(events[0].threshold_secs, 60);

    // After the cooldown (15 minutes), the next one.
    clock.advance(Duration::from_secs(15 * 60));
    eventually("a second event", || async {
        inbox.0.lock().unwrap().len() == 2
    })
    .await;
    assert_eq!(inbox.0.lock().unwrap()[1].wait_secs, 120 + 15 * 60);
    one.stop().await;
    two.stop().await;
}

#[tokio::test]
async fn database_errors_are_survived_and_the_workers_keep_running() {
    let h = Harness::new(builder().config("fleet.snapshot_secs", 1)).await;
    let master = Running::start(&h).await;
    eventually("a worker", || async { master.spawner.running().len() == 1 }).await;
    let db = h.app.get::<navree_db::Db>().await;
    for table in ["fleet_state", "fleet_metrics", "fleet_jobs"] {
        sea_orm::ConnectionTrait::execute_raw(
            db.conn(),
            sea_orm::Statement::from_string(db.backend(), format!("DROP TABLE {table}")),
        )
        .await
        .unwrap();
    }
    master.spawner.report(
        0,
        WorkerReport::Jobs {
            jobs: vec![CompletedJob {
                job_id: "j".into(),
                queue: "default".into(),
                job_type: "test.record".into(),
                attempt: 1,
                outcome: AttemptOutcome::Succeeded,
                runtime_ms: 5,
                finished_at: h.fleet.now(),
            }],
        },
    );
    // Every tick fails to read the state, store jobs or snapshot; the
    // master logs it and goes on.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!master.task.is_finished());
    assert_eq!(master.spawner.running().len(), 1);
    assert_eq!(master.spawner.commands(0), []);
    let summary = master.stop().await;
    assert_eq!(summary.jobs, 1);
}
