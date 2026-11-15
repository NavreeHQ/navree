//! The board as a native client gets it: which of its routes answer a screen
//! and how much each belongs to the client's system, which fall back to the
//! web view and why, and that a browser asking the same routes gets HTML.
//!
//! `cargo test -p board-demo --features native-preview --test native -- --nocapture`
//! prints the coverage table.
#![cfg(feature = "native-preview")]

use board_demo::{Settings, app};
use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use navree::ui::native::protocol::Envelope;
use navree::ui::native::{Answered, Coverage};

const PHONE: &str =
    "platform=android; os=16; form=phone; app=0.1.0; vocab=1; locale=en-IN; tz=Asia/Kolkata";

/// The board on a database of its own, written afresh, with no teammate moving cards.
async fn boot() -> (TestApp, TempDirGuard) {
    let (url, dir) = temp_sqlite_url();
    let settings = Settings {
        simulate_every: None,
        test_hooks: false,
    };
    (TestApp::from_builder(app(0, url, settings)).await, dir)
}

#[tokio::test]
async fn the_board_answers_a_native_client() {
    let (app, _dir) = boot().await;
    let mut coverage = Coverage::new();
    for path in [
        "/",
        "/fragments/board",
        "/cards/1",
        "/cards/search?q=a",
        "/cards/1/peek",
        "/cards/1/comments",
        "/cards/1/history",
        "/columns/doing",
    ] {
        let native = app
            .client()
            .get(path)
            .header("x-navree-native", "1")
            .header("x-navree-client", PHONE)
            .send()
            .await;
        let status = native.status().as_u16();
        let location = native.header("location").map(str::to_owned);
        let kind = native.header("content-type").map(str::to_owned);
        let body = native.text();
        // A reply that can't be said natively answers its no-script fallback.
        if (300..400).contains(&status) {
            println!("{path}: {status} to {location:?}");
            continue;
        }
        // Bare HTML never reaches a native client.
        assert_eq!(
            kind.as_deref(),
            Some("application/vnd.navree.native+json"),
            "{path}: {status} {body:.200}"
        );
        let envelope: Envelope =
            serde_json::from_str(&body).unwrap_or_else(|error| panic!("{path}: {error}"));
        let html = app.client().get(path).send().await.text();
        assert!(!html.contains("\"navree\":"), "{path}");
        coverage.add_answer(path, html.len(), body.len(), &Answered::of(&envelope));
    }
    println!("{coverage}");
    assert!(coverage.pages() >= 4, "{coverage}");
    // The board's parts say what they are, and a checklist box that submits its form when
    // it changes is a field that says so: every page is native.
    assert_eq!(coverage.native(), coverage.pages(), "{coverage}");
    assert_eq!(
        coverage.graded(navree::ui::native::Grade::Idiom),
        coverage.native(),
        "{coverage}"
    );
}

/// The header that names the person a native answer is for.
const PERSON: &str = "x-navree-person";

/// The person a native client is told the answer for `path` is for.
async fn person_of(app: &TestApp, path: &str) -> Option<String> {
    app.client()
        .get(path)
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .send()
        .await
        .header(PERSON)
        .map(str::to_owned)
}

#[tokio::test]
async fn the_shell_names_the_first_member_every_time() {
    let (app, _dir) = boot().await;
    let first = person_of(&app, "/_navree/native/shell").await;
    let second = person_of(&app, "/_navree/native/shell").await;
    let member = board_demo::data::viewer_number(None);
    assert_eq!(
        first,
        Some(board_demo::person::person_of(member).to_string())
    );
    assert_eq!(first, second);
}

#[tokio::test]
async fn every_native_page_names_the_same_person_as_the_shell() {
    let (app, _dir) = boot().await;
    let shell = person_of(&app, "/_navree/native/shell").await;
    assert!(shell.is_some());
    for path in ["/", "/fragments/board", "/cards/1", "/columns/doing"] {
        assert_eq!(person_of(&app, path).await, shell, "{path}");
    }
}

#[tokio::test]
async fn a_browser_gets_the_page_with_no_person_named() {
    let (app, _dir) = boot().await;
    let page = app.client().get("/").send().await;
    assert_eq!(page.status().as_u16(), 200);
    assert!(
        page.header("content-type")
            .is_some_and(|kind| kind.starts_with("text/html")),
        "{:?}",
        page.header("content-type")
    );
    assert_eq!(page.header(PERSON), None);
    assert!(page.header("set-cookie").is_none());
}

#[tokio::test]
async fn the_visitors_may_hold_more_live_streams_than_one_person_is_given_by_default() {
    let (app, _dir) = boot().await;
    // Everyone viewing the board as one member is one person: sixteen open tabs, the
    // framework's default for a person, are not all of the board's visitors.
    let mut open = Vec::new();
    for count in 1..=20 {
        let stream = app
            .client()
            .get("/broadcasting/stream?topics=board")
            .header("accept", "text/event-stream")
            .send_stream()
            .await;
        assert_eq!(stream.status().as_u16(), 200, "stream {count}");
        open.push(stream);
    }
}

#[tokio::test]
async fn a_browser_asking_for_an_address_that_does_not_exist_gets_the_404_in_the_app_shell() {
    let (app, _dir) = boot().await;
    let page = app
        .client()
        .get("/no-such-page")
        .header("accept", "text/html")
        .send()
        .await;
    assert_eq!(page.status().as_u16(), 404);
    let html = page.text();
    // The account the shell shows is there, as on every other page.
    assert!(html.contains("team@example.com"), "{html:.600}");
}

/// The person a native client is told the answer for `path` is for, on the way `client` has
/// (its cookies stay).
async fn person_on(client: &navree::testing::TestClient, path: &str) -> Option<String> {
    client
        .get(path)
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .send()
        .await
        .header(PERSON)
        .map(str::to_owned)
}

#[tokio::test]
async fn a_visitor_who_switches_viewer_is_another_person_on_the_next_native_answer() {
    let (app, _dir) = boot().await;
    let client = app.client();
    let first = person_on(&client, "/").await;
    assert!(first.is_some());
    // The cookie the board's own switch sets.
    client.set_cookie("board_as", "3");
    let third = person_on(&client, "/").await;
    assert!(third.is_some());
    assert_ne!(third, first);
    // The shell and every page name the same person for the viewer.
    assert_eq!(person_on(&client, "/_navree/native/shell").await, third);
    assert_eq!(person_on(&client, "/cards/1").await, third);
    // Another viewer, another person; the same viewer, the same person.
    client.set_cookie("board_as", "1");
    let lead = person_on(&client, "/").await;
    assert_ne!(lead, third);
    assert_ne!(lead, first);
    client.set_cookie("board_as", "3");
    assert_eq!(person_on(&client, "/").await, third);
}

#[tokio::test]
async fn a_cookie_that_names_nobody_is_the_default_viewer() {
    let (app, _dir) = boot().await;
    let fresh = person_on(&app.client(), "/").await;
    assert!(fresh.is_some());
    for named in [
        "99",
        "0",
        "-1",
        "abc",
        "",
        "3.5",
        "9999999999999999999999999",
    ] {
        let client = app.client();
        client.set_cookie("board_as", named);
        assert_eq!(person_on(&client, "/").await, fresh, "{named:?}");
    }
    // The default viewer is the first member, the person a cookie for them names too.
    let client = app.client();
    client.set_cookie("board_as", "2");
    assert_eq!(person_on(&client, "/").await, fresh);
}

#[tokio::test]
async fn the_person_is_the_viewer_the_pages_show_the_board_as() {
    let (app, _dir) = boot().await;
    let db = app.get::<navree::db::Db>().await;
    let uow = db.uow(None, None).await;
    // Whatever a cookie says, the person is the one the pages' own rule makes the viewer.
    for named in [
        None,
        Some("1"),
        Some("2"),
        Some("8"),
        Some("9"),
        Some("0"),
        Some("-3"),
        Some("+4"),
        Some("x"),
    ] {
        let shown = board_demo::store::viewer(&uow, named.and_then(|text| text.parse().ok()))
            .await
            .expect("the team")
            .id;
        let client = app.client();
        if let Some(value) = named {
            client.set_cookie("board_as", value);
        }
        assert_eq!(
            person_on(&client, "/").await,
            Some(board_demo::person::person_of(shown).to_string()),
            "{named:?}"
        );
    }
}

#[tokio::test]
async fn a_page_that_is_not_found_runs_no_statement_at_all() {
    let (app, _dir) = boot().await;
    let (page, ran) = navree::db::QueryLog::capture(
        app.client()
            .get("/no-such-page")
            .header("accept", "text/html")
            .send(),
    )
    .await;
    assert_eq!(page.status().as_u16(), 404);
    assert!(
        ran.is_empty(),
        "{:?}",
        ran.iter().map(|q| &q.sql).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn the_live_streams_one_person_may_hold_are_the_number_the_app_is_built_with() {
    let (app, _dir) = boot().await;
    let options = app.get::<navree::realtime::RealtimeOptions>().await;
    assert_eq!(options.streams.per_user, 10_000);
}
