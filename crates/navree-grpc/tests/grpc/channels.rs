//! `GrpcChannels`: calls to another service carry the request id, the
//! authorization, the workspace and the deadline of the call or request
//! being handled; `?` on a status in an HTTP handler keeps its meaning.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::Path;
use navree_grpc::testing::TestChannel;
use navree_grpc::{Code, GrpcChannels};
use navree_http::{HttpServer, Json, ModuleBuilderHttpExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{controller, injectable};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::support::pb::ping::PingRequest;
use crate::support::pb::ping::ping_client::PingClient;
use crate::support::ping_app;

/// An HTTP route that pings the `upstream` service.
#[injectable]
pub struct UpstreamController {
    channels: Arc<GrpcChannels>,
}

#[controller("/upstream")]
impl UpstreamController {
    #[get("/{message}")]
    #[public]
    async fn call(&self, Path(message): Path<String>) -> navree_http::Result<Json<Value>> {
        let mut upstream = PingClient::new(self.channels.get("upstream")?);
        let reply = upstream
            .ping(PingRequest {
                message,
                sleep_ms: 0,
            })
            .await?
            .into_inner();
        Ok(Json(json!({
            "message": reply.message,
            "user_id": reply.user_id,
            "request_id": reply.request_id,
            "workspace": reply.workspace,
        })))
    }
}

struct UpstreamModule;

impl Module for UpstreamModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<UpstreamController>();
    }
}

/// Serves an upstream ping app on a real port while `test` runs with its
/// address.
async fn with_upstream<F, Fut>(test: F)
where
    F: FnOnce(SocketAddr) -> Fut,
    Fut: Future<Output = ()>,
{
    let upstream = ping_app()
        .config("http.addr", "127.0.0.1:0")
        .config("http.shutdown_grace_secs", 1)
        .build()
        .await;
    let (done, finished) = oneshot::channel::<()>();
    let serve = upstream.run_command("serve", &[]);
    let run = async {
        let addr = upstream.get::<HttpServer>().await.wait_bound().await;
        test(addr).await;
        let _ = done.send(());
    };
    let stop = async {
        let _ = finished.await;
        upstream.ctx().shutdown_token().cancel();
    };
    let (served, (), ()) = tokio::join!(serve, run, stop);
    served.unwrap();
}

/// The calling app: the ping service and the upstream route, with
/// `grpc.clients.upstream` pointing at `addr`.
fn caller(addr: SocketAddr) -> TestAppBuilder {
    ping_app()
        .config("grpc.clients.upstream.url", format!("http://{addr}"))
        .module(UpstreamModule)
}

#[tokio::test]
async fn grpc_calls_forward_the_calls_context() {
    with_upstream(|addr| async move {
        let app = caller(addr).build().await;
        let user = Uuid::now_v7();
        let mut client = PingClient::new(
            TestChannel::new(&app)
                .bearer(&user.to_string())
                .metadata("x-request-id", "req-forwarded")
                .metadata("x-workspace", "acme"),
        );
        let reply = client
            .ping(PingRequest {
                message: "call:upstream".into(),
                sleep_ms: 0,
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(reply.message, "from upstream");
        assert_eq!(
            reply.user_id,
            user.to_string(),
            "the authorization was forwarded"
        );
        assert_eq!(reply.request_id, "req-forwarded");
        assert_eq!(reply.workspace, "acme");

        let missing = client
            .ping(PingRequest {
                message: "call:billing".into(),
                sleep_ms: 0,
            })
            .await
            .unwrap_err();
        assert_eq!(
            missing.code(),
            Code::Internal,
            "no such client is a server error"
        );
    })
    .await;
}

#[tokio::test]
async fn http_handlers_forward_their_requests_context() {
    with_upstream(|addr| async move {
        let app = caller(addr).build().await;
        let user = Uuid::now_v7();
        let response = app
            .client()
            .get("/api/upstream/hello")
            .bearer(&user.to_string())
            .header("x-request-id", "req-http")
            .header("x-workspace", "acme")
            .send()
            .await
            .assert_status(200);
        assert_eq!(
            response.json_value(),
            json!({
                "message": "hello",
                "user_id": user.to_string(),
                "request_id": "req-http",
                "workspace": "acme",
            })
        );

        // `?` on the upstream's ALREADY_EXISTS is a 409 here.
        app.client()
            .get("/api/upstream/conflict")
            .send()
            .await
            .assert_error(409, "already pinged");
    })
    .await;
}

#[tokio::test]
async fn forward_auth_can_be_turned_off() {
    with_upstream(|addr| async move {
        let app: TestApp = caller(addr)
            .config("grpc.clients.upstream.forward_auth", false)
            .build()
            .await;
        let response = app
            .client()
            .get("/api/upstream/hello")
            .bearer(&Uuid::now_v7().to_string())
            .send()
            .await
            .assert_status(200);
        assert_eq!(
            response.json_value()["user_id"],
            "",
            "the upstream saw a guest"
        );
    })
    .await;
}
