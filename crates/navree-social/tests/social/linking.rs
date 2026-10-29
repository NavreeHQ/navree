//! Finding, linking and creating users; two-factor authentication; the
//! accounts routes; events and audit entries.

use std::sync::Arc;

use chrono::Utc;
use navree_auth::{AuthService, JwtService, TokenType, User};
use navree_contracts::AuditSink;
use navree_db::Db;
use navree_events::{EventBus, EventsModule};
use navree_social::{Social, SocialUser, social_account};
use navree_testing::TestApp;
use navree_orm::Model as _;
use serde_json::{Value, json};

use crate::support::{
    PASSWORD, RecordingSink, access_token, callback, create_user, fake_app, redirect, sign_in,
};

fn github(id: &str, email: Option<&str>, verified: bool) -> SocialUser {
    SocialUser::new("github", id)
        .name("Ada Lovelace")
        .email(email, verified)
        .avatar_url(format!("https://avatars.test/{id}.png"))
        .nickname("ada")
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
        github("1", Some("ada@example.com"), true),
    );
    fake.on_code(
        "github",
        "second",
        github("1", Some("ada@new.example.com"), true).name("Ada King"),
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
        Some("ada@new.example.com"),
        "refreshed"
    );
    assert_eq!(rows[0].name.as_deref(), Some("Ada King"));
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
    let ada = create_user(&app, "Ada", "ada@example.com").await;
    fake.on_code(
        "github",
        "code",
        github("583231", Some("ADA@example.com"), true),
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
        login(&app, "ada@example.com").await,
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
    create_user(&app, "Ada", "ada@example.com").await;
    fake.on_code(
        "github",
        "code",
        github("1", Some("ada@example.com"), false),
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
    create_user(&app, "Ada", "ada@example.com").await;
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
    sign_in(&app, "github", "code").await.assert_status(409);
}

#[tokio::test]
async fn a_first_sign_in_creates_a_user_without_a_password() {
    let sink = RecordingSink::default();
    let (app, fake) = fake_app(|b| b.override_value::<dyn AuditSink>(Arc::new(sink.clone()))).await;
    fake.on_code(
        "github",
        "verified",
        github("1", Some("ada@example.com"), true),
    );
    fake.on_code(
        "github",
        "unverified",
        SocialUser::new("github", "2").email(Some("grace@example.com"), false),
    );

    sign_in(&app, "github", "verified")
        .await
        .assert_status(200)
        .assert_json_path("user.name", json!("Ada Lovelace"));
    let grace = sign_in(&app, "github", "unverified")
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        grace["user"]["name"], "grace",
        "the email's local part without a name"
    );
    assert_eq!(grace["user"]["email_verified_at"], Value::Null);

    let all = users(&app).await;
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .any(|u| u.email == "ada@example.com" && u.email_verified_at.is_some())
    );
    assert_eq!(login(&app, "ada@example.com").await, 401, "no password yet");
    let register = sink
        .entries()
        .into_iter()
        .find(|e| e.action == "auth.register")
        .expect("auth.register");
    assert_eq!(register.metadata["provider"], "github");

    // A password set through the reset flow makes it a normal account.
    let ada = all.iter().find(|u| u.email == "ada@example.com").unwrap();
    app.get::<AuthService>()
        .await
        .update_password(ada.id, PASSWORD)
        .await
        .unwrap();
    assert_eq!(login(&app, "ada@example.com").await, 200);
}

#[tokio::test]
async fn registration_follows_auth_registration_unless_set() {
    let (closed, fake) = fake_app(|b| b.config("auth.registration", false)).await;
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
    sign_in(&closed, "github", "code")
        .await
        .assert_error(403, "Registration is closed")
        .assert_error_code(403, "registration_closed");

    let (open, fake) = fake_app(|b| {
        b.config("auth.registration", false)
            .config("social.registration", true)
    })
    .await;
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
    sign_in(&open, "github", "code").await.assert_status(200);

    let (social_off, fake) = fake_app(|b| b.config("social.registration", false)).await;
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
    sign_in(&social_off, "github", "code")
        .await
        .assert_error(403, "Registration is closed");
}

#[tokio::test]
async fn a_new_user_needs_an_email() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code("github", "code", SocialUser::new("github", "1").name("Ada"));
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
        github("1", Some("ada@example.com"), true),
    );
    fake.on_code(
        "github",
        "unverified",
        github("2", Some("ada@acme.test"), false),
    );
    fake.on_code("github", "member", github("3", Some("ada@ACME.test"), true));
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
        github("1", Some("ada@example.com"), true),
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
    let ada = create_user(&app, "Ada", "ada@example.com").await;
    turn_on_two_factor(&app, ada.id).await;
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
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
    let ada = create_user(&app, "Ada", "ada@example.com").await;
    turn_on_two_factor(&app, ada.id).await;
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
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
    let grace = create_user(&app, "Grace", "grace@example.com").await;
    let token = access_token(&app, &grace).await;
    fake.on_code(
        "github",
        "code",
        github("42", Some("grace@github.test"), false),
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
    assert_eq!(linked["email"], "grace@github.test");
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

    // The linked account now signs Grace in, although its email differs.
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
    assert_eq!(login(&app, "grace@example.com").await, 200);
}

#[tokio::test]
async fn a_link_state_works_only_for_its_user() {
    let (app, fake) = fake_app(|b| b).await;
    let grace = create_user(&app, "Grace", "grace@example.com").await;
    let mallory = create_user(&app, "Mallory", "mallory@example.com").await;
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
    let ada = create_user(&app, "Ada", "ada@example.com").await;
    let grace = create_user(&app, "Grace", "grace@example.com").await;
    let (ada_token, grace_token) = (
        access_token(&app, &ada).await,
        access_token(&app, &grace).await,
    );
    fake.on_code("github", "ada", github("1", None, false));
    fake.on_code("github", "other", github("2", None, false));

    let state = link_state(&app, "github", &ada_token).await;
    link(&app, "github", &ada_token, "ada", &state)
        .await
        .assert_status(201);
    // The same account again: fine (refreshed).
    let state = link_state(&app, "github", &ada_token).await;
    link(&app, "github", &ada_token, "ada", &state)
        .await
        .assert_status(201);
    // Ada's GitHub account for Grace: 409.
    let state = link_state(&app, "github", &grace_token).await;
    link(&app, "github", &grace_token, "ada", &state)
        .await
        .assert_error(409, "This GitHub account is linked to another user")
        .assert_error_code(409, "account_already_linked");
    // A second GitHub account for Ada: 409.
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
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
    fake.on_code(
        "google",
        "code",
        SocialUser::new("google", "g-1").email(Some("ada@gmail.test"), true),
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
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
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
    assert_eq!(user.email, "ada@example.com");
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
    let ada = create_user(&app, "Ada", "ada@example.com").await;
    let uow = db.uow(Some(ada.id), None).await;
    let account = social
        .link(&uow, ada.id, &github("1", Some("ada@example.com"), true))
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
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
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
    fake.on_code("github", "code", github("1", Some("ada@example.com"), true));
    let state = crate::support::state(&app, "github").await;
    callback(&app, "github", "", &state)
        .await
        .assert_error(400, "GitHub refused the sign-in: no code");
}
