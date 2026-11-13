//! `POST /api/auth/register`.

use navree_auth::{AuthService, User};
use navree_db::Db;
use navree_orm::Model as _;
use serde_json::{Value, json};

use crate::support::{self, PASSWORD, tokens};

#[tokio::test]
async fn register_hashes_password_and_returns_tokens() {
    let app = support::app().await;
    let response = app
        .client()
        .post("/api/auth/register")
        .json(
            &json!({ "name": "Gauri Hundal", "email": "Gauri@Example.com", "password": PASSWORD }),
        )
        .send()
        .await
        .assert_status(201)
        .assert_json_path("user.__type", json!("User"))
        .assert_json_path("user.name", json!("Gauri Hundal"))
        .assert_json_path("user.email", json!("gauri@example.com"))
        .assert_json_path("user.email_verified_at", Value::Null)
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("expires_in", json!(900));
    let body = response.json_value();
    assert!(body["user"].get("password_hash").is_none(), "{body}");
    assert!(body["user"]["created_at"].is_string(), "{body}");

    let db = app.get::<Db>().await;
    let users = User::all(&*db).await.unwrap().into_vec();
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
        .assert_json_path("email", json!("gauri@example.com"));
    support::refresh(&app.client(), &refresh)
        .await
        .assert_status(200);
}

#[tokio::test]
async fn register_rejects_a_taken_email() {
    let app = support::app().await;
    support::create_user(&app, "anu@example.com").await;
    app.client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Anu", "email": "ANU@example.com", "password": PASSWORD }))
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
        .json(&json!({ "name": "Anu", "email": "not-an-email", "password": PASSWORD }))
        .send()
        .await
        .assert_validation_error("email");
    client
        .post("/api/auth/register")
        .json(&json!({ "email": "anu@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_validation_error("name");
    client
        .post("/api/auth/register")
        .json(&json!({ "name": "Anu", "email": "anu@example.com", "password": "short" }))
        .send()
        .await
        .assert_validation_error("password")
        .assert_error(400, "password must be longer than or equal to 8 characters");
    assert!(
        app.get::<AuthService>()
            .await
            .find_by_email("anu@example.com")
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
        .json(&json!({ "name": "Anu", "email": "anu@example.com", "password": "elevenchars" }))
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
        .json(&json!({ "name": "Anu", "email": "anu@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_error(404, None);
    // Login still works.
    support::create_user(&app, "anu@example.com").await;
    support::login(&app.client(), "anu@example.com", PASSWORD)
        .await
        .assert_status(200);
}

// One client may ask for five accounts a minute; the sixth waits, which also slows a walk
// through addresses asking which have accounts.
#[tokio::test]
async fn register_is_throttled_per_client() {
    let app = support::app().await;
    let client = app.client();
    for n in 0..5 {
        client
            .post("/api/auth/register")
            .json(&json!({
                "name": "Simran Kaur",
                "email": format!("simran{n}@example.com"),
                "password": "correct horse battery",
            }))
            .send()
            .await
            .assert_status(201);
    }
    client
        .post("/api/auth/register")
        .json(&json!({
            "name": "Simran Kaur",
            "email": "simran5@example.com",
            "password": "correct horse battery",
        }))
        .send()
        .await
        .assert_status(429);
}
