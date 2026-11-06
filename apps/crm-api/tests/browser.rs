//! Browser journeys through crm-web against the CRM API
//! (`packages/browser-testing.mdx`). They need Chrome or Chromium, pnpm and
//! a production build of crm-web (`pnpm --dir apps/crm-web build`); without
//! them they print a notice and pass, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.
//!
//! Run them on their own with `cargo test -p crm-api --test browser`.

use std::path::Path;

use crm_testkit::{Role, TestCrm};
use navree_browser::{Browser, BrowserError, FrontEnd, Page, Target, WebSession};
use serde_json::json;

/// crm-web, built with `pnpm --dir apps/crm-web build`, started once per test binary.
fn crm_web() -> FrontEnd {
    FrontEnd::new("crm-web")
        .command("pnpm", ["--dir", "../crm-web", "start", "--port", "3100"])
        .url("http://127.0.0.1:3100")
        .api_env("NAVREE_API_URL")
        // `next start` is a production server: allow its cookies over plain http.
        .env("NAVREE_COOKIE_SECURE", "0")
}

/// Whether the journeys can run here; prints why not.
fn ready(test: &str) -> bool {
    let missing = if let Err(error) = Browser::find_chrome() {
        Some(error.to_string())
    } else if !Path::new("../crm-web/.next/BUILD_ID").is_file() {
        Some("crm-web is not built: run `pnpm --dir apps/crm-web install` and `pnpm --dir apps/crm-web build`".to_owned())
    } else {
        None
    };
    let Some(reason) = missing else {
        return true;
    };
    if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") {
        panic!("{test}: {reason} (NAVREE_TEST_REQUIRE_SERVICES=1)");
    }
    eprintln!("skipped {test}: {reason}");
    false
}

#[tokio::test]
async fn a_rep_signs_in_and_sees_the_pipeline() -> Result<(), BrowserError> {
    if !ready("a_rep_signs_in_and_sees_the_pipeline") {
        return Ok(());
    }
    let crm = TestCrm::builder().app(crm_api::app).build().await;
    let web = crm_web().serve(crm.ctx()).await?;
    let browser = Browser::launch().await?;

    let page = browser.visit(web.url("/login")).await?;
    page.type_into("Email", "rep1@navree.test").await?;
    page.type_into("Password", "password123").await?;
    page.press("Sign in").await?;

    page.assert_path_is("/").await?;
    page.assert_see("Pipeline by stage").await?;
    page.assert_no_console_errors().await?;
    Ok(())
}

#[tokio::test]
async fn a_rep_moves_a_deal_to_won() -> Result<(), BrowserError> {
    if !ready("a_rep_moves_a_deal_to_won") {
        return Ok(());
    }
    let crm = TestCrm::builder().app(crm_api::app).build().await;
    let rep = crm.login_as(Role::Rep1).await;
    let company = rep
        .post("/api/v1/companies")
        .json(&json!({ "name": "Lakhotia Analytics" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let deal = rep
        .post("/api/v1/deals")
        .json(&json!({
            "title": "Lakhotia renewal",
            "amount_cents": 4_200_000,
            "company_id": company["id"],
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();

    let web = crm_web().serve(crm.ctx()).await?;
    // Wide enough for the board's six columns side by side.
    let browser = Browser::builder().window(2200, 1100).launch().await?;
    let page = sign_in(&browser, &web, Role::Rep1).await?;

    page.click("Deals").await?;
    let card = Target::from("@deal-card").containing("Lakhotia renewal");
    page.drag(card, "[data-stage=won]").await?;
    page.assert_see_in("[data-stage=won]", "Lakhotia renewal")
        .await?;
    // The board moves the card at once; wait for its API call to finish.
    let moved = page.wait_for_request("/api/v1/deals/*/move").await?;
    assert_eq!(moved.status, Some(200));

    rep.get(&format!("/api/v1/deals/{}", deal["id"].as_str().unwrap()))
        .send()
        .await
        .assert_json_path("stage", json!("won"));
    Ok(())
}

/// Signs in through the login form, as a user would.
async fn sign_in(browser: &Browser, web: &WebSession, role: Role) -> Result<Page, BrowserError> {
    let page = browser.visit(web.url("/login")).await?;
    page.type_into("Email", role.email()).await?;
    page.type_into("Password", "password123").await?;
    page.press("Sign in").await?;
    page.assert_path_is("/").await?;
    Ok(page)
}
