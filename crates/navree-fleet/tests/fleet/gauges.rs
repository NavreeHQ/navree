//! Fleet's gauges on `GET /metrics`, read from the database when it is
//! scraped.

use std::sync::Arc;

use navree_fleet::testing::FakeSpawner;
use navree_fleet::{CompletedJob, Master, WorkerReport};
use navree_observe::{ObserveModule, ObserveOptions};
use navree_queue::{AttemptOutcome, WorkOptions};
use tokio_util::sync::CancellationToken;

use crate::support::{Export, Harness, Record, builder, eventually, fast};

/// The value of the sample `name{labels}` (or `name` without labels).
fn sample(body: &str, series: &str) -> Option<f64> {
    body.lines()
        .find_map(|line| line.strip_prefix(&format!("{series} ")))
        .and_then(|value| value.trim().parse().ok())
}

#[tokio::test]
async fn the_metrics_route_shows_fleets_numbers() {
    let h =
        Harness::new(builder().module(ObserveModule::for_root(ObserveOptions::default()))).await;
    for i in 0..2 {
        h.queue
            .dispatch(Record::new(&format!("r{i}")))
            .await
            .unwrap();
    }
    h.queue
        .dispatch(Export {
            contact: "anu".into(),
            password: "p".into(),
        })
        .await
        .unwrap();
    h.queue
        .work(WorkOptions {
            queues: vec!["exports".into()],
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();
    let job = |ms| CompletedJob {
        job_id: format!("j{ms}"),
        queue: "default".into(),
        job_type: "test.record".into(),
        attempt: 1,
        outcome: AttemptOutcome::Succeeded,
        runtime_ms: ms,
        finished_at: h.fleet.now(),
    };
    h.fleet
        .record(None, &[job(100), job(200), job(3000)])
        .await
        .unwrap();
    let spawner = Arc::new(FakeSpawner::new());
    let master = Master::from_app(h.app.ctx())
        .await
        .unwrap()
        .with_spawner(spawner.clone())
        .with_settings(fast());
    let stop = CancellationToken::new();
    let task = tokio::spawn(master.run(stop.clone()));
    eventually("a heartbeat", || async {
        !h.fleet.masters().await.unwrap().is_empty()
    })
    .await;
    spawner.report(
        0,
        WorkerReport::Heartbeat {
            jobs: 0,
            memory_mb: None,
        },
    );

    let body = h
        .app
        .client()
        .get("/metrics")
        .send()
        .await
        .assert_status(200)
        .text();
    assert_eq!(
        sample(&body, r#"fleet_queue_pending{queue="default"}"#),
        Some(2.0),
        "{body}"
    );
    assert!(
        sample(&body, r#"fleet_queue_wait_seconds{queue="default"}"#).is_some(),
        "{body}"
    );
    assert_eq!(
        sample(
            &body,
            r#"fleet_queue_throughput_per_minute{queue="default"}"#
        ),
        Some(3.0),
        "{body}"
    );
    assert_eq!(
        sample(&body, r#"fleet_job_runtime_seconds{job="test.record"}"#),
        Some(3.0),
        "{body}"
    );
    assert_eq!(
        sample(
            &body,
            r#"fleet_processes{supervisor="default",queue="default"}"#
        ),
        Some(1.0),
        "{body}"
    );
    assert_eq!(sample(&body, "fleet_failed_jobs"), Some(1.0), "{body}");

    stop.cancel();
    task.await.unwrap().unwrap();
    let body = h.app.client().get("/metrics").send().await.text();
    assert_eq!(
        sample(
            &body,
            r#"fleet_processes{supervisor="default",queue="default"}"#
        ),
        Some(0.0),
        "a stopped master's processes go to zero: {body}"
    );
}
