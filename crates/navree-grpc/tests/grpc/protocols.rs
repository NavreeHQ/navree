//! gRPC-Web and Connect on the HTTP port, next to the app's routes, sent
//! through the app's full router (`TestClient`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use http::Method;
use navree_grpc::GrpcModule;
use navree_testing::{TestApp, principal};
use prost::Message;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::pb::deals::{Deal, DealEvent, GetDealRequest};
use crate::support::{GuardedPingModule, deals_app, insert_deal};

const GET_DEAL: &str = "/crm.deals.v1.Deals/GetDeal";

/// A length-prefixed message (gRPC framing, Connect's envelopes).
fn frame(flag: u8, payload: &[u8]) -> Bytes {
    let mut out = BytesMut::new();
    out.put_u8(flag);
    out.put_u32(payload.len() as u32);
    out.put_slice(payload);
    out.freeze()
}

/// Every frame in `body`.
fn frames(mut body: Bytes) -> Vec<(u8, Bytes)> {
    let mut out = Vec::new();
    while body.len() >= 5 {
        let flag = body.get_u8();
        let length = body.get_u32() as usize;
        out.push((flag, body.split_to(length)));
    }
    assert!(body.is_empty(), "trailing bytes in the body");
    out
}

#[tokio::test]
async fn grpc_web_calls_are_answered_on_the_http_port() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme renewal", rep).await;
    let message = GetDealRequest {
        id: deal.id.to_string(),
    }
    .encode_to_vec();

    let response = app
        .acting_as(principal(rep))
        .post(GET_DEAL)
        .header("content-type", "application/grpc-web+proto")
        .header("x-grpc-web", "1")
        .body(frame(0, &message))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-type"),
        Some("application/grpc-web+proto")
    );
    assert!(response.header("x-request-id").is_some());
    let frames = frames(response.bytes().clone());
    assert_eq!(frames.len(), 2, "the message, then the trailers");
    let found = Deal::decode(frames[0].1.clone()).unwrap();
    assert_eq!(found.title, "Acme renewal");
    assert_eq!(frames[1].0, 0x80, "a trailers frame");
    let trailers = String::from_utf8(frames[1].1.to_vec()).unwrap();
    assert!(trailers.contains("grpc-status:0"), "{trailers}");

    // A guest gets the status in the trailers-only response headers.
    let response = app
        .client()
        .post(GET_DEAL)
        .header("content-type", "application/grpc-web+proto")
        .body(frame(0, &message))
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("grpc-status"), Some("16"));
}

#[tokio::test]
async fn connect_unary_calls_carry_json_or_protobuf() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme renewal", rep).await;
    let owner = app.acting_as(principal(rep));

    let response = owner
        .post(GET_DEAL)
        .json(&json!({ "id": deal.id }))
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(
        response.json_value(),
        json!({
            "id": deal.id.to_string(),
            "title": "Acme renewal",
            "amountCents": "1200000",
            "currency": "USD",
            "stage": "lead",
            "ownerId": rep.to_string(),
        }),
        "protobuf's JSON mapping: camelCase names, 64-bit integers as strings"
    );

    let response = owner
        .post(GET_DEAL)
        .header("content-type", "application/proto")
        .header("connect-protocol-version", "1")
        .body(
            GetDealRequest {
                id: deal.id.to_string(),
            }
            .encode_to_vec(),
        )
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("content-type"), Some("application/proto"));
    assert_eq!(
        Deal::decode(response.bytes().clone()).unwrap().title,
        "Acme renewal"
    );
}

#[tokio::test]
async fn connect_errors_use_http_statuses_and_a_json_body() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme", rep).await;

    let guest = app
        .client()
        .post(GET_DEAL)
        .json(&json!({ "id": deal.id }))
        .send()
        .await
        .assert_status(401);
    assert_eq!(
        guest.json_value(),
        json!({ "code": "unauthenticated", "message": "Unauthorized" })
    );

    let owner = app.acting_as(principal(rep));
    let missing = owner
        .post(GET_DEAL)
        .json(&json!({ "id": Uuid::now_v7() }))
        .send()
        .await
        .assert_status(404);
    assert_eq!(missing.json_value()["code"], "not_found");

    let invalid = owner
        .post("/crm.deals.v1.Deals/MoveDeal")
        .json(&json!({ "id": deal.id, "stage": "", "position": "-1" }))
        .send()
        .await
        .assert_status(400);
    let body = invalid.json_value();
    assert_eq!(body["code"], "invalid_argument");
    assert_eq!(body["details"][0]["type"], "google.rpc.BadRequest");
    let detail = STANDARD_NO_PAD
        .decode(body["details"][0]["value"].as_str().unwrap())
        .unwrap();
    let bad_request = tonic_types::pb::BadRequest::decode(detail.as_slice()).unwrap();
    assert_eq!(bad_request.field_violations[0].field, "position");

    let malformed = owner
        .post(GET_DEAL)
        .json(&json!({ "id": 5 }))
        .send()
        .await
        .assert_status(400);
    assert_eq!(malformed.json_value()["code"], "invalid_argument");
}

#[tokio::test]
async fn connect_streams_use_envelopes_and_an_end_message() {
    let app = deals_app().build().await;
    let user = app.acting_as(principal(Uuid::now_v7()));

    let deals = [json!({ "title": "One" }), json!({ "title": "Two" })];
    let mut body = BytesMut::new();
    for deal in &deals {
        body.put(frame(0, &serde_json::to_vec(deal).unwrap()));
    }
    let response = user
        .post("/crm.deals.v1.Deals/ImportDeals")
        .header("content-type", "application/connect+json")
        .body(body.freeze())
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-type"),
        Some("application/connect+json")
    );
    let frames = frames(response.bytes().clone());
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].0, 0);
    let reply: Value = serde_json::from_slice(&frames[0].1).unwrap();
    assert_eq!(reply, json!({ "imported": 2 }));
    assert_eq!(frames[1].0, 0x02, "the end-of-stream message");
    assert_eq!(
        serde_json::from_slice::<Value>(&frames[1].1).unwrap(),
        json!({})
    );

    // An error ends the stream inside the body, with HTTP 200.
    let response = user
        .post("/crm.deals.v1.Deals/ImportDeals")
        .header("content-type", "application/connect+json")
        .body(frame(0, b"{}"))
        .send()
        .await
        .assert_status(200);
    let frames = self::frames(response.bytes().clone());
    let end: Value = serde_json::from_slice(&frames.last().unwrap().1).unwrap();
    assert_eq!(end["error"]["code"], "invalid_argument");
    assert_eq!(end["error"]["message"], "a deal needs a title");

    // Protobuf envelopes pass through unchanged (a bidirectional echo).
    let events = [
        DealEvent {
            deal_id: "1".into(),
            stage: "won".into(),
        },
        DealEvent {
            deal_id: "2".into(),
            stage: "lost".into(),
        },
    ];
    let mut body = BytesMut::new();
    for event in &events {
        body.put(frame(0, &event.encode_to_vec()));
    }
    let response = user
        .post("/crm.deals.v1.Deals/Echo")
        .header("content-type", "application/connect+proto")
        .body(body.freeze())
        .send()
        .await
        .assert_status(200);
    let frames = self::frames(response.bytes().clone());
    let echoed: Vec<DealEvent> = frames[..2]
        .iter()
        .map(|(_, payload)| DealEvent::decode(payload.clone()).unwrap())
        .collect();
    assert_eq!(echoed, events);
    assert_eq!(frames[2].0, 0x02);
}

#[tokio::test]
async fn browsers_get_the_grpc_web_and_connect_headers_in_cors() {
    let app = deals_app().build().await;
    let response = app
        .client()
        .request(Method::OPTIONS, GET_DEAL)
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "content-type,x-grpc-web,x-user-agent,connect-protocol-version",
        )
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("access-control-allow-origin"),
        Some("http://localhost:3000")
    );
    let allowed = response
        .header("access-control-allow-headers")
        .unwrap_or_default();
    for header in [
        "x-grpc-web",
        "x-user-agent",
        "connect-protocol-version",
        "grpc-timeout",
    ] {
        assert!(allowed.contains(header), "{header} in {allowed}");
    }

    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme", rep).await;
    let response = app
        .acting_as(principal(rep))
        .post(GET_DEAL)
        .json(&json!({ "id": deal.id }))
        .send()
        .await
        .assert_status(200);
    let exposed = response
        .header("access-control-expose-headers")
        .unwrap_or_default();
    assert!(
        exposed.contains("grpc-status") && exposed.contains("x-request-id"),
        "{exposed}"
    );
}

#[tokio::test]
async fn the_routes_keep_everything_else() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();

    app.acting_as(principal(rep))
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200);
    app.client()
        .post("/api/deals")
        .json(&json!({ "title": "x" }))
        .send()
        .await
        .assert_error(404, None);

    // A gRPC request to an unknown service is a gRPC answer, not a route's.
    let response = app
        .client()
        .post("/crm.other.v1.Nothing/Call")
        .header("content-type", "application/grpc")
        .body(frame(0, b""))
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("grpc-status"), Some("12"), "UNIMPLEMENTED");
}

#[tokio::test]
async fn grpc_web_and_connect_can_be_turned_off() {
    let app = deals_app()
        .config("grpc.web", false)
        .config("grpc.connect", false)
        .build()
        .await;
    app.client()
        .post(GET_DEAL)
        .header("content-type", "application/grpc-web+proto")
        .body(frame(0, b""))
        .send()
        .await
        .assert_error(404, None);
    app.client()
        .post(GET_DEAL)
        .json(&json!({ "id": "x" }))
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn connect_json_needs_the_descriptors_and_respects_the_size_limit() {
    let app = TestApp::builder()
        .config("grpc.max_message_bytes", 64)
        .module(GrpcModule::from_config())
        .module(GuardedPingModule)
        .build()
        .await;
    let response = app
        .client()
        .post("/navree.test.v1.Ping/Ping")
        .json(&json!({ "message": "hi" }))
        .send()
        .await
        .assert_status(501);
    assert_eq!(response.json_value()["code"], "unimplemented");

    let response = app
        .client()
        .post("/navree.test.v1.Ping/Ping")
        .header("content-type", "application/proto")
        .body(vec![0u8; 100])
        .send()
        .await
        .assert_status(429);
    assert_eq!(response.json_value()["code"], "resource_exhausted");
}
