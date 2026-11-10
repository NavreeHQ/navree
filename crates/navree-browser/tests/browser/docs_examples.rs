//! The examples of `packages/browser-testing.mdx`, run against the
//! fixture app (the CRM ones run in the CRM repository's `api/tests/browser.rs`).

use std::time::Duration;

use navree_browser::{Browser, BrowserError, Page, Target, TestServer, Viewport};

use crate::support::{self, has_chrome};

/// Signs in through the login form, as a user would.
async fn sign_in(browser: &Browser, server: &TestServer) -> Result<Page, BrowserError> {
    let page = browser.visit(server.url("/login")).await?;
    page.type_into("Email", "rep1@navree.test").await?;
    page.type_into("Password", "password123").await?;
    page.press("Sign in").await?;
    page.assert_path_is("/").await?;
    Ok(page)
}

#[tokio::test]
async fn a_first_test() -> Result<(), BrowserError> {
    if !has_chrome("a_first_test") {
        return Ok(());
    }
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await?;
    let browser = Browser::launch().await?;

    let page = browser.visit(server.url("/login")).await?;
    page.type_into("Email", "rep1@navree.test").await?;
    page.type_into("Password", "password123").await?;
    page.press("Sign in").await?;

    page.assert_path_is("/").await?;
    page.assert_see("Pipeline by stage").await?;
    page.assert_no_console_errors().await?;
    Ok(())
}

#[tokio::test]
async fn a_wrong_password_stays_on_the_login_page() -> Result<(), BrowserError> {
    if !has_chrome("a_wrong_password_stays_on_the_login_page") {
        return Ok(());
    }
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await?;
    let browser = Browser::launch().await?;
    let page = browser.visit(server.url("/login")).await?;
    page.type_into("Email", "rep1@navree.test").await?;
    page.type_into("Password", "not-the-password").await?;
    page.press("Sign in").await?;
    page.assert_path_is("/login").await?;
    page.assert_see("That email and password don't match an account.")
        .await?;
    Ok(())
}

#[tokio::test]
async fn finding_elements() -> Result<(), BrowserError> {
    if !has_chrome("finding_elements") {
        return Ok(());
    }
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await?;
    let browser = Browser::launch().await?;
    let page = sign_in(&browser, &server).await?;
    page.click("Playground").await?;

    let renewal = Target::from("@card").containing("Gamma renewal");
    page.click(renewal.clone()).await?;

    let card = page.within(renewal).await?;
    card.press("Edit").await?;
    page.assert_see("Editing Gamma renewal").await?;
    page.click(Target::from("@card").nth(1)).await?;
    Ok(())
}

#[tokio::test]
async fn actions_assertions_waits_and_reading() -> Result<(), BrowserError> {
    if !has_chrome("actions_assertions_waits_and_reading") {
        return Ok(());
    }
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await?;
    let browser = Browser::launch().await?;
    let page = browser.visit(server.url("/playground")).await?;
    let logo = std::env::temp_dir().join(format!(
        "navree-test-browser-agra-{}.png",
        std::process::id()
    ));
    std::fs::write(&logo, b"png").expect("a fixture file");

    // Actions.
    page.type_into("Name", "Lakhotia renewal").await?; // clears, then types
    page.append("Memo", " back Monday.").await?; // types without clearing
    page.select("Stage", "Negotiation").await?; // by text or value
    page.check("Send me a summary").await?;
    page.uncheck("Send me a summary").await?;
    page.attach("Logo", &logo).await?; // a file input
    page.press("Save").await?; // a button or link
    page.assert_see("Saved!").await?;
    page.click("@card").await?; // any element
    page.keys("Escape").await?; // "Enter", "Control+K"
    page.drag(Target::from("@card").containing("Beta"), "[data-stage=won]")
        .await?;
    page.scroll_to("@load-more").await?;

    // Assertions.
    page.assert_see("Dropped in won").await?;
    page.assert_dont_see("Something went wrong").await?;
    page.assert_see_in("[data-stage=won]", "Beta launch")
        .await?;
    page.assert_path_is("/playground").await?;
    page.assert_title("Playground · Fixture").await?;
    page.assert_value("Name", "Lakhotia renewal").await?;
    page.assert_visible("@toast").await?;
    page.assert_missing("@spinner").await?;
    page.assert_count("@card", 3).await?;
    page.assert_enabled("Save").await?;

    // Waiting.
    page.wait_for("Loaded later").await?;
    page.wait_until_missing("Loading…").await?;
    page.press("Move deal").await?;
    page.wait_for_request("/api/deals/*/move").await?;
    page.with_timeout(Duration::from_secs(20))
        .assert_see("Moved 42")
        .await?;

    // Reading the page.
    let title: String = page.text("#title").await?;
    assert_eq!(title, "Playground");
    let value: String = page.value("Memo").await?;
    assert_eq!(value, "Call back Monday.");
    let cards = page.all("@card").await?; // Vec<Element>
    assert_eq!(cards.len(), 3);
    let total: f64 = page.script("return 4200000 / 100").await?;
    assert_eq!(total, 42000.0);
    assert_eq!(page.cookie("navree_at").await?, None);

    // Screenshots on purpose.
    page.screenshot("board-after-move").await?;
    page.screenshot_full_page("whole-board").await?;

    // History.
    page.press("Next page").await?;
    page.wait_for_url("/second*").await?;
    page.assert_query_has("stage", "won").await?;
    page.back().await?;
    page.refresh().await?;
    page.assert_path_is("/playground").await?;
    let _ = std::fs::remove_file(&logo);
    Ok(())
}

#[tokio::test]
async fn phones_and_other_viewports() -> Result<(), BrowserError> {
    if !has_chrome("phones_and_other_viewports") {
        return Ok(());
    }
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await?;
    let browser = Browser::launch().await?;

    // 390 x 844, touch, a mobile user agent:
    let page = browser
        .visit_with(server.url("/mobile"), Viewport::phone())
        .await?;
    page.press("Open menu").await?;
    page.assert_see("Companies").await?;

    page.resize(768, 1024).await?;
    page.assert_dont_see("Open menu").await?;
    Ok(())
}

#[tokio::test]
async fn options_in_code() -> Result<(), BrowserError> {
    if !has_chrome("options_in_code") {
        return Ok(());
    }
    let artifacts = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("a temporary folder");
    let browser = Browser::builder()
        .headless(true)
        .chrome(Browser::find_chrome()?)
        .slow_mo(0)
        .timeout(Duration::from_secs(5))
        .window(1440, 900)
        .artifacts(artifacts.path())
        .launch()
        .await?;
    let page = browser.new_page().await?;
    assert_eq!(page.url().await?, "about:blank");
    Ok(())
}
