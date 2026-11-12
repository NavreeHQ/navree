//! The rest of browser sign-in on the pages: the two-factor setup page, the
//! mailed link, the emailed code and the passkey page, the JSON routes of a
//! single-page app, and impersonating in the browser. They go through the
//! full router with navree-session's layer, as a browser drives them.

use std::sync::Arc;

use navree_auth::testing::VirtualAuthenticator;
use navree_auth::{AuthPages, User};
use navree_contracts::{AuditSink, FixedClock};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_ui::UiModule;
use serde_json::{Value, json};

use crate::session_strategy::EveryoneTwoFactorModule;
use crate::support::{
    self, PASSWORD, RecordingSink, create_user, enable_two_factor, grant_role, link_param,
    sent_mail, signed_in, totp_code,
};

const XSRF: &str = "XSRF-TOKEN";

const RP_ID: &str = "example.com";
const ORIGIN: &str = "https://app.example.com";

/// `builder` with the session layer, navree-ui and the sign-in pages.
fn with_pages(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
}

/// The session's CSRF token, from the cookie the pages hand out.
async fn token(client: &TestClient) -> String {
    if client.cookie(XSRF).is_none() {
        client
            .get("/sign-in")
            .header("accept", "text/html")
            .send()
            .await
            .assert_status(200);
    }
    client.cookie(XSRF).expect("the XSRF-TOKEN cookie")
}

/// Posts a form as a browser without JavaScript does.
async fn post_form(client: &TestClient, path: &str, fields: &[(&str, &str)]) -> TestResponse {
    let csrf = token(client).await;
    let mut all = fields.to_vec();
    all.push(("_csrf", csrf.as_str()));
    client
        .post(path)
        .header("accept", "text/html")
        .form(&all)
        .send()
        .await
}

/// Opens `path` as a browser does.
async fn open(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// `GET /api/probe/whoami`.
async fn whoami(client: &TestClient) -> TestResponse {
    client.get("/api/probe/whoami").send().await
}

/// The text between the first `<code data-slot="code">` and its end.
fn first_code(html: &str) -> String {
    let marker = "<code data-slot=\"code\">";
    let start = html.find(marker).expect("a code") + marker.len();
    html[start..]
        .split("</code>")
        .next()
        .expect("a closed code")
        .to_owned()
}

// A user whom a policy makes set up two-factor authentication is
// sent to the setup page after signing in, scans a secret, confirms the
// first code, and is shown recovery codes once; then the app opens.
#[tokio::test]
async fn the_setup_page_sets_two_factor_up_for_a_user_a_policy_requires_it_of() {
    let clock = support::clock();
    let app = with_pages(support::clocked(&clock))
        .module(EveryoneTwoFactorModule)
        .build()
        .await;
    create_user(&app, "anu@example.com").await;
    let browser = app.client();

    let response = post_form(
        &browser,
        "/sign-in",
        &[
            ("email", "anu@example.com"),
            ("password", PASSWORD),
            ("next", "/deals/42"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/account/two-factor?next=%2Fdeals%2F42")
    );
    whoami(&browser)
        .await
        .assert_error_code(403, "two_factor_required");

    let page = open(&browser, "/account/two-factor?next=%2Fdeals%2F42")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Set it up"), "{page}");
    assert!(page.contains(r#"action="/account/two-factor""#), "{page}");

    // The secret is made when the person asks for it.
    let setup = post_form(&browser, "/account/two-factor", &[("next", "/deals/42")])
        .await
        .assert_status(200)
        .text();
    assert!(setup.contains("<svg"), "the QR code: {setup}");
    assert!(
        setup.contains(r#"action="/account/two-factor/confirm""#),
        "{setup}"
    );
    let secret = first_code(&setup);

    // A wrong code starts again with a new key.
    let wrong = post_form(
        &browser,
        "/account/two-factor/confirm",
        &[("code", "000000"), ("next", "/deals/42")],
    )
    .await
    .assert_status(422)
    .text();
    assert!(wrong.contains("That code didn&#39;t work."), "{wrong}");
    let secret_again = first_code(&wrong);
    assert_ne!(secret, secret_again, "a new key");
    whoami(&browser)
        .await
        .assert_error_code(403, "two_factor_required");

    let done = post_form(
        &browser,
        "/account/two-factor/confirm",
        &[
            ("code", totp_code(&secret_again, &clock).as_str()),
            ("next", "/deals/42"),
        ],
    )
    .await
    .assert_status(200)
    .text();
    assert!(done.contains("Save your recovery codes"), "{done}");
    assert!(done.contains(r#"href="/deals/42""#), "{done}");
    assert!(first_code(&done).contains('-'), "a recovery code: {done}");
    whoami(&browser).await.assert_status(200);

    // It is on now: the page says so, and the next sign-in asks for a code.
    let page = open(&browser, "/account/two-factor")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Two-factor authentication is on."), "{page}");
}

// A guest cannot reach the setup page, and posting it needs the token.
#[tokio::test]
async fn the_setup_page_needs_a_sign_in_and_the_csrf_token() {
    let app = with_pages(support::builder()).build().await;
    let guest = app.client();
    open(&guest, "/account/two-factor").await.assert_status(401);
    // The page gave the guest a session and its token; a post without the
    // token is refused before anything else.
    token(&guest).await;
    guest
        .post("/account/two-factor")
        .header("accept", "text/html")
        .form(&[("next", "/")])
        .send()
        .await
        .assert_status(403);
}

/// An app with magic links and emailed codes on, mail kept in memory.
async fn passwordless_app(clock: &Arc<FixedClock>, sink: Arc<RecordingSink>) -> TestApp {
    with_pages(
        support::mail_builder()
            .override_value::<dyn navree_contracts::Clock>(clock.clone())
            .config("auth.magic_link.enabled", true)
            .config(
                "auth.magic_link.url",
                "https://app.example.com/sign-in/link/verify",
            )
            .config("auth.otp.enabled", true)
            .override_value::<dyn AuditSink>(sink),
    )
    .build()
    .await
}

// The sign-in page offers the other ways in only when they are on.
#[tokio::test]
async fn the_sign_in_page_links_only_the_ways_that_are_on() {
    let plain = with_pages(support::builder()).build().await;
    let page = open(&plain.client(), "/sign-in")
        .await
        .assert_status(200)
        .text();
    assert!(!page.contains("/sign-in/link"), "{page}");
    assert!(!page.contains("/sign-in/code"), "{page}");
    assert!(!page.contains("/sign-in/passkey"), "{page}");
    open(&plain.client(), "/sign-in/link")
        .await
        .assert_status(404);
    open(&plain.client(), "/sign-in/code")
        .await
        .assert_status(404);
    open(&plain.client(), "/sign-in/passkey")
        .await
        .assert_status(404);

    let clock = support::clock();
    let app = passwordless_app(&clock, Arc::new(RecordingSink::default())).await;
    let page = open(&app.client(), "/sign-in?next=%2Fdeals")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("/sign-in/link?next=%2Fdeals"), "{page}");
    assert!(page.contains("/sign-in/code?next=%2Fdeals"), "{page}");
}

// A mailed link opens a page with one button; only the button
// signs in, once; a used link is told so.
#[tokio::test]
async fn a_mailed_link_signs_a_browser_in_once_and_only_by_its_button() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = passwordless_app(&clock, sink.clone()).await;
    let user = create_user(&app, "anu@example.com").await;
    let browser = app.client();

    let page = open(&browser, "/sign-in/link")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains(r#"name="email""#), "{page}");
    let sent = post_form(
        &browser,
        "/sign-in/link",
        &[("email", "anu@example.com"), ("next", "/deals/42")],
    )
    .await
    .assert_status(200)
    .text();
    assert!(sent.contains("we have sent it a link"), "{sent}");
    // An address nobody has gets the same answer and no mail.
    let unknown = post_form(
        &app.client(),
        "/sign-in/link",
        &[("email", "nobody@example.com")],
    )
    .await
    .assert_status(200)
    .text();
    assert!(unknown.contains("we have sent it a link"), "{unknown}");
    let mails = sent_mail(&app).await;
    assert_eq!(mails.len(), 1);
    let token = link_param(mails[0].text.as_deref().expect("a text body"), "token")
        .expect("the link carries its token");

    // Opening the link signs nobody in: a mail scanner may open it first.
    let confirm = open(
        &browser,
        &format!("/sign-in/link/verify?token={token}&next=%2Fdeals%2F42"),
    )
    .await
    .assert_status(200)
    .text();
    assert!(confirm.contains(r#"name="token""#), "{confirm}");
    whoami(&browser).await.assert_status(401);

    let response = post_form(
        &browser,
        "/sign-in/link/verify",
        &[("token", token.as_str()), ("next", "/deals/42")],
    )
    .await
    .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals/42"));
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"))
        .assert_json_path("user_id", json!(user.id));
    let logins = sink.with_action("auth.login");
    let last = logins.last().expect("an auth.login entry");
    assert_eq!(last.metadata["via"], "session");
    assert_eq!(last.metadata["method"], "magic_link");

    // The link worked once.
    let other = app.client();
    let again = post_form(&other, "/sign-in/link/verify", &[("token", token.as_str())])
        .await
        .assert_status(400)
        .text();
    assert!(again.contains("expired or was used already"), "{again}");
    whoami(&other).await.assert_status(401);
}

// A mailed code is asked for on the next page, a wrong one is a
// field error, and the right one signs in.
#[tokio::test]
async fn an_emailed_code_signs_a_browser_in() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = passwordless_app(&clock, sink.clone()).await;
    let user = create_user(&app, "anu@example.com").await;
    let browser = app.client();

    let asked = post_form(
        &browser,
        "/sign-in/code",
        &[("email", "anu@example.com"), ("next", "/deals/42")],
    )
    .await
    .assert_status(200)
    .text();
    assert!(
        asked.contains(r#"action="/sign-in/code/verify""#),
        "{asked}"
    );
    assert!(asked.contains(r#"name="code""#), "{asked}");
    let mails = sent_mail(&app).await;
    assert_eq!(mails.len(), 1);
    let text = mails[0].text.clone().expect("a text body");
    let start = text.find("code: ").expect("a code") + "code: ".len();
    let code: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();

    let wrong = post_form(
        &browser,
        "/sign-in/code/verify",
        &[("email", "anu@example.com"), ("code", "000000")],
    )
    .await
    .assert_status(422)
    .text();
    assert!(wrong.contains("That code didn&#39;t work."), "{wrong}");
    whoami(&browser).await.assert_status(401);

    let response = post_form(
        &browser,
        "/sign-in/code/verify",
        &[
            ("email", "anu@example.com"),
            ("code", code.as_str()),
            ("remember", "true"),
            ("next", "/deals/42"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals/42"));
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id));
    let logins = sink.with_action("auth.login");
    let last = logins.last().expect("an auth.login entry");
    assert_eq!(last.metadata["method"], "otp");
    assert_eq!(last.metadata["remember"], true);
}

// A mailed link or code for an account with two-factor
// authentication continues at the second factor, not in a session.
#[tokio::test]
async fn the_passwordless_pages_ask_for_the_second_factor() {
    let clock = support::clock();
    let app = passwordless_app(&clock, Arc::new(RecordingSink::default())).await;
    let user = create_user(&app, "anu@example.com").await;
    // A verified owner's second factor: an unverified account's is reset when its
    // address is first proven.
    app.get::<navree_auth::AuthService>()
        .await
        .verify_email(
            user.id,
            &navree_crypto::sha256_hex(user.email.as_bytes()),
            &navree_auth::ClientMeta::default(),
        )
        .await
        .unwrap();
    let secret = enable_two_factor(&app, &clock, &user).await;
    let browser = app.client();

    post_form(&browser, "/sign-in/link", &[("email", "anu@example.com")])
        .await
        .assert_status(200);
    let mails = sent_mail(&app).await;
    let token = link_param(mails[0].text.as_deref().expect("a text body"), "token").unwrap();
    let response = post_form(
        &browser,
        "/sign-in/link/verify",
        &[("token", token.as_str()), ("next", "/deals/42")],
    )
    .await
    .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/sign-in/two-factor?next=%2Fdeals%2F42")
    );
    whoami(&browser).await.assert_status(401);

    let response = post_form(
        &browser,
        "/sign-in/two-factor",
        &[
            ("code", totp_code(&secret, &clock).as_str()),
            ("next", "/deals/42"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals/42"));
    whoami(&browser).await.assert_status(200);
}

/// An app with passkeys on.
async fn passkey_app(sink: Arc<RecordingSink>) -> TestApp {
    let clock = support::clock();
    with_pages(
        support::clocked(&clock)
            .config("auth.passkeys.enabled", true)
            .config("auth.passkeys.rp_id", RP_ID)
            .config("auth.passkeys.origins", json!([ORIGIN]))
            .override_value::<dyn AuditSink>(sink),
    )
    .build()
    .await
}

/// A JSON post of a browser that holds the CSRF token.
async fn post_json(client: &TestClient, path: &str, body: Value) -> TestResponse {
    token(client).await;
    client.post(path).xsrf().json(&body).send().await
}

// The passkey page ships its button and its script, and the two
// JSON routes sign a browser in with a registered passkey.
#[tokio::test]
async fn a_passkey_signs_a_browser_in_through_the_page_routes() {
    let sink = Arc::new(RecordingSink::default());
    let app = passkey_app(sink.clone()).await;
    let (user, access, _) = signed_in(&app, "anu@example.com").await;
    let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN);

    // The passkey is registered through the token API, as an app's
    // settings page would.
    let options = app
        .client()
        .post("/api/auth/passkeys/register/options")
        .bearer(&access)
        .json(&json!({}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let credential = authenticator.create(&options["publicKey"]);
    app.client()
        .post("/api/auth/passkeys/register")
        .bearer(&access)
        .json(&json!({ "name": "Work laptop", "credential": credential }))
        .send()
        .await
        .assert_status(201);

    let browser = app.client();
    let page = open(&browser, "/sign-in?next=%2Fdeals")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("/sign-in/passkey?next=%2Fdeals"), "{page}");
    assert!(page.contains("data-ui-noboost"), "{page}");

    let page = open(&browser, "/sign-in/passkey?next=%2Fdeals")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains(r#"id="passkey-sign-in""#), "{page}");
    assert!(page.contains(r#"data-next="/deals""#), "{page}");
    assert!(
        page.contains(r#"<script src="/sign-in/passkey.js" defer></script>"#),
        "{page}"
    );
    let script = open(&browser, "/sign-in/passkey.js")
        .await
        .assert_status(200);
    assert_eq!(
        script.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert!(script.text().contains("credentials.get"));

    let options = post_json(&browser, "/sign-in/passkey/options", json!({}))
        .await
        .assert_status(200)
        .json_value();
    let assertion = authenticator
        .get(&options["publicKey"])
        .expect("the passkey answers");
    whoami(&browser).await.assert_status(401);

    // A post without the token is refused.
    browser
        .post("/sign-in/passkey/verify")
        .json(&json!({ "credential": assertion }))
        .send()
        .await
        .assert_status(403);

    let done = post_json(
        &browser,
        "/sign-in/passkey/verify",
        json!({ "credential": assertion, "next": "/deals" }),
    )
    .await
    .assert_status(200)
    .json_value();
    assert_eq!(done, json!({ "status": "signed_in", "next": "/deals" }));
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"))
        .assert_json_path("user_id", json!(user.id));
    let logins = sink.with_action("auth.login");
    let last = logins.last().expect("an auth.login entry");
    assert_eq!(last.metadata["method"], "passkey");

    // A next that leaves the site is dropped.
    let other = app.client();
    let options = post_json(&other, "/sign-in/passkey/options", json!({}))
        .await
        .json_value();
    let assertion = authenticator.get(&options["publicKey"]).unwrap();
    let done = post_json(
        &other,
        "/sign-in/passkey/verify",
        json!({ "credential": assertion, "next": "//evil.example" }),
    )
    .await
    .assert_status(200)
    .json_value();
    assert_eq!(done["next"], "/");
}

// A single-page app signs a browser in with JSON on the cookie: no
// token comes back, two-factor authentication works, and the CSRF token is
// required.
#[tokio::test]
async fn a_single_page_app_signs_in_with_json_on_the_cookie() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = with_pages(support::clocked(&clock))
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let user = create_user(&app, "anu@example.com").await;
    let two = create_user(&app, "bal@example.com").await;
    let secret = enable_two_factor(&app, &clock, &two).await;
    let spa = app.client();
    token(&spa).await;

    // No token, no sign-in.
    spa.post("/api/auth/session/login")
        .json(&json!({ "email": "anu@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(403);
    post_json(
        &spa,
        "/api/auth/session/login",
        json!({ "email": "anu@example.com", "password": "wrong password" }),
    )
    .await
    .assert_error_code(401, "invalid_credentials");
    whoami(&spa).await.assert_status(401);

    let body = post_json(
        &spa,
        "/api/auth/session/login",
        json!({ "email": "anu@example.com", "password": PASSWORD, "remember": true }),
    )
    .await
    .assert_status(200)
    .json_value();
    assert_eq!(body["id"], json!(user.id));
    assert!(body.get("access_token").is_none(), "no token: {body}");
    assert!(body.get("refresh_token").is_none(), "no token: {body}");
    whoami(&spa)
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"));
    let logins = sink.with_action("auth.login");
    let last = logins.last().expect("an auth.login entry");
    assert_eq!(last.metadata["via"], "session");
    assert_eq!(last.metadata["remember"], true);

    // Signing out is the logout route.
    spa.post("/api/auth/logout")
        .xsrf()
        .send()
        .await
        .assert_status(204);
    whoami(&spa).await.assert_status(401);

    // Two-factor authentication: the password alone signs nothing in.
    let spa = app.client();
    let body = post_json(
        &spa,
        "/api/auth/session/login",
        json!({ "email": "bal@example.com", "password": PASSWORD }),
    )
    .await
    .assert_status(200)
    .json_value();
    assert_eq!(body, json!({ "two_factor_required": true }));
    whoami(&spa).await.assert_status(401);
    post_json(
        &spa,
        "/api/auth/session/2fa/verify",
        json!({ "code": "000000" }),
    )
    .await
    .assert_status(401);
    let body = post_json(
        &spa,
        "/api/auth/session/2fa/verify",
        json!({ "code": totp_code(&secret, &clock) }),
    )
    .await
    .assert_status(200)
    .json_value();
    assert_eq!(body["id"], json!(two.id));
    whoami(&spa).await.assert_status(200);
}

/// An app where `admin` may impersonate, with the pages.
async fn impersonation_app(sink: Arc<RecordingSink>) -> (TestApp, User, User, TestClient) {
    let clock = support::clock();
    let app = with_pages(
        support::with_access(&clock)
            .config("auth.impersonation.enabled", true)
            .override_value::<dyn AuditSink>(sink),
    )
    .build()
    .await;
    let admin = create_user(&app, "admin@example.com").await;
    grant_role(&app, admin.id, "support", &["users.impersonate"]).await;
    let target = create_user(&app, "anu@example.com").await;
    let browser = app.client();
    post_form(
        &browser,
        "/sign-in",
        &[("email", "admin@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    (app, admin, target, browser)
}

// An administrator impersonates in the browser: the session becomes
// the user's, marked as an impersonation, and a button brings it back.
#[tokio::test]
async fn a_browser_impersonates_a_user_and_goes_back() {
    let sink = Arc::new(RecordingSink::default());
    let (app, admin, target, browser) = impersonation_app(sink.clone()).await;
    let before = browser.cookie("__Host-navree_session").unwrap();

    let started = post_json(
        &browser,
        &format!("/api/auth/impersonate/{}", target.id),
        json!({ "reason": "Ticket 42" }),
    )
    .await
    .assert_status(200)
    .json_value();
    assert_eq!(started["user"]["id"], json!(target.id));
    assert_eq!(started["impersonator_id"], json!(admin.id));
    assert!(started.get("access_token").is_none(), "no token: {started}");
    assert_ne!(
        browser.cookie("__Host-navree_session").unwrap(),
        before,
        "a new session"
    );

    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(target.id));
    browser
        .get("/api/probe/actor")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("impersonating", json!(true))
        .assert_json_path("impersonator_id", json!(admin.id));
    // The account-security routes and the recent-sign-in routes refuse it.
    browser
        .post("/api/probe/no-impersonation")
        .xsrf()
        .send()
        .await
        .assert_error_code(403, "impersonation_forbidden");
    browser
        .post("/api/probe/sensitive")
        .xsrf()
        .send()
        .await
        .assert_status(403);
    // It cannot start a second one.
    post_json(
        &browser,
        &format!("/api/auth/impersonate/{}", admin.id),
        json!({ "reason": "again" }),
    )
    .await
    .assert_status(403);
    let started = sink.with_action("auth.impersonation_started");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].actor_id, Some(admin.id));
    assert_eq!(started[0].impersonator_id, Some(admin.id));

    // Back to the administrator's own sign-in.
    let response = post_form(&browser, "/impersonate/stop", &[])
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/"));
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(admin.id));
    browser
        .get("/api/probe/actor")
        .send()
        .await
        .assert_json_path("impersonating", json!(false));
    assert_eq!(sink.with_action("auth.impersonation_ended").len(), 1);

    // Only an impersonation can be stopped.
    post_form(&browser, "/impersonate/stop", &[])
        .await
        .assert_status(400);
    let _ = app;
}

// Signing out while impersonating ends the administrator's own
// sign-in as well.
#[tokio::test]
async fn signing_out_while_impersonating_ends_both_sign_ins() {
    let sink = Arc::new(RecordingSink::default());
    let (app, admin, target, browser) = impersonation_app(sink).await;
    post_json(
        &browser,
        &format!("/api/auth/impersonate/{}", target.id),
        json!({ "reason": "Ticket 42" }),
    )
    .await
    .assert_status(200);
    post_form(&browser, "/sign-out", &[])
        .await
        .assert_status(303);
    whoami(&browser).await.assert_status(401);
    let service = app.get::<navree_auth::AuthService>().await;
    let live = service.sessions(admin.id, None).await.unwrap();
    assert!(
        live.is_empty(),
        "the administrator's sign-in ended too: {live:?}"
    );
}

// An app without navree-session's layer has no routes that sign a browser in
// on the session: the JSON routes are not there to fail.
#[tokio::test]
async fn the_json_session_routes_exist_only_with_the_session_layer() {
    let app = support::app().await;
    app.client()
        .post("/api/auth/session/login")
        .json(&json!({ "email": "anu@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(404);
    // The token routes are as they were.
    create_user(&app, "anu@example.com").await;
    support::login(&app.client(), "anu@example.com", PASSWORD)
        .await
        .assert_status(200);
}

// `navree check` asks every route for its API documentation: so is every
// route of the pages, the passwordless and passkey ones too.
#[tokio::test]
async fn every_route_of_the_pages_is_documented() {
    let clock = support::clock();
    let app = with_pages(
        support::mail_builder()
            .override_value::<dyn navree_contracts::Clock>(clock)
            .config("auth.magic_link.enabled", true)
            .config(
                "auth.magic_link.url",
                "https://app.example.com/sign-in/link/verify",
            )
            .config("auth.otp.enabled", true)
            .config("auth.passkeys.enabled", true)
            .config("auth.passkeys.rp_id", RP_ID)
            .config("auth.passkeys.origins", json!([ORIGIN])),
    )
    .build()
    .await;
    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let mut pages = 0;
    for route in routes.iter().filter(|route| {
        route
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("auth.pages."))
    }) {
        pages += 1;
        let label = format!("{} {}", route.method, route.full_path);
        let api = route.api.as_ref().unwrap_or_else(|| panic!("{label}"));
        assert!(api.summary.is_some(), "{label}");
        assert_eq!(api.tags, ["Auth pages"], "{label}");
    }
    assert_eq!(pages, 24);
}
