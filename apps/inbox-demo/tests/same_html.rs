//! Every page of the inbox as a browser gets it is byte for byte the same with
//! the native-preview feature on as with it off.
//!
//! One build can't be both, so the test runs once in each, against one
//! directory: the build without the feature writes each page, and the build
//! with it reads each back and compares. The app's clock is held at one
//! moment in both, so the two write the same data and draw the same pages:
//!
//! ```text
//! NAVREE_HTML_DIR=/tmp/inbox-demo-html cargo test -p inbox-demo --test same_html -- --ignored
//! NAVREE_HTML_DIR=/tmp/inbox-demo-html cargo test -p inbox-demo --features native-preview --test same_html -- --ignored
//! ```

use std::path::PathBuf;

use inbox_demo::{Settings, app};
use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};

/// The inbox on a database of its own, written afresh, with nothing happening by itself.
async fn boot() -> (TestApp, TempDirGuard) {
    // Both runs write their data and draw their pages at one moment.
    inbox_demo::live::hold_clock(1_794_457_800);
    let (url, dir) = temp_sqlite_url();
    let settings = Settings {
        simulate_every: None,
        tick_every: None,
        test_hooks: false,
        // A desk of a few thousand: the pages are the same at any size, and a seed of
        // seconds keeps the two runs near enough for their clocks to agree.
        scale: 2_000,
        ..Settings::default()
    };
    (TestApp::from_builder(app(0, url, settings)).await, dir)
}

/// Every page and fragment of the inbox a browser asks for.
const PAGES: &[&str] = &[
    "/",
    "/inbox",
    "/inbox?view=mine",
    "/inbox/1",
    "/inbox/2",
    "/inbox/1/fragments/head",
    "/inbox/1/fragments/customer",
    "/inbox/fragments/summary",
    "/inbox/fragments/views",
    "/inbox/rows",
    "/inbox/1/messages",
    "/inbox/agents/options",
    "/inbox/999999",
];

/// `html` less what differs from one request to the next whatever the build:
/// the session's token in a form.
fn steady(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let marker = r#"name="_csrf" value=""#;
    while let Some(at) = rest.find(marker) {
        let (before, after) = rest.split_at(at + marker.len());
        out.push_str(before);
        out.push_str("token");
        rest = after.find('"').map_or("", |end| &after[end..]);
    }
    out.push_str(rest);
    out
}

/// Where the page at `path` is kept.
fn file(dir: &str, path: &str) -> PathBuf {
    let name: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    PathBuf::from(dir).join(format!("{name}.html"))
}

#[tokio::test]
#[ignore = "runs in two builds against one directory: see the file's first lines"]
async fn every_page_is_the_html_it_was() {
    let dir = std::env::var("NAVREE_HTML_DIR").expect("NAVREE_HTML_DIR names the directory");
    std::fs::create_dir_all(&dir).expect("the directory");
    let (app, _db) = boot().await;
    let mut differ = Vec::new();
    let mut same = 0;
    for path in PAGES {
        let response = app.client().get(path).send().await;
        let status = response.status().as_u16();
        let html = format!("{status}\n{}", steady(&response.text()));
        let file = file(&dir, path);
        if cfg!(feature = "native-preview") {
            let was = std::fs::read_to_string(&file).unwrap_or_else(|error| {
                panic!("{path}: run the build without the feature first: {error}")
            });
            if was == html {
                same += 1;
            } else {
                let at = was
                    .bytes()
                    .zip(html.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or(was.len().min(html.len()));
                let from = at.saturating_sub(80);
                differ.push(format!(
                    "{path} at byte {at}:\n  was {:?}\n  now {:?}",
                    was.get(from..(at + 80).min(was.len())).unwrap_or(""),
                    html.get(from..(at + 80).min(html.len())).unwrap_or("")
                ));
            }
        } else {
            std::fs::write(&file, html).expect("the page written");
        }
    }
    assert!(differ.is_empty(), "{}", differ.join("\n"));
    if cfg!(feature = "native-preview") {
        println!("{same} of {} pages byte for byte", PAGES.len());
    } else {
        println!("{} pages written", PAGES.len());
    }
}
