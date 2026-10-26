//! The examples of `microservices/grpc.mdx`, with the crates' own paths.
//!
//! - `build.rs` (protox and tonic-prost-build): `tests/codegen.rs` runs it.
//! - The deals service, its interceptor and `WatchDeals` from the event bus:
//!   `tests/support/mod.rs` (`DealsGrpc`, `require_client_version`), driven
//!   by `tests/grpc/*.rs`; the page's test is
//!   `tests/grpc/pipeline.rs::get_deal_checks_access`.
//! - The same code through the facade (`navree::grpc`, `navree::prelude`):
//!   `crates/navree/tests/grpc.rs`.
//!
//! Here: the configuration examples and the client example.

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use navree_config::{Config, ConfigSources};
use navree_grpc::{GrpcChannels, GrpcModule, GrpcOptions};
use navree_http::Result;
use navree_kernel::BoxError;
use uuid::Uuid;

use support::pb::deals::deals_client::DealsClient;
use support::pb::deals::{Deal, GetDealRequest};

fn options_from(sources: &ConfigSources) -> GrpcOptions {
    Config::load(sources)
        .expect("the configuration loads")
        .typed::<GrpcOptions>()
        .expect("the grpc section is valid")
}

/// `config/app.toml` with `[grpc]` and `[grpc.clients.deals]`.
#[test]
fn the_toml_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.toml");
    std::fs::write(
        &file,
        r#"
[grpc]
addr = "0.0.0.0:50051"
web = true
connect = true

[grpc.clients.deals]
url = "http://deals.internal:8081"
timeout_ms = 3000
"#,
    )
    .unwrap();
    let options = options_from(&ConfigSources::new().file(&file));
    assert_eq!(options.addr, Some("0.0.0.0:50051".parse().unwrap()));
    assert!(options.web && options.connect);
    let deals = &options.clients["deals"];
    assert_eq!(deals.url, "http://deals.internal:8081");
    assert_eq!(deals.timeout_ms, 3000);
    assert!(deals.forward_auth, "the default");
}

/// `APP_GRPC__ADDR=0.0.0.0:50051`, `APP_GRPC__REFLECTION=false`.
#[test]
fn the_environment_variables() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_GRPC__ADDR", "0.0.0.0:50051"),
            ("APP_GRPC__REFLECTION", "false"),
        ],
    );
    let options = options_from(&sources);
    assert_eq!(options.addr, Some("0.0.0.0:50051".parse().unwrap()));
    assert!(!options.reflection);
}

/// `GrpcModule::for_root(GrpcOptions { addr: Some(..), ..GrpcOptions::default() })`.
#[test]
fn for_root_takes_the_options() -> std::result::Result<(), BoxError> {
    let _module = GrpcModule::for_root(GrpcOptions {
        addr: Some("0.0.0.0:50051".parse()?),
        ..GrpcOptions::default()
    });
    Ok(())
}

/// The page's client: a tonic client on a named channel.
struct DealLookup {
    channels: Arc<GrpcChannels>,
}

impl DealLookup {
    async fn get(&self, id: Uuid) -> Result<Deal> {
        let mut deals = DealsClient::new(self.channels.get("deals")?);
        let reply = deals
            .get_deal(GetDealRequest { id: id.to_string() })
            .await?;
        Ok(reply.into_inner())
    }
}

#[tokio::test]
async fn the_client_example_names_its_channel() {
    let app = support::ping_app().build().await;
    let lookup = DealLookup {
        channels: app.get::<GrpcChannels>().await,
    };
    let error = lookup.get(Uuid::now_v7()).await.unwrap_err();
    assert_eq!(error.status.as_u16(), 500, "no [grpc.clients.deals] here");
    assert!(
        format!("{error:?}").contains("grpc.clients.deals"),
        "the error names the missing key"
    );
}
