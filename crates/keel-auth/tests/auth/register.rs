//! `POST /api/auth/register`.

use keel_auth::{AuthService, user};
use keel_db::Db;
use sea_orm::EntityTrait;
use serde_json::{Value, json};

use crate::support::{self, PASSWORD, tokens};

#[tokio::test]
async fn register_hashes_password_and_returns_tokens() {
    let app = support::app().await;
    let response = app
        .client()
        .post("/api/auth/register")
        .json(
            &json!({ "name": "Grace Hopper", "email": "Grace@Example.com", "password": PASSWORD }),
        )
        .send()
        .await
        .assert_status(201)
        .assert_json_path("user.__type", json!("User"))
        .assert_json_path("user.name", json!("Grace Hopper"))
        .assert_json_path("user.email", json!("grace@example.com"))
        .assert_json_path("user.email_verified_at", Value::Null)
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("expires_in", json!(900));
    let body = response.json_value();
    assert!(body["user"].get("password_hash").is_none(), "{body}");
    assert!(body["user"]["created_at"].is_string(), "{body}");

    let db = app.get::<Db>().await;
    let users = user::Entity::find().all(db.conn()).await.unwrap();
    assert_eq!(users.len(), 1);
    let stored = &users[0];
    assert_eq!(body["user"]["id"], json!(stored.id));
    assert!(
        stored.password_hash.starts_with("$argon2id$"),
        "argon2id PHC string"
    );
    assert!(!stored.password_hash.contains(PASSWORD));

    // The tokens work right away.
    let (access, refresh) = tokens(&response);
    app.client()
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("grace@example.com"));
    support::refresh(&app.client(), &refresh)
        .await
        .assert_status(200);
}

#[tokio::test]
async fn register_rejects_a_taken_email() {
    let app = support::app().await;
    support::create_user(&app, "ada@example.com").await;
    app.client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Ada", "email": "ADA@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_validation_error("email")
        .assert_error(400, "email has already been taken");
}

#[tokio::test]
async fn register_validates_input_and_password_length() {
    let app = support::app().await;
    let client = app.client();
    client
        .post("/api/auth/register")
        .json(&json!({ "name": "Ada", "email": "not-an-email", "password": PASSWORD }))
        .send()
        .await
        .assert_validation_error("email");
    client
        .post("/api/auth/register")
        .json(&json!({ "email": "ada@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_validation_error("name");
    client
        .post("/api/auth/register")
        .json(&json!({ "name": "Ada", "email": "ada@example.com", "password": "short" }))
        .send()
        .await
        .assert_validation_error("password")
        .assert_error(400, "password must be longer than or equal to 8 characters");
    assert!(
        app.get::<AuthService>()
            .await
            .find_by_email("ada@example.com")
            .await
            .unwrap()
            .is_none(),
        "nothing was stored"
    );
}

#[tokio::test]
async fn password_min_length_comes_from_config() {
    let app = support::builder()
        .config("auth.password_min_length", 12)
        .build()
        .await;
    app.client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Ada", "email": "ada@example.com", "password": "elevenchars" }))
        .send()
        .await
        .assert_error(
            400,
            "password must be longer than or equal to 12 characters",
        );
}

#[tokio::test]
async fn registration_disabled_is_404() {
    let app = support::builder()
        .config("auth.registration", false)
        .build()
        .await;
    app.client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Ada", "email": "ada@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_error(404, None);
    // Login still works.
    support::create_user(&app, "ada@example.com").await;
    support::login(&app.client(), "ada@example.com", PASSWORD)
        .await
        .assert_status(200);
}
