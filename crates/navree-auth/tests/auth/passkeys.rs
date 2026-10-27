//! Passkeys: registration and sign-in through `/api/auth/passkeys/*`, driven
//! by `VirtualAuthenticator`.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_auth::testing::VirtualAuthenticator;
use navree_contracts::AuditSink;
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::{Value, json};

use crate::support::{self, RecordingSink, enable_two_factor, signed_in, tokens};

const RP_ID: &str = "example.com";
const ORIGIN: &str = "https://app.example.com";

async fn passkey_app(sink: Arc<RecordingSink>) -> TestApp {
    let clock = support::clock();
    support::clocked(&clock)
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", RP_ID)
        .config("auth.passkeys.origins", json!([ORIGIN]))
        .override_value::<dyn AuditSink>(sink)
        .build()
        .await
}

async fn post(client: &TestClient, path: &str, access: Option<&str>, body: Value) -> TestResponse {
    let mut request = client
        .post(&format!("/api/auth/passkeys{path}"))
        .json(&body);
    if let Some(access) = access {
        request = request.bearer(access);
    }
    request.send().await
}

/// Registers a passkey of `authenticator` for the user of `access`.
async fn register(
    client: &TestClient,
    access: &str,
    authenticator: &VirtualAuthenticator,
) -> Value {
    let options = post(client, "/register/options", Some(access), json!({}))
        .await
        .assert_status(200)
        .json_value();
    let credential = authenticator.create(&options["publicKey"]);
    post(
        client,
        "/register",
        Some(access),
        json!({ "name": "Work laptop", "credential": credential }),
    )
    .await
    .assert_status(201)
    .json_value()
}

/// The sign-in options, for `email` when given.
async fn login_options(client: &TestClient, email: Option<&str>) -> Value {
    let body = match email {
        Some(email) => json!({ "email": email }),
        None => json!({}),
    };
    post(client, "/login/options", None, body)
        .await
        .assert_status(200)
        .json_value()["publicKey"]
        .clone()
}

#[tokio::test]
async fn passkey_routes_are_off_by_default() {
    let app = support::app().await;
    post(&app.client(), "/login/options", None, json!({}))
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn a_registered_passkey_signs_the_user_in() {
    let sink = Arc::new(RecordingSink::default());
    let app = passkey_app(sink.clone()).await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN);

    let options = post(&client, "/register/options", Some(&access), json!({}))
        .await
        .assert_status(200)
        .json_value();
    let public_key = &options["publicKey"];
    assert_eq!(public_key["rp"], json!({ "id": RP_ID, "name": "Navree" }));
    assert_eq!(public_key["user"]["name"], "ada@example.com");
    assert_eq!(
        public_key["user"]["id"],
        json!(URL_SAFE_NO_PAD.encode(user.id.as_bytes()))
    );
    assert_eq!(public_key["attestation"], "none");
    assert_eq!(
        public_key["pubKeyCredParams"],
        json!([{"type": "public-key", "alg": -8}, {"type": "public-key", "alg": -7}])
    );
    let credential = authenticator.create(public_key);
    let created = post(
        &client,
        "/register",
        Some(&access),
        json!({ "name": "Work laptop", "credential": credential }),
    )
    .await
    .assert_status(201)
    .assert_json_path("name", json!("Work laptop"))
    .assert_json_path("algorithm", json!("ES256"))
    .assert_json_path("transports", json!(["internal", "hybrid"]))
    .json_value();
    assert!(created.get("public_key").is_none());

    // The same registration again: the challenge is used up.
    post(
        &client,
        "/register",
        Some(&access),
        json!({ "credential": credential }),
    )
    .await
    .assert_error(401, "Invalid passkey");

    let list = client
        .get("/api/auth/passkeys")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(list["data"].as_array().unwrap().len(), 1);

    // Sign in, discoverable (no email) and by email.
    for email in [None, Some("ada@example.com")] {
        let options = login_options(&client, email).await;
        assert_eq!(options["rpId"], RP_ID);
        let assertion = authenticator.get(&options).expect("a passkey");
        let response = post(&client, "/login", None, json!({ "credential": assertion }))
            .await
            .assert_status(200)
            .assert_json_path("user.email", json!("ada@example.com"));
        let (signed_in_access, _) = tokens(&response);
        client
            .get("/api/auth/me")
            .bearer(&signed_in_access)
            .send()
            .await
            .assert_status(200);
    }
    let options = login_options(&client, Some("ada@example.com")).await;
    assert_eq!(
        options["allowCredentials"][0]["id"],
        json!(authenticator.credential_ids()[0])
    );

    assert_eq!(sink.with_action("auth.passkey_registered").len(), 1);
    let passkey_logins = sink
        .with_action("auth.login")
        .into_iter()
        .filter(|entry| entry.metadata["method"] == "passkey")
        .count();
    assert_eq!(passkey_logins, 2);
}

#[tokio::test]
async fn ed25519_passkeys_work_too() {
    let app = passkey_app(Arc::new(RecordingSink::default())).await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN).ed25519();
    let created = register(&client, &access, &authenticator).await;
    assert_eq!(created["algorithm"], "EdDSA");
    let assertion = authenticator
        .get(&login_options(&client, None).await)
        .unwrap();
    post(&client, "/login", None, json!({ "credential": assertion }))
        .await
        .assert_status(200);
}

#[tokio::test]
async fn forged_replayed_and_foreign_assertions_are_refused() {
    let sink = Arc::new(RecordingSink::default());
    let app = passkey_app(sink.clone()).await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN);
    register(&client, &access, &authenticator).await;
    let refused = |assertion: Value| {
        let client = client.clone();
        async move {
            post(&client, "/login", None, json!({ "credential": assertion }))
                .await
                .assert_error(401, "Invalid passkey");
        }
    };

    // Replay: the challenge is single use.
    let assertion = authenticator
        .get(&login_options(&client, None).await)
        .unwrap();
    post(
        &client,
        "/login",
        None,
        json!({ "credential": assertion.clone() }),
    )
    .await
    .assert_status(200);
    refused(assertion).await;

    // A tampered signature.
    let mut tampered = authenticator
        .get(&login_options(&client, None).await)
        .unwrap();
    let mut signature = URL_SAFE_NO_PAD
        .decode(tampered["response"]["signature"].as_str().unwrap())
        .unwrap();
    let last = signature.len() - 1;
    signature[last] ^= 1;
    tampered["response"]["signature"] = json!(URL_SAFE_NO_PAD.encode(signature));
    refused(tampered).await;

    // A challenge the server never issued.
    let invented = authenticator
        .get(&json!({ "challenge": URL_SAFE_NO_PAD.encode(b"made up") }))
        .unwrap();
    refused(invented).await;

    // Another origin, another relying party.
    let phishing = VirtualAuthenticator::new(RP_ID, "https://evil.example.net");
    let other_rp = VirtualAuthenticator::new("evil.example.net", ORIGIN);
    for foreign in [&phishing, &other_rp] {
        let options = post(&client, "/register/options", Some(&access), json!({}))
            .await
            .json_value();
        post(
            &client,
            "/register",
            Some(&access),
            json!({ "credential": foreign.create(&options["publicKey"]) }),
        )
        .await
        .assert_error(401, "Invalid passkey");
    }

    // A passkey nobody registered.
    let stranger = VirtualAuthenticator::new(RP_ID, ORIGIN);
    stranger.create(&json!({ "challenge": "eA", "user": { "id": "eA" } }));
    refused(stranger.get(&login_options(&client, None).await).unwrap()).await;

    let failed = sink.with_action("auth.login_failed");
    assert!(failed.len() >= 4);
    assert!(
        failed
            .iter()
            .all(|entry| entry.metadata["method"] == "passkey")
    );
}

#[tokio::test]
async fn presence_only_passkeys_keep_the_second_factor() {
    let clock = support::clock();
    let app = support::clocked(&clock)
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", RP_ID)
        .config("auth.passkeys.origins", json!([ORIGIN]))
        .build()
        .await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    enable_two_factor(&app, &clock, &user).await;
    let client = app.client();
    let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN).without_user_verification();
    register(&client, &access, &authenticator).await;
    let assertion = authenticator
        .get(&login_options(&client, None).await)
        .unwrap();
    post(&client, "/login", None, json!({ "credential": assertion }))
        .await
        .assert_status(200)
        .assert_json_path("two_factor_required", json!(true));

    // `required` user verification refuses them outright.
    let strict = support::builder()
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", RP_ID)
        .config("auth.passkeys.origins", json!([ORIGIN]))
        .config("auth.passkeys.user_verification", "required")
        .build()
        .await;
    let (_, strict_access, _) = signed_in(&strict, "grace@example.com").await;
    let options = post(
        &strict.client(),
        "/register/options",
        Some(&strict_access),
        json!({}),
    )
    .await
    .json_value();
    post(
        &strict.client(),
        "/register",
        Some(&strict_access),
        json!({ "credential": authenticator.create(&options["publicKey"]) }),
    )
    .await
    .assert_error(401, "Invalid passkey");
}

#[tokio::test]
async fn passkeys_are_removed_by_their_owner_only() {
    let app = passkey_app(Arc::new(RecordingSink::default())).await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let (_, grace_access, _) = signed_in(&app, "grace@example.com").await;
    let client = app.client();
    let authenticator = VirtualAuthenticator::new(RP_ID, ORIGIN);
    let created = register(&client, &access, &authenticator).await;
    let id = created["id"].as_str().unwrap().to_owned();
    client
        .delete(&format!("/api/auth/passkeys/{id}"))
        .bearer(&grace_access)
        .send()
        .await
        .assert_error(404, None);
    client
        .delete(&format!("/api/auth/passkeys/{id}"))
        .bearer(&access)
        .send()
        .await
        .assert_status(204);
    let assertion = authenticator
        .get(&login_options(&client, None).await)
        .unwrap();
    post(&client, "/login", None, json!({ "credential": assertion }))
        .await
        .assert_error(401, "Invalid passkey");
}
