//! [`UiModule`]: serves the toolkit's files and answers each [`Reply`] the way
//! the request asked: ops for the runtime, the fallback for everyone else.
//!
//! [`Reply`]: crate::Reply

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, OnceLock};

use bytes::Bytes;

use http::header::{self, HeaderName, HeaderValue};
use http::{HeaderMap, Method, StatusCode};
use navree_config::{Config, ModuleBuilderConfigExt};
use navree_http::extract::Request;
use navree_http::middleware::{self, Next};
use navree_http::response::{IntoResponse, Response};
use navree_http::{
    ApiMeta, Controller, GlobalLayer, LayerSlot, ModuleBuilderHttpExt, Path, RouteDef,
};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Factory, Injectable, KernelError, Module,
    ModuleBuilder, ModuleId, ProviderKey, Resolver,
};

use crate::op::{Envelope, PendingOps, UI_HEADER, is_prefetch};
use crate::pages::{PagesOptions, REGION_HEADER, Renderer, UiOptions};
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
pub(crate) struct UiGate(pub(crate) Arc<OnceLock<bool>>);

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
    #[cfg(feature = "native-preview")]
    let for_context = slot.clone();
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
    // Who asked, made current where handlers run, as a task-local value of the
    // request belongs: inside the unit of work.
    #[cfg(feature = "native-preview")]
    m.http_layer(GlobalLayer::new(
        LayerSlot::Context,
        "ui.native",
        move |_, router| {
            if for_context.get().copied().unwrap_or(false) {
                Ok(router.layer(middleware::from_fn(crate::native::swap::context_layer)))
            } else {
                Ok(router)
            }
        },
    ));
    m.controller::<Assets>();
    #[cfg(feature = "native-preview")]
    {
        m.controller::<crate::native::Routes>();
        // What a native client keeps its screens under.
        m.factory(Factory::<crate::native::Stamp>::new(
            |r: Resolver| async move { Ok(Arc::new(crate::native::app_stamp(&r.app()).await)) },
        ));
    }
    // The appearance route sets only the reader's theme cookie and refuses posts from other
    // sites itself: the session layer checks its origin in place of the token, so a theme
    // toggle on a page cached for everyone (with no token in it) works for a reader with a
    // session too.
    m.contribute(navree_http::csrf::SameOriginOnly(
        crate::theme::SETTINGS_PATH,
    ));
    m.command(
        CommandSpec::new(
            "ui:render",
            "Draw one page as a guest and print the answer as JSON (<path> --json)",
            render_command,
        )
        .stdio(),
    );
    // `navree native:coverage` asks the app's page routes as a native client.
    #[cfg(feature = "native-preview")]
    m.command(
        CommandSpec::new(
            "native:coverage",
            "Ask every page route as a native client and print each one's grade as JSON ([--platform <p>] [--form <f>] [--plan <json>])",
            crate::native::survey::command,
        )
        .stdio(),
    );
}

/// The most of a page `ui:render` reads.
const RENDER_AT_MOST: usize = 4 * 1024 * 1024;

/// `ui:render <path> --json`: one `GET` through the app's own router, as a guest
/// from 127.0.0.1, printed as one JSON object (`path`, `status`, `content_type`,
/// `location`, `html`) whatever the status. Redirects are not followed, and
/// `html` is the body only when it is HTML. Navigator reads it to outline a page.
async fn render_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let path = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .filter(|path| path.starts_with('/') && !path.starts_with("//"))
        .ok_or("ui:render needs a path of the app, such as /deals")?;
    let json = render_json(&ctx, path).await?;
    println!("{json}");
    Ok(())
}

/// What [`render_command`] prints for `path`.
pub(crate) async fn render_json(
    ctx: &AppContext,
    path: &str,
) -> Result<serde_json::Value, BoxError> {
    use tower::ServiceExt;

    let router = navree_http::build_router(ctx).await?;
    let mut request = http::Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header::ACCEPT, "text/html,application/xhtml+xml")
        .body(navree_http::body::Body::empty())?;
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49152);
    request
        .extensions_mut()
        .insert(navree_http::serve::ConnectInfo(peer));
    let response = router
        .oneshot(request)
        .await
        .unwrap_or_else(|never| match never {});
    let (parts, body) = response.into_parts();
    let bytes = navree_http::body::to_bytes(body, RENDER_AT_MOST).await?;
    let text = |name: HeaderName| {
        parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let content_type = text(header::CONTENT_TYPE);
    let html = content_type
        .as_deref()
        .is_some_and(|kind| kind.starts_with("text/html"))
        .then(|| String::from_utf8_lossy(&bytes).into_owned());
    Ok(serde_json::json!({
        "path": path,
        "status": parts.status.as_u16(),
        "content_type": content_type,
        "location": text(header::LOCATION),
        "html": html,
    }))
}

/// Swaps a reply's fallback for its ops when the request came from the runtime,
/// and sends rendered responses in brotli to clients that take it.
async fn reply_layer(request: Request, next: Next) -> Response {
    let asked = crate::asked::who_asked(request.headers());
    // A native client's request, or its web view's, is answered its own way;
    // a browser's is handed back untouched.
    #[cfg(feature = "native-preview")]
    let (request, next) = match crate::native::swap::layer(asked, request, next).await {
        Ok(response) => return response,
        Err(back) => back,
    };
    let from_runtime = asked.is_runtime();
    let for_region = request.headers().contains_key(REGION_HEADER);
    let accept = compress::accept_of(request.headers());
    let method = request.method().clone();
    // What the runtime asked for, for telemetry: a fetch ahead of a click is
    // not an operation anyone performed.
    let operation = (from_runtime && !is_prefetch(request.headers()))
        .then(|| operation_kind(&method, request.headers()));
    let started = std::time::Instant::now();
    let mut response = next.run(request).await;
    let ops = response
        .extensions()
        .get::<PendingOps>()
        .map_or(0, |pending| {
            u32::try_from(pending.0.ops.len()).unwrap_or(u32::MAX)
        });
    if let Some(PendingOps(envelope, status)) = response.extensions_mut().remove::<PendingOps>() {
        if from_runtime {
            response = to_ops(response, envelope, status);
        }
        // The same URL answers ops or a page: a cache has to keep them apart.
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static(UI_HEADER));
    }
    // A native client asking the same URL gets another answer.
    #[cfg(feature = "native-preview")]
    crate::native::swap::vary_on_native(&mut response);
    // A page the toolkit drew, answered to a region's own fetch, is that region's fragment
    // even when it loads the runtime (a lazy tab inside it does): never a page elsewhere.
    if for_region
        && response.status().is_success()
        && response
            .extensions()
            .get::<crate::view::ViewAnswer>()
            .is_some()
    {
        response
            .headers_mut()
            .insert(FRAGMENT_HEADER, HeaderValue::from_static("1"));
    }
    if let Some(kind) = operation {
        record_operation(kind, &response, ops, started);
    }
    compress::encode(&method, &accept, response).await
}

/// Records an operation the runtime or a native client asked for, in the
/// telemetry of the request.
pub(crate) fn record_operation(
    kind: navree_telemetry::UiOperationKind,
    response: &Response,
    ops: u32,
    started: std::time::Instant,
) {
    navree_telemetry::Telemetry::current().ui_operation(|| {
        let name = response
            .extensions()
            .get::<Arc<navree_http::RouteMeta>>()
            .map_or_else(|| "unmatched".to_owned(), |route| route.full_path.clone());
        navree_telemetry::UiOperation::new(
            name,
            kind,
            response.status().as_u16(),
            ops,
            started.elapsed(),
        )
    });
}

/// The header on a region's answer that says it is the region's fragment, not a page.
pub(crate) const FRAGMENT_HEADER: HeaderName = HeaderName::from_static("x-navree-fragment");

/// What a request from the runtime is: a region fetched again, a fragment
/// of a page, a form posted or another action.
pub(crate) fn operation_kind(
    method: &Method,
    headers: &HeaderMap,
) -> navree_telemetry::UiOperationKind {
    use navree_telemetry::UiOperationKind;

    if method == Method::GET || method == Method::HEAD {
        return if headers.contains_key(REGION_HEADER) {
            UiOperationKind::LiveRegion
        } else {
            UiOperationKind::Fragment
        };
    }
    let form = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.starts_with("application/x-www-form-urlencoded")
                || value.starts_with("multipart/form-data")
        });
    if form {
        UiOperationKind::FormPost
    } else {
        UiOperationKind::Action
    }
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
pub(crate) fn to_ops(fallback: Response, envelope: Envelope, status: StatusCode) -> Response {
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
    app_files: Arc<Vec<crate::app_files::Served>>,
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
            let sheets = r
                .app()
                .contributions::<crate::AppSheet>()
                .into_iter()
                .map(|contributed| *contributed.value);
            let fonts = r
                .app()
                .contributions::<crate::AppFont>()
                .into_iter()
                .map(|contributed| *contributed.value);
            let images = r
                .app()
                .contributions::<crate::AppImage>()
                .into_iter()
                .map(|contributed| *contributed.value);
            Ok(Self {
                wanted: gate.0.get().copied().unwrap_or(false),
                islands: Arc::new(crate::island::served(scripts)),
                app_files: Arc::new(crate::app_files::served(sheets, fonts, images)),
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
            .public()
            .api(api(
                "A toolkit asset (script, stylesheet, font or icon)",
                "Named by its content hash: cached for good under the current hash, checked \
                 again under an older one. Brotli or gzip when the client takes it.",
                &[304, 404],
            )),
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
            .public()
            .api(api(
                "An island script of the app",
                "A script one of the app's modules contributed, by its name and content hash; \
                 under an older hash it is still served, uncached.",
                &[304, 404],
            )),
            // The app's own stylesheets, fonts and pictures: by their name and content hash.
            RouteDef::on(
                Method::GET,
                format!("!{}/{{file}}", crate::app_files::APP_PATH),
                {
                    let files = self.app_files.clone();
                    move |Path(file): Path<String>, headers: HeaderMap| {
                        let files = files.clone();
                        async move { crate::app_files::respond(&file, &headers, &files) }
                    }
                },
            )
            .name("ui.app-files")
            .public()
            .api(api(
                "A stylesheet, font or picture of the app",
                "A file one of the app's modules contributed, by its name and content hash; \
                 under an older hash it is still served, uncached. A picture is served under \
                 a content security policy that runs nothing.",
                &[304, 404],
            )),
            // A theme of your own: the file's name is the theme, read by a strict parser.
            RouteDef::on(
                Method::GET,
                format!("!{}/theme/{{file}}", assets::MOUNT),
                |Path(file): Path<String>, headers: HeaderMap| async move {
                    crate::theme::respond_sheet(&file, &headers)
                },
            )
            .name("ui.theme")
            .public()
            .api(api(
                "The stylesheet of a theme of your own",
                "The file's name is the theme's short form, then a dash and the build; a name \
                 that is not a theme is not found.",
                &[304, 404],
            )),
            // The theme menu's post: sets the reader's cookie and goes back.
            RouteDef::on(
                Method::POST,
                format!("!{}", crate::theme::SETTINGS_PATH),
                |parts: http::request::Parts, body: Bytes| async move {
                    let mut headers = parts.headers;
                    // HTTP/2 and HTTP/3 name the host in `:authority`, and may send no `Host`.
                    if !headers.contains_key(header::HOST)
                        && let Some(authority) = parts.uri.authority()
                        && let Ok(host) = HeaderValue::from_str(authority.as_str())
                    {
                        headers.insert(header::HOST, host);
                    }
                    crate::theme::respond_setting(&headers, &body)
                },
            )
            .name("ui.appearance")
            .public()
            .api(ApiMeta {
                success_status: 303,
                ..api(
                    "Sets the reader's theme and mode",
                    "The theme menu's form (`theme`, `mode`, `back`): sets the appearance cookie \
                     and goes back to `back` (a path of this site, else `/`), so it works with \
                     JavaScript off. A post from another site is refused; the session layer \
                     checks its origin in place of the CSRF token.",
                    &[400, 403],
                )
            }),
        ]
    }
}

/// The documentation of a route of the toolkit. Its answers are files or a
/// redirect, so it has no response schema.
fn api(summary: &str, description: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        description: Some(description.to_owned()),
        tags: vec!["UI".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

#[cfg(test)]
mod tests {
    use navree_http::{HttpModule, RouteDef};
    use navree_testing::TestApp;

    use super::*;
    use crate::View;

    struct Pages;

    impl Injectable for Pages {
        fn dependencies() -> Vec<ProviderKey> {
            Vec::new()
        }

        fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async { Ok(Self) })
        }
    }

    impl Controller for Pages {
        fn prefix(&self) -> &'static str {
            ""
        }

        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            vec![
                RouteDef::on(Method::GET, "!/deals", || async { View::new("Deals") })
                    .page()
                    .public(),
                RouteDef::on(
                    Method::GET,
                    "!/deals/{id}",
                    |Path(id): Path<String>| async move { View::new(format!("Deal {id}")) },
                )
                .page()
                .public(),
                RouteDef::on(Method::GET, "!/old", || async {
                    crate::Reply::redirect("/deals")
                })
                .public(),
            ]
        }
    }

    struct App;

    impl Module for App {
        fn register(&self, m: &mut ModuleBuilder) {
            m.controller::<Pages>();
        }
    }

    async fn app() -> TestApp {
        TestApp::builder()
            .module(HttpModule::for_root(Default::default()))
            .module(UiModule)
            .module(App)
            .build()
            .await
    }

    #[tokio::test]
    async fn ui_render_answers_a_page_with_its_html() {
        let app = app().await;
        assert!(
            app.ctx()
                .commands()
                .iter()
                .any(|c| c.name == "ui:render" && c.stdio)
        );
        let json = render_json(app.ctx(), "/deals").await.unwrap();
        assert_eq!(json["path"], "/deals");
        assert_eq!(json["status"], 200);
        assert!(
            json["content_type"]
                .as_str()
                .unwrap()
                .starts_with("text/html")
        );
        assert_eq!(json["location"], serde_json::Value::Null);
        assert!(json["html"].as_str().unwrap().contains("Deals"));
    }

    #[tokio::test]
    async fn ui_render_reports_a_redirect_without_following_it() {
        let app = app().await;
        let json = render_json(app.ctx(), "/old").await.unwrap();
        assert_eq!(json["status"], 303);
        assert_eq!(json["location"], "/deals");
        assert_eq!(json["html"], serde_json::Value::Null);
    }

    #[cfg(feature = "native-preview")]
    #[tokio::test]
    async fn native_coverage_asks_the_page_routes_of_the_app() {
        use crate::native::survey::{Plan, survey};
        use navree_native::{Client, FormFactor, Platform};

        let app = app().await;
        assert!(
            app.ctx()
                .commands()
                .iter()
                .any(|c| c.name == "native:coverage" && c.stdio)
        );
        let routes = navree_http::route_table(app.ctx()).await.unwrap();
        let router = navree_http::build_router(app.ctx()).await.unwrap();
        let mut plan = Plan::default();
        plan.values.insert("id".to_owned(), "1".to_owned());
        let client = Client::new(Platform::Android, FormFactor::Phone);
        let report = survey(&router, &routes, "/api", &plan, &client)
            .await
            .unwrap();
        let pages: Vec<(&str, &str, &str)> = report
            .pages
            .iter()
            .map(|row| (row.route.as_str(), row.path.as_str(), row.grade.as_str()))
            .collect();
        assert_eq!(
            pages,
            [
                ("/deals", "/deals", "idiom"),
                ("/deals/{id}", "/deals/1", "idiom")
            ]
        );
        // The redirect is listed, with the way to a page behind it.
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        assert_eq!(report.skipped[0].route, "/old");
        assert!(
            report.skipped[0].reason.contains("/deals"),
            "{:?}",
            report.skipped
        );
        // The toolkit's own files are counted, not asked.
        assert!(
            report
                .not_pages
                .iter()
                .any(|n| n.reason.contains("own files"))
        );
        // Without a value for `id`, the route is listed as skipped.
        let report = survey(&router, &routes, "/api", &Plan::default(), &client)
            .await
            .unwrap();
        assert!(
            report
                .skipped
                .iter()
                .any(|s| s.route == "/deals/{id}" && s.reason == "needs a value for {id}"),
            "{:?}",
            report.skipped
        );
    }

    // `navree check` asks every route for its API documentation, the
    // toolkit's own included.
    #[tokio::test]
    async fn every_route_of_the_toolkit_is_documented() {
        let app = app().await;
        let routes = navree_http::route_table(app.ctx()).await.unwrap();
        let mut toolkit = 0;
        for route in routes.iter().filter(|route| {
            route
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with("ui."))
        }) {
            toolkit += 1;
            let label = format!("{} {}", route.method, route.full_path);
            let api = route.api.as_ref().unwrap_or_else(|| panic!("{label}"));
            assert!(api.summary.is_some(), "{label}");
            assert_eq!(api.tags, ["UI"], "{label}");
        }
        assert_eq!(toolkit, 5);
    }
}
