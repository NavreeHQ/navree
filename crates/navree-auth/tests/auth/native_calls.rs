//! Signing in with the system's own sheet in a native app: the sign-in page
//! gives a client that can the `passkey` device call, and a provider's
//! sign-in sheet where a module is set up to take it; every other client gets
//! the page as it was. The passkey call's two routes check its ticket, the
//! challenge and the assertion.

use std::sync::Arc;

use navree_auth::testing::VirtualAuthenticator;
use navree_auth::{AuthPages, ProviderSheet, ProviderSheets, SheetProvider, User};
use navree_contracts::{AuditSink, Clock as _, FixedClock};
use navree_kernel::{Factory, Module, ModuleBuilder};
use navree_native::device::{
    Alt, Call, DeviceCall, OriginPath, Passkey, PasskeyMode, PlatformSignIn, SignInProvider,
};
use navree_native::node::Node as N;
use navree_native::{Action, Envelope, Op};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestAppBuilder, TestClient, TestResponse};
use navree_ui::UiModule;
use serde_json::{Value, json};

use crate::support::{self, RecordingSink, enable_two_factor, signed_in};

const NATIVE: &str = "x-navree-native";
const CLIENT: &str = "x-navree-client";
const XSRF: &str = "XSRF-TOKEN";

const RP_ID: &str = "example.com";
/// The origin an Apple app's assertions carry: `https://<rp_id>`.
const APP_ORIGIN: &str = "https://example.com";
const WEB_ORIGIN: &str = "https://app.example.com";

const FINGERPRINT: &str = "14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5";

const OPTIONS: &str = "/sign-in/passkey/native/options";
const RESULT: &str = "/sign-in/passkey/native";

/// What a client on `platform` whose app has the device features `caps` says of
/// itself.
fn client_of(platform: &str, caps: &str) -> String {
    format!(
        "platform={platform}; form=phone; vocab={}; caps={caps}",
        navree_native::VOCAB
    )
}

/// An iPhone app that can show both sheets.
fn iphone() -> String {
    client_of("ios", "passkey+platform_sign_in")
}

/// `builder` with passkeys on and the session layer, navree-ui and the pages.
fn with_passkey_pages(builder: TestAppBuilder, sink: &Arc<RecordingSink>) -> TestAppBuilder {
    builder
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", RP_ID)
        .config("auth.passkeys.origins", json!([WEB_ORIGIN, APP_ORIGIN]))
        .override_value::<dyn AuditSink>(sink.clone())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
}

/// The site with an Apple app that shares its passkeys.
async fn apple_app(clock: &Arc<FixedClock>, sink: &Arc<RecordingSink>) -> TestApp {
    with_passkey_pages(support::clocked(clock), sink)
        .config("auth.apps.apple.orders.team_id", "ABCDE12345")
        .config("auth.apps.apple.orders.bundle_id", "com.example.orders")
        .build()
        .await
}

/// A user with a passkey registered from the app: the user and the
/// authenticator that holds it.
async fn with_passkey(app: &TestApp, email: &str) -> (User, VirtualAuthenticator) {
    with_passkey_of(app, email, VirtualAuthenticator::new(RP_ID, APP_ORIGIN)).await
}

async fn with_passkey_of(
    app: &TestApp,
    email: &str,
    authenticator: VirtualAuthenticator,
) -> (User, VirtualAuthenticator) {
    let (user, access, _) = signed_in(app, email).await;
    let options = app
        .client()
        .post("/api/auth/passkeys/register/options")
        .bearer(&access)
        .json(&json!({}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    app.client()
        .post("/api/auth/passkeys/register")
        .bearer(&access)
        .json(&json!({ "credential": authenticator.create(&options["publicKey"]) }))
        .send()
        .await
        .assert_status(201);
    (user, authenticator)
}

/// What the native client `descriptor` is answered at `path`: a screen, or
/// the word to show the page in its web view.
async fn answer(client: &TestClient, descriptor: &str, path: &str) -> Envelope {
    let response = client
        .get(path)
        .header(NATIVE, "1")
        .header(CLIENT, descriptor)
        .send()
        .await
        .assert_status(200);
    serde_json::from_str(&response.text()).expect("an envelope")
}

/// The sign-in page at `path`, as the native client `descriptor` gets it
/// when the page has a call to give it: a screen.
async fn screen(client: &TestClient, descriptor: &str, path: &str) -> Envelope {
    let envelope = answer(client, descriptor, path).await;
    assert!(
        envelope.screen.is_some(),
        "the sign-in page is a screen: {:?}",
        envelope.web
    );
    envelope
}

/// Whether the client is told to show the page in its web view, as every
/// native client was before a page could give it a call: the layout of the
/// sign-in pages has no native kind.
fn is_the_web_views(envelope: &Envelope) -> bool {
    envelope.screen.is_none()
        && envelope
            .web
            .as_ref()
            .is_some_and(|web| web.why == ["slot:auth-layout"])
}

/// What a tap on the link or the button named `label` does.
fn tap(envelope: &Envelope, label: &str) -> Option<Action> {
    let screen = envelope.screen.as_ref()?;
    let mut walk = vec![&screen.body];
    while let Some(node) = walk.pop() {
        match node {
            N::Link(link) if link.label == label => return Some(link.action.clone()),
            N::Button(button) if button.label == label => return button.action.clone(),
            _ => {}
        }
        walk.extend(node.children());
    }
    None
}

/// The passkey call of a sign-in screen: the route of its options, and where
/// and with which ticket its result goes.
fn passkey_call(envelope: &Envelope) -> (String, String, String) {
    let Some(Action::Device(DeviceCall {
        call: Call::Passkey(passkey),
        then: Some(then),
        ..
    })) = tap(envelope, "Sign in with a passkey")
    else {
        panic!(
            "no passkey call: {:?}",
            tap(envelope, "Sign in with a passkey")
        );
    };
    (
        passkey.options.as_str().to_owned(),
        then.href.as_str().to_owned(),
        then.ticket.as_str().to_owned(),
    )
}

/// The call of the sign-in page at `/sign-in?next=/deals`, for an iPhone app.
async fn call_of(client: &TestClient) -> (String, String, String) {
    passkey_call(&screen(client, &iphone(), "/sign-in?next=%2Fdeals").await)
}

/// The request options of the call, asked as the app's core asks: a post of
/// `{}` with the session's token.
async fn options(client: &TestClient) -> Value {
    client
        .post(OPTIONS)
        .xsrf()
        .json(&json!({}))
        .send()
        .await
        .assert_status(200)
        .json_value()["publicKey"]
        .clone()
}

/// The call's result, posted as the app's core posts it: a form, with the
/// session's token in its header. Asked for JSON, a refusal says its code.
async fn post_result(client: &TestClient, href: &str, fields: &[(&str, &str)]) -> TestResponse {
    client
        .post(href)
        .header("accept", "application/json")
        .xsrf()
        .form(fields)
        .send()
        .await
}

/// The result of a call that brought an assertion.
async fn post_assertion(
    client: &TestClient,
    href: &str,
    ticket: &str,
    assertion: &Value,
) -> TestResponse {
    let credential = assertion.to_string();
    post_result(
        client,
        href,
        &[
            ("credential", credential.as_str()),
            ("_call", ticket),
            ("outcome", "ok"),
        ],
    )
    .await
}

/// Where an answer sends the app: a redirect's address, or the address a
/// native answer's op names.
fn went_to(response: &TestResponse) -> String {
    if let Some(location) = response.header("location") {
        return location.to_owned();
    }
    let envelope: Envelope = serde_json::from_str(&response.text()).expect("an envelope");
    envelope
        .ops
        .iter()
        .find_map(|op| match op {
            Op::Navigate { href, .. } => Some(href.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("nowhere to go: {}", response.text()))
}

async fn whoami(client: &TestClient) -> TestResponse {
    client.get("/api/probe/whoami").send().await
}

/// Why sign-ins with a passkey were refused, as the audit trail says.
fn refusals(sink: &RecordingSink) -> Vec<String> {
    sink.with_action("auth.login_failed")
        .iter()
        .filter(|entry| entry.metadata["method"] == "passkey")
        .map(|entry| entry.metadata["reason"].as_str().unwrap_or("").to_owned())
        .collect()
}

// An app that says it can do passkeys, of a site with an app that shares its
// passkeys, is given the call on the link to the passkey page: where its
// options come from, where its result goes, and a ticket to post with it.
#[tokio::test]
async fn the_sign_in_page_gives_an_app_that_can_the_passkey_call() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let phone = app.client();

    let envelope = screen(&phone, &iphone(), "/sign-in?next=%2Fdeals").await;
    let Some(Action::Device(DeviceCall { call, then, alt })) =
        tap(&envelope, "Sign in with a passkey")
    else {
        panic!("not a device call");
    };
    assert_eq!(
        call,
        Call::Passkey(Passkey {
            mode: PasskeyMode::SignIn,
            options: OriginPath::parse(OPTIONS).unwrap(),
            conditional: false,
        })
    );
    let then = then.expect("where the assertion goes");
    assert_eq!(then.href.as_str(), "/sign-in/passkey/native?next=%2Fdeals");
    assert!(!then.ticket.as_str().is_empty());
    // The ticket is good for five minutes, and says so.
    let now = clock.now().timestamp();
    assert!(then.exp > now && then.exp <= now + 300, "{}", then.exp);
    // A build that can't start the call goes to the passkey page, as a browser does.
    assert_eq!(
        alt,
        Some(Alt::Navigate {
            href: OriginPath::parse("/sign-in/passkey?next=%2Fdeals").unwrap()
        })
    );
    // Each page has a ticket of its own.
    let again = screen(&phone, &iphone(), "/sign-in?next=%2Fdeals").await;
    assert_ne!(passkey_call(&again).2, then.ticket.as_str());

    // A browser's page is the page it was: the link, and no word of the call.
    let page = app
        .client()
        .get("/sign-in?next=%2Fdeals")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("/sign-in/passkey?next=%2Fdeals"), "{page}");
    assert!(page.contains("data-ui-noboost"), "{page}");
    assert!(!page.contains("/sign-in/passkey/native"), "{page}");
}

// Without the capability, or where the site is not set up for the client's
// family, the client is given no call and the page is what it was for a native
// client: the web view's, where the link leads to the passkey page.
#[tokio::test]
async fn without_the_capability_or_an_app_of_its_family_the_page_is_what_it_was() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    for descriptor in [
        // An app that does not say it can.
        client_of("ios", "share+platform_sign_in"),
        // The site has no Android app.
        client_of("android", "passkey"),
        // A system no app of the site is on.
        client_of("windows", "passkey"),
    ] {
        let envelope = answer(&app.client(), &descriptor, "/sign-in").await;
        assert!(is_the_web_views(&envelope), "{descriptor}: {envelope:?}");
    }
    // With an Android app that shares the site's passkeys, its clients get the call.
    let both = with_passkey_pages(support::clocked(&clock), &sink)
        .config("auth.apps.android.orders.package", "com.example.orders")
        .config(
            "auth.apps.android.orders.fingerprints",
            json!([FINGERPRINT]),
        )
        .build()
        .await;
    let envelope = screen(&both.client(), &client_of("android", "passkey"), "/sign-in").await;
    assert!(matches!(
        tap(&envelope, "Sign in with a passkey"),
        Some(Action::Device(_))
    ));
    // That site has no Apple app.
    let envelope = answer(&both.client(), &iphone(), "/sign-in").await;
    assert!(is_the_web_views(&envelope), "{envelope:?}");
}

// An Apple app is offered the call only when it shares the site's passkeys
// and its assertions' origin is one the passkey routes take.
#[tokio::test]
async fn an_apple_app_needs_credentials_and_the_relying_partys_origin() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    // The app does not share the site's passkeys.
    let links_only = with_passkey_pages(support::clocked(&clock), &sink)
        .config("auth.apps.apple.orders.team_id", "ABCDE12345")
        .config("auth.apps.apple.orders.bundle_id", "com.example.orders")
        .config("auth.apps.apple.orders.credentials", false)
        .build()
        .await;
    let envelope = answer(&links_only.client(), &iphone(), "/sign-in").await;
    assert!(is_the_web_views(&envelope), "{envelope:?}");
    // `https://<rp_id>` is not among the origins.
    let web_only = support::clocked(&clock)
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", RP_ID)
        .config("auth.passkeys.origins", json!([WEB_ORIGIN]))
        .config("auth.apps.apple.orders.team_id", "ABCDE12345")
        .config("auth.apps.apple.orders.bundle_id", "com.example.orders")
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
        .build()
        .await;
    let envelope = answer(&web_only.client(), &iphone(), "/sign-in").await;
    assert!(is_the_web_views(&envelope), "{envelope:?}");
}

// The whole call: the options, the system's sheet, the assertion posted with
// the ticket. The app's session is signed in as the page's own route signs a
// browser in.
#[tokio::test]
async fn a_passkey_signs_an_app_in_through_its_call() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let (user, authenticator) = with_passkey(&app, "simran@example.com").await;
    let phone = app.client();

    let (options_at, result_at, ticket) = call_of(&phone).await;
    assert_eq!(options_at, OPTIONS);
    let public_key = options(&phone).await;
    assert_eq!(public_key["rpId"], RP_ID);
    let assertion = authenticator.get(&public_key).expect("the passkey answers");
    whoami(&phone).await.assert_status(401);

    // Posted as the app's core posts it, to a native client's answer.
    let credential = assertion.to_string();
    let done = phone
        .post(&result_at)
        .header(NATIVE, "1")
        .header(CLIENT, iphone())
        .xsrf()
        .form(&[
            ("credential", credential.as_str()),
            ("_call", ticket.as_str()),
            ("outcome", "ok"),
        ])
        .send()
        .await;
    assert_eq!(went_to(&done), "/deals");
    whoami(&phone)
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"))
        .assert_json_path("user_id", json!(user.id));
    let logins = sink.with_action("auth.login");
    let last = logins.last().expect("an auth.login entry");
    assert_eq!(last.metadata["method"], "passkey");
    assert_eq!(last.metadata["via"], "session");
    assert!(refusals(&sink).is_empty());
}

// The result is taken only with the ticket the page sealed: none, a made-up
// one, another session's, and one used before are all refused, and nobody is
// signed in.
#[tokio::test]
async fn a_result_is_taken_once_with_the_ticket_of_its_own_session() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let (_, authenticator) = with_passkey(&app, "simran@example.com").await;
    let phone = app.client();
    let (_, result_at, ticket) = call_of(&phone).await;
    let assertion = authenticator.get(&options(&phone).await).unwrap();

    // No ticket, and one nobody sealed.
    let credential = assertion.to_string();
    post_result(
        &phone,
        &result_at,
        &[("credential", credential.as_str()), ("outcome", "ok")],
    )
    .await
    .assert_error_code(422, "invalid_ticket");
    post_assertion(&phone, &result_at, "made-up", &assertion)
        .await
        .assert_error_code(422, "invalid_ticket");
    // Another session's ticket.
    let other = app.client();
    let (_, _, others_ticket) = call_of(&other).await;
    post_assertion(&phone, &result_at, &others_ticket, &assertion)
        .await
        .assert_error_code(422, "invalid_ticket");
    // A session with no cookie has no ticket at all: its post is not its own.
    app.client()
        .post(&result_at)
        .header("accept", "application/json")
        .form(&[
            ("credential", credential.as_str()),
            ("_call", ticket.as_str()),
            ("outcome", "ok"),
        ])
        .send()
        .await
        .assert_status(303);
    whoami(&phone).await.assert_status(401);

    // The ticket works once: the first post spends it, whatever it brought.
    let cancelled = post_result(
        &phone,
        &result_at,
        &[("_call", ticket.as_str()), ("outcome", "cancelled")],
    )
    .await
    .assert_status(303);
    assert_eq!(went_to(&cancelled), "/sign-in?next=%2Fdeals");
    post_assertion(&phone, &result_at, &ticket, &assertion)
        .await
        .assert_error_code(422, "invalid_ticket");
    whoami(&phone).await.assert_status(401);
    // None of it was a passkey that failed.
    assert!(refusals(&sink).is_empty(), "{:?}", refusals(&sink));
}

// A forged assertion is refused, and the challenge it answered is used up: the
// true assertion over the same challenge, posted with a fresh ticket, is
// refused too.
#[tokio::test]
async fn a_forged_assertion_is_refused_and_its_challenge_is_used_up() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let (_, authenticator) = with_passkey(&app, "simran@example.com").await;
    let phone = app.client();
    let (_, result_at, ticket) = call_of(&phone).await;
    let assertion = authenticator.get(&options(&phone).await).unwrap();

    // The same answer with another's signature: 36 bytes that no key of the person's made.
    let mut forged = assertion.clone();
    forged["response"]["signature"] = json!("MEUCIQDforgedforgedforgedforgedforgedforgedforge");
    let refused = post_assertion(&phone, &result_at, &ticket, &forged)
        .await
        .assert_status(303);
    assert_eq!(
        went_to(&refused),
        "/sign-in?next=%2Fdeals&status=passkey-failed"
    );
    whoami(&phone).await.assert_status(401);
    // The page it goes back to says so, and carries a new ticket.
    let page = screen(&phone, &iphone(), &went_to(&refused)).await;
    let (_, _, fresh) = passkey_call(&page);
    assert_ne!(fresh, ticket);

    // The challenge was used by the first attempt: the true assertion is too late.
    let late = post_assertion(&phone, &result_at, &fresh, &assertion)
        .await
        .assert_status(303);
    assert_eq!(
        went_to(&late),
        "/sign-in?next=%2Fdeals&status=passkey-failed"
    );
    whoami(&phone).await.assert_status(401);
    assert_eq!(
        refusals(&sink),
        ["bad signature", "no challenge asked for in this session"]
    );

    // Something that is not an assertion at all is refused the same way.
    let (_, _, third) = call_of(&phone).await;
    options(&phone).await;
    let junk = post_result(
        &phone,
        &result_at,
        &[
            ("credential", "not json"),
            ("_call", third.as_str()),
            ("outcome", "ok"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(
        went_to(&junk),
        "/sign-in?next=%2Fdeals&status=passkey-failed"
    );
    assert_eq!(refusals(&sink).len(), 3);
}

// An assertion answers the challenge its own session asked for. One made for
// the challenge of another session signs nobody in here, and is still good
// where it was asked for.
#[tokio::test]
async fn an_assertion_over_another_sessions_challenge_signs_nobody_in() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let (user, authenticator) = with_passkey(&app, "simran@example.com").await;

    // One phone asks for a challenge and the person answers it.
    let phone = app.client();
    let (_, result_at, ticket) = call_of(&phone).await;
    let assertion = authenticator.get(&options(&phone).await).unwrap();

    // Another session, with a ticket and a challenge of its own, posts that answer.
    let other = app.client();
    let (_, _, others_ticket) = call_of(&other).await;
    options(&other).await;
    let refused = post_assertion(&other, &result_at, &others_ticket, &assertion)
        .await
        .assert_status(303);
    assert_eq!(
        went_to(&refused),
        "/sign-in?next=%2Fdeals&status=passkey-failed"
    );
    whoami(&other).await.assert_status(401);
    assert_eq!(refusals(&sink), ["challenge of another session"]);

    // A session that asked for no challenge has none to answer.
    let third = app.client();
    let (_, _, thirds_ticket) = call_of(&third).await;
    post_assertion(&third, &result_at, &thirds_ticket, &assertion)
        .await
        .assert_status(303);
    whoami(&third).await.assert_status(401);

    // Where it was asked for, the answer still signs the person in.
    let done = post_assertion(&phone, &result_at, &ticket, &assertion)
        .await
        .assert_status(303);
    assert_eq!(went_to(&done), "/deals");
    whoami(&phone)
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id));
}

// The same assertion never signs in twice: after the sign-in, the page of a
// new session gives a new ticket, and the old answer is refused there.
#[tokio::test]
async fn an_assertion_that_signed_in_once_is_refused_the_second_time() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let (_, authenticator) = with_passkey(&app, "simran@example.com").await;
    let phone = app.client();
    let (_, result_at, ticket) = call_of(&phone).await;
    let assertion = authenticator.get(&options(&phone).await).unwrap();
    post_assertion(&phone, &result_at, &ticket, &assertion)
        .await
        .assert_status(303);
    whoami(&phone).await.assert_status(200);

    // The ticket died with the session the sign-in replaced.
    post_assertion(&phone, &result_at, &ticket, &assertion)
        .await
        .assert_error_code(422, "invalid_ticket");
    // Somewhere else, with a ticket and a challenge of that session's own.
    let other = app.client();
    let (_, _, others_ticket) = call_of(&other).await;
    options(&other).await;
    post_assertion(&other, &result_at, &others_ticket, &assertion)
        .await
        .assert_status(303);
    whoami(&other).await.assert_status(401);
    assert_eq!(refusals(&sink), ["challenge of another session"]);
}

// A call that brings no assertion goes back to the sign-in page: with no
// words when the person closed the sheet, with a notice otherwise.
#[tokio::test]
async fn a_call_that_brought_no_assertion_goes_back_to_the_sign_in_page() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let phone = app.client();
    let (_, result_at, ticket) = call_of(&phone).await;
    let unused = post_result(
        &phone,
        &result_at,
        &[("_call", ticket.as_str()), ("outcome", "unavailable")],
    )
    .await
    .assert_status(303);
    assert_eq!(
        went_to(&unused),
        "/sign-in?next=%2Fdeals&status=passkey-unused"
    );
    let page = phone
        .get(&went_to(&unused))
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("The passkey was not used."), "{page}");
    whoami(&phone).await.assert_status(401);
    assert!(refusals(&sink).is_empty());
}

// A passkey that did not verify the person is one factor: an account with
// two-factor authentication goes on to its second step, signed in to nothing.
#[tokio::test]
async fn a_passkey_without_user_verification_still_waits_for_the_second_factor() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    let key = VirtualAuthenticator::new(RP_ID, APP_ORIGIN).without_user_verification();
    let (user, authenticator) = with_passkey_of(&app, "simran@example.com", key).await;
    enable_two_factor(&app, &clock, &user).await;
    let phone = app.client();
    let (_, result_at, ticket) = call_of(&phone).await;
    let assertion = authenticator.get(&options(&phone).await).unwrap();
    let waits = post_assertion(&phone, &result_at, &ticket, &assertion)
        .await
        .assert_status(303);
    assert_eq!(went_to(&waits), "/sign-in/two-factor?next=%2Fdeals");
    whoami(&phone).await.assert_status(401);
}

// The two routes are the pages' own: without the session's cookie and its
// token nothing is answered, and with passkeys off they are not there.
#[tokio::test]
async fn the_calls_routes_take_only_the_sessions_own_posts() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = apple_app(&clock, &sink).await;
    // No session at all.
    app.client()
        .post(OPTIONS)
        .json(&json!({}))
        .send()
        .await
        .assert_status(403);
    // The session, without its token.
    let phone = app.client();
    call_of(&phone).await;
    assert!(phone.cookie(XSRF).is_some());
    phone
        .post(OPTIONS)
        .json(&json!({}))
        .send()
        .await
        .assert_status(403);
    phone
        .post(RESULT)
        .header("accept", "application/json")
        .form(&[("outcome", "cancelled")])
        .send()
        .await
        .assert_status(403);

    let off = support::clocked(&clock)
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
        .build()
        .await;
    let phone = off.client();
    // The page starts the session, and has no call to give.
    assert!(is_the_web_views(
        &answer(&phone, &iphone(), "/sign-in").await
    ));
    phone
        .post(OPTIONS)
        .xsrf()
        .json(&json!({}))
        .send()
        .await
        .assert_status(404);
}

/// A module that takes Sign in with Apple and Sign in with Google from an app.
struct Sheets;

impl ProviderSheets for Sheets {
    fn sheets(&self) -> Vec<ProviderSheet> {
        vec![
            ProviderSheet::new(
                SheetProvider::Apple,
                "/api/auth/social/apple/native/nonce",
                "/api/auth/social/apple/native/session",
            ),
            ProviderSheet::new(
                SheetProvider::Google,
                "/api/auth/social/google/native/nonce",
                "/api/auth/social/google/native/session",
            )
            .client_id("4081.apps.example"),
        ]
    }
}

struct SheetsModule;

impl Module for SheetsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(Factory::<dyn ProviderSheets>::new(|_| async {
            Ok(Arc::new(Sheets) as Arc<dyn ProviderSheets>)
        }));
    }
}

fn path(text: &str) -> OriginPath {
    OriginPath::parse(text).unwrap()
}

// A provider's sheet is offered to the app whose system has it and that says
// it can open it: Apple's on Apple's systems, Google's on Android. The call
// names the module's two routes and the page of a second step.
#[tokio::test]
async fn a_providers_sheet_is_offered_to_the_app_whose_system_has_it() {
    let clock = support::clock();
    let app = support::clocked(&clock)
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
        .module(SheetsModule)
        .build()
        .await;

    let apple = screen(&app.client(), &iphone(), "/sign-in").await;
    assert_eq!(
        tap(&apple, "Sign in with Apple"),
        Some(Action::Device(DeviceCall::new(PlatformSignIn {
            provider: SignInProvider::Apple,
            nonce: path("/api/auth/social/apple/native/nonce"),
            sign_in: path("/api/auth/social/apple/native/session"),
            web: None,
            client_id: None,
            second_step: Some(path("/sign-in/two-factor")),
        })))
    );
    assert_eq!(tap(&apple, "Sign in with Google"), None);

    let android = client_of("android", "platform_sign_in");
    let google = screen(&app.client(), &android, "/sign-in").await;
    assert_eq!(
        tap(&google, "Sign in with Google"),
        Some(Action::Device(DeviceCall::new(PlatformSignIn {
            provider: SignInProvider::Google,
            nonce: path("/api/auth/social/google/native/nonce"),
            sign_in: path("/api/auth/social/google/native/session"),
            web: None,
            client_id: Some("4081.apps.example".to_owned()),
            second_step: Some(path("/sign-in/two-factor")),
        })))
    );
    assert_eq!(tap(&google, "Sign in with Apple"), None);
    // The form and its links stand on the same screen.
    assert_eq!(
        tap(&google, "Forgot your password?"),
        Some(Action::Navigate {
            href: "/forgot-password".to_owned(),
            present: navree_native::Present::Push,
        })
    );

    // An app that does not say it can, and a system with no such sheet, get neither:
    // the page is the web view's, as it was.
    for descriptor in [
        client_of("ios", "passkey+share"),
        client_of("windows", "platform_sign_in"),
    ] {
        let envelope = answer(&app.client(), &descriptor, "/sign-in").await;
        assert!(is_the_web_views(&envelope), "{descriptor}: {envelope:?}");
    }
    // A browser's page has no such button.
    let page = app
        .client()
        .get("/sign-in")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(!page.contains("Sign in with Apple"), "{page}");
    assert!(!page.contains("Sign in with Google"), "{page}");
}

// With no module set up for it, no client is offered a provider's sheet.
#[tokio::test]
async fn without_a_module_that_takes_it_no_sheet_is_offered() {
    let clock = support::clock();
    let app = support::clocked(&clock)
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(UiModule)
        .module(AuthPages::new())
        .build()
        .await;
    let envelope = answer(&app.client(), &iphone(), "/sign-in").await;
    assert!(is_the_web_views(&envelope), "{envelope:?}");
}

// A screen that has one call keeps what it has no call for as it was: an app
// that opens a provider's sheet and can't do passkeys gets the link to the
// passkey page.
#[tokio::test]
async fn a_screen_with_one_call_keeps_the_link_it_has_no_call_for() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = with_passkey_pages(support::clocked(&clock), &sink)
        .config("auth.apps.apple.orders.team_id", "ABCDE12345")
        .config("auth.apps.apple.orders.bundle_id", "com.example.orders")
        .module(SheetsModule)
        .build()
        .await;
    let sheet_only = client_of("ios", "platform_sign_in");
    let envelope = screen(&app.client(), &sheet_only, "/sign-in?next=%2Fdeals").await;
    assert!(matches!(
        tap(&envelope, "Sign in with Apple"),
        Some(Action::Device(_))
    ));
    assert_eq!(
        tap(&envelope, "Sign in with a passkey"),
        Some(Action::navigate("/sign-in/passkey?next=%2Fdeals"))
    );
    // An app that can do both gets both.
    let envelope = screen(&app.client(), &iphone(), "/sign-in?next=%2Fdeals").await;
    assert!(matches!(
        tap(&envelope, "Sign in with Apple"),
        Some(Action::Device(_))
    ));
    assert!(matches!(
        tap(&envelope, "Sign in with a passkey"),
        Some(Action::Device(_))
    ));
}
