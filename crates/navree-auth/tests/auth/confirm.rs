//! Confirming a sensitive action with a passkey: the page issues a
//! `confirm` ticket bound to the action, `POST /api/auth/confirm/options`
//! turns it into a challenge, and the handler of the action checks the
//! assertion against the action's own details with
//! `AuthService::confirm_action`.

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_auth::testing::VirtualAuthenticator;
use navree_auth::{
    AccessClaims, AuthError, AuthService, CONFIRM_TTL_SECS, ClientMeta, ConfirmAction, ConfirmProof,
};
use navree_contracts::{
    AuditSink, AuthState, CallTickets, FixedClock, TicketHolder, TicketRequest,
};
use navree_http::{AppError, Controller, Extension, Json, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::support::{self, PASSWORD, RecordingSink, login, signed_in, tokens};

const RP_ID: &str = "example.com";
const ORIGIN: &str = "https://app.example.com";

/// ₹1,00,000 and ₹100, in paise.
const LAKH: u64 = 10_000_000;
const HUNDRED: u64 = 10_000;

/// The action a payment approval confirms.
fn approval(payment: &str, amount_paise: u64) -> ConfirmAction {
    ConfirmAction::new("payments.approve")
        .field("payment", payment)
        .field("amount_paise", amount_paise)
        .field("payee", "Gurpreet Kaur, HDFC ••4821")
}

/// The payment page (it hands out the `confirm` ticket) and the approval
/// route (it checks the confirmation).
struct PaymentsController {
    auth: Arc<AuthService>,
    tickets: Arc<dyn CallTickets>,
}

impl Injectable for PaymentsController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(PaymentsController {
                auth: r.get::<AuthService>().await?,
                tickets: r.get::<dyn CallTickets>().await?,
            })
        })
    }
}

#[derive(Deserialize)]
struct Amount {
    amount_paise: u64,
    #[serde(default)]
    feature: Option<String>,
}

#[derive(Deserialize)]
struct Approve {
    amount_paise: u64,
    credential: Value,
}

impl Controller for PaymentsController {
    fn prefix(&self) -> &'static str {
        "/payments"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let tickets = self.tickets.clone();
        let page = move |state: Extension<AuthState>,
                         Path(id): Path<String>,
                         Json(input): Json<Amount>| {
            let tickets = tickets.clone();
            async move {
                let request = TicketRequest::new(
                    TicketHolder::of(state.0.0.as_ref()),
                    input.feature.unwrap_or_else(|| "confirm".to_owned()),
                    "POST",
                    &format!("/api/payments/{id}/approve"),
                )
                .binding(approval(&id, input.amount_paise).digest());
                let issued = tickets
                    .issue(&request)
                    .map_err(|error| AuthError::from(error).into_app_error())?;
                Ok::<_, AppError>(Json(json!({ "ticket": issued.ticket })))
            }
        };
        let auth = self.auth.clone();
        let approve = move |claims: AccessClaims,
                            meta: ClientMeta,
                            Path(id): Path<String>,
                            Json(input): Json<Approve>| {
            let auth = auth.clone();
            async move {
                // The details come from the server's own record, never from
                // the confirmation.
                let action = approval(&id, input.amount_paise);
                auth.confirm_action(
                    &claims,
                    &action,
                    ConfirmProof::Passkey(input.credential),
                    &meta,
                )
                .await
                .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(json!({ "approved": id })))
            }
        };
        vec![
            RouteDef::on(Method::POST, "/{id}/confirm-ticket", page).authenticated(),
            RouteDef::on(Method::POST, "/{id}/approve", approve).authenticated(),
        ]
    }
}

struct PaymentsModule;

impl Module for PaymentsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<PaymentsController>();
    }
}

async fn app(clock: &Arc<FixedClock>, sink: &Arc<RecordingSink>) -> TestApp {
    support::clocked(clock)
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", RP_ID)
        .config("auth.passkeys.origins", json!([ORIGIN]))
        .override_value::<dyn AuditSink>(sink.clone())
        .module(PaymentsModule)
        .build()
        .await
}

/// Registers a passkey of `authenticator` for the user of `access`.
async fn register(client: &TestClient, access: &str, authenticator: &VirtualAuthenticator) {
    let options = client
        .post("/api/auth/passkeys/register/options")
        .bearer(access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    client
        .post("/api/auth/passkeys/register")
        .bearer(access)
        .json(&json!({ "credential": authenticator.create(&options["publicKey"]) }))
        .send()
        .await
        .assert_status(201);
}

/// The `confirm` ticket of the page of `payment` for `amount`.
async fn page_ticket(client: &TestClient, access: &str, payment: &str, amount: u64) -> String {
    client
        .post(&format!("/api/payments/{payment}/confirm-ticket"))
        .bearer(access)
        .json(&json!({ "amount_paise": amount }))
        .send()
        .await
        .assert_status(200)
        .json_value()["ticket"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// `POST /api/auth/confirm/options`.
async fn options(client: &TestClient, access: &str, body: Value) -> TestResponse {
    client
        .post("/api/auth/confirm/options")
        .bearer(access)
        .json(&body)
        .send()
        .await
}

/// The challenge options for the page of `payment` at `amount`.
async fn challenge(client: &TestClient, access: &str, payment: &str, amount: u64) -> Value {
    let ticket = page_ticket(client, access, payment, amount).await;
    options(client, access, json!({ "ticket": ticket }))
        .await
        .assert_status(200)
        .json_value()["publicKey"]
        .clone()
}

async fn approve(
    client: &TestClient,
    access: &str,
    payment: &str,
    amount: u64,
    credential: &Value,
) -> TestResponse {
    client
        .post(&format!("/api/payments/{payment}/approve"))
        .bearer(access)
        .json(&json!({ "amount_paise": amount, "credential": credential }))
        .send()
        .await
}

fn refused(response: TestResponse) {
    response.assert_error_code(403, "confirmation_refused");
}

/// A signed-in user with a registered passkey: the access token and the
/// authenticator.
async fn with_passkey(app: &TestApp, email: &str) -> (String, VirtualAuthenticator) {
    let (_, access, _) = signed_in(app, email).await;
    let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN);
    register(&app.client(), &access, &authenticator).await;
    (access, authenticator)
}

#[tokio::test]
async fn a_passkey_confirms_the_action_it_was_asked_for() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (access, phone) = with_passkey(&app, "simran@example.com").await;

    let public_key = challenge(&client, &access, "PAY-2024", LAKH).await;
    assert_eq!(public_key["userVerification"], "required");
    assert_eq!(public_key["rpId"], RP_ID);
    assert_eq!(public_key["timeout"], CONFIRM_TTL_SECS * 1000);
    assert_eq!(
        public_key["allowCredentials"][0]["id"],
        json!(phone.credential_ids()[0])
    );
    let assertion = phone.get(&public_key).unwrap();
    approve(&client, &access, "PAY-2024", LAKH, &assertion)
        .await
        .assert_status(200)
        .assert_json_path("approved", json!("PAY-2024"));

    let confirmed = sink.with_action("auth.confirmed");
    let last = confirmed.last().unwrap();
    assert_eq!(last.metadata["method"], "passkey");
    assert_eq!(last.metadata["action"], "payments.approve");
    assert!(last.metadata["passkey_id"].is_string());
}

#[tokio::test]
async fn a_replayed_assertion_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (access, phone) = with_passkey(&app, "simran@example.com").await;
    let assertion = phone
        .get(&challenge(&client, &access, "PAY-2024", LAKH).await)
        .unwrap();
    approve(&client, &access, "PAY-2024", LAKH, &assertion)
        .await
        .assert_status(200);

    // Its own challenge, after use.
    refused(approve(&client, &access, "PAY-2024", LAKH, &assertion).await);
    // With a fresh challenge waiting: the old assertion names its own.
    challenge(&client, &access, "PAY-2024", LAKH).await;
    refused(approve(&client, &access, "PAY-2024", LAKH, &assertion).await);
    // From another session of the same user.
    let other = tokens(
        &login(&client, "simran@example.com", PASSWORD)
            .await
            .assert_status(200),
    )
    .0;
    refused(approve(&client, &other, "PAY-2024", LAKH, &assertion).await);

    let refusals = sink.with_action("auth.confirmation_refused");
    assert_eq!(refusals.len(), 3);
    assert!(
        refusals
            .iter()
            .all(|e| e.metadata["action"] == "payments.approve")
    );
}

#[tokio::test]
async fn an_assertion_for_a_different_amount_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (access, phone) = with_passkey(&app, "simran@example.com").await;
    // Confirmed for ₹100, posted to the ₹1,00,000 approval.
    let assertion = phone
        .get(&challenge(&client, &access, "PAY-2024", HUNDRED).await)
        .unwrap();
    refused(approve(&client, &access, "PAY-2024", LAKH, &assertion).await);
    // Nor for another payment of the same amount.
    let assertion = phone
        .get(&challenge(&client, &access, "PAY-2024", LAKH).await)
        .unwrap();
    refused(approve(&client, &access, "PAY-2025", LAKH, &assertion).await);
    let reasons: Vec<Value> = sink
        .with_action("auth.confirmation_refused")
        .iter()
        .map(|e| e.metadata["reason"].clone())
        .collect();
    assert_eq!(
        reasons,
        [
            json!("challenge of another action"),
            json!("challenge of another action")
        ]
    );
}

#[tokio::test]
async fn an_assertion_without_user_verification_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (_, access, _) = signed_in(&app, "simran@example.com").await;
    // A security key without a PIN: fine for signing in under `preferred`,
    // never for a confirmation.
    let key = VirtualAuthenticator::new(RP_ID, ORIGIN).without_user_verification();
    register(&client, &access, &key).await;
    let assertion = key
        .get(&challenge(&client, &access, "PAY-2024", LAKH).await)
        .unwrap();
    refused(approve(&client, &access, "PAY-2024", LAKH, &assertion).await);
    assert_eq!(
        sink.with_action("auth.confirmation_refused")[0].metadata["reason"],
        "user not verified"
    );
}

#[tokio::test]
async fn an_assertion_past_its_time_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (access, phone) = with_passkey(&app, "simran@example.com").await;
    let assertion = phone
        .get(&challenge(&client, &access, "PAY-2024", LAKH).await)
        .unwrap();
    clock.advance(Duration::from_secs(CONFIRM_TTL_SECS + 1));
    refused(approve(&client, &access, "PAY-2024", LAKH, &assertion).await);
}

#[tokio::test]
async fn another_users_key_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (access, _phone) = with_passkey(&app, "simran@example.com").await;
    let (_, harpreets_phone) = with_passkey(&app, "harpreet@example.com").await;
    let mut public_key = challenge(&client, &access, "PAY-2024", LAKH).await;
    // Harpreet's authenticator signs Simran's challenge.
    public_key["allowCredentials"] = json!([]);
    let assertion = harpreets_phone.get(&public_key).unwrap();
    refused(approve(&client, &access, "PAY-2024", LAKH, &assertion).await);
    assert_eq!(
        sink.with_action("auth.confirmation_refused")[0].metadata["reason"],
        "a passkey of another user"
    );
}

#[tokio::test]
async fn another_sessions_challenge_is_refused() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (phone_session, phone) = with_passkey(&app, "simran@example.com").await;
    let laptop_session = tokens(
        &login(&client, "simran@example.com", PASSWORD)
            .await
            .assert_status(200),
    )
    .0;
    // The laptop's challenge, answered and posted by the phone's session.
    let assertion = phone
        .get(&challenge(&client, &laptop_session, "PAY-2024", LAKH).await)
        .unwrap();
    refused(approve(&client, &phone_session, "PAY-2024", LAKH, &assertion).await);
    assert_eq!(
        sink.with_action("auth.confirmation_refused")[0].metadata["reason"],
        "challenge of another session"
    );
}

#[tokio::test]
async fn the_challenge_route_takes_a_confirm_ticket_once() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (access, _) = with_passkey(&app, "simran@example.com").await;
    let ticket = page_ticket(&client, &access, "PAY-2024", LAKH).await;
    options(&client, &access, json!({ "ticket": ticket }))
        .await
        .assert_status(200);
    options(&client, &access, json!({ "ticket": ticket }))
        .await
        .assert_error_code(422, "invalid_ticket");
    // Forged, of another call, of another user.
    options(&client, &access, json!({ "ticket": "v1.k.n.c" }))
        .await
        .assert_error_code(422, "invalid_ticket");
    let scan = client
        .post("/api/payments/PAY-2024/confirm-ticket")
        .bearer(&access)
        .json(&json!({ "amount_paise": LAKH, "feature": "scan_code" }))
        .send()
        .await
        .json_value()["ticket"]
        .as_str()
        .unwrap()
        .to_owned();
    options(&client, &access, json!({ "ticket": scan }))
        .await
        .assert_error_code(422, "invalid_ticket");
    let (other, _) = with_passkey(&app, "harpreet@example.com").await;
    let ticket = page_ticket(&client, &access, "PAY-2024", LAKH).await;
    options(&client, &other, json!({ "ticket": ticket }))
        .await
        .assert_error_code(422, "invalid_ticket");
    // Guests get nothing.
    client
        .post("/api/auth/confirm/options")
        .json(&json!({}))
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn password_and_code_still_confirm_an_action() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let (user, access, _) = signed_in(&app, "simran@example.com").await;
    let auth = app.get::<AuthService>().await;
    let claims = support::claims_of(auth.jwt(), &access);
    auth.confirm_action(
        &claims,
        &approval("PAY-2024", LAKH),
        ConfirmProof::Password(PASSWORD.to_owned()),
        &ClientMeta::default(),
    )
    .await
    .unwrap();
    let error = auth
        .confirm_action(
            &claims,
            &approval("PAY-2024", LAKH),
            ConfirmProof::Password("wrong password".to_owned()),
            &ClientMeta::default(),
        )
        .await
        .unwrap_err();
    assert!(
        !matches!(error, AuthError::ConfirmationRefused(_)),
        "{error}"
    );
    let confirmed = sink.with_action("auth.confirmed");
    assert_eq!(confirmed[0].metadata["method"], "password");
    assert_eq!(confirmed[0].metadata["action"], "payments.approve");
    assert_eq!(confirmed[0].actor_id, Some(user.id));
}

#[tokio::test]
async fn confirming_an_action_does_not_make_the_sign_in_recent() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (_, access, refresh) = signed_in(&app, "simran@example.com").await;
    let phone = VirtualAuthenticator::new(RP_ID, ORIGIN);
    register(&client, &access, &phone).await;
    let auth = app.get::<AuthService>().await;
    let claims = support::claims_of(auth.jwt(), &access);
    // Past the probe's ten minutes of recent sign-in, inside the access
    // token's fifteen.
    clock.advance(Duration::from_secs(11 * 60));

    // The payment is approved with the passkey, and another with the
    // password.
    let assertion = phone
        .get(&challenge(&client, &access, "PAY-2024", LAKH).await)
        .unwrap();
    approve(&client, &access, "PAY-2024", LAKH, &assertion)
        .await
        .assert_status(200);
    auth.confirm_action(
        &claims,
        &approval("PAY-2025", HUNDRED),
        ConfirmProof::Password(PASSWORD.to_owned()),
        &ClientMeta::default(),
    )
    .await
    .unwrap();
    assert_eq!(sink.with_action("auth.confirmed").len(), 2);

    // Neither counts as a step-up: a token refreshed afterwards still
    // carries the sign-in of eleven minutes ago.
    let fresh = tokens(
        &support::refresh(&client, &refresh)
            .await
            .assert_status(200),
    )
    .0;
    assert_eq!(
        support::claims_of(auth.jwt(), &fresh).auth_time,
        claims.auth_time
    );
    client
        .post("/api/probe/sensitive")
        .bearer(&fresh)
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn a_passkey_steps_up_through_post_auth_confirm_passkey() {
    let (clock, sink) = (support::clock(), Arc::new(RecordingSink::default()));
    let app = app(&clock, &sink).await;
    let client = app.client();
    let (access, phone) = with_passkey(&app, "simran@example.com").await;
    // Past the probe's ten minutes of recent sign-in, inside the access
    // token's fifteen.
    clock.advance(Duration::from_secs(11 * 60));
    client
        .post("/api/probe/sensitive")
        .bearer(&access)
        .send()
        .await
        .assert_status(403);

    let public_key = options(&client, &access, json!({}))
        .await
        .assert_status(200)
        .json_value()["publicKey"]
        .clone();
    let assertion = phone.get(&public_key).unwrap();
    // A step-up's challenge approves no action.
    refused(approve(&client, &access, "PAY-2024", LAKH, &assertion).await);

    let assertion = phone
        .get(&options(&client, &access, json!({})).await.json_value()["publicKey"])
        .unwrap();
    let confirmed = client
        .post("/api/auth/confirm/passkey")
        .bearer(&access)
        .json(&json!({ "credential": assertion }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let fresh = confirmed["access_token"].as_str().unwrap();
    client
        .post("/api/probe/sensitive")
        .bearer(fresh)
        .send()
        .await
        .assert_status(200);
}
