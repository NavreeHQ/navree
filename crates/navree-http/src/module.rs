//! [`HttpModule`], the [`HttpServer`] provider and the `serve`, `routes`,
//! `down` and `up` commands.

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::{FormRejection, JsonRejection, PathRejection, QueryRejection};
use navree_config::{Config, ModuleBuilderConfigExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Factory, Injectable, KernelError, Lifecycle,
    Module, ModuleBuilder, ModuleId, ProviderKey, Resolver,
};
use tokio::sync::watch;

use crate::maintenance::{MaintenanceInfo, MaintenanceMode};
use crate::{
    AppError, HttpOptions, ModuleBuilderHttpExt, RouteMeta, UrlSigner, build_router, route_table,
};

/// The HTTP module: Axum server, default layer stack, problem+json errors,
/// controllers. Global; provides and exports [`HttpOptions`], [`HttpServer`]
/// and [`UrlSigner`]; registers the commands `serve`, `routes`, `down` and
/// `up`; and checks every route at boot (access declarations, duplicate
/// routes, missing authenticator or authorizer), before `serve` binds a port.
///
/// ```no_run
/// use navree_http::{HttpModule, HttpOptions};
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(HttpModule::for_root(HttpOptions::default()))
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(HttpModule)`) or through
/// [`from_config`](HttpModule::from_config), it reads the `http`
/// configuration section, so the app must include `ConfigModule`.
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpModule;

impl HttpModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: HttpOptions) -> impl Module {
        ConfiguredHttpModule {
            options: Some(options),
        }
    }

    /// The module configured from the `http` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredHttpModule { options: None }
    }
}

impl Module for HttpModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `HttpModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredHttpModule {
    options: Option<HttpOptions>,
}

impl Module for ConfiguredHttpModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<HttpModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<HttpOptions>) {
    m.global();
    match options {
        Some(options) => m.value(options),
        None => m.config::<HttpOptions>(),
    };
    m.exports::<HttpOptions>()
        .provider::<HttpServer>()
        .exports::<HttpServer>()
        .lifecycle::<HttpServer>()
        .factory(
            Factory::<UrlSigner>::new(|r: Resolver| async move {
                let options = r.get::<HttpOptions>().await?;
                let config = r.app().try_get::<Config>().await;
                Ok(Arc::new(url_signer(&options, config.as_deref())))
            })
            .deps(vec![ProviderKey::of::<HttpOptions>()]),
        )
        .exports::<UrlSigner>()
        .map_error::<JsonRejection>(|r| Some(rejection(r.status(), r.body_text())))
        .map_error::<FormRejection>(|r| Some(rejection(r.status(), r.body_text())))
        .map_error::<QueryRejection>(|r| Some(rejection(r.status(), r.body_text())))
        .map_error::<PathRejection>(|r| Some(rejection(r.status(), r.body_text())))
        .command(CommandSpec::new(
            "serve",
            "Start the HTTP server ([--addr HOST:PORT])",
            serve,
        ))
        .command(CommandSpec::new(
            "routes",
            "List every route with its access declaration ([--json])",
            routes,
        ))
        .command(CommandSpec::new(
            "down",
            "Put the app into maintenance mode ([--retry SECS] [--message TEXT])",
            down,
        ))
        .command(CommandSpec::new(
            "up",
            "Bring the app out of maintenance mode",
            up,
        ));
}

/// An Axum extractor rejection propagated with `?`: its own status, and its
/// message as the detail of client errors.
fn rejection(status: http::StatusCode, text: String) -> AppError {
    let error = AppError::from_status(status);
    if status.is_client_error() {
        error.with_detail(text)
    } else {
        error.with_source(text)
    }
}

/// The signer for `http.signing_key`; else a key derived from the app key
/// (`app.key`, or top-level `key`, i.e. `APP_KEY`), so signed URLs survive
/// restarts; else a random per-process key.
fn url_signer(options: &HttpOptions, config: Option<&Config>) -> UrlSigner {
    let app_key = || {
        let config = config?;
        ["app.key", "key"]
            .into_iter()
            .filter_map(|key| config.get_string(key))
            .map(|key| key.trim().to_owned())
            .find(|key| !key.is_empty())
    };
    if let Some(key) = options.signing_key.as_ref().map(|k| k.expose().as_str())
        && !key.is_empty()
    {
        return UrlSigner::new(key.as_bytes());
    }
    match app_key() {
        Some(app_key) => UrlSigner::derived_from_app_key(&app_key),
        None => {
            tracing::debug!(
                target: "navree::http",
                "neither http.signing_key nor app.key is set; signed URLs use a random key and stop working after a restart"
            );
            let mut key = Vec::with_capacity(32);
            key.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
            key.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
            UrlSigner::new(&key)
        }
    }
}

/// The HTTP server state shared by the commands and the pipeline: options,
/// maintenance mode and the address `serve` is bound to.
pub struct HttpServer {
    options: Arc<HttpOptions>,
    pub(crate) maintenance: Arc<MaintenanceMode>,
    bound: watch::Sender<Option<SocketAddr>>,
}

impl HttpServer {
    /// The HTTP options.
    pub fn options(&self) -> &HttpOptions {
        &self.options
    }

    /// The maintenance-mode state.
    pub fn maintenance(&self) -> &MaintenanceMode {
        &self.maintenance
    }

    /// The address `serve` listens on, while it runs.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *self.bound.borrow()
    }

    /// Waits until `serve` is listening and returns its address (useful with
    /// `addr = "127.0.0.1:0"`).
    pub async fn wait_bound(&self) -> SocketAddr {
        let mut receiver = self.bound.subscribe();
        loop {
            if let Some(addr) = *receiver.borrow_and_update() {
                return addr;
            }
            if receiver.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }

    fn set_bound(&self, addr: Option<SocketAddr>) {
        self.bound.send_replace(addr);
    }
}

impl std::fmt::Debug for HttpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpServer")
            .field("options", &self.options)
            .field("local_addr", &self.local_addr())
            .finish_non_exhaustive()
    }
}

impl Injectable for HttpServer {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<HttpOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r.get::<HttpOptions>().await?;
            if let Err(errors) = options.check() {
                return Err(KernelError::Construction {
                    provider: std::any::type_name::<HttpOptions>().to_owned(),
                    source: errors.join("; ").into(),
                });
            }
            Ok(Self {
                maintenance: Arc::new(MaintenanceMode::new(options.maintenance_file.clone())),
                options,
                bound: watch::Sender::new(None),
            })
        })
    }
}

/// Builds the router once at boot, so route mistakes fail the boot.
impl Lifecycle for HttpServer {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move { build_router(&ctx).await.map(drop) })
    }
}

/// The value of `--name VALUE` (or `--name=VALUE`) in `args`.
fn flag(args: &[String], name: &str) -> Result<Option<String>, BoxError> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            return iter
                .next()
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("{name} needs a value").into());
        }
        if let Some(value) = arg
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Ok(Some(value.to_owned()));
        }
    }
    Ok(None)
}

/// `serve [--addr HOST:PORT]`: serves until the app's shutdown token is
/// cancelled, then drains in-flight requests for up to `shutdown_grace_secs`.
async fn serve(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let server = ctx.get::<HttpServer>().await?;
    let addr = flag(&args, "--addr")?.unwrap_or_else(|| server.options().addr.clone());
    let router = build_router(&ctx).await?;
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|error| format!("cannot listen on {addr}: {error}"))?;
    let local = listener.local_addr()?;
    server.set_bound(Some(local));
    tracing::info!(target: "navree::http", addr = %local, "listening on http://{local}");

    let token = ctx.shutdown_token();
    let grace = Duration::from_secs(server.options().shutdown_grace_secs);
    let serving = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(token.clone().cancelled_owned())
    .into_future();
    let result = tokio::select! {
        result = serving => result.map_err(BoxError::from),
        () = async {
            token.cancelled().await;
            tokio::time::sleep(grace).await;
        } => {
            tracing::warn!(target: "navree::http", ?grace, "in-flight requests did not finish within the grace period; closing them");
            Ok(())
        }
    };
    server.set_bound(None);
    tracing::info!(target: "navree::http", "server stopped");
    result
}

/// `routes [--json]`: prints the route table.
async fn routes(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let routes = route_table(&ctx).await?;
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&routes_json(&routes))?);
    } else {
        print!("{}", render_routes(&routes));
    }
    Ok(())
}

/// The route table as text columns: method, path, name, access, module.
pub(crate) fn render_routes(routes: &[RouteMeta]) -> String {
    let mut rows: Vec<[String; 5]> = routes
        .iter()
        .map(|r| {
            [
                r.method.clone(),
                r.full_path.clone(),
                r.name.clone().unwrap_or_default(),
                r.access.describe(),
                r.module.unwrap_or("").to_owned(),
            ]
        })
        .collect();
    rows.sort_by(|a, b| (&a[1], &a[0]).cmp(&(&b[1], &b[0])));
    let header = ["METHOD", "PATH", "NAME", "ACCESS", "MODULE"].map(str::to_owned);
    let mut widths = header.clone().map(|h| h.len());
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    let mut out = String::new();
    for row in std::iter::once(&header).chain(&rows) {
        let line: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect();
        out.push_str(line.join("  ").trim_end());
        out.push('\n');
    }
    out.push_str(&format!("\n{} routes\n", rows.len()));
    out
}

fn routes_json(routes: &[RouteMeta]) -> serde_json::Value {
    routes
        .iter()
        .map(|r| {
            serde_json::json!({
                "method": r.method,
                "path": r.full_path,
                "name": r.name,
                "access": r.access.describe(),
                "module": r.module,
                "throttle": r.throttle.map(|t| serde_json::json!({"limit": t.limit, "per_secs": t.per.as_secs_f64()})),
                "timeout_ms": r.timeout.map(|t| t.as_millis() as u64),
                "body_limit": r.body_limit,
                "transactional": r.transactional,
                "audit": r.audit_action,
                "guards": r.guards.len(),
            })
        })
        .collect()
}

/// `down [--retry SECS] [--message TEXT]`.
async fn down(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let server = ctx.get::<HttpServer>().await?;
    let retry = flag(&args, "--retry")?
        .map(|v| v.parse::<u64>())
        .transpose()
        .map_err(|_| "--retry must be a number of seconds")?;
    let info = MaintenanceInfo {
        time: Some(chrono::Utc::now().timestamp()),
        retry,
        message: flag(&args, "--message")?,
    };
    server.maintenance().down(info).await?;
    println!(
        "The application is in maintenance mode ({}).",
        server.maintenance().path().display()
    );
    Ok(())
}

/// `up`.
async fn up(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let server = ctx.get::<HttpServer>().await?;
    server.maintenance().up().await?;
    println!("The application is up.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn route_table_renders_sorted_aligned_columns() {
        let route = |method: &str, path: &str, name: Option<&str>, public: bool| {
            let mut meta = RouteMeta {
                method: method.to_owned(),
                full_path: path.to_owned(),
                name: name.map(str::to_owned),
                module: Some("DealsModule"),
                ..RouteMeta::default()
            };
            meta.access.public = public;
            if !public {
                meta.access.roles = vec!["admin".to_owned()];
            }
            meta
        };
        let text = render_routes(&[
            route("POST", "/api/v1/deals", Some("deals.store"), false),
            route("GET", "/api/v1/deals", None, true),
            route("DELETE", "/api/v1/deals/{id}", None, false),
        ]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                "METHOD  PATH                NAME         ACCESS       MODULE",
                "GET     /api/v1/deals                    public       DealsModule",
                "POST    /api/v1/deals       deals.store  roles:admin  DealsModule",
                "DELETE  /api/v1/deals/{id}               roles:admin  DealsModule",
                "",
                "3 routes",
            ]
        );
        let json = routes_json(&[route("GET", "/x", None, true)]);
        assert_eq!(json[0]["path"], "/x");
        assert_eq!(json[0]["access"], "public");
    }

    #[test]
    fn flags_accept_both_forms() {
        assert_eq!(
            flag(&args(&["--addr", "0.0.0.0:1"]), "--addr").unwrap(),
            Some("0.0.0.0:1".to_owned())
        );
        assert_eq!(
            flag(&args(&["--retry=5"]), "--retry").unwrap(),
            Some("5".to_owned())
        );
        assert_eq!(flag(&args(&["--json"]), "--addr").unwrap(), None);
        assert!(flag(&args(&["--addr"]), "--addr").is_err());
    }
}
