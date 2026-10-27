//! Services talking to each other: an HTTP route that sends messages (the
//! context it forwards, the errors it maps), two apps over TCP with
//! forwarded credentials, hybrid `serve`, `micro:listen`, the commands,
//! named clients, the readiness check and the boot errors.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::extract::Path;
use http::header::AUTHORIZATION;
use http::request::Parts;
use navree_access::AccessModule;
use navree_contracts::{HealthCheck, HealthStatus, Principal};
use navree_db::DatabaseModule;
use navree_http::{AppError, Authenticator, HttpServer, Json, ModuleBuilderHttpExt, Result};
use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_macros::{controller, injectable, message_handlers};
use navree_micro::{
    ClientOptions, Delivery, Envelope, MemoryTransport, MessageHandlers, MicroError, MicroFake,
    MicroModule, MicroOptions, MicroTransport, ModuleBuilderMicroExt, PatternDef, Payload, Reply,
    ServiceClient, Subscription, TcpOptions, TcpTransport, Transport, headers,
};
use navree_testing::{TestApp, principal};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::support::{DealView, DealsModule, GetDeal, access, insert_deal};

// ---------------------------------------------------------------------------
// Fixtures

/// `Authorization: Bearer <uuid>` is that user: a stand-in for the JWTs
/// navree-auth verifies in every service.
#[derive(Debug)]
struct UuidBearer;

impl Authenticator for UuidBearer {
    fn authenticate<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let user = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|token| token.parse::<Uuid>().ok());
        Box::pin(async move { Ok(user.map(principal)) })
    }
}

/// Registers [`UuidBearer`] as the app's `dyn Authenticator`.
struct BearerAuthModule;

impl Module for BearerAuthModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.factory(Factory::<dyn Authenticator>::new(|_| async {
            Ok(Arc::new(UuidBearer) as Arc<dyn Authenticator>)
        }))
        .exports::<dyn Authenticator>();
    }
}

/// The API's routes, which call the deals service.
#[injectable]
pub struct GatewayController {
    client: Arc<ServiceClient>,
}

#[controller("/gateway")]
impl GatewayController {
    #[get("/deals/{id}")]
    #[public]
    async fn deal(&self, Path(id): Path<Uuid>) -> Result<Json<Value>> {
        let deal: Value = self.client.send("deals.get", &GetDeal { id }).await?;
        Ok(Json(deal))
    }

    #[get("/whoami")]
    #[public]
    async fn whoami(&self) -> Result<Json<Uuid>> {
        Ok(Json(self.client.send("deals.whoami", &()).await?))
    }

    #[get("/anonymous")]
    #[public]
    async fn anonymous(&self) -> Result<Json<Value>> {
        Ok(Json(self.client.send("deals.echo", &()).without_auth().await?))
    }

    #[get("/as-service")]
    #[public]
    async fn as_service(&self) -> Result<Json<Value>> {
        Ok(Json(self.client.send("deals.echo", &()).bearer("svc-token").await?))
    }
}

/// The API: routes only.
struct GatewayModule;

impl Module for GatewayModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<GatewayController>();
    }
}

/// A free local port.
fn free_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    listener.local_addr().expect("its address").to_string()
}

/// A request envelope with a deadline.
fn request(pattern: &str, payload: &str) -> Envelope {
    let mut envelope = Envelope::new(pattern, payload.to_owned())
        .with_header(headers::MESSAGE_ID, Uuid::now_v7().to_string())
        .with_header(headers::CONTENT_TYPE, "application/json");
    envelope.set_deadline(SystemTime::now() + Duration::from_secs(5));
    envelope
}

/// A TCP client that routes `deals.*` to `addr`.
fn tcp_client(addr: &str) -> TcpTransport {
    let options = TcpOptions {
        routes: BTreeMap::from([("deals.*".to_owned(), addr.to_owned())]),
        ..TcpOptions::default()
    };
    TcpTransport::new(&options, None, 1 << 20, Duration::from_secs(1)).expect("a TCP client")
}

/// Sends `deals.echo` until a listener answers (it starts in the
/// background), for up to two seconds.
async fn echo_until_answered(client: &TcpTransport) -> Result<Envelope, MicroError> {
    let mut last = None;
    for _ in 0..100 {
        match client.request(request("deals.echo", "{}")).await {
            Ok(reply) => return Ok(reply),
            Err(error) => last = Some(error),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(last.expect("an attempt"))
}

/// Two apps on one broker: the deals service (`deals`) answers the API's
/// route (`crm-api`) with the user's forwarded credentials, and its
/// refusals come back unchanged. Each broker's tests run it.
#[allow(dead_code)]
pub(crate) async fn deals_answer_the_api_over(deals: MicroOptions, api: MicroOptions) {
    let deals_app = TestApp::builder()
        .temp_database()
        .module(BearerAuthModule)
        .module(DatabaseModule)
        .module(AccessModule::for_root(access()))
        .module(MicroModule::for_root(MicroOptions {
            service: "deals".into(),
            ..deals
        }))
        .module(DealsModule)
        .build()
        .await;
    assert!(MicroModule::start_listener(deals_app.ctx()).await.unwrap());
    let rep = Uuid::now_v7();
    let deal = insert_deal(&deals_app, rep, "Acme renewal").await;
    let api_app = TestApp::builder()
        .module(BearerAuthModule)
        .module(MicroModule::for_root(MicroOptions {
            service: "crm-api".into(),
            ..api
        }))
        .module(GatewayModule)
        .build()
        .await;
    let path = format!("/api/gateway/deals/{}", deal.id);

    let view: DealView = api_app
        .client()
        .get(&path)
        .bearer(&rep.to_string())
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(view.title, "Acme renewal");
    api_app
        .client()
        .get(&path)
        .bearer(&Uuid::now_v7().to_string())
        .send()
        .await
        .assert_error(403, "Forbidden");
    api_app
        .client()
        .get(&format!("/api/gateway/deals/{}", Uuid::now_v7()))
        .bearer(&rep.to_string())
        .send()
        .await
        .assert_error(404, "Not Found");

    api_app.shutdown().await.unwrap();
    deals_app.shutdown().await.unwrap();
}

/// The deals service on TCP at `addr`, authenticating forwarded bearers.
async fn deals_on_tcp(addr: &str, listen_with_serve: bool) -> TestApp {
    TestApp::builder()
        .temp_database()
        .module(BearerAuthModule)
        .module(DatabaseModule)
        .module(AccessModule::for_root(access()))
        .module(MicroModule::for_root(MicroOptions {
            service: "deals".into(),
            listen_with_serve,
            ..MicroOptions::from(Transport::tcp(addr))
        }))
        .module(DealsModule)
        .build()
        .await
}

// ---------------------------------------------------------------------------
// HTTP routes that send messages

#[tokio::test]
async fn routes_forward_the_request_context_and_map_remote_errors() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .module(MicroModule::for_root(MicroOptions {
            service: "crm-api".into(),
            ..MicroOptions::from(micro.transport())
        }))
        .module(GatewayModule)
        .build()
        .await;
    let user = Uuid::now_v7();
    let id = Uuid::now_v7();
    let path = format!("/api/gateway/deals/{id}");

    micro.on("deals.get", Reply::json(&json!({ "id": id })));
    let response = app
        .client()
        .get(&path)
        .bearer(&user.to_string())
        .header("x-workspace", "acme")
        .header("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(id));
    let sent = &micro.sent_envelopes("deals.get")[0];
    assert_eq!(sent.header("authorization"), Some(format!("Bearer {user}").as_str()));
    assert_eq!(sent.header("x-workspace"), Some("acme"));
    assert_eq!(sent.service(), Some("crm-api"));
    assert_eq!(sent.header("x-request-id"), response.header("x-request-id"), "the request id travels");
    let traceparent = sent.header("traceparent").expect("a traceparent");
    assert!(
        traceparent.starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-") && !traceparent.contains("00f067aa0ba902b7"),
        "the trace continues with a new span: {traceparent}"
    );
    micro.assert_sent("deals.get", |payload: &GetDeal| payload.id == id);

    // A remote 4xx passes through; a 5xx is a 502; no reply is a 504;
    // nobody listening is a 503.
    micro.on("deals.get", Reply::error(AppError::not_found()));
    app.client().get(&path).send().await.assert_error(404, "Not Found");
    micro.on("deals.get", Reply::error(AppError::internal("db down")));
    app.client().get(&path).send().await.assert_error(502, "Bad Gateway");
    micro.on("deals.get", Reply::timeout());
    app.client().get(&path).send().await.assert_error(504, None);
    app.client()
        .get("/api/gateway/whoami")
        .send()
        .await
        .assert_error(503, "Service Unavailable");

    // `.without_auth()` and `.bearer(..)` replace the forwarded credentials.
    micro.on("deals.echo", Reply::json(&json!({})));
    app.client()
        .get("/api/gateway/anonymous")
        .bearer(&user.to_string())
        .send()
        .await
        .assert_status(200);
    app.client()
        .get("/api/gateway/as-service")
        .bearer(&user.to_string())
        .send()
        .await
        .assert_status(200);
    let echoes = micro.sent_envelopes("deals.echo");
    assert_eq!(echoes[0].header("authorization"), None);
    assert_eq!(echoes[1].header("authorization"), Some("Bearer svc-token"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn forward_auth_off_sends_no_credentials() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .module(MicroModule::for_root(MicroOptions {
            forward_auth: false,
            ..MicroOptions::from(micro.transport())
        }))
        .module(GatewayModule)
        .build()
        .await;
    micro.on("deals.get", Reply::json(&json!({})));
    app.client()
        .get(&format!("/api/gateway/deals/{}", Uuid::now_v7()))
        .bearer("token")
        .send()
        .await
        .assert_status(200);
    assert_eq!(micro.sent_envelopes("deals.get")[0].header("authorization"), None);
    app.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// Two apps over TCP

#[tokio::test]
async fn two_services_talk_over_tcp_with_the_users_credentials() {
    let deals_addr = free_addr();
    let deals = deals_on_tcp(&deals_addr, true).await;
    assert!(MicroModule::start_listener(deals.ctx()).await.unwrap());
    assert!(!MicroModule::start_listener(deals.ctx()).await.unwrap(), "started once");
    let rep = Uuid::now_v7();
    let deal = insert_deal(&deals, rep, "Acme renewal").await;

    let api = TestApp::builder()
        .module(BearerAuthModule)
        .module(MicroModule::for_root(MicroOptions {
            service: "crm-api".into(),
            tcp: TcpOptions {
                routes: BTreeMap::from([("deals.*".to_owned(), deals_addr.clone())]),
                ..TcpOptions::default()
            },
            ..MicroOptions::from(Transport::tcp(free_addr()))
        }))
        .module(GatewayModule)
        .build()
        .await;
    let path = format!("/api/gateway/deals/{}", deal.id);

    // The owner reads the deal: the API forwards the token, the deals
    // service authenticates it and checks access.
    let view: DealView = api
        .client()
        .get(&path)
        .bearer(&rep.to_string())
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(view.title, "Acme renewal");
    api.client()
        .get("/api/gateway/whoami")
        .bearer(&rep.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("", json!(rep));

    // Refusals come back unchanged.
    api.client()
        .get(&path)
        .bearer(&Uuid::now_v7().to_string())
        .send()
        .await
        .assert_error(403, "Forbidden");
    api.client().get(&path).send().await.assert_error(401, "Unauthorized");
    api.client()
        .get(&format!("/api/gateway/deals/{}", Uuid::now_v7()))
        .bearer(&rep.to_string())
        .send()
        .await
        .assert_error(404, "Not Found");

    api.shutdown().await.unwrap();
    deals.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// Hybrid apps

#[tokio::test]
async fn serve_starts_the_listener_next_to_the_http_server() {
    let addr = free_addr();
    let app = deals_on_tcp(&addr, true).await;
    let client = tcp_client(&addr);
    let (served, answered) = tokio::join!(app.run_command("serve", &["--addr", "127.0.0.1:0"]), async {
        app.get::<HttpServer>().await.wait_bound().await;
        let answered = echo_until_answered(&client).await;
        app.ctx().shutdown_token().cancel();
        answered
    });
    served.unwrap();
    assert_eq!(answered.unwrap().status(), Some(200));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn listen_with_serve_off_serves_http_only() {
    let addr = free_addr();
    let app = deals_on_tcp(&addr, false).await;
    let client = tcp_client(&addr);
    let (served, outcome) = tokio::join!(app.run_command("serve", &["--addr", "127.0.0.1:0"]), async {
        app.get::<HttpServer>().await.wait_bound().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let outcome = client.request(request("deals.echo", "{}")).await;
        app.ctx().shutdown_token().cancel();
        outcome
    });
    served.unwrap();
    assert!(matches!(outcome, Err(MicroError::Transport(_))), "{outcome:?}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn micro_listen_handles_messages_without_http() {
    let addr = free_addr();
    let app = deals_on_tcp(&addr, false).await;
    let client = tcp_client(&addr);
    let (listened, answered) = tokio::join!(app.run_command("micro:listen", &[]), async {
        let answered = echo_until_answered(&client).await;
        app.ctx().shutdown_token().cancel();
        answered
    });
    listened.unwrap();
    let reply = answered.unwrap();
    let body: Value = serde_json::from_slice(reply.payload()).unwrap();
    assert_eq!(body["pattern"], "deals.echo");
    assert!(app.get::<HttpServer>().await.local_addr().is_none(), "no HTTP port");
    app.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// Commands

#[tokio::test]
async fn the_commands_run_against_the_app() {
    let micro = MicroFake::new();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(AccessModule::for_root(access()))
        .module(MicroModule::for_root(micro.transport()))
        .module(DealsModule)
        .build()
        .await;
    app.run_command("micro:patterns", &[]).await.unwrap();
    app.run_command("micro:patterns", &["--json"]).await.unwrap();
    app.run_command("micro:send", &["deals.echo", r#"{"n":1}"#]).await.unwrap();
    let refused = app.run_command("micro:send", &["deals.whoami", "null"]).await.unwrap_err();
    assert!(refused.to_string().contains("401"), "{refused}");
    app.run_command("micro:emit", &["deal.won", r#"{"id":1}"#]).await.unwrap();
    micro.assert_emitted("deal.won", |event: &Value| event["id"] == 1);
    app.run_command("micro:setup", &[]).await.unwrap();
    app.run_command("micro:failed", &[]).await.unwrap();
    app.run_command("micro:flush", &[]).await.unwrap();
    let usage = app.run_command("micro:send", &[]).await.unwrap_err();
    assert!(usage.to_string().contains("usage: micro:send"), "{usage}");
    app.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// Named clients

#[injectable]
pub struct Invoices {
    #[inject(name = "billing")]
    billing: Arc<ServiceClient>,
}

struct InvoicesModule;

impl Module for InvoicesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.provider::<Invoices>();
    }
}

#[tokio::test]
async fn named_clients_send_through_their_own_transport() {
    let micro = MicroFake::new();
    let billing = MicroFake::new();
    let app = TestApp::builder()
        .module(MicroModule::for_root(MicroOptions {
            clients: BTreeMap::from([(
                "billing".to_owned(),
                ClientOptions {
                    transport: billing.transport(),
                    request_timeout_ms: Some(2_000),
                    ..ClientOptions::default()
                },
            )]),
            ..MicroOptions::from(micro.transport())
        }))
        .module(InvoicesModule)
        .build()
        .await;
    billing.on("billing.charge", Reply::json(&json!({ "charged": true })));
    let invoices = app.get::<Invoices>().await;
    let reply: Value = invoices
        .billing
        .send("billing.charge", &json!({ "cents": 100 }))
        .await
        .unwrap();
    assert_eq!(reply, json!({ "charged": true }));
    billing.assert_sent_times("billing.charge", 1);
    micro.assert_nothing_sent();
    let deadline = billing.sent_envelopes("billing.charge")[0].remaining().unwrap();
    assert!(deadline <= Duration::from_secs(2), "the client's request_timeout_ms");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn from_config_names_its_clients_in_code() {
    let app = TestApp::builder()
        .config("micro.transport", "memory")
        .config("micro.clients.billing.transport", "memory")
        .module(MicroModule::from_config().client("billing"))
        .module(InvoicesModule)
        .build()
        .await;
    let invoices = app.get::<Invoices>().await;
    let outcome = invoices.billing.send::<_, Value>("billing.charge", &()).await;
    assert!(matches!(outcome, Err(MicroError::NoResponders { .. })), "{outcome:?}");
    app.shutdown().await.unwrap();

    // A client named in code without its section fails the boot.
    let error = TestApp::builder()
        .config("micro.transport", "memory")
        .module(MicroModule::from_config().client("billing"))
        .try_build()
        .await
        .unwrap_err();
    assert!(error.to_string().contains("[micro.clients.billing]"), "{error}");
}

// ---------------------------------------------------------------------------
// Readiness

/// A memory broker that says it is disconnected, without wildcards.
#[derive(Clone)]
struct Limited {
    broker: MemoryTransport,
    connected: bool,
}

impl MicroTransport for Limited {
    fn name(&self) -> &'static str {
        "limited"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        self.broker.request(envelope)
    }

    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
        self.broker.publish(envelope)
    }

    fn listen(&self, subscriptions: Vec<Subscription>, deliveries: mpsc::Sender<Delivery>) -> BoxFuture<'_, Result<(), MicroError>> {
        self.broker.listen(subscriptions, deliveries)
    }

    fn supports_wildcards(&self) -> bool {
        false
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn listens_at_boot(&self) -> bool {
        true
    }
}

async fn micro_health(app: &TestApp) -> HealthStatus {
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    let check = checks.iter().find(|check| check.name() == "micro").expect("a micro check");
    check.check().await
}

#[tokio::test]
async fn the_readiness_check_follows_the_connection() {
    for connected in [true, false] {
        let transport = Limited {
            broker: MemoryTransport::new(),
            connected,
        };
        let app = TestApp::builder()
            .module(MicroModule::for_root(Transport::custom(transport)))
            .build()
            .await;
        match (connected, micro_health(&app).await) {
            (true, HealthStatus::Up) => {}
            (false, HealthStatus::Down(reason)) => {
                assert_eq!(reason, "the limited transport is disconnected");
            }
            (_, other) => panic!("connected = {connected}: {other:?}"),
        }
        app.shutdown().await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// Boot errors

/// A second handler for `deals.echo`.
#[injectable]
pub struct EchoAgain;

#[message_handlers]
impl EchoAgain {
    #[message_pattern("deals.echo")]
    #[public]
    async fn echo(&self) -> Result<()> {
        Ok(())
    }
}

/// Hand-written handlers: one without an access declaration, and a
/// wildcard event.
#[injectable]
pub struct HandWritten;

impl MessageHandlers for HandWritten {
    fn patterns(self: Arc<Self>) -> Vec<PatternDef> {
        vec![
            PatternDef::message("hand.open", |Payload(value): Payload<Value>| async move { Json(value) })
                .handler_name("open"),
            PatternDef::event("audit.*", || async { http::StatusCode::NO_CONTENT }).handler_name("audit"),
        ]
    }
}

struct Handlers<T>(std::marker::PhantomData<T>);

impl<T: MessageHandlers + navree_kernel::Injectable> Module for Handlers<T> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.message_handlers::<T>();
    }
}

fn handlers<T>() -> Handlers<T> {
    Handlers(std::marker::PhantomData)
}

#[tokio::test]
async fn mistakes_in_the_handlers_fail_the_boot_naming_them() {
    let error = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(AccessModule::for_root(access()))
        .module(MicroModule::for_root(Transport::memory()))
        .module(DealsModule)
        .module(handlers::<EchoAgain>())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("message pattern `deals.echo` is handled twice"), "{error}");
    assert!(error.contains("DealsHandlers") && error.contains("EchoAgain"), "{error}");

    let error = TestApp::builder()
        .module(MicroModule::for_root(Transport::custom(Limited {
            broker: MemoryTransport::new(),
            connected: true,
        })))
        .module(handlers::<HandWritten>())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("message pattern `hand.open` (`open` of HandWritten") && error.contains("has no access declaration"),
        "{error}"
    );
    assert!(
        error.contains("`audit.*`") && error.contains("the limited transport does not support"),
        "{error}"
    );
}

#[tokio::test]
async fn bad_settings_fail_the_boot_naming_the_key() {
    let error = TestApp::builder()
        .module(MicroModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("micro.transport is required"), "{error}");

    let error = TestApp::builder()
        .config("micro.transport", "sqs")
        .module(MicroModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown transport `sqs`"), "{error}");

    let error = TestApp::builder()
        .config("micro.transport", "memory")
        .config("micro.events.dead_letter", "database")
        .module(MicroModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("needs DatabaseModule"), "{error}");
}

#[cfg(not(feature = "nats"))]
#[tokio::test]
async fn a_transport_without_its_feature_names_the_feature() {
    let error = TestApp::builder()
        .config("micro.transport", "nats")
        .module(MicroModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("needs the `micro-nats` feature"), "{error}");
}
