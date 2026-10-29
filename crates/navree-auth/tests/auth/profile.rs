//! `PATCH /api/auth/me/password` and `PATCH /api/auth/me`.

use std::sync::Arc;

use chrono::Utc;
use navree_contracts::AuditSink;
use navree_db::Db;
use navree_testing::{TestClient, TestResponse};
use navree_auth::User;
use navree_orm::Model as _;
use serde_json::{Value, json};

use crate::support::{
    self, PASSWORD, RecordingSink, first_url, login, refresh, sent_mail, signed_in, tokens,
};

const NEW_PASSWORD: &str = "a brand new password";

async fn change_password(client: &TestClient, access: &str, body: Value) -> TestResponse {
    client
        .patch("/api/auth/me/password")
        .bearer(access)
        .json(&body)
        .send()
        .await
}

async fn update_profile(client: &TestClient, access: &str, body: Value) -> TestResponse {
    client
        .patch("/api/auth/me")
        .bearer(access)
        .json(&body)
        .send()
        .await
}

#[tokio::test]
async fn update_password_requires_current() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::builder()
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (user, access, refresh_token) = signed_in(&app, "ada@example.com").await;
    let client = app.client();

    change_password(
        &client,
        &access,
        json!({ "current_password": "wrong password", "password": NEW_PASSWORD }),
    )
    .await
    .assert_validation_error("current_password");
    change_password(&client, &access, json!({ "password": NEW_PASSWORD }))
        .await
        .assert_validation_error("current_password");
    change_password(
        &client,
        &access,
        json!({ "current_password": PASSWORD, "password": "short" }),
    )
    .await
    .assert_validation_error("password");
    client
        .patch("/api/auth/me/password")
        .json(&json!({ "current_password": PASSWORD, "password": NEW_PASSWORD }))
        .send()
        .await
        .assert_error(401, None);
    login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_status(200);

    let response = change_password(
        &client,
        &access,
        json!({ "current_password": PASSWORD, "password": NEW_PASSWORD }),
    )
    .await
    .assert_status(200)
    .assert_json_path("token_type", json!("Bearer"));
    let (new_access, new_refresh) = tokens(&response);

    // Every other session is signed out; the caller keeps a fresh one.
    refresh(&client, &refresh_token)
        .await
        .assert_error(401, "Invalid refresh token");
    refresh(&client, &new_refresh).await.assert_status(200);
    client
        .get("/api/auth/me")
        .bearer(&new_access)
        .send()
        .await
        .assert_status(200);
    login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_error(401, "Invalid credentials");
    login(&client, "ada@example.com", NEW_PASSWORD)
        .await
        .assert_status(200);

    let changed = sink.with_action("auth.password_changed");
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert_eq!(changed[0].actor_id, Some(user.id));
    let recorded = serde_json::to_string(&sink.entries()).unwrap();
    assert!(!recorded.contains(NEW_PASSWORD), "{recorded}");
}

#[tokio::test]
async fn update_profile_changes_name_and_email() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::mail_builder()
        .config("auth.verification.base_url", "https://app.example.com")
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    let db = app.get::<Db>().await;
    User::query()
        .where_key(user.id)
        .update(&*db, [User::EMAIL_VERIFIED_AT.set(Some(Utc::now()))])
        .await
        .unwrap();
    let client = app.client();

    update_profile(&client, &access, json!({ "name": "  Ada King  " }))
        .await
        .assert_status(200)
        .assert_json_path("__type", json!("User"))
        .assert_json_path("name", json!("Ada King"))
        .assert_json_path("email", json!("ada@example.com"));
    let me = client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .json_value();
    assert!(me["email_verified_at"].is_string(), "{me}");
    assert!(sent_mail(&app).await.is_empty());

    // The same address in another case changes nothing.
    update_profile(&client, &access, json!({ "email": "ADA@example.com" }))
        .await
        .assert_status(200)
        .assert_json_path("email", json!("ada@example.com"));
    assert!(sent_mail(&app).await.is_empty());

    // A new address must be verified again; the link goes to it, and the
    // old address is told.
    update_profile(
        &client,
        &access,
        json!({ "email": "Ada.King@Example.com", "current_password": PASSWORD }),
    )
    .await
    .assert_status(200)
    .assert_json_path("email", json!("ada.king@example.com"))
    .assert_json_path("email_verified_at", Value::Null);
    let sent = sent_mail(&app).await;
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0].to, vec!["ada.king@example.com".to_owned()]);
    assert_eq!(sent[1].to, vec!["ada@example.com".to_owned()]);
    let link = first_url(sent[0].text.as_deref().unwrap()).unwrap();
    client
        .get(link.strip_prefix("https://app.example.com").unwrap())
        .send()
        .await
        .assert_status(200);
    login(&client, "ada.king@example.com", PASSWORD)
        .await
        .assert_status(200);

    let updated = sink.with_action("auth.profile_updated");
    assert_eq!(updated.len(), 2, "{updated:?}");
    assert_eq!(updated[0].actor_id, Some(user.id));
    assert_eq!(
        updated[0].old_values,
        Some(json!({ "name": "Ada Lovelace" }))
    );
    assert_eq!(updated[0].new_values, Some(json!({ "name": "Ada King" })));
    assert_eq!(
        updated[1].new_values,
        Some(json!({ "email": "ada.king@example.com" }))
    );
}

#[tokio::test]
async fn update_profile_rejects_a_taken_email_and_invalid_input() {
    let app = support::app().await;
    support::create_user(&app, "grace@example.com").await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();

    update_profile(
        &client,
        &access,
        json!({ "email": "Grace@Example.com", "current_password": PASSWORD }),
    )
    .await
    .assert_status(400)
    .assert_json_path("errors.email.0", json!("email has already been taken"));
    update_profile(&client, &access, json!({ "name": "   " }))
        .await
        .assert_validation_error("name");
    update_profile(&client, &access, json!({ "email": "not-an-email" }))
        .await
        .assert_validation_error("email");
    client
        .patch("/api/auth/me")
        .json(&json!({ "name": "Nobody" }))
        .send()
        .await
        .assert_error(401, None);
}

/// A bearer token alone must not move the account to another address
/// (which forgot-password would then turn into a password reset): the
/// current password is required, and the old address is told.
#[tokio::test]
async fn changing_email_requires_current_password() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::mail_builder()
        .config("auth.verification.base_url", "https://app.example.com")
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();

    update_profile(&client, &access, json!({ "email": "mallory@example.com" }))
        .await
        .assert_validation_error("current_password");
    update_profile(
        &client,
        &access,
        json!({ "email": "mallory@example.com", "current_password": "wrong password" }),
    )
    .await
    .assert_validation_error("current_password");
    // A taken address is not revealed without the password either.
    support::create_user(&app, "grace@example.com").await;
    update_profile(&client, &access, json!({ "email": "grace@example.com" }))
        .await
        .assert_validation_error("current_password");
    assert!(sent_mail(&app).await.is_empty());
    client
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "mallory@example.com" }))
        .send()
        .await
        .assert_status(202);
    assert!(
        sent_mail(&app).await.is_empty(),
        "nothing for an unknown address"
    );
    assert_eq!(
        sink.with_action("auth.password_confirmation_failed").len(),
        1
    );

    // A name change alone needs no password.
    update_profile(&client, &access, json!({ "name": "Ada King" }))
        .await
        .assert_status(200);

    update_profile(
        &client,
        &access,
        json!({ "email": "ada.king@example.com", "current_password": PASSWORD }),
    )
    .await
    .assert_status(200)
    .assert_json_path("email", json!("ada.king@example.com"));
    let sent = sent_mail(&app).await;
    let to_old: Vec<_> = sent
        .iter()
        .filter(|mail| mail.to == vec!["ada@example.com".to_owned()])
        .collect();
    assert_eq!(to_old.len(), 1, "{sent:?}");
    let text = to_old[0].text.as_deref().unwrap();
    assert!(text.contains("ada.king@example.com"), "{text}");
    assert!(
        sent.iter()
            .any(|mail| mail.to == vec!["ada.king@example.com".to_owned()]),
        "the verification link goes to the new address: {sent:?}"
    );
}

/// A reset token is keyed by the address it was mailed to: once the user
/// moves to another address it must not reset whoever takes the old one.
#[tokio::test]
async fn an_email_change_cancels_a_pending_reset() {
    let app = support::mail_builder()
        .config("auth.password_reset.url", "https://app.example.com/reset")
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    client
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_status(202);
    let sent = sent_mail(&app).await;
    let link = first_url(sent[0].text.as_deref().unwrap()).unwrap();
    let token = support::link_param(&link, "token").unwrap();

    update_profile(
        &client,
        &access,
        json!({ "email": "ada.king@example.com", "current_password": PASSWORD }),
    )
    .await
    .assert_status(200);
    // Someone else signs up with the freed address.
    support::create_user(&app, "ada@example.com").await;

    for email in ["ada@example.com", "ada.king@example.com"] {
        client
            .post("/api/auth/reset-password")
            .json(&json!({ "email": email, "token": token, "password": NEW_PASSWORD }))
            .send()
            .await
            .assert_error(400, "Invalid or expired password reset token");
    }
    login(&client, "ada@example.com", PASSWORD)
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_password_change_cancels_a_pending_reset() {
    let app = support::mail_builder()
        .config("auth.password_reset.url", "https://app.example.com/reset")
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    client
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_status(202);
    let sent = sent_mail(&app).await;
    let link = first_url(sent[0].text.as_deref().unwrap()).unwrap();
    let token = support::link_param(&link, "token").unwrap();

    change_password(
        &client,
        &access,
        json!({ "current_password": PASSWORD, "password": NEW_PASSWORD }),
    )
    .await
    .assert_status(200);
    client
        .post("/api/auth/reset-password")
        .json(
            &json!({ "email": "ada@example.com", "token": token, "password": "another password" }),
        )
        .send()
        .await
        .assert_error(400, "Invalid or expired password reset token");
}
