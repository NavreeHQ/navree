//! Names every public item the Fleet docs page
//! (`docs/tools/fleet.mdx`) documents, so a rename
//! fails the build: the module, the service and its methods, the config
//! keys, the commands, the tables, the routes, the gauges, the event and
//! the test helpers.

use std::collections::BTreeSet;

use navree_access::{AccessModule, AccessOptions, Subject};
use navree_config::{ConfigModule, ConfigSources};
use navree_db::{DatabaseModule, Db, Executor as _, Statement};
use navree_events::Event;
use navree_fleet::testing::FakeSpawner;
use navree_fleet::{
    Balance, FAILED_JOBS, Fleet, FleetModule, FleetOptions, FleetSubject, InProcessSpawner,
    JOB_RUNTIME, LongWaitDetected, Master, MasterSettings, MetricsPeriod, OPERATE, PROCESSES,
    ProcessSpawner, QUEUE_PENDING, QUEUE_THROUGHPUT, QUEUE_WAIT, QueueWorkload, Supervisor,
    SupervisorOptions, VIEW, initial_allocation, next_allocation, target_allocation,
};
use navree_kernel::{Module, Navree};
use navree_queue::{QueueModule, QueueOptions};
use serde_json::json;

use crate::support::{Harness, builder};

#[test]
fn the_module_service_and_helpers_exist() {
    let _: fn() -> _ = FleetModule::from_config;
    let _: fn(FleetOptions) -> _ = FleetModule::for_root;
    fn is_module(_: &impl Module) {}
    is_module(&FleetModule);
    let _ = (Balance::Auto, Balance::Simple, Balance::Off);
    let _ = (MetricsPeriod::Hour, MetricsPeriod::Day, MetricsPeriod::Week);
    let _ = FakeSpawner::new();
    let _ = MasterSettings::default();
    let _: fn(_) -> _ = InProcessSpawner::new;
    let _ = ProcessSpawner::current_exe();
    let _: fn(_) -> _ = Master::new;
    let s = Supervisor::resolve(
        "default",
        &SupervisorOptions::new(["mail"]),
        &QueueOptions::default(),
    );
    let work = [QueueWorkload::new("mail", 10, 0, 3, 100)];
    let start = initial_allocation(&s);
    let _ = target_allocation(&s, &work);
    let _ = next_allocation(&s, &start, &work, 1);

    assert_eq!(LongWaitDetected::NAME, "fleet.long_wait");
    assert_eq!(FleetSubject::SUBJECT, "Fleet");
    assert_eq!(FleetSubject::PERMISSION_PREFIX, "fleet");
    assert_eq!((VIEW, OPERATE), ("view", "operate"));
    assert_eq!(
        [
            QUEUE_PENDING,
            QUEUE_WAIT,
            QUEUE_THROUGHPUT,
            JOB_RUNTIME,
            PROCESSES,
            FAILED_JOBS
        ],
        [
            "fleet_queue_pending",
            "fleet_queue_wait_seconds",
            "fleet_queue_throughput_per_minute",
            "fleet_job_runtime_seconds",
            "fleet_processes",
            "fleet_failed_jobs"
        ]
    );
}

#[test]
fn every_documented_config_key_is_read() {
    let options: FleetOptions = serde_json::from_value(json!({
        "dashboard": false,
        "path": "/ops",
        "balance_cooldown_secs": 5,
        "balance_max_shift": 2,
        "snapshot_secs": 30,
        "trim": { "recent_minutes": 30, "metrics_days": 3 },
        "long_wait": { "mail": 30 },
        "long_wait_cooldown_minutes": 5,
        "only": "default",
        "supervisors": {
            "default": {
                "queues": "default,mail",
                "balance": "simple",
                "min_processes": 1,
                "max_processes": 4,
                "concurrency": 2,
                "max_jobs": 10,
                "max_memory_mb": 128,
                "max_time_secs": 60
            }
        }
    }))
    .unwrap();
    assert!(!options.dashboard);
    assert_eq!(options.path, "/ops");
    assert_eq!(options.balance_cooldown_secs, 5);
    assert_eq!(options.balance_max_shift, 2);
    assert_eq!(options.snapshot_secs, 30);
    assert_eq!(options.trim.recent_minutes, 30);
    assert_eq!(options.trim.metrics_days, 3);
    assert_eq!(options.long_wait["mail"], 30);
    assert_eq!(options.long_wait_cooldown_minutes, 5);
    assert_eq!(options.only, ["default"]);
    let default = &options.supervisors["default"];
    assert_eq!(default.queues, ["default", "mail"]);
    assert_eq!(default.balance, Balance::Simple);
    assert_eq!(default.concurrency, Some(2));
    assert_eq!(
        (
            default.max_jobs,
            default.max_memory_mb,
            default.max_time_secs
        ),
        (10, 128, 60)
    );
}

#[test]
fn the_documented_commands_are_registered() {
    let report = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(FleetModule::from_config())
        .validate()
        .expect("the graph is valid");
    let commands: BTreeSet<&str> = report
        .modules
        .iter()
        .flat_map(|m| m.commands.iter().map(|c| c.name))
        .collect();
    for command in [
        "fleet",
        "fleet:status",
        "fleet:pause",
        "fleet:continue",
        "fleet:terminate",
        "fleet:forget",
        "fleet:work",
    ] {
        assert!(commands.contains(command), "{command} in {commands:?}");
    }
}

#[tokio::test]
async fn the_migrations_create_the_documented_tables() {
    let h = Harness::new(builder()).await;
    let db = h.app.get::<Db>().await;
    assert_eq!(
        navree_db::Executor::backend(&*db),
        crate::database::Server::current().backend(),
        "the suite runs on the configured server"
    );
    for table in [
        "fleet_masters",
        "fleet_state",
        "fleet_jobs",
        "fleet_metrics",
    ] {
        db.fetch_all(&Statement::text(format!("SELECT COUNT(*) FROM {table}")))
            .await
            .unwrap_or_else(|error| panic!("{table}: {error}"));
    }
    let _ = h.app.get::<Fleet>().await;
}

#[tokio::test]
async fn the_documented_routes_are_mounted() {
    let h = Harness::new(
        builder()
            .config("fleet.dashboard", true)
            .module(AccessModule::for_root(AccessOptions::default())),
    )
    .await;
    let routes = navree_http::route_table(h.app.ctx()).await.unwrap();
    let mounted: BTreeSet<String> = routes
        .iter()
        .map(|r| format!("{} {}", r.method, r.full_path))
        .collect();
    for route in [
        "GET /_fleet",
        "GET /_fleet/supervisors",
        "GET /_fleet/jobs",
        "GET /_fleet/jobs/{id}",
        "GET /_fleet/failed",
        "GET /_fleet/failed/{id}",
        "GET /_fleet/metrics",
        "POST /_fleet/pause",
        "POST /_fleet/continue",
        "POST /_fleet/failed/retry",
        "POST /_fleet/failed/{id}/retry",
        "POST /_fleet/failed/{id}/forget",
        "GET /_fleet/api/overview",
        "GET /_fleet/api/masters",
        "GET /_fleet/api/jobs",
        "GET /_fleet/api/jobs/{id}",
        "GET /_fleet/api/failed",
        "GET /_fleet/api/failed/{id}",
        "GET /_fleet/api/metrics",
        "POST /_fleet/api/failed/{id}/retry",
        "POST /_fleet/api/failed/retry",
        "DELETE /_fleet/api/failed/{id}",
        "POST /_fleet/api/pause",
        "POST /_fleet/api/continue",
    ] {
        assert!(mounted.contains(route), "{route} in {mounted:?}");
    }
}
