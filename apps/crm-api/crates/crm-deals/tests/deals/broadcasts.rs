//! `deal.updated` `{deal}` on `private-crm.deals` after every change, so
//! open boards refresh.

use std::time::Duration;

use crm_deals::DealsModule;
use crm_testkit::{Role, TestCrm};
use navree::config::Config;
use serde_json::json;

use crate::support::{
    CHANNEL, EVENT, Socket, company, create_deal, crm, id_of, move_deal, new_deal, serve,
};

#[tokio::test]
async fn deal_update_broadcasts() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let watcher = crm.login_as(Role::Rep2).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let addr = serve(&crm).await;
    // Another user's board, subscribed the way crm-web subscribes.
    let mut board = Socket::connect(addr).await;
    board.join(&watcher, CHANNEL).await;

    let created = create_deal(&rep, new_deal("Agra renewal", agra, 100)).await;
    let frame = board.expect(EVENT).await;
    assert_eq!(frame.channel.as_deref(), Some(CHANNEL));
    assert_eq!(frame.data, json!({ "deal": created }));
    let id = id_of(&created);

    let updated = rep
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "title": "Agra renewal 2027" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(board.expect(EVENT).await.data, json!({ "deal": updated }));

    let moved = move_deal(&rep, id, json!({ "stage": "proposal", "position": 0 })).await;
    assert_eq!(board.expect(EVENT).await.data, json!({ "deal": moved }));

    // Refused changes broadcast nothing.
    watcher
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "title": "Mine now" }))
        .send()
        .await
        .assert_error(403, None);
    rep.post(&format!("/api/v1/deals/{id}/move"))
        .json(&json!({ "stage": "lost" }))
        .send()
        .await
        .assert_validation_error("lost_reason");
    board.expect_silence(Duration::from_millis(300)).await;

    // A deleted deal is announced with its last state.
    rep.delete(&format!("/api/v1/deals/{id}"))
        .send()
        .await
        .assert_status(204);
    let deleted = board.expect(EVENT).await;
    assert_eq!(deleted.data, json!({ "deal": moved }));

    board.close().await;
    crm.shutdown().await;
}

/// Deal notifications come from a queued listener, so in production they
/// run in `queue:work`: a process without the WebSocket server. The worker
/// relays its broadcasts to the process serving the sockets (here: another
/// app on the same database, as `serve` and `queue:work` share one).
#[tokio::test]
async fn notifications_from_a_worker_process_reach_the_socket() {
    let key = navree::crypto::generate_key();
    let server = TestCrm::builder()
        .module(DealsModule)
        .config("app.key", key.clone())
        .build()
        .await;
    let addr = serve(&server).await;
    let url = server
        .get::<Config>()
        .await
        .get_string("database.url")
        .expect("the database url");
    let worker = TestCrm::builder()
        .module(DealsModule)
        .config("app.key", key)
        .config("database.url", url)
        .config("realtime.relay_url", format!("http://{addr}"))
        .build()
        .await;

    let rep = server.login_as(Role::Rep1).await;
    let manager = server.login_as(Role::Manager).await;
    let agra = company(&server, "Agra", manager.id()).await;
    let channel = format!("private-users.{}", rep.id());
    let mut socket = Socket::connect(addr).await;
    socket.join(&rep, &channel).await;

    let mut input = new_deal("Agra renewal", agra, 100);
    input["owner_id"] = json!(rep.id());
    let deal = create_deal(&manager, input).await;
    // The serving process does not run the queue.
    socket.expect_silence(Duration::from_millis(300)).await;

    let stats = worker.run_queue().await;
    assert!(stats.processed >= 1, "{stats:?}");
    let frame = socket.expect("notification.created").await;
    assert_eq!(frame.channel.as_deref(), Some(channel.as_str()));
    assert_eq!(frame.data["data"]["deal_id"], deal["id"]);

    socket.close().await;
    worker.shutdown().await;
    server.shutdown().await;
}

/// When a user's roles change or they
/// sign out, their sockets close (code 4200: reconnect and authorize
/// again), so subscriptions authorized under the old access end.
#[tokio::test]
async fn role_changes_and_logout_end_the_users_subscriptions() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let admin = crm.login_as(Role::Admin).await;
    let agra = company(&crm, "Agra", manager.id()).await;
    let addr = serve(&crm).await;
    let mut rep_board = Socket::connect(addr).await;
    rep_board.join(&rep, CHANNEL).await;
    let mut manager_board = Socket::connect(addr).await;
    manager_board.join(&manager, CHANNEL).await;

    // The rep loses every role.
    admin
        .put(&format!("/api/access/users/{}/roles", rep.id()))
        .json(&json!({ "roles": [] }))
        .send()
        .await
        .assert_status(200);
    let closed = rep_board.expect("pusher:error").await;
    assert_eq!(closed.data["code"], 4200, "{closed:?}");
    // Re-authorizing fails now, and changes no longer reach the rep.
    rep.post("/broadcasting/auth")
        .json(&json!({ "socket_id": "123.456", "channel_name": CHANNEL }))
        .send()
        .await
        .assert_status(403);
    create_deal(&manager, new_deal("Agra renewal", agra, 100)).await;
    manager_board.expect(EVENT).await;

    // Signing out ends the manager's subscriptions too.
    manager
        .post("/api/auth/logout")
        .json(&json!({ "refresh_token": manager.refresh_token() }))
        .send()
        .await
        .assert_status(204);
    let closed = manager_board.expect("pusher:error").await;
    assert_eq!(closed.data["code"], 4200, "{closed:?}");
    crm.shutdown().await;
}
