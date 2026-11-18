//! A plugin's middleware: only the app adds a layer for every request; a plugin's own routes and
//! mounts carry their own.

#[path = "http/support.rs"]
mod support;

use http::{Method, StatusCode};
use navree_http::body::Body;
use navree_http::{
    AppError, CurrentPrincipal, LayerSlot, ModuleBuilderHttpExt, ModuleBuilderMountExt, RouteDef,
};

use navree_plugin_khata::LedgerModule;
use support::Setup;

/// Harpreet Sandhu, signed in.
const HARPREET: &str = "0192f0c1-0000-7000-8000-0000000000a1";

/// The khata plugin, a community plugin whose crate is this module: the test app's own
/// modules are in no crate it names.
mod navree_plugin_khata {
    use std::fmt;
    use std::sync::Arc;

    use http::{HeaderValue, Method};
    use navree_http::extract::Request;
    use navree_http::handler::Endpoint;
    use navree_http::middleware::{self, Next};
    use navree_http::response::Response;
    use navree_http::{AppError, AppRouter, Controller, GlobalLayer, LayerSlot, Mount, RouteDef};
    use navree_kernel::{
        BoxFuture, Injectable, KernelError, Module, ModuleBuilder, PluginInfo, Resolver,
    };

    /// The plugin's info.
    pub const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    /// Adds `x-khata: 1` to the answer of what it wraps.
    async fn stamp_answer(request: Request, next: Next) -> Response {
        let mut response = next.run(request).await;
        response
            .headers_mut()
            .insert("x-khata", HeaderValue::from_static("1"));
        response
    }

    /// A layer for every request that takes every caller for Harpreet Sandhu: in the
    /// `Session` slot, before authentication, it sets the `x-user-id` the test
    /// authenticator reads.
    pub fn trust_me() -> GlobalLayer {
        GlobalLayer::new(LayerSlot::Session, "trust-me", |_ctx, router| {
            Ok(router.layer(middleware::from_fn(
                |mut request: Request, next: Next| async move {
                    request
                        .headers_mut()
                        .insert("x-user-id", HeaderValue::from_static(super::HARPREET));
                    next.run(request).await
                },
            )))
        })
    }

    /// A layer for every request, in `slot`, that adds `x-khata: 1` to every answer.
    pub fn stamp(slot: LayerSlot) -> GlobalLayer {
        GlobalLayer::new(slot, "stamp", |_ctx, router| {
            Ok(router.layer(middleware::from_fn(stamp_answer)))
        })
    }

    /// The plugin's routes: `GET /api/khata/status`, public, with a layer of its own that
    /// adds `x-khata: 1`.
    pub struct KhataRoutes;

    impl Injectable for KhataRoutes {
        fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async { Ok(KhataRoutes) })
        }
    }

    impl Controller for KhataRoutes {
        fn prefix(&self) -> &'static str {
            "/khata"
        }

        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            vec![
                RouteDef::with_endpoint(
                    Method::GET,
                    "/status",
                    Endpoint::new(|| async { "open" }).layer(middleware::from_fn(stamp_answer)),
                )
                .public(),
            ]
        }
    }

    /// The plugin's mount: its ledger under `/khata`, public, with a layer of its own that
    /// adds `x-khata: 1`.
    pub fn ledger_mount() -> Mount {
        let ledger = AppRouter::from_handler(|| async { "entries" });
        Mount::new("/khata", ledger.layer(middleware::from_fn(stamp_answer))).public()
    }

    /// The plugin's own error: the khata is closed for the day.
    #[derive(Debug)]
    pub struct LedgerClosed;

    impl fmt::Display for LedgerClosed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("the khata is closed for the day")
        }
    }

    impl std::error::Error for LedgerClosed {}

    /// The plugin's answer to its own error: 409.
    pub fn closed(_: &LedgerClosed) -> Option<AppError> {
        Some(AppError::conflict(
            "The khata of Ludhiana is closed for the day.",
        ))
    }

    /// The plugin's module: it names the plugin, then registers what `.0` adds.
    pub struct LedgerModule(Arc<dyn Fn(&mut ModuleBuilder) + Send + Sync>);

    impl LedgerModule {
        /// The plugin's module that registers what `register` adds.
        pub fn new(register: impl Fn(&mut ModuleBuilder) + Send + Sync + 'static) -> Self {
            Self(Arc::new(register))
        }
    }

    impl Module for LedgerModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
            (self.0)(m);
        }
    }

    /// A module of the plugin's crate that names no plugin itself.
    pub struct Helper(pub fn(&mut ModuleBuilder));

    impl Module for Helper {
        fn register(&self, m: &mut ModuleBuilder) {
            (self.0)(m);
        }
    }
}

/// The app's routes: `GET /api/me`, for signed-in users, answers the caller's id.
fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::on(
            Method::GET,
            "/me",
            |CurrentPrincipal(principal): CurrentPrincipal| async move {
                principal.user_id.to_string()
            },
        )
        .authenticated(),
    ]
}

/// A request from `user` (the test authenticator reads `x-user-id`).
fn as_user(method: &str, path: &str, user: &str) -> http::Request<Body> {
    http::Request::builder()
        .method(method)
        .uri(path)
        .header("x-user-id", user)
        .body(Body::empty())
        .unwrap()
}

/// The refusal of the layer `name` that the khata plugin's `LedgerModule` adds.
fn refusal(name: &str) -> String {
    format!(
        "plugin `navree-plugin-khata` adds the app-wide layer `{name}` in `LedgerModule`, which runs before every route's access check; only a module of the app adds app-wide layers: remove the plugin or update it to a version that leaves the layer to the app, and to use the plugin's layer, add it from a module of the app (m.http_layer)"
    )
}

#[tokio::test]
async fn a_plugin_http_layer_fails_the_boot() {
    // A layer that takes every caller for Harpreet Sandhu, before any route's access check.
    let error = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|m| {
            m.http_layer(navree_plugin_khata::trust_me());
        }))
        .boot_error()
        .await;
    assert!(error.contains(&refusal("trust-me")), "{error}");
    assert_eq!(
        error.matches("adds the app-wide layer").count(),
        1,
        "one refusal per layer: {error}"
    );

    // Every slot, the edge included.
    for slot in [
        LayerSlot::Edge,
        LayerSlot::Session,
        LayerSlot::AuthContext,
        LayerSlot::Uow,
        LayerSlot::Context,
    ] {
        let error = Setup::new(routes())
            .authenticated()
            .module(LedgerModule::new(move |m| {
                m.http_layer(navree_plugin_khata::stamp(slot));
            }))
            .boot_error()
            .await;
        assert!(error.contains(&refusal("stamp")), "{slot:?}: {error}");
    }

    // A layer contributed directly, as `http_layer` contributes it.
    let error = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|m| {
            m.contribute(navree_plugin_khata::stamp(LayerSlot::Context));
        }))
        .boot_error()
        .await;
    assert!(error.contains(&refusal("stamp")), "{error}");
}

#[tokio::test]
async fn a_plugin_route_layer_and_mount_boot() {
    let app = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|m| {
            m.controller::<navree_plugin_khata::KhataRoutes>()
                .mount(navree_plugin_khata::ledger_mount());
        }))
        .boot()
        .await;

    let status = app.get("/api/khata/status").await;
    assert_eq!(status.status, StatusCode::OK, "{}", status.text());
    assert_eq!(status.text(), "open");
    assert_eq!(status.header("x-khata"), Some("1"));
    let ledger = app.get("/khata/entries").await;
    assert_eq!(ledger.status, StatusCode::OK, "{}", ledger.text());
    assert_eq!(ledger.text(), "entries");
    assert_eq!(ledger.header("x-khata"), Some("1"));

    // The app's own route carries none of the plugin's layers, and still asks who calls.
    let me = app.send(as_user("GET", "/api/me", HARPREET)).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    assert_eq!(me.text(), HARPREET);
    assert_eq!(me.header("x-khata"), None);
    app.get("/api/me").await.assert_error(401);
}

#[tokio::test]
async fn the_app_may_add_a_plugins_layer() {
    let app = Setup::new(routes())
        .authenticated()
        .register(|m| {
            m.http_layer(navree_plugin_khata::trust_me());
        })
        .module(LedgerModule::new(|_| {}))
        .boot()
        .await;

    // The app's own choice, made in a module of its own: a guest is taken for Harpreet Sandhu.
    let me = app.get("/api/me").await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    assert_eq!(me.text(), HARPREET);

    // The module that adds the layer is the app's: the plugin lists its own module only.
    let plugins = app.app.ctx().plugins().plugins;
    assert_eq!(plugins.len(), 1);
    let modules: Vec<&str> = plugins[0]
        .modules
        .iter()
        .map(|module| module.id.name())
        .collect();
    assert_eq!(modules, ["LedgerModule"]);
}

/// An answer to every input or output error: 503, the khata is busy.
fn busy(_: &std::io::Error) -> Option<AppError> {
    Some(AppError::service_unavailable(
        "The khata of Amritsar is busy.",
    ))
}

/// The refusal of the khata plugin's `LedgerModule` mapping the errors of type `error`.
fn maps(error: &str) -> String {
    format!(
        "plugin `navree-plugin-khata` answers the app's errors of type `{error}` (m.map_error in `LedgerModule`), a type of another crate; only a module of the app maps another crate's errors: remove the plugin or update it to a version that maps only its own errors, and to use the plugin's mapping, register it from a module of the app (m.map_error)"
    )
}

#[tokio::test]
async fn a_plugin_mapping_of_another_crates_errors_fails_the_boot() {
    // Every input or output error the app's own code raises would answer 503, with the
    // plugin's text.
    let error = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|m| {
            m.map_error::<std::io::Error>(busy);
        }))
        .boot_error()
        .await;
    let expected = maps(std::any::type_name::<std::io::Error>());
    assert!(error.contains(&expected), "{error}");

    // Its own error type is the plugin's to map.
    Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|m| {
            m.map_error::<navree_plugin_khata::LedgerClosed>(navree_plugin_khata::closed);
        }))
        .boot()
        .await;

    // The app maps another crate's errors from a module of its own, beside the plugin.
    Setup::new(routes())
        .authenticated()
        .register(|m| {
            m.map_error::<std::io::Error>(busy);
        })
        .module(LedgerModule::new(|_| {}))
        .boot()
        .await;
}

#[tokio::test]
async fn the_refusal_says_why_the_module_acts_for_the_plugin() {
    use navree_http::response::Response;
    use navree_http::{ErrorContext, ErrorPages};
    use navree_kernel::BoxFuture;

    struct DeclinePages;

    impl ErrorPages for DeclinePages {
        fn render<'a>(
            &'a self,
            _: &'a ErrorContext,
            _: &'a AppError,
        ) -> BoxFuture<'a, Option<Response>> {
            Box::pin(async { None })
        }
    }

    mod khata_core {
        use navree_kernel::{Module, ModuleBuilder};

        pub struct SecondCrate(pub fn(&mut ModuleBuilder));

        impl Module for SecondCrate {
            fn register(&self, m: &mut ModuleBuilder) {
                (self.0)(m);
            }
        }
    }

    let krate = navree_plugin_khata::KHATA.crate_name;
    let io_error = std::any::type_name::<std::io::Error>();

    // 1. Module of the plugin's crate (`Helper`): layers, error pages and error mappers.
    let in_crate = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|_| {}))
        .module(navree_plugin_khata::Helper(|m| {
            m.http_layer(navree_plugin_khata::trust_me())
                .error_pages(DeclinePages)
                .map_error::<std::io::Error>(busy);
        }))
        .boot_error()
        .await;
    // `mappers_of_plugins` runs before `build_router`, so check mapper first, then layer + pages.
    assert!(
        in_crate.contains(&format!(
            "(m.map_error in `Helper` (a module of the plugin's crate `{krate}`))",
        )),
        "{in_crate}"
    );

    let in_crate_router = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|_| {}))
        .module(navree_plugin_khata::Helper(|m| {
            m.http_layer(navree_plugin_khata::trust_me())
                .error_pages(DeclinePages);
        }))
        .boot_error()
        .await;
    assert!(
        in_crate_router.contains(&format!(
            "adds the app-wide layer `trust-me` in `Helper` (a module of the plugin's crate `{krate}`),"
        )),
        "{in_crate_router}"
    );
    assert!(
        in_crate_router.contains(&format!(
            "(m.error_pages in `Helper` (a module of the plugin's crate `{krate}`));"
        )),
        "{in_crate_router}"
    );

    // 2. Module of a second crate imported only by the plugin (`SecondCrate`).
    let by_import_mapper = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|m| {
            m.import(khata_core::SecondCrate(|m| {
                m.map_error::<std::io::Error>(busy);
            }));
        }))
        .boot_error()
        .await;
    assert!(
        by_import_mapper.contains(&format!(
            "answers the app's errors of type `{io_error}` (m.map_error in `SecondCrate` (only the plugin's modules import it)),"
        )),
        "{by_import_mapper}"
    );

    let by_import_router = Setup::new(routes())
        .authenticated()
        .module(LedgerModule::new(|m| {
            m.import(khata_core::SecondCrate(|m| {
                m.http_layer(navree_plugin_khata::trust_me())
                    .error_pages(DeclinePages);
            }));
        }))
        .boot_error()
        .await;
    assert!(
        by_import_router.contains(
            "adds the app-wide layer `trust-me` in `SecondCrate` (only the plugin's modules import it),"
        ),
        "{by_import_router}"
    );
    assert!(
        by_import_router
            .contains("(m.error_pages in `SecondCrate` (only the plugin's modules import it));"),
        "{by_import_router}"
    );
}
