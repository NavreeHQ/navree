//! [`UiModule`]: serves the toolkit's files and answers each [`Reply`] the way
//! the request asked: ops for the runtime, the fallback for everyone else.
//!
//! [`Reply`]: crate::Reply

use std::sync::Arc;

use http::{HeaderMap, Method};
use navree_http::extract::Request;
use navree_http::middleware::{self, Next};
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, GlobalLayer, LayerSlot, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::{assets, compress};
use crate::op::{PendingOps, UI_HEADER};

/// Registers the toolkit in an app: the routes of its files, and the layer
/// that turns a [`Reply`](crate::Reply) into ops when the runtime asked.
///
/// ```ignore
/// App::builder().module(HttpModule::from_config()).module(UiModule::from_config())
/// ```
pub struct UiModule;

impl UiModule {
    /// The module. There is nothing to configure: the files are served at
    /// [`assets::MOUNT`].
    pub fn from_config() -> impl Module {
        Self
    }
}

impl Module for UiModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<UiModule>()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.http_layer(GlobalLayer::new(LayerSlot::Session, "ui.reply", |_, router| {
            Ok(router.layer(middleware::from_fn(reply_layer)))
        }));
        m.controller::<Assets>();
    }
}

/// Swaps a reply's fallback for its ops when the request came from the runtime,
/// and sends rendered responses in brotli to clients that take it.
async fn reply_layer(request: Request, next: Next) -> Response {
    let from_runtime = request.headers().contains_key(UI_HEADER);
    let accept = compress::accept_of(request.headers());
    let method = request.method().clone();
    let mut response = next.run(request).await;
    if let Some(PendingOps(envelope, status)) = response.extensions_mut().remove::<PendingOps>()
        && from_runtime
    {
        response = (status, envelope).into_response();
    }
    compress::encode(&method, &accept, response).await
}

/// The route of the files.
struct Assets;

impl Injectable for Assets {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Assets {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                format!("!{}/{{file}}", assets::MOUNT),
                |Path(file): Path<String>, headers: HeaderMap| async move { assets::respond(&file, &headers) },
            )
            .name("ui.assets")
            .public(),
        ]
    }
}
