//! What Lighthouse records from a running app: exceptions grouped into
//! issues, slow requests and queries, reports from code, counts under
//! sampling, ignored routes, and records from other modules (jobs,
//! scheduled tasks, outgoing calls).

use std::time::Duration;

use chrono::Utc;
use navree_lighthouse::{IssueFilter, IssueKind, IssueStatus, Level, Period, REDACTED};
use navree_telemetry::{
    EventContext, HttpClientRequest, JobFailed, ScheduledTaskRan, Telemetry, TelemetryEvent,
    TelemetryRecord,
};
use serde_json::json;

use crate::support::{builder, flushed, lighthouse};

fn all() -> IssueFilter {
    IssueFilter::default()
}

#[tokio::test]
async fn a_500_becomes_one_issue_per_fingerprint_with_its_request() {
    let app = builder().build().await;
    let client = app.client();
    for n in [41, 42] {
        client
            .get(&format!("/api/demo/boom/{n}"))
            .header("authorization", "Bearer abc")
            .header("x-api-token", "t0k")
            .header("accept", "application/json")
            .send()
            .await
            .assert_status(500);
    }
    let store = flushed(&app).await;
    let page = store.issues(&all()).await.unwrap();
    assert_eq!(page.data.len(), 1, "both messages normalize to one issue");
    let issue = &page.data[0].issue;
    assert_eq!(issue.kind, IssueKind::Exception);
    assert_eq!(issue.level, Level::Error);
    assert_eq!(issue.status, IssueStatus::Open);
    assert_eq!(issue.culprit, "GET /api/demo/boom/{n}");
    assert_eq!(issue.count, 2);
    assert!(
        issue
            .url
            .starts_with("https://api.acme.test/_lighthouse/issues/")
    );

    let detail = store.issue_detail(issue.id).await.unwrap();
    let latest = detail.latest.expect("an occurrence");
    assert_eq!(latest.status, Some(500));
    assert_eq!(latest.method.as_deref(), Some("GET"));
    assert_eq!(latest.release.as_deref(), Some("r1"));
    assert_eq!(latest.environment.as_deref(), Some("test"));
    let headers = &latest.context["request"]["headers"];
    assert_eq!(headers["authorization"], REDACTED);
    assert_eq!(headers["x-api-token"], REDACTED, "log.redact's `token`");
    assert_eq!(headers["accept"], "application/json");
    let trace = store
        .sample(latest.sample_id.expect("a 500 keeps its trace"))
        .await
        .unwrap();
    assert_eq!(trace.summary.status, Some(500));
    assert_eq!(trace.summary.name, "GET /api/demo/boom/{n}");
    assert_eq!(
        store.occurrences(issue.id, 1, 10).await.unwrap().meta.total,
        2
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn json_bodies_are_kept_masked_when_asked_and_addresses_can_be_left_out() {
    let app = builder()
        .config("lighthouse.capture.request_body", true)
        .config("lighthouse.capture.ip", false)
        .config("lighthouse.redact", "card")
        .build()
        .await;
    app.client()
        .post("/api/demo/charge")
        .json(&json!({ "amount": 1200, "password": "hunter2", "card_number": "4242" }))
        .send()
        .await
        .assert_status(500);
    let store = flushed(&app).await;
    let issue = &store.issues(&all()).await.unwrap().data[0].issue;
    let latest = store.issue_detail(issue.id).await.unwrap().latest.unwrap();
    let request = &latest.context["request"];
    assert_eq!(request["body"]["amount"], 1200);
    assert_eq!(request["body"]["password"], REDACTED);
    assert_eq!(
        request["body"]["card_number"], REDACTED,
        "lighthouse.redact"
    );
    assert!(request.get("ip").is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn panics_group_by_their_location() {
    let app = builder().build().await;
    for n in [3, 7] {
        app.client()
            .get(&format!("/api/demo/panic/{n}"))
            .send()
            .await
            .assert_status(500);
    }
    let store = flushed(&app).await;
    let page = store.issues(&all()).await.unwrap();
    assert_eq!(page.data.len(), 1, "one location, one issue");
    let issue = &page.data[0].issue;
    assert_eq!(issue.count, 2);
    let detail = store.issue_detail(issue.id).await.unwrap();
    assert!(
        detail.location.as_deref().unwrap().contains("support.rs"),
        "{detail:?}"
    );
    assert_eq!(detail.latest.unwrap().context["panic"], true);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn slow_requests_and_slow_queries_open_their_own_issues() {
    let app = builder()
        .config("lighthouse.thresholds.slow_request_ms", 10)
        .config("lighthouse.thresholds.slow_query_ms", 0)
        .build()
        .await;
    let client = app.client();
    client.get("/api/demo/slow").send().await.assert_status(200);
    client
        .get("/api/demo/query")
        .send()
        .await
        .assert_status(200);
    let store = flushed(&app).await;
    let kinds: Vec<IssueKind> = store
        .issues(&all())
        .await
        .unwrap()
        .data
        .iter()
        .map(|summary| summary.issue.kind)
        .collect();
    assert!(kinds.contains(&IssueKind::SlowRequest), "{kinds:?}");
    assert!(kinds.contains(&IssueKind::SlowQuery), "{kinds:?}");
    let slow = store
        .issues(&IssueFilter {
            kind: Some("slow_request".into()),
            ..IssueFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(slow.data[0].issue.culprit, "GET /api/demo/slow");
    assert_eq!(slow.data[0].issue.level, Level::Warning);

    let queries = store.queries(Period::Hour).await.unwrap();
    let statement = queries
        .iter()
        .find(|query| query.statement == "SELECT ? WHERE ? = ?")
        .expect("the normalized statement");
    assert_eq!(statement.origins[0].name, "GET /api/demo/query");
    let routes = store.routes(Period::Hour).await.unwrap();
    let slow_route = routes
        .iter()
        .find(|route| route.endpoint == "GET /api/demo/slow")
        .unwrap();
    assert_eq!(slow_route.count, 1);
    assert!(slow_route.p95_ms >= 25.0, "{}", slow_route.p95_ms);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn reports_from_code_carry_their_route_tags_and_masked_context() {
    let app = builder().build().await;
    let client = app.client();
    client
        .post("/api/demo/report")
        .send()
        .await
        .assert_status(204);
    client
        .post("/api/demo/tagged")
        .send()
        .await
        .assert_status(204);
    let store = flushed(&app).await;
    let issues = store.issues(&all()).await.unwrap().data;
    assert_eq!(issues.len(), 2);
    let charged = issues
        .iter()
        .find(|summary| summary.issue.title == "cannot charge invoice")
        .unwrap();
    assert_eq!(charged.issue.culprit, "POST /api/demo/report");
    let detail = store.issue_detail(charged.issue.id).await.unwrap();
    assert_eq!(detail.chain, ["cannot charge invoice", "card declined"]);
    assert!(
        detail
            .error_type
            .as_deref()
            .unwrap()
            .ends_with("InvoiceError")
    );
    assert!(detail.location.as_deref().unwrap().contains("support.rs"));
    let latest = detail.latest.unwrap();
    assert!(
        latest.sample_id.is_some(),
        "a request that reported keeps its trace"
    );

    let tagged = issues
        .iter()
        .find(|summary| summary.issue.title == "payment event without a customer")
        .unwrap();
    let latest = store
        .issue_detail(tagged.issue.id)
        .await
        .unwrap()
        .latest
        .unwrap();
    assert_eq!(latest.context["tags"]["plan"], "pro");
    assert_eq!(latest.context["context"]["invoice_id"], 1042);
    assert_eq!(latest.context["context"]["api_token"], REDACTED);
    let filtered = store
        .issues(&IssueFilter {
            tag: Some("plan:pro".into()),
            ..IssueFilter::default()
        })
        .await
        .unwrap();
    assert_eq!(filtered.data.len(), 1);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn counts_stay_exact_when_no_trace_is_sampled() {
    let app = builder()
        .config("lighthouse.sample.requests", 0.0)
        .build()
        .await;
    let client = app.client();
    for _ in 0..12 {
        client.get("/api/demo/ok").send().await.assert_status(200);
    }
    let store = flushed(&app).await;
    let routes = store.routes(Period::Hour).await.unwrap();
    let ok = routes
        .iter()
        .find(|route| route.endpoint == "GET /api/demo/ok")
        .unwrap();
    assert_eq!(ok.count, 12);
    assert!(
        store
            .route("GET /api/demo/ok", Period::Hour)
            .await
            .unwrap()
            .slowest
            .is_empty()
    );
    let overview = store.overview(Period::Hour).await.unwrap();
    assert_eq!(overview.requests, 12);
    assert_eq!(overview.error_rate, 0.0);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn made_up_request_methods_do_not_make_routes_of_their_own() {
    let app = builder().build().await;
    let client = app.client();
    // Any token is a method to HTTP: each made-up one would otherwise be a
    // route kept in memory for an hour, and a row in the database.
    for n in 0..40 {
        let method = http::Method::from_bytes(format!("PROBE{n}").as_bytes()).unwrap();
        client.request(method, "/api/demo/ok").send().await;
        let method = http::Method::from_bytes(format!("SCAN{n}").as_bytes()).unwrap();
        client.request(method, "/nowhere").send().await;
    }
    client.get("/api/demo/ok").send().await.assert_status(200);
    let store = flushed(&app).await;
    let mut endpoints: Vec<String> = store
        .routes(Period::Hour)
        .await
        .unwrap()
        .into_iter()
        .map(|route| route.endpoint)
        .collect();
    endpoints.sort();
    assert!(endpoints.len() <= 3, "{endpoints:?}");
    assert!(
        endpoints.iter().all(|endpoint| !endpoint.contains("PROBE")
            && !endpoint.contains("SCAN")),
        "{endpoints:?}"
    );
    assert!(
        endpoints.contains(&"GET /api/demo/ok".to_owned()),
        "{endpoints:?}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn ignored_routes_and_the_dashboard_itself_are_not_recorded() {
    let app = builder()
        .config("lighthouse.ignore.routes", "/api/demo/*")
        .build()
        .await;
    let client = app.client();
    client
        .get("/api/demo/boom/1")
        .send()
        .await
        .assert_status(500);
    client
        .get("/_lighthouse/login")
        .send()
        .await
        .assert_status(200);
    let store = flushed(&app).await;
    assert!(store.issues(&all()).await.unwrap().data.is_empty());
    assert!(store.routes(Period::Hour).await.unwrap().is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_jobs_scheduled_tasks_and_outgoing_calls_become_issues() {
    let app = builder().build().await;
    let telemetry = Telemetry::from_app(app.ctx()).await;
    let record = |context: EventContext, event: TelemetryEvent| TelemetryRecord {
        at: Utc::now(),
        context,
        event,
    };
    let in_job = EventContext {
        job_id: Some("j1".into()),
        ..EventContext::default()
    };
    telemetry.record(record(
        in_job.clone(),
        TelemetryEvent::HttpClientRequest(HttpClientRequest {
            method: "GET".into(),
            url: "https://api.exchangerate.test/latest".into(),
            status: Some(503),
            duration: Duration::from_millis(150),
            attempts: 3,
            error: None,
            span_id: None,
        }),
    ));
    telemetry.record(record(
        in_job,
        TelemetryEvent::JobFailed(JobFailed {
            queue: "default".into(),
            job_type: "invoices.sync".into(),
            job_id: "j1".into(),
            attempt: 3,
            duration: Duration::from_millis(20),
            error: "provider answered 503".into(),
            will_retry: false,
        }),
    ));
    telemetry.record(record(
        EventContext::default(),
        TelemetryEvent::ScheduledTaskRan(ScheduledTaskRan {
            task: "reports.refresh".into(),
            tick: Utc::now(),
            duration: Duration::from_millis(5),
            error: Some("disk full".into()),
        }),
    ));
    let store = flushed(&app).await;
    let issues = store.issues(&all()).await.unwrap().data;
    let culprit = |kind: IssueKind| {
        issues
            .iter()
            .find(|summary| summary.issue.kind == kind)
            .map(|summary| summary.issue.culprit.clone())
    };
    assert_eq!(
        culprit(IssueKind::FailedJob).as_deref(),
        Some("invoices.sync")
    );
    assert_eq!(
        culprit(IssueKind::ScheduledTask).as_deref(),
        Some("reports.refresh")
    );
    assert_eq!(
        culprit(IssueKind::OutgoingRequest).as_deref(),
        Some("api.exchangerate.test")
    );
    let jobs = store.jobs(Period::Hour).await.unwrap();
    assert_eq!(jobs[0].job_type, "invoices.sync");
    assert_eq!(jobs[0].failed_jobs, 1);
    let hosts = store.outgoing(Period::Hour).await.unwrap();
    assert_eq!(hosts[0].host, "api.exchangerate.test");
    assert_eq!(hosts[0].errors, 1);
    assert_eq!(hosts[0].origins[0].name, "invoices.sync");
    let overview = store.overview(Period::Day).await.unwrap();
    assert_eq!(overview.failed_jobs, 1);
    assert_eq!(overview.open_issues, 3);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn nothing_is_recorded_when_lighthouse_is_disabled() {
    let app = builder().config("lighthouse.enabled", false).build().await;
    app.client()
        .get("/api/demo/boom/1")
        .send()
        .await
        .assert_status(500);
    let lighthouse = lighthouse(&app).await;
    assert!(!lighthouse.is_recording());
    let store = flushed(&app).await;
    assert!(store.issues(&all()).await.unwrap().data.is_empty());
    let _ = json!({});
    app.shutdown().await.unwrap();
}
