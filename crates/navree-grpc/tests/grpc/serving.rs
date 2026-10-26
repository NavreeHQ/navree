//! Health, reflection, boot checks, and serving over real sockets: on the
//! HTTP port (h2c next to the routes) and on a listener of its own.

use std::sync::Arc;
use std::time::Duration;

use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{HealthCheck, HealthStatus};
use navree_grpc::testing::TestChannel;
use navree_grpc::{
    Code, GrpcMethod, GrpcModule, GrpcServer, GrpcService, ModuleBuilderGrpcExt, Request, Response,
    ServiceLimits, Status,
};
use navree_http::{HttpModule, HttpServer};
use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder, Navree};
use navree_macros::injectable;
use navree_testing::TestApp;
use tonic::service::Routes;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;
use tonic_reflection::pb::v1::ServerReflectionRequest;
use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;

use crate::support::pb::ping::ping_client::PingClient;
use crate::support::pb::ping::ping_server::{Ping, PingServer};
use crate::support::pb::ping::{PingReply, PingRequest};
use crate::support::{GuardedPingModule, PingModule, deals_app, ping_app};

fn ping(message: &str) -> PingRequest {
    PingRequest {
        message: message.to_owned(),
        sleep_ms: 0,
    }
}

/// A readiness check that is always down.
struct Down;

impl HealthCheck for Down {
    fn name(&self) -> &str {
        "storage"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async { HealthStatus::Down("unreachable".into()) })
    }
}

struct DownModule;

impl Module for DownModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(Factory::<dyn HealthCheck>::new(|_r| async {
            Ok(Arc::new(Down) as Arc<dyn HealthCheck>)
        }));
    }
}

async fn health_of(app: &TestApp, service: &str) -> Result<i32, Status> {
    let mut health = HealthClient::new(TestChannel::new(app));
    health
        .check(HealthCheckRequest {
            service: service.to_owned(),
        })
        .await
        .map(|response| response.into_inner().status)
}

#[tokio::test]
async fn health_answers_from_the_readiness_checks() {
    let app = deals_app().build().await;
    assert_eq!(
        health_of(&app, "").await.unwrap(),
        ServingStatus::Serving as i32
    );
    assert_eq!(
        health_of(&app, "crm.deals.v1.Deals").await.unwrap(),
        ServingStatus::Serving as i32
    );
    assert_eq!(
        health_of(&app, "nope.Service").await.unwrap_err().code(),
        Code::NotFound
    );

    let down = deals_app().module(DownModule).build().await;
    assert_eq!(
        health_of(&down, "").await.unwrap(),
        ServingStatus::NotServing as i32
    );

    let off = deals_app().config("grpc.health", false).build().await;
    assert_eq!(
        health_of(&off, "").await.unwrap_err().code(),
        Code::Unimplemented
    );
}

async fn reflected_services(app: &TestApp) -> Result<Vec<String>, Status> {
    let mut reflection = ServerReflectionClient::new(TestChannel::new(app));
    let request = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    };
    let mut responses = reflection
        .server_reflection_info(tokio_stream::iter([request]))
        .await?
        .into_inner();
    let response = responses.message().await?.expect("one response");
    match response.message_response {
        Some(MessageResponse::ListServicesResponse(list)) => {
            Ok(list.service.into_iter().map(|s| s.name).collect())
        }
        other => panic!("unexpected reflection response {other:?}"),
    }
}

#[tokio::test]
async fn reflection_lists_the_registered_descriptors() {
    let app = deals_app().config("grpc.reflection", true).build().await;
    let services = reflected_services(&app).await.unwrap();
    for name in [
        "crm.deals.v1.Deals",
        "navree.test.v1.Ping",
        "grpc.health.v1.Health",
        "grpc.reflection.v1.ServerReflection",
    ] {
        assert!(services.iter().any(|s| s == name), "{name} in {services:?}");
    }

    // From the environment, as APP_GRPC__REFLECTION=false.
    let off = deals_app()
        .sources(ConfigSources::new().env_vars("APP_", [("APP_GRPC__REFLECTION", "false")]))
        .build()
        .await;
    assert!(!off.get::<GrpcServer>().await.options().reflection);
    assert_eq!(
        reflected_services(&off).await.unwrap_err().code(),
        Code::Unimplemented
    );
}

#[tokio::test]
async fn the_server_lists_its_services() {
    let app = deals_app().build().await;
    assert_eq!(
        app.get::<GrpcServer>().await.services(),
        ["crm.deals.v1.Deals"]
    );
}

/// A service written without the macro: the escape hatch, whose methods
/// skip Navree's pipeline but still declare their access for the boot check.
#[injectable]
struct HandWritten;

#[tonic::async_trait]
impl Ping for HandWritten {
    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingReply>, Status> {
        Ok(Response::new(PingReply {
            message: request.into_inner().message,
            ..PingReply::default()
        }))
    }
}

impl GrpcService for HandWritten {
    const NAME: &'static str = "navree.test.v1.Ping";

    fn methods(self: &Arc<Self>) -> Vec<GrpcMethod> {
        vec![GrpcMethod::new("ping")]
    }

    fn add_to(self: Arc<Self>, routes: Routes, _limits: &ServiceLimits) -> Routes {
        routes.add_service(PingServer::from_arc(self))
    }
}

struct HandWrittenModule;

impl Module for HandWrittenModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.grpc_service::<HandWritten>();
    }
}

async fn boot_error(builder: navree_kernel::AppBuilder) -> String {
    match builder.boot().await {
        Ok(_) => panic!("the boot must fail"),
        Err(errors) => errors.to_string(),
    }
}

#[tokio::test]
async fn mistakes_fail_the_boot() {
    let base = || {
        Navree::app()
            .module(ConfigModule::for_root(ConfigSources::new()))
            .module(HttpModule)
            .module(GrpcModule::from_config())
    };

    let error = boot_error(base().module(GuardedPingModule)).await;
    assert!(
        error.contains("gRPC method `ping` of `navree.test.v1.Ping`")
            && error.contains("no `dyn Authenticator` is registered"),
        "{error}"
    );

    let error = boot_error(base().module(HandWrittenModule)).await;
    assert!(error.contains("has no access declaration"), "{error}");

    let error = TestApp::builder()
        .module(GrpcModule::from_config())
        .module(PingModule)
        .module(GuardedPingModule)
        .try_build()
        .await
        .map(drop)
        .expect_err("two services with one name")
        .to_string();
    assert!(error.contains("is registered twice"), "{error}");

    let error = TestApp::builder()
        .config("grpc.max_message_bytes", 0)
        .module(GrpcModule::from_config())
        .try_build()
        .await
        .map(drop)
        .expect_err("a bad option")
        .to_string();
    assert!(error.contains("grpc.max_message_bytes"), "{error}");

    let hand_written = TestApp::builder()
        .module(GrpcModule::for_root(navree_grpc::GrpcOptions::default()))
        .module(HandWrittenModule)
        .try_build()
        .await;
    assert!(hand_written.is_err(), "for_root runs the same checks");
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
async fn grpc_shares_the_http_port_over_h2c() {
    let app = ping_app()
        .config("http.addr", "127.0.0.1:0")
        .config("http.shutdown_grace_secs", 1)
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .expect("connects over h2c");
        let mut client = PingClient::new(channel);
        let response = client.ping(ping("over the wire")).await.unwrap();
        assert!(response.metadata().get("x-request-id").is_some());
        assert_eq!(response.into_inner().message, "over the wire");
        let status = client.ping(ping("conflict")).await.unwrap_err();
        assert_eq!(status.code(), Code::AlreadyExists);
    })
    .await;
}

#[tokio::test]
async fn grpc_addr_gives_grpc_a_listener_of_its_own() {
    let app = ping_app()
        .config("http.addr", "127.0.0.1:0")
        .config("grpc.addr", "127.0.0.1:0")
        .config("http.shutdown_grace_secs", 1)
        .build()
        .await;
    serving(&app, async {
        let http = app.get::<HttpServer>().await.wait_bound().await;
        let grpc = tokio::time::timeout(
            Duration::from_secs(5),
            app.get::<GrpcServer>().await.wait_bound(),
        )
        .await
        .expect("the gRPC listener starts with serve");
        assert_ne!(http.port(), grpc.port());

        let channel = tonic::transport::Endpoint::from_shared(format!("http://{grpc}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let reply = PingClient::new(channel)
            .ping(ping("own port"))
            .await
            .unwrap();
        assert_eq!(reply.into_inner().message, "own port");

        // The HTTP port no longer answers gRPC.
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{http}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let status = PingClient::new(channel).ping(ping("x")).await.unwrap_err();
        assert!(
            status.message().contains("404 Not Found"),
            "the routes answered: {status:?}"
        );

        // The gRPC port answers nothing but gRPC, gRPC-Web and Connect.
        assert_eq!(
            plain_http_status(grpc).await,
            "HTTP/1.1 415 Unsupported Media Type"
        );
    })
    .await;
    assert_eq!(
        app.get::<GrpcServer>().await.local_addr(),
        None,
        "the listener stopped"
    );
}

#[tokio::test]
async fn other_commands_never_open_the_grpc_port() {
    let app = ping_app().config("grpc.addr", "127.0.0.1:0").build().await;
    app.run_command("routes", &[]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(app.get::<GrpcServer>().await.local_addr(), None);
}

/// The status line a plain `GET /` over HTTP/1.1 gets from `addr`.
async fn plain_http_status(addr: std::net::SocketAddr) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            b"GET / HTTP/1.1
host: localhost
connection: close

",
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn unary_calls_in_flight_finish_on_shutdown() {
    let app = ping_app()
        .config("http.addr", "127.0.0.1:0")
        .config("http.shutdown_grace_secs", 5)
        .build()
        .await;
    let serve = app.run_command("serve", &[]);
    let call = async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let mut client = PingClient::new(channel);
        let slow = client.ping(PingRequest {
            message: "slow".to_owned(),
            sleep_ms: 300,
        });
        let stop = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            app.ctx().shutdown_token().cancel();
        };
        let (reply, ()) = tokio::join!(slow, stop);
        reply
    };
    let (served, reply) = tokio::join!(serve, call);
    served.expect("serve stops cleanly");
    let reply = reply.expect("the call in flight finished");
    assert_eq!(reply.into_inner().message, "slow");
}
