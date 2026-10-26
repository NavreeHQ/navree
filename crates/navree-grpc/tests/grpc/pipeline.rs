//! The call pipeline: authentication, access rules, guards, interceptors,
//! validation, the unit of work, errors, request ids and deadlines.

use std::sync::Arc;
use std::time::{Duration, Instant};

use navree_contracts::{AuditKind, AuditSink};
use navree_grpc::testing::TestChannel;
use navree_grpc::{Code, GrpcModule};
use navree_testing::{TestApp, principal};
use tonic_types::StatusExt;
use uuid::Uuid;

use crate::support::pb::deals::deals_client::DealsClient;
use crate::support::pb::deals::{GetDealRequest, ListDealsRequest, MoveDealRequest};
use crate::support::pb::ping::PingRequest;
use crate::support::pb::ping::ping_client::PingClient;
use crate::support::{
    GuardedPingModule, RecordingAudit, deals_app, find_deal, insert_deal, ping_app,
};

fn ping(message: &str) -> PingRequest {
    PingRequest {
        message: message.to_owned(),
        sleep_ms: 0,
    }
}

/// The docs' test: guests, strangers and owners.
#[tokio::test]
async fn get_deal_checks_access() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme renewal", rep).await;
    let request = || GetDealRequest {
        id: deal.id.to_string(),
    };

    let mut guest = DealsClient::new(TestChannel::new(&app));
    let status = guest.get_deal(request()).await.unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);

    let mut stranger =
        DealsClient::new(TestChannel::new(&app).acting_as(principal(Uuid::now_v7())));
    let status = stranger.get_deal(request()).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);

    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)));
    let found = owner.get_deal(request()).await.unwrap().into_inner();
    assert_eq!(found.owner_id, rep.to_string());
    assert_eq!(found.title, "Acme renewal");
    assert_eq!(found.amount_cents, 1_200_000);

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn record_checks_read_the_id_field() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)));

    let missing = owner
        .get_deal(GetDealRequest {
            id: Uuid::now_v7().to_string(),
        })
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);

    let empty = owner
        .get_deal(GetDealRequest { id: String::new() })
        .await
        .unwrap_err();
    assert_eq!(empty.code(), Code::InvalidArgument);
    assert_eq!(empty.message(), "id is required");

    let not_a_uuid = owner
        .get_deal(GetDealRequest { id: "a/b c".into() })
        .await
        .unwrap_err();
    assert_eq!(not_a_uuid.code(), Code::NotFound);
}

#[tokio::test]
async fn lists_are_filtered_by_the_same_rules() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    insert_deal(&app, "Beta", rep).await;
    insert_deal(&app, "Alpha", rep).await;
    insert_deal(&app, "Someone else's", Uuid::now_v7()).await;

    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)));
    let list = owner
        .list_deals(ListDealsRequest::default())
        .await
        .unwrap()
        .into_inner();
    let titles: Vec<&str> = list.deals.iter().map(|d| d.title.as_str()).collect();
    assert_eq!(titles, ["Alpha", "Beta"]);
    assert_eq!(list.total, 2);
}

#[tokio::test]
async fn valid_messages_report_every_broken_rule() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme", rep).await;
    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)));

    let status = owner
        .move_deal(MoveDealRequest {
            id: deal.id.to_string(),
            stage: String::new(),
            position: Some(-1),
            lost_reason: None,
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().contains("position must not be less than 0"),
        "{}",
        status.message()
    );
    let bad_request = status.get_details_bad_request().expect("a BadRequest detail");
    let fields: Vec<&str> = bad_request
        .field_violations
        .iter()
        .map(|v| v.field.as_str())
        .collect();
    assert_eq!(fields, ["position", "stage"]);
    assert_eq!(
        bad_request.field_violations[0].description,
        "position must not be less than 0"
    );
    assert_eq!(find_deal(&app, deal.id).await.stage, "lead", "nothing ran");
}

#[tokio::test]
async fn the_unit_of_work_commits_on_ok_and_rolls_back_on_errors() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme", rep).await;
    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)));
    let request = |stage: &str, lost_reason: Option<&str>| MoveDealRequest {
        id: deal.id.to_string(),
        stage: stage.to_owned(),
        position: Some(1),
        lost_reason: lost_reason.map(str::to_owned),
    };

    let moved = owner.move_deal(request("won", None)).await.unwrap();
    assert_eq!(moved.into_inner().stage, "won");
    assert_eq!(find_deal(&app, deal.id).await.stage, "won");

    let status = owner
        .move_deal(request("lost", Some("fail after writing")))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::AlreadyExists);
    assert_eq!(status.message(), "the move was undone");
    assert_eq!(
        find_deal(&app, deal.id).await.stage,
        "won",
        "the write of the failed call was rolled back"
    );

    let mut stranger =
        DealsClient::new(TestChannel::new(&app).acting_as(principal(Uuid::now_v7())));
    let status = stranger.move_deal(request("lost", None)).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn app_errors_become_statuses_of_the_same_meaning() {
    let app = ping_app().build().await;
    let mut client = PingClient::new(TestChannel::new(&app));

    let status = client.ping(ping("conflict")).await.unwrap_err();
    assert_eq!((status.code(), status.message()), (Code::AlreadyExists, "already pinged"));

    let status = client.ping(ping("boom")).await.unwrap_err();
    assert_eq!(
        (status.code(), status.message()),
        (Code::Internal, "Internal server error"),
        "a server error's detail never reaches the client"
    );

    let status = client.ping(ping("upstream")).await.unwrap_err();
    assert_eq!(
        (status.code(), status.message()),
        (Code::FailedPrecondition, "upstream says no"),
        "`?` on a Status keeps its meaning"
    );

    let status = client.ping(ping("missing")).await.unwrap_err();
    assert_eq!(status.code(), Code::NotFound, "database errors are mapped");
}

#[tokio::test]
async fn the_request_id_is_kept_or_generated_and_returned() {
    let app = ping_app().build().await;

    let mut client = PingClient::new(TestChannel::new(&app).metadata("x-request-id", "req-123_abc"));
    let response = client.ping(ping("hi")).await.unwrap();
    assert_eq!(response.metadata().get("x-request-id").unwrap(), "req-123_abc");
    assert_eq!(response.into_inner().request_id, "req-123_abc");

    let mut client = PingClient::new(TestChannel::new(&app));
    let response = client.ping(ping("hi")).await.unwrap();
    let returned = response
        .metadata()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(returned.len(), 32, "a generated UUID v7");
    assert_eq!(response.into_inner().request_id, returned);

    let mut client = PingClient::new(TestChannel::new(&app).metadata("x-request-id", "no spaces allowed"));
    let response = client.ping(ping("hi")).await.unwrap();
    assert_ne!(response.metadata().get("x-request-id").unwrap(), "no spaces allowed");
}

#[tokio::test]
async fn bearer_credentials_go_through_the_apps_authenticator() {
    let app = ping_app().build().await;
    let user = Uuid::now_v7();

    let mut client = PingClient::new(TestChannel::new(&app).bearer(&user.to_string()));
    let reply = client.ping(ping("hi")).await.unwrap().into_inner();
    assert_eq!(reply.user_id, user.to_string());
    assert_eq!(reply.authorization, format!("Bearer {user}"));

    let mut client = PingClient::new(TestChannel::new(&app).bearer("invalid"));
    let status = client.ping(ping("hi")).await.unwrap_err();
    assert_eq!(
        (status.code(), status.message()),
        (Code::Unauthenticated, "Invalid token"),
        "invalid credentials end the call, even on a public method"
    );

    let mut guest = PingClient::new(TestChannel::new(&app));
    assert_eq!(guest.ping(ping("hi")).await.unwrap().into_inner().user_id, "");
}

#[tokio::test]
async fn interceptors_run_first_and_guards_after_authentication() {
    let audit = RecordingAudit::default();
    let app = TestApp::builder()
        .module(GrpcModule::from_config())
        .module(GuardedPingModule)
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
        .build()
        .await;
    let user = Uuid::now_v7();

    let mut guest = PingClient::new(TestChannel::new(&app));
    let status = guest.ping(ping("hi")).await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition, "the interceptor ran first");
    assert_eq!(status.message(), "x-client-version is required");

    let mut guest = PingClient::new(TestChannel::new(&app).metadata("x-client-version", "1.4"));
    assert_eq!(guest.ping(ping("hi")).await.unwrap_err().code(), Code::Unauthenticated);

    let versioned = || TestChannel::new(&app).metadata("x-client-version", "1.4");
    let mut user_without_version = PingClient::new(versioned().acting_as(principal(user)));
    let status = user_without_version.ping(ping("hi")).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied, "the guard refused");
    let denied = audit
        .entries()
        .into_iter()
        .find(|entry| entry.action == "access.denied")
        .expect("the refusal is audited");
    assert_eq!(denied.kind, AuditKind::Security);
    assert_eq!(denied.actor_id, Some(user));
    let metadata = denied.metadata;
    assert_eq!(metadata["route"], "/navree.test.v1.Ping/Ping");
    assert_eq!(metadata["check"], "guard");

    let mut allowed = PingClient::new(
        versioned()
            .metadata("x-api-version", "2")
            .acting_as(principal(user)),
    );
    let reply = allowed.ping(ping("hi")).await.unwrap().into_inner();
    assert_eq!(reply.user_id, user.to_string());
}

#[tokio::test]
async fn deadlines_cut_calls_off() {
    let app = ping_app().config("grpc.timeout_secs", 1).build().await;
    let mut client = PingClient::new(TestChannel::new(&app));

    let mut request = tonic::Request::new(PingRequest {
        message: "slow".into(),
        sleep_ms: 5_000,
    });
    request.set_timeout(Duration::from_millis(100));
    let started = Instant::now();
    let status = client.ping(request).await.unwrap_err();
    assert_eq!(status.code(), Code::DeadlineExceeded);
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());

    // Without grpc-timeout, grpc.timeout_secs applies.
    let started = Instant::now();
    let status = client
        .ping(PingRequest {
            message: "slow".into(),
            sleep_ms: 5_000,
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::DeadlineExceeded);
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(900) && elapsed < Duration::from_secs(4), "{elapsed:?}");
}
