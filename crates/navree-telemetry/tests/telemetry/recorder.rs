//! `TelemetryRecorder`: the in-memory sink for tests.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_telemetry::{
    AiCall, BroadcastSent, CacheEvent, CacheOperation, EventEmitted, EventKind, ExceptionReported,
    HttpClientRequest, JobFailed, JobProcessed, LogLevel, LogRecord, MailSent, NotificationSent,
    QueryExecuted, RequestHandled, ScheduledTaskRan, SearchMode, SearchQueried, SignInEvent,
    SignInMethod, StorageOp, StorageOperation, Telemetry, TelemetryEvent, TelemetryRecorder,
    UiOperation, UiOperationKind, WebhookDelivery,
};

fn every_event() -> Vec<TelemetryEvent> {
    let ms = Duration::from_millis;
    vec![
        TelemetryEvent::RequestHandled(RequestHandled {
            method: "GET".into(),
            route: Some("/api/v1/deals/{id}".into()),
            path: "/api/v1/deals/7".into(),
            status: 200,
            duration: ms(12),
            user_id: Some("u-1".into()),
            workspace_id: None,
            request_id: "req-1".into(),
            client_ip: Some("127.0.0.1".into()),
        }),
        TelemetryEvent::QueryExecuted(QueryExecuted {
            sql: "SELECT 1".into(),
            duration: ms(1),
            pool_wait: std::time::Duration::ZERO,
            rows: None,
            connection: "sqlite".into(),
            failed: false,
        }),
        TelemetryEvent::JobProcessed(JobProcessed {
            queue: "default".into(),
            job_type: "mail.send".into(),
            job_id: "j-1".into(),
            attempt: 1,
            duration: ms(30),
        }),
        TelemetryEvent::JobFailed(JobFailed {
            queue: "default".into(),
            job_type: "mail.send".into(),
            job_id: "j-2".into(),
            attempt: 3,
            duration: ms(30),
            error: "smtp down".into(),
            will_retry: false,
        }),
        TelemetryEvent::ScheduledTaskRan(ScheduledTaskRan {
            task: "audit:prune".into(),
            tick: Utc::now(),
            duration: ms(5),
            error: None,
        }),
        TelemetryEvent::MailSent(MailSent {
            to: vec!["ada@example.com".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            from: Some("crm@example.com".into()),
            subject: "Welcome".into(),
            text: Some("Hi".into()),
            html: None,
            transport: "array".into(),
            duration: ms(2),
        }),
        TelemetryEvent::NotificationSent(NotificationSent {
            notification_id: "n-1".into(),
            notification_type: "deal.won".into(),
            notifiable_type: "user".into(),
            notifiable_id: "u-1".into(),
            channel: "database".into(),
        }),
        TelemetryEvent::Cache(CacheEvent {
            operation: CacheOperation::Miss,
            key: "stats".into(),
            store: "memory".into(),
        }),
        TelemetryEvent::HttpClientRequest(HttpClientRequest {
            method: "GET".into(),
            url: "https://api.example.com/v1/rates".into(),
            status: Some(200),
            duration: ms(80),
            attempts: 1,
            error: None,
            span_id: None,
        }),
        TelemetryEvent::ExceptionReported(ExceptionReported {
            message: "boom".into(),
            chain: vec!["boom".into()],
            location: Some("src/deals.rs:10:5".into()),
            panic: true,
            request: None,
        }),
        TelemetryEvent::Log(LogRecord {
            level: LogLevel::Warn,
            target: "crm::deals".into(),
            message: "slow".into(),
            fields: Default::default(),
        }),
        TelemetryEvent::EventEmitted(EventEmitted {
            name: "deal.won".into(),
            payload: serde_json::json!({ "id": 7 }),
            listeners: vec!["crm::NotifyOwner".into()],
            queued: Vec::new(),
            failed: Vec::new(),
            duration: ms(1),
        }),
        TelemetryEvent::AiCall(
            AiCall::new("anthropic", "claude-sonnet-4-5", "generate", ms(900)).tokens(1200, 340),
        ),
        TelemetryEvent::Broadcast(BroadcastSent::new(
            ["private-crm.deals"],
            "deal.updated",
            3,
            ms(1),
        )),
        TelemetryEvent::StorageOp(
            StorageOp::new("uploads", "local", StorageOperation::Write, "kyc/pan.pdf", ms(4))
                .bytes(52_314),
        ),
        TelemetryEvent::WebhookDelivery(WebhookDelivery::new(
            "ep-1",
            "invoice.paid",
            1,
            200,
            ms(40),
        )),
        TelemetryEvent::Search(
            SearchQueried::new("memory", "deals", SearchMode::Text, "Ludhiana", ms(2)).hits(7),
        ),
        TelemetryEvent::SignIn(SignInEvent::succeeded(
            SignInMethod::Password,
            "u-1",
            ms(90),
        )),
        TelemetryEvent::UiOperation(UiOperation::new(
            "/deals/{id}/stage",
            UiOperationKind::FormPost,
            200,
            2,
            ms(8),
        )),
    ]
}

#[test]
fn typed_accessors_return_each_kind() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    for event in every_event() {
        telemetry.emit(event);
    }
    assert_eq!(recorder.len(), 19);
    assert_eq!(
        recorder.requests()[0].route.as_deref(),
        Some("/api/v1/deals/{id}")
    );
    assert_eq!(recorder.queries()[0].sql, "SELECT 1");
    assert_eq!(recorder.jobs_processed()[0].job_type, "mail.send");
    assert_eq!(recorder.jobs_failed()[0].error, "smtp down");
    assert_eq!(recorder.scheduled_tasks()[0].task, "audit:prune");
    assert_eq!(recorder.mails()[0].subject, "Welcome");
    assert_eq!(recorder.notifications()[0].channel, "database");
    assert_eq!(recorder.cache_events()[0].operation, CacheOperation::Miss);
    assert_eq!(recorder.http_client_requests()[0].status, Some(200));
    assert_eq!(recorder.exceptions()[0].message, "boom");
    assert_eq!(recorder.logs()[0].level, LogLevel::Warn);
    assert_eq!(recorder.of_kind(EventKind::JobFailed).len(), 1);
    assert_eq!(recorder.emitted_events()[0].name, "deal.won");
    assert_eq!(recorder.ai_calls()[0].model, "claude-sonnet-4-5");
    assert_eq!(recorder.broadcasts()[0].delivered, 3);
    assert_eq!(recorder.storage_ops()[0].bytes, Some(52_314));
    assert_eq!(recorder.webhook_deliveries()[0].status, Some(200));
    assert_eq!(recorder.searches()[0].hits, Some(7));
    assert_eq!(recorder.sign_ins()[0].method, SignInMethod::Password);
    assert_eq!(recorder.ui_operations()[0].kind, UiOperationKind::FormPost);
    assert_eq!(recorder.events().len(), 19);

    recorder.clear();
    assert!(recorder.is_empty());
}

#[test]
fn a_recorder_can_narrow_its_interest() {
    let recorder = TelemetryRecorder::only([EventKind::RequestHandled]);
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    assert!(!telemetry.enabled(EventKind::Log));
    for event in every_event() {
        telemetry.emit(event);
    }
    assert_eq!(recorder.len(), 1);
}

#[test]
fn clones_share_their_records() {
    let recorder = TelemetryRecorder::new();
    let clone = recorder.clone();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(clone)]);
    telemetry.emit(every_event().remove(0));
    assert_eq!(recorder.len(), 1);
}

#[tokio::test]
async fn wait_for_returns_once_an_event_arrives() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let background = telemetry.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        background.emit(every_event().remove(1));
    });
    let found = recorder
        .wait_for(EventKind::QueryExecuted, Duration::from_secs(5))
        .await;
    assert!(found, "the query arrived");
    let missing = recorder
        .wait_for(EventKind::MailSent, Duration::from_millis(30))
        .await;
    assert!(!missing);
}
