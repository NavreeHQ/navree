//! Ingest under load: counts and per-minute aggregates are never lost, only
//! details (traces and occurrences) are, and every lost detail is counted
//! as sampled out.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_db::{Db, Executor, Statement};
use navree_lighthouse::{IssueFilter, IssueKind, Period};
use navree_telemetry::{
    EventContext, ExceptionReported, QueryExecuted, RequestHandled, Telemetry, TelemetryEvent,
    TelemetryRecord,
};

use crate::support::{builder, flushed, lighthouse};

fn in_request(id: &str) -> EventContext {
    EventContext {
        request_id: Some(id.to_owned()),
        ..EventContext::default()
    }
}

fn handled(id: &str, route: &str, status: u16, ms: u64) -> TelemetryRecord {
    TelemetryRecord {
        at: Utc::now(),
        context: in_request(id),
        event: TelemetryEvent::RequestHandled(RequestHandled {
            method: "GET".into(),
            route: Some(route.into()),
            path: route.into(),
            status,
            duration: Duration::from_millis(ms),
            user_id: None,
            workspace_id: None,
            request_id: id.into(),
            client_ip: None,
        }),
    }
}

fn query(id: &str, sql: &str) -> TelemetryRecord {
    TelemetryRecord {
        at: Utc::now(),
        context: in_request(id),
        event: TelemetryEvent::QueryExecuted(QueryExecuted {
            sql: sql.into(),
            duration: Duration::from_millis(1),
            rows: None,
            connection: "sqlite".into(),
            failed: false,
        }),
    }
}

fn exception(id: &str) -> TelemetryRecord {
    TelemetryRecord {
        at: Utc::now(),
        context: in_request(id),
        event: TelemetryEvent::ExceptionReported(ExceptionReported {
            message: "deal not found".into(),
            chain: vec!["deal not found".into()],
            location: None,
            panic: false,
            request: None,
        }),
    }
}

/// Sends one request's records: a query, maybe an exception, the end.
fn request(telemetry: &Telemetry, id: &str, route: &str, failing: bool) {
    telemetry.record(query(id, "SELECT * FROM deals WHERE id = $1"));
    if failing {
        telemetry.record(exception(id));
    }
    let status = if failing { 500 } else { 200 };
    telemetry.record(handled(id, route, status, 2));
}

#[tokio::test]
async fn a_burst_far_larger_than_the_buffer_loses_no_counts() {
    // A tiny detail queue and every trace wanted: details must be dropped,
    // counts must not.
    let app = builder()
        .config("lighthouse.buffer", 100)
        .config("lighthouse.sample.requests", 1.0)
        .build()
        .await;
    let telemetry = Telemetry::from_app(app.ctx()).await;
    // Sent in one go, without yielding: nothing is written meanwhile.
    for i in 0..20_000 {
        let route = ["/api/v1/deals/{id}", "/api/v1/deals/board"][i % 2];
        request(&telemetry, &format!("burst-{i}"), route, i % 100 == 0);
    }
    let store = flushed(&app).await;
    let routes = store.routes(Period::Hour).await.unwrap();
    let count = |endpoint: &str| {
        routes
            .iter()
            .find(|route| route.endpoint == endpoint)
            .map_or(0, |route| route.count)
    };
    assert_eq!(count("GET /api/v1/deals/{id}"), 10_000);
    assert_eq!(count("GET /api/v1/deals/board"), 10_000);
    let server_errors: i64 = routes.iter().map(|route| route.server_errors).sum();
    assert_eq!(server_errors, 200);
    let queries = store.queries(Period::Hour).await.unwrap();
    assert_eq!(queries.iter().map(|query| query.count).sum::<i64>(), 20_000);

    // Every exception is counted on its issue, stored or not.
    let issues = store.issues(&IssueFilter::default()).await.unwrap().data;
    let exceptions: i64 = issues
        .iter()
        .filter(|summary| summary.issue.kind == IssueKind::Exception)
        .map(|summary| summary.issue.count)
        .sum();
    assert_eq!(exceptions, 200);

    // Details beyond the buffer were sampled out, and that is visible.
    let lighthouse = lighthouse(&app).await;
    let sampled_out = lighthouse.sampled_out();
    assert!(sampled_out > 0, "details were dropped");
    let overview = store.overview(Period::Hour).await.unwrap();
    assert_eq!(overview.requests, 20_000);
    assert_eq!(overview.exceptions, 200);
    assert_eq!(overview.sampled_out, sampled_out as i64);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_senders_lose_no_counts() {
    let app = builder().config("lighthouse.buffer", 100).build().await;
    let telemetry = Arc::new(Telemetry::from_app(app.ctx()).await);
    let mut tasks = Vec::new();
    for task in 0..16 {
        let telemetry = telemetry.clone();
        tasks.push(tokio::spawn(async move {
            for i in 0..2_500 {
                request(
                    &telemetry,
                    &format!("t{task}-{i}"),
                    "/api/v1/tasks",
                    i % 250 == 0,
                );
                if i % 500 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let store = flushed(&app).await;
    let routes = store.routes(Period::Hour).await.unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].count, 40_000);
    assert_eq!(routes[0].server_errors, 160);
    let overview = store.overview(Period::Hour).await.unwrap();
    assert_eq!(overview.requests, 40_000);
    assert_eq!(overview.exceptions, 160);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn by_default_errors_and_slow_requests_keep_their_traces_and_a_tenth_of_the_rest() {
    let app = builder()
        .config("lighthouse.thresholds.slow_request_ms", 1_000)
        .build()
        .await;
    let lighthouse = lighthouse(&app).await;
    assert!((lighthouse.options().sample.requests - 0.1).abs() < f64::EPSILON);
    assert!((lighthouse.options().sample.jobs - 0.1).abs() < f64::EPSILON);
    let telemetry = Telemetry::from_app(app.ctx()).await;
    for i in 0..1_000 {
        telemetry.record(handled(&format!("ok-{i}"), "/api/v1/ok", 200, 2));
    }
    for i in 0..20 {
        telemetry.record(handled(&format!("err-{i}"), "/api/v1/err", 503, 2));
        telemetry.record(handled(&format!("slow-{i}"), "/api/v1/slow", 200, 1_500));
    }
    let store = flushed(&app).await;
    let _ = store;
    let db = app.get::<Db>().await;
    let traces = |endpoint: &'static str| {
        let db = db.clone();
        async move {
            let row = db
                .fetch_one(&Statement::text(format!(
                    "SELECT COUNT(*) AS n FROM lighthouse_samples WHERE name = '{endpoint}'"
                )))
                .await
                .unwrap();
            row.get::<i64>("n").unwrap()
        }
    };
    let ok = traces("GET /api/v1/ok").await;
    assert!((60..=140).contains(&ok), "about a tenth kept: {ok}");
    assert_eq!(traces("GET /api/v1/err").await, 20, "every error is kept");
    assert_eq!(
        traces("GET /api/v1/slow").await,
        20,
        "every slow one is kept"
    );
    assert_eq!(lighthouse.sampled_out(), 0);
    app.shutdown().await.unwrap();
}
