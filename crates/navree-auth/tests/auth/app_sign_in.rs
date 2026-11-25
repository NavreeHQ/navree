//! Handing a sign-in made in the system's sign-in session over to a native
//! app: `GET /auth/app/sign-in` with an S256 code challenge, the sign-in
//! pages, `GET /auth/app/finish` to the app's callback with a one-time
//! code, and `POST /api/auth/app/exchange` with the verifier from the app's
//! own HTTP stack; the page that asks a browser that was already signed in
//! first, `POST /auth/app/confirm`. The same for a Windows app, which comes
//! back on a link of its own scheme. Also: a platform sign-in waiting for its
//! second step, finished on the two-factor page.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::Method;
use navree_auth::{
    APP_CODE_TTL_SECS, AuthError, AuthPages, AuthService, ClientMeta, ModuleBuilderAuthExt,
    SignInAttempt, SignInMethod, SignInPolicy, User,
};
use navree_contracts::{AuditSink, Clock as _, FixedClock};
use navree_http::response::Html;
use navree_http::{AppError, Controller, IntoResponse, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{Session, SessionModule, SessionOptions};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_ui::UiModule;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::support::{self, PASSWORD, RecordingSink, create_user, enable_two_factor, totp_code};

const CALLBACK: &str = "https://example.com/app/signed-in";
const BUNDLE: &str = "com.example.orders";

/// A verifier as an app makes one, and its S256 challenge.
fn pkce(seed: &str) -> (String, String) {
    let verifier = format!("{seed}-{}", "v".repeat(48));
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// Signs the session of the request in as a user waiting for the second
/// step of a platform sign-in, as navree-social's native session route
/// does.
struct PlatformController {
    service: Arc<AuthService>,
}

impl Injectable for PlatformController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(PlatformController {
                service: r.get::<AuthService>().await?,
            })
        })
    }
}

impl Controller for PlatformController {
    fn prefix(&self) -> &'static str {
        "/platform-sign-in"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let service = self.service.clone();
        let sign_in = move |session: Session, Json(body): Json<serde_json::Value>| {
            let service = service.clone();
            async move {
                let user = service
                    .find_by_email(body["email"].as_str().unwrap_or_default())
                    .await
                    .map_err(|e| e.into_app_error())?
                    .ok_or_else(AppError::not_found)?;
                let method = SignInMethod::Social {
                    provider: "apple".to_owned(),
                };
                service
                    .await_two_factor_session(&session, &user, method, true, &ClientMeta::default())
                    .await
                    .map_err(|e| e.into_app_error())?;
                Ok::<_, AppError>(Json(json!({ "two_factor_required": true })))
            }
        };
        vec![RouteDef::on(Method::POST, "/start", sign_in).public()]
    }
}

/// Keeps the people of `blocked.test` to the website: their sign-ins are
/// never handed over to an app.
struct WebOnly;

impl Injectable for WebOnly {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(WebOnly) })
    }
}

impl SignInPolicy for WebOnly {
    fn check<'a>(&'a self, attempt: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            if attempt.method.name() == "app_hand_over" && attempt.domain() == Some("blocked.test")
            {
                return Err(AuthError::SignInRefused {
                    code: "web_only".to_owned(),
                    message: "Sign in on the website".to_owned(),
                });
            }
            Ok(())
        })
    }
}

struct PlatformModule;

impl Module for PlatformModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<PlatformController>();
        m.sign_in_policy::<WebOnly>();
    }
}

async fn app(clock: &Arc<FixedClock>, sink: &Arc<RecordingSink>) -> TestApp {
    builder(clock, sink).build().await
}

/// The app of these tests, not booted yet.
fn builder(clock: &Arc<FixedClock>, sink: &Arc<RecordingSink>) -> TestAppBuilder {
    support::clocked(clock)
        .config("auth.apps.apple.orders.team_id", "ABCDE12345")
        .config("auth.apps.apple.orders.bundle_id", BUNDLE)
        .config("auth.apps.apple.orders.callback", CALLBACK)
        // An app of the site without a hand-over callback.
        .config("auth.apps.apple.kiosk.team_id", "ABCDE12345")
        .config("auth.apps.apple.kiosk.bundle_id", "com.example.kiosk")
        .config("auth.apps.android.orders.package", BUNDLE)
        .config(
            "auth.apps.android.orders.fingerprints",
            json!(["14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5"]),
        )
        .config("auth.apps.android.orders.callback", CALLBACK)
        // An older app that can only take a custom scheme.
        .config("auth.apps.android.legacy.package", "com.example.legacy")
        .config(
            "auth.apps.android.legacy.fingerprints",
            json!(["14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5"]),
        )
        .config("auth.apps.android.legacy.callback", "com.example.legacy:/signed-in")
        .config("auth.apps.android.legacy.custom_scheme", true)
        .override_value::<dyn AuditSink>(sink.clone())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
        .module(PlatformModule)
}

/// The browser's CSRF token (opening the sign-in page when it has none).
async fn csrf(client: &TestClient) -> String {
    if client.cookie("XSRF-TOKEN").is_none() {
        client
            .get("/sign-in")
            .header("accept", "text/html")
            .send()
            .await
            .assert_status(200);
    }
    client.cookie("XSRF-TOKEN").expect("the XSRF-TOKEN cookie")
}

fn location(response: &TestResponse) -> String {
    response.header("location").expect("a redirect").to_owned()
}

async fn get(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

/// `GET /auth/app/sign-in` for the Apple app with `challenge`.
async fn start(client: &TestClient, challenge: &str, state: &str) -> TestResponse {
    get(
        client,
        &format!(
            "/auth/app/sign-in?platform=apple&app={BUNDLE}&code_challenge={challenge}\
             &code_challenge_method=S256&state={state}"
        ),
    )
    .await
}

/// The sign-in form, as the system's sign-in session shows it.
async fn sign_in_form(client: &TestClient, email: &str, next: &str) -> TestResponse {
    let token = csrf(client).await;
    client
        .post("/sign-in")
        .header("accept", "text/html")
        .form(&[
            ("email", email),
            ("password", PASSWORD),
            ("next", next),
            ("_csrf", token.as_str()),
        ])
        .send()
        .await
}

/// The person signs in in the sheet: the code and state of the callback.
async fn sheet_sign_in(app: &TestApp, email: &str, challenge: &str) -> (TestClient, String) {
    let sheet = app.client();
    let started = start(&sheet, challenge, "s-1").await.assert_status(303);
    assert_eq!(location(&started), "/auth/app/finish");
    let to_sign_in = get(&sheet, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    let signed_in = sign_in_form(&sheet, email, &back).await.assert_status(303);
    assert_eq!(location(&signed_in), back);
    let code = callback_code(&get(&sheet, &back).await, "s-1");
    (sheet, code)
}

/// The page a browser that was signed in when the hand-over began gets
/// instead of a code: its text, and the one-time value and CSRF token its
/// form posts back.
fn confirmation(response: &TestResponse) -> (String, String, String) {
    assert_eq!(response.status().as_u16(), 200, "{}", response.text());
    assert_eq!(response.header("location"), None);
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(response.header("x-frame-options"), Some("DENY"));
    let page = response.text();
    assert!(!page.contains("code="), "{page}");
    let field = |name: &str| {
        let marker = format!("name=\"{name}\" value=\"");
        let start = page.find(&marker).expect(name) + marker.len();
        page[start..].split('"').next().unwrap().to_owned()
    };
    let (token, csrf) = (field("confirm"), field("_csrf"));
    (page, token, csrf)
}

/// Posts the confirmation page's form with the button `decision`
/// (`continue` or `cancel`).
async fn answer(client: &TestClient, token: &str, csrf: &str, decision: &str) -> TestResponse {
    client
        .post("/auth/app/confirm")
        .header("accept", "text/html")
        .form(&[("_csrf", csrf), ("confirm", token), ("decision", decision)])
        .send()
        .await
}

/// The code of a callback answer, checking where it goes and the state.
fn callback_code(response: &TestResponse, state: &str) -> String {
    assert_eq!(response.status().as_u16(), 303, "{}", response.text());
    let target = url::Url::parse(&location(response)).unwrap();
    assert_eq!(
        format!(
            "{}://{}{}",
            target.scheme(),
            target.host_str().unwrap(),
            target.path()
        ),
        CALLBACK
    );
    let pairs: Vec<(String, String)> = target.query_pairs().into_owned().collect();
    assert!(
        pairs.contains(&("state".to_owned(), state.to_owned())),
        "{pairs:?}"
    );
    pairs
        .into_iter()
        .find(|(key, _)| key == "code")
        .map(|(_, code)| code)
        .expect("a code")
}

/// The app's own HTTP stack.
async fn phone(app: &TestApp) -> TestClient {
    let client = app.client();
    client.csrf_cookie().await;
    client
}

async fn exchange(client: &TestClient, platform: &str, code: &str, verifier: &str) -> TestResponse {
    client
        .post("/api/auth/app/exchange")
        .xsrf()
        .json(&json!({
            "platform": platform,
            "app": BUNDLE,
            "code": code,
            "code_verifier": verifier,
        }))
        .send()
        .await
}

async fn whoami(client: &TestClient) -> TestResponse {
    client.get("/api/probe/user").send().await
}

fn refused(response: TestResponse) {
    response.assert_error_code(400, "invalid_hand_over");
}

/// A refusal as the system's sign-in session shows it: a 400 page, no
/// redirect.
fn refused_page(response: TestResponse) {
    assert_eq!(response.status().as_u16(), 400, "{}", response.text());
    assert_eq!(response.header("location"), None);
    assert!(
        response.text().contains("invalid_hand_over"),
        "{}",
        response.text()
    );
}

#[tokio::test]
async fn a_sign_in_in_the_system_session_is_handed_to_the_app() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let user = create_user(&app, "simran@example.com").await;
    let (verifier, challenge) = pkce("phone");
    let (sheet, code) = sheet_sign_in(&app, "simran@example.com", &challenge).await;

    // The sheet signed in only for the hand-over: it is signed out again.
    whoami(&sheet).await.assert_status(401);

    let phone = phone(&app).await;
    whoami(&phone).await.assert_status(401);
    exchange(&phone, "apple", &code, &verifier)
        .await
        .assert_status(200)
        .assert_json_path("email", json!("simran@example.com"));
    whoami(&phone)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));

    // A session of the app's own: the only live one now.
    let sessions = phone
        .get("/api/auth/sessions")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let live = sessions["data"].as_array().unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0]["current"], json!(true));
    let login = sink.with_action("auth.login");
    let handed = login.last().unwrap();
    assert_eq!(handed.metadata["method"], "app_hand_over");
    assert_eq!(handed.metadata["app"], BUNDLE);
}

#[tokio::test]
async fn two_steps_and_a_browser_signed_in_before_both_hand_over() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let user = create_user(&app, "simran@example.com").await;
    let secret = enable_two_factor(&app, &clock, &user).await;
    let (verifier, challenge) = pkce("two-steps");

    let sheet = app.client();
    start(&sheet, &challenge, "s-2").await.assert_status(303);
    let to_sign_in = get(&sheet, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    let first = sign_in_form(&sheet, "simran@example.com", &back)
        .await
        .assert_status(303);
    let second_step = location(&first);
    assert!(
        second_step.starts_with("/sign-in/two-factor"),
        "{second_step}"
    );
    let token = csrf(&sheet).await;
    let code = totp_code(&secret, &clock);
    let done = sheet
        .post("/sign-in/two-factor")
        .header("accept", "text/html")
        .form(&[
            ("code", code.as_str()),
            ("next", back.as_str()),
            ("_csrf", token.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
    assert_eq!(location(&done), back);
    let code = callback_code(&get(&sheet, &back).await, "s-2");
    let iphone = phone(&app).await;
    exchange(&iphone, "apple", &code, &verifier)
        .await
        .assert_status(200);
    whoami(&iphone).await.assert_status(200);

    // A browser that was signed in before: handed over once the person
    // says so, and still signed in afterwards.
    let other = create_user(&app, "harpreet@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "harpreet@example.com", "/")
        .await
        .assert_status(303);
    let (verifier, challenge) = pkce("browser");
    start(&browser, &challenge, "s-3").await.assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    let code = callback_code(
        &answer(&browser, &token, &csrf_token, "continue").await,
        "s-3",
    );
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(other.id));
    let tablet = phone(&app).await;
    exchange(&tablet, "apple", &code, &verifier)
        .await
        .assert_status(200);
    whoami(&tablet)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(other.id));

    // The Android app the same way.
    let (verifier, challenge) = pkce("android");
    get(
        &browser,
        &format!(
            "/auth/app/sign-in?platform=android&app={BUNDLE}&code_challenge={challenge}\
             &code_challenge_method=S256&state=s-4"
        ),
    )
    .await
    .assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    let code = callback_code(
        &answer(&browser, &token, &csrf_token, "continue").await,
        "s-4",
    );
    let pixel = phone(&app).await;
    exchange(&pixel, "android", &code, &verifier)
        .await
        .assert_status(200);
    whoami(&pixel).await.assert_status(200);
}

#[tokio::test]
async fn a_code_works_once_for_its_app_with_its_verifier_in_time() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let (verifier, challenge) = pkce("once");

    // Used twice.
    let (_, code) = sheet_sign_in(&app, "simran@example.com", &challenge).await;
    let iphone = phone(&app).await;
    exchange(&iphone, "apple", &code, &verifier)
        .await
        .assert_status(200);
    refused(exchange(&iphone, "apple", &code, &verifier).await);

    // From another app (the Android app of the same package).
    let (_, code) = sheet_sign_in(&app, "simran@example.com", &challenge).await;
    refused(exchange(&phone(&app).await, "android", &code, &verifier).await);

    // A wrong verifier, and the code is gone with it.
    let (_, code) = sheet_sign_in(&app, "simran@example.com", &challenge).await;
    let (other_verifier, _) = pkce("other");
    let stranger = phone(&app).await;
    refused(exchange(&stranger, "apple", &code, &other_verifier).await);
    refused(exchange(&stranger, "apple", &code, &verifier).await);
    whoami(&stranger).await.assert_status(401);

    // Expired.
    let (_, code) = sheet_sign_in(&app, "simran@example.com", &challenge).await;
    clock.advance(Duration::from_secs(APP_CODE_TTL_SECS + 1));
    refused(exchange(&stranger, "apple", &code, &verifier).await);

    let reasons: Vec<String> = sink
        .with_action("auth.app_sign_in_refused")
        .iter()
        .map(|entry| entry.metadata["reason"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        reasons,
        [
            "unknown or expired code",
            "code of another app",
            "wrong code verifier",
            "unknown or expired code",
            "code expired",
        ]
    );
}

#[tokio::test]
async fn a_start_without_an_s256_challenge_or_a_callback_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let (_, challenge) = pkce("start");
    let client = app.client();
    for query in [
        // No challenge, a plain one, a short one.
        format!("platform=apple&app={BUNDLE}&state=x"),
        format!(
            "platform=apple&app={BUNDLE}&code_challenge={challenge}&code_challenge_method=plain"
        ),
        format!("platform=apple&app={BUNDLE}&code_challenge={challenge}"),
        format!("platform=apple&app={BUNDLE}&code_challenge=abc&code_challenge_method=S256"),
        // Not an app of auth.apps, an app without a callback, a system the
        // hand-over does not know, and one these settings list no app of.
        format!(
            "platform=apple&app=com.evil.app&code_challenge={challenge}&code_challenge_method=S256"
        ),
        format!(
            "platform=apple&app=com.example.kiosk&code_challenge={challenge}&code_challenge_method=S256"
        ),
        format!(
            "platform=linux&app={BUNDLE}&code_challenge={challenge}&code_challenge_method=S256"
        ),
        format!(
            "platform=windows&app={BUNDLE}&code_challenge={challenge}&code_challenge_method=S256"
        ),
        // A callback the request names that is not the app's.
        format!(
            "platform=apple&app={BUNDLE}&code_challenge={challenge}&code_challenge_method=S256\
             &redirect_uri=https%3A%2F%2Fevil.example%2Fcb"
        ),
        format!(
            "platform=apple&app={BUNDLE}&code_challenge={challenge}&code_challenge_method=S256\
             &redirect_uri=com.example.orders%3A%2Fsigned-in"
        ),
    ] {
        let response = get(&client, &format!("/auth/app/sign-in?{query}")).await;
        assert_eq!(
            response.status().as_u16(),
            400,
            "{query}: {}",
            response.text()
        );
    }
    // Nothing waits in the session. The sheet shows the error page.
    let finish = get(&client, "/auth/app/finish").await;
    assert_eq!(finish.status().as_u16(), 400);
    assert!(finish.text().contains("invalid_hand_over"));
    assert_eq!(finish.header("location"), None);
}

/// The current session of `client`, as `GET /api/auth/sessions` lists it.
async fn current_session(client: &TestClient) -> serde_json::Value {
    let sessions = client
        .get("/api/auth/sessions")
        .send()
        .await
        .assert_status(200)
        .json_value();
    sessions["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["current"] == json!(true))
        .expect("the current session")
        .clone()
}

/// A time of a session resource, in Unix seconds.
fn unix(session: &serde_json::Value, key: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(session[key].as_str().expect(key))
        .expect(key)
        .timestamp()
}

#[tokio::test]
async fn an_apps_session_keeps_the_sign_in_time_of_the_browser_session() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let (verifier, challenge) = pkce("time");
    let signed_in_at = clock.now().timestamp();
    let (_, code) = sheet_sign_in(&app, "simran@example.com", &challenge).await;
    clock.advance(Duration::from_secs(50));
    let phone = phone(&app).await;
    exchange(&phone, "apple", &code, &verifier)
        .await
        .assert_status(200);

    // The sign-in the app holds is the one made in the sheet: same time,
    // and it ends when a "remember me" sign-in made then would (30 days).
    let session = current_session(&phone).await;
    assert_eq!(unix(&session, "authenticated_at"), signed_in_at);
    assert_eq!(unix(&session, "expires_at"), signed_in_at + 30 * 24 * 3600);
    clock.advance(Duration::from_secs(30 * 24 * 3600 - 50 - 60));
    whoami(&phone).await.assert_status(200);
    clock.advance(Duration::from_secs(61));
    whoami(&phone).await.assert_status(401);
}

#[tokio::test]
async fn the_exchange_asks_the_sign_in_policies() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "gurpreet@blocked.test").await;
    let (verifier, challenge) = pkce("policy");
    // The sheet signs in on the website as usual.
    let (_, code) = sheet_sign_in(&app, "gurpreet@blocked.test", &challenge).await;
    let phone = phone(&app).await;
    exchange(&phone, "apple", &code, &verifier)
        .await
        .assert_error_code(403, "web_only");
    whoami(&phone).await.assert_status(401);
    let refused = sink.with_action("auth.login_refused");
    let last = refused.last().expect("the refusal is audited");
    assert_eq!(last.metadata["method"], "app_hand_over");
    assert_eq!(last.metadata["platform"], "apple");
    assert_eq!(last.metadata["app"], BUNDLE);
}

// Fixation: an attacker signs in as themselves, keeps the code the
// callback carries, and sends it to someone in a link to the app's
// callback. The person's app has no verifier for it (or one of its own),
// so the exchange plants nothing.
#[tokio::test]
async fn a_planted_code_signs_no_one_in() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "attacker@example.com").await;
    let victim = create_user(&app, "simran@example.com").await;
    let (_, attacker_challenge) = pkce("attacker");
    let (_, planted) = sheet_sign_in(&app, "attacker@example.com", &attacker_challenge).await;

    // The victim's app, signed in as the victim, started its own hand-over:
    // it posts the planted code with its own verifier.
    let (victim_verifier, victim_challenge) = pkce("victim");
    let (_, own_code) = sheet_sign_in(&app, "simran@example.com", &victim_challenge).await;
    let victims_app = phone(&app).await;
    exchange(&victims_app, "apple", &own_code, &victim_verifier)
        .await
        .assert_status(200);
    refused(exchange(&victims_app, "apple", &planted, &victim_verifier).await);
    whoami(&victims_app)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(victim.id));

    // A signed-out app with no hand-over of its own has nothing to prove it.
    let (_, planted) = sheet_sign_in(&app, "attacker@example.com", &attacker_challenge).await;
    let fresh = phone(&app).await;
    refused(exchange(&fresh, "apple", &planted, &"x".repeat(43)).await);
    whoami(&fresh).await.assert_status(401);
}

// Another app can claim a custom scheme and start the hand-over itself:
// an app whose callback is a scheme never rides on the browser's sign-in.
#[tokio::test]
async fn a_custom_scheme_app_never_gets_the_browsers_sign_in() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    whoami(&browser).await.assert_status(200);
    start_legacy(&browser, "s-5").await.assert_status(303);
    // The person is asked first; continuing signs the browser out, and the
    // person signs in again for the app.
    let (page, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    assert!(
        page.contains("Sign in again to continue to the Android app com.example.legacy"),
        "{page}"
    );
    assert!(
        page.contains("Continue only if you opened com.example.legacy yourself."),
        "{page}"
    );
    whoami(&browser).await.assert_status(200);
    let to_sign_in = answer(&browser, &token, &csrf_token, "continue")
        .await
        .assert_status(303);
    whoami(&browser).await.assert_status(401);
    let back = finish_with_attempt(&location(&to_sign_in));
    let signed_in = sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    assert_eq!(location(&signed_in), back);
    // The sign-in made for the app does not leave by itself either: the
    // person is told which app asks, and the code leaves when they continue.
    let (page, token, csrf_token) = confirmation(&get(&browser, &back).await);
    assert!(
        page.contains("The Android app com.example.legacy wants to sign in as"),
        "{page}"
    );
    let handed = answer(&browser, &token, &csrf_token, "continue")
        .await
        .assert_status(303);
    assert!(location(&handed).starts_with("com.example.legacy:/signed-in?code="));
    assert_eq!(handed.header("cache-control"), Some("no-store"));
    assert_eq!(handed.header("referrer-policy"), Some("no-referrer"));
}

// Whoever holds a scheme can open the start address itself, with a challenge
// of its own, so the proof of possession says nothing about who asks. A
// sign-in made for an app whose callback is not https is therefore not sent
// on by itself: the page names the app and the system the settings list it
// under, and the code leaves only on Continue. The page is that sign-in's:
// opened again it is the same page, another sign-in gets none, and Continue
// hands over the sign-in the page was shown for and no other.
#[tokio::test]
async fn a_sign_in_for_an_app_on_a_scheme_leaves_only_when_the_person_continues() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let user = create_user(&app, "simran@example.com").await;
    let other = create_user(&app, "harpreet@example.com").await;

    // The browser is signed out, as when another program opens the start.
    let browser = app.client();
    start_legacy(&browser, "s-40").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let asked = get(&browser, &back).await;
    let (page, token, csrf_token) = confirmation(&asked);
    assert!(
        page.contains(
            "The Android app com.example.legacy wants to sign in as Anu Lakhotia (simran@example.com)"
        ),
        "{page}"
    );
    assert!(
        page.contains(
            "You are signed out here. Continue only if you opened com.example.legacy yourself."
        ),
        "{page}"
    );
    // Nothing left for the scheme, and the browser is still signed in.
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));
    // Opened again, with the attempt or without it: the same question.
    for again in [back.as_str(), "/auth/app/finish"] {
        let (_, same, _) = confirmation(&get(&browser, again).await);
        assert_eq!(same, token, "{again}");
    }
    // Continue: the code leaves, once, and the browser is signed out.
    let handed = answer(&browser, &token, &csrf_token, "continue").await;
    let (code, error) = scheme_callback(&handed, LEGACY_CALLBACK, "s-40");
    assert_eq!(error, None);
    whoami(&browser).await.assert_status(401);
    let fresh = csrf(&browser).await;
    refused_page(answer(&browser, &token, &fresh, "continue").await);
    let (verifier, _) = pkce("legacy");
    let phone = phone(&app).await;
    phone
        .post("/api/auth/app/exchange")
        .xsrf()
        .json(&json!({
            "platform": "android",
            "app": "com.example.legacy",
            "code": code.expect("a code"),
            "code_verifier": verifier,
        }))
        .send()
        .await
        .assert_status(200);
    whoami(&phone)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));

    // Cancel: the app hears no, no code leaves, and the browser stays as it is.
    let browser = app.client();
    start_legacy(&browser, "s-41").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, &back).await);
    let cancelled = answer(&browser, &token, &csrf_token, "cancel").await;
    assert_eq!(
        scheme_callback(&cancelled, LEGACY_CALLBACK, "s-41"),
        (None, Some("access_denied".to_owned()))
    );
    whoami(&browser).await.assert_status(200);
    refused_page(get(&browser, &back).await);
    let fresh = csrf(&browser).await;
    refused_page(answer(&browser, &token, &fresh, "continue").await);

    // The page is the sign-in's it was shown for. Somebody else signs in in
    // that browser before it is answered: the page is not shown to them, and
    // the hand-over is over.
    let browser = app.client();
    start_legacy(&browser, "s-42").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let (_, token, _) = confirmation(&get(&browser, &back).await);
    sign_in_form(&browser, "harpreet@example.com", "/orders")
        .await
        .assert_status(303);
    refused_page(get(&browser, "/auth/app/finish").await);
    let fresh = csrf(&browser).await;
    refused_page(answer(&browser, &token, &fresh, "continue").await);

    // Continue pressed once another sign-in has taken that one's place hands
    // nothing over: not the sign-in the page was shown for, which has ended,
    // and not the one the browser has now.
    let browser = app.client();
    start_legacy(&browser, "s-43").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let (_, token, _) = confirmation(&get(&browser, &back).await);
    sign_in_form(&browser, "harpreet@example.com", "/orders")
        .await
        .assert_status(303);
    let fresh = csrf(&browser).await;
    refused_page(answer(&browser, &token, &fresh, "continue").await);
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(other.id));

    let reasons = refusal_reasons(&sink);
    assert_eq!(
        reasons,
        [
            "no app sign-in was started in this browser",
            "no app sign-in was started in this browser",
            "no app sign-in was started in this browser",
            "the sign-in was not made for this app sign-in",
            "no app sign-in was started in this browser",
            "the sign-in made for this app sign-in has ended",
        ]
    );
}

// Where the browser is kept signed in after a hand-over
// (`auth.apps.keep_browser_signed_in`), the page says so, and it is.
#[tokio::test]
async fn a_browser_kept_signed_in_is_told_so_before_its_sign_in_leaves_for_a_scheme() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = builder(&clock, &sink)
        .config("auth.apps.keep_browser_signed_in", true)
        .build()
        .await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    start_legacy(&browser, "s-44").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let (page, token, csrf_token) = confirmation(&get(&browser, &back).await);
    assert!(
        page.contains(
            "You stay signed in here. Continue only if you opened com.example.legacy yourself."
        ),
        "{page}"
    );
    let handed = answer(&browser, &token, &csrf_token, "continue").await;
    let (code, error) = scheme_callback(&handed, LEGACY_CALLBACK, "s-44");
    assert_eq!(error, None);
    assert!(code.is_some());
    whoami(&browser).await.assert_status(200);
}

// Any site can send a browser to the start address. For a signed-in
// browser that changes nothing until the person answers: it is not signed
// out, and no code leaves.
#[tokio::test]
async fn opening_the_start_address_signs_no_one_out() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let user = create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);

    // A custom-scheme app and an https app, each opened and left.
    start_legacy(&browser, "s-10").await.assert_status(303);
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));
    let (_, challenge) = pkce("left");
    start(&browser, &challenge, "s-11").await.assert_status(303);
    whoami(&browser).await.assert_status(200);
    let (page, _, _) = confirmation(&get(&browser, "/auth/app/finish").await);
    assert!(
        page.contains("com.example.orders wants to sign in as Anu Lakhotia (simran@example.com)"),
        "{page}"
    );
    whoami(&browser).await.assert_status(200);

    // Cancel: the app hears no, and the browser stays signed in.
    start_legacy(&browser, "s-12").await.assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    let cancelled = answer(&browser, &token, &csrf_token, "cancel")
        .await
        .assert_status(303);
    assert_eq!(
        location(&cancelled),
        "com.example.legacy:/signed-in?error=access_denied&state=s-12"
    );
    assert_eq!(cancelled.header("cache-control"), Some("no-store"));
    whoami(&browser).await.assert_status(200);
    // The hand-over is over.
    refused_page(get(&browser, "/auth/app/finish").await);
    assert!(sink.with_action("auth.logout").is_empty());
}

// One client starts thirty hand-overs a minute, and the next is told to wait.
#[tokio::test]
async fn one_client_starts_thirty_hand_overs_a_minute() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let browser = app.client();
    let (_, challenge) = pkce("many");
    for n in 0..30 {
        start(&browser, &challenge, &format!("s-{n}"))
            .await
            .assert_status(303);
    }
    start(&browser, &challenge, "s-30").await.assert_status(429);
}

// The confirmation page's answer is bound to its own hand-over and works
// once; a post without the session's CSRF token does nothing.
#[tokio::test]
async fn the_confirmation_works_once_for_its_own_hand_over() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    let (verifier, challenge) = pkce("once-more");

    // A page left open from an earlier hand-over does not answer a later one.
    start(&browser, &challenge, "s-13").await.assert_status(303);
    let (_, stale, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    start(&browser, &challenge, "s-14").await.assert_status(303);
    refused_page(answer(&browser, &stale, &csrf_token, "continue").await);
    refused_page(get(&browser, "/auth/app/finish").await);

    // Without the CSRF token, nothing is answered.
    start(&browser, &challenge, "s-15").await.assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    answer(&browser, &token, "not-the-token", "continue")
        .await
        .assert_status(403);
    let code = callback_code(
        &answer(&browser, &token, &csrf_token, "continue").await,
        "s-15",
    );
    // Once.
    refused_page(answer(&browser, &token, &csrf_token, "continue").await);
    exchange(&phone(&app).await, "apple", &code, &verifier)
        .await
        .assert_status(200);
}

/// Signs the browser out on the sign-out page.
async fn sign_out(client: &TestClient) {
    let token = csrf(client).await;
    client
        .post("/sign-out")
        .header("accept", "text/html")
        .form(&[("_csrf", token.as_str())])
        .send()
        .await
        .assert_status(303);
}

/// A hand-over of the Apple app with `challenge` started in `browser`, which
/// is signed in already, and confirmed on the page: the code of the
/// callback.
async fn confirmed_code(browser: &TestClient, challenge: &str, state: &str) -> String {
    start(browser, challenge, state).await.assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(browser, "/auth/app/finish").await);
    callback_code(
        &answer(browser, &token, &csrf_token, "continue").await,
        state,
    )
}

/// The reasons of the refused hand-overs so far.
fn refusal_reasons(sink: &RecordingSink) -> Vec<String> {
    sink.with_action("auth.app_sign_in_refused")
        .iter()
        .map(|entry| entry.metadata["reason"].as_str().unwrap().to_owned())
        .collect()
}

// A browser signed in without "remember me" lasts a day (24 hours by
// default): the app's session a confirmed hand-over gives ends with that
// browser session, not 30 days after the sign-in.
#[tokio::test]
async fn a_confirmed_hand_over_from_a_session_without_remember_me_ends_when_that_session_does() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    let browser_session = current_session(&browser).await;
    let signed_in_at = unix(&browser_session, "authenticated_at");
    let browser_ends = unix(&browser_session, "expires_at");
    assert_eq!(browser_ends, signed_in_at + 24 * 3600);

    clock.advance(Duration::from_secs(3600));
    let (verifier, challenge) = pkce("not-remembered");
    let code = confirmed_code(&browser, &challenge, "s-20").await;
    let phone = phone(&app).await;
    exchange(&phone, "apple", &code, &verifier)
        .await
        .assert_status(200);

    let session = current_session(&phone).await;
    assert_eq!(unix(&session, "authenticated_at"), signed_in_at);
    assert_eq!(unix(&session, "expires_at"), browser_ends);
    clock.advance(Duration::from_secs(23 * 3600 - 60));
    whoami(&phone).await.assert_status(200);
    clock.advance(Duration::from_secs(61));
    whoami(&phone).await.assert_status(401);
}

// A code is worth only what its browser session still is: once the person
// signs that browser out, a confirmed hand-over's code signs no app in.
#[tokio::test]
async fn a_confirmed_hand_overs_code_is_refused_once_the_browser_is_signed_out() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    let (verifier, challenge) = pkce("signed-out");
    let code = confirmed_code(&browser, &challenge, "s-21").await;
    sign_out(&browser).await;
    whoami(&browser).await.assert_status(401);

    let phone = phone(&app).await;
    refused(exchange(&phone, "apple", &code, &verifier).await);
    whoami(&phone).await.assert_status(401);
    assert_eq!(
        refusal_reasons(&sink),
        ["the browser session the code came from has ended"]
    );
}

// Signing out everywhere ends the sign-in a code came from, and the code
// with it: a sign-in made for the hand-over (the browser kept signed in) and
// a confirmed one alike.
#[tokio::test]
async fn a_code_is_refused_once_the_person_signs_out_everywhere() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = builder(&clock, &sink)
        .config("auth.apps.keep_browser_signed_in", true)
        .build()
        .await;
    let user = create_user(&app, "simran@example.com").await;
    let auth = app.get::<AuthService>().await;
    let (verifier, challenge) = pkce("everywhere");
    let (sheet, code) = sheet_sign_in(&app, "simran@example.com", &challenge).await;
    whoami(&sheet).await.assert_status(200);
    auth.logout(None, user.id, true).await.unwrap();
    whoami(&sheet).await.assert_status(401);
    let phone = phone(&app).await;
    refused(exchange(&phone, "apple", &code, &verifier).await);
    whoami(&phone).await.assert_status(401);

    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    let (verifier, challenge) = pkce("everywhere-confirmed");
    let code = confirmed_code(&browser, &challenge, "s-22").await;
    auth.logout(None, user.id, true).await.unwrap();
    refused(exchange(&phone, "apple", &code, &verifier).await);
    whoami(&phone).await.assert_status(401);
    assert_eq!(
        refusal_reasons(&sink),
        [
            "the sign-in the code came from has ended",
            "the browser session the code came from has ended",
        ]
    );
}

// The confirmation page never writes the session's CSRF token as it is: each
// page carries a mask of its own, and each mask posts.
#[tokio::test]
async fn the_confirmation_pages_token_differs_between_renders_and_each_posts() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    let session_token = browser.cookie("XSRF-TOKEN").expect("the XSRF-TOKEN cookie");
    let (verifier, challenge) = pkce("masked");
    start(&browser, &challenge, "s-23").await.assert_status(303);
    let (_, token, first) = confirmation(&get(&browser, "/auth/app/finish").await);
    let (_, same, second) = confirmation(&get(&browser, "/auth/app/finish").await);
    assert_eq!(token, same, "one hand-over, one value");
    assert_ne!(first, second, "a fresh mask on every page");
    assert_ne!(first, session_token);
    assert_ne!(second, session_token);

    // The first page's token posts.
    let code = callback_code(&answer(&browser, &token, &first, "continue").await, "s-23");
    exchange(&phone(&app).await, "apple", &code, &verifier)
        .await
        .assert_status(200);
    // And the second's, for the next hand-over of the same browser.
    start(&browser, &challenge, "s-24").await.assert_status(303);
    let (_, token, _) = confirmation(&get(&browser, "/auth/app/finish").await);
    let code = callback_code(&answer(&browser, &token, &second, "continue").await, "s-24");
    exchange(&phone(&app).await, "apple", &code, &verifier)
        .await
        .assert_status(200);
}

// The callback's scheme is read as a URL's, in any case: `HTTPS://` is an
// https callback, so a confirmed browser's sign-in is handed over and the
// browser stays signed in (a custom scheme would sign it out).
#[tokio::test]
async fn an_upper_case_https_callback_is_an_https_callback() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = builder(&clock, &sink)
        .config(
            "auth.apps.apple.orders.callback",
            "HTTPS://example.com/app/signed-in",
        )
        .build()
        .await;
    let user = create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    let (verifier, challenge) = pkce("upper-case");
    start(&browser, &challenge, "s-25").await.assert_status(303);
    let (page, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    assert!(
        page.contains("com.example.orders wants to sign in as"),
        "{page}"
    );
    let code = callback_code(
        &answer(&browser, &token, &csrf_token, "continue").await,
        "s-25",
    );
    whoami(&browser)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));
    exchange(&phone(&app).await, "apple", &code, &verifier)
        .await
        .assert_status(200);
}

/// How many of `responses` send the browser to the app's callback with a
/// code.
fn codes_among(responses: &[TestResponse]) -> usize {
    responses
        .iter()
        .filter(|response| {
            response.status().as_u16() == 303
                && response
                    .header("location")
                    .is_some_and(|to| to.starts_with(CALLBACK) && to.contains("code="))
        })
        .count()
}

// The hand-over's one-time values work once also for requests sent at the
// same time, before either has saved the session: the attempt a sign-in
// came back with, and the answer of the confirmation page, each give one
// code between two requests.
#[tokio::test]
async fn an_attempt_or_a_confirmation_used_twice_at_once_issues_one_code() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let (_, challenge) = pkce("at-once");

    let sheet = app.client();
    start(&sheet, &challenge, "s-26").await.assert_status(303);
    let to_sign_in = get(&sheet, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&sheet, "simran@example.com", &back)
        .await
        .assert_status(303);
    let (one, two) = tokio::join!(get(&sheet, &back), get(&sheet, &back));
    assert_eq!(codes_among(&[one, two]), 1);

    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    start(&browser, &challenge, "s-27").await.assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    let (one, two) = tokio::join!(
        answer(&browser, &token, &csrf_token, "continue"),
        answer(&browser, &token, &csrf_token, "continue")
    );
    assert_eq!(codes_among(&[one, two]), 1);
}

/// `GET path` with `headers` on the request, as the browser or a script
/// says how it asks.
async fn asked_with(client: &TestClient, path: &str, headers: &[(&str, &str)]) -> TestResponse {
    let mut request = client.get(path).header("accept", "text/html");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.send().await
}

/// The sign-in form posted by the page's own script (navree-ui's runtime),
/// which is answered with ops: each op's name and where it goes.
async fn sign_in_by_script(client: &TestClient, email: &str, next: &str) -> Vec<(String, String)> {
    let token = csrf(client).await;
    let told = client
        .post("/sign-in")
        .header("accept", "text/html")
        .header("x-navree-ui", "1")
        .form(&[
            ("email", email),
            ("password", PASSWORD),
            ("next", next),
            ("_csrf", token.as_str()),
        ])
        .send()
        .await
        .assert_status(200);
    assert_eq!(told.header("location"), None);
    told.json_value()["ops"]
        .as_array()
        .expect("ops")
        .iter()
        .map(|op| {
            (
                op["op"].as_str().unwrap_or_default().to_owned(),
                op["href"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

// The sign-in page's own script posts the form and is told where to go. A
// page of the site it fetches and shows in place (`navigate`). The finish
// address it is told to leave for, as for another site
// (`navigate_external`): the browser goes there itself, and the script
// fetches nothing.
#[tokio::test]
async fn the_sign_in_pages_send_the_browser_itself_to_the_finish_address() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let (_, challenge) = pkce("told");
    let sheet = app.client();
    start(&sheet, &challenge, "s-31").await.assert_status(303);
    let to_sign_in = get(&sheet, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    assert_eq!(
        sign_in_by_script(&sheet, "simran@example.com", &back).await,
        [("navigate_external".to_owned(), back.clone())]
    );
    // A browser that is signed in already and opens the sign-in page again
    // is sent on the same way.
    let again = sheet
        .get(&format!(
            "/sign-in?next={}",
            url::form_urlencoded::byte_serialize(back.as_bytes()).collect::<String>()
        ))
        .header("accept", "text/html")
        .header("x-navree-ui", "1")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(again["ops"][0]["op"], "navigate_external");
    assert_eq!(again["ops"][0]["href"].as_str(), Some(back.as_str()));

    // Any other page of the site: fetched and shown in place, as before.
    let browser = app.client();
    assert_eq!(
        sign_in_by_script(&browser, "simran@example.com", "/orders?page=2").await,
        [("navigate".to_owned(), "/orders?page=2".to_owned())]
    );
    // An address that only looks like the finish is such a page.
    let other = app.client();
    assert_eq!(
        sign_in_by_script(&other, "simran@example.com", "/auth/app/finished").await,
        [("navigate".to_owned(), "/auth/app/finished".to_owned())]
    );
}

// Only the browser itself opens the finish address. Fetched by the sign-in
// page's script or any other, loaded ahead of a click or in a frame, it
// changes nothing, so the hand-over is still there for the browser; the
// answer is a page no runtime shows, so navree-ui's goes there for real.
#[tokio::test]
async fn only_the_browsers_own_navigation_finishes_a_hand_over() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let (verifier, challenge) = pkce("fetched");
    let sheet = app.client();
    start(&sheet, &challenge, "s-30").await.assert_status(303);
    let to_sign_in = get(&sheet, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&sheet, "simran@example.com", &back)
        .await
        .assert_status(303);

    for headers in [
        // navree-ui's runtime, going there or fetching ahead of a click.
        &[("x-navree-ui", "1")][..],
        &[("x-navree-ui", "1"), ("x-navree-prefetch", "1")],
        // Any other script's fetch.
        &[
            ("sec-fetch-mode", "cors"),
            ("sec-fetch-dest", "empty"),
            ("sec-fetch-site", "same-origin"),
        ],
        // The browser loading the page ahead of a click or for a preview, and
        // a frame.
        &[
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-dest", "document"),
            ("sec-purpose", "prefetch"),
        ],
        &[
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-dest", "document"),
            ("x-purpose", "preview"),
        ],
        &[("sec-fetch-mode", "navigate"), ("sec-fetch-dest", "iframe")],
    ] {
        let refused = asked_with(&sheet, &back, headers).await;
        assert_eq!(
            refused.status().as_u16(),
            403,
            "{headers:?}: {}",
            refused.text()
        );
        assert_eq!(refused.header("location"), None, "{headers:?}");
        // HTML that does not say it is a page of the app: what navree-ui's
        // runtime loads for real, at the address the answer came from.
        assert!(
            refused
                .header("content-type")
                .is_some_and(|kind| kind.starts_with("text/html")),
            "{headers:?}"
        );
        assert_eq!(refused.header("x-navree-page"), None, "{headers:?}");
        assert_eq!(refused.header("cache-control"), Some("no-store"));
        assert_eq!(refused.header("referrer-policy"), Some("no-referrer"));
        assert_eq!(refused.header("x-frame-options"), Some("DENY"));
        let page = refused.text();
        assert!(
            !page.contains("code=") && !page.contains("<script"),
            "{page}"
        );
    }
    // Nothing changed: the browser is still signed in for the hand-over, and
    // no hand-over was refused.
    whoami(&sheet).await.assert_status(200);
    assert!(sink.with_action("auth.app_sign_in_refused").is_empty());

    // The browser itself goes there, and the code leaves for the app.
    let handed = asked_with(
        &sheet,
        &back,
        &[
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-dest", "document"),
            ("sec-fetch-site", "same-origin"),
        ],
    )
    .await;
    let code = callback_code(&handed, "s-30");
    exchange(&phone(&app).await, "apple", &code, &verifier)
        .await
        .assert_status(200);
    // A fetch afterwards is told the same, whatever became of the hand-over.
    asked_with(&sheet, &back, &[("x-navree-ui", "1")])
        .await
        .assert_status(403);
    assert!(sink.with_action("auth.app_sign_in_refused").is_empty());
}

/// The `next` of the sign-in page a hand-over sends the browser to: the
/// finish address with the hand-over's `attempt`.
fn finish_with_attempt(sign_in: &str) -> String {
    let url = url::Url::parse(&format!("https://example.com{sign_in}")).unwrap();
    assert_eq!(url.path(), "/sign-in");
    let next = url
        .query_pairs()
        .find(|(key, _)| key == "next")
        .map(|(_, next)| next.into_owned())
        .expect("a next");
    assert!(next.starts_with("/auth/app/finish?attempt="), "{next}");
    assert!(
        next.len() > "/auth/app/finish?attempt=".len() + 20,
        "{next}"
    );
    next
}

/// The callback of the custom-scheme Android app.
const LEGACY_CALLBACK: &str = "com.example.legacy:/signed-in";

/// `GET /auth/app/sign-in` for the custom-scheme Android app.
async fn start_legacy(client: &TestClient, state: &str) -> TestResponse {
    let (_, challenge) = pkce("legacy");
    get(
        client,
        &format!(
            "/auth/app/sign-in?platform=android&app=com.example.legacy\
             &code_challenge={challenge}&code_challenge_method=S256&state={state}"
        ),
    )
    .await
}

// Another app on the phone starts a hand-over for a custom-scheme app while
// the browser is signed out, and the person leaves the sign-in page. Later
// they sign in to the site for their own reasons, in the same browser; the
// other app then opens the finish address itself. That sign-in was not made
// for the hand-over, so no code leaves for the scheme, and the hand-over is
// over.
#[tokio::test]
async fn a_sign_in_made_for_something_else_is_never_handed_to_a_custom_scheme_app() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    start_legacy(&browser, "s-6").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let mut back = finish_with_attempt(&location(&to_sign_in));

    // The person signs in to the site, not for the app.
    clock.advance(Duration::from_secs(120));
    sign_in_form(&browser, "simran@example.com", "/orders")
        .await
        .assert_status(303);
    whoami(&browser).await.assert_status(200);

    // The finish address opened without the attempt, or with another one.
    for opened in ["/auth/app/finish", "/auth/app/finish?attempt=guessed"] {
        refused_page(get(&browser, opened).await);
        // The hand-over is over: not even its own address works now.
        refused_page(get(&browser, &back).await);
        // A new start asks the signed-in person first; continuing signs the
        // browser out, and the person is to sign in again for the app.
        start_legacy(&browser, "s-7").await.assert_status(303);
        let (_, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
        let to_sign_in = answer(&browser, &token, &csrf_token, "continue")
            .await
            .assert_status(303);
        whoami(&browser).await.assert_status(401);
        back = finish_with_attempt(&location(&to_sign_in));
        // The person signs in for something else again.
        sign_in_form(&browser, "simran@example.com", "/orders")
            .await
            .assert_status(303);
    }
    let reasons: Vec<String> = sink
        .with_action("auth.app_sign_in_refused")
        .iter()
        .map(|entry| entry.metadata["reason"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        reasons,
        [
            "the sign-in was not made for this app sign-in",
            "no app sign-in was started in this browser",
            "the sign-in was not made for this app sign-in",
            "no app sign-in was started in this browser",
        ]
    );

    // The hand-over's own sign-in still finishes, once the person says they
    // opened the app.
    let browser = app.client();
    start_legacy(&browser, "s-8").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, &back).await);
    let handed = answer(&browser, &token, &csrf_token, "continue")
        .await
        .assert_status(303);
    assert!(location(&handed).starts_with("com.example.legacy:/signed-in?code="));
}

// The same for an app with an https callback: while its sheet is open, a
// sign-in made in that browser for anything else (in another tab, or one
// another site posted) is not handed to the app.
#[tokio::test]
async fn a_sign_in_made_for_something_else_is_never_handed_to_an_https_app() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    let (_, challenge) = pkce("elsewhere");
    start(&browser, &challenge, "s-9").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));

    clock.advance(Duration::from_secs(120));
    sign_in_form(&browser, "simran@example.com", "/orders")
        .await
        .assert_status(303);
    refused_page(get(&browser, "/auth/app/finish").await);
    refused_page(get(&browser, &back).await);
    whoami(&browser).await.assert_status(200);
    let reasons: Vec<String> = sink
        .with_action("auth.app_sign_in_refused")
        .iter()
        .map(|entry| entry.metadata["reason"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        reasons,
        [
            "the sign-in was not made for this app sign-in",
            "no app sign-in was started in this browser",
        ]
    );
}

// When the system does not open the app, the browser lands on the https
// callback itself, with the code in its address: a site that turns the page
// on answers with an empty page that keeps nothing and sends nothing on.
#[tokio::test]
async fn the_site_answers_its_callback_address_with_an_empty_page() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = builder(&clock, &sink)
        .config("auth.apps.callback_page", true)
        .build()
        .await;
    let landed = get(&app.client(), "/app/signed-in?code=Tq7uX0b2&state=s-1")
        .await
        .assert_status(200);
    assert_eq!(landed.header("cache-control"), Some("no-store"));
    assert_eq!(landed.header("referrer-policy"), Some("no-referrer"));
    let page = landed.text();
    assert!(!page.contains("Tq7uX0b2"), "{page}");
    assert!(!page.contains("<script"), "{page}");

    // Off by default: the framework takes no path of the app's.
    let app = builder(&clock, &sink).build().await;
    let response = get(&app.client(), "/app/signed-in?code=Tq7uX0b2").await;
    assert_eq!(response.status().as_u16(), 404);
}

/// An app's own page at the path of its https callback.
struct OwnCallbackController;

impl Injectable for OwnCallbackController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(OwnCallbackController) })
    }
}

impl Controller for OwnCallbackController {
    fn prefix(&self) -> &'static str {
        "!/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/app/signed-in", || async {
                Html("<p>Open the orders app to continue.</p>").into_response()
            })
            .public(),
        ]
    }
}

struct OwnCallbackModule;

impl Module for OwnCallbackModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<OwnCallbackController>();
    }
}

// An app that serves the path of its https callback itself boots as
// configured, and its own page answers there.
#[tokio::test]
async fn an_app_with_its_own_route_at_the_callback_path_boots_by_default() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = builder(&clock, &sink)
        .module(OwnCallbackModule)
        .try_build()
        .await
        .expect("the app boots with its own route at the callback path");
    let page = get(&app.client(), "/app/signed-in?code=Tq7uX0b2")
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Open the orders app to continue."), "{page}");
}

#[tokio::test]
async fn an_https_callback_must_be_on_the_sites_own_host() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let error = builder(&clock, &sink)
        .config("app.url", "https://shop.example.com")
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("auth.apps.apple.orders.callback must be on the site's own host"),
        "{error}"
    );
    assert!(
        error.contains("auth.apps.android.orders.callback"),
        "{error}"
    );
    // The custom scheme has no host to be on.
    assert!(!error.contains("legacy"), "{error}");
    let app = builder(&clock, &sink)
        .config("app.url", "https://example.com")
        .config("auth.apps.callback_page", true)
        .build()
        .await;
    get(&app.client(), "/app/signed-in")
        .await
        .assert_status(200);
}

/// The package family name of the Windows app, by which a build installed
/// from its package names itself.
const FAMILY: &str = "CharkhaSupply_8wekyb3d8bbwe";
/// The id a build of the Windows app with no package was built with.
const UNPACKAGED: &str = "com.charkha.supply";
/// The Windows app's callback: a link on its own scheme.
const WINDOWS_CALLBACK: &str = "charkha:/signed-in";

/// The app of these tests with a Windows app listed beside the others.
fn with_windows(clock: &Arc<FixedClock>, sink: &Arc<RecordingSink>) -> TestAppBuilder {
    builder(clock, sink)
        .config("auth.apps.windows.supply.package_family_name", FAMILY)
        .config("auth.apps.windows.supply.app_id", UNPACKAGED)
        .config("auth.apps.windows.supply.callback", WINDOWS_CALLBACK)
}

/// `GET /auth/app/sign-in` for the Windows app named `id`, with `more` after
/// the query.
async fn start_windows(
    client: &TestClient,
    id: &str,
    challenge: &str,
    state: &str,
    more: &str,
) -> TestResponse {
    get(
        client,
        &format!(
            "/auth/app/sign-in?platform=windows&app={id}&code_challenge={challenge}\
             &code_challenge_method=S256&state={state}{more}"
        ),
    )
    .await
}

/// The Windows callback the hand-over sent the browser to: its `code`, or
/// its `error`, with the state checked.
fn windows_callback(response: &TestResponse, state: &str) -> (Option<String>, Option<String>) {
    scheme_callback(response, WINDOWS_CALLBACK, state)
}

/// The callback on a scheme of an app's own that the hand-over sent the
/// browser to: its `code`, or its `error`, with the state checked. The
/// address before the query is the configured `callback`, to the letter.
fn scheme_callback(
    response: &TestResponse,
    callback: &str,
    state: &str,
) -> (Option<String>, Option<String>) {
    assert_eq!(response.status().as_u16(), 303, "{}", response.text());
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
    let to = location(response);
    let (address, query) = to.split_once('?').expect("a query");
    assert_eq!(address, callback);
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    assert!(
        pairs.contains(&("state".to_owned(), state.to_owned())),
        "{pairs:?}"
    );
    let named = |name: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    (named("code"), named("error"))
}

/// The person signs in in the browser for the Windows app named `id`, and
/// says on the page that follows that they opened it: the browser and the
/// code its callback carried.
async fn browser_sign_in_for_windows(
    app: &TestApp,
    id: &str,
    challenge: &str,
    state: &str,
) -> (TestClient, String) {
    let browser = app.client();
    let started = start_windows(&browser, id, challenge, state, "")
        .await
        .assert_status(303);
    assert_eq!(location(&started), "/auth/app/finish");
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let (_, token, csrf_token) = confirmation(&get(&browser, &back).await);
    let handed = answer(&browser, &token, &csrf_token, "continue").await;
    let (code, error) = windows_callback(&handed, state);
    assert_eq!(error, None);
    (browser, code.expect("a code"))
}

/// The exchange as the Windows app `id` posts it.
async fn exchange_as(
    client: &TestClient,
    platform: &str,
    id: &str,
    code: &str,
    verifier: &str,
) -> TestResponse {
    client
        .post("/api/auth/app/exchange")
        .xsrf()
        .json(&json!({
            "platform": platform,
            "app": id,
            "code": code,
            "code_verifier": verifier,
        }))
        .send()
        .await
}

// A Windows app signs a person in through their own browser: the start with
// its id, an S256 challenge and a state, the sign-in pages, the page that
// names the app and asks, a one-time code on the link of its own scheme, and
// the exchange from its own HTTP stack. A build installed from a package
// names itself by its package family name, a build with no package by the id
// it was built with.
#[tokio::test]
async fn a_sign_in_in_the_browser_is_handed_to_a_windows_app() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = with_windows(&clock, &sink).build().await;
    let user = create_user(&app, "simran@example.com").await;
    for (id, state) in [(FAMILY, "w-1"), (UNPACKAGED, "w-2")] {
        let (verifier, challenge) = pkce(id);
        // The app names the callback it comes back on, as the shell told it.
        let browser = app.client();
        start_windows(
            &browser,
            id,
            &challenge,
            state,
            "&redirect_uri=charkha%3A%2Fsigned-in",
        )
        .await
        .assert_status(303);
        let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
        let back = finish_with_attempt(&location(&to_sign_in));
        sign_in_form(&browser, "simran@example.com", &back)
            .await
            .assert_status(303);
        // Any program can hold the scheme: the person is told which app
        // asks, as the settings name it, and nothing leaves before they
        // continue.
        let (page, token, csrf_token) = confirmation(&get(&browser, &back).await);
        assert!(
            page.contains(&format!(
                "The Windows app {id} wants to sign in as Anu Lakhotia (simran@example.com)"
            )),
            "{page}"
        );
        assert!(
            page.contains(&format!("Continue only if you opened {id} yourself.")),
            "{page}"
        );
        whoami(&browser).await.assert_status(200);
        let handed = answer(&browser, &token, &csrf_token, "continue").await;
        let (code, error) = windows_callback(&handed, state);
        assert_eq!(error, None);
        let code = code.expect("a code");
        // The browser signed in only for the hand-over: it is signed out again.
        whoami(&browser).await.assert_status(401);

        let desktop = phone(&app).await;
        whoami(&desktop).await.assert_status(401);
        exchange_as(&desktop, "windows", id, &code, &verifier)
            .await
            .assert_status(200)
            .assert_json_path("email", json!("simran@example.com"));
        whoami(&desktop)
            .await
            .assert_status(200)
            .assert_json_path("id", json!(user.id));
        let login = sink.with_action("auth.login");
        let handed = login.last().unwrap();
        assert_eq!(handed.metadata["method"], "app_hand_over");
        assert_eq!(handed.metadata["platform"], "windows");
        assert_eq!(handed.metadata["app"], id);
    }
    assert!(sink.with_action("auth.app_sign_in_refused").is_empty());
}

// The start is refused for an id that is not a Windows app's, a callback
// that is not the app's, a challenge that is not S256, and a state that is
// no state; and nothing then waits in the browser.
#[tokio::test]
async fn a_windows_start_with_a_wrong_app_callback_challenge_or_state_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = with_windows(&clock, &sink).build().await;
    let (_, challenge) = pkce("windows-start");
    let s256 = format!("code_challenge={challenge}&code_challenge_method=S256");
    let client = app.client();
    for (what, query) in [
        // The app's id.
        (
            "an id no Windows app has",
            format!("platform=windows&app=CharkhaOther_8wekyb3d8bbwe&{s256}&state=x"),
        ),
        (
            "the id the app was built with, in another case",
            format!("platform=windows&app=Com.Charkha.Supply&{s256}&state=x"),
        ),
        (
            "the package family name with another publisher id",
            format!("platform=windows&app=CharkhaSupply_8wekyb3d8bbwf&{s256}&state=x"),
        ),
        (
            "the name of the app's settings",
            format!("platform=windows&app=supply&{s256}&state=x"),
        ),
        (
            "the Apple app's id",
            format!("platform=windows&app={BUNDLE}&{s256}&state=x"),
        ),
        (
            "the Windows app's id as an Apple app",
            format!("platform=apple&app={FAMILY}&{s256}&state=x"),
        ),
        (
            "the Windows app's id as an Android app",
            format!("platform=android&app={UNPACKAGED}&{s256}&state=x"),
        ),
        ("no app", format!("platform=windows&{s256}&state=x")),
        // The callback the request names.
        (
            "another path on the scheme",
            format!("platform=windows&app={FAMILY}&{s256}&state=x&redirect_uri=charkha%3A%2Fother"),
        ),
        (
            "the scheme with a host",
            format!(
                "platform=windows&app={FAMILY}&{s256}&state=x&redirect_uri=charkha%3A%2F%2Fsigned-in"
            ),
        ),
        (
            "the scheme in capitals",
            format!(
                "platform=windows&app={FAMILY}&{s256}&state=x&redirect_uri=Charkha%3A%2Fsigned-in"
            ),
        ),
        (
            "another scheme",
            format!(
                "platform=windows&app={FAMILY}&{s256}&state=x&redirect_uri=charkhb%3A%2Fsigned-in"
            ),
        ),
        (
            "the Apple app's callback",
            format!(
                "platform=windows&app={FAMILY}&{s256}&state=x\
                 &redirect_uri=https%3A%2F%2Fexample.com%2Fapp%2Fsigned-in"
            ),
        ),
        (
            "a port of the machine",
            format!(
                "platform=windows&app={FAMILY}&{s256}&state=x\
                 &redirect_uri=http%3A%2F%2F127.0.0.1%3A5123%2Fsigned-in"
            ),
        ),
        // The challenge.
        (
            "no challenge",
            format!("platform=windows&app={FAMILY}&state=x"),
        ),
        (
            "a plain challenge",
            format!(
                "platform=windows&app={FAMILY}&code_challenge={challenge}\
                 &code_challenge_method=plain&state=x"
            ),
        ),
        (
            "no method",
            format!("platform=windows&app={FAMILY}&code_challenge={challenge}&state=x"),
        ),
        (
            "a short challenge",
            format!(
                "platform=windows&app={FAMILY}&code_challenge=abc&code_challenge_method=S256&state=x"
            ),
        ),
        // The state.
        (
            "a state of 513 bytes",
            format!(
                "platform=windows&app={FAMILY}&{s256}&state={}",
                "s".repeat(513)
            ),
        ),
        (
            "a state with a line break",
            format!("platform=windows&app={FAMILY}&{s256}&state=a%0Ab"),
        ),
    ] {
        let response = get(&client, &format!("/auth/app/sign-in?{query}")).await;
        assert_eq!(
            response.status().as_u16(),
            400,
            "{what}: {}",
            response.text()
        );
        assert_eq!(response.header("location"), None, "{what}");
        // Nothing waits in the browser.
        refused_page(get(&client, "/auth/app/finish").await);
    }
    // The longest state there is, and the callback named to the letter, start.
    start_windows(
        &client,
        FAMILY,
        &challenge,
        &"s".repeat(512),
        "&redirect_uri=charkha%3A%2Fsigned-in",
    )
    .await
    .assert_status(303);
}

// The code works once, for the id that started the hand-over, with the
// verifier whose challenge started it: not twice, not for the app's other
// id, not for another system's app, not with another verifier.
#[tokio::test]
async fn a_windows_apps_code_works_once_for_its_id_with_its_verifier() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = with_windows(&clock, &sink).build().await;
    create_user(&app, "simran@example.com").await;
    let (verifier, challenge) = pkce("windows-once");

    // Used twice.
    let (_, code) = browser_sign_in_for_windows(&app, FAMILY, &challenge, "w-3").await;
    let desktop = phone(&app).await;
    exchange_as(&desktop, "windows", FAMILY, &code, &verifier)
        .await
        .assert_status(200);
    refused(exchange_as(&desktop, "windows", FAMILY, &code, &verifier).await);

    // As the same app's other id, as another system's app, and as an app
    // of another system that has the same id: each use takes the code.
    for (platform, id) in [
        ("windows", UNPACKAGED),
        ("apple", BUNDLE),
        ("apple", FAMILY),
        ("android", FAMILY),
    ] {
        let (_, code) = browser_sign_in_for_windows(&app, FAMILY, &challenge, "w-4").await;
        let stranger = phone(&app).await;
        refused(exchange_as(&stranger, platform, id, &code, &verifier).await);
        refused(exchange_as(&stranger, "windows", FAMILY, &code, &verifier).await);
        whoami(&stranger).await.assert_status(401);
    }

    // A wrong verifier, and the code is gone with it.
    let (_, code) = browser_sign_in_for_windows(&app, FAMILY, &challenge, "w-5").await;
    let (other_verifier, _) = pkce("windows-other");
    let stranger = phone(&app).await;
    refused(exchange_as(&stranger, "windows", FAMILY, &code, &other_verifier).await);
    refused(exchange_as(&stranger, "windows", FAMILY, &code, &verifier).await);
    whoami(&stranger).await.assert_status(401);

    // Too late.
    let (_, code) = browser_sign_in_for_windows(&app, FAMILY, &challenge, "w-6").await;
    clock.advance(Duration::from_secs(APP_CODE_TTL_SECS + 1));
    refused(exchange_as(&stranger, "windows", FAMILY, &code, &verifier).await);

    assert_eq!(
        refusal_reasons(&sink),
        [
            "unknown or expired code",
            "code of another app",
            "unknown or expired code",
            "code of another app",
            "unknown or expired code",
            "code of another app",
            "unknown or expired code",
            "code of another app",
            "unknown or expired code",
            "wrong code verifier",
            "unknown or expired code",
            "code expired",
        ]
    );
}

// Windows tells package family names apart without regard to the case of
// their letters. A build that writes its own in another case than the
// settings do starts, and exchanges, as that app, and the audit trail is
// given the name as the settings write it. The id an app was built with is
// held to the letter.
#[tokio::test]
async fn a_windows_build_that_writes_its_package_family_name_in_another_case_is_that_app() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = with_windows(&clock, &sink).build().await;
    let user = create_user(&app, "simran@example.com").await;
    let (verifier, challenge) = pkce("windows-case");

    let (_, code) =
        browser_sign_in_for_windows(&app, "charkhasupply_8wekyb3d8bbwe", &challenge, "w-10").await;
    let desktop = phone(&app).await;
    exchange_as(
        &desktop,
        "windows",
        "CHARKHASUPPLY_8WEKYB3D8BBWE",
        &code,
        &verifier,
    )
    .await
    .assert_status(200);
    whoami(&desktop)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));
    let login = sink.with_action("auth.login");
    let handed = login.last().unwrap();
    assert_eq!(handed.metadata["method"], "app_hand_over");
    assert_eq!(handed.metadata["app"], FAMILY);
    assert!(sink.with_action("auth.app_sign_in_refused").is_empty());

    // The id the app was built with, in another case, is no app's: not at
    // the start, and not at the exchange of a code the app itself started.
    let browser = app.client();
    start_windows(&browser, "Com.Charkha.Supply", &challenge, "w-11", "")
        .await
        .assert_status(400);
    let (_, code) = browser_sign_in_for_windows(&app, UNPACKAGED, &challenge, "w-12").await;
    let stranger = phone(&app).await;
    refused(exchange_as(&stranger, "windows", "COM.CHARKHA.SUPPLY", &code, &verifier).await);
    whoami(&stranger).await.assert_status(401);
}

// Any program on the machine can register a scheme and start the hand-over
// itself, so a Windows app never rides on a sign-in the browser already
// had: the person is asked, is signed out there, signs in again, and says
// once more that they opened the app before the code leaves.
#[tokio::test]
async fn a_windows_app_never_gets_the_browsers_sign_in() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = with_windows(&clock, &sink).build().await;
    create_user(&app, "simran@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "simran@example.com", "/")
        .await
        .assert_status(303);
    let (verifier, challenge) = pkce("windows-browser");

    // Opening the start address changes nothing by itself.
    start_windows(&browser, FAMILY, &challenge, "w-7", "")
        .await
        .assert_status(303);
    whoami(&browser).await.assert_status(200);
    // Cancel: the app hears no on its callback, and the browser stays signed in.
    let (_, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    let cancelled = answer(&browser, &token, &csrf_token, "cancel").await;
    assert_eq!(
        windows_callback(&cancelled, "w-7"),
        (None, Some("access_denied".to_owned()))
    );
    whoami(&browser).await.assert_status(200);
    refused_page(get(&browser, "/auth/app/finish").await);

    // Continue: no code leaves for the scheme. The browser is signed out,
    // and the person signs in again for the app.
    start_windows(&browser, FAMILY, &challenge, "w-8", "")
        .await
        .assert_status(303);
    let (page, token, csrf_token) = confirmation(&get(&browser, "/auth/app/finish").await);
    assert!(
        page.contains(&format!(
            "Sign in again to continue to the Windows app {FAMILY}"
        )),
        "{page}"
    );
    assert!(
        page.contains(&format!("Continue only if you opened {FAMILY} yourself.")),
        "{page}"
    );
    let to_sign_in = answer(&browser, &token, &csrf_token, "continue")
        .await
        .assert_status(303);
    assert!(location(&to_sign_in).starts_with("/sign-in?next="));
    whoami(&browser).await.assert_status(401);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    // The new sign-in does not leave by itself either.
    let (page, token, csrf_token) = confirmation(&get(&browser, &back).await);
    assert!(
        page.contains(&format!("The Windows app {FAMILY} wants to sign in as")),
        "{page}"
    );
    let handed = answer(&browser, &token, &csrf_token, "continue").await;
    let (code, error) = windows_callback(&handed, "w-8");
    assert_eq!(error, None);
    exchange_as(
        &phone(&app).await,
        "windows",
        FAMILY,
        &code.expect("a code"),
        &verifier,
    )
    .await
    .assert_status(200);

    // A sign-in made for something else while a hand-over waits is not
    // handed to the scheme either.
    let other = app.client();
    start_windows(&other, FAMILY, &challenge, "w-9", "")
        .await
        .assert_status(303);
    get(&other, "/auth/app/finish").await.assert_status(303);
    sign_in_form(&other, "simran@example.com", "/orders")
        .await
        .assert_status(303);
    refused_page(get(&other, "/auth/app/finish").await);
}

// A Windows app's callback is a link on its own scheme and nothing else,
// and it has an id: anything else stops the boot, naming the setting.
#[tokio::test]
async fn a_windows_app_with_another_kind_of_callback_or_no_id_does_not_boot() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    for callback in [
        "https://example.com/app/signed-in",
        "http://127.0.0.1:5123/signed-in",
        "Charkha:/signed-in",
        "charkha:/signed-in?from=windows",
        "ms-settings:/signed-in",
    ] {
        let error = with_windows(&clock, &sink)
            .config("auth.apps.windows.supply.callback", callback)
            .try_build()
            .await
            .expect_err(callback)
            .to_string();
        assert!(
            error.contains("auth.apps.windows.supply.callback"),
            "{callback}: {error}"
        );
    }
    let error = builder(&clock, &sink)
        .config("auth.apps.windows.supply.callback", WINDOWS_CALLBACK)
        .try_build()
        .await
        .expect_err("no id")
        .to_string();
    assert!(
        error.contains("auth.apps.windows.supply must name the app"),
        "{error}"
    );
    // With an https `app.url`, a Windows app's callback has no host to be on.
    with_windows(&clock, &sink)
        .config("app.url", "https://example.com")
        .try_build()
        .await
        .expect("a scheme is checked against no host");
}

// A platform sign-in (Sign in with Apple in the app) that needs a second
// step leaves it waiting in the app's own session; the two-factor page,
// opened inside the app, finishes it.
#[tokio::test]
async fn a_platform_sign_in_waiting_for_its_second_step_finishes_on_the_page() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let user: User = create_user(&app, "simran@example.com").await;
    let secret = enable_two_factor(&app, &clock, &user).await;
    let phone = self::phone(&app).await;
    phone
        .post("/api/platform-sign-in/start")
        .xsrf()
        .json(&json!({ "email": "simran@example.com" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("two_factor_required", json!(true));
    whoami(&phone).await.assert_status(401);

    let page = get(&phone, "/sign-in/two-factor?next=%2Forders").await;
    assert_eq!(page.status().as_u16(), 200, "{}", page.text());
    let token = csrf(&phone).await;
    let code = totp_code(&secret, &clock);
    let done = phone
        .post("/sign-in/two-factor")
        .header("accept", "text/html")
        .form(&[
            ("code", code.as_str()),
            ("next", "/orders"),
            ("_csrf", token.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
    assert_eq!(location(&done), "/orders");
    whoami(&phone)
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));
}
