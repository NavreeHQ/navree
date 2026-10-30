//! crm-web's backend-for-frontend calls the API for every
//! browser, from its own address (loopback in the documented setup), and
//! names the browser in `X-Forwarded-For`. The API must tell the browsers
//! apart, or one browser's wrong passwords lock every user of the web app
//! out of signing in, and all of them share one per-IP rate limit.

use crm_core::seed::DEMO_PASSWORD;
use crm_testkit::{Role, TestCrm};
use navree::testing::TestResponse;
use serde_json::json;

async fn crm() -> TestCrm {
    TestCrm::builder().app(crm_api::app).build().await
}

/// `POST /api/auth/login` through the BFF for the browser at `client`.
async fn login(crm: &TestCrm, client: &str, password: &str) -> TestResponse {
    crm.client()
        .post("/api/auth/login")
        .header("x-forwarded-for", client)
        .json(&json!({ "email": Role::Rep1.email(), "password": password }))
        .send()
        .await
}

#[tokio::test]
async fn the_login_throttle_is_per_browser_behind_the_bff() {
    let crm = crm().await;
    for _ in 0..5 {
        login(&crm, "203.0.113.10", "wrong password")
            .await
            .assert_status(401);
    }
    login(&crm, "203.0.113.10", "wrong password")
        .await
        .assert_status(429);
    // Another browser signs in as the same user.
    login(&crm, "203.0.113.20", DEMO_PASSWORD)
        .await
        .assert_status(200);
    crm.shutdown().await;
}

#[tokio::test]
async fn the_per_ip_rate_limit_is_per_browser_behind_the_bff() {
    let crm = TestCrm::builder()
        .app(crm_api::app)
        .config("http.per_ip_rate_limit", 3)
        .build()
        .await;
    let rep = crm.login_as(Role::Rep1).await;
    let me = |client: &'static str| {
        rep.get("/api/v1/me")
            .header("x-forwarded-for", client)
            .send()
    };
    for _ in 0..3 {
        me("203.0.113.10").await.assert_status(200);
    }
    me("203.0.113.10").await.assert_status(429);
    me("203.0.113.20").await.assert_status(200);
    crm.shutdown().await;
}
