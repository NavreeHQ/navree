//! Signing in to the panel with the app's users: the session cookie, CSRF,
//! two-factor codes, the login throttle and signing out.

use std::sync::Arc;
use std::time::Duration;

use navree_access::AccessModule;
use navree_auth::{AuthModule, AuthService, ClientMeta};
use navree_contracts::{Clock, FixedClock};
use navree_crypto::CryptoModule;
use navree_helm::HelmModule;
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::{Value, json};
use totp_rs::{Builder, Secret};

use crate::support::*;

const PASSWORD: &str = "correct horse battery";

/// An app with real sign-in (AuthModule, CryptoModule) and its clock.
async fn signin_app() -> (TestApp, Arc<FixedClock>) {
    let clock = Arc::new(FixedClock::new(now()));
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .override_value::<dyn Clock>(clock.clone())
        .module(CryptoModule::from_config())
        .module(navree_db::DatabaseModule)
        .module(AuthModule::from_config())
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

/// The value of `name="{name}" value="..."` in a page.
fn hidden(html: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\" value=\"");
    let start = html
        .find(&marker)
        .unwrap_or_else(|| panic!("no {name} in {html}"))
        + marker.len();
    html[start..].split('"').next().unwrap().to_owned()
}

/// The CSRF token of a panel page.
fn csrf(html: &str) -> String {
    let marker = "<meta name=\"csrf-token\" content=\"";
    let start = html.find(marker).unwrap() + marker.len();
    html[start..].split('"').next().unwrap().to_owned()
}

async fn sign_in_form(client: &TestClient) -> String {
    let response = client
        .get("/admin/login")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    hidden(&response.text(), "_login")
}

async fn sign_in(client: &TestClient, email: &str, password: &str, next: &str) -> TestResponse {
    let token = sign_in_form(client).await;
    client
        .post("/admin/login")
        .header("accept", "text/html")
        .form(&[
            ("email", email),
            ("password", password),
            ("_login", &token),
            ("next", next),
        ])
        .send()
        .await
}

#[tokio::test]
async fn signing_in_sets_a_strict_cookie_scoped_to_the_panel() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let client = app.client();
    let response = sign_in(
        &client,
        "priya@example.com",
        PASSWORD,
        "/admin/resources/deals",
    )
    .await
    .assert_status(303);
    assert_eq!(response.header("location"), Some("/admin/resources/deals"));
    let cookie = response
        .set_cookie("navree_helm")
        .expect("a session cookie");
    assert_eq!(cookie.path(), Some("/admin"));
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.same_site(), Some(cookie::SameSite::Strict));

    let html = client
        .get("/admin/resources/deals")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(html.contains("Priya Patel"));
    assert!(html.contains("Sign out"));

    // The API under the panel accepts it too.
    client
        .get("/admin/api/resources")
        .send()
        .await
        .assert_status(200);
    // The app's own API never does.
    let value = cookie.value().to_owned();
    app.client()
        .get("/api/auth/me")
        .header("cookie", format!("navree_helm={value}"))
        .send()
        .await
        .assert_error(401, None);
    // Signed in, the sign-in screen sends you on.
    let response = client
        .get("/admin/login")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/admin"));
}

#[tokio::test]
async fn changes_need_the_csrf_token() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let client = app.client();
    sign_in(&client, "priya@example.com", PASSWORD, "/admin")
        .await
        .assert_status(303);
    let html = client
        .get("/admin")
        .header("accept", "text/html")
        .send()
        .await
        .text();
    let token = csrf(&html);
    assert!(!token.is_empty());

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
async fn wrong_passwords_and_stale_forms_are_refused() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let client = app.client();
    let response = sign_in(&client, "priya@example.com", "wrong password", "/admin")
        .await
        .assert_status(401);
    assert!(
        response
            .text()
            .contains("These credentials do not match our records.")
    );
    assert!(response.set_cookie("navree_helm").is_none());

    let response = client
        .post("/admin/login")
        .header("accept", "text/html")
        .form(&[
            ("email", "priya@example.com"),
            ("password", PASSWORD),
            ("_login", "stale"),
        ])
        .send()
        .await
        .assert_status(403);
    assert!(response.text().contains("The sign-in form expired"));

    // The login throttle of `POST /api/auth/login`: 5 failures a minute.
    for _ in 0..4 {
        sign_in(&client, "priya@example.com", "wrong", "/admin")
            .await
            .assert_status(401);
    }
    let response = sign_in(&client, "priya@example.com", "wrong", "/admin")
        .await
        .assert_status(429);
    assert!(response.text().contains("Too many attempts"));
    assert!(response.header("retry-after").is_some());
}

#[tokio::test]
async fn next_never_leaves_the_panel() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let response = sign_in(
        &app.client(),
        "priya@example.com",
        PASSWORD,
        "https://evil.example/admin",
    )
    .await;
    assert_eq!(
        response.assert_status(303).header("location"),
        Some("/admin")
    );
}

#[tokio::test]
async fn accounts_with_two_factor_enter_a_code() {
    let (app, clock) = signin_app().await;
    let id = account(&app, "priya@example.com", "rep").await;
    let auth = app.get::<AuthService>().await;
    let setup = auth
        .enable_two_factor(id, &ClientMeta::default())
        .await
        .unwrap();
    let code = |clock: &FixedClock| {
        Builder::new()
            .with_secret(Secret::try_from_base32(&setup.secret).unwrap())
            .build()
            .unwrap()
            .generate(u64::try_from(clock.now().timestamp()).unwrap())
            .to_string()
    };
    auth.confirm_two_factor(id, &code(&clock), &ClientMeta::default())
        .await
        .unwrap();
    clock.advance(Duration::from_secs(30));

    let client = app.client();
    let response = sign_in(&client, "priya@example.com", PASSWORD, "/admin")
        .await
        .assert_status(200);
    assert!(response.set_cookie("navree_helm").is_none());
    let html = response.text();
    assert!(html.contains("Enter the code from your authenticator app."));
    let challenge = hidden(&html, "challenge_token");
    let login = hidden(&html, "_login");

    let wrong = client
        .post("/admin/login/two-factor")
        .header("accept", "text/html")
        .form(&[
            ("challenge_token", challenge.as_str()),
            ("code", "000000"),
            ("_login", login.as_str()),
        ])
        .send()
        .await;
    if wrong.status().as_u16() == 401 {
        assert!(wrong.text().contains("That code is not valid."));
    }
    let html = wrong.text();
    let login = hidden(&html, "_login");
    let response = client
        .post("/admin/login/two-factor")
        .header("accept", "text/html")
        .form(&[
            ("challenge_token", challenge.as_str()),
            ("code", code(&clock).as_str()),
            ("_login", login.as_str()),
            ("next", "/admin/resources/companies"),
        ])
        .send()
        .await
        .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/admin/resources/companies")
    );
    assert!(response.set_cookie("navree_helm").is_some());
    client
        .get("/admin/api/resources")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn signing_out_ends_the_session() {
    let (app, _) = signin_app().await;
    account(&app, "priya@example.com", "rep").await;
    let client = app.client();
    sign_in(&client, "priya@example.com", PASSWORD, "/admin")
        .await
        .assert_status(303);
    let html = client
        .get("/admin")
        .header("accept", "text/html")
        .send()
        .await
        .text();
    let response = client
        .post("/admin/logout")
        .header("accept", "text/html")
        .form(&[("_csrf", csrf(&html).as_str())])
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/admin/login"));
    let cleared = response.set_cookie("navree_helm").unwrap();
    assert_eq!(cleared.value(), "");
    let response = client
        .get("/admin")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/admin/login?next=%2Fadmin")
    );
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
