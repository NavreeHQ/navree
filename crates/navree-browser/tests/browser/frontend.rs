//! FrontEnd and WebSession, with a small Node.js front end
//! (`frontend.mjs`). Without `node` on PATH these tests print a notice and
//! pass.

use std::time::Duration;

use navree_browser::{FrontEnd, TestServer};

use crate::support;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/browser/frontend.mjs");

fn has_node(test: &str) -> bool {
    let ok = std::process::Command::new(if cfg!(windows) { "node.exe" } else { "node" })
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
            panic!("{test}: node is not on PATH (NAVREE_TEST_REQUIRE_SERVICES=1)");
        }
        eprintln!("skipped {test}: node is not on PATH");
    }
    ok
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn node_front_end(name: &str, port: u16) -> FrontEnd {
    FrontEnd::new(name)
        .command("node", [SCRIPT.to_owned(), port.to_string()])
        .url(format!("http://127.0.0.1:{port}"))
        .api_env("FAKE_API_URL")
        .timeout(Duration::from_secs(30))
}

/// GETs `url` with a raw HTTP/1.1 request; returns the response text.
async fn get(url: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
    let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text.find(start).unwrap() + start.len();
    let to = from + text[from..].find(end).unwrap();
    &text[from..to]
}

#[tokio::test]
async fn one_process_serves_each_test_s_app_in_turn() {
    if !has_node("one_process_serves_each_test_s_app_in_turn") {
        return;
    }
    let first_app = support::labeled_app("first").await;
    let second_app = support::labeled_app("second").await;
    let web = node_front_end("fake-web-turns", free_port()).env("FAKE_GREETING", "hello from env");

    let first = web.serve(first_app.ctx()).await.unwrap();
    let page = get(&first.url("/")).await;
    assert!(page.contains("Serving first"), "{page}");
    assert!(page.contains("hello from env"), "{page}");
    let pid = between(&page, "pid ", "<").to_owned();
    // The page's API calls reach this test's app through the proxy.
    let api = get(&first.url("/api/label")).await;
    assert!(api.contains(r#""label":"first""#), "{api}");
    assert!(first.server().url("/").starts_with("http://127.0.0.1:"));

    // A second serve waits for the first session to end, and reuses the
    // running process.
    let check_first = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let page = get(&first.url("/")).await;
        assert!(page.contains("Serving first"), "still the first app: {page}");
        drop(first);
    };
    let (_, second) = tokio::join!(check_first, web.serve(second_app.ctx()));
    let second = second.unwrap();
    let page = get(&second.url("/")).await;
    assert!(page.contains("Serving second"), "{page}");
    assert_eq!(between(&page, "pid ", "<"), pid, "the same process");
}

#[tokio::test]
async fn a_front_end_drives_a_browser_journey() {
    if !has_node("a_front_end_drives_a_browser_journey")
        || !support::has_chrome("a_front_end_drives_a_browser_journey")
    {
        return;
    }
    let app = support::labeled_app("journey").await;
    let web = node_front_end("fake-web-journey", free_port())
        .serve(app.ctx())
        .await
        .unwrap();
    let browser = navree_browser::Browser::launch().await.unwrap();
    let page = browser.visit(web.url("/")).await.unwrap();
    page.assert_see("Serving journey").await.unwrap();
    page.assert_title("Front end").await.unwrap();
}

#[tokio::test]
async fn a_front_end_that_never_answers_times_out() {
    if !has_node("a_front_end_that_never_answers_times_out") {
        return;
    }
    let app = support::app().await;
    let port = free_port();
    let error = FrontEnd::new("fake-web-silent")
        .command("node", ["-e", "setTimeout(() => {}, 60000)"])
        .url(format!("http://127.0.0.1:{port}"))
        .timeout(Duration::from_millis(800))
        .serve(app.ctx())
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("did not answer at http://127.0.0.1:"), "{message}");
    assert!(message.contains("within 800ms"), "{message}");
}

#[tokio::test]
async fn a_front_end_that_exits_is_reported_with_its_output() {
    if !has_node("a_front_end_that_exits_is_reported_with_its_output") {
        return;
    }
    let app = support::app().await;
    let error = FrontEnd::new("fake-web-crash")
        .command("node", ["-e", "console.error('missing build'); process.exit(3)"])
        .url(format!("http://127.0.0.1:{}", free_port()))
        .serve(app.ctx())
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("exited"), "{message}");
    assert!(message.contains('3'), "{message}");
    assert!(message.contains("missing build"), "{message}");
}

#[tokio::test]
async fn a_missing_program_is_reported() {
    let app = support::app().await;
    let error = FrontEnd::new("fake-web-missing")
        .command("navree-no-such-program", ["start"])
        .url(format!("http://127.0.0.1:{}", free_port()))
        .serve(app.ctx())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("was not found on PATH"), "{error}");
}

#[tokio::test]
async fn a_port_in_use_is_reported_before_starting() {
    let app = support::app().await;
    let busy = TestServer::start(app.ctx()).await.unwrap();
    let error = FrontEnd::new("fake-web-busy")
        .command("node", ["-e", "0"])
        .url(busy.url(""))
        .serve(app.ctx())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("already answers"), "{error}");
}
