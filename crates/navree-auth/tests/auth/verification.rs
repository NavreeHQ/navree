//! `POST /api/auth/email/verification` and the signed
//! `GET /api/auth/email/verify/{id}/{hash}` link.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_auth::{AuthError, AuthService, ClientMeta, User};
use navree_contracts::{AuditSink, Clock, FixedClock};
use navree_crypto::sha256_hex;
use navree_db::Db;
use navree_orm::Model as _;
use navree_testing::{TestApp, TestClient, TestResponse};

use crate::support::{self, RecordingSink, first_url, sent_mail, signed_in};

const BASE_URL: &str = "https://app.example.com";

/// A clock that runs a hook the first time it is read after
/// [`arm`](HookedClock::arm). Verifying an address first reads the clock
/// between checking the link and the write that marks the address, so the
/// hook runs between the two.
#[derive(Default)]
struct HookedClock(Mutex<Option<Box<dyn FnOnce() + Send>>>);

impl HookedClock {
    /// Runs `hook` at the next reading, which waits for it.
    fn arm(&self, hook: impl FnOnce() + Send + 'static) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(hook));
    }
}

impl Clock for HookedClock {
    fn now(&self) -> DateTime<Utc> {
        let hook = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(hook) = hook {
            hook();
        }
        Utc::now()
    }
}

/// Arms `clock` to run `change` on user `id`'s row (through the app's
/// database, in a statement of its own) when the service next reads it.
fn change_on_next_reading(
    clock: &HookedClock,
    db: Arc<Db>,
    id: uuid::Uuid,
    change: navree_orm::Assignment,
) {
    let handle = tokio::runtime::Handle::current();
    clock.arm(move || {
        tokio::task::block_in_place(|| {
            handle.block_on(async move {
                User::query()
                    .where_key(id)
                    .update(&*db, [change])
                    .await
                    .expect("the row changes");
            });
        });
    });
}

/// Verifies `user`'s address as its link does, in a task of its own (a
/// worker thread, where the clock's hook may block).
async fn verify(app: &TestApp, user: &User) -> Result<User, AuthError> {
    let service = app.get::<AuthService>().await;
    let (id, hash) = (user.id, sha256_hex(user.email.as_bytes()));
    tokio::spawn(async move {
        service
            .verify_email(id, &hash, &ClientMeta::default())
            .await
    })
    .await
    .expect("the verification finishes")
}

/// The account `id` as it is stored now.
async fn stored(app: &TestApp, id: uuid::Uuid) -> User {
    let db = app.get::<Db>().await;
    User::find(&*db, id)
        .await
        .unwrap()
        .expect("the account")
        .into_inner()
}

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
    let (user, access, _) = signed_in(&app, "anu@example.com").await;
    let client = app.client();

    request_link(&client, &access).await.assert_status(202);
    let sent = sent_mail(&app).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, vec!["anu@example.com".to_owned()]);
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
    let (_, access, _) = signed_in(&app, "anu@example.com").await;
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
    let (user, access, _) = signed_in(&app, "anu@example.com").await;
    let client = app.client();
    request_link(&client, &access).await.assert_status(202);
    let link = mailed_link(&app).await;

    let db = app.get::<Db>().await;
    User::query()
        .where_key(user.id)
        .update(&*db, [User::EMAIL.set("anu@new.example.com".to_owned())])
        .await
        .unwrap();

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
    let (user, access, _) = signed_in(&app, "anu@example.com").await;
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

/// The account moves to another address after its link is checked and
/// before the write that marks it: the write answers as an invalid link and
/// marks nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verification_racing_an_address_change_proves_nothing() {
    let clock = Arc::new(HookedClock::default());
    let app = support::builder()
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let user = support::create_user(&app, "harpreet@sutlej.in").await;
    change_on_next_reading(
        &clock,
        app.get::<Db>().await,
        user.id,
        User::EMAIL.set("simran@sutlej.in".to_owned()),
    );

    let verified = verify(&app, &user).await;
    assert!(
        matches!(verified, Err(AuthError::InvalidVerificationLink)),
        "{verified:?}"
    );
    let account = stored(&app, user.id).await;
    assert_eq!(account.email, "simran@sutlej.in");
    assert_eq!(account.email_verified_at, None);
}

/// The positive side: a write between the check and the mark that leaves
/// the address alone (a new name) does not stop the link verifying it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ordinary_verification_still_verifies() {
    let clock = Arc::new(HookedClock::default());
    let app = support::builder()
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let user = support::create_user(&app, "harpreet@sutlej.in").await;
    change_on_next_reading(
        &clock,
        app.get::<Db>().await,
        user.id,
        User::NAME.set("Harpreet Kaur".to_owned()),
    );

    let verified = verify(&app, &user).await.expect("the address is verified");
    assert_eq!(verified.email, "harpreet@sutlej.in");
    assert_eq!(verified.name, "Harpreet Kaur");
    assert!(verified.email_verified_at.is_some());
    // Following the link again is harmless.
    let again = verify(&app, &user).await.expect("still verified");
    assert_eq!(again.email_verified_at, verified.email_verified_at);
    assert_eq!(stored(&app, user.id).await, again);
}
