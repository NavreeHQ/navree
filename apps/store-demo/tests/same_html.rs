//! Every page of the store as a browser gets it is byte for byte the same with
//! the native-preview feature on as with it off.
//!
//! One build can't be both, so the test runs once in each, against one
//! directory: the build without the feature writes each page, and the build
//! with it reads each back and compares. Build both first and run them one
//! right after the other, so the two apps write their data and draw their
//! pages at nearly the same time:
//!
//! ```text
//! NAVREE_HTML_DIR=/tmp/store-demo-html cargo test -p store-demo --test same_html -- --ignored
//! NAVREE_HTML_DIR=/tmp/store-demo-html cargo test -p store-demo --features native-preview --test same_html -- --ignored
//! ```

use std::path::PathBuf;

use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use store_demo::{Settings, app};

/// The store on a database of its own, written afresh, with nothing happening by itself.
async fn boot() -> (TestApp, TempDirGuard) {
    let (url, dir) = temp_sqlite_url();
    let settings = Settings {
        simulate_every: None,
        test_hooks: false,
    };
    (TestApp::from_builder(app(0, url, settings)).await, dir)
}

/// Every page and fragment of the store a browser asks for.
const PAGES: &[&str] = &[
    "/",
    "/orders",
    "/orders?filter[status][in]=open&sort=total",
    "/orders?page=2",
    "/orders/1",
    "/orders/999999",
    "/orders/1/steps",
    "/orders/1/others",
    "/orders/1/peek",
    "/inventory",
    "/inventory?page=2",
    "/fragments/figures",
    "/fragments/sales",
    "/fragments/shelves",
    "/fragments/activity",
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

/// `html` with every number written as `#`: what is left when the two runs'
/// clocks are set aside. The app writes its data and draws its times from
/// the clock, so two runs a few seconds apart differ in the numbers that
/// come from it (a time, a chart's points) and in nothing else.
fn without_numbers(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_number = false;
    for c in html.chars() {
        let digit = c.is_ascii_digit() || (in_number && c == '.');
        if digit && !in_number {
            out.push('#');
        } else if !digit {
            out.push(c);
        }
        in_number = digit;
    }
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
    let mut clocked = Vec::new();
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
            } else if without_numbers(&was) == without_numbers(&html) {
                // The same markup; only numbers drawn from the clock moved.
                clocked.push(*path);
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
        println!(
            "{same} of {} pages byte for byte; {} the same but for numbers from the clock: {clocked:?}",
            PAGES.len(),
            clocked.len()
        );
    } else {
        println!("{} pages written", PAGES.len());
    }
}
