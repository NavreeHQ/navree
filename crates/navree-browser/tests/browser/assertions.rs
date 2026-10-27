//! Assertions retry until they pass or the timeout ends.

use std::time::{Duration, Instant};

use crate::browser_or_skip;

#[tokio::test]
async fn assert_see_waits_for_visible_text_only() {
    let (browser, site) = browser_or_skip!("assert_see_waits_for_visible_text_only");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.assert_see("Loaded later").await.unwrap(); // appears after 600 ms
    page.press("Save").await.unwrap();
    page.assert_see("Saved!").await.unwrap(); // appears 300 ms after the click
    page.assert_see("Shouted words").await.unwrap(); // upper-cased by CSS
    page.assert_see("SHOUTED WORDS").await.unwrap(); // as shown
    page.assert_dont_see("Secret text").await.unwrap(); // hidden
    page.assert_dont_see("Invisible text").await.unwrap(); // visibility: hidden
    page.assert_dont_see("Something went wrong").await.unwrap();

    let error = page
        .with_timeout(Duration::from_millis(300))
        .assert_see("Secret text")
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.starts_with("assert_see(\"Secret text\") failed after"),
        "{message}"
    );
    assert!(message.contains("the text is not visible on http://127.0.0.1:"), "{message}");
}

#[tokio::test]
async fn assert_dont_see_waits_for_text_to_go() {
    let (browser, site) = browser_or_skip!("assert_dont_see_waits_for_text_to_go");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.assert_dont_see("Loading…").await.unwrap(); // removed after 500 ms
    let error = page
        .with_timeout(Duration::from_millis(200))
        .assert_dont_see("Playground")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("the text is visible"), "{error}");
}

#[tokio::test]
async fn path_query_and_title() {
    let (browser, site) = browser_or_skip!("path_query_and_title");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.assert_title("Playground · Fixture").await.unwrap();
    page.press("Next page").await.unwrap();
    page.assert_path_is("/second").await.unwrap();
    page.assert_query_has("stage", "won").await.unwrap();
    page.assert_query_has("page", "2").await.unwrap();
    page.assert_title("Second page").await.unwrap();

    let quick = page.with_timeout(Duration::from_millis(200));
    let error = quick.assert_path_is("/deals").await.unwrap_err();
    assert!(error.to_string().contains(r#"the path is "/second""#), "{error}");
    let error = quick.assert_query_has("stage", "lost").await.unwrap_err();
    assert!(error.to_string().contains("?stage=won&page=2"), "{error}");
    let error = quick.assert_title("Deals").await.unwrap_err();
    assert!(error.to_string().contains(r#"the title is "Second page""#), "{error}");
}

#[tokio::test]
async fn values_visibility_counts_and_enabled_state() {
    let (browser, site) = browser_or_skip!("values_visibility_counts_and_enabled_state");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.assert_value("Memo", "Call").await.unwrap();
    page.assert_missing("@toast").await.unwrap(); // hidden
    page.assert_missing("@spinner").await.unwrap(); // removed after 500 ms
    page.assert_count("@card", 3).await.unwrap();
    page.assert_enabled("Later").await.unwrap(); // enabled after 500 ms
    page.assert_see_in("[data-stage=won]", "Gamma renewal").await.unwrap();

    let quick = page.with_timeout(Duration::from_millis(200));
    let error = quick.assert_value("Memo", "Other").await.unwrap_err();
    assert!(error.to_string().contains(r#"its value is "Call""#), "{error}");
    let error = quick.assert_count("@card", 5).await.unwrap_err();
    assert!(error.to_string().contains("3 visible elements match"), "{error}");
    let error = quick.assert_visible("Secret text").await.unwrap_err();
    assert!(error.to_string().contains(r#""Secret text" is not visible"#), "{error}");
    let error = quick.assert_visible("@nothing").await.unwrap_err();
    assert!(error.to_string().contains(r#"no element matches "@nothing""#), "{error}");
    let error = quick.assert_missing("Playground").await.unwrap_err();
    assert!(error.to_string().contains("is still visible"), "{error}");
    let error = quick
        .assert_see_in("[data-stage=won]", "Alpha renewal")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not show the text"), "{error}");
}

#[tokio::test]
async fn console_errors_and_uncaught_exceptions_fail_the_check() {
    let (browser, site) = browser_or_skip!("console_errors_and_uncaught_exceptions_fail_the_check");
    let page = browser.visit(site.url("/second")).await.unwrap();
    page.assert_no_console_errors().await.unwrap();

    let broken = browser.visit(site.url("/broken")).await.unwrap();
    broken.assert_see("Half a page").await.unwrap();
    // Let the timer throw and the failed request finish.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = Instant::now();
    let error = broken.assert_no_console_errors().await.unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(4), "it does not wait");
    let message = error.to_string();
    assert!(message.starts_with("assert_no_console_errors() failed: 2 console errors"), "{message}");
    assert!(message.contains("console.error: the widget failed"), "{message}");
    assert!(message.contains("kaboom"), "{message}");
    assert!(!message.contains("missing.json"), "failed requests are only logged: {message}");

    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.press("Log error").await.unwrap();
    page.with_timeout(Duration::from_millis(100))
        .assert_no_console_errors()
        .await
        .unwrap_err();
}
