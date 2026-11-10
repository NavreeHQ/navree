//! The notification bell of a Navree UI shell: the list, the count, mark read, and the nudge a
//! new notification sends the recipient's live pages.
#![cfg(feature = "ui")]

use std::sync::Arc;

use navree_contracts::Broadcaster;
use navree_notify::{Channel, INBOX_TOPIC, Notifier, NotifyOptions};
use navree_realtime::{Live, NUDGE_EVENT};
use navree_testing::principal;

use crate::support::{self, DealWon, RecordingBroadcaster, TestUser};

// The list has the unread count, what each notification says and a link to what it is about;
// someone else's list is not shown to anyone.
#[tokio::test]
async fn the_bell_lists_the_signed_in_users_notifications() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;
    let (alice, bob) = (TestUser::new(), TestUser::new());
    for title in ["Lakhotia renewal", "Hundal expansion"] {
        notifier
            .send(&alice, DealWon::via(&[Channel::Database], title))
            .await
            .unwrap();
    }
    notifier
        .send(&bob, DealWon::via(&[Channel::Database], "Bal's deal"))
        .await
        .unwrap();

    let html = app
        .acting_as(principal(alice.id))
        .get("/notifications/bell")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(html.contains(r#"data-unread="2""#), "{html}");
    assert!(html.contains("Lakhotia renewal"), "{html}");
    assert!(html.contains("Hundal expansion was won"), "{html}");
    assert!(html.contains(r#"href="/deals/1""#), "{html}");
    assert!(!html.contains("Bal"), "{html}");
    assert!(html.contains("Mark all read"), "{html}");

    app.client()
        .get("/notifications/bell")
        .send()
        .await
        .assert_status(401);
}

// Marking one read, or every one, leaves the page with a redirect when the runtime is not
// there, and the count goes down; another user's notification is not found.
#[tokio::test]
async fn marking_read_changes_the_count_and_stays_with_the_owner() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;
    let (alice, bob) = (TestUser::new(), TestUser::new());
    let first = notifier
        .send(&alice, DealWon::via(&[Channel::Database], "first"))
        .await
        .unwrap();
    notifier
        .send(&alice, DealWon::via(&[Channel::Database], "second"))
        .await
        .unwrap();
    let bobs = notifier
        .send(&bob, DealWon::via(&[Channel::Database], "bal's"))
        .await
        .unwrap();
    let as_alice = app.acting_as(principal(alice.id));
    let count = || async {
        as_alice
            .get("/api/notifications/unread-count")
            .send()
            .await
            .json_value()["count"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(count().await, 2);

    let response = as_alice
        .post(&format!("/notifications/{first}/mark-read"))
        .send()
        .await;
    assert!(response.status().is_redirection(), "{}", response.status());
    assert_eq!(count().await, 1);

    as_alice
        .post(&format!("/notifications/{bobs}/mark-read"))
        .send()
        .await
        .assert_error(404, None);

    let response = as_alice.post("/notifications/read-all").send().await;
    assert!(response.status().is_redirection(), "{}", response.status());
    assert_eq!(count().await, 0);
    // Nothing unread: the bell has no count and no button for all.
    let html = as_alice.get("/notifications/bell").send().await.text();
    assert!(html.contains(r#"data-unread="0""#), "{html}");
    assert!(!html.contains("Mark all read"), "{html}");
}

// With the live hub in the app, a stored notification nudges its recipient's pages on the
// topic the bell listens to, and nobody else's.
#[tokio::test]
async fn a_new_notification_nudges_the_recipients_live_pages() {
    let broadcaster = Arc::new(RecordingBroadcaster::default());
    let app = support::builder(NotifyOptions::default())
        .override_value::<dyn Broadcaster>(broadcaster.clone())
        .override_value::<Live>(Arc::new(Live::new(broadcaster.clone())))
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;
    let alice = TestUser::new();
    notifier
        .send(&alice, DealWon::via(&[Channel::Database], "first"))
        .await
        .unwrap();
    let nudges: Vec<Vec<String>> = broadcaster
        .sent()
        .into_iter()
        .filter(|(_, event, _)| event == NUDGE_EVENT)
        .map(|(channels, _, _)| channels)
        .collect();
    assert_eq!(nudges.len(), 1, "{nudges:?}");
    assert!(
        nudges[0][0].contains(&alice.id.to_string()) && nudges[0][0].ends_with(INBOX_TOPIC),
        "{nudges:?}"
    );
}

// `navree check` asks every route for its API documentation, the bell's too.
#[tokio::test]
async fn the_bell_routes_are_documented() {
    let app = support::app().await;
    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let mut bell = 0;
    for route in routes.iter().filter(|route| {
        route
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("notifications.bell"))
    }) {
        bell += 1;
        let label = format!("{} {}", route.method, route.full_path);
        let api = route.api.as_ref().unwrap_or_else(|| panic!("{label}"));
        assert!(api.summary.is_some(), "{label}");
        assert_eq!(api.tags, ["Notifications"], "{label}");
    }
    assert_eq!(bell, 3);
}
