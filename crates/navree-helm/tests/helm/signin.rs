//! Signing in to the panel on the app's browser session: the sign-in pages
//! are the app's (`AuthPages`), the panel sends guests there and takes
//! whoever signed in, with the session's CSRF token on every change.

use std::sync::Arc;
use std::time::Duration;

use navree_access::AccessModule;
use navree_auth::{AuthModule, AuthPages, AuthService, ClientMeta};
use navree_contracts::{Clock, FixedClock};
use navree_crypto::CryptoModule;
use navree_crypto::totp::{Secret, Totp};
use navree_helm::HelmModule;
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestClient, TestResponse};
use navree_ui::UiModule;
use serde_json::{Value, json};

use crate::support::*;

const PASSWORD: &str = "correct horse battery";

/// The session cookie of a browser that signs in.
const SESSION_COOKIE: &str = "__Host-navree_session";

/// An app with real sign-in: the session, the auth module and its pages, and
/// the clock.
async fn signin_app() -> (TestApp, Arc<FixedClock>) {
    let clock = Arc::new(FixedClock::new(now()));
    let app = crate::servers::database(TestApp::builder())
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .override_value::<dyn Clock>(clock.clone())
        .module(CryptoModule::from_config())
        .module(navree_db::DatabaseModule)
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule::from_config())
        .module(UiModule)
        .module(AuthPages::new().app_name("Agra CRM"))
        .module(AccessModule::for_root(options()))
        .module(CrmModule)
        .module(HelmModule::from_config())
        .build()
        .await;
    (app, clock)
}

/// A user with `role` and `helm.view`.
async fn account(app: &TestApp, email: &str, role: &str) -> uuid::Uuid {
    let auth = app.get::<AuthService>().await;
    let user = auth
        .create_user("Priya Patel", email, PASSWORD)
        .await
        .unwrap();
    give_role(app, user.id, role, &["helm.view"]).await;
    user.id
}

async fn get(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// Posts `form` with the session's CSRF token, as a page without JavaScript
/// does.
async fn post(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    let token = client
        .cookie("XSRF-TOKEN")
        .expect("a page handed out the token");
    let mut form = form.to_vec();
    form.push(("_csrf", token.as_str()));
    client
        .post(path)
        .header("accept", "text/html")
        .form(&form)
        .send()
        .await
}

/// Signs `client` in on the app's sign-in page, landing on `next`.
async fn sign_in(client: &TestClient, email: &str, next: &str) -> TestResponse {
    get(client, "/sign-in").await.assert_status(200);
    post(
        client,
        "/sign-in",
        &[("email", email), ("password", PASSWORD), ("next", next)],
    )
    .await
}

/// The CSRF token a panel page carries.
fn csrf(html: &str) -> String {
    let marker = "<input type=\"hidden\" name=\"_csrf\" value=\"";
    let start = html.find(marker).unwrap() + marker.len();
    html[start..].split('"').next().unwrap().to_owned()
}

#[tokio::test]
async fn a_guest_signs_in_on_the_apps_page_and_comes_back() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let client = app.client();

    // The panel sends a guest to the app's sign-in page, and keeps the page.
    let response = get(&client, "/admin/resources/deals?page=2")
        .await
        .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/sign-in?next=%2Fadmin%2Fresources%2Fdeals%3Fpage%3D2")
    );
    // API clients get the JSON error instead.
    client
        .get("/admin/api/resources")
        .send()
        .await
        .assert_error(401, None);

    let response = sign_in(&client, "priya@example.com", "/admin/resources/deals")
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/admin/resources/deals"));
    // The cookie is the app's session: no cookie of the panel's own.
    let cookie = response.set_cookie(SESSION_COOKIE).expect("the session");
    assert_eq!(cookie.http_only(), Some(true));
    assert!(response.set_cookie("navree_helm").is_none());

    let html = get(&client, "/admin/resources/deals")
        .await
        .assert_status(200)
        .text();
    assert!(html.contains("Priya Patel"));
    assert!(html.contains("Sign out"));
    // The sign-out form posts to the app's sign-out route.
    assert!(
        html.replace("&#x2f;", "/").contains("action=\"/sign-out\""),
        "{html}"
    );
    // The panel's API takes the same session.
    client
        .get("/admin/api/resources")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn changes_need_the_csrf_token() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let client = app.client();
    sign_in(&client, "priya@example.com", "/admin")
        .await
        .assert_status(303);
    let html = get(&client, "/admin").await.text();
    let token = csrf(&html);
    assert!(!token.is_empty());
    // It is the session's token, the one the `XSRF-TOKEN` cookie holds, masked.
    let cookie = client.cookie("XSRF-TOKEN").expect("the token cookie");
    assert_ne!(cookie, token);
    assert_eq!(navree_http::csrf::unmask_token(&token), Some(cookie));

    client
        .post("/admin/api/resources/deals")
        .json(&json!({ "title": "No token", "amount_cents": 1 }))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    client
        .post("/admin/api/resources/deals")
        .header("x-csrf-token", "forged")
        .json(&json!({ "title": "Forged", "amount_cents": 1 }))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    client
        .post("/admin/api/resources/deals")
        .header("x-csrf-token", token.as_str())
        .json(&json!({ "title": "With token", "amount_cents": 1 }))
        .send()
        .await
        .assert_status(201);
    // Forms send it as `_csrf`.
    client
        .post("/admin/resources/deals")
        .header("accept", "text/html")
        .form(&[("title", "Form"), ("amount_cents", "1")])
        .send()
        .await
        .assert_status(403);
    client
        .post("/admin/resources/deals")
        .header("accept", "text/html")
        .form(&[
            ("title", "Form"),
            ("amount_cents", "1"),
            ("_csrf", token.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
}

#[tokio::test]
async fn a_user_without_the_panel_gets_the_error_page() {
    let (app, _) = signin_app().await;
    // Signed in, but `helm.view` was never given.
    let auth = app.get::<AuthService>().await;
    auth.create_user("Priya Patel", "priya@example.com", PASSWORD)
        .await
        .unwrap();
    let client = app.client();
    sign_in(&client, "priya@example.com", "/admin")
        .await
        .assert_status(303);
    let response = get(&client, "/admin").await.assert_status(403);
    assert!(
        response
            .header("content-type")
            .unwrap()
            .starts_with("text/html")
    );
    assert!(response.text().contains("No access"));
}

#[tokio::test]
async fn accounts_with_two_factor_enter_a_code_on_the_apps_page() {
    let (app, clock) = signin_app().await;
    let id = account(&app, "priya@example.com", "rep").await;
    let auth = app.get::<AuthService>().await;
    let setup = auth
        .enable_two_factor(id, &ClientMeta::default())
        .await
        .unwrap();
    let code = |clock: &FixedClock| {
        Totp::builder(Secret::from_base32(&setup.secret).unwrap())
            .build()
            .unwrap()
            .generate(u64::try_from(clock.now().timestamp()).unwrap())
    };
    auth.confirm_two_factor(id, &code(&clock), &ClientMeta::default())
        .await
        .unwrap();
    clock.advance(Duration::from_secs(30));

    let client = app.client();
    let response = sign_in(&client, "priya@example.com", "/admin")
        .await
        .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/sign-in/two-factor?next=%2Fadmin")
    );
    // The password alone opens nothing.
    get(&client, "/admin").await.assert_status(303);
    get(&client, "/sign-in/two-factor?next=%2Fadmin")
        .await
        .assert_status(200);
    let response = post(
        &client,
        "/sign-in/two-factor",
        &[("code", code(&clock).as_str()), ("next", "/admin")],
    )
    .await
    .assert_status(303);
    assert_eq!(response.header("location"), Some("/admin"));
    client
        .get("/admin/api/resources")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn signing_out_ends_a_copied_session_cookie() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let client = app.client();
    let response = sign_in(&client, "priya@example.com", "/admin")
        .await
        .assert_status(303);
    let copied = response
        .set_cookie(SESSION_COOKIE)
        .expect("the session")
        .value()
        .to_owned();
    let with_copy = |path: &str| {
        app.client()
            .get(path)
            .header("cookie", format!("{SESSION_COOKIE}={copied}"))
    };
    with_copy("/admin/api/resources")
        .send()
        .await
        .assert_status(200);

    let html = get(&client, "/admin").await.text();
    let response = client
        .post("/sign-out")
        .header("accept", "text/html")
        .form(&[("_csrf", csrf(&html).as_str())])
        .send()
        .await
        .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/sign-in?status=signed-out")
    );
    get(&client, "/admin").await.assert_status(303);
    // The sign-in ended on the server, so the copy no longer opens the panel.
    with_copy("/admin/api/resources")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn bearer_tokens_open_the_panel_without_a_cookie() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let body = app
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": "priya@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let token = body["access_token"].as_str().unwrap();
    // No cookie session, so no CSRF token is needed.
    app.client()
        .post("/admin/api/resources/deals")
        .bearer(token)
        .json(&json!({ "title": "From a BFF", "amount_cents": 1 }))
        .send()
        .await
        .assert_status(201);
}
