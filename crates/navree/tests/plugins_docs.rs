//! The examples of the Plugins section's pages (`docs/plugins/*`) through the
//! facade.
//!
//! `existing-crates` ("Connecting an existing crate"): a client crate as a
//! `ClientModule` of a plugin, a tower layer whose service can fail, and a
//! service mounted under `/khata` behind the app's sign-in. The page's tonic
//! example is the gRPC page's, proved in `tests/grpc.rs`.

// The pages' examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use std::time::Duration;

use navree::config::ConfigSection;
use navree::http::extract::Request;
use navree::http::{AppRouter, FallibleLayer, GlobalLayer, LayerSlot, Mount};
use navree::plugin::ClientModule;
use navree::plugin::prelude::*;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde::Deserialize;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// existing-crates: a ledger plugin that connects an existing crate
// ---------------------------------------------------------------------------

/// The `khata` section.
#[derive(Clone, Deserialize, ConfigSection)]
#[config(key = "khata")]
pub struct KhataOptions {
    /// Where the ledger service answers.
    pub base_url: String,
}

/// The client the ledger crate gives.
pub struct KhataClient {
    pub base_url: String,
}

/// The ledger crate's client as a module of the plugin.
pub fn khata_client() -> ClientModule<KhataClient, KhataOptions> {
    ClientModule::<KhataClient>::new("khata")
        .options::<KhataOptions>()
        .connect(|options, _resolver| async move {
            Ok(KhataClient {
                base_url: options.base_url.clone(),
            })
        })
        .health(|client| async move {
            if client.base_url.starts_with("https://") {
                Ok(())
            } else {
                Err("the khata service is not on https".into())
            }
        })
        .plugin(navree::plugin_info!(namespace = "khata"))
}

/// The ledger's entries: the service mounted under `/khata`.
async fn entries(request: Request) -> Result<String> {
    let ctx = request
        .extensions()
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal("the request has no app context"))?;
    let khata = ctx.get::<KhataClient>().await.map_err(AppError::internal)?;
    Ok(format!("{} {}", khata.base_url, request.uri().path()))
}

/// The plugin's own module: a timeout on every request, and the ledger under
/// `/khata` for signed-in users.
pub struct LedgerModule;

impl Module for LedgerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(navree::plugin_info!(namespace = "khata"));
        m.http_layer(GlobalLayer::new(
            LayerSlot::Context,
            "khata-timeout",
            |_ctx, router| {
                Ok(router.layer(FallibleLayer::new(tower::timeout::TimeoutLayer::new(
                    Duration::from_secs(10),
                ))))
            },
        ))
        .map_error::<tower::timeout::error::Elapsed>(|_| {
            Some(AppError::service_unavailable("The khata took too long."))
        });
        m.mount(
            Mount::new("/khata", AppRouter::from_handler(entries))
                .authenticated()
                .throttle(60, Duration::from_secs(60)),
        );
    }
}

#[tokio::test]
async fn the_existing_crates_page_examples_mount_inside_the_pipeline() {
    let app = TestApp::builder()
        .config("khata.base_url", "https://khata.example/ludhiana")
        .module(khata_client())
        .module(LedgerModule)
        .build()
        .await;
    app.client()
        .get("/khata/entries")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    let harpreet = principal(Uuid::now_v7());
    let response = app
        .acting_as(harpreet)
        .get("/khata/entries")
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.text(), "https://khata.example/ludhiana /entries");

    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.namespace == "khata")
        .expect("the ledger plugin");
    assert!(!plugin.first_party);
    let modules: Vec<&str> = plugin
        .modules
        .iter()
        .map(|module| module.id.name())
        .collect();
    assert!(
        modules.contains(&"khata") && modules.contains(&"LedgerModule"),
        "{modules:?}"
    );

    app.shutdown().await.unwrap();
}
