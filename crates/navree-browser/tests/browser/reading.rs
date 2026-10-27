//! Reading the page: text, values, scripts and cookies.

use std::time::Duration;

use serde::Deserialize;

use crate::browser_or_skip;

#[tokio::test]
async fn text_and_value_read_the_first_visible_match() {
    let (browser, site) = browser_or_skip!("text_and_value_read_the_first_visible_match");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    assert_eq!(page.text("#title").await.unwrap(), "Playground");
    assert_eq!(page.text("Loaded later").await.unwrap(), "Loaded later");
    assert_eq!(page.value("Memo").await.unwrap(), "Call");
    assert_eq!(page.value("Stage").await.unwrap(), "lead");
    let error = page
        .with_timeout(Duration::from_millis(200))
        .text("@nothing")
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("no element matches \"@nothing\""),
        "{error}"
    );
}

#[tokio::test]
async fn script_runs_a_function_body_and_deserializes_its_result() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Size {
        width: u32,
        height: u32,
    }
    let (browser, site) =
        browser_or_skip!("script_runs_a_function_body_and_deserializes_its_result");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let title: String = page.script("return document.title").await.unwrap();
    assert_eq!(title, "Playground · Fixture");
    let size: Size = page
        .script("return { width: innerWidth, height: innerHeight }")
        .await
        .unwrap();
    assert_eq!(
        size,
        Size {
            width: 1440,
            height: 900
        }
    );
    let awaited: u32 = page
        .script("await new Promise(r => setTimeout(r, 10)); return 42")
        .await
        .unwrap();
    assert_eq!(awaited, 42);
    page.script::<()>("window.__done = true").await.unwrap();

    let error = page.script::<u32>("return 'text'").await.unwrap_err();
    assert!(error.to_string().contains("is not a u32"), "{error}");
    let error = page
        .script::<()>("throw new Error('nope')")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("nope"), "{error}");
}

#[tokio::test]
async fn cookies_are_read_with_their_flags() {
    let (browser, site) = browser_or_skip!("cookies_are_read_with_their_flags");
    let page = browser.visit(site.url("/login")).await.unwrap();
    assert_eq!(page.cookie("navree_at").await.unwrap(), None);
    page.type_into("Email", "rep1@navree.test").await.unwrap();
    page.type_into("Password", "password123").await.unwrap();
    page.press("Sign in").await.unwrap();
    page.assert_see("Pipeline by stage").await.unwrap();
    let cookie = page.cookie("navree_at").await.unwrap().expect("the cookie");
    assert_eq!(cookie.value, "token-rep1");
    assert!(cookie.http_only);
    assert!(!cookie.secure);
    assert_eq!(cookie.same_site.as_deref(), Some("Lax"));
    assert_eq!(cookie.path, "/");
    assert_eq!(cookie.expires, None);
    // Scripts cannot see it.
    let visible: String = page.script("return document.cookie").await.unwrap();
    assert!(!visible.contains("navree_at"));
}

#[tokio::test]
async fn url_reports_the_current_address() {
    let (browser, site) = browser_or_skip!("url_reports_the_current_address");
    let page = browser.visit(site.url("/second")).await.unwrap();
    assert_eq!(page.url().await.unwrap(), site.url("/second"));
}
