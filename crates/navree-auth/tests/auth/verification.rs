//! `POST /api/auth/email/verification` and the signed
//! `GET /api/auth/email/verify/{id}/{hash}` link.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_contracts::{AuditSink, Clock, FixedClock};
use navree_db::Db;
use navree_testing::{TestApp, TestClient, TestResponse};
use sea_orm::{ActiveModelTrait, IntoActiveModel, Set};

use crate::support::{self, RecordingSink, first_url, sent_mail, signed_in};

const BASE_URL: &str = "https://app.example.com";

async fn request_link(client: &TestClient, access: &str) -> TestResponse {
    client
        .post("/api/auth/email/verification")
        .bearer(access)
        .send()
        .await
}

/// The link of the last verification mail.
async fn mailed_link(app: &TestApp) -> String {
    let sent = sent_mail(app).await;
    let text = sent
        .last()
        .and_then(|mail| mail.text.clone())
        .expect("a verification mail with a text body");
    first_url(&text).expect("a link in the mail")
}

#[tokio::test]
async fn verify_email_signed_link() {
    let sink = Arc::new(RecordingSink::default());
    let app = support::mail_builder()
        .config("auth.verification.base_url", BASE_URL)
        .override_value::<dyn AuditSink>(sink.clone())
        .build()
        .await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();

    request_link(&client, &access).await.assert_status(202);
    let sent = sent_mail(&app).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, vec!["ada@example.com".to_owned()]);
    let link = mailed_link(&app).await;
    let prefix = format!("{BASE_URL}/api/auth/email/verify/{}/", user.id);
    assert!(link.starts_with(&prefix), "{link}");
    assert!(
        link.contains("expires=") && link.contains("signature="),
        "{link}"
    );
    assert!(
        sent[0]
            .html
            .as_deref()
            .is_some_and(|html| html.contains("signature=")),
        "the HTML body links too"
    );
    let path = link.strip_prefix(BASE_URL).unwrap().to_owned();

    // Tampered links fail the signature check.
    let other = uuid::Uuid::now_v7().to_string();
    client
        .get(&path.replace(&user.id.to_string(), &other))
        .send()
        .await
        .assert_status(401);
    client
        .get(&path.replace("signature=", "signature=00"))
        .send()
        .await
        .assert_status(401);
    let unsigned = path.split('?').next().unwrap();
    client.get(unsigned).send().await.assert_status(401);

    // The link works without a bearer token (it is clicked in a mail).
    client.get(&path).send().await.assert_status(200);
    let me = client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(me["email_verified_at"].is_string(), "{me}");
    // Clicking again is harmless.
    client.get(&path).send().await.assert_status(200);

    // A verified user gets no more mail.
    request_link(&client, &access).await.assert_status(204);
    assert_eq!(sent_mail(&app).await.len(), 1);

    let verified = sink.with_action("auth.email_verified");
    assert_eq!(verified.len(), 1, "{verified:?}");
    assert_eq!(verified[0].actor_id, Some(user.id));
}

#[tokio::test]
async fn verification_link_expires() {
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = support::mail_builder()
        .config("auth.verification.base_url", BASE_URL)
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    request_link(&client, &access).await.assert_status(202);
    let link = mailed_link(&app).await;
    let path = link.strip_prefix(BASE_URL).unwrap();
    clock.advance(Duration::from_secs(60 * 60 + 1));
    client.get(path).send().await.assert_status(401);
}

#[tokio::test]
async fn verification_link_for_a_previous_address_is_rejected() {
    let app = support::mail_builder()
        .config("auth.verification.base_url", BASE_URL)
        .build()
        .await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    let client = app.client();
    request_link(&client, &access).await.assert_status(202);
    let link = mailed_link(&app).await;

    let db = app.get::<Db>().await;
    let mut row = user.into_active_model();
    row.email = Set("ada@new.example.com".to_owned());
    row.update(db.conn()).await.unwrap();

    client
        .get(link.strip_prefix(BASE_URL).unwrap())
        .send()
        .await
        .assert_error(403, "Invalid verification link");
}

#[tokio::test]
async fn verification_needs_a_signed_in_user() {
    let app = support::mail_builder().build().await;
    app.client()
        .post("/api/auth/email/verification")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn verification_link_defaults_to_the_app_url() {
    let app = support::mail_builder()
        .config("app.url", "http://localhost:8080/")
        .build()
        .await;
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    request_link(&app.client(), &access)
        .await
        .assert_status(202);
    let link = mailed_link(&app).await;
    assert!(
        link.starts_with(&format!(
            "http://localhost:8080/api/auth/email/verify/{}/",
            user.id
        )),
        "{link}"
    );
}
