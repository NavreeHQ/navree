//! The inbox as a native client gets it: which of its routes answer a screen
//! and how much each belongs to the client's system, which fall back to the
//! web view and why, and that a browser asking the same routes gets HTML.
//!
//! `cargo test -p inbox-demo --features native-preview --test native -- --nocapture`
//! prints the coverage table.
#![cfg(feature = "native-preview")]

use inbox_demo::{Settings, app};
use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use navree::ui::native::Grade;
use navree::ui::native::protocol::Envelope;
use navree::ui::native::protocol::node::{self as n, Node as N};
use navree::ui::native::{Answered, Coverage};

const PHONE: &str =
    "platform=android; os=16; form=phone; app=0.1.0; vocab=1; locale=en-IN; tz=Asia/Kolkata";

/// The inbox on a database of its own, written afresh, with nothing happening by itself.
async fn boot() -> (TestApp, TempDirGuard) {
    let (url, dir) = temp_sqlite_url();
    let settings = Settings {
        simulate_every: None,
        tick_every: None,
        test_hooks: false,
        ..Settings::default()
    };
    (TestApp::from_builder(app(0, url, settings)).await, dir)
}

#[tokio::test]
async fn the_inbox_answers_a_native_client() {
    let (app, _dir) = boot().await;
    let mut coverage = Coverage::new();
    for path in [
        "/inbox",
        "/inbox/1",
        "/inbox/1/fragments/head",
        "/inbox/1/fragments/customer",
        "/inbox/fragments/summary",
        "/inbox/fragments/views",
        "/inbox/rows",
        "/inbox/1/messages",
        "/inbox/agents/options",
        "/_navree/native/shell",
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
        if path == "/_navree/native/shell" {
            continue;
        }
        let envelope: Envelope =
            serde_json::from_str(&body).unwrap_or_else(|error| panic!("{path}: {error}"));
        let html = app.client().get(path).send().await.text();
        assert!(!html.contains("\"navree\":"), "{path}");
        coverage.add_answer(path, html.len(), body.len(), &Answered::of(&envelope));
    }
    println!("{coverage}");
    assert!(coverage.pages() >= 4, "{coverage}");
    // The inbox says what its parts are: every page of it is native and belongs to its system.
    assert_eq!(coverage.native(), coverage.pages(), "{coverage}");
    assert_eq!(
        coverage.graded(Grade::Idiom),
        coverage.pages(),
        "{coverage}"
    );
}

/// Every node under `node`, itself first.
fn all(node: &N) -> Vec<&N> {
    let mut out = vec![node];
    for child in node.children() {
        out.extend(all(child));
    }
    out
}

#[tokio::test]
async fn an_open_conversation_is_panes_with_the_one_a_narrow_screen_shows_and_its_messages() {
    let (app, _dir) = boot().await;
    let native = app
        .client()
        .get("/inbox/1")
        .header("x-navree-native", "1")
        .header("x-navree-client", PHONE)
        .send()
        .await
        .text();
    let envelope: Envelope = serde_json::from_str(&native).expect("an envelope");
    let screen = envelope.screen.expect("a screen");
    let nodes = all(&screen.body);
    // The three panes: the list, the conversation (the one to show first), the customer.
    let panes: Vec<(&str, n::PaneRole, bool)> = nodes
        .iter()
        .filter_map(|node| match node {
            N::Pane(pane) => Some((pane.id.as_deref().unwrap_or(""), pane.role, pane.current)),
            _ => None,
        })
        .collect();
    assert_eq!(
        panes,
        [
            ("conversations", n::PaneRole::List, false),
            ("thread", n::PaneRole::Detail, true),
            ("customer", n::PaneRole::Inspector, false),
        ]
    );
    // The conversation is messages, the team's own on the far side, a note set apart.
    let messages: Vec<&n::Message> = nodes
        .iter()
        .filter_map(|node| match node {
            N::Message(message) => Some(message),
            _ => None,
        })
        .collect();
    assert!(!messages.is_empty());
    assert!(messages.iter().all(|message| !message.author.is_empty()));
    // The customer wrote some of them.
    assert!(messages.iter().any(|message| !message.mine));
}

/// The inbox with the hand-written conversations only, which is all these tests read.
async fn boot_desk() -> (TestApp, TempDirGuard) {
    let (url, dir) = temp_sqlite_url();
    let settings = Settings {
        simulate_every: None,
        tick_every: None,
        test_hooks: false,
        scale: 0,
    };
    (TestApp::from_builder(app(0, url, settings)).await, dir)
}

/// The header that names the person a native answer is for.
const PERSON: &str = "x-navree-person";

/// The person a native client is told the answer for `path` is for, when it carries `cookie`.
async fn person_of(app: &TestApp, path: &str, cookie: Option<&str>) -> Option<String> {
    let client = app.client();
    if let Some(value) = cookie {
        client.set_cookie("inbox_agent", value);
    }
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
async fn the_shell_names_the_default_agent_every_time() {
    let (app, _dir) = boot_desk().await;
    let first = person_of(&app, "/_navree/native/shell", None).await;
    let second = person_of(&app, "/_navree/native/shell", None).await;
    assert_eq!(
        first,
        Some(inbox_demo::person::person_of(inbox_demo::data::DEFAULT_AGENT).to_string())
    );
    assert_eq!(first, second);
}

#[tokio::test]
async fn every_native_page_names_the_same_person_as_the_shell() {
    let (app, _dir) = boot_desk().await;
    let shell = person_of(&app, "/_navree/native/shell", None).await;
    assert!(shell.is_some());
    for path in ["/inbox", "/inbox/1", "/inbox/rows", "/inbox/1/messages"] {
        assert_eq!(person_of(&app, path, None).await, shell, "{path}");
    }
}

#[tokio::test]
async fn an_agent_the_visitor_switched_to_is_another_person() {
    let (app, _dir) = boot_desk().await;
    let signer = app.get::<navree::http::UrlSigner>().await;
    let cookie = inbox_demo::pages::agent_cookie(&signer, 2, chrono::Utc::now());
    let other = person_of(&app, "/_navree/native/shell", Some(&cookie)).await;
    assert_eq!(other, Some(inbox_demo::person::person_of(2).to_string()));
    assert_ne!(other, person_of(&app, "/_navree/native/shell", None).await);
    // A cookie the app did not sign names nobody: the default agent.
    let forged = person_of(&app, "/_navree/native/shell", Some("2.4102444800.00ff")).await;
    assert_eq!(
        forged,
        Some(inbox_demo::person::person_of(inbox_demo::data::DEFAULT_AGENT).to_string())
    );
}

#[tokio::test]
async fn a_browser_gets_the_page_with_no_person_named() {
    let (app, _dir) = boot_desk().await;
    let page = app.client().get("/inbox").send().await;
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
    let (app, _dir) = boot_desk().await;
    // Everyone working as one agent is one person: sixteen open tabs, the framework's
    // default for a person, are not all of the desk's visitors.
    let mut open = Vec::new();
    for count in 1..=20 {
        let stream = app
            .client()
            .get("/broadcasting/stream?topics=conversations")
            .header("accept", "text/event-stream")
            .send_stream()
            .await;
        assert_eq!(stream.status().as_u16(), 200, "stream {count}");
        open.push(stream);
    }
}

#[tokio::test]
async fn a_browser_asking_for_an_address_that_does_not_exist_gets_the_404_in_the_app_shell() {
    let (app, _dir) = boot_desk().await;
    let page = app
        .client()
        .get("/no-such-page")
        .header("accept", "text/html")
        .send()
        .await;
    assert_eq!(page.status().as_u16(), 404);
    let html = page.text();
    // The account the shell shows is there, as on every other page.
    assert!(html.contains("@support.example.com"), "{html:.600}");
}

/// Runs `ask`, and says the statements it ran (and not those of the app's clocks, which
/// run beside it).
async fn statements_of(
    ask: impl std::future::Future<Output = navree::testing::TestResponse>,
) -> (navree::testing::TestResponse, Vec<String>) {
    let (response, ran) = navree::db::QueryLog::capture(ask).await;
    (response, ran.into_iter().map(|query| query.sql).collect())
}

#[tokio::test]
async fn a_page_that_is_not_found_runs_no_query_on_the_conversations() {
    let (app, _dir) = boot_desk().await;
    let (page, ran) = statements_of(
        app.client()
            .get("/no-such-page")
            .header("accept", "text/html")
            .send(),
    )
    .await;
    assert_eq!(page.status().as_u16(), 404);
    assert!(page.text().contains("@support.example.com"));
    // The shell it is drawn in is read (the agent, the team): the log sees the app's statements.
    assert!(!ran.is_empty());
    assert!(
        ran.iter().all(|sql| !sql.contains("conversations")),
        "{ran:?}"
    );
    // The inbox's own page counts them, as it did.
    let (_, ran) = statements_of(
        app.client()
            .get("/inbox")
            .header("accept", "text/html")
            .send(),
    )
    .await;
    assert!(ran.iter().any(|sql| sql.contains("conversations")));
}

#[tokio::test]
async fn the_shell_a_native_client_draws_its_navigation_from_keeps_its_counts() {
    let (app, _dir) = boot_desk().await;
    let (shell, ran) = statements_of(
        app.client()
            .get("/_navree/native/shell")
            .header("x-navree-native", "1")
            .header("x-navree-client", PHONE)
            .send(),
    )
    .await;
    assert!(ran.iter().any(|sql| sql.contains("conversations")));
    assert!(shell.text().contains("\"badge\""), "{:.400}", shell.text());
}
