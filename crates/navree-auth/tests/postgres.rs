//! PostgreSQL integration: the auth migrations (the session columns of
//! `refresh_tokens`, `login_tokens`, `passkeys`) and the audit log's
//! `impersonator_id`, exercised through sessions, impersonation, emailed
//! codes and passkeys.
//!
//! Opt-in: build with `--features test-postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway server, for
//! example `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each
//! test creates its own database and drops it at the end.
#![cfg(feature = "test-postgres")]

use navree_access::{AccessModule, AccessOptions, AccessService};
use navree_audit::{AuditFilter, AuditLog, AuditModule};
use navree_auth::testing::VirtualAuthenticator;
use navree_auth::{AuthModule, AuthService};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::{DatabaseModule, Db, DbOptions, Executor, PageQuery, Pool};
use navree_mail::{MailModule, MailOptions, Mailer, TransportConfig};
use navree_testing::TestApp;
use navree_view::{ViewModule, ViewOptions};
use serde_json::{Value, json};
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery";

/// A database of its own on the server of `NAVREE_TEST_POSTGRES_URL`.
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new() -> Option<Self> {
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "NAVREE_TEST_POSTGRES_URL is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            eprintln!("NAVREE_TEST_POSTGRES_URL is not set: skipping the PostgreSQL test");
            return None;
        };
        let name = format!("navree_auth_{}", Uuid::now_v7().simple());
        let conn = admin_pool(&admin).await;
        conn.execute_script(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        conn.close().await;
        let (base, _) = admin.rsplit_once('/').unwrap();
        let url = format!("{base}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let conn = admin_pool(&self.admin).await;
        conn.execute_script(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .await
        .unwrap();
        conn.close().await;
    }
}

/// A connection to the server of the admin URL.
async fn admin_pool(admin: &str) -> Pool {
    Pool::connect(&DbOptions {
        url: admin.to_owned(),
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    .unwrap()
}

async fn app(scratch: &Scratch) -> TestApp {
    TestApp::builder()
        .config("database.url", scratch.url.clone())
        .config("database.migrate_on_boot", true)
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("auth.impersonation.enabled", true)
        .config("auth.otp.enabled", true)
        .config("auth.passkeys.enabled", true)
        .config("auth.passkeys.rp_id", "example.com")
        .config("auth.passkeys.origins", json!(["https://app.example.com"]))
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(AuditModule::from_config())
        .module(ViewModule::for_root(ViewOptions {
            dirs: vec![],
            reload: false,
        }))
        .module(MailModule::for_root(MailOptions {
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
        .module(AuthModule::from_config())
        .build()
        .await
}

async fn sign_in(app: &TestApp, email: &str) -> (Uuid, String, String) {
    let user = app
        .get::<AuthService>()
        .await
        .create_user("Ada Lovelace", email, PASSWORD)
        .await
        .unwrap();
    let body = app
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    (
        user.id,
        body["access_token"].as_str().unwrap().to_owned(),
        body["refresh_token"].as_str().unwrap().to_owned(),
    )
}

#[tokio::test]
async fn sessions_impersonation_and_its_audit_trail_work_on_postgres() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch).await;
    let client = app.client();
    let (agent, agent_access, _) = sign_in(&app, "agent@example.com").await;
    let (ada, ada_access, ada_refresh) = sign_in(&app, "ada@example.com").await;

    // Sessions: a refresh keeps the session columns.
    client
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": ada_refresh }))
        .send()
        .await
        .assert_status(200);
    let sessions = client
        .get("/api/auth/sessions")
        .bearer(&ada_access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(sessions["data"].as_array().unwrap().len(), 1, "{sessions}");
    assert!(sessions["data"][0]["authenticated_at"].is_string());

    // Impersonation, and the audit trail naming the impersonator.
    let db = app.get::<Db>().await;
    let access = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    access.create_role(&uow, "support", None).await.unwrap();
    access
        .grant(&uow, "support", "users.impersonate")
        .await
        .unwrap();
    access.assign_role(&uow, agent, "support").await.unwrap();
    uow.commit().await.unwrap();
    let body = client
        .post(&format!("/api/auth/impersonate/{ada}"))
        .bearer(&agent_access)
        .json(&json!({ "reason": "support ticket" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let impersonation = body["access_token"].as_str().unwrap().to_owned();
    let refresh = body["refresh_token"].as_str().unwrap().to_owned();
    client
        .post("/api/auth/logout")
        .bearer(&impersonation)
        .json(&json!({ "refresh_token": refresh }))
        .send()
        .await
        .assert_status(204);
    let log = app.get::<AuditLog>().await;
    log.flush().await;
    let page = log
        .list(
            &AuditFilter {
                impersonator_id: Some(agent),
                ..AuditFilter::default()
            },
            PageQuery::default(),
        )
        .await
        .unwrap();
    let actions: Vec<&str> = page.data.iter().map(|row| row.action.as_str()).collect();
    assert!(
        actions.contains(&"auth.impersonation_started"),
        "{actions:?}"
    );
    assert!(actions.contains(&"auth.logout"), "{actions:?}");

    scratch.drop().await;
}

#[tokio::test]
async fn emailed_codes_and_passkeys_work_on_postgres() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let app = app(&scratch).await;
    let client = app.client();
    let (_, access, _) = sign_in(&app, "ada@example.com").await;

    // An emailed code: a wrong guess is counted, the right one signs in.
    client
        .post("/api/auth/otp")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_status(202);
    let sent = app.get::<Mailer>().await.array_transport().unwrap().sent();
    let text = sent.last().unwrap().text.clone().unwrap();
    let start = text.find("code: ").unwrap() + "code: ".len();
    let code: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    client
        .post("/api/auth/otp/verify")
        .json(&json!({ "email": "ada@example.com", "code": wrong }))
        .send()
        .await
        .assert_status(401);
    client
        .post("/api/auth/otp/verify")
        .json(&json!({ "email": "ada@example.com", "code": code }))
        .send()
        .await
        .assert_status(200);

    // A passkey: registered, then signing in twice (the counter grows).
    let authenticator = VirtualAuthenticator::new("example.com", "https://app.example.com");
    let options = client
        .post("/api/auth/passkeys/register/options")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    client
        .post("/api/auth/passkeys/register")
        .bearer(&access)
        .json(&json!({ "credential": authenticator.create(&options["publicKey"]) }))
        .send()
        .await
        .assert_status(201);
    for _ in 0..2 {
        let options = client
            .post("/api/auth/passkeys/login/options")
            .json(&json!({ "email": "ada@example.com" }))
            .send()
            .await
            .json_value();
        let assertion = authenticator.get(&options["publicKey"]).unwrap();
        client
            .post("/api/auth/passkeys/login")
            .json(&json!({ "credential": assertion }))
            .send()
            .await
            .assert_status(200);
    }

    scratch.drop().await;
}
