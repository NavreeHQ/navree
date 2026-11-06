//! Every public item `packages/browser-testing.mdx` names, so a rename
//! fails the build. The function is compiled, never run.

use std::path::PathBuf;
use std::time::Duration;

use navree_browser::{
    Browser, BrowserBuilder, BrowserError, Cookie, Element, FailureFiles, FrontEnd, Page,
    PageRequest, StepFailure, Target, TestServer, Viewport, WebSession,
};
use navree_kernel::AppContext;

#[allow(dead_code, unused_variables, clippy::too_many_lines)]
async fn surface(ctx: &AppContext) -> Result<(), BrowserError> {
    // Browser and its builder.
    let chrome: PathBuf = Browser::find_chrome()?;
    let builder: BrowserBuilder = Browser::builder()
        .headless(false)
        .chrome(chrome)
        .slow_mo(250)
        .timeout(Duration::from_secs(5))
        .window(1440, 900)
        .artifacts("target/navree-browser");
    let browser: Browser = builder.launch().await?;
    let browser: Browser = Browser::launch().await?;
    let page: Page = browser.visit("http://127.0.0.1:3100/login").await?;
    let page: Page = browser
        .visit_with("http://127.0.0.1:3100", Viewport::phone())
        .await?;
    let page: Page = browser.new_page().await?;
    let _: &str = browser.version();

    // Targets.
    let target: Target = Target::from("@deal-card")
        .containing("Lakhotia renewal")
        .nth(0);

    // Actions.
    page.type_into("Title", "Lakhotia renewal").await?;
    page.append("Notes", " Call back Monday.").await?;
    page.select("Stage", "Negotiation").await?;
    page.check("Send me a summary").await?;
    page.uncheck("Send me a summary").await?;
    page.attach("Logo", "tests/fixtures/agra.png").await?;
    page.press("Save").await?;
    page.click(target.clone()).await?;
    page.keys("Control+K").await?;
    page.drag("@deal-card", "[data-stage=won]").await?;
    page.scroll_to("@load-more").await?;
    page.back().await?;
    page.refresh().await?;
    page.visit("http://127.0.0.1:3100/deals").await?;
    page.resize(768, 1024).await?;

    // Assertions.
    page.assert_see("Deal saved").await?;
    page.assert_dont_see("Something went wrong").await?;
    page.assert_see_in("[data-stage=won]", "Lakhotia renewal")
        .await?;
    page.assert_path_is("/deals").await?;
    page.assert_query_has("stage", "won").await?;
    page.assert_title("Deals · Navree CRM").await?;
    page.assert_value("Title", "Lakhotia renewal").await?;
    page.assert_visible("@toast").await?;
    page.assert_missing("@spinner").await?;
    page.assert_count("@deal-card", 3).await?;
    page.assert_enabled("Save").await?;
    page.assert_no_console_errors().await?;

    // Waits.
    page.wait_for("@chart").await?;
    page.wait_until_missing("Loading…").await?;
    page.wait_for_url("/deals/*").await?;
    let request: PageRequest = page.wait_for_request("/api/v1/deals/*/move").await?;
    page.with_timeout(Duration::from_secs(20))
        .assert_see("Export ready")
        .await?;

    // Reading.
    let title: String = page.text("@deal-title").await?;
    let value: String = page.value("Amount").await?;
    let cards: Vec<Element> = page.all("@deal-card").await?;
    let total: f64 = page.script("return window.__DEAL_TOTAL__").await?;
    let cookie: Option<Cookie> = page.cookie("navree_at").await?;
    let url: String = page.url().await?;
    let element: Element = page.within(target).await?;
    element.press("Edit").await?;
    let text: String = element.inner_text().await?;

    // Screenshots.
    let shot: PathBuf = page.screenshot("board-after-move").await?;
    let full: PathBuf = page.screenshot_full_page("board").await?;

    // Viewports.
    let viewports = [
        Viewport::phone(),
        Viewport::tablet(),
        Viewport::desktop(),
        Viewport::new(1024, 768),
    ];

    // Serving the app and the front end.
    let server: TestServer = TestServer::start(ctx).await?;
    let _: String = server.url("/api/v1/deals");
    let web: WebSession = FrontEnd::new("crm-web")
        .command("pnpm", ["--dir", "../crm-web", "start", "--port", "3100"])
        .url("http://127.0.0.1:3100")
        .api_env("NAVREE_API_URL")
        .env("NAVREE_COOKIE_SECURE", "0")
        .timeout(Duration::from_secs(60))
        .serve(ctx)
        .await?;
    let _: String = web.url("/login");

    // Errors.
    let error = BrowserError::Config(String::new());
    let _: bool = error.is_chrome_not_found();
    let _: Option<&StepFailure> = error.step();
    let _: Option<&FailureFiles> = error.files();
    Ok(())
}

#[test]
fn the_documented_api_exists() {
    let _ = surface;
}
