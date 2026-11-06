//! [`UiModule`]: serves the toolkit's files and answers each [`Reply`] the way
//! the request asked: ops for the runtime, the fallback for everyone else.
//!
//! [`Reply`]: crate::Reply

use std::sync::{Arc, OnceLock};

use bytes::Bytes;

use http::header::{self, HeaderName, HeaderValue};
use http::{HeaderMap, Method, StatusCode};
use navree_config::{Config, ModuleBuilderConfigExt};
use navree_http::extract::Request;
use navree_http::middleware::{self, Next};
use navree_http::response::{IntoResponse, Response};
use navree_http::{Controller, GlobalLayer, LayerSlot, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{
    AppContext, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};

use crate::op::{Envelope, PendingOps, UI_HEADER};
use crate::pages::{PagesOptions, Renderer, UiOptions};
use crate::shell::FLASH_COOKIE;
use crate::{assets, compress};

/// Registers the toolkit in an app: the routes of its files, and the layer
/// that turns a [`Reply`](crate::Reply) into ops when the runtime asked.
///
/// ```ignore
/// App::builder().module(HttpModule::from_config()).module(UiModule::from_config())
/// ```
///
/// An app that names `UiModule` always has the files and the layer. A module
/// that only sometimes serves pages imports [`UiOnDemand`] instead and says
/// when with a [`UiNeed`]; with no need standing, the app gets no route and
/// no layer from the toolkit.
pub struct UiModule;

impl UiModule {
    /// The module. There is nothing to configure: the files are served at
    /// [`assets::MOUNT`].
    pub fn from_config() -> impl Module {
        Self
    }

    /// The module, with the settings of the error pages it draws: where "Sign in" and
    /// "Go to home" lead, and the words. The `ui` section of the configuration, when the
    /// app has one, overrides them.
    pub fn with_pages(options: PagesOptions) -> impl Module {
        UiWithPages(options)
    }
}

/// `UiModule` with settings: the same module, configured.
struct UiWithPages(PagesOptions);

impl Module for UiWithPages {
    fn id(&self) -> ModuleId {
        ModuleId::of::<UiModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, true, Some(self.0.clone()));
    }
}

impl Module for UiModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<UiModule>()
    }

    /// Named by the app, it outranks the same module imported on demand.
    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, true, None);
    }
}

/// The toolkit for a module that serves pages only some of the time (a
/// debugging tool that is off in a release build). It has `UiModule`'s id, so
/// an app that names `UiModule` gets the whole of it whatever the order; alone,
/// it adds a route and a layer only while a [`UiNeed`] says so.
pub struct UiOnDemand;

impl Module for UiOnDemand {
    fn id(&self) -> ModuleId {
        ModuleId::of::<UiModule>()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, false, None);
    }
}

/// What a module contributes (`ModuleBuilder::contribute`) to say that it
/// needs the toolkit's files and layer now. [`UiOnDemand`] asks every one once,
/// at boot, after providers can be resolved.
pub struct UiNeed(Arc<dyn Fn(AppContext) -> BoxFuture<'static, bool> + Send + Sync>);

impl UiNeed {
    /// A need that stands when `wanted` answers true.
    pub fn when<F, Fut>(wanted: F) -> Self
    where
        F: Fn(AppContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = bool> + Send + 'static,
    {
        Self(Arc::new(move |ctx| Box::pin(wanted(ctx))))
    }
}

/// Marks an app that named `UiModule` itself.
pub(crate) struct UiAlways;

/// The answer to "does anything need the toolkit", settled before the first
/// request and read by the layer.
struct UiGate(Arc<OnceLock<bool>>);

fn register(m: &mut ModuleBuilder, always: bool, pages: Option<PagesOptions>) {
    m.global();
    m.error_pages(Renderer);
    // The `ui` section is read by the factory below (an app need not have
    // `ConfigModule`), so it is registered here without a provider.
    m.config_section::<UiOptions>();
    m.factory(Factory::<PagesOptions>::new(move |r: Resolver| {
        let explicit = pages.clone();
        async move {
            let base = explicit.unwrap_or_default();
            let options = match r.app().try_get::<Config>().await {
                Some(config) => base.try_configured(&config)?,
                None => base,
            };
            Ok(Arc::new(options.checked()))
        }
    }));
    if always {
        m.contribute(UiAlways);
    }
    let slot: Arc<OnceLock<bool>> = Arc::new(OnceLock::new());
    let for_gate = slot.clone();
    m.factory(Factory::<UiGate>::new(move |r: Resolver| {
        let slot = for_gate.clone();
        async move {
            let ctx = r.app();
            let mut wanted = !ctx.contributions::<UiAlways>().is_empty();
            if !wanted {
                for need in ctx.contributions::<UiNeed>() {
                    if (need.value.0)(ctx.clone()).await {
                        wanted = true;
                        break;
                    }
                }
            }
            let _ = slot.set(wanted);
            Ok(Arc::new(UiGate(slot)))
        }
    }));
    m.http_layer(GlobalLayer::new(
        LayerSlot::Session,
        "ui.reply",
        move |_, router| {
            if slot.get().copied().unwrap_or(false) {
                Ok(router.layer(middleware::from_fn(reply_layer)))
            } else {
                Ok(router)
            }
        },
    ));
    m.controller::<Assets>();
}

/// Swaps a reply's fallback for its ops when the request came from the runtime,
/// and sends rendered responses in brotli to clients that take it.
async fn reply_layer(request: Request, next: Next) -> Response {
    let from_runtime = request.headers().contains_key(UI_HEADER);
    let accept = compress::accept_of(request.headers());
    let method = request.method().clone();
    let mut response = next.run(request).await;
    if let Some(PendingOps(envelope, status)) = response.extensions_mut().remove::<PendingOps>() {
        if from_runtime {
            response = to_ops(response, envelope, status);
        }
        // The same URL answers ops or a page: a cache has to keep them apart.
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static(UI_HEADER));
    }
    compress::encode(&method, &accept, response).await
}

/// What only the fallback's own body or destination means: everything else on a
/// response (a session's cookies, a layer's headers) belongs to the answer,
/// whichever form it goes out in.
fn fallback_only(name: &HeaderName) -> bool {
    name == header::LOCATION
        || name == header::CONTENT_TYPE
        || name == header::CONTENT_LENGTH
        || name == header::CONTENT_ENCODING
        || name == header::CACHE_CONTROL
        || name == header::CONTENT_SECURITY_POLICY
        || name == "speculation-rules"
}

/// The reply's ops, in place of its fallback. Headers set on the fallback stay,
/// so a cookie survives whichever side of this layer it was set on. A toast is
/// on its way as an op, so the cookie that would show it on the next page isn't.
fn to_ops(fallback: Response, envelope: Envelope, status: StatusCode) -> Response {
    let (parts, _) = fallback.into_parts();
    let mut response = (status, envelope).into_response();
    let headers = response.headers_mut();
    for (name, value) in &parts.headers {
        let flash = *name == header::SET_COOKIE
            && value
                .as_bytes()
                .strip_prefix(FLASH_COOKIE.as_bytes())
                .is_some_and(|rest| rest.first() == Some(&b'='));
        if !flash && !fallback_only(name) {
            headers.append(name, value.clone());
        }
    }
    *response.extensions_mut() = parts.extensions;
    response
}

/// The route of the files, there only while the toolkit is wanted, with the island
/// scripts the app's modules contributed.
struct Assets {
    wanted: bool,
    islands: Arc<Vec<crate::island::Served>>,
}

impl Injectable for Assets {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<UiGate>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let gate = r.get::<UiGate>().await?;
            let scripts = r
                .app()
                .contributions::<crate::IslandScript>()
                .into_iter()
                .map(|contributed| *contributed.value);
            Ok(Self {
                wanted: gate.0.get().copied().unwrap_or(false),
                islands: Arc::new(crate::island::served(scripts)),
            })
        })
    }
}

impl Controller for Assets {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.wanted {
            return Vec::new();
        }
        vec![
            RouteDef::on(
                Method::GET,
                format!("!{}/{{file}}", assets::MOUNT),
                |Path(file): Path<String>, headers: HeaderMap| async move {
                    assets::respond(&file, &headers)
                },
            )
            .name("ui.assets")
            .public(),
            // An island's script, of the app's own: by its name and content hash.
            RouteDef::on(
                Method::GET,
                format!("!{}/{{file}}", crate::island::ISLAND_PATH),
                {
                    let islands = self.islands.clone();
                    move |Path(file): Path<String>, headers: HeaderMap| {
                        let islands = islands.clone();
                        async move { crate::island::respond(&file, &headers, &islands) }
                    }
                },
            )
            .name("ui.islands")
            .public(),
            // A theme of your own: the file's name is the theme, read by a strict parser.
            RouteDef::on(
                Method::GET,
                format!("!{}/theme/{{file}}", assets::MOUNT),
                |Path(file): Path<String>, headers: HeaderMap| async move {
                    crate::theme::respond_sheet(&file, &headers)
                },
            )
            .name("ui.theme")
            .public(),
            // The theme menu's post: sets the reader's cookie and goes back.
            RouteDef::on(
                Method::POST,
                format!("!{}", crate::theme::SETTINGS_PATH),
                |headers: HeaderMap, body: Bytes| async move {
                    crate::theme::respond_setting(&headers, &body)
                },
            )
            .name("ui.appearance")
            .public(),
        ]
    }
}
