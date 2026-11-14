//! Browser sign-in on the session cookie: `AuthPages` and the
//! session strategy, through the full router with navree-session's layer,
//! as a browser without JavaScript drives them (plain form posts carrying
//! `_csrf`), and the JSON routes a signed-in browser calls with
//! `X-XSRF-TOKEN`.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree_auth::{
    AuthError, AuthPages, AuthService, ModuleBuilderAuthExt, SignInAttempt, SignInPolicy,
    TwoFactorPolicy, User,
};
use navree_contracts::{AuditSink, BoxError, BoxFuture, Broadcaster, FixedClock};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_ui::UiModule;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{
    self, PASSWORD, RecordingSink, create_user, enable_two_factor, first_url, link_param, login,
    refresh, sent_mail, tokens, totp_code,
};

/// The session cookie under the default options (secure, no domain).
const COOKIE: &str = "__Host-navree_session";

/// The cookie the CSRF token is read from.
const XSRF: &str = "XSRF-TOKEN";

/// `builder` with the session layer, navree-ui and the sign-in pages.
fn with_pages(builder: TestAppBuilder) -> TestAppBuilder {
    builder
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
}

async fn app() -> TestApp {
    with_pages(support::builder()).build().await
}

/// An app with `clock` and the audit trail recorded in `sink`.
async fn audited(clock: &Arc<FixedClock>, sink: &Arc<RecordingSink>) -> TestApp {
    with_pages(support::clocked(clock))
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await
}

/// The session's CSRF token, from the cookie the pages hand out (opening
/// the sign-in page first when the client has none).
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

/// Posts the sign-in form as a browser without JavaScript does: an unchecked
/// "remember me" box is left out of the post.
async fn sign_in(client: &TestClient, email: &str, password: &str, remember: bool) -> TestResponse {
    let csrf = token(client).await;
    let mut fields = vec![("email", email), ("password", password)];
    if remember {
        fields.push(("remember", "true"));
    }
    fields.extend([("next", "/deals/42"), ("_csrf", csrf.as_str())]);
    client
        .post("/sign-in")
        .header("accept", "text/html")
        .form(&fields)
        .send()
        .await
}

/// Posts the sign-out button.
async fn sign_out(client: &TestClient) -> TestResponse {
    let csrf = token(client).await;
    client
        .post("/sign-out")
        .header("accept", "text/html")
        .form(&[("_csrf", csrf.as_str())])
        .send()
        .await
}

/// Posts the two-factor form.
async fn second_factor(client: &TestClient, field: &str, code: &str) -> TestResponse {
    let csrf = token(client).await;
    client
        .post("/sign-in/two-factor")
        .header("accept", "text/html")
        .form(&[
            (field, code),
            ("next", "/deals/42"),
            ("_csrf", csrf.as_str()),
        ])
        .send()
        .await
}

/// `GET /api/probe/whoami`: the status, and how the request is signed in.
async fn whoami(client: &TestClient) -> TestResponse {
    client.get("/api/probe/whoami").send().await
}

/// A client signed in as a new user `email` through the sign-in page.
async fn signed_in_browser(app: &TestApp, email: &str) -> (User, TestClient) {
    let user = create_user(app, email).await;
    let client = app.client();
    sign_in(&client, email, PASSWORD, false)
        .await
        .assert_status(303);
    (user, client)
}

// Signing in gives the session a new id and a new CSRF token
// (session fixation), and the id the browser had before signs nobody in.
#[tokio::test]
async fn signing_in_regenerates_the_session_against_fixation() {
    let app = app().await;
    create_user(&app, "anu@example.com").await;
    let client = app.client();
    let before_token = token(&client).await;
    let before = client
        .cookie(COOKIE)
        .expect("the sign-in page starts a session");

    let response = sign_in(&client, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals/42"));
    let after = client.cookie(COOKIE).expect("a session cookie");
    assert_ne!(after, before, "a new session id");
    assert_ne!(
        client.cookie(XSRF).unwrap(),
        before_token,
        "a new CSRF token"
    );
    whoami(&client)
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"));

    // Someone who planted or kept the old id is no one.
    let planted = app.client();
    planted.set_cookie(COOKIE, &before);
    whoami(&planted).await.assert_status(401);
}

// The cookie is HttpOnly, Secure, SameSite=Lax, for the whole site,
// with the `__Host-` prefix and no Domain; "remember me" makes it last.
#[tokio::test]
async fn the_session_cookie_is_httponly_secure_lax_and_host_bound() {
    use navree_testing::cookie::SameSite;
    use navree_testing::cookie::time::Duration as CookieDuration;

    let app = app().await;
    create_user(&app, "anu@example.com").await;
    let response = sign_in(&app.client(), "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    let cookie = response.set_cookie(COOKIE).expect("the session cookie");
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.domain(), None);
    assert_eq!(cookie.max_age(), Some(CookieDuration::minutes(120)));
    assert!(response.set_cookie("navree_session").is_none());

    let remembered = sign_in(&app.client(), "anu@example.com", PASSWORD, true)
        .await
        .assert_status(303);
    let cookie = remembered.set_cookie(COOKIE).expect("the session cookie");
    assert_eq!(cookie.max_age(), Some(CookieDuration::days(30)));
    assert_eq!(
        remembered.set_cookie(XSRF).unwrap().max_age(),
        Some(CookieDuration::days(30))
    );
}

// One guard for both: `.authenticated()`, `CurrentUser`,
// `#[recent_auth]` (with `POST /auth/confirm` for a browser),
// `platform_session` and the sessions list hold the same for a cookie as
// for a token.
#[tokio::test]
async fn the_same_guards_hold_for_bearer_tokens_and_the_cookie() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&clock, &sink).await;
    let (user, browser) = signed_in_browser(&app, "anu@example.com").await;

    let api = app.client();
    let (access, _) = tokens(
        &login(&api, "anu@example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    api.get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Token"));
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id))
        .assert_json_path("via", json!("Session"));
    browser
        .get("/api/probe/user")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("anu@example.com"));
    // A bearer token wins over a cookie on the same request.
    browser
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_json_path("via", json!("Token"));

    // Recent sign-in: fine now, not eleven minutes later, fine again after
    // confirming the password; never a token for the browser.
    let sensitive = |client: &TestClient| {
        let request = client.post("/api/probe/sensitive").xsrf();
        async move { request.send().await }
    };
    sensitive(&browser).await.assert_status(200);
    clock.advance(Duration::from_secs(11 * 60));
    sensitive(&browser)
        .await
        .assert_error_code(403, "reauthentication_required");
    browser
        .post("/api/auth/confirm")
        .xsrf()
        .json(&json!({ "password": "wrong password" }))
        .send()
        .await
        .assert_status(400);
    let confirmed = browser
        .post("/api/auth/confirm")
        .xsrf()
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .assert_status(204);
    assert!(confirmed.text().is_empty(), "no token for a browser");
    sensitive(&browser).await.assert_status(200);
    assert_eq!(sink.with_action("auth.confirmed").len(), 1);

    // A browser session is never a platform session.
    browser
        .get("/api/probe/platform")
        .send()
        .await
        .assert_error_code(403, "platform_session_required");

    // The sessions list shows the browser's sign-in, marked as current.
    let listed = browser
        .get("/api/auth/sessions")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let current: Vec<&Value> = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|session| session["current"] == json!(true))
        .collect();
    assert_eq!(current.len(), 1, "{listed}");
}

// Signing out ends the sign-in on the server; the cookie, or a
// copy of it, signs nobody in afterwards.
#[tokio::test]
async fn signing_out_ends_the_sign_in_on_the_server() {
    let app = app().await;
    let (user, browser) = signed_in_browser(&app, "anu@example.com").await;
    let copy = app.client();
    copy.set_cookie(COOKIE, &browser.cookie(COOKIE).unwrap());
    whoami(&copy).await.assert_status(200);

    let response = sign_out(&browser).await.assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/sign-in?status=signed-out")
    );
    whoami(&browser).await.assert_status(401);
    whoami(&copy).await.assert_status(401);
    let service = app.get::<AuthService>().await;
    assert!(
        service.sessions(user.id, None).await.unwrap().is_empty(),
        "the server-side session is revoked"
    );
}

// A copied cookie dies with the sign-in even while the session
// record still exists: every request checks the sign-in on the server.
#[tokio::test]
async fn a_copied_cookie_dies_when_the_sign_in_is_ended_elsewhere() {
    let app = app().await;
    let (user, browser) = signed_in_browser(&app, "anu@example.com").await;
    let thief = app.client();
    thief.set_cookie(COOKIE, &browser.cookie(COOKIE).unwrap());
    whoami(&thief).await.assert_status(200);

    // From another device: list the sessions and end the browser's.
    let api = app.client();
    let (access, _) = tokens(
        &login(&api, "anu@example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    let service = app.get::<AuthService>().await;
    let sessions = service.sessions(user.id, None).await.unwrap();
    assert_eq!(sessions.len(), 2, "the browser's and the token's");
    let token_session = support::claims_of(service.jwt(), &access)
        .sid
        .expect("a session claim");
    let browser_session = sessions
        .iter()
        .map(|session| session.id)
        .find(|id| *id != token_session)
        .expect("the browser's session");
    api.delete(&format!("/api/auth/sessions/{browser_session}"))
        .bearer(&access)
        .send()
        .await
        .assert_status(204);

    whoami(&thief).await.assert_status(401);
    whoami(&browser).await.assert_status(401);
    // The token's session goes on.
    api.get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);
}

// "sign out everywhere" from a token ends the browser's sign-in too.
#[tokio::test]
async fn signing_out_everywhere_ends_the_browser_too() {
    let app = app().await;
    let (_, browser) = signed_in_browser(&app, "anu@example.com").await;
    let api = app.client();
    let (access, _) = tokens(
        &login(&api, "anu@example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    api.post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "all": true }))
        .send()
        .await
        .assert_status(204);
    whoami(&browser).await.assert_status(401);
}

// `POST /api/auth/logout` from a browser signs it out of its
// session, CSRF token required.
#[tokio::test]
async fn the_logout_route_signs_a_browser_out_of_its_session() {
    let app = app().await;
    let (_, browser) = signed_in_browser(&app, "anu@example.com").await;
    browser
        .post("/api/auth/logout")
        .send()
        .await
        .assert_error_code(403, "csrf_token_mismatch");
    whoami(&browser).await.assert_status(200);
    browser
        .post("/api/auth/logout")
        .xsrf()
        .send()
        .await
        .assert_status(204);
    whoami(&browser).await.assert_status(401);
}

// Changing the password from the browser ends every other session
// (other browsers, tokens) and keeps this one, on a new session id.
#[tokio::test]
async fn a_password_change_ends_the_other_sessions_and_keeps_this_one() {
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&support::clock(), &sink).await;
    let (_, browser) = signed_in_browser(&app, "anu@example.com").await;
    let other_browser = app.client();
    sign_in(&other_browser, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    let api = app.client();
    let (_, refresh_token) = tokens(
        &login(&api, "anu@example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    let before = browser.cookie(COOKIE).unwrap();

    browser
        .patch("/api/auth/me/password")
        .xsrf()
        .json(&json!({ "current_password": PASSWORD, "password": "a brand new password" }))
        .send()
        .await
        .assert_status(204);

    whoami(&browser).await.assert_status(200);
    assert_ne!(browser.cookie(COOKIE).unwrap(), before, "a new session id");
    whoami(&other_browser).await.assert_status(401);
    refresh(&api, &refresh_token).await.assert_status(401);
    assert_eq!(sink.with_action("auth.password_changed").len(), 1);
    // The old password no longer signs in; the new one does.
    sign_in(&app.client(), "anu@example.com", PASSWORD, false)
        .await
        .assert_status(422);
    sign_in(
        &app.client(),
        "anu@example.com",
        "a brand new password",
        false,
    )
    .await
    .assert_status(303);
}

// A password reset through the pages ends every session, the
// browser's included, and the new password signs in.
#[tokio::test]
async fn a_password_reset_through_the_pages_ends_every_session() {
    let app = with_pages(support::mail_builder())
        .config(
            "auth.password_reset.url",
            "https://app.example.com/reset-password",
        )
        .build()
        .await;
    let (_, browser) = signed_in_browser(&app, "anu@example.com").await;

    let visitor = app.client();
    let csrf = token(&visitor).await;
    visitor
        .get("/forgot-password")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    let sent = visitor
        .post("/forgot-password")
        .header("accept", "text/html")
        .form(&[("email", "anu@example.com"), ("_csrf", csrf.as_str())])
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(sent.contains("If an account uses that address"), "{sent}");
    // The same answer for an address with no account.
    let unknown = visitor
        .post("/forgot-password")
        .header("accept", "text/html")
        .form(&[("email", "nobody@example.com"), ("_csrf", csrf.as_str())])
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(unknown.contains("If an account uses that address"));

    let mail = sent_mail(&app).await;
    let text = mail
        .last()
        .and_then(|m| m.text.clone())
        .expect("a reset mail");
    let link = first_url(&text).expect("a link");
    assert!(
        link.starts_with("https://app.example.com/reset-password?"),
        "{link}"
    );
    let reset_token = link_param(&link, "token").expect("a token");
    let page = visitor
        .get(&format!(
            "/reset-password?token={reset_token}&email=anu%40example.com"
        ))
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Choose a new password"), "{page}");

    let csrf = token(&visitor).await;
    let mismatch = visitor
        .post("/reset-password")
        .header("accept", "text/html")
        .form(&[
            ("email", "anu@example.com"),
            ("token", reset_token.as_str()),
            ("password", "a brand new password"),
            ("password_confirmation", "another one"),
            ("_csrf", csrf.as_str()),
        ])
        .send()
        .await
        .assert_status(422);
    assert!(mismatch.text().contains("not the same"));
    let done = visitor
        .post("/reset-password")
        .header("accept", "text/html")
        .form(&[
            ("email", "anu@example.com"),
            ("token", reset_token.as_str()),
            ("password", "a brand new password"),
            ("password_confirmation", "a brand new password"),
            ("_csrf", csrf.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
    assert_eq!(
        done.header("location"),
        Some("/sign-in?status=password-reset")
    );
    whoami(&browser).await.assert_status(401);

    // The link works once.
    let csrf = token(&visitor).await;
    let again = visitor
        .post("/reset-password")
        .header("accept", "text/html")
        .form(&[
            ("email", "anu@example.com"),
            ("token", reset_token.as_str()),
            ("password", "yet another password"),
            ("password_confirmation", "yet another password"),
            ("_csrf", csrf.as_str()),
        ])
        .send()
        .await
        .assert_status(400);
    assert!(again.text().contains("expired or was used already"));
    sign_in(
        &app.client(),
        "anu@example.com",
        "a brand new password",
        false,
    )
    .await
    .assert_status(303);
}

// CSRF on sign-in and sign-out: a form with the session cookie
// needs the session's token; a cross-site form without one is refused
// (login CSRF).
#[tokio::test]
async fn sign_in_and_sign_out_need_the_csrf_token() {
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&support::clock(), &sink).await;
    create_user(&app, "anu@example.com").await;
    let client = app.client();
    token(&client).await;

    for csrf in [None, Some("forged-token")] {
        let mut form = vec![("email", "anu@example.com"), ("password", PASSWORD)];
        if let Some(csrf) = csrf {
            form.push(("_csrf", csrf));
        }
        client
            .post("/sign-in")
            .header("accept", "text/html")
            .form(&form)
            .send()
            .await
            .assert_status(403);
    }
    whoami(&client).await.assert_status(401);

    // A form another site posts, with no session cookie to carry a token.
    app.client()
        .post("/sign-in")
        .header("accept", "text/html")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .form(&[("email", "anu@example.com"), ("password", PASSWORD)])
        .send()
        .await
        .assert_status(403);

    sign_in(&client, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    client
        .post("/sign-out")
        .header("accept", "text/html")
        .form(&[("_csrf", "forged-token")])
        .send()
        .await
        .assert_status(403);
    client
        .post("/sign-out")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(403);
    whoami(&client).await.assert_status(200);
    assert!(sink.with_action("csrf.mismatch").len() >= 4);
}

/// Posts the sign-in form with `csrf` in its `_csrf` field, the session's cookie, and
/// neither of the headers a browser says where a post came from with: as the test
/// client does, and a native app's HTTP stack.
async fn sign_in_without_origin_headers(client: &TestClient, csrf: &str) -> TestResponse {
    client
        .post("/sign-in")
        .header("accept", "text/html")
        .without_header("origin")
        .without_header("sec-fetch-site")
        .form(&[
            ("email", "anu@example.com"),
            ("password", PASSWORD),
            ("next", "/deals/42"),
            ("_csrf", csrf),
        ])
        .send()
        .await
}

// A form post that carries the session's cookie and says nothing of where it came from
// is taken by the form when the session's token check vouched for it: the token is what
// only a page of this site holds. A wrong token is refused by that check.
#[tokio::test]
async fn a_form_post_with_the_sessions_token_needs_no_origin_header() {
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&support::clock(), &sink).await;
    create_user(&app, "anu@example.com").await;
    let client = app.client();
    let page = client
        .get("/sign-in")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    let marker = r#"name="_csrf" value=""#;
    let start = page.find(marker).expect("the token field") + marker.len();
    let masked = page[start..].split('"').next();
    let masked = masked.unwrap_or_default().to_owned();
    assert!(client.cookie(COOKIE).is_some(), "the page started a session");

    let refused = sink.with_action("csrf.mismatch").len();
    sign_in_without_origin_headers(&client, "forged-token")
        .await
        .assert_status(403);
    assert_eq!(
        sink.with_action("csrf.mismatch").len(),
        refused + 1,
        "the token check refused it"
    );
    whoami(&client).await.assert_status(401);

    // The token the page carries, masked: taken.
    let response = sign_in_without_origin_headers(&client, &masked)
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals/42"));
    whoami(&client).await.assert_status(200);
    // And the cookie's value, as a script sends it, on the new session.
    let csrf = token(&client).await;
    client
        .post("/sign-out")
        .header("accept", "text/html")
        .without_header("origin")
        .form(&[("_csrf", csrf.as_str())])
        .send()
        .await
        .assert_status(303);
    whoami(&client).await.assert_status(401);
}

// Two-factor authentication on cookies: the password alone signs
// nothing in; a wrong code is refused and audited; the right one signs in
// once; a recovery code works once.
#[tokio::test]
async fn two_factor_sign_in_on_the_cookie() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&clock, &sink).await;
    let user = create_user(&app, "anu@example.com").await;
    let secret = enable_two_factor(&app, &clock, &user).await;
    let client = app.client();
    let before = {
        token(&client).await;
        client.cookie(COOKIE).unwrap()
    };

    let response = sign_in(&client, "anu@example.com", PASSWORD, true)
        .await
        .assert_status(303);
    assert_eq!(
        response.header("location"),
        Some("/sign-in/two-factor?next=%2Fdeals%2F42")
    );
    assert_ne!(client.cookie(COOKIE).unwrap(), before, "regenerated");
    whoami(&client).await.assert_status(401);
    assert_eq!(sink.with_action("auth.two_factor_challenge").len(), 1);
    let page = client
        .get("/sign-in/two-factor?next=%2Fdeals%2F42")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("authenticator app"), "{page}");

    second_factor(&client, "code", "000000")
        .await
        .assert_status(422);
    whoami(&client).await.assert_status(401);
    assert_eq!(sink.with_action("auth.two_factor_failed").len(), 1);

    let response = second_factor(&client, "code", &totp_code(&secret, &clock))
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals/42"));
    whoami(&client)
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"));
    let logins = sink.with_action("auth.login");
    let last = logins.last().expect("an auth.login entry");
    assert_eq!(last.actor_id, Some(user.id));
    assert_eq!(last.metadata["via"], "session");
    assert_eq!(last.metadata["two_factor"], "totp");
    assert_eq!(last.metadata["remember"], true);

    // The challenge is spent: the page sends the browser back to sign in.
    let response = second_factor(&client, "code", &totp_code(&secret, &clock))
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/sign-in?status=expired"));

    // A recovery code, once.
    let codes = app
        .get::<AuthService>()
        .await
        .regenerate_recovery_codes(user.id, &navree_auth::ClientMeta::default())
        .await
        .unwrap();
    let other = app.client();
    sign_in(&other, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    second_factor(&other, "recovery_code", &codes[0])
        .await
        .assert_status(303);
    whoami(&other).await.assert_status(200);
    assert_eq!(
        sink.with_action("auth.two_factor_recovery_code_used").len(),
        1
    );
    let third = app.client();
    sign_in(&third, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    second_factor(&third, "recovery_code", &codes[0])
        .await
        .assert_status(422);
    whoami(&third).await.assert_status(401);
}

// Wrong codes on the cookie are throttled per user like the
// token's, and the throttle is audited once.
#[tokio::test]
async fn two_factor_codes_on_the_cookie_are_throttled() {
    let clock = support::clock();
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&clock, &sink).await;
    let user = create_user(&app, "anu@example.com").await;
    let secret = enable_two_factor(&app, &clock, &user).await;
    let client = app.client();
    sign_in(&client, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    for _ in 0..5 {
        second_factor(&client, "code", "000000")
            .await
            .assert_status(422);
    }
    let throttled = second_factor(&client, "code", &totp_code(&secret, &clock))
        .await
        .assert_status(429);
    assert!(throttled.header("retry-after").is_some());
    assert!(throttled.text().contains("Too many attempts"));
    whoami(&client).await.assert_status(401);
    assert_eq!(sink.with_action("auth.throttled").len(), 1);
}

// Failed sign-ins are counted per email and address, refused with
// 429 past the limit (the right password too), and audited.
#[tokio::test]
async fn failed_browser_sign_ins_are_throttled_and_audited() {
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&support::clock(), &sink).await;
    create_user(&app, "anu@example.com").await;
    let client = app.client();
    for _ in 0..5 {
        let refused = sign_in(&client, "anu@example.com", "wrong password", false)
            .await
            .assert_status(422)
            .text();
        assert!(refused.contains("match an account"), "{refused}");
        assert!(
            !refused.contains("wrong password"),
            "the password is never echoed"
        );
    }
    let throttled = sign_in(&client, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(429);
    assert!(throttled.header("retry-after").is_some());
    whoami(&client).await.assert_status(401);
    let failed = sink.with_action("auth.login_failed");
    assert_eq!(failed.len(), 5);
    assert_eq!(failed[0].metadata["email"], "anu@example.com");
    assert_eq!(sink.with_action("auth.throttled").len(), 1);
    assert!(sink.with_action("auth.login").is_empty());
}

// The audit trail names the browser sign-in and sign-out.
#[tokio::test]
async fn sign_in_and_sign_out_are_audited_as_the_session_s() {
    let sink = Arc::new(RecordingSink::default());
    let app = audited(&support::clock(), &sink).await;
    let (user, browser) = signed_in_browser(&app, "anu@example.com").await;
    let login = sink.with_action("auth.login");
    assert_eq!(login.len(), 1);
    assert_eq!(login[0].actor_id, Some(user.id));
    assert_eq!(login[0].metadata["via"], "session");
    assert_eq!(login[0].metadata["remember"], false);
    assert!(login[0].ip.is_some(), "with the client's address");

    sign_out(&browser).await.assert_status(303);
    let logout = sink.with_action("auth.logout");
    assert_eq!(logout.len(), 1);
    assert_eq!(logout[0].actor_id, Some(user.id));
    assert_eq!(logout[0].metadata["via"], "session");
    assert_eq!(logout[0].metadata["revoked"], 1);
}

// "remember me" keeps a browser signed in past the idle lifetime,
// up to its days; without it the sign-in ends when the session idles out,
// and at the latest after `auth.session.lifetime_hours`.
#[tokio::test]
async fn remember_me_and_the_sign_in_lifetimes() {
    let clock = support::clock();
    let app = with_pages(support::clocked(&clock)).build().await;
    create_user(&app, "anu@example.com").await;
    let remembered = app.client();
    sign_in(&remembered, "anu@example.com", PASSWORD, true)
        .await
        .assert_status(303);
    let plain = app.client();
    sign_in(&plain, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);

    clock.advance(Duration::from_secs(3 * 3600));
    whoami(&remembered).await.assert_status(200);
    whoami(&plain).await.assert_status(401);

    // A busy browser without "remember me": signed out after 24 hours.
    let busy = app.client();
    sign_in(&busy, "anu@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    for _ in 0..23 {
        clock.advance(Duration::from_secs(3600));
        whoami(&busy).await.assert_status(200);
    }
    clock.advance(Duration::from_secs(3600 + 60));
    whoami(&busy).await.assert_status(401);

    // Remembered: 30 days after the sign-in, however active.
    clock.advance(Duration::from_secs(27 * 24 * 3600));
    whoami(&remembered).await.assert_status(200);
    clock.advance(Duration::from_secs(2 * 24 * 3600));
    whoami(&remembered).await.assert_status(401);
}

/// Refuses passwords for `@sso.example.com`.
struct SsoOnly;

impl Injectable for SsoOnly {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(SsoOnly) })
    }
}

impl SignInPolicy for SsoOnly {
    fn check<'a>(&'a self, attempt: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            if attempt.domain() == Some("sso.example.com") {
                return Err(AuthError::SignInRefused {
                    code: "sso_required".to_owned(),
                    message: "Sign in with your company's single sign-on.".to_owned(),
                });
            }
            Ok(())
        })
    }
}

struct SsoOnlyModule;

impl Module for SsoOnlyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.sign_in_policy::<SsoOnly>();
    }
}

// Sign-in policies hold for browsers, and a refusal is audited.
#[tokio::test]
async fn sign_in_policies_hold_for_browsers() {
    let sink = Arc::new(RecordingSink::default());
    let app = with_pages(support::builder())
        .module(SsoOnlyModule)
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    create_user(&app, "anu@sso.example.com").await;
    let client = app.client();
    let refused = sign_in(&client, "anu@sso.example.com", PASSWORD, false)
        .await
        .assert_status(403)
        .text();
    assert!(refused.contains("single sign-on"), "{refused}");
    whoami(&client).await.assert_status(401);
    let entries = sink.with_action("auth.login_refused");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].metadata["code"], "sso_required");
}

/// Requires two-factor authentication of everyone.
struct EveryoneTwoFactor;

impl Injectable for EveryoneTwoFactor {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(EveryoneTwoFactor) })
    }
}

impl TwoFactorPolicy for EveryoneTwoFactor {
    fn requires_two_factor<'a>(&'a self, _user: &'a User) -> BoxFuture<'a, Result<bool, BoxError>> {
        Box::pin(async { Ok(true) })
    }
}

pub(crate) struct EveryoneTwoFactorModule;

impl Module for EveryoneTwoFactorModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.two_factor_policy::<EveryoneTwoFactor>();
    }
}

// A user who must set up two-factor authentication reaches the
// setup routes and signing out only, on the cookie as with a token; once set
// up, everything.
#[tokio::test]
async fn a_browser_that_must_set_up_two_factor_reaches_the_setup_only() {
    let clock = support::clock();
    let app = with_pages(support::clocked(&clock))
        .module(EveryoneTwoFactorModule)
        .build()
        .await;
    let (_, browser) = signed_in_browser(&app, "anu@example.com").await;
    whoami(&browser)
        .await
        .assert_error_code(403, "two_factor_required");
    let setup = browser
        .post("/api/auth/2fa/enable")
        .xsrf()
        .send()
        .await
        .assert_status(200)
        .json_value();
    let secret = setup["secret"].as_str().unwrap().to_owned();
    browser
        .post("/api/auth/2fa/confirm")
        .xsrf()
        .json(&json!({ "code": totp_code(&secret, &clock) }))
        .send()
        .await
        .assert_status(200);
    whoami(&browser).await.assert_status(200);

    let (_, other) = signed_in_browser(&app, "bal@example.com").await;
    whoami(&other)
        .await
        .assert_error_code(403, "two_factor_required");
    sign_out(&other).await.assert_status(303);
    whoami(&other).await.assert_status(401);
}

/// Records `end_user_subscriptions` calls.
#[derive(Default)]
struct RecordingBroadcaster(Mutex<Vec<Uuid>>);

impl Broadcaster for RecordingBroadcaster {
    fn broadcast(
        &self,
        _channels: Vec<String>,
        _event: String,
        _data: Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async { Ok(()) })
    }

    fn end_user_subscriptions(&self, user_id: Uuid) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(user_id);
        Box::pin(async { Ok(()) })
    }
}

// Live hub: signing a browser out ends the user's real-time
// subscriptions and live streams, so no open page keeps receiving.
#[tokio::test]
async fn signing_a_browser_out_ends_its_live_subscriptions() {
    let broadcaster = Arc::new(RecordingBroadcaster::default());
    let app = with_pages(support::builder())
        .override_value::<dyn Broadcaster>(broadcaster.clone())
        .build()
        .await;
    let (user, browser) = signed_in_browser(&app, "anu@example.com").await;
    assert!(broadcaster.0.lock().unwrap().is_empty());
    sign_out(&browser).await.assert_status(303);
    assert_eq!(*broadcaster.0.lock().unwrap(), [user.id]);
}

// The pages: a signed-in browser skips the sign-in form, a `next`
// off the site is dropped, and the two-factor page without a waiting
// sign-in sends the browser to sign in.
#[tokio::test]
async fn the_pages_keep_people_on_the_site() {
    let app = app().await;
    let guest = app.client();
    let page = guest
        .get("/sign-in?next=%2Fdeals%2F42")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(
        page.contains(r#"name="_csrf""#),
        "the form carries the token"
    );
    assert!(page.contains("/deals/42"), "and the next page");
    let page = guest
        .get("/sign-in?next=https%3A%2F%2Fevil.example")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(!page.contains("evil.example"));
    let response = guest
        .get("/sign-in/two-factor")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/sign-in"));

    create_user(&app, "anu@example.com").await;
    let browser = app.client();
    let csrf = token(&browser).await;
    let response = browser
        .post("/sign-in")
        .header("accept", "text/html")
        .form(&[
            ("email", "anu@example.com"),
            ("password", PASSWORD),
            ("next", "//evil.example/steal"),
            ("_csrf", csrf.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/"));
    let response = browser
        .get("/sign-in?next=%2Fdeals")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(303);
    assert_eq!(response.header("location"), Some("/deals"));
}

// Without SessionModule the strategy stays out of the way.
#[tokio::test]
async fn without_the_session_module_tokens_work_as_before() {
    let app = support::app().await;
    let (_, access, _) = support::signed_in(&app, "anu@example.com").await;
    let client = app.client();
    client
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_status(401);
    client
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Token"));
}

// A browser that signs in again (as someone else, say) ends the
// sign-in it held on the server, so a copy of the old cookie is no one.
#[tokio::test]
async fn signing_in_again_ends_the_sign_in_the_browser_held() {
    let app = app().await;
    let (ada, browser) = signed_in_browser(&app, "anu@example.com").await;
    let copy = app.client();
    copy.set_cookie(COOKIE, &browser.cookie(COOKIE).unwrap());
    whoami(&copy).await.assert_status(200);

    create_user(&app, "gauri@example.com").await;
    sign_in(&browser, "gauri@example.com", PASSWORD, false)
        .await
        .assert_status(303);
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"));
    whoami(&copy).await.assert_status(401);
    let service = app.get::<AuthService>().await;
    assert!(service.sessions(ada.id, None).await.unwrap().is_empty());
}
