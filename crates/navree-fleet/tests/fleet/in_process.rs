//! Masters with real workers running as tasks of the test app
//! (`InProcessSpawner`): jobs run, are reported and stored; pausing holds
//! the workers of every master; limits recycle workers.

use std::sync::Arc;
use std::time::Duration;

use navree_fleet::{InProcessSpawner, Master, MasterSummary};
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::support::{Harness, Record, SendDigest, Slow, builder, eventually, fast};

struct Running {
    stop: CancellationToken,
    task: JoinHandle<Result<MasterSummary, navree_fleet::FleetError>>,
}

impl Running {
    async fn start(h: &Harness) -> Self {
        let master = Master::from_app(h.app.ctx())
            .await
            .expect("a master")
            .with_spawner(Arc::new(InProcessSpawner::new(h.app.ctx().clone())))
            .with_settings(fast());
        let stop = CancellationToken::new();
        let task = tokio::spawn(master.run(stop.clone()));
        Self { stop, task }
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

fn two_queues() -> serde_json::Value {
    json!({ "default": { "queues": ["default", "mail"], "max_processes": 2, "concurrency": 2 } })
}

#[tokio::test]
async fn workers_run_jobs_and_the_master_stores_them() {
    let h = Harness::new(builder().config("fleet.supervisors", two_queues())).await;
    let master = Running::start(&h).await;
    for i in 0..5 {
        h.queue
            .dispatch(Record::new(&format!("r{i}")))
            .await
            .unwrap();
    }
    h.queue
        .dispatch(SendDigest {
            user: "anu".into(),
            api_token: "t".into(),
        })
        .await
        .unwrap();
    eventually("every job ran", || async { h.journal.count() == 6 }).await;
    eventually("every attempt is stored", || async {
        h.fleet.completed_jobs(None).await.unwrap().len() == 6
    })
    .await;
    let completed = h.fleet.completed_jobs(Some("mail")).await.unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].job_type, "mail.digest");
    eventually("the heartbeat counts the jobs", || async {
        let masters = h.fleet.masters().await.unwrap();
        masters.len() == 1
            && masters[0]
                .supervisors
                .iter()
                .flat_map(|s| &s.processes)
                .map(|p| p.jobs)
                .sum::<u64>()
                == 6
    })
    .await;
    let workload = h.fleet.workload().await.unwrap();
    assert_eq!(workload[0].jobs_per_minute, 5);
    assert_eq!(workload[0].processes, 1);

    let summary = master.stop().await;
    assert_eq!(summary.jobs, 6);
    assert_eq!(summary.started, 2);
}

#[tokio::test]
async fn a_pause_holds_the_workers_of_every_master() {
    let h = Harness::new(builder()).await;
    let one = Running::start(&h).await;
    let two = Running::start(&h).await;
    eventually("two masters", || async {
        h.fleet.masters().await.unwrap().len() == 2
    })
    .await;
    h.queue.dispatch(Record::new("before")).await.unwrap();
    eventually("the first job ran", || async { h.journal.count() == 1 }).await;

    h.fleet.pause(None).await.unwrap();
    eventually("both masters paused", || async {
        let masters = h.fleet.masters().await.unwrap();
        masters.len() == 2
            && masters
                .iter()
                .all(|m| m.supervisors.iter().all(|s| s.paused))
    })
    .await;
    // Give the workers time to hear about it.
    tokio::time::sleep(Duration::from_millis(100)).await;
    for i in 0..3 {
        h.queue
            .dispatch(Record::new(&format!("held{i}")))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(h.journal.count(), 1, "nothing ran while paused");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 3);

    h.fleet.resume(None).await.unwrap();
    eventually("the held jobs ran", || async { h.journal.count() == 4 }).await;
    one.stop().await;
    two.stop().await;
}

#[tokio::test]
async fn workers_are_recycled_after_max_jobs() {
    let h = Harness::new(builder().config(
        "fleet.supervisors.default",
        json!({ "queues": ["default"], "max_processes": 1, "concurrency": 1, "max_jobs": 2 }),
    ))
    .await;
    let master = Running::start(&h).await;
    for i in 0..5 {
        h.queue
            .dispatch(Record::new(&format!("r{i}")))
            .await
            .unwrap();
    }
    eventually("every job ran", || async { h.journal.count() == 5 }).await;
    let summary = master.stop().await;
    assert!(summary.recycled >= 2, "{summary:?}");
    assert!(summary.started >= 3, "{summary:?}");
    assert_eq!(summary.crashed, 0);
}

#[tokio::test]
async fn stopping_lets_running_jobs_finish() {
    let h = Harness::new(builder()).await;
    let master = Running::start(&h).await;
    h.queue
        .dispatch(Slow {
            label: "slow".into(),
            ms: 400,
        })
        .await
        .unwrap();
    eventually("the job is running", || async {
        h.queue.status("default").await.unwrap().running == 1
    })
    .await;
    master.stop().await;
    assert_eq!(
        h.journal.labels(),
        ["slow"],
        "the job finished before the master stopped"
    );
}
