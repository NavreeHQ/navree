//! Finding, linking and creating users; two-factor authentication; the
//! accounts routes; events and audit entries.

use std::sync::Arc;

use chrono::Utc;
use navree_auth::{AuthService, JwtService, TokenType, User};
use navree_contracts::AuditSink;
use navree_db::Db;
use navree_events::{EventBus, EventsModule};
use navree_orm::Model as _;
use navree_social::{Social, SocialUser, social_account};
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{
    PASSWORD, RecordingSink, access_token, callback, create_user, create_verified_user, fake_app,
    redirect, sign_in, verify_email,
};

fn github(id: &str, email: Option<&str>, verified: bool) -> SocialUser {
    SocialUser::new("github", id)
        .name("Anu Lakhotia")
        .email(email, verified)
        .avatar_url(format!("https://avatars.test/{id}.png"))
        .nickname("anu")
}

async fn accounts(app: &TestApp) -> Vec<social_account::Model> {
    let db = app.get::<Db>().await;
    social_account::Model::query()
        .get(&*db)
        .await
        .unwrap()
        .into_models()
}

async fn users(app: &TestApp) -> Vec<navree_auth::User> {
    let db = app.get::<Db>().await;
    User::query().get(&*db).await.unwrap().into_models()
}

async fn login(app: &TestApp, email: &str) -> u16 {
    app.client()
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": PASSWORD }))
        .send()
        .await
        .status()
        .as_u16()
}

#[tokio::test]
async fn a_linked_account_signs_in_its_user() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code(
        "github",
        "first",
        github("1", Some("anu@example.com"), true),
    );
    fake.on_code(
        "github",
        "second",
        github("1", Some("anu@new.example.com"), true).name("Anu Kaur"),
    );
    let first = sign_in(&app, "github", "first").await.assert_status(200);
    let second = sign_in(&app, "github", "second").await.assert_status(200);
    assert_eq!(
        first.json_value()["user"]["id"],
        second.json_value()["user"]["id"]
    );
    assert_eq!(users(&app).await.len(), 1);
    let rows = accounts(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].email.as_deref(),
        Some("anu@new.example.com"),
        "refreshed"
    );
    assert_eq!(rows[0].name.as_deref(), Some("Anu Kaur"));
}

#[tokio::test]
async fn a_verified_email_links_the_existing_user() {
    let sink = RecordingSink::default();
    let (app, fake) = fake_app(|b| {
        b.module(EventsModule::for_root())
            .override_value::<dyn AuditSink>(Arc::new(sink.clone()))
    })
    .await;
    let mut linked = app.get::<EventBus>().await.subscribe_json("social.linked");
    let ada = create_verified_user(&app, "Anu", "anu@example.com").await;
    fake.on_code(
        "github",
        "code",
        github("583231", Some("ANU@example.com"), true),
    );

    let body = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["user"]["id"], json!(ada.id));
    let rows = accounts(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].user_id, ada.id);
    assert_eq!(rows[0].provider, "github");
    assert_eq!(rows[0].provider_user_id, "583231");
    assert_eq!(
        login(&app, "anu@example.com").await,
        200,
        "the password still works"
    );

    let event = linked.try_recv().expect("social.linked was emitted");
    assert_eq!(
        event,
        json!({ "user_id": ada.id, "provider": "github", "provider_user_id": "583231" })
    );
    let actions = sink.actions();
    assert!(actions.contains(&"social.linked".to_owned()), "{actions:?}");
    let login_entry = sink
        .entries()
        .into_iter()
        .find(|e| e.action == "auth.login" && e.metadata["provider"] == "github")
        .expect("auth.login with metadata.provider");
    assert_eq!(login_entry.actor_id, Some(ada.id));
    assert!(
        !serde_json::to_string(
            &sink
                .entries()
                .iter()
                .map(|e| &e.metadata)
                .collect::<Vec<_>>()
        )
        .unwrap()
        .contains("token"),
        "provider tokens are never audited"
    );
}

#[tokio::test]
async fn an_unverified_email_never_links() {
    let (app, fake) = fake_app(|b| b).await;
    create_user(&app, "Anu", "anu@example.com").await;
    fake.on_code(
        "github",
        "code",
        github("1", Some("anu@example.com"), false),
    );
    sign_in(&app, "github", "code")
        .await
        .assert_error(
            409,
            "An account with this email already exists. Sign in, then connect GitHub in your settings.",
        )
        .assert_error_code(409, "email_taken");
    assert!(accounts(&app).await.is_empty());
}

#[tokio::test]
async fn link_by_email_never_asks_the_user_to_connect_from_settings() {
    let (app, fake) = fake_app(|b| b.config("social.link_by_email", "never")).await;
    create_user(&app, "Anu", "anu@example.com").await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    sign_in(&app, "github", "code").await.assert_status(409);
}

#[tokio::test]
async fn an_account_whose_own_address_is_not_verified_is_not_linked() {
    let sink = RecordingSink::default();
    let (app, fake) = fake_app(|b| b.override_value::<dyn AuditSink>(Arc::new(sink.clone()))).await;
    // Registered with a password; nobody has proved the address yet.
    let registered = create_user(&app, "Anu", "anu@example.com").await;
    fake.on_code(
        "github",
        "code",
        github("583231", Some("anu@example.com"), true),
    );
    fake.on_code(
        "github",
        "again",
        github("583231", Some("anu@example.com"), true),
    );

    sign_in(&app, "github", "code")
        .await
        .assert_error(
            409,
            "An account with this email already exists. Sign in, then connect GitHub in your settings.",
        )
        .assert_error_code(409, "email_taken");
    assert!(accounts(&app).await.is_empty());
    assert_eq!(users(&app).await.len(), 1, "no second user is made");
    let actions = sink.actions();
    assert!(!actions.contains(&"auth.login".to_owned()), "{actions:?}");
    let refused = sink
        .entries()
        .into_iter()
        .find(|e| e.action == "social.link_refused")
        .expect("the refusal is audited");
    assert_eq!(refused.actor_id, Some(registered.id));
    assert_eq!(refused.metadata["provider"], "github");
    assert_eq!(refused.metadata["reason"], "email_unverified");

    // Once the address is proved, the same sign-in links the account.
    verify_email(&app, registered.id).await;
    sign_in(&app, "github", "again")
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(registered.id));
    assert_eq!(accounts(&app).await.len(), 1);
}

#[tokio::test]
async fn a_user_made_by_a_sign_in_links_a_second_provider_with_the_same_address() {
    let (app, fake) =
        fake_app(|b| b.config("social.providers.google.client_id", "google-client")).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    fake.on_code(
        "google",
        "code",
        SocialUser::new("google", "g-1").email(Some("anu@example.com"), true),
    );
    let first = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    assert!(first["user"]["email_verified_at"].is_string(), "{first}");
    let second = sign_in(&app, "google", "code")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(first["user"]["id"], second["user"]["id"]);
    assert_eq!(users(&app).await.len(), 1);
    assert_eq!(accounts(&app).await.len(), 2);
}

#[tokio::test]
async fn a_user_made_with_an_address_no_provider_verified_is_not_linked_by_another_provider() {
    let (app, fake) =
        fake_app(|b| b.config("social.providers.google.client_id", "google-client")).await;
    fake.on_code(
        "github",
        "code",
        SocialUser::new("github", "2").email(Some("gauri@example.com"), false),
    );
    fake.on_code(
        "google",
        "code",
        SocialUser::new("google", "g-2").email(Some("gauri@example.com"), true),
    );
    sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .assert_json_path("user.email_verified_at", Value::Null);
    sign_in(&app, "google", "code")
        .await
        .assert_error_code(409, "email_taken");
    assert_eq!(accounts(&app).await.len(), 1);
}

#[tokio::test]
async fn a_signed_in_user_connects_a_provider_whose_address_their_own_account_has_not_verified() {
    let (app, fake) = fake_app(|b| b).await;
    let anu = create_user(&app, "Anu", "anu@example.com").await;
    let token = access_token(&app, &anu).await;
    fake.on_code(
        "github",
        "code",
        github("583231", Some("anu@example.com"), true),
    );
    let state = link_state(&app, "github", &token).await;
    link(&app, "github", &token, "code", &state)
        .await
        .assert_status(201);
    assert_eq!(accounts(&app).await.len(), 1);
}

#[tokio::test]
async fn a_first_sign_in_creates_a_user_without_a_password() {
    let sink = RecordingSink::default();
    let (app, fake) = fake_app(|b| b.override_value::<dyn AuditSink>(Arc::new(sink.clone()))).await;
    fake.on_code(
        "github",
        "verified",
        github("1", Some("anu@example.com"), true),
    );
    fake.on_code(
        "github",
        "unverified",
        SocialUser::new("github", "2").email(Some("gauri@example.com"), false),
    );

    sign_in(&app, "github", "verified")
        .await
        .assert_status(200)
        .assert_json_path("user.name", json!("Anu Lakhotia"));
    let grace = sign_in(&app, "github", "unverified")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        grace["user"]["name"], "gauri",
        "the email's local part without a name"
    );
    assert_eq!(grace["user"]["email_verified_at"], Value::Null);

    let all = users(&app).await;
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .any(|u| u.email == "anu@example.com" && u.email_verified_at.is_some())
    );
    assert_eq!(login(&app, "anu@example.com").await, 401, "no password yet");
    let register = sink
        .entries()
        .into_iter()
        .find(|e| e.action == "auth.register")
        .expect("auth.register");
    assert_eq!(register.metadata["provider"], "github");

    // A password set through the reset flow makes it a normal account.
    let ada = all.iter().find(|u| u.email == "anu@example.com").unwrap();
    app.get::<AuthService>()
        .await
        .update_password(ada.id, PASSWORD)
        .await
        .unwrap();
    assert_eq!(login(&app, "anu@example.com").await, 200);
}

#[tokio::test]
async fn registration_follows_auth_registration_unless_set() {
    let (closed, fake) = fake_app(|b| b.config("auth.registration", false)).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    sign_in(&closed, "github", "code")
        .await
        .assert_error(403, "Registration is closed")
        .assert_error_code(403, "registration_closed");

    let (open, fake) = fake_app(|b| {
        b.config("auth.registration", false)
            .config("social.registration", true)
    })
    .await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    sign_in(&open, "github", "code").await.assert_status(200);

    let (social_off, fake) = fake_app(|b| b.config("social.registration", false)).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    sign_in(&social_off, "github", "code")
        .await
        .assert_error(403, "Registration is closed");
}

#[tokio::test]
async fn a_new_user_needs_an_email() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code("github", "code", SocialUser::new("github", "1").name("Anu"));
    sign_in(&app, "github", "code")
        .await
        .assert_error(400, "GitHub did not share an email address");
}

#[tokio::test]
async fn allowed_email_domains_limit_sign_ins() {
    let (app, fake) = fake_app(|b| b.config("social.allowed_email_domains", "acme.test")).await;
    fake.on_code(
        "github",
        "outsider",
        github("1", Some("anu@example.com"), true),
    );
    fake.on_code(
        "github",
        "unverified",
        github("2", Some("anu@acme.test"), false),
    );
    fake.on_code("github", "member", github("3", Some("anu@ACME.test"), true));
    sign_in(&app, "github", "outsider")
        .await
        .assert_error(403, "Your email address may not sign in here")
        .assert_error_code(403, "sign_in_not_allowed");
    sign_in(&app, "github", "unverified")
        .await
        .assert_status(403);
    sign_in(&app, "github", "member").await.assert_status(200);

    let (app, fake) = fake_app(|b| {
        b.config(
            "social.providers.github.allowed_email_domains",
            json!(["acme.test"]),
        )
    })
    .await;
    fake.on_code(
        "github",
        "outsider",
        github("1", Some("anu@example.com"), true),
    );
    sign_in(&app, "github", "outsider").await.assert_status(403);
}

async fn turn_on_two_factor(app: &TestApp, id: uuid::Uuid) {
    let db = app.get::<Db>().await;
    User::query()
        .where_key(id)
        .update(&*db, [User::TWO_FACTOR_CONFIRMED_AT.set(Some(Utc::now()))])
        .await
        .unwrap();
}

#[tokio::test]
async fn two_factor_users_get_the_login_challenge() {
    let (app, fake) = fake_app(|b| b).await;
    let ada = create_verified_user(&app, "Anu", "anu@example.com").await;
    turn_on_two_factor(&app, ada.id).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    let body = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["two_factor_required"], true);
    assert_eq!(body["expires_in"], 300);
    assert!(body.get("access_token").is_none());
    let challenge = body["challenge_token"].as_str().unwrap();
    let claims = app
        .get::<JwtService>()
        .await
        .verify(challenge, TokenType::TwoFactorChallenge)
        .expect("a two-factor challenge token");
    assert_eq!(claims.sub, ada.id);
    // It is not an access token.
    app.client()
        .get("/api/auth/me")
        .bearer(challenge)
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn two_factor_skip_trusts_the_provider() {
    let (app, fake) = fake_app(|b| b.config("social.two_factor", "skip")).await;
    let ada = create_verified_user(&app, "Anu", "anu@example.com").await;
    turn_on_two_factor(&app, ada.id).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(ada.id));
}

async fn link_state(app: &TestApp, provider: &str, token: &str) -> String {
    redirect(app, provider, "?intent=link", Some(token))
        .await
        .assert_status(200)
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn link(
    app: &TestApp,
    provider: &str,
    token: &str,
    code: &str,
    state: &str,
) -> navree_testing::TestResponse {
    app.client()
        .post(&format!("/api/auth/social/{provider}/link"))
        .bearer(token)
        .json(&json!({ "code": code, "state": state }))
        .send()
        .await
}

#[tokio::test]
async fn users_connect_list_and_disconnect_providers() {
    let (app, fake) = fake_app(|b| {
        b.module(EventsModule::for_root())
            .config("social.providers.google.client_id", "google-client")
    })
    .await;
    let mut unlinked = app
        .get::<EventBus>()
        .await
        .subscribe_json("social.unlinked");
    let grace = create_user(&app, "Gauri", "gauri@example.com").await;
    let token = access_token(&app, &grace).await;
    fake.on_code(
        "github",
        "code",
        github("42", Some("gauri@github.test"), false),
    );

    let empty = app
        .client()
        .get("/api/auth/social/accounts")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(empty, json!([]));
    app.client()
        .get("/api/auth/social/accounts")
        .send()
        .await
        .assert_status(401);

    let state = link_state(&app, "github", &token).await;
    let linked = link(&app, "github", &token, "code", &state)
        .await
        .assert_status(201)
        .json_value();
    assert_eq!(linked["provider"], "github");
    assert_eq!(linked["provider_name"], "GitHub");
    assert_eq!(linked["email"], "gauri@github.test");
    assert_eq!(linked["avatar_url"], "https://avatars.test/42.png");
    assert!(linked["linked_at"].is_string());

    let list = app
        .client()
        .get("/api/auth/social/accounts")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(list.as_array().unwrap().len(), 1);

    // The linked account now signs Gauri in, although its email differs.
    fake.on_code("github", "again", github("42", None, false));
    sign_in(&app, "github", "again")
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(grace.id));

    app.client()
        .delete("/api/auth/social/accounts/github")
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    assert_eq!(
        unlinked.try_recv().expect("social.unlinked"),
        json!({ "user_id": grace.id, "provider": "github", "provider_user_id": "42" })
    );
    app.client()
        .delete("/api/auth/social/accounts/github")
        .bearer(&token)
        .send()
        .await
        .assert_error(404, "GitHub is not connected");
    assert_eq!(login(&app, "gauri@example.com").await, 200);
}

#[tokio::test]
async fn a_link_state_works_only_for_its_user() {
    let (app, fake) = fake_app(|b| b).await;
    let grace = create_user(&app, "Gauri", "gauri@example.com").await;
    let mallory = create_user(&app, "Manjeet", "manjeet@example.com").await;
    let grace_token = access_token(&app, &grace).await;
    let mallory_token = access_token(&app, &mallory).await;
    fake.on_code("github", "code", github("42", None, false));
    let state = link_state(&app, "github", &mallory_token).await;
    link(&app, "github", &grace_token, "code", &state)
        .await
        .assert_error(400, "Invalid or expired state");
    assert!(accounts(&app).await.is_empty());
}

#[tokio::test]
async fn an_account_links_to_one_user_and_a_user_to_one_account_per_provider() {
    let (app, fake) = fake_app(|b| b).await;
    let ada = create_user(&app, "Anu", "anu@example.com").await;
    let grace = create_user(&app, "Gauri", "gauri@example.com").await;
    let (ada_token, grace_token) = (
        access_token(&app, &ada).await,
        access_token(&app, &grace).await,
    );
    fake.on_code("github", "anu", github("1", None, false));
    fake.on_code("github", "other", github("2", None, false));

    let state = link_state(&app, "github", &ada_token).await;
    link(&app, "github", &ada_token, "anu", &state)
        .await
        .assert_status(201);
    // The same account again: fine (refreshed).
    let state = link_state(&app, "github", &ada_token).await;
    link(&app, "github", &ada_token, "anu", &state)
        .await
        .assert_status(201);
    // Anu's GitHub account for Gauri: 409.
    let state = link_state(&app, "github", &grace_token).await;
    link(&app, "github", &grace_token, "anu", &state)
        .await
        .assert_error(409, "This GitHub account is linked to another user")
        .assert_error_code(409, "account_already_linked");
    // A second GitHub account for Anu: 409.
    let state = link_state(&app, "github", &ada_token).await;
    link(&app, "github", &ada_token, "other", &state)
        .await
        .assert_error(409, "GitHub is already connected. Disconnect it first.")
        .assert_error_code(409, "provider_already_connected");
}

#[tokio::test]
async fn the_last_way_to_sign_in_cannot_be_disconnected() {
    let (app, fake) =
        fake_app(|b| b.config("social.providers.google.client_id", "google-client")).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    fake.on_code(
        "google",
        "code",
        SocialUser::new("google", "g-1").email(Some("anu@gmail.test"), true),
    );
    let body = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    let token = body["access_token"].as_str().unwrap().to_owned();
    app.client()
        .delete("/api/auth/social/accounts/github")
        .bearer(&token)
        .send()
        .await
        .assert_error(
            409,
            "Set a password or connect another provider before disconnecting GitHub",
        )
        .assert_error_code(409, "last_sign_in_method");

    let state = link_state(&app, "google", &token).await;
    link(&app, "google", &token, "code", &state)
        .await
        .assert_status(201);
    app.client()
        .delete("/api/auth/social/accounts/github")
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
}

#[tokio::test]
async fn the_service_works_without_the_routes() {
    let (app, fake) = fake_app(|b| b.config("social.routes", false)).await;
    let social = app.get::<Social>().await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    let redirect = social.redirect("github").await.unwrap();
    let callback = navree_social::SocialCallback::new("code", redirect.state.clone());
    let profile = social.user("github", &callback).await.unwrap();
    assert_eq!(profile.id, "1");
    assert_eq!(profile.provider, "github");
    let outcome = social
        .sign_in(&profile, &navree_auth::ClientMeta::default())
        .await
        .unwrap();
    let navree_auth::LoginOutcome::Authenticated { user, tokens } = outcome else {
        panic!("signed in");
    };
    assert_eq!(user.email, "anu@example.com");
    assert!(!tokens.access_token.is_empty());
    assert_eq!(
        social.find_user("github", "1").await.unwrap().map(|u| u.id),
        Some(user.id)
    );
    assert_eq!(social.find_user("github", "2").await.unwrap(), None);
    let accounts = social.accounts(user.id).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(social.providers().unwrap(), ["github"]);

    // Used state.
    assert!(matches!(
        social.user("github", &callback).await,
        Err(navree_social::SocialError::InvalidState)
    ));
    let _ = callback;
    let _ = redirect;
}

#[tokio::test]
async fn link_and_unlink_take_the_unit_of_work() {
    let (app, _fake) = fake_app(|b| b).await;
    let social = app.get::<Social>().await;
    let db = app.get::<Db>().await;
    let ada = create_user(&app, "Anu", "anu@example.com").await;
    let uow = db.uow(Some(ada.id), None).await;
    let account = social
        .link(&uow, ada.id, &github("1", Some("anu@example.com"), true))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(account.provider, "github");
    assert_eq!(social.accounts(ada.id).await.unwrap(), [account]);

    // A rolled-back unlink keeps the account.
    let uow = db.uow(Some(ada.id), None).await;
    social.unlink(&uow, ada.id, "github").await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(social.accounts(ada.id).await.unwrap().len(), 1);

    let uow = db.uow(Some(ada.id), None).await;
    social.unlink(&uow, ada.id, "github").await.unwrap();
    uow.commit().await.unwrap();
    assert!(social.accounts(ada.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn deleting_a_user_deletes_their_accounts() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    let body = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    let id: uuid::Uuid = serde_json::from_value(body["user"]["id"].clone()).unwrap();
    let db = app.get::<Db>().await;
    User::query().where_key(id).delete(&*db).await.unwrap();
    assert!(accounts(&app).await.is_empty());
}

#[tokio::test]
async fn callbacks_are_answered_with_provider_errors_mapped() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code("github", "code", github("1", Some("anu@example.com"), true));
    let state = crate::support::state(&app, "github").await;
    callback(&app, "github", "", &state)
        .await
        .assert_error(400, "GitHub refused the sign-in: no code");
}
