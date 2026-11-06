//! One test per watcher: what each telemetry event becomes.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::Utc;
use navree_logbook::{Content, EntryKind, Filter, JobOutcome, ListQuery, Term};
use navree_telemetry::{
    AiCall, BroadcastSent, CacheEvent, CacheOperation, EventContext, EventEmitted, EventKind,
    ExceptionReported, HttpClientRequest, JobFailed, JobProcessed, LogLevel, LogRecord, MailSent,
    NotificationSent, RequestHandled, ScheduledTaskRan, SearchMode, SearchQueried, SignInEvent,
    SignInMethod, StorageOp, StorageOperation, TelemetryEvent, TelemetrySink, TraceContext,
    UiOperation, UiOperationKind, WebhookDelivery,
};
use serde_json::json;

use crate::support::{feed, in_job, in_request, logbook, logbook_with, query_event, request_event};

#[tokio::test]
async fn requests_are_batches_with_their_entries_and_tags() {
    let logbook = logbook();
    let mut context = in_request("req-1");
    context.user_id = Some("0192f0c1-user".into());
    feed(&logbook, &context, query_event("SELECT * FROM deals", 2));
    feed(
        &logbook,
        &context,
        TelemetryEvent::RequestHandled(RequestHandled {
            method: "GET".into(),
            route: Some("/api/v1/deals/{id}".into()),
            path: "/api/v1/deals/7".into(),
            status: 200,
            duration: Duration::from_millis(20),
            user_id: Some("0192f0c1-user".into()),
            workspace_id: Some("ws-1".into()),
            request_id: "req-1".into(),
            client_ip: Some("127.0.0.1".into()),
        }),
    );
    let batch = logbook.last_request().await.expect("a request");
    assert_eq!(batch.id(), "req-1");
    assert_eq!(batch.entries().len(), 2);
    let request = batch.request().unwrap();
    assert_eq!(request.route.as_deref(), Some("/api/v1/deals/{id}"));
    assert_eq!(request.summary.queries, 1);
    assert_eq!(batch.queries()[0].sql, "SELECT * FROM deals");
    let head = batch.head().unwrap();
    assert_eq!(head.user_id.as_deref(), Some("0192f0c1-user"));
    assert_eq!(head.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(
        batch.tags(),
        [
            "route:/api/v1/deals/{id}",
            "status:200",
            "user:0192f0c1-user",
            "workspace:ws-1"
        ]
    );
    // The request started 20 ms before it ended.
    assert!((head.duration_ms.unwrap() - 20.0).abs() < 0.01);
}

#[tokio::test]
async fn failed_requests_are_marked() {
    let logbook = logbook();
    feed(
        &logbook,
        &in_request("req-9"),
        request_event("req-9", "POST", "/api/v1/deals", 503),
    );
    let batch = logbook.last_request().await.unwrap();
    assert!(batch.head().unwrap().failed);
    let failed = logbook
        .entries(&ListQuery::new(Filter::parse("failed").unwrap(), 10))
        .await
        .unwrap();
    assert_eq!(failed.len(), 1);
}

#[tokio::test]
async fn queries_are_marked_slow_and_repeats_are_duplicates() {
    let logbook = logbook_with(|o| {
        o.slow_query_ms = 50;
        o.duplicate_queries = 3;
    });
    let context = in_request("req-2");
    for _ in 0..3 {
        feed(
            &logbook,
            &context,
            query_event("SELECT * FROM companies WHERE id = ?", 1),
        );
    }
    feed(
        &logbook,
        &context,
        query_event("SELECT COUNT(*) FROM deals", 80),
    );
    feed(
        &logbook,
        &context,
        request_event("req-2", "GET", "/api/v1/deals", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    assert_eq!(batch.queries().len(), 4);
    assert!(batch.queries()[3].slow);
    assert!(!batch.queries()[0].slow);
    let duplicates = batch.duplicate_queries();
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].count, 3);
    assert_eq!(duplicates[0].sql, "SELECT * FROM companies WHERE id = ?");
    let summary = &batch.request().unwrap().summary;
    assert_eq!((summary.slow_queries, summary.duplicate_queries), (1, 1));
    let head = batch.head().unwrap();
    assert!(head.slow, "a batch that ran a slow query is slow");
    let slow = logbook
        .entries(&ListQuery::new(Filter::parse("slow").unwrap(), 10))
        .await
        .unwrap();
    assert_eq!(slow.len(), 1);
}

#[tokio::test]
async fn jobs_are_batches_tagged_with_type_and_queue() {
    let logbook = logbook();
    let context = in_job("job-1");
    feed(
        &logbook,
        &context,
        query_event("UPDATE deals SET stage = ?", 1),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::JobFailed(JobFailed {
            queue: "default".into(),
            job_type: "deals.sync".into(),
            job_id: "job-1".into(),
            attempt: 1,
            duration: Duration::from_millis(5),
            error: "timeout".into(),
            will_retry: true,
        }),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::JobProcessed(JobProcessed {
            queue: "default".into(),
            job_type: "deals.sync".into(),
            job_id: "job-1".into(),
            attempt: 2,
            duration: Duration::from_millis(5),
        }),
    );
    let batch = logbook.last_job().await.unwrap();
    assert_eq!(batch.id(), "job-1");
    let jobs = batch.jobs();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].outcome, JobOutcome::Released);
    assert_eq!(jobs[0].error.as_deref(), Some("timeout"));
    assert_eq!(jobs[1].outcome, JobOutcome::Processed);
    assert_eq!(batch.tags(), ["job:deals.sync", "queue:default"]);
    assert!(batch.entries()[1].failed);
    assert_eq!(batch.kind(), Some(EntryKind::Job));
}

#[tokio::test]
async fn failed_jobs_are_failed_for_good() {
    let logbook = logbook();
    feed(
        &logbook,
        &in_job("job-2"),
        TelemetryEvent::JobFailed(JobFailed {
            queue: "mail".into(),
            job_type: "mail.send".into(),
            job_id: "job-2".into(),
            attempt: 3,
            duration: Duration::from_millis(1),
            error: "smtp down".into(),
            will_retry: false,
        }),
    );
    let batch = logbook.last_job().await.unwrap();
    assert_eq!(batch.jobs()[0].outcome, JobOutcome::Failed);
}

#[tokio::test]
async fn mail_is_recorded_with_its_bodies() {
    let logbook = logbook();
    let context = in_request("req-3");
    feed(
        &logbook,
        &context,
        TelemetryEvent::MailSent(MailSent {
            to: vec!["anu@example.com".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            from: Some("crm@example.com".into()),
            subject: "Welcome".into(),
            text: Some("Hi Anu".into()),
            html: Some("<p>Hi Anu</p>".into()),
            transport: "log".into(),
            duration: Duration::from_millis(1),
        }),
    );
    feed(
        &logbook,
        &context,
        request_event("req-3", "POST", "/api/auth/register", 201),
    );
    let batch = logbook.last_request().await.unwrap();
    let mail = batch.mail();
    assert_eq!(mail.len(), 1);
    assert_eq!(mail[0].subject, "Welcome");
    assert_eq!(mail[0].html.as_deref(), Some("<p>Hi Anu</p>"));
    assert_eq!(batch.request().unwrap().summary.mail, 1);
    // Free text finds the subject and the recipients.
    let found = logbook
        .entries(&ListQuery::new(
            Filter::parse("type:mail anu@").unwrap(),
            10,
        ))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
}

#[tokio::test]
async fn events_are_recorded_with_payload_and_listeners() {
    let logbook = logbook();
    let context = in_request("req-4");
    feed(
        &logbook,
        &context,
        TelemetryEvent::EventEmitted(EventEmitted {
            name: "deal.won".into(),
            payload: json!({ "id": 7 }),
            listeners: vec!["crm::NotifyOwner".into()],
            queued: vec!["crm::SyncCrm".into()],
            failed: Vec::new(),
            duration: Duration::from_millis(1),
        }),
    );
    feed(
        &logbook,
        &context,
        request_event("req-4", "PATCH", "/api/v1/deals/7", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    let events = batch.events();
    assert_eq!(events[0].name, "deal.won");
    assert_eq!(events[0].payload, json!({ "id": 7 }));
    assert_eq!(events[0].queued, ["crm::SyncCrm"]);
    assert_eq!(batch.request().unwrap().summary.events, 1);
}

#[tokio::test]
async fn cache_calls_are_recorded() {
    let logbook = logbook();
    let context = in_request("req-5");
    for operation in [
        CacheOperation::Miss,
        CacheOperation::Write,
        CacheOperation::Hit,
    ] {
        feed(
            &logbook,
            &context,
            TelemetryEvent::Cache(CacheEvent {
                operation,
                key: "stats".into(),
                store: "memory".into(),
            }),
        );
    }
    feed(
        &logbook,
        &context,
        request_event("req-5", "GET", "/stats", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    let calls: Vec<&str> = batch
        .cache_calls()
        .iter()
        .map(|c| c.operation.as_str())
        .collect();
    assert_eq!(calls, ["miss", "write", "hit"]);
}

fn log(level: LogLevel, target: &str, message: &str) -> TelemetryEvent {
    TelemetryEvent::Log(LogRecord {
        level,
        target: target.into(),
        message: message.into(),
        fields: BTreeMap::from([("deal".to_owned(), json!(7))]),
    })
}

#[tokio::test]
async fn log_lines_are_recorded_from_the_logbook_level_up() {
    let logbook = logbook_with(|o| o.log_level = "info".into());
    let context = in_request("req-6");
    feed(
        &logbook,
        &context,
        log(LogLevel::Debug, "crm::deals", "too chatty"),
    );
    feed(
        &logbook,
        &context,
        log(LogLevel::Warn, "crm::deals", "stage skipped"),
    );
    feed(
        &logbook,
        &context,
        log(LogLevel::Warn, "navree::logbook", "our own line"),
    );
    feed(
        &logbook,
        &context,
        request_event("req-6", "GET", "/deals", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    let logs = batch.logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].message, "stage skipped");
    assert_eq!(logs[0].level, "WARN");
    assert_eq!(logs[0].fields["deal"], json!(7));
}

#[tokio::test]
async fn exceptions_are_recorded_with_their_chain() {
    let logbook = logbook();
    let context = in_request("req-7");
    feed(
        &logbook,
        &context,
        TelemetryEvent::ExceptionReported(ExceptionReported {
            message: "boom".into(),
            chain: vec!["boom".into(), "disk full".into()],
            location: Some("src/deals.rs:10:5".into()),
            panic: true,
            request: None,
        }),
    );
    feed(
        &logbook,
        &context,
        request_event("req-7", "GET", "/boom", 500),
    );
    let batch = logbook.last_request().await.unwrap();
    let exceptions = batch.exceptions();
    assert_eq!(exceptions[0].chain, ["boom", "disk full"]);
    assert!(exceptions[0].panic);
    assert_eq!(batch.request().unwrap().summary.exceptions, 1);
}

#[tokio::test]
async fn outgoing_http_calls_are_recorded() {
    let logbook = logbook();
    let context = in_request("req-8");
    feed(
        &logbook,
        &context,
        TelemetryEvent::HttpClientRequest(HttpClientRequest {
            method: "GET".into(),
            url: "https://api.example.com/rates".into(),
            status: Some(502),
            duration: Duration::from_millis(80),
            attempts: 3,
            error: None,
            span_id: None,
        }),
    );
    feed(
        &logbook,
        &context,
        request_event("req-8", "GET", "/rates", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    let calls = batch.http_calls();
    assert_eq!(calls[0].attempts, 3);
    assert!(batch.entries()[0].failed, "a 5xx answer is a failed call");
}

#[tokio::test]
async fn notifications_are_recorded_per_channel() {
    let logbook = logbook();
    let context = in_request("req-10");
    for channel in ["database", "mail"] {
        feed(
            &logbook,
            &context,
            TelemetryEvent::NotificationSent(NotificationSent {
                notification_id: "n-1".into(),
                notification_type: "deal.assigned".into(),
                notifiable_type: "user".into(),
                notifiable_id: "u-1".into(),
                channel: channel.into(),
            }),
        );
    }
    feed(
        &logbook,
        &context,
        request_event("req-10", "PATCH", "/deals/1", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    assert_eq!(batch.notifications().len(), 2);
    assert_eq!(batch.request().unwrap().summary.notifications, 2);
}

#[tokio::test]
async fn scheduled_runs_are_batches_of_their_trace() {
    let logbook = logbook();
    let context = EventContext {
        trace: Some(TraceContext::root(true)),
        ..EventContext::default()
    };
    feed(&logbook, &context, query_event("DELETE FROM audit_logs", 3));
    feed(
        &logbook,
        &context,
        TelemetryEvent::ScheduledTaskRan(ScheduledTaskRan {
            task: "audit:prune".into(),
            tick: Utc::now(),
            duration: Duration::from_millis(4),
            error: None,
        }),
    );
    let batch = logbook.last_schedule().await.unwrap();
    assert_eq!(batch.schedule().unwrap().task, "audit:prune");
    assert_eq!(batch.queries().len(), 1);
    assert_eq!(batch.tags(), ["schedule:audit:prune"]);
    assert_eq!(
        batch.id(),
        context.trace.unwrap().trace_id.to_string(),
        "the batch is the run's trace"
    );
}

#[tokio::test]
async fn dumps_and_tags_land_in_the_running_batch() {
    let logbook = logbook();
    let telemetry = navree_telemetry::Telemetry::disabled();
    let context = navree_telemetry::TelemetryContext::new(telemetry).with_request_id("req-11");
    context
        .scope(async {
            logbook.tag("import:42");
            logbook.dump("parsed rows", &[1, 2, 3][..]);
        })
        .await;
    feed(
        &logbook,
        &in_request("req-11"),
        request_event("req-11", "POST", "/import", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    assert_eq!(batch.dumps()[0].label, "parsed rows");
    assert_eq!(batch.dumps()[0].value, json!([1, 2, 3]));
    assert!(batch.tags().contains(&"import:42".to_owned()));
    let tagged = logbook
        .entries(&ListQuery::new(Filter::parse("tag:import:").unwrap(), 10))
        .await
        .unwrap();
    assert_eq!(tagged.len(), 1);

    // Outside a batch: a loose dump, and a tag that goes nowhere.
    logbook.tag("nowhere");
    logbook.dump("loose", &"value");
    let loose = logbook
        .entries(&ListQuery::new(
            Filter::of([Term::Type(EntryKind::Dump)]),
            10,
        ))
        .await
        .unwrap();
    assert_eq!(loose.len(), 2);
    assert!(loose[0].batch_id.is_none());
}

#[tokio::test]
async fn turned_off_watchers_want_nothing() {
    let logbook = logbook_with(|o| {
        o.watchers.cache = false;
        o.watchers.requests = false;
    });
    let interest = logbook.interest();
    assert!(!interest.contains(EventKind::Cache));
    assert!(!interest.contains(EventKind::RequestHandled));
    assert!(interest.contains(EventKind::QueryExecuted));
    // Without the requests watcher, a request's queries are loose entries.
    feed(&logbook, &in_request("req-12"), query_event("SELECT 1", 1));
    let loose = logbook
        .entries(&ListQuery::new(Filter::all(), 10))
        .await
        .unwrap();
    assert_eq!(loose.len(), 1);
    assert!(loose[0].batch_id.is_none());
}

#[tokio::test]
async fn a_disabled_logbook_wants_and_records_nothing() {
    let logbook = navree_logbook::Logbook::disabled(navree_logbook::LogbookOptions::default());
    assert!(logbook.interest().is_empty());
    assert!(!logbook.is_enabled());
    feed(&logbook, &in_request("r"), query_event("SELECT 1", 1));
    logbook.tag("x");
    logbook.dump("y", &1);
    assert!(logbook.last_request().await.is_none());
    assert!(logbook.count().await.is_err());
    let record_kinds: Vec<_> = navree_logbook::EntryKind::ALL.to_vec();
    assert_eq!(record_kinds.len(), 20);
    let _ = Content::Dump;
}

/// What the outbox's poll runs every second on PostgreSQL, statement by statement.
const OUTBOX_POLL: [&str; 4] = [
    "BEGIN",
    "SELECT set_config('navree.actor', $1, true), set_config('navree.request_id', $2, true)",
    "SELECT id, pattern, headers, payload FROM micro_outbox WHERE created_at < $1 ORDER BY created_at, seq LIMIT $2 FOR UPDATE SKIP LOCKED",
    "COMMIT",
];

async fn queries_recorded(logbook: &navree_logbook::Logbook) -> Vec<String> {
    let mut sql: Vec<String> = logbook
        .entries(&ListQuery::new(Filter::parse("type:query").unwrap(), 100))
        .await
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry.content {
            Content::Query(query) => Some(query.sql),
            _ => None,
        })
        .collect();
    sql.reverse();
    sql
}

#[tokio::test]
async fn the_frameworks_own_polling_is_left_out_by_default() {
    let logbook = logbook();
    let outside = EventContext::default();
    for _ in 0..3 {
        for sql in OUTBOX_POLL {
            feed(&logbook, &outside, query_event(sql, 1));
        }
    }
    // The same statements on MySQL and SQLite, and a savepoint.
    for sql in [
        "START TRANSACTION READ ONLY",
        "SET @navree_actor = ?, @navree_request_id = ?",
        "SELECT id FROM micro_inbox WHERE id = ?",
        "SET @navree_actor = NULL, @navree_request_id = NULL",
        "ROLLBACK",
        "BEGIN IMMEDIATE",
        "SAVEPOINT _sqlx_savepoint_1",
        "RELEASE SAVEPOINT _sqlx_savepoint_1",
    ] {
        feed(&logbook, &outside, query_event(sql, 1));
    }
    assert_eq!(queries_recorded(&logbook).await, Vec::<String>::new());
    assert_eq!(logbook.count().await.unwrap(), 0);

    // What the app runs outside a request is its own business, and stays.
    feed(&logbook, &outside, query_event("SELECT * FROM deals", 1));
    assert_eq!(
        queries_recorded(&logbook).await,
        ["SELECT * FROM deals".to_owned()]
    );

    // Inside a request or a job nothing is left out: the app asked for it.
    let request = in_request("req-poll");
    feed(&logbook, &request, query_event(OUTBOX_POLL[2], 1));
    feed(&logbook, &request, query_event("BEGIN", 1));
    feed(
        &logbook,
        &request,
        request_event("req-poll", "GET", "/outbox", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    assert_eq!(batch.queries().len(), 2);
    let job = in_job("job-poll");
    feed(&logbook, &job, query_event("COMMIT", 1));
    assert_eq!(
        logbook
            .entries(&ListQuery::new(Filter::parse("type:query").unwrap(), 100))
            .await
            .unwrap()
            .len(),
        4,
        "the deals query, two in the request, one in the job"
    );
}

#[tokio::test]
async fn the_frameworks_own_polling_shows_when_asked_for() {
    let logbook = logbook_with(|o| o.ignore.framework = false);
    let outside = EventContext::default();
    for sql in OUTBOX_POLL {
        feed(&logbook, &outside, query_event(sql, 1));
    }
    assert_eq!(queries_recorded(&logbook).await, OUTBOX_POLL);
}

#[test]
fn the_framework_switch_is_a_config_key() {
    let options: navree_logbook::LogbookOptions =
        serde_json::from_value(json!({ "ignore": { "framework": false } })).unwrap();
    assert!(!options.ignore.framework);
    assert_eq!(
        options.ignore.routes,
        ["/health/*", "/metrics"],
        "the routes keep their default"
    );
    assert!(navree_logbook::LogbookOptions::default().ignore.framework);
}

// ---------------------------------------------------------------------------
// The platform watchers: AI calls, broadcasts, storage, webhooks, searches,
// sign-ins and UI operations.

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[tokio::test]
async fn model_calls_are_recorded_with_tokens_and_cost_but_no_text() {
    let logbook = logbook();
    let context = in_request("req-ai");
    feed(
        &logbook,
        &context,
        TelemetryEvent::AiCall(
            AiCall::new("anthropic", "claude-sonnet-4-5", "generate", ms(900))
                .tokens(1200, 340)
                .cached_tokens(800)
                .cost_micros(5_400),
        ),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::AiCall(
            AiCall::new("openai", "gpt-5", "stream", ms(40)).failed("rate_limited"),
        ),
    );
    feed(
        &logbook,
        &context,
        request_event("req-ai", "POST", "/api/v1/summaries", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    let calls = batch.ai_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].provider, "anthropic");
    assert_eq!(calls[0].input_tokens, Some(1200));
    assert_eq!(calls[0].output_tokens, Some(340));
    assert_eq!(calls[0].cached_input_tokens, Some(800));
    assert_eq!(calls[0].cost_micros, Some(5_400));
    assert_eq!(calls[1].error.as_deref(), Some("rate_limited"));
    assert_eq!(batch.request().unwrap().summary.ai_calls, 2);
    let entries = batch.entries();
    assert!(!entries[0].failed);
    assert!(entries[1].failed, "a refused call is a failed entry");
    assert!((entries[0].duration_ms.unwrap() - 900.0).abs() < 0.01);
    assert_eq!(entries[0].kind(), EntryKind::Ai);
}

#[tokio::test]
async fn broadcasts_storage_and_webhooks_are_recorded() {
    let logbook = logbook();
    let context = in_request("req-platform");
    feed(
        &logbook,
        &context,
        TelemetryEvent::Broadcast(BroadcastSent::new(
            ["private-crm.deals"],
            "deal.updated",
            4,
            ms(1),
        )),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::StorageOp(
            StorageOp::new(
                "uploads",
                "local",
                StorageOperation::Write,
                "kyc/pan.pdf",
                ms(6),
            )
            .bytes(52_314),
        ),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::StorageOp(
            StorageOp::new(
                "uploads",
                "local",
                StorageOperation::Read,
                "kyc/gone.pdf",
                ms(1),
            )
            .failed("not found"),
        ),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::WebhookDelivery(WebhookDelivery::new(
            "ep-1",
            "invoice.paid",
            2,
            503,
            ms(30),
        )),
    );
    feed(
        &logbook,
        &context,
        request_event("req-platform", "POST", "/invoices/7/pay", 200),
    );
    let batch = logbook.last_request().await.unwrap();

    let broadcasts = batch.broadcasts();
    assert_eq!(broadcasts[0].channels, ["private-crm.deals"]);
    assert_eq!(broadcasts[0].delivered, 4);

    let files = batch.storage_ops();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].operation, "write");
    assert_eq!(files[0].bytes, Some(52_314));
    assert_eq!(files[1].error.as_deref(), Some("not found"));

    let hooks = batch.webhooks();
    assert_eq!(hooks[0].attempt, 2);
    assert_eq!(hooks[0].status, Some(503));
    let hook_entry = batch
        .entries()
        .iter()
        .find(|entry| entry.kind() == EntryKind::Webhook)
        .unwrap();
    assert!(hook_entry.failed, "a 503 is not a delivery");

    let summary = &batch.request().unwrap().summary;
    assert_eq!(
        (summary.broadcasts, summary.storage_ops, summary.webhooks),
        (1, 2, 1)
    );
}

#[tokio::test]
async fn searches_keep_the_length_of_the_term_and_never_the_term() {
    let logbook = logbook();
    let context = in_request("req-search");
    feed(
        &logbook,
        &context,
        TelemetryEvent::Search(
            SearchQueried::new(
                "meilisearch",
                "contacts",
                SearchMode::Text,
                "Harpreet Kaur Ludhiana",
                ms(5),
            )
            .hits(3),
        ),
    );
    feed(
        &logbook,
        &context,
        request_event("req-search", "GET", "/contacts", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    let searches = batch.searches();
    assert_eq!(searches[0].engine, "meilisearch");
    assert_eq!(searches[0].index, "contacts");
    assert_eq!(searches[0].mode, "text");
    assert_eq!(searches[0].term_length, 22);
    assert_eq!(searches[0].hits, Some(3));
    let stored = serde_json::to_string(batch.entries()).unwrap();
    assert!(!stored.contains("Harpreet"), "{stored}");
    assert!(!stored.contains("Ludhiana"), "{stored}");
    assert_eq!(batch.request().unwrap().summary.searches, 1);
}

#[tokio::test]
async fn sign_ins_name_the_user_only_when_they_worked() {
    let logbook = logbook();
    // A sign-in happens before the context knows its user.
    let context = in_request("req-sign-in");
    feed(
        &logbook,
        &context,
        TelemetryEvent::SignIn(SignInEvent::refused(
            SignInMethod::Password,
            "invalid_credentials",
        )),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::SignIn(
            SignInEvent::succeeded(SignInMethod::Social, "0199c1d2-user").provider("github"),
        ),
    );
    feed(
        &logbook,
        &context,
        request_event("req-sign-in", "POST", "/sign-in", 303),
    );
    let batch = logbook.last_request().await.unwrap();
    let sign_ins = batch.sign_ins();
    assert_eq!(sign_ins.len(), 2);
    assert_eq!(sign_ins[0].method, "password");
    assert_eq!(sign_ins[0].outcome, "failure");
    assert_eq!(sign_ins[0].reason.as_deref(), Some("invalid_credentials"));
    assert_eq!(sign_ins[1].provider.as_deref(), Some("github"));

    let entries: Vec<_> = batch
        .entries()
        .iter()
        .filter(|entry| entry.kind() == EntryKind::SignIn)
        .collect();
    assert!(entries[0].failed);
    assert_eq!(entries[0].user_id, None, "a refusal names nobody");
    assert!(!entries[1].failed);
    assert_eq!(entries[1].user_id.as_deref(), Some("0199c1d2-user"));
    assert_eq!(batch.request().unwrap().summary.sign_ins, 2);
}

#[tokio::test]
async fn ui_operations_are_recorded_and_the_toolkits_tool_path_is_left_out() {
    let logbook = logbook();
    let context = in_request("req-ui");
    feed(
        &logbook,
        &context,
        TelemetryEvent::UiOperation(UiOperation::new(
            "/deals/{id}/stage",
            UiOperationKind::FormPost,
            200,
            3,
            ms(9),
        )),
    );
    feed(
        &logbook,
        &context,
        TelemetryEvent::UiOperation(UiOperation::new(
            "/deals/{id}/stage",
            UiOperationKind::Action,
            500,
            0,
            ms(2),
        )),
    );
    // Logbook's own pages are never recorded, as its requests are not.
    feed(
        &logbook,
        &context,
        TelemetryEvent::UiOperation(UiOperation::new(
            "/_logbook/rows",
            UiOperationKind::LiveRegion,
            200,
            1,
            ms(2),
        )),
    );
    feed(
        &logbook,
        &context,
        request_event("req-ui", "POST", "/deals/7/stage", 200),
    );
    let batch = logbook.last_request().await.unwrap();
    let ops = batch.ui_operations();
    assert_eq!(ops.len(), 2, "{ops:?}");
    assert_eq!(ops[0].kind, "form_post");
    assert_eq!(ops[0].ops, 3);
    let entries: Vec<_> = batch
        .entries()
        .iter()
        .filter(|entry| entry.kind() == EntryKind::Ui)
        .collect();
    assert!(!entries[0].failed);
    assert!(entries[1].failed, "a 5xx answer is a failed operation");
    assert_eq!(batch.request().unwrap().summary.ui_ops, 2);
}

#[tokio::test]
async fn the_new_kinds_filter_by_type_and_by_text() {
    let logbook = logbook();
    let outside = EventContext::default();
    feed(
        &logbook,
        &outside,
        TelemetryEvent::AiCall(AiCall::new(
            "anthropic",
            "claude-sonnet-4-5",
            "generate",
            ms(5),
        )),
    );
    feed(
        &logbook,
        &outside,
        TelemetryEvent::SignIn(SignInEvent::refused(
            SignInMethod::Passkey,
            "policy_refused",
        )),
    );
    feed(
        &logbook,
        &outside,
        TelemetryEvent::Search(SearchQueried::new(
            "memory",
            "deals",
            SearchMode::Hybrid,
            "x",
            ms(1),
        )),
    );
    let count = |filter: &str| {
        let logbook = logbook.clone();
        let filter = Filter::parse(filter).unwrap();
        async move {
            logbook
                .entries(&ListQuery::new(filter, 10))
                .await
                .unwrap()
                .len()
        }
    };
    assert_eq!(count("type:ai").await, 1);
    assert_eq!(count("type:sign-ins").await, 1);
    assert_eq!(count("type:searches").await, 1);
    assert_eq!(count("type:ai type:sign_in").await, 2);
    assert_eq!(count("type:broadcast").await, 0);
    assert_eq!(
        count("anthropic").await,
        1,
        "free text reaches the provider"
    );
    assert_eq!(count("policy_refused").await, 1, "and a sign-in's reason");
    assert_eq!(count("failed").await, 1);
    let counts = logbook.counts().await.unwrap();
    assert_eq!(counts.get(&EntryKind::Ai), Some(&1));
    assert_eq!(counts.get(&EntryKind::SignIn), Some(&1));
}

#[test]
fn the_new_watchers_decide_the_interest_and_are_config_keys() {
    let all = navree_logbook::LogbookOptions::default().interest();
    for kind in [
        EventKind::AiCall,
        EventKind::Broadcast,
        EventKind::StorageOp,
        EventKind::WebhookDelivery,
        EventKind::Search,
        EventKind::SignIn,
        EventKind::UiOperation,
    ] {
        assert!(all.contains(kind), "{kind}");
    }
    let options: navree_logbook::LogbookOptions = serde_json::from_value(json!({
        "watchers": { "ai": false, "sign_ins": false, "ui": false }
    }))
    .unwrap();
    let narrowed = options.interest();
    assert!(!narrowed.contains(EventKind::AiCall));
    assert!(!narrowed.contains(EventKind::SignIn));
    assert!(!narrowed.contains(EventKind::UiOperation));
    assert!(narrowed.contains(EventKind::Search));
    assert!(narrowed.contains(EventKind::StorageOp));
}
