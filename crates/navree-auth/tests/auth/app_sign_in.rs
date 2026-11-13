//! Handing a sign-in made in the system's sign-in session over to a native
//! app: `GET /auth/app/sign-in` with an S256 code challenge, the sign-in
//! pages, `GET /auth/app/finish` to the app's callback with a one-time
//! code, and `POST /api/auth/app/exchange` with the verifier from the app's
//! own HTTP stack. Also: a platform sign-in waiting for its second step,
//! finished on the two-factor page.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::Method;
use navree_auth::{APP_CODE_TTL_SECS, AuthPages, AuthService, ClientMeta, SignInMethod, User};
use navree_contracts::{AuditSink, FixedClock};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{Session, SessionModule, SessionOptions};
use navree_testing::{TestApp, TestClient, TestResponse};
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

struct PlatformModule;

impl Module for PlatformModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<PlatformController>();
    }
}

async fn app(clock: &Arc<FixedClock>, sink: &Arc<RecordingSink>) -> TestApp {
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
        .build()
        .await
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
    assert_eq!(
        location(&to_sign_in),
        "/sign-in?next=%2Fauth%2Fapp%2Ffinish"
    );
    let signed_in = sign_in_form(&sheet, email, "/auth/app/finish")
        .await
        .assert_status(303);
    assert_eq!(location(&signed_in), "/auth/app/finish");
    let code = callback_code(&get(&sheet, "/auth/app/finish").await, "s-1");
    (sheet, code)
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
    let first = sign_in_form(&sheet, "simran@example.com", "/auth/app/finish")
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
            ("next", "/auth/app/finish"),
            ("_csrf", token.as_str()),
        ])
        .send()
        .await
        .assert_status(303);
    assert_eq!(location(&done), "/auth/app/finish");
    let code = callback_code(&get(&sheet, "/auth/app/finish").await, "s-2");
    let iphone = phone(&app).await;
    exchange(&iphone, "apple", &code, &verifier)
        .await
        .assert_status(200);
    whoami(&iphone).await.assert_status(200);

    // A browser that was signed in before: handed over at once, and still
    // signed in afterwards.
    let other = create_user(&app, "harpreet@example.com").await;
    let browser = app.client();
    sign_in_form(&browser, "harpreet@example.com", "/")
        .await
        .assert_status(303);
    let (verifier, challenge) = pkce("browser");
    start(&browser, &challenge, "s-3").await.assert_status(303);
    let code = callback_code(&get(&browser, "/auth/app/finish").await, "s-3");
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
    let code = callback_code(&get(&browser, "/auth/app/finish").await, "s-4");
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
    let (_, challenge) = pkce("scheme");
    get(
        &browser,
        &format!(
            "/auth/app/sign-in?platform=android&app=com.example.legacy\
             &code_challenge={challenge}&code_challenge_method=S256&state=s-5"
        ),
    )
    .await
    .assert_status(303);
    whoami(&browser).await.assert_status(401);
    let to_sign_in = get(&browser, "/auth/app/finish").await.assert_status(303);
    assert_eq!(
        location(&to_sign_in),
        "/sign-in?next=%2Fauth%2Fapp%2Ffinish"
    );
    sign_in_form(&browser, "simran@example.com", "/auth/app/finish")
        .await
        .assert_status(303);
    let handed = get(&browser, "/auth/app/finish").await.assert_status(303);
    assert!(location(&handed).starts_with("com.example.legacy:/signed-in?code="));
    assert_eq!(handed.header("cache-control"), Some("no-store"));
    assert_eq!(handed.header("referrer-policy"), Some("no-referrer"));
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
