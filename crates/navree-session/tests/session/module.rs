//! Wiring: options at boot, the route table, the extractor outside the
//! module, a missing app key and the audit trail of CSRF failures.

use std::sync::{Arc, Mutex};

use http::Method;
use navree_contracts::{AuditEntry, AuditKind, AuditSink, BoxError, BoxFuture, SessionStore};
use navree_crypto::CryptoModule;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef, route_table};
use navree_kernel::{Factory, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{MemoryStore, Session, SessionModule, SessionOptions};
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{self, COOKIE};

#[tokio::test]
async fn invalid_root_options_fail_the_boot() {
    let error = support::base()
        .module(SessionModule::for_root(SessionOptions {
            lifetime_minutes: 0,
            ..SessionOptions::default()
        }))
        .try_build()
        .await
        .expect_err("lifetime 0 is invalid");
    assert!(
        error.to_string().contains("session.lifetime_minutes"),
        "{error}"
    );
}

#[tokio::test]
async fn invalid_configuration_fails_the_boot() {
    let error = support::base()
        .config("session.same_site", "none")
        .config("session.secure", false)
        .module(SessionModule::from_config())
        .try_build()
        .await
        .expect_err("same_site none needs secure");
    assert!(error.to_string().contains("session.same_site"), "{error}");
}

#[tokio::test]
async fn the_plain_module_reads_the_session_section() {
    let app = support::base()
        .config("session.cookie", "crm_session")
        .module(SessionModule)
        .build()
        .await;
    let options = app.get::<SessionOptions>().await;
    assert_eq!(options.cookie, "crm_session");
    let response = app.client().get("/csrf-cookie").send().await;
    // The cookie carries the `__Host-` prefix (secure, no domain).
    assert!(response.set_cookie("__Host-crm_session").is_some());
    assert!(response.set_cookie("crm_session").is_none());
    assert!(response.set_cookie(COOKIE).is_none());
}

#[tokio::test]
async fn the_csrf_cookie_route_is_public_and_absolute() {
    let app = support::app().await;
    let routes = route_table(app.ctx()).await.unwrap();
    let route = routes
        .iter()
        .find(|r| r.full_path == "/csrf-cookie")
        .expect("the route is mounted without the API prefix");
    assert_eq!(route.method, "GET");
    assert!(route.access.public);
    assert_eq!(route.name.as_deref(), Some("session.csrf-cookie"));
    assert_eq!(route.module, Some("SessionModule"));
}

/// A route taking `Option<Session>` and one taking `Session`.
struct NeedsSession;

impl Injectable for NeedsSession {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(NeedsSession) })
    }
}

impl Controller for NeedsSession {
    fn prefix(&self) -> &'static str {
        "/needs"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "/optional",
                |session: Option<Session>| async move {
                    Json(json!({ "has_session": session.is_some() }))
                },
            )
            .public(),
            RouteDef::on(Method::GET, "/required", |session: Session| async move {
                Json(json!({ "id": session.id() }))
            })
            .public(),
        ]
    }
}

struct NeedsSessionModule;

impl Module for NeedsSessionModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<NeedsSession>();
    }
}

#[tokio::test]
async fn the_extractor_needs_the_session_module() {
    let without = TestApp::builder().module(NeedsSessionModule).build().await;
    without
        .client()
        .get("/api/needs/optional")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("has_session", json!(false));
    without
        .client()
        .get("/api/needs/required")
        .send()
        .await
        .assert_error(500, "Internal server error");

    let with = support::base()
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(NeedsSessionModule)
        .build()
        .await;
    with.client()
        .get("/api/needs/optional")
        .send()
        .await
        .assert_json_path("has_session", json!(true));
    with.client()
        .get("/api/needs/required")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn without_an_app_key_sessions_cannot_be_written() {
    let store = Arc::new(MemoryStore::new());
    let app = TestApp::builder()
        .override_value::<dyn SessionStore>(store.clone())
        .module(CryptoModule)
        .module(support::ProbeModule)
        .module(SessionModule::for_root(SessionOptions::default()))
        .build()
        .await;
    let client = app.client();
    client
        .get("/api/probe/untouched")
        .send()
        .await
        .assert_status(200);
    let response = client
        .post("/api/probe/put")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_error(500, "Internal server error");
    assert!(response.set_cookie(COOKIE).is_none());
    assert!(
        store.is_empty(),
        "nothing is stored without a cookie to find it"
    );
}

/// Provides the `redis` store the way navree-redis does: a `dyn
/// SessionStore` provider named `redis` (memory-backed here).
struct NamedStoreModule(Arc<MemoryStore>);

impl Module for NamedStoreModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let store = self.0.clone();
        m.factory(
            Factory::<dyn SessionStore>::new(move |_| {
                let store = store.clone();
                async move { Ok(store as Arc<dyn SessionStore>) }
            })
            .named("redis"),
        );
    }
}

#[tokio::test]
async fn the_redis_store_is_the_provider_named_redis() {
    let store = Arc::new(MemoryStore::new());
    let app = support::base()
        .config("session.store", "redis")
        .module(NamedStoreModule(store.clone()))
        .module(SessionModule::from_config())
        .build()
        .await;
    let client = app.client();
    let id = support::put(&client, "k", json!(1)).await;
    let record = store
        .load(&id)
        .await
        .unwrap()
        .expect("saved in the named store");
    assert_eq!(record.data["k"], json!(1));
}

#[tokio::test]
async fn the_redis_store_without_its_provider_fails_the_boot() {
    let error = support::base()
        .config("session.store", "redis")
        .module(SessionModule::from_config())
        .try_build()
        .await
        .expect_err("no provider named redis");
    let message = error.to_string();
    assert!(
        message.contains("session.store = \"redis\"") && message.contains("RedisModule"),
        "{message}"
    );
}

/// Collects audit entries.
#[derive(Default)]
struct Recorder(Mutex<Vec<AuditEntry>>);

impl AuditSink for Recorder {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn csrf_failures_are_audited() {
    let recorder = Arc::new(Recorder::default());
    let app = support::base()
        .override_value::<dyn AuditSink>(recorder.clone())
        .module(SessionModule::for_root(SessionOptions::default()))
        .build()
        .await;
    let client = app.client();
    client.csrf_cookie().await;
    let response = client
        .post("/api/probe/write")
        .header("user-agent", "curl/8")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");

    let entries = recorder.0.lock().unwrap().clone();
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = &entries[0];
    assert_eq!(entry.kind, AuditKind::Security);
    assert_eq!(entry.action, "csrf.mismatch");
    assert_eq!(entry.ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(entry.user_agent.as_deref(), Some("curl/8"));
    assert_eq!(entry.request_id.as_deref(), response.header("x-request-id"));
    assert_eq!(entry.metadata["method"], "POST");
    assert_eq!(entry.metadata["path"], "/api/probe/write");
    assert_eq!(entry.metadata["reason"], "missing");
    let text = serde_json::to_string(entry).unwrap();
    assert!(
        !text.contains(&client.cookie("XSRF-TOKEN").unwrap()),
        "no token in the trail"
    );

    client
        .post("/api/probe/write")
        .header("x-xsrf-token", "wrong")
        .send()
        .await
        .assert_status(403);
    let entries = recorder.0.lock().unwrap().clone();
    assert_eq!(entries[1].metadata["reason"], Value::from("mismatch"));
}
