//! Sessions and the sign-in options (`/api/v1/account`).

mod support;

use serde_json::json;
use support::{PASSWORD, boot, sign_in, user};

#[tokio::test]
async fn people_see_and_end_their_sessions() {
    let (app, _guard) = boot().await;
    user(&app, "Ada Lovelace", "ada@example.com").await;
    let laptop = sign_in(&app, "ada@example.com").await;
    let phone = app
        .client()
        .post("/api/auth/login")
        .header("user-agent", "Phone")
        .json(&json!({ "email": "ada@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let phone_refresh = phone["refresh_token"].as_str().expect("a refresh token");

    let sessions = app
        .client()
        .get("/api/v1/account/sessions")
        .bearer(&laptop)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let data = sessions["data"].as_array().expect("sessions");
    assert_eq!(data.len(), 2, "{sessions}");
    let phone_session = data
        .iter()
        .find(|session| session["user_agent"] == "Phone")
        .expect("the phone's session");
    let id = phone_session["id"].as_str().expect("an id");

    // Ending it stops its refresh token.
    app.client()
        .delete(&format!("/api/v1/account/sessions/{id}"))
        .bearer(&laptop)
        .send()
        .await
        .assert_status(204);
    app.client()
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": phone_refresh }))
        .send()
        .await
        .assert_status(401);
    app.client()
        .delete(&format!("/api/v1/account/sessions/{id}"))
        .bearer(&laptop)
        .send()
        .await
        .assert_status(404);

    // Guests have no sessions to see.
    app.client()
        .get("/api/v1/account/sessions")
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn the_sign_in_page_lists_the_configured_providers() {
    let (app, _guard) = boot().await;
    // config/app.toml configures none.
    app.client()
        .get("/api/v1/account/providers")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
}
