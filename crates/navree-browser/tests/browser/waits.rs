//! Explicit waits and per-call timeouts.

use std::time::{Duration, Instant};

use crate::browser_or_skip;

#[tokio::test]
async fn wait_for_and_wait_until_missing() {
    let (browser, site) = browser_or_skip!("wait_for_and_wait_until_missing");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.wait_for("Loaded later").await.unwrap();
    page.wait_until_missing("Loading…").await.unwrap();
    page.wait_until_missing("@spinner").await.unwrap();
    let error = page
        .with_timeout(Duration::from_millis(200))
        .wait_for("@never")
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("wait_for(\"@never\") failed after"),
        "{error}"
    );
}

#[tokio::test]
async fn wait_for_url_matches_a_glob() {
    let (browser, site) = browser_or_skip!("wait_for_url_matches_a_glob");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.press("Next page").await.unwrap();
    page.wait_for_url("/sec*").await.unwrap();
    page.wait_for_url("/second?stage=won*").await.unwrap();
    page.wait_for_url("http://127.0.0.1:*/second*")
        .await
        .unwrap();
    let error = page
        .with_timeout(Duration::from_millis(200))
        .wait_for_url("/deals/*")
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("the URL is http://127.0.0.1:"),
        "{error}"
    );
}

#[tokio::test]
async fn wait_for_request_sees_calls_made_by_earlier_steps() {
    let (browser, site) = browser_or_skip!("wait_for_request_sees_calls_made_by_earlier_steps");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.press("Move deal").await.unwrap();
    let request = page.wait_for_request("/api/deals/*/move").await.unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.status, Some(200));
    assert!(
        request.url.ends_with("/api/deals/42/move"),
        "{}",
        request.url
    );
    page.assert_see("Moved 42").await.unwrap();
    // Each request satisfies one wait.
    let error = page
        .with_timeout(Duration::from_millis(200))
        .wait_for_request("/api/deals/*/move")
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("made no request matching"),
        "{error}"
    );
}

#[tokio::test]
async fn with_timeout_sets_one_step_s_patience() {
    let (browser, site) = browser_or_skip!("with_timeout_sets_one_step_s_patience");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let started = Instant::now();
    page.with_timeout(Duration::from_millis(250))
        .assert_see("Never shown")
        .await
        .unwrap_err();
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(250), "{waited:?}");
    assert!(waited < Duration::from_secs(4), "{waited:?}");
    assert_eq!(
        page.timeout(),
        Duration::from_secs(5),
        "the page keeps its own"
    );
}
