//! A real master with real worker processes: this test binary is its own
//! app. Started with a command (`fleet:work ...`), `main` runs the app as a
//! worker would; started by `cargo test`, it runs the test, whose master
//! spawns this binary as its workers. Everything goes through the shared
//! SQLite database and the workers' stdin and stdout.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{BoxError, Job, JobContext, ModuleBuilderJobsExt};
use navree_db::DatabaseModule;
use navree_fleet::{Fleet, FleetModule, Master, MasterSettings, ProcessSpawner};
use navree_kernel::{AppBuilder, Module, ModuleBuilder, Navree};
use navree_queue::{Queue, QueueModule};
use navree_testing::TestApp;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[path = "fleet/database.rs"]
mod database;

/// Takes 20 ms; on queue `default`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Work {
    n: u32,
}

impl Job for Work {
    const NAME: &'static str = "test.work";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(())
    }
}

struct JobsModule;

impl Module for JobsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Work>();
    }
}

/// The worker app: configured from `APP_*` variables the master passes.
fn worker_app() -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().env()))
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(FleetModule::from_config())
        .module(JobsModule)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|command| command.starts_with("fleet"))
    {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        return match runtime.block_on(worker_app().run_with_args(args)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("worker failed: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    runtime.block_on(a_master_supervises_real_worker_processes());
    println!("test a_master_supervises_real_worker_processes ... ok");
    println!("\ntest result: ok. 1 passed; 0 failed");
    ExitCode::SUCCESS
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(60);
    while !check().await {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn a_master_supervises_real_worker_processes() {
    let (url, _guard) = database::scratch();
    let app = TestApp::builder()
        .config("database.url", url.clone())
        .config("database.migrate_on_boot", true)
        .config("queue.poll_interval_ms", 20)
        .config("fleet.dashboard", false)
        .config(
            "fleet.supervisors.default",
            json!({ "queues": ["default"], "max_processes": 2, "concurrency": 2, "max_jobs": 4 }),
        )
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(FleetModule::from_config())
        .module(JobsModule)
        .build()
        .await;
    let fleet = app.get::<Fleet>().await;
    let queue = app.get::<Queue>().await;
    for n in 0..10 {
        queue.dispatch(Work { n }).await.unwrap();
    }

    let spawner = ProcessSpawner::current_exe()
        .unwrap()
        .env("APP_DATABASE__URL", &url)
        .env("APP_QUEUE__POLL_INTERVAL_MS", "20")
        .env("APP_FLEET__DASHBOARD", "false");
    let master = Master::new(fleet.clone())
        .unwrap()
        .with_spawner(Arc::new(spawner))
        .with_settings(MasterSettings {
            name: "e2e".into(),
            tick: Duration::from_millis(50),
            heartbeat: Duration::from_millis(100),
            worker_heartbeat: Duration::from_millis(100),
            ..MasterSettings::default()
        });
    let stop = CancellationToken::new();
    let task = tokio::spawn(master.run(stop.clone()));

    // The workers run every job and report each attempt.
    eventually("every job ran and was reported", || async {
        fleet.completed_jobs(None).await.unwrap().len() == 10
    })
    .await;
    assert_eq!(queue.pending_count("default").await.unwrap(), 0);
    eventually("processes with pids and memory", || async {
        let masters = fleet.masters().await.unwrap();
        let processes: Vec<_> = masters
            .iter()
            .flat_map(|m| &m.supervisors)
            .flat_map(|s| &s.processes)
            .cloned()
            .collect();
        // An idle `auto` supervisor keeps `min_processes` (1).
        !processes.is_empty()
            && processes.iter().all(|p| {
                p.pid.is_some_and(|pid| pid != std::process::id()) && p.memory_mb.is_some()
            })
    })
    .await;

    // Paused workers take nothing; continued, they run the backlog.
    fleet.pause(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    for n in 10..13 {
        queue.dispatch(Work { n }).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(queue.pending_count("default").await.unwrap(), 3, "paused");
    fleet.resume(None).await.unwrap();
    eventually("the backlog ran", || async {
        fleet.completed_jobs(None).await.unwrap().len() == 13
    })
    .await;

    stop.cancel();
    let summary = tokio::time::timeout(Duration::from_secs(60), task)
        .await
        .expect("the master stops")
        .unwrap()
        .expect("the master ran");
    assert!(
        summary.recycled >= 1,
        "max_jobs = 4 recycles workers: {summary:?}"
    );
    assert_eq!(summary.crashed, 0, "{summary:?}");
    assert_eq!(summary.jobs, 13, "{summary:?}");
    assert!(fleet.masters().await.unwrap().is_empty());
    app.shutdown().await.unwrap();
}
