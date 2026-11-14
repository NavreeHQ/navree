//! `TonicService`: a tonic server built elsewhere, served behind the call
//! pipeline of `#[grpc_service]` methods, in-process with `TestChannel` and
//! over a real socket.

#[path = "support/mod.rs"]
mod support;

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use navree_contracts::AuthState;
use navree_db::Uow;
use navree_grpc::testing::TestChannel;
use navree_grpc::{
    Code, GrpcMethod, GrpcModule, ModuleBuilderTonicExt, Request, Response, Status, Streaming,
    TonicService,
};
use navree_http::{Authenticator, HttpServer};
use navree_kernel::{BoxFuture, Module, ModuleBuilder, ModuleId};
use navree_testing::{TestApp, TestAppBuilder};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::server::NamedService;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;
use tower::ServiceExt;
use uuid::Uuid;

use support::pb::deals::deals_client::DealsClient;
use support::pb::deals::deals_server::{Deals, DealsServer};
use support::pb::deals::{
    Deal, DealEvent, GetDealRequest, ImportDealsResponse, ListDealsRequest, ListDealsResponse,
    MoveDealRequest, WatchDealsRequest,
};
use support::pb::ping::ping_server::{Ping, PingServer};
use support::pb::ping::{PingReply, PingRequest};
use support::{ApiVersion, BearerAuthenticator, DealEvents, ping_app};

const HARPREET: &str = "0192f0c1-0000-7000-8000-0000000000a1";
const MEERA: &str = "0192f0c1-0000-7000-8000-0000000000a2";

/// A deals server written by hand, without `#[grpc_service]`: it counts the
/// calls it served and the units of work it saw committed.
#[derive(Clone, Default)]
struct Ledger {
    calls: Arc<AtomicUsize>,
    commits: Arc<AtomicUsize>,
}

impl Ledger {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn commits(&self) -> usize {
        self.commits.load(Ordering::SeqCst)
    }

    fn served(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[tonic::async_trait]
impl Deals for Ledger {
    async fn get_deal(&self, request: Request<GetDealRequest>) -> Result<Response<Deal>, Status> {
        self.served();
        let owner_id = request
            .extensions()
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id.to_string())
            .unwrap_or_default();
        let title = if Uow::current().is_some() {
            "in a unit of work"
        } else {
            "outside"
        };
        Ok(Response::new(Deal {
            id: request.into_inner().id,
            title: title.to_owned(),
            owner_id,
            ..Deal::default()
        }))
    }

    async fn list_deals(
        &self,
        _request: Request<ListDealsRequest>,
    ) -> Result<Response<ListDealsResponse>, Status> {
        self.served();
        Ok(Response::new(ListDealsResponse::default()))
    }

    async fn move_deal(&self, request: Request<MoveDealRequest>) -> Result<Response<Deal>, Status> {
        self.served();
        let commits = self.commits.clone();
        Uow::current()
            .expect("the method runs in the call's unit of work")
            .after_commit(move || {
                Box::pin(async move {
                    commits.fetch_add(1, Ordering::SeqCst);
                })
            });
        let input = request.into_inner();
        if input.stage == "lost" {
            return Err(Status::aborted("the deal is lost"));
        }
        Ok(Response::new(Deal {
            id: input.id,
            stage: input.stage,
            ..Deal::default()
        }))
    }

    type WatchDealsStream = DealEvents;

    async fn watch_deals(
        &self,
        _request: Request<WatchDealsRequest>,
    ) -> Result<Response<Self::WatchDealsStream>, Status> {
        Err(Status::unimplemented("this ledger is not watched"))
    }

    async fn import_deals(
        &self,
        request: Request<Streaming<Deal>>,
    ) -> Result<Response<ImportDealsResponse>, Status> {
        self.served();
        let mut deals = request.into_inner();
        let mut imported = 0;
        while deals.message().await?.is_some() {
            imported += 1;
        }
        Ok(Response::new(ImportDealsResponse { imported }))
    }

    type EchoStream = DealEvents;

    async fn echo(
        &self,
        request: Request<Streaming<DealEvent>>,
    ) -> Result<Response<Self::EchoStream>, Status> {
        self.served();
        Ok(Response::new(Box::pin(request.into_inner()) as DealEvents))
    }
}

/// The ping service written by hand.
struct PlainPing;

#[tonic::async_trait]
impl Ping for PlainPing {
    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingReply>, Status> {
        Ok(Response::new(PingReply {
            message: request.into_inner().message,
            ..PingReply::default()
        }))
    }
}

/// A layer inside a wrapped server that calls the server, then answers its
/// head as HTTP 503 with no `grpc-status`.
#[derive(Clone)]
struct Http503<S>(S);

impl<S> NamedService for Http503<S> {
    const NAME: &'static str = "crm.deals.v1.Deals";
}

impl<S> tower::Service<http::Request<tonic::body::Body>> for Http503<S>
where
    S: tower::Service<
            http::Request<tonic::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = Infallible,
        > + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
        let server = self.0.clone();
        Box::pin(async move {
            let response = server
                .oneshot(request)
                .await
                .unwrap_or_else(|never| match never {});
            let (mut head, _body) = response.into_parts();
            head.status = http::StatusCode::SERVICE_UNAVAILABLE;
            head.headers.remove("grpc-status");
            Ok(http::Response::from_parts(head, tonic::body::Body::empty()))
        })
    }
}

/// A module that serves one wrapped server; its id names it `Wrapped`.
struct Wrapped<S>(TonicService<S>);

impl<S> Module for Wrapped<S>
where
    S: tower::Service<
            http::Request<tonic::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = Infallible,
        > + NamedService
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    fn id(&self) -> ModuleId {
        ModuleId::named::<Self>("Wrapped")
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.tonic_service(self.0.clone());
    }
}

/// An app with a database, `GrpcModule`, the bearer authenticator and
/// `service`.
fn wrapped<S>(service: TonicService<S>) -> TestAppBuilder
where
    S: tower::Service<
            http::Request<tonic::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = Infallible,
        > + NamedService
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(navree_db::DatabaseModule::from_config())
        .module(GrpcModule::from_config())
        .module(Wrapped(service))
        .override_value::<dyn Authenticator>(Arc::new(BearerAuthenticator))
}

/// The ledger's deals server, wrapped with nothing declared yet.
fn deals(ledger: &Ledger) -> TonicService<DealsServer<Ledger>> {
    TonicService::new(DealsServer::new(ledger.clone()))
}

/// A move of a new deal to `stage`.
fn moving(stage: &str) -> MoveDealRequest {
    MoveDealRequest {
        id: Uuid::now_v7().to_string(),
        stage: stage.to_owned(),
        position: None,
        lost_reason: None,
    }
}

fn get_deal() -> GetDealRequest {
    GetDealRequest {
        id: Uuid::now_v7().to_string(),
    }
}

async fn boot_error(builder: TestAppBuilder) -> String {
    builder
        .try_build()
        .await
        .map(drop)
        .expect_err("the boot must fail")
        .to_string()
}

#[tokio::test]
async fn a_wrapped_tonic_server_runs_the_access_checks_and_denies_undeclared_methods() {
    let ledger = Ledger::default();
    let app = wrapped(
        deals(&ledger)
            .method(GrpcMethod::new("get_deal").public())
            .method(
                GrpcMethod::new("move_deal")
                    .authenticated()
                    .guard(Arc::new(ApiVersion)),
            ),
    )
    .build()
    .await;
    let mut guest = DealsClient::new(TestChannel::new(&app));
    let deal = guest.get_deal(get_deal()).await.unwrap().into_inner();
    assert_eq!(deal.title, "in a unit of work");
    assert!(deal.owner_id.is_empty(), "a guest");

    let calls = ledger.calls();
    let status = guest.move_deal(moving("won")).await.unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    let mut harpreet = DealsClient::new(TestChannel::new(&app).bearer(HARPREET));
    let status = harpreet.move_deal(moving("won")).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied, "the guard");
    assert_eq!(ledger.calls(), calls, "refused before the server ran");
    let mut versioned = DealsClient::new(
        TestChannel::new(&app)
            .bearer(HARPREET)
            .metadata("x-api-version", "2"),
    );
    assert!(versioned.move_deal(moving("won")).await.is_ok());
    assert_eq!(ledger.calls(), calls + 1);
    let deal = versioned.get_deal(get_deal()).await.unwrap().into_inner();
    assert_eq!(deal.owner_id, HARPREET, "the server sees the caller");

    // Neither its own declaration nor a default: refused for everyone,
    // never served.
    let calls = ledger.calls();
    for client in [&mut guest, &mut versioned] {
        let status = client
            .list_deals(ListDealsRequest::default())
            .await
            .unwrap_err();
        assert_eq!(
            (status.code(), status.message()),
            (Code::PermissionDenied, "Forbidden")
        );
    }
    assert_eq!(ledger.calls(), calls);

    // A service default covers the methods without their own declaration.
    let ledger = Ledger::default();
    let app = wrapped(
        deals(&ledger)
            .authenticated()
            .method(GrpcMethod::new("get_deal").public()),
    )
    .build()
    .await;
    let mut guest = DealsClient::new(TestChannel::new(&app));
    let status = guest
        .list_deals(ListDealsRequest::default())
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    assert_eq!(ledger.calls(), 0);
    let mut meera = DealsClient::new(TestChannel::new(&app).bearer(MEERA));
    assert!(meera.list_deals(ListDealsRequest::default()).await.is_ok());
    assert!(guest.get_deal(get_deal()).await.is_ok());
    assert_eq!(ledger.calls(), 2);
}

#[tokio::test]
async fn a_tonic_method_is_matched_by_its_rust_name() {
    // `r#list_deals` is the same declaration, its `r#` set aside.
    for declared in ["list_deals", "r#list_deals"] {
        let ledger = Ledger::default();
        let app = wrapped(deals(&ledger).method(GrpcMethod::new(declared).public()))
            .build()
            .await;
        let mut guest = DealsClient::new(TestChannel::new(&app));
        assert!(
            guest.list_deals(ListDealsRequest::default()).await.is_ok(),
            "{declared}"
        );
        assert_eq!(ledger.calls(), 1, "{declared}");
        let status = guest.get_deal(get_deal()).await.unwrap_err();
        assert_eq!(status.code(), Code::PermissionDenied, "{declared}");
        assert_eq!(ledger.calls(), 1, "{declared}");
    }

    // A name not in Rust form would match no call, and the method would
    // fall to the service default (here, every guest could call it): the
    // boot fails, with no default as with a looser one.
    let error = boot_error(wrapped(
        deals(&Ledger::default()).method(GrpcMethod::new("ListDeals").public()),
    ))
    .await;
    assert!(
        error.contains("gRPC method `ListDeals` of `crm.deals.v1.Deals` (module `Wrapped`) is not a Rust method name: write `list_deals` (the name the generated server trait spells)"),
        "{error}"
    );
    let error = boot_error(wrapped(
        deals(&Ledger::default())
            .public()
            .method(GrpcMethod::new("DeleteDeal").roles(&["munshi"])),
    ))
    .await;
    assert!(
        error.contains("gRPC method `DeleteDeal` of `crm.deals.v1.Deals` (module `Wrapped`) is not a Rust method name: write `delete_deal` (the name the generated server trait spells)"),
        "{error}"
    );
}

#[tokio::test]
async fn a_wrapped_method_commits_its_unit_of_work_only_when_it_answers_ok() {
    let ledger = Ledger::default();
    let app = wrapped(deals(&ledger).authenticated()).build().await;
    let mut harpreet = DealsClient::new(TestChannel::new(&app).bearer(HARPREET));
    harpreet.move_deal(moving("won")).await.unwrap();
    assert_eq!(ledger.commits(), 1);
    let status = harpreet.move_deal(moving("lost")).await.unwrap_err();
    assert_eq!(
        (status.code(), status.message()),
        (Code::Aborted, "the deal is lost")
    );
    assert_eq!(ledger.commits(), 1, "an error status rolls back");
    assert_eq!(ledger.calls(), 2);

    // A head that is not HTTP 200 and has no `grpc-status` rolls back and is
    // answered with the code the gRPC specification gives that HTTP status.
    let ledger = Ledger::default();
    let app = wrapped(
        TonicService::new(Http503(DealsServer::new(ledger.clone()))).authenticated(),
    )
    .build()
    .await;
    let mut harpreet = DealsClient::new(TestChannel::new(&app).bearer(HARPREET));
    let status = harpreet.move_deal(moving("won")).await.unwrap_err();
    assert_eq!(
        (status.code(), status.message()),
        (Code::Unavailable, "HTTP 503")
    );
    assert_eq!(ledger.calls(), 1, "the server ran");
    assert_eq!(ledger.commits(), 0, "and its unit of work rolled back");
}

#[tokio::test]
async fn a_refused_call_is_answered_before_its_messages_are_read() {
    let ledger = Ledger::default();
    let app = wrapped(deals(&ledger).authenticated()).build().await;
    let mut guest = DealsClient::new(TestChannel::new(&app));
    let refused = tokio::time::timeout(
        Duration::from_secs(5),
        guest.import_deals(tokio_stream::pending::<Deal>()),
    )
    .await
    .expect("answered without waiting for a message");
    assert_eq!(refused.unwrap_err().code(), Code::Unauthenticated);
    assert_eq!(ledger.calls(), 0);

    // The same call from a signed-in caller reaches the server.
    let mut harpreet = DealsClient::new(TestChannel::new(&app).bearer(HARPREET));
    let imported = harpreet
        .import_deals(tokio_stream::iter([Deal::default(), Deal::default()]))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(imported.imported, 2);
    assert_eq!(ledger.calls(), 1);
}

/// Runs `serve` while `calls` runs, then stops it.
async fn serving<F: Future<Output = ()>>(app: &TestApp, calls: F) {
    let serve = app.run_command("serve", &[]);
    let calls = async {
        calls.await;
        app.ctx().shutdown_token().cancel();
    };
    let (served, ()) = tokio::join!(serve, calls);
    served.expect("serve stops cleanly");
}

#[tokio::test]
async fn a_wrapped_client_stream_is_not_held_to_the_body_read_pace_once_let_in() {
    let ledger = Ledger::default();
    let app = wrapped(deals(&ledger).authenticated())
        .config("http.addr", "127.0.0.1:0")
        .config("http.shutdown_grace_secs", 1)
        .config("http.server.body_read_timeout_ms", 100)
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let event = |stage: &str| DealEvent {
            deal_id: "1".to_owned(),
            stage: stage.to_owned(),
        };

        // A guest is refused, and the server never sees the stream.
        let status = DealsClient::new(channel.clone())
            .echo(tokio_stream::iter([event("first")]))
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated);
        assert_eq!(ledger.calls(), 0);

        // Harpreet's stream sits silent four times longer than the read pace
        // allows, and stays open.
        let (sender, messages) = mpsc::channel(1);
        let mut request = tonic::Request::new(ReceiverStream::new(messages));
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {HARPREET}").parse().unwrap(),
        );
        let mut echoed = DealsClient::new(channel)
            .echo(request)
            .await
            .unwrap()
            .into_inner();
        sender.send(event("first")).await.unwrap();
        assert_eq!(echoed.message().await.unwrap(), Some(event("first")));
        tokio::time::sleep(Duration::from_millis(400)).await;
        sender.send(event("second")).await.unwrap();
        assert_eq!(echoed.message().await.unwrap(), Some(event("second")));
        drop(sender);
        assert_eq!(echoed.message().await.unwrap(), None, "a clean end");
        assert_eq!(ledger.calls(), 1);
    })
    .await;
}

#[tokio::test]
async fn a_tonic_service_that_cannot_honour_its_declaration_fails_the_boot() {
    let no_declaration = "gRPC service `crm.deals.v1.Deals` in `Wrapped` has no access declaration: add .public(), .authenticated(), .roles(..), .permissions(..) or .can(..) to TonicService, or give each method its own with .method(..) (deny by default)";
    let ledger = Ledger::default();
    for (service, expected) in [
        (deals(&ledger), no_declaration),
        // A guard declares no access.
        (deals(&ledger).guard(Arc::new(ApiVersion)), no_declaration),
        (
            deals(&ledger).method(GrpcMethod::new("get_deal")),
            "gRPC method `get_deal` of `crm.deals.v1.Deals` (module `Wrapped`) has no access declaration: add .public(), .authenticated(), .roles(..), .permissions(..) or .can(..) (deny by default)",
        ),
        (
            deals(&ledger).method(GrpcMethod::new("get_deal").can("read", "Deal", Some("id"))),
            "gRPC method `get_deal` of `crm.deals.v1.Deals` (module `Wrapped`) checks `read Deal` on the record named by the message's `id` field, which a TonicService cannot read: give it a .can(..) without a field, or write the service with #[grpc_service]",
        ),
        (
            deals(&ledger)
                .method(GrpcMethod::new("get_deal").public())
                .method(GrpcMethod::new("get_deal").authenticated()),
            "gRPC method `get_deal` of `crm.deals.v1.Deals` (module `Wrapped`) is declared twice in TonicService; declare it once",
        ),
        (
            deals(&ledger).public().roles(&["munshi"]),
            "gRPC service `crm.deals.v1.Deals` in `Wrapped` is public but also declares public roles:munshi; remove one of them",
        ),
        (
            deals(&ledger).roles(&["munshi"]),
            "gRPC service `crm.deals.v1.Deals` in `Wrapped` declares roles:munshi but no `dyn RouteAuthorizer` is registered (import the access module)",
        ),
    ] {
        let error = boot_error(wrapped(service)).await;
        assert!(error.contains(expected), "{expected}\n{error}");
    }
    assert_eq!(ledger.calls(), 0);

    // Two servers of one name.
    let error = boot_error(
        ping_app().module(Wrapped(TonicService::new(PingServer::new(PlainPing)).public())),
    )
    .await;
    assert!(
        error.contains("gRPC service `navree.test.v1.Ping` is registered twice"),
        "{error}"
    );

    // GrpcModule's own health service would take the name's calls.
    let (_reporter, health) = tonic_health::server::health_reporter();
    let error = boot_error(wrapped(TonicService::new(health.clone()).public())).await;
    assert!(
        error.contains("gRPC service `grpc.health.v1.Health` in `Wrapped` has the name of GrpcModule's own health service; set grpc.health = false to serve your own"),
        "{error}"
    );
    let app = wrapped(TonicService::new(health).public())
        .config("grpc.health", false)
        .build()
        .await;
    let status = HealthClient::new(TestChannel::new(&app))
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await
        .unwrap()
        .into_inner()
        .status;
    assert_eq!(status, ServingStatus::Serving as i32);
}
