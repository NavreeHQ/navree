//! Reaching the sockets from other processes (`queue:work`, the
//! scheduler, commands) through the relay, and ending a user's
//! subscriptions when what they may see changed.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use http::{Request, StatusCode};
use keel_contracts::Broadcaster;
use keel_realtime::RealtimeOptions;
use serde_json::json;
use uuid::Uuid;

use crate::support::{Server, app, options};

/// A second app of the same deployment that does not serve the sockets
/// (as `queue:work` next to `serve`): it relays to `server`.
async fn worker(server: &Server) -> keel_kernel::App {
    app(RealtimeOptions {
        relay_url: format!("http://{}", server.addr),
        ..options()
    })
    .boot()
    .await
    .expect("the worker app boots")
}

#[tokio::test]
async fn broadcasts_from_a_worker_reach_the_serving_process() {
    let server = Server::start().await;
    let user = Uuid::now_v7();
    let mut client = server.connect().await;
    let channel = format!("private-users.{user}");
    server.join(&mut client, user, &channel).await;

    let worker = worker(&server).await;
    let broadcaster = worker
        .ctx()
        .get::<dyn Broadcaster>()
        .await
        .expect("dyn Broadcaster");
    broadcaster
        .broadcast(
            vec![channel.clone()],
            "notification.created".to_owned(),
            json!({ "title": "Deal assigned" }),
        )
        .await
        .expect("the relay accepts the broadcast");
    let event = client.expect("notification.created").await;
    assert_eq!(event.channel.as_deref(), Some(channel.as_str()));
    assert_eq!(event.data, json!({ "title": "Deal assigned" }));

    // Reserved event names are refused before anything is sent.
    assert!(
        broadcaster
            .broadcast(vec![channel], "pusher:error".to_owned(), json!({}))
            .await
            .is_err()
    );
    client.assert_alive().await;
}

/// The relay endpoint delivers only correctly signed, fresh messages for
/// this app.
#[tokio::test]
async fn the_relay_endpoint_checks_its_signature() {
    let server = Server::start().await;
    let mut client = server.connect().await;
    server
        .join(&mut client, Uuid::now_v7(), "private-crm.deals")
        .await;
    let body = json!({
        "type": "broadcast",
        "channels": ["private-crm.deals"],
        "event": "deal.updated",
        "data": { "deal": "forged" },
    })
    .to_string();
    let post = |path: &'static str, signature: Option<&'static str>| {
        let mut request = Request::post(path).header("content-type", "application/json");
        if let Some(signature) = signature {
            request = request.header("x-keel-relay-signature", signature);
        }
        request.body(Body::from(body.clone())).unwrap()
    };
    let unsigned = server.send(post("/apps/test/events", None)).await;
    assert_eq!(
        unsigned.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        unsigned.json
    );
    let forged = server
        .send(post("/apps/test/events", Some("t=1,v1=00ff")))
        .await;
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);
    let other_app = server.send(post("/apps/other/events", None)).await;
    assert_eq!(other_app.status, StatusCode::NOT_FOUND);
    client.expect_silence(Duration::from_millis(200)).await;
}

/// After a role change or a sign-out, the user's sockets close with a
/// reconnect code: their subscriptions end, and a client reconnecting must
/// authorize each private channel again. Other users stay connected.
#[tokio::test]
async fn ending_a_users_subscriptions_closes_their_sockets_with_a_reconnect_code() {
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let mut ada_socket = server.connect().await;
    server.join(&mut ada_socket, ada, "private-crm.deals").await;
    let mut bob_socket = server.connect().await;
    server.join(&mut bob_socket, bob, "private-crm.deals").await;

    let broadcaster: Arc<dyn Broadcaster> = server.realtime.clone();
    broadcaster
        .end_user_subscriptions(ada)
        .await
        .expect("ended");
    let message = ada_socket.expect_error(4200).await;
    assert!(message.contains("reconnect"), "{message}");
    assert_eq!(ada_socket.expect_close().await, 4200);
    assert_eq!(server.realtime.subscribers("private-crm.deals"), 1);

    server
        .realtime
        .broadcast(["private-crm.deals"], "deal.updated", json!({}))
        .unwrap();
    bob_socket.expect("deal.updated").await;

    // From a worker process, through the relay.
    let worker = worker(&server).await;
    let relayed = worker
        .ctx()
        .get::<dyn Broadcaster>()
        .await
        .expect("dyn Broadcaster");
    relayed.end_user_subscriptions(bob).await.expect("relayed");
    bob_socket.expect_error(4200).await;
    assert_eq!(server.realtime.connections(), 0);
}

#[tokio::test]
async fn without_a_shared_key_the_relay_is_off() {
    // No realtime.secret and no app key: nothing to sign with, so the
    // worker keeps its broadcasts (and says so), and the server's relay
    // endpoint does not exist.
    let server = Server::start_with(RealtimeOptions {
        secret: Default::default(),
        ..options()
    })
    .await;
    let reply = server
        .send(
            Request::post("/apps/test/events")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let worker = app(RealtimeOptions {
        secret: Default::default(),
        relay_url: format!("http://{}", server.addr),
        ..options()
    })
    .boot()
    .await
    .expect("boots");
    let broadcaster = worker.ctx().get::<dyn Broadcaster>().await.unwrap();
    broadcaster
        .broadcast(
            vec!["private-crm.deals".to_owned()],
            "deal.updated".to_owned(),
            json!({}),
        )
        .await
        .expect("delivered locally (to nobody)");
}
