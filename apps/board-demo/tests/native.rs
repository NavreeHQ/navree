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
    // The board's parts say what they are: all it lacks is a checklist box that submits
    // its form when it changes, so one page, the card, is still the web view's.
    assert_eq!(coverage.native(), coverage.pages() - 1, "{coverage}");
    assert_eq!(
        coverage.graded(navree::ui::native::Grade::Idiom),
        coverage.native(),
        "{coverage}"
    );
}
