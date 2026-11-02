//! The `Fleet` service: pausing, workload, long waits, failed jobs,
//! completed jobs, metrics and trimming.

use std::time::Duration;

use chrono::TimeDelta;
use navree_fleet::{
    Balance, CompletedJob, FleetError, FleetStatus, LongWaitDetected, MetricKind, MetricSnapshot,
    MetricsPeriod, RECENT_FAILURES, REDACTED, THROUGHPUT_MINUTES,
};
use navree_queue::{AttemptOutcome, QueueOptions};
use serde_json::json;

use crate::support::{Export, Harness, Record, SendDigest, builder, start};

fn supervisors() -> serde_json::Value {
    json!({
        "default": { "queues": ["default", "mail"], "max_processes": 4 },
        "exports": { "queues": ["exports"], "balance": "off", "max_processes": 2 }
    })
}

#[tokio::test]
async fn pausing_one_or_every_supervisor_is_stored_for_every_server() {
    let h = Harness::new(builder().config("fleet.supervisors", supervisors())).await;
    let fleet = &h.fleet;
    assert!(fleet.paused().await.unwrap().is_empty());

    fleet.pause(Some("exports")).await.unwrap();
    assert!(fleet.is_paused("exports").await.unwrap());
    assert!(!fleet.is_paused("default").await.unwrap());

    fleet.pause(None).await.unwrap();
    assert_eq!(
        fleet
            .paused()
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        ["default", "exports"]
    );
    fleet.resume(Some("default")).await.unwrap();
    assert_eq!(
        fleet
            .paused()
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        ["exports"]
    );
    fleet.resume(None).await.unwrap();
    assert!(fleet.paused().await.unwrap().is_empty());

    assert!(matches!(
        fleet.pause(Some("nope")).await,
        Err(FleetError::UnknownSupervisor(name)) if name == "nope"
    ));
    assert!(matches!(
        fleet.resume(Some("nope")).await,
        Err(FleetError::UnknownSupervisor(_))
    ));
}

#[tokio::test]
async fn without_masters_fleet_is_inactive() {
    let h = Harness::new(builder()).await;
    assert_eq!(h.fleet.status().await.unwrap(), FleetStatus::Inactive);
    assert!(h.fleet.masters().await.unwrap().is_empty());
    let names: Vec<_> = h
        .fleet
        .supervisors()
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["default"],
        "an app without supervisors gets `default`"
    );
    assert_eq!(h.fleet.queues(), ["default"]);
}

#[tokio::test]
async fn only_must_name_a_supervisor() {
    let error = builder()
        .config("fleet.supervisors", supervisors())
        .config("fleet.only", "imports")
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("imports"), "{error}");
}

#[tokio::test]
async fn an_invalid_supervisor_fails_the_boot_with_its_name() {
    let error = builder()
        .config(
            "fleet.supervisors.busy",
            json!({ "queues": ["a", "b", "c"], "min_processes": 2, "max_processes": 4 }),
        )
        .try_build()
        .await
        .map(|_| ())
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("fleet.supervisors.busy"), "{error}");
}

/// The docs' example: a mail queued two minutes ago on a queue with a
/// 60-second threshold is a long wait.
#[tokio::test]
async fn a_mail_waiting_two_minutes_is_a_long_wait() {
    let (h, clock) = Harness::with_clock(
        builder()
            .config("fleet.long_wait.mail", 60)
            .config("fleet.long_wait.default", 600),
    )
    .await;
    h.queue
        .dispatch(SendDigest {
            user: "ada".into(),
            api_token: "t".into(),
        })
        .await
        .unwrap();
    clock.advance(Duration::from_secs(60));
    assert!(
        h.fleet.long_waits().await.unwrap().is_empty(),
        "60 s is not longer than 60 s"
    );
    clock.advance(Duration::from_secs(60));

    let late = h.fleet.long_waits().await.unwrap();
    assert_eq!(
        late,
        [LongWaitDetected {
            queue: "mail".into(),
            wait_secs: 120,
            threshold_secs: 60,
        }]
    );
}

#[tokio::test]
async fn the_workload_counts_pending_jobs_runtimes_and_throughput() {
    let (h, clock) = Harness::with_clock(
        builder()
            .config("fleet.supervisors", supervisors())
            .config("fleet.long_wait.default", 30),
    )
    .await;
    for i in 0..3 {
        h.queue
            .dispatch(Record::new(&format!("r{i}")))
            .await
            .unwrap();
    }
    h.queue
        .later(Record::new("later"), Duration::from_secs(600))
        .await
        .unwrap();
    clock.advance(Duration::from_secs(45));
    // Attempts the masters stored: two `default` jobs of 400 and 600 ms a
    // minute and a half ago, one 30 seconds ago, and an old one.
    let job = |queue: &str, ms: u64, ago: i64| CompletedJob {
        job_id: format!("{queue}-{ms}-{ago}"),
        queue: queue.into(),
        job_type: "test.record".into(),
        attempt: 1,
        outcome: AttemptOutcome::Succeeded,
        runtime_ms: ms,
        finished_at: h.fleet.now() - TimeDelta::seconds(ago),
    };
    h.fleet
        .record(
            Some("m1"),
            &[
                job("default", 400, 90),
                job("default", 600, 90),
                job("default", 500, 30),
                job("default", 9000, 600),
            ],
        )
        .await
        .unwrap();

    let workload = h.fleet.workload().await.unwrap();
    let queues: Vec<_> = workload.iter().map(|w| w.queue.as_str()).collect();
    assert_eq!(queues, ["default", "mail", "exports"]);
    let default = &workload[0];
    assert_eq!(
        (default.waiting, default.delayed, default.running),
        (3, 1, 0)
    );
    assert_eq!(default.pending, 4);
    assert_eq!(default.oldest_wait_secs, 45);
    assert_eq!(default.avg_runtime_ms, 500, "the last five minutes only");
    assert!(
        (default.workload_secs - 1.5).abs() < 1e-9,
        "3 jobs x 500 ms"
    );
    assert_eq!(default.jobs_per_minute, 1);
    assert_eq!(default.processes, 0, "no master runs");
    assert_eq!(default.long_wait_threshold_secs, Some(30));
    assert!(default.over_threshold);
    let mail = &workload[1];
    assert_eq!(mail.pending, 0);
    assert!(!mail.over_threshold);

    let overview = h.fleet.overview().await.unwrap();
    assert_eq!(overview.status, FleetStatus::Inactive);
    assert_eq!(overview.jobs_per_minute, 1);
    assert_eq!(overview.jobs_past_hour, 4);
    assert_eq!(overview.longest_wait.as_ref().unwrap().queue, "default");
    assert_eq!(overview.longest_wait.as_ref().unwrap().wait_secs, 45);
    assert_eq!(overview.workload.len(), 3);
}

#[tokio::test]
async fn the_overview_groups_queues_by_supervisor_and_charts_the_last_half_hour() {
    let (h, _clock) =
        Harness::with_clock(builder().config("fleet.supervisors", supervisors())).await;
    let snapshot = |kind, name: &str, processed, ago_secs: i64| MetricSnapshot {
        at: h.fleet.now() - TimeDelta::seconds(ago_secs),
        kind,
        name: name.into(),
        processed,
        failed: 0,
        runtime_avg_ms: 20,
        runtime_p95_ms: 40,
        pending: None,
        wait_ms: None,
    };
    h.fleet
        .record_snapshots(
            Some("m1"),
            &[
                snapshot(MetricKind::Queue, "mail", 12, 30),
                snapshot(MetricKind::Queue, "mail", 40, 95),
                snapshot(MetricKind::Queue, "mail", 5, 29 * 60),
                // Two snapshots in one minute add up.
                snapshot(MetricKind::Queue, "mail", 3, 605),
                snapshot(MetricKind::Queue, "mail", 4, 615),
                // Outside the half hour, and not a queue: left out.
                snapshot(MetricKind::Queue, "mail", 99, 31 * 60),
                snapshot(MetricKind::Job, "mail.send", 77, 30),
                snapshot(MetricKind::Queue, "default", 8, 61),
            ],
        )
        .await
        .unwrap();

    let overview = h.fleet.overview().await.unwrap();
    let names: Vec<_> = overview
        .supervisors
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["default", "exports"]);
    let default = &overview.supervisors[0];
    assert_eq!(default.queues, ["default", "mail"]);
    assert_eq!(default.balance, Balance::Auto);
    assert_eq!((default.min_processes, default.max_processes), (1, 4));
    assert_eq!(default.processes_label, "1-4 processes");
    assert!(!default.paused);
    assert_eq!((default.processes, default.slots), (0, 4), "no master runs");
    let exports = &overview.supervisors[1];
    assert_eq!(exports.balance, Balance::Off);
    assert_eq!(exports.processes_label, "2 processes");
    assert!(overview.masters.is_empty());

    let queues: Vec<_> = overview
        .throughput
        .iter()
        .map(|t| t.queue.as_str())
        .collect();
    assert_eq!(queues, ["default", "mail", "exports"]);
    let mail = &overview.throughput[1].per_minute;
    assert_eq!(mail.len(), THROUGHPUT_MINUTES);
    assert_eq!(mail[29], 12, "the last minute");
    assert_eq!(mail[28], 40);
    assert_eq!(mail[19], 7, "two snapshots of one minute");
    assert_eq!(mail[1], 5);
    assert_eq!(mail.iter().sum::<u64>(), 12 + 40 + 7 + 5);
    let default_queue = &overview.throughput[0].per_minute;
    assert_eq!(default_queue[28], 8);
    assert_eq!(default_queue.iter().sum::<u64>(), 8);
    assert_eq!(
        overview.throughput[2].per_minute,
        vec![0; THROUGHPUT_MINUTES]
    );

    fleet_paused_is_shown(&h).await;
}

async fn fleet_paused_is_shown(h: &Harness) {
    h.fleet.pause(Some("exports")).await.unwrap();
    let overview = h.fleet.overview().await.unwrap();
    assert!(!overview.supervisors[0].paused);
    assert!(overview.supervisors[1].paused);
}

#[tokio::test]
async fn the_overview_lists_the_newest_failed_jobs_with_their_attempts() {
    let h = Harness::new(builder()).await;
    let empty = h.fleet.overview().await.unwrap();
    assert_eq!(empty.failed_total, 0);
    assert!(empty.recent_failures.is_empty());

    let ids = fail_exports(&h, &["a", "b", "c", "d", "e", "f", "g"]).await;
    let overview = h.fleet.overview().await.unwrap();
    assert_eq!(overview.failed_total, 7);
    assert_eq!(overview.failed_past_week, 7);
    assert_eq!(overview.recent_failures.len(), RECENT_FAILURES);
    let first = &overview.recent_failures[0];
    assert_eq!(first.job_type, "contacts.export");
    assert_eq!(first.queue, "exports");
    assert_eq!((first.attempts, first.max_attempts), (1, 1));
    assert_eq!(first.error, "the mail server is down");
    let listed = h.fleet.failed_jobs(None).await.unwrap();
    assert_eq!(first.id, listed[0].id, "newest first");
    assert!(ids.contains(&first.id));
}

async fn fail_exports(h: &Harness, contacts: &[&str]) -> Vec<String> {
    let mut ids = Vec::new();
    for contact in contacts {
        ids.push(
            h.queue
                .dispatch(Export {
                    contact: (*contact).into(),
                    password: "hunter2".into(),
                })
                .await
                .unwrap(),
        );
    }
    let options = navree_queue::WorkOptions {
        queues: vec!["exports".into()],
        stop_when_empty: true,
        ..navree_queue::WorkOptions::default()
    };
    let stats = h.queue.work(options).await.unwrap();
    assert!(stats.failed >= contacts.len() as u64);
    ids
}

#[tokio::test]
async fn failed_jobs_are_searched_retried_and_forgotten() {
    let h = Harness::new(builder()).await;
    let ids = fail_exports(&h, &["ada", "grace", "linus"]).await;
    h.queue.dispatch(Record::new("fine")).await.unwrap();

    let all = h.fleet.failed_jobs(None).await.unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].error, "the mail server is down");
    for search in ["CONTACTS.export", "exports", "mail server", "grace"] {
        let found = h.fleet.failed_jobs(Some(search)).await.unwrap();
        assert!(!found.is_empty(), "{search}");
    }
    assert_eq!(h.fleet.failed_jobs(Some("grace")).await.unwrap().len(), 1);
    assert!(
        h.fleet
            .failed_jobs(Some("nothing"))
            .await
            .unwrap()
            .is_empty()
    );

    // Retry one, then forget one, then retry every match.
    h.fleet.retry(&ids[0]).await.unwrap();
    assert!(h.fleet.failed_job(&ids[0]).await.unwrap().is_none());
    assert_eq!(h.queue.pending_count("exports").await.unwrap(), 1);
    h.fleet.forget(&ids[1]).await.unwrap();
    assert!(matches!(
        h.fleet.forget(&ids[1]).await,
        Err(FleetError::FailedJobNotFound { id }) if id == ids[1]
    ));
    assert!(matches!(
        h.fleet.retry("missing").await,
        Err(FleetError::FailedJobNotFound { .. })
    ));
    assert_eq!(h.fleet.retry_all(Some("linus")).await.unwrap(), 1);
    assert!(h.fleet.failed_jobs(None).await.unwrap().is_empty());
    assert_eq!(h.queue.pending_count("exports").await.unwrap(), 2);

    // Without a search, retry all retries everything.
    fail_exports(&h, &["x"]).await;
    let failed_again = h.fleet.failed_jobs(None).await.unwrap().len() as u64;
    assert_eq!(h.fleet.retry_all(None).await.unwrap(), failed_again);
}

#[tokio::test]
async fn payloads_are_redacted_by_the_log_redact_fragments() {
    let h = Harness::new(builder()).await;
    let payload =
        json!({ "contact": "ada", "password": "hunter2", "nested": { "api_token": "x" } });
    assert_eq!(
        h.fleet.redact(&payload),
        json!({ "contact": "ada", "password": REDACTED, "nested": { "api_token": REDACTED } })
    );
    let custom = Harness::new(builder().config("log.redact", "contact")).await;
    assert_eq!(
        custom
            .fleet
            .redact(&json!({ "contact": "ada", "password": "p" })),
        json!({ "contact": REDACTED, "password": "p" })
    );
}

#[tokio::test]
async fn pending_and_completed_jobs_are_listed_and_searched() {
    let (h, clock) = Harness::with_clock(builder()).await;
    let first = h.queue.dispatch(Record::new("one")).await.unwrap();
    h.queue
        .dispatch(SendDigest {
            user: "ada".into(),
            api_token: "t".into(),
        })
        .await
        .unwrap();
    let pending = h.fleet.pending_jobs(None).await.unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(h.fleet.pending_jobs(Some("mail")).await.unwrap().len(), 1);
    assert_eq!(
        h.fleet.pending_job(&first).await.unwrap().unwrap().job_type,
        "test.record"
    );

    let job = |id: &str, queue: &str, ago_minutes: i64| CompletedJob {
        job_id: id.into(),
        queue: queue.into(),
        job_type: format!("{queue}.job"),
        attempt: 1,
        outcome: AttemptOutcome::Succeeded,
        runtime_ms: 10,
        finished_at: start() - TimeDelta::minutes(ago_minutes),
    };
    h.fleet
        .record(
            None,
            &[
                job("a", "default", 5),
                job("b", "mail", 1),
                job("c", "mail", 90),
            ],
        )
        .await
        .unwrap();
    let completed = h.fleet.completed_jobs(None).await.unwrap();
    let ids: Vec<_> = completed.iter().map(|j| j.job_id.as_str()).collect();
    assert_eq!(ids, ["b", "a"], "newest first, within the last hour");
    assert_eq!(h.fleet.completed_jobs(Some("MAIL")).await.unwrap().len(), 1);

    // Trimming drops what is older than the retention.
    h.fleet.trim().await.unwrap();
    clock.advance(Duration::from_secs(3 * 60));
    assert_eq!(h.fleet.completed_jobs(None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn metrics_are_charted_from_the_snapshots_of_every_master() {
    let (h, clock) = Harness::with_clock(builder()).await;
    let snap = |minutes_ago: i64, processed: u64| MetricSnapshot {
        at: start() - TimeDelta::minutes(minutes_ago),
        kind: MetricKind::Queue,
        name: "mail".into(),
        processed,
        failed: 1,
        runtime_avg_ms: 100,
        runtime_p95_ms: 250,
        pending: Some(3),
        wait_ms: Some(2000),
    };
    h.fleet
        .record_snapshots(Some("m1"), &[snap(2, 10), snap(30, 5), snap(30 * 60, 99)])
        .await
        .unwrap();
    h.fleet
        .record_snapshots(Some("m2"), &[snap(2, 20)])
        .await
        .unwrap();

    let hour = h.fleet.metrics(MetricsPeriod::Hour).await.unwrap();
    assert_eq!(hour.len(), 1);
    assert_eq!(hour[0].kind, MetricKind::Queue);
    let points = &hour[0].points;
    assert_eq!(points.len(), 2);
    assert_eq!(points[1].processed, 30, "both masters in one minute");
    assert_eq!(points[1].failed, 2);
    assert_eq!(points[1].wait_secs, Some(2.0));
    let week = h.fleet.metrics(MetricsPeriod::Week).await.unwrap();
    let total: u64 = week[0].points.iter().map(|p| p.processed).sum();
    assert_eq!(total, 134);

    // Snapshots older than `fleet.trim.metrics_days` are trimmed.
    clock.advance(Duration::from_secs(7 * 24 * 3600 - 60 * 60));
    h.fleet.trim().await.unwrap();
    let week = h.fleet.metrics(MetricsPeriod::Week).await.unwrap();
    let total: u64 = week[0].points.iter().map(|p| p.processed).sum();
    assert_eq!(total, 35, "the snapshot of 30 hours ago is gone");
}

#[tokio::test]
async fn the_queue_settings_shape_the_default_supervisor() {
    let h = Harness::new(
        builder()
            .config("queue.queues", "high,default")
            .config("queue.workers", 2),
    )
    .await;
    let default = &h.fleet.supervisors()[0];
    assert_eq!(default.queues, ["high", "default"]);
    assert_eq!(default.concurrency, 2);
    let _ = QueueOptions::default();
}
