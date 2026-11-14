//! Handing a sign-in made in the system's sign-in session over to a native
//! app: `GET /auth/app/sign-in` with an S256 code challenge, the sign-in
//! pages, `GET /auth/app/finish` to the app's callback with a one-time
//! code, and `POST /api/auth/app/exchange` with the verifier from the app's
//! own HTTP stack; the page that asks a browser that was already signed in
//! first, `POST /auth/app/confirm`. Also: a platform sign-in waiting for its
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
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
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
    let signed_in = sign_in_form(&sheet, email, &back)
        .await
        .assert_status(303);
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
        // Not an app of auth.apps, an app without a callback, another platform.
        format!(
            "platform=apple&app=com.evil.app&code_challenge={challenge}&code_challenge_method=S256"
        ),
        format!(
            "platform=apple&app=com.example.kiosk&code_challenge={challenge}&code_challenge_method=S256"
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
        page.contains("Sign in again to continue to com.example.legacy"),
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
    let handed = get(&browser, &back).await.assert_status(303);
    assert!(location(&handed).starts_with("com.example.legacy:/signed-in?code="));
    assert_eq!(handed.header("cache-control"), Some("no-store"));
    assert_eq!(handed.header("referrer-policy"), Some("no-referrer"));
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

    // The hand-over's own sign-in still finishes.
    let browser = app.client();
    start_legacy(&browser, "s-8").await.assert_status(303);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    let back = finish_with_attempt(&location(&to_sign_in));
    sign_in_form(&browser, "simran@example.com", &back)
        .await
        .assert_status(303);
    let handed = get(&browser, &back).await.assert_status(303);
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
// callback itself, with the code in its address: the site answers with an
// empty page that keeps nothing and sends nothing on.
#[tokio::test]
async fn the_site_answers_its_callback_address_with_an_empty_page() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let landed = get(&app.client(), "/app/signed-in?code=Tq7uX0b2&state=s-1")
        .await
        .assert_status(200);
    assert_eq!(landed.header("cache-control"), Some("no-store"));
    assert_eq!(landed.header("referrer-policy"), Some("no-referrer"));
    let page = landed.text();
    assert!(!page.contains("Tq7uX0b2"), "{page}");
    assert!(!page.contains("<script"), "{page}");

    // An app that answers that address itself turns it off.
    let app = builder(&clock, &sink)
        .config("auth.apps.callback_page", false)
        .build()
        .await;
    let response = get(&app.client(), "/app/signed-in?code=Tq7uX0b2").await;
    assert_eq!(response.status().as_u16(), 404);
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
        .build()
        .await;
    get(&app.client(), "/app/signed-in")
        .await
        .assert_status(200);
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
