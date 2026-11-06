//! Live streams to pages (`GET /broadcasting/stream`): topics
//! scoped to the reader's user and workspace, channels under the same
//! authorizers as a WebSocket, resuming with `Last-Event-ID`, backpressure
//! and caps, and the path from a queue worker or another process to the page.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::StatusCode;
use navree_contracts::{Broadcaster, Principal};
use navree_db::{Db, DbOptions};
use navree_http::{HttpModule, HttpOptions};
use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder, Navree};
use navree_realtime::{
    Backplane, ChannelDecision, Live, MemoryBackplane, ModuleBuilderRealtimeExt, RealtimeModule,
    RealtimeOptions, StreamOptions, Topic,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::live_support::Workspaces;
use crate::support::{ORIGIN, Server, app, options};

/// How long "nothing more comes" is waited for.
const QUIET: Duration = Duration::from_millis(300);

async fn live(server: &Server) -> Arc<Live> {
    server
        .app
        .ctx()
        .get::<Live>()
        .await
        .expect("RealtimeModule provides Live")
}

fn with_streams(streams: StreamOptions) -> RealtimeOptions {
    RealtimeOptions {
        streams,
        ..options()
    }
}

// ---- What a stream says --------------------------------------------------

#[tokio::test]
async fn a_public_topic_reaches_a_guest_with_its_name_and_never_the_data() {
    let server = Server::start().await;
    let (mut guest, first) = server.listen("topics=orders", None, &[]).await;
    assert_eq!(
        first.event, "ui:live",
        "a stream that resumes nothing starts fresh"
    );
    assert!(first.id.is_some(), "every event carries an id: {first:?}");
    assert_eq!(server.realtime.streams(), 1);
    assert_eq!(
        server
            .realtime
            .listeners(&Topic::public("orders").channel()),
        1
    );

    live(&server)
        .await
        .nudge([Topic::public("orders")])
        .await
        .unwrap();
    let nudge = guest.expect("orders").await;
    assert_eq!(nudge.data, "changed");
    assert!(nudge.id.is_some());

    // A broadcast with data on the topic's channel: the stream says only its
    // event name. A page never reads it anyway; the reader may not be meant to.
    server
        .realtime
        .broadcast(
            [Topic::public("orders").channel()],
            "order.placed",
            json!({ "customer": "Gurpreet Kaur", "total": 125_000 }),
        )
        .unwrap();
    let nudge = guest.expect("orders").await;
    assert_eq!(nudge.data, "order.placed");
    assert!(!nudge.data.contains("Gurpreet"));
    guest.expect_silence(QUIET).await;
}

#[tokio::test]
async fn the_stream_is_an_unbuffered_event_stream() {
    let server = Server::start().await;
    let (stream, _) = server.listen("topics=orders", None, &[]).await;
    let header = |name: &str| {
        stream
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(header("content-type"), "text/event-stream");
    assert_eq!(header("cache-control"), "no-cache");
    assert_eq!(
        header("x-accel-buffering"),
        "no",
        "proxies must not hold nudges back"
    );
}

// ---- Scoping: the user and their workspace ----------------------------------

#[tokio::test]
async fn a_user_topic_reaches_only_that_user() {
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let (mut ada_page, _) = server.listen("topics=inbox", Some(ada), &[]).await;
    let (mut bob_page, _) = server.listen("topics=inbox", Some(bob), &[]).await;
    let (mut guest, _) = server.listen("topics=inbox", None, &[]).await;
    let live = live(&server).await;

    live.nudge([Topic::user(ada, "inbox")]).await.unwrap();
    ada_page.expect("inbox").await;
    bob_page.expect_silence(QUIET).await;
    guest.expect_silence(QUIET).await;

    // The same name, public: everyone listening to it.
    live.nudge([Topic::public("inbox")]).await.unwrap();
    for page in [&mut ada_page, &mut bob_page, &mut guest] {
        page.expect("inbox").await;
    }
}

#[tokio::test]
async fn a_workspace_topic_reaches_only_the_members_of_that_workspace() {
    let (acme, globex) = (Uuid::now_v7(), Uuid::now_v7());
    let (ada, bob, cy) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let workspaces = Workspaces::new([(acme, vec![ada]), (globex, vec![bob, cy])]);
    let server = Server::serve(app(options()).module(workspaces.module())).await;
    let (in_acme, in_globex) = (acme.to_string(), globex.to_string());

    let (mut ada_in_acme, _) = server
        .listen("topics=deals", Some(ada), &[("x-workspace", &in_acme)])
        .await;
    let (mut bob_in_globex, _) = server
        .listen("topics=deals", Some(bob), &[("x-workspace", &in_globex)])
        .await;
    // Cy names a workspace she is not a member of: her stream is in none.
    let (mut cy_in_acme, _) = server
        .listen("topics=deals", Some(cy), &[("x-workspace", &in_acme)])
        .await;
    // A guest naming a workspace is in none either.
    let (mut guest_in_acme, _) = server
        .listen("topics=deals", None, &[("x-workspace", &in_acme)])
        .await;
    assert_eq!(
        server
            .realtime
            .listeners(&Topic::workspace(acme, "deals").channel()),
        1,
        "only Ada listens in Acme"
    );

    let live = live(&server).await;
    live.nudge([Topic::workspace(acme, "deals")]).await.unwrap();
    ada_in_acme.expect("deals").await;
    for page in [&mut bob_in_globex, &mut cy_in_acme, &mut guest_in_acme] {
        page.expect_silence(QUIET).await;
    }

    live.nudge([Topic::workspace(globex, "deals")])
        .await
        .unwrap();
    bob_in_globex.expect("deals").await;
    for page in [&mut ada_in_acme, &mut cy_in_acme, &mut guest_in_acme] {
        page.expect_silence(QUIET).await;
    }
}

#[tokio::test]
async fn a_page_cannot_name_the_channel_of_another_scope() {
    // Topics are reached through `topics`, which scopes them.
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    for (query, user) in [
        (
            format!("channels={}", Topic::user(bob, "inbox").channel()),
            Some(ada),
        ),
        (
            format!("channels={}", Topic::user(ada, "inbox").channel()),
            Some(ada),
        ),
        (
            format!(
                "channels={}",
                Topic::workspace(Uuid::now_v7(), "deals").channel()
            ),
            Some(ada),
        ),
        ("channels=navree-live.orders".to_owned(), None),
    ] {
        let refused = server.stream(&query, user, &[]).await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "{query}");
    }
    assert_eq!(server.realtime.streams(), 0);

    // Nor can a WebSocket client get a signature for one.
    let socket_id = "1234.5678";
    let reply = server
        .auth(Some(ada), socket_id, &Topic::user(ada, "inbox").channel())
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.json);
}

// ---- Channels: the same rules as the API --------------------------------------

#[tokio::test]
async fn private_channels_follow_the_channel_authorizers() {
    // `users.{id}` lets only the user in, `broken` fails.
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let own = format!("private-users.{ada}");
    let query = format!("channels={own}");

    let (mut ada_page, first) = server.listen(&query, Some(ada), &[]).await;
    assert_eq!(first.event, "ui:live");
    assert_eq!(
        server.stream(&query, Some(bob), &[]).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        server.stream(&query, None, &[]).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        server
            .stream("channels=private-broken", Some(ada), &[])
            .await
            .status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        server
            .stream("channels=private-nobody.listens", Some(ada), &[])
            .await
            .status,
        StatusCode::FORBIDDEN,
        "no authorizer matches: 403, as for a WebSocket"
    );

    // One broadcast reaches the WebSocket subscriber and the page alike.
    let mut socket = server.connect().await;
    server.join(&mut socket, ada, &own).await;
    server
        .realtime
        .broadcast(
            [own.as_str()],
            "notification.created",
            json!({ "title": "Deal assigned" }),
        )
        .unwrap();
    assert_eq!(
        socket.expect("notification.created").await.data,
        json!({ "title": "Deal assigned" })
    );
    let nudge = ada_page.expect(&own).await;
    assert_eq!(nudge.data, "notification.created");
}

#[tokio::test]
async fn a_presence_channel_is_heard_without_joining_it() {
    let server = Server::start().await;
    let ada = Uuid::now_v7();
    let (mut page, _) = server
        .listen("channels=presence-room.7", Some(ada), &[])
        .await;
    assert!(
        server.realtime.members("presence-room.7").is_empty(),
        "a stream is no member"
    );
    server
        .realtime
        .broadcast(["presence-room.7"], "room.renamed", json!({}))
        .unwrap();
    assert_eq!(page.expect("presence-room.7").await.data, "room.renamed");
}

#[tokio::test]
async fn public_channels_and_topics_share_one_stream() {
    let server = Server::start().await;
    let (mut page, _) = server
        .listen("topics=orders&channels=news", None, &[])
        .await;
    server
        .realtime
        .broadcast(["news"], "posted", json!({}))
        .unwrap();
    assert_eq!(page.expect("news").await.data, "posted");
    live(&server)
        .await
        .nudge([Topic::public("orders")])
        .await
        .unwrap();
    page.expect("orders").await;
}

// ---- What the runtime is told when a stream is refused -------------------------

#[tokio::test]
async fn the_probe_answers_what_the_stream_would_and_opens_nothing() {
    // The runtime asks with `x-ui-probe` what a stream that failed answers.
    let server = Server::start().await;
    let probe = [("x-ui-probe", "1")];
    let refused = server
        .stream("channels=private-crm.deals", None, &probe)
        .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    let mut fine = server.stream("topics=orders", None, &probe).await;
    assert_eq!(fine.status, StatusCode::OK);
    assert_eq!(
        fine.headers
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    fine.expect_end().await;
    assert_eq!(server.realtime.streams(), 0);
}

#[tokio::test]
async fn a_stream_that_names_nothing_is_a_204() {
    // The runtime does not try a 204 again.
    let server = Server::start().await;
    for query in ["", "topics=", "topics=,%20,&channels="] {
        assert_eq!(
            server.stream(query, None, &[]).await.status,
            StatusCode::NO_CONTENT,
            "{query}"
        );
    }
}

#[tokio::test]
async fn names_are_checked_and_capped() {
    let server = Server::start_with(with_streams(StreamOptions {
        names: 3,
        ..StreamOptions::default()
    }))
    .await;
    let too_many = server.stream("topics=a,b&channels=c,d", None, &[]).await;
    assert_eq!(too_many.status, StatusCode::BAD_REQUEST);

    let bad = server
        .stream(
            "topics=has%20space,ui:live&channels=private-a%3Db",
            None,
            &[],
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let body = bad.json().await;
    assert!(
        body["errors"]["topics"]
            .as_array()
            .is_some_and(|e| e.len() == 2),
        "{body}"
    );
    assert!(body["errors"]["channels"].is_array(), "{body}");

    let long = "x".repeat(65);
    assert_eq!(
        server
            .stream(&format!("topics={long}"), None, &[])
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(server.realtime.streams(), 0);
}

#[tokio::test]
async fn a_page_from_a_disallowed_origin_is_refused() {
    // The same origins as the WebSocket.
    let server = Server::start().await;
    let refused = server
        .stream("topics=orders", None, &[("origin", "https://evil.example")])
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let allowed = server
        .stream("topics=orders", None, &[("origin", ORIGIN)])
        .await;
    assert_eq!(allowed.status, StatusCode::OK);
}

#[tokio::test]
async fn a_user_holds_at_most_their_share_of_streams() {
    // One stream per open tab, capped per user.
    let server = Server::start_with(with_streams(StreamOptions {
        per_user: 2,
        ..StreamOptions::default()
    }))
    .await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let (first, _) = server.listen("topics=orders", Some(ada), &[]).await;
    let (_second, _) = server.listen("topics=orders", Some(ada), &[]).await;
    let refused = server.stream("topics=orders", Some(ada), &[]).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    let (_bob, _) = server.listen("topics=orders", Some(bob), &[]).await;

    // A tab that closes gives its place back.
    drop(first);
    for _ in 0..200 {
        if server.realtime.streams() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(server.realtime.streams(), 2);
    let (_again, _) = server.listen("topics=orders", Some(ada), &[]).await;
}

// ---- Resuming ------------------------------------------------------------------

#[tokio::test]
async fn a_stream_that_comes_back_is_told_only_what_it_missed() {
    let server = Server::start().await;
    let live = live(&server).await;
    let (mut page, _) = server.listen("topics=orders,stats", None, &[]).await;
    live.nudge([Topic::public("orders")]).await.unwrap();
    let last = page.expect("orders").await.id.expect("an id");
    drop(page);

    live.nudge([Topic::public("stats")]).await.unwrap();
    live.nudge([Topic::public("orders")]).await.unwrap();
    live.nudge([Topic::public("orders")]).await.unwrap();
    live.nudge([Topic::public("somewhere-else")]).await.unwrap();

    let (mut back, first) = server
        .listen("topics=orders,stats", None, &[("last-event-id", &last)])
        .await;
    assert_eq!(first.event, "ui:resumed");
    assert_eq!(
        first.id.as_deref(),
        Some(last.as_str()),
        "until it reads what it missed, a page comes back from the same place"
    );
    back.expect("stats").await;
    back.expect("orders").await;
    back.expect_silence(QUIET).await;

    // The query parameter is what a page's runtime sends (an EventSource
    // can't set headers).
    let (mut again, first) = server
        .listen(&format!("topics=stats&last_event_id={last}"), None, &[])
        .await;
    assert_eq!(first.event, "ui:resumed");
    again.expect("stats").await;
    again.expect_silence(QUIET).await;
}

#[tokio::test]
async fn a_resumed_stream_is_told_only_what_its_reader_may_hear() {
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let (mut page, _) = server.listen("topics=inbox", Some(ada), &[]).await;
    server
        .realtime
        .broadcast([Topic::user(ada, "inbox").channel()], "seen", json!({}))
        .unwrap();
    let last = page.expect("inbox").await.id.unwrap();
    drop(page);
    server
        .realtime
        .broadcast([Topic::user(bob, "inbox").channel()], "for-bob", json!({}))
        .unwrap();
    server
        .realtime
        .broadcast([Topic::user(ada, "inbox").channel()], "for-ada", json!({}))
        .unwrap();
    let (mut back, first) = server
        .listen("topics=inbox", Some(ada), &[("last-event-id", &last)])
        .await;
    assert_eq!(first.event, "ui:resumed");
    assert_eq!(back.expect("inbox").await.data, "for-ada");
    back.expect_silence(QUIET).await;
}

#[tokio::test]
async fn a_stream_that_cannot_be_resumed_starts_fresh() {
    // An id it can't read, or one older than what is kept.
    let server = Server::start_with(with_streams(StreamOptions {
        replay: 2,
        ..StreamOptions::default()
    }))
    .await;
    let publisher = live(&server).await;
    let (_, first) = server
        .listen("topics=orders", None, &[("last-event-id", "not-an-id")])
        .await;
    assert_eq!(first.event, "ui:live");

    let (mut page, _) = server.listen("topics=orders", None, &[]).await;
    publisher.nudge([Topic::public("orders")]).await.unwrap();
    let last = page.expect("orders").await.id.unwrap();
    drop(page);
    for _ in 0..3 {
        publisher.nudge([Topic::public("orders")]).await.unwrap();
    }
    let (_, first) = server
        .listen("topics=orders", None, &[("last-event-id", &last)])
        .await;
    assert_eq!(
        first.event, "ui:live",
        "what it missed is no longer all kept"
    );

    // Resuming turned off: always fresh.
    let off = Server::start_with(with_streams(StreamOptions {
        replay: 0,
        ..StreamOptions::default()
    }))
    .await;
    let (mut page, _) = off.listen("topics=orders", None, &[]).await;
    live(&off)
        .await
        .nudge([Topic::public("orders")])
        .await
        .unwrap();
    let last = page.expect("orders").await.id.unwrap();
    let (_, first) = off
        .listen("topics=orders", None, &[("last-event-id", &last)])
        .await;
    assert_eq!(first.event, "ui:live");
}

// ---- Backpressure, lifetime and revocation ----------------------------------------

#[tokio::test]
async fn a_page_that_does_not_read_never_holds_a_broadcaster_up() {
    // A stream keeps one unread nudge per name, however many come.
    let server = Server::start().await;
    let mut page = server.stream("topics=orders,stats", None, &[]).await;
    assert_eq!(page.status, StatusCode::OK);
    let channel = Topic::public("orders").channel();
    for n in 0..5_000 {
        server
            .realtime
            .broadcast([channel.as_str()], "changed", json!({ "n": n }))
            .unwrap();
    }
    live(&server)
        .await
        .nudge([Topic::public("stats")])
        .await
        .unwrap();

    assert_eq!(page.expect("ui:live").await.data, "live");
    page.expect("orders").await;
    page.expect("stats").await;
    page.expect_silence(QUIET).await;
}

#[tokio::test]
async fn a_stream_ends_after_its_lifetime_and_resumes() {
    // A reader is authorized again now and then.
    let server = Server::start_with(with_streams(StreamOptions {
        lifetime_secs: 1,
        ..StreamOptions::default()
    }))
    .await;
    let (mut page, first) = server.listen("topics=orders", None, &[]).await;
    let last = first.id.unwrap();
    page.expect_end().await;
    let (_, first) = server
        .listen("topics=orders", None, &[("last-event-id", &last)])
        .await;
    assert_eq!(first.event, "ui:resumed");
}

#[tokio::test]
async fn ending_a_users_subscriptions_ends_their_streams() {
    // After a role change or a sign-out the page comes back and is
    // authorized again.
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let (mut ada_page, _) = server.listen("topics=inbox", Some(ada), &[]).await;
    let (mut bob_page, _) = server.listen("topics=inbox", Some(bob), &[]).await;
    let broadcaster: Arc<dyn Broadcaster> = server.realtime.clone();
    broadcaster.end_user_subscriptions(ada).await.unwrap();
    ada_page.expect_end().await;
    live(&server)
        .await
        .nudge([Topic::user(bob, "inbox")])
        .await
        .unwrap();
    bob_page.expect("inbox").await;

    server.realtime.disconnect_user(bob);
    bob_page.expect_end().await;
}

#[tokio::test]
async fn streams_end_when_the_app_shuts_down() {
    let server = Server::start().await;
    let (mut page, _) = server.listen("topics=orders", None, &[]).await;
    let Server { app, .. } = server;
    let shutdown = tokio::spawn(async move { app.shutdown().await });
    page.expect_end().await;
    shutdown.await.unwrap().unwrap();
}

// ---- From workers and other processes to the page ------------------------------

#[tokio::test]
async fn a_nudge_from_a_queue_worker_reaches_the_page() {
    // The worker relays to the serving process, which tells its streams.
    let server = Server::start().await;
    let ada = Uuid::now_v7();
    let (mut page, _) = server
        .listen("topics=orders,notifications", Some(ada), &[])
        .await;
    let worker = app(RealtimeOptions {
        relay_url: format!("http://{}", server.addr),
        ..options()
    })
    .boot()
    .await
    .expect("the worker app boots");
    let worker_live = worker
        .ctx()
        .get::<Live>()
        .await
        .expect("Live in the worker");
    worker_live
        .nudge([Topic::public("orders"), Topic::user(ada, "notifications")])
        .await
        .expect("the relay accepts it");
    let mut heard = vec![
        page.next().await.unwrap().event,
        page.next().await.unwrap().event,
    ];
    heard.sort();
    assert_eq!(heard, ["notifications", "orders"]);
    page.expect_silence(QUIET).await;
}

/// Serves the test app on `backplane` (another `serve` process).
async fn node(backplane: &MemoryBackplane) -> Server {
    Server::serve(app(options()).override_value::<dyn Backplane>(Arc::new(backplane.clone()))).await
}

#[tokio::test]
async fn a_page_hears_a_nudge_published_in_another_process_and_resumes_on_any() {
    // Several `serve` processes behind a load balancer.
    let backplane = MemoryBackplane::new();
    let (a, b) = (node(&backplane).await, node(&backplane).await);
    let (mut on_b, _) = b.listen("topics=orders", None, &[]).await;
    // A keeps broadcasts once it has served a stream.
    let (_on_a, _) = a.listen("topics=orders", None, &[]).await;

    let from_a = live(&a).await;
    from_a.nudge([Topic::public("orders")]).await.unwrap();
    let last = on_b.expect("orders").await.id.unwrap();
    drop(on_b);
    from_a.nudge([Topic::public("orders")]).await.unwrap();

    // The page comes back through the load balancer to A, with B's last id:
    // the id is the sender's, the same on every process.
    let (mut back, first) = a
        .listen("topics=orders", None, &[("last-event-id", &last)])
        .await;
    assert_eq!(first.event, "ui:resumed");
    back.expect("orders").await;
    back.expect_silence(QUIET).await;
}

// ---- Publishing ------------------------------------------------------------------

/// A broadcaster that records what it is asked to send.
#[derive(Default)]
struct Recording(Mutex<Vec<(Vec<String>, String)>>);

impl Recording {
    fn sent(&self) -> Vec<(Vec<String>, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Broadcaster for Recording {
    fn broadcast(
        &self,
        channels: Vec<String>,
        event: String,
        _data: Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((channels, event));
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn a_nudge_in_a_unit_of_work_waits_for_its_commit_and_never_follows_a_rollback() {
    // A page told before the commit would ask too soon and miss it.
    let recording = Arc::new(Recording::default());
    let live = Live::new(recording.clone());
    let db = Db::connect(&DbOptions {
        url: "sqlite::memory:".into(),
        ..DbOptions::default()
    })
    .await
    .expect("an in-memory database");
    let workspace = Uuid::now_v7();

    let uow = db.uow(None, None).await;
    uow.scope(async {
        live.nudge([Topic::workspace(workspace, "deals")])
            .await
            .unwrap()
    })
    .await;
    assert!(recording.sent().is_empty(), "nothing before the commit");
    uow.commit().await.expect("the commit");
    assert_eq!(
        recording.sent(),
        [(
            vec![Topic::workspace(workspace, "deals").channel()],
            "changed".to_owned()
        )]
    );

    let uow = db.uow(None, None).await;
    uow.scope(async { live.nudge([Topic::public("orders")]).await.unwrap() })
        .await;
    uow.rollback().await.expect("the rollback");
    assert_eq!(recording.sent().len(), 1, "nothing after a rollback");

    // Outside a unit of work nothing waits; each channel goes once.
    live.nudge([
        Topic::public("orders"),
        Topic::public("orders"),
        Topic::public("stats"),
    ])
    .await
    .unwrap();
    assert_eq!(
        recording.sent()[1].0,
        ["navree-live.orders", "navree-live.stats"]
    );

    // A name that is not a topic sends nothing.
    assert!(live.nudge([Topic::public("not a topic")]).await.is_err());
    assert!(
        live.nudge([Topic::public("ok"), Topic::public("ui:live")])
            .await
            .is_err()
    );
    assert_eq!(recording.sent().len(), 2);
}

// ---- Apps without sign-in, and reserved names ----------------------------------------

#[tokio::test]
async fn an_app_without_an_authenticator_serves_public_topics_and_no_auth_route() {
    // The demos sign nobody in.
    let server = Server::serve(
        Navree::app()
            .module(HttpModule::for_root(HttpOptions {
                per_ip_rate_limit: None,
                ..HttpOptions::default()
            }))
            .module(RealtimeModule::for_root(options())),
    )
    .await;
    let (mut page, _) = server.listen("topics=orders", None, &[]).await;
    live(&server)
        .await
        .nudge([Topic::public("orders")])
        .await
        .unwrap();
    page.expect("orders").await;
    assert_eq!(
        server
            .stream("channels=private-crm.deals", None, &[])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let reply = server.auth(None, "1234.5678", "private-crm.deals").await;
    assert_eq!(
        reply.status,
        StatusCode::NOT_FOUND,
        "nobody could be authorized"
    );
}

fn allow(
    _principal: &Principal,
    _params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    Box::pin(async { Ok(ChannelDecision::Allow) })
}

/// Registers a pattern under the live hub's prefix.
struct Squatter;

impl Module for Squatter {
    fn register(&self, m: &mut ModuleBuilder) {
        m.channel("navree-live.user.{id}.inbox", allow);
    }
}

#[tokio::test]
async fn a_channel_pattern_under_the_live_prefix_fails_the_boot() {
    // No app authorizer may open another user's topics.
    let error = app(options())
        .module(Squatter)
        .boot()
        .await
        .expect_err("boot must fail")
        .to_string();
    assert!(error.contains("reserved for live topics"), "{error}");
}
