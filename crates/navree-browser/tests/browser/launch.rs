//! Launching: one Chrome shared by the tests, a context per browser.

use std::time::Duration;

use navree_browser::{Browser, BrowserError};

use crate::browser_or_skip;
use crate::support::{Fixture, has_chrome};

#[tokio::test]
async fn find_chrome_names_a_browser_binary() {
    if !has_chrome("find_chrome_names_a_browser_binary") {
        return;
    }
    let chrome = Browser::find_chrome().unwrap();
    assert!(chrome.is_file(), "{}", chrome.display());
}

#[tokio::test]
async fn a_browser_reports_its_version_and_opens_blank_pages() {
    let (browser, _site) = browser_or_skip!("a_browser_reports_its_version_and_opens_blank_pages");
    let major: u32 = browser
        .version()
        .split('.')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(major >= 100, "{}", browser.version());
    let page = browser.new_page().await.unwrap();
    assert_eq!(page.url().await.unwrap(), "about:blank");
    assert_eq!(browser.timeout(), Duration::from_secs(5));
}

#[tokio::test]
async fn browsers_share_chrome_but_not_cookies() {
    let (first, site) = browser_or_skip!("browsers_share_chrome_but_not_cookies");
    let second = Browser::launch().await.unwrap();
    assert_eq!(first.version(), second.version());

    let page = first.visit(site.url("/login")).await.unwrap();
    page.type_into("Email", "rep1@navree.test").await.unwrap();
    page.type_into("Password", "password123").await.unwrap();
    page.press("Sign in").await.unwrap();
    page.assert_path_is("/").await.unwrap();
    assert!(page.cookie("navree_at").await.unwrap().is_some());

    // The second browser has a context of its own: no cookie, so the
    // dashboard sends it to the login page.
    let other = second.visit(site.url("/")).await.unwrap();
    other.assert_path_is("/login").await.unwrap();
    assert_eq!(other.cookie("navree_at").await.unwrap(), None);

    // A new page of the first browser shares its cookies.
    let again = first.visit(site.url("/")).await.unwrap();
    again.assert_see("Pipeline by stage").await.unwrap();
}

#[tokio::test]
async fn the_builder_sets_the_window_and_timeout() {
    if !has_chrome("the_builder_sets_the_window_and_timeout") {
        return;
    }
    let site = Fixture::start().await;
    let browser = Browser::builder()
        .window(1024, 700)
        .timeout(Duration::from_millis(1500))
        .slow_mo(1)
        .launch()
        .await
        .unwrap();
    assert_eq!(browser.timeout(), Duration::from_millis(1500));
    let page = browser.visit(site.url("/mobile")).await.unwrap();
    let width: u32 = page.script("return innerWidth").await.unwrap();
    assert_eq!(width, 1024);
    assert_eq!(page.timeout(), Duration::from_millis(1500));
}

#[tokio::test]
async fn a_missing_browser_binary_is_reported() {
    let error = Browser::builder()
        .chrome("/no/such/chrome")
        .launch()
        .await
        .unwrap_err();
    assert!(error.is_chrome_not_found(), "{error}");
    assert!(error.to_string().contains("/no/such/chrome"), "{error}");
}

#[tokio::test]
async fn invalid_builder_values_are_config_errors() {
    let error = Browser::builder()
        .timeout(Duration::ZERO)
        .launch()
        .await
        .unwrap_err();
    assert!(matches!(error, BrowserError::Config(_)), "{error}");
    let error = Browser::builder().window(0, 10).launch().await.unwrap_err();
    assert!(error.to_string().contains("window"), "{error}");
}

#[tokio::test]
async fn a_binary_that_is_not_a_browser_fails_to_launch() {
    // The test executable itself never opens a DevTools port: it exits.
    let me = std::env::current_exe().unwrap();
    let error = Browser::builder()
        .chrome(&me)
        .launch()
        .await
        .unwrap_err();
    assert!(matches!(error, BrowserError::Launch { .. }), "{error}");
}
