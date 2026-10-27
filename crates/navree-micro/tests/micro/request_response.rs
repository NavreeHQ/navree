//! Message handlers through the whole pipeline, with `MicroFake`:
//! authentication, guards, access checks, validation, the unit of work,
//! replies, errors, deadlines and dedupe.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use http::StatusCode;
use navree_access::AccessModule;
use navree_contracts::AuditSink;
use navree_db::DatabaseModule;
use navree_http::AppError;
use navree_micro::{
    Codec, Envelope, MicroError, MicroFake, MicroModule, MicroOptions, Reply, ServiceClient,
    headers,
};
use navree_testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{
    DealView, DealsModule, GetDeal, NewNote, RecordingAudit, Rename, access, give_role,
    insert_deal, notes,
};

/// An app with the deals service on a `MicroFake`.
async fn deals_app(micro: &MicroFake) -> (TestApp, RecordingAudit) {
    let audit = RecordingAudit::default();
    let app = TestApp::builder()
        .temp_database()
        .config("micro.service", "deals")
        .module(DatabaseModule)
        .module(AccessModule::for_root(access()))
        .module(MicroModule::for_root(MicroOptions {
            service: "deals".into(),
            ..MicroOptions::from(micro.transport())
        }))
        .module(DealsModule)
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
        .build()
        .await;
    (app, audit)
}

#[tokio::test]
async fn can_from_reads_the_record_id_from_the_payload() {
    let micro = MicroFake::new();
    let (app, audit) = deals_app(&micro).await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, rep, "Acme renewal").await;

    // The owner reads it.
    let view: DealView = micro
        .client()
        .acting_as(principal(rep))
        .send("deals.get", &GetDeal { id: deal.id })
        .await
        .unwrap();
    assert_eq!(view.title, "Acme renewal");

    // Anonymous messages are refused.
    micro
        .client()
        .request("deals.get", &GetDeal { id: deal.id })
        .await
        .assert_error(401, "Unauthorized");

    // Someone else's deal is refused, and the denial is audited with the
    // pattern in place of the route.
    micro
        .client()
        .acting_as(principal(Uuid::now_v7()))
        .request("deals.get", &GetDeal { id: deal.id })
        .await
        .assert_error(403, "Forbidden");
    let denial = audit
        .entries()
        .into_iter()
        .find(|entry| entry.action == "access.denied")
        .expect("an access.denied entry");
    assert_eq!(denial.metadata["route"], "deals.get");

    // A missing record is a 404.
    micro
        .client()
        .acting_as(principal(rep))
        .request("deals.get", &GetDeal { id: Uuid::now_v7() })
        .await
        .assert_error(404, "Not Found");

    // A payload without the field is a 400.
    micro
        .client()
        .acting_as(principal(rep))
        .request("deals.get", &json!({ "title": "x" }))
        .await
        .assert_status(400);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn validation_runs_before_the_handler_and_the_unit_of_work_commits_on_ok() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, rep, "Acme renewal").await;
    let rename = |title: &str| Rename {
        id: deal.id,
        title: title.to_owned(),
    };

    let response = micro
        .client()
        .acting_as(principal(rep))
        .request("deals.rename", &rename("x"))
        .await
        .assert_validation_error("title");
    assert_eq!(
        response.json_value()["errors"],
        json!({ "title": ["title must be longer than or equal to 2 characters"] })
    );
    micro.assert_nothing_emitted();

    // A payload that does not decode is a 400 with serde's message for the field.
    micro
        .client()
        .acting_as(principal(rep))
        .request("deals.rename", &json!({ "id": deal.id }))
        .await
        .assert_validation_error("title");

    let renamed: DealView = micro
        .client()
        .acting_as(principal(rep))
        .send("deals.rename", &rename("Acme renewal 2027"))
        .await
        .unwrap();
    assert_eq!(renamed.title, "Acme renewal 2027");
    // Committed: read back through another message.
    let view: DealView = micro
        .client()
        .acting_as(principal(rep))
        .send("deals.get", &GetDeal { id: deal.id })
        .await
        .unwrap();
    assert_eq!(view.title, "Acme renewal 2027");
    // The event went through the outbox after the commit.
    micro.assert_emitted("deal.renamed", |event: &crate::support::DealRenamed| {
        event.deal_id == deal.id && event.title == "Acme renewal 2027"
    });
    let event = &micro.emitted_envelopes("deal.renamed")[0];
    assert_eq!(event.key(), Some(deal.id.to_string().as_str()));
    assert_eq!(event.header("navree-actor"), Some(rep.to_string().as_str()));
    assert_eq!(
        event.header("authorization"),
        None,
        "events never carry tokens"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_error_rolls_the_unit_of_work_back_and_drops_its_events() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;
    micro
        .client()
        .request(
            "notes.write_then_fail",
            &NewNote {
                body: "lost".into(),
            },
        )
        .await
        .assert_error(409, "not today");
    assert!(notes(&app).await.is_empty(), "rolled back");
    micro.assert_nothing_emitted();
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn roles_guards_and_authentication_work_as_on_routes() {
    let micro = MicroFake::new();
    let (app, audit) = deals_app(&micro).await;
    let user = Uuid::now_v7();

    let me: Uuid = micro
        .client()
        .acting_as(principal(user))
        .send("deals.whoami", &())
        .await
        .unwrap();
    assert_eq!(me, user);

    micro
        .client()
        .acting_as(principal(user))
        .request("deals.admin", &())
        .await
        .assert_error(403, "Forbidden");
    give_role(&app, user, "admin", &[]).await;
    micro
        .client()
        .acting_as(principal(user))
        .request("deals.admin", &())
        .await
        .assert_status(200)
        .assert_json_path("", json!("admin area"));

    micro
        .client()
        .request("deals.internal", &())
        .await
        .assert_error(403, "Forbidden");
    assert!(audit.actions().contains(&"access.denied".to_owned()));
    let ok: String = micro
        .client()
        .send("deals.internal", &())
        .header("x-service-key", "s3cret")
        .await
        .unwrap();
    assert_eq!(ok, "internal");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn handlers_see_the_message_context() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;
    let reply = micro
        .client()
        .request("deals.echo", &json!({ "n": 1 }))
        .await
        .assert_status(200);
    let body = reply.json_value();
    assert_eq!(body["payload"], json!({ "n": 1 }));
    assert_eq!(body["pattern"], "deals.echo");
    assert_eq!(body["service"], "micro-fake");
    assert_eq!(body["attempt"], 1);
    assert_eq!(body["deadline"], true);
    assert!(body["message_id"].as_str().is_some_and(|id| id.len() == 36));
    let traceparent = body["traceparent"].as_str().expect("a traceparent");
    let mut fields = traceparent.split('-').skip(1);
    let (trace_id, span_id) = (fields.next().unwrap(), fields.next().unwrap());
    assert_eq!(
        body["trace_id"], trace_id,
        "the handler continues the sender's trace"
    );
    assert_eq!(body["parent_span_id"], span_id, "under the sender's span");
    assert_eq!(body["request_id_seen"], body["request_id"]);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_message_sent_from_a_handler_stays_in_the_trace() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;
    micro.on("companies.get", Reply::json(&json!({ "ok": true })));
    let _: Value = micro
        .client()
        .send("deals.relay", &"companies.get")
        .header(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        )
        .await
        .unwrap();
    let relayed = &micro.sent_envelopes("companies.get")[0];
    let traceparent = relayed.header("traceparent").expect("a traceparent");
    assert!(
        traceparent.starts_with("00-0af7651916cd43dd8448eb211c80319c-"),
        "{traceparent}"
    );
    assert!(
        !traceparent.contains("b7ad6b7169203331"),
        "a span of its own: {traceparent}"
    );
    assert!(
        traceparent.ends_with("-01"),
        "the sampling decision travels: {traceparent}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn server_errors_keep_their_internals_on_the_handler_side() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;
    let response = micro
        .client()
        .request("deals.broken", &())
        .await
        .assert_error(500, "Internal server error");
    assert!(!response.text().contains("hunter2"));
    match micro.client().send::<_, Value>("deals.broken", &()).await {
        Err(MicroError::Remote(error)) => {
            assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(error.to_string(), "500 Internal server error");
        }
        other => panic!("{other:?}"),
    }
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_handler_past_its_time_limit_is_cut_off_and_rolled_back() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;
    micro
        .client()
        .request("deals.slow", &())
        .await
        .assert_error(503, "The handler did not finish in time.");
    assert!(notes(&app).await.is_empty());

    // The caller's deadline bounds it too: the caller times out.
    let outcome = micro
        .client()
        .send::<_, Value>("deals.slow", &())
        .timeout(Duration::from_millis(50))
        .await;
    assert!(
        matches!(outcome, Err(MicroError::Timeout { .. })),
        "{outcome:?}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn dedupe_answers_a_repeated_message_id_with_the_stored_reply() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;
    let transport = app.get::<ServiceClient>().await.transport().clone();
    let message_id = Uuid::now_v7().to_string();
    let request = |body: &str| {
        let mut envelope = Envelope::new("notes.add", format!(r#"{{"body":"{body}"}}"#))
            .with_header(headers::MESSAGE_ID, message_id.clone())
            .with_header(headers::CONTENT_TYPE, "application/json");
        envelope.set_deadline(SystemTime::now() + Duration::from_secs(5));
        envelope
    };

    let first = transport.request(request("one")).await.unwrap();
    assert_eq!(first.status(), Some(200));
    assert_eq!(first.payload().as_ref(), br#"{"notes":1}"#);
    // A retry of the same message (same id) gets the stored reply; the
    // handler does not run again.
    let again = transport.request(request("one")).await.unwrap();
    assert_eq!(again.status(), Some(200));
    assert_eq!(again.payload().as_ref(), br#"{"notes":1}"#);
    assert_eq!(notes(&app).await.len(), 1);

    // A new message runs it.
    let next: Value = micro
        .client()
        .send("notes.add", &NewNote { body: "two".into() })
        .await
        .unwrap();
    assert_eq!(next, json!({ "notes": 2 }));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn msgpack_payloads_and_replies() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(AccessModule::for_root(access()))
        .module(MicroModule::for_root(MicroOptions {
            codec: Codec::Msgpack,
            ..MicroOptions::from(micro.transport())
        }))
        .module(DealsModule)
        .build()
        .await;
    let client = app.get::<ServiceClient>().await;
    let reply: Value = client.send("deals.echo", &json!({ "n": 2 })).await.unwrap();
    assert_eq!(reply["payload"], json!({ "n": 2 }));
    let sent = &micro.sent_envelopes("deals.echo")[0];
    assert_eq!(sent.content_type(), "application/msgpack");
    let decoded: Value = Codec::Msgpack.decode(sent.payload()).unwrap();
    assert_eq!(decoded, json!({ "n": 2 }));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn stubs_answer_what_the_app_sends_elsewhere() {
    let micro = MicroFake::new();
    let (app, _) = deals_app(&micro).await;

    // Nobody answers an unknown pattern.
    micro
        .client()
        .request("deals.relay", &"companies.get")
        .await
        .assert_error(503, "Service Unavailable");

    micro.on("companies.get", Reply::json(&json!({ "name": "Acme" })));
    let reply: Value = micro
        .client()
        .send("deals.relay", &"companies.get")
        .await
        .unwrap();
    assert_eq!(reply, json!({ "name": "Acme" }));

    // A remote 4xx passes through with its status and message.
    micro.on("companies.get", Reply::error(AppError::not_found()));
    micro
        .client()
        .request("deals.relay", &"companies.get")
        .await
        .assert_error(404, "Not Found");

    // A remote 5xx is a 502.
    micro.on("companies.get", Reply::error(AppError::internal("db down")));
    micro
        .client()
        .request("deals.relay", &"companies.get")
        .await
        .assert_error(502, "Bad Gateway");

    // No reply before the deadline is a 504.
    micro.on("companies.get", Reply::timeout());
    micro
        .client()
        .request("deals.relay", &"companies.get")
        .await
        .assert_error(504, "Gateway Timeout");

    // Inline answers get the decoded payload.
    micro.on(
        "companies.get",
        Reply::from_fn(|input: Value| -> Result<Value, AppError> { Ok(json!({ "got": input })) }),
    );
    let reply: Value = micro
        .client()
        .send("deals.relay", &"companies.get")
        .await
        .unwrap();
    assert_eq!(reply, json!({ "got": {} }));

    // Every attempt is recorded, the unanswered first one included.
    micro.assert_sent_times("companies.get", 6);
    micro.assert_sent("companies.get", |payload: &Value| payload == &json!({}));
    let sent = &micro.sent_envelopes("companies.get")[0];
    assert_eq!(sent.service(), Some("deals"));
    assert!(sent.deadline().is_some(), "the inherited deadline travels");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn retries_resend_with_the_same_message_id() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .module(MicroModule::for_root(micro.transport()))
        .build()
        .await;
    let client = app.get::<ServiceClient>().await;
    micro.on(
        "companies.get",
        Reply::sequence([
            Reply::timeout(),
            Reply::timeout(),
            Reply::json(&json!("third time")),
        ]),
    );
    let reply: String = client
        .send("companies.get", &json!({ "id": 1 }))
        .timeout(Duration::from_millis(200))
        .retries(2)
        .await
        .unwrap();
    assert_eq!(reply, "third time");
    let sent = micro.sent_envelopes("companies.get");
    assert_eq!(sent.len(), 3);
    assert!(
        sent.windows(2)
            .all(|pair| pair[0].message_id() == pair[1].message_id())
    );

    // Without retries a timeout is the answer, and error replies are never retried.
    micro.on(
        "companies.get",
        Reply::sequence([Reply::error(AppError::conflict("no")), Reply::json(&1)]),
    );
    let outcome = client
        .send::<_, Value>("companies.get", &())
        .retries(3)
        .await;
    assert!(
        matches!(outcome, Err(MicroError::Remote(ref e)) if e.status() == StatusCode::CONFLICT)
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn sending_checks_the_pattern_and_the_size_limit() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .module(MicroModule::for_root(MicroOptions {
            max_message_bytes: 16,
            ..MicroOptions::from(micro.transport())
        }))
        .build()
        .await;
    let client = app.get::<ServiceClient>().await;
    let outcome = client.send::<_, Value>("deals.get", &"x".repeat(64)).await;
    assert!(
        matches!(
            outcome,
            Err(MicroError::PayloadTooLarge {
                size: 66,
                limit: 16
            })
        ),
        "{outcome:?}"
    );
    let outcome = client.send::<_, Value>("Deals.*", &()).await;
    assert!(
        matches!(outcome, Err(MicroError::InvalidPattern { .. })),
        "{outcome:?}"
    );
    let outcome = client.emit("deal.*", &()).await;
    assert!(
        matches!(outcome, Err(MicroError::InvalidPattern { .. })),
        "{outcome:?}"
    );
    micro.assert_nothing_sent();
    app.shutdown().await.unwrap();
}
