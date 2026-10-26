//! Every item `microservices/grpc.mdx` names, so a rename fails the build:
//! types, functions, methods, configuration keys and environment variables.

#[path = "support/mod.rs"]
mod support;

use std::time::Duration;

use navree_config::{Config, ConfigSources};
use navree_grpc::testing::TestChannel;
use navree_grpc::{
    ClientOptions, ClientTlsOptions, Code, GrpcChannel, GrpcChannels, GrpcMethod, GrpcModule,
    GrpcOptions, GrpcServer, GrpcService, ModuleBuilderGrpcExt, Request, Response, ServiceLimits,
    Status, Streaming, Valid, error_from_status, status_from_error,
};
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::{TestApp, principal};
use serde_json::json;
use uuid::Uuid;

/// The attribute and tonic's names, re-exported where the page imports them.
#[allow(unused_imports)]
use navree_grpc::{async_trait, grpc_service, metadata};

#[test]
fn the_module_and_its_options() {
    fn modules() -> [Box<dyn Module>; 3] {
        [
            Box::new(GrpcModule),
            Box::new(GrpcModule::from_config()),
            Box::new(GrpcModule::for_root(GrpcOptions::default())),
        ]
    }
    assert_eq!(modules().len(), 3);

    // Every `grpc.*` key of the page's tables, by name.
    let options: GrpcOptions = serde_json::from_value(json!({
        "addr": "0.0.0.0:50051",
        "web": false,
        "connect": false,
        "reflection": true,
        "health": false,
        "timeout_secs": 7,
        "max_message_bytes": 1024,
        "concurrency_per_connection": 8,
        "clients": {
            "deals": {
                "url": "https://deals.internal:8443",
                "timeout_ms": 3000,
                "forward_auth": false,
                "tls": { "ca": "ca.pem", "cert": "client.pem", "key": "client.key" },
            },
        },
    }))
    .unwrap();
    assert_eq!(options.addr, Some("0.0.0.0:50051".parse().unwrap()));
    assert!(!options.web && !options.connect && options.reflection && !options.health);
    assert_eq!(options.default_timeout(30), Duration::from_secs(7));
    assert_eq!(options.max_message_bytes, 1024);
    assert_eq!(options.concurrency_per_connection, 8);
    let client: &ClientOptions = &options.clients["deals"];
    assert_eq!((client.timeout_ms, client.forward_auth), (3000, false));
    let tls: &ClientTlsOptions = &client.tls;
    assert!(tls.ca.is_some() && tls.cert.is_some() && tls.key.is_some());
    assert!(options.check().is_ok());

    // `grpc.timeout_secs` falls back to `http.timeout_secs`.
    assert_eq!(GrpcOptions::default().default_timeout(30), Duration::from_secs(30));

    // The environment variables of the page.
    let options = Config::load(&ConfigSources::new().env_vars(
        "APP_",
        [("APP_GRPC__ADDR", "0.0.0.0:50051"), ("APP_GRPC__REFLECTION", "false")],
    ))
    .unwrap()
    .typed::<GrpcOptions>()
    .unwrap();
    assert!(options.addr.is_some() && !options.reflection);
}

#[test]
fn the_registration_api() {
    fn wire(m: &mut ModuleBuilder) {
        m.grpc_service::<support::PingGrpc>()
            .grpc_descriptors(support::pb::FILE_DESCRIPTOR_SET);
    }
    let _: fn(&mut ModuleBuilder) = wire;
    let _: &str = <support::PingGrpc as GrpcService>::NAME;
    let _ = GrpcMethod::new("ping").public();
    let _ = ServiceLimits {
        max_message_bytes: 1,
    };
}

#[test]
fn the_error_mappings() {
    assert_eq!(status_from_error(&AppError::not_found()).code(), Code::NotFound);
    assert_eq!(error_from_status(&Status::not_found("x")).status.as_u16(), 404);
}

#[tokio::test]
async fn the_server_channels_and_test_channel() {
    let app = TestApp::builder()
        .config("grpc.clients.deals.url", "http://127.0.0.1:1")
        .module(GrpcModule::from_config())
        .module(support::PingModule)
        .module(support::DealsTableModule)
        .temp_database()
        .module(navree_db::DatabaseModule::from_config())
        .build()
        .await;
    let server = app.get::<GrpcServer>().await;
    assert_eq!(server.services(), ["navree.test.v1.Ping"]);
    assert_eq!(server.local_addr(), None);

    let channels = app.get::<GrpcChannels>().await;
    let _channel: GrpcChannel = channels.get("deals").unwrap();
    assert_eq!(channels.names().collect::<Vec<_>>(), ["deals"]);

    let channel = TestChannel::new(&app)
        .acting_as(principal(Uuid::now_v7()))
        .bearer("token")
        .metadata("x-client-version", "1.4");
    let _ = format!("{channel:?}");
}

/// The method signatures the page shows compile: `Request<T>`,
/// `Valid<Request<T>>`, `Streaming<T>` and both result types.
#[allow(dead_code)]
fn signatures(
    _plain: Request<support::pb::ping::PingRequest>,
    _valid: Valid<Request<support::pb::deals::MoveDealRequest>>,
    _stream: Request<Streaming<support::pb::deals::Deal>>,
) -> (
    navree_http::Result<Response<support::pb::ping::PingReply>>,
    Result<Response<support::pb::ping::PingReply>, Status>,
) {
    unimplemented!()
}
