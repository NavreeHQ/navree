//! What the app's gRPC methods require under a plugin's namespace: the boot reads a method's
//! declaration, and a wrapped tonic server's, as it reads a route's.

use std::sync::Arc;

use navree_contracts::Permission;
use navree_grpc::{GrpcMethod, GrpcService, ModuleBuilderGrpcExt, ModuleBuilderTonicExt};
use navree_grpc::{Request, Response, Routes, ServiceLimits, Status, TonicService};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::injectable;
use navree_testing::TestAppBuilder;

use crate::support::deals_app;
use crate::support::pb::ping::ping_server::{Ping, PingServer};
use crate::support::pb::ping::{PingReply, PingRequest};

use navree_plugin_reports::ReportsPlugin;

/// The reports plugin, a community plugin whose crate is this module: its namespace is
/// `reports`.
mod navree_plugin_reports {
    use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    const REPORTS: PluginInfo =
        PluginInfo::new("navree-plugin-reports", module_path!(), "reports", "1.0.0");

    /// The plugin's module: it names the plugin and describes the permissions `.0`.
    pub struct ReportsPlugin(pub &'static [Permission]);

    impl Module for ReportsPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(REPORTS).permissions(self.0);
        }
    }
}

/// What the reports plugin describes, when it describes anything.
const REPORTS_VIEW: &[Permission] = &[Permission::new(
    "reports.view",
    "See the sales reports of the Ludhiana office",
)];

/// The tail of the refusal of a permission under a plugin's namespace that the plugin does not
/// describe.
const NOT_DESCRIBED: &str = ", and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";

/// The tail of the refusal of a subject under a plugin's namespace that is not registered as
/// the plugin's.
const NOT_REGISTERED: &str = ", and the plugin registers no subject of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: check a subject the plugin registers, or name the app's subject under another word";

/// The app's sales service, written without the macro: its one method asks for
/// `reports.view`.
#[injectable]
struct SalesPing;

#[tonic::async_trait]
impl Ping for SalesPing {
    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingReply>, Status> {
        Ok(Response::new(PingReply {
            message: request.into_inner().message,
            ..PingReply::default()
        }))
    }
}

impl GrpcService for SalesPing {
    const NAME: &'static str = "navree.test.v1.Ping";

    fn methods(self: &Arc<Self>) -> Vec<GrpcMethod> {
        vec![GrpcMethod::new("ping").permissions(&["reports.view"])]
    }

    fn add_to(self: Arc<Self>, routes: Routes, _limits: &ServiceLimits) -> Routes {
        routes.add_service(PingServer::from_arc(self))
    }
}

/// A module of the app that serves [`SalesPing`].
struct SalesModule;

impl Module for SalesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.grpc_service::<SalesPing>();
    }
}

/// The ping service of another crate, written by hand.
struct PlainPing;

#[tonic::async_trait]
impl Ping for PlainPing {
    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingReply>, Status> {
        Ok(Response::new(PingReply {
            message: request.into_inner().message,
            ..PingReply::default()
        }))
    }
}

/// The tonic server of [`PlainPing`], wrapped for the app.
type Wrapped = TonicService<PingServer<PlainPing>>;

/// A module of the app that serves the tonic server of [`PlainPing`], with the access `.0`
/// declares.
struct WrappedSales(fn(Wrapped) -> Wrapped);

impl Module for WrappedSales {
    fn register(&self, m: &mut ModuleBuilder) {
        m.tonic_service((self.0)(TonicService::new(PingServer::new(PlainPing))));
    }
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    builder
        .try_build()
        .await
        .map(drop)
        .expect_err("the boot must fail")
        .to_string()
}

#[tokio::test]
async fn a_grpc_method_under_a_plugins_namespace_is_read_as_a_route_is() {
    // The method asks for `reports.view`, which no subject maps and the reports plugin does
    // not describe: the plugin's policies would decide who may call it.
    let app = deals_app().module(SalesModule).module(ReportsPlugin(&[]));
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-reports` uses the namespace `reports`, the first word of the permission `reports.view` that the gRPC method `ping` (`navree.test.v1.Ping`) of `SalesModule` requires{NOT_DESCRIBED}"
    );
    assert!(error.contains(&expected), "{error}");

    // The plugin describes `reports.view`: the method requires it on purpose, and the app
    // boots.
    deals_app()
        .module(SalesModule)
        .module(ReportsPlugin(REPORTS_VIEW))
        .try_build()
        .await
        .map(drop)
        .expect("a method of the app may require a permission the plugin describes");

    // Without the plugin the app boots as before.
    deals_app()
        .module(SalesModule)
        .try_build()
        .await
        .map(drop)
        .expect("the app boots without the plugin");
}

#[tokio::test]
async fn a_wrapped_tonic_server_under_a_plugins_namespace_is_read_as_a_route_is() {
    // The service default of a wrapped server.
    let wrapped = WrappedSales(|service| service.can("view", "reports"));
    let app = deals_app().module(wrapped).module(ReportsPlugin(&[]));
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-reports` uses the namespace `reports`, the first word of the subject `reports` that the gRPC service `navree.test.v1.Ping` of `WrappedSales` checks{NOT_REGISTERED}"
    );
    assert!(error.contains(&expected), "{error}");

    // A method of a wrapped server with a declaration of its own.
    let wrapped = WrappedSales(|service| {
        service
            .authenticated()
            .method(GrpcMethod::new("ping").permissions(&["reports.view"]))
    });
    let app = deals_app().module(wrapped).module(ReportsPlugin(&[]));
    let error = boot_error(app).await;
    let expected = format!(
        "plugin `navree-plugin-reports` uses the namespace `reports`, the first word of the permission `reports.view` that the gRPC method `ping` (`navree.test.v1.Ping`) of `WrappedSales` requires{NOT_DESCRIBED}"
    );
    assert!(error.contains(&expected), "{error}");

    // The plugin describes the permission: the wrapped server's method may require it.
    let wrapped = WrappedSales(|service| {
        service
            .authenticated()
            .method(GrpcMethod::new("ping").permissions(&["reports.view"]))
    });
    deals_app()
        .module(wrapped)
        .module(ReportsPlugin(REPORTS_VIEW))
        .try_build()
        .await
        .map(drop)
        .expect("a wrapped server's method may require a permission the plugin describes");
}

mod navree_plugin_mapper {
    use navree_http::{AppError, ModuleBuilderHttpExt};
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    const MAPPER: PluginInfo =
        PluginInfo::new("navree-plugin-mapper", module_path!(), "mapper", "1.0.0");

    pub struct MapperPlugin;

    impl Module for MapperPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(MAPPER)
                .map_error::<std::io::Error>(|_| Some(AppError::forbidden()));
        }
    }
}

#[tokio::test]
async fn a_plugins_mapping_of_another_crates_error_is_refused_in_an_app_with_no_http_module() {
    use std::any::type_name;

    use navree_grpc::{GrpcModule, GrpcOptions};
    use navree_http::HttpOptions;
    use navree_kernel::Navree;

    // Plain `GrpcModule` with neither `HttpModule` nor `HttpOptions` refuses to boot at once.
    let no_http = Navree::app()
        .module(GrpcModule::for_root(GrpcOptions {
            health: false,
            reflection: false,
            ..GrpcOptions::default()
        }))
        .boot()
        .await
        .expect_err("GrpcModule needs HttpOptions")
        .to_string();
    assert!(
        no_http.contains("GrpcModule needs HttpModule in the app"),
        "{no_http}"
    );

    // Even when `HttpOptions` is supplied without `HttpModule` (so `HttpServer`'s bootstrap
    // never runs), `GrpcServer` still checks plugin error mappers at boot.
    struct StandaloneHttpOptions;
    impl Module for StandaloneHttpOptions {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value(HttpOptions::default()).exports::<HttpOptions>();
        }
    }

    let app = Navree::app()
        .module(StandaloneHttpOptions)
        .module(GrpcModule::for_root(GrpcOptions {
            health: false,
            reflection: false,
            ..GrpcOptions::default()
        }))
        .boot()
        .await
        .expect("boots with HttpOptions and no plugin mapper");
    app.shutdown().await.unwrap();

    let error = Navree::app()
        .module(StandaloneHttpOptions)
        .module(GrpcModule::for_root(GrpcOptions {
            health: false,
            reflection: false,
            ..GrpcOptions::default()
        }))
        .module(navree_plugin_mapper::MapperPlugin)
        .boot()
        .await
        .expect_err("foreign error mapper from a plugin must fail the boot")
        .to_string();
    let io_error = type_name::<std::io::Error>();
    let expected = format!(
        "plugin `navree-plugin-mapper` answers the app's errors of type `{io_error}` (m.map_error in `MapperPlugin`), a type of another crate; only a module of the app maps another crate's errors: remove the plugin or update it to a version that maps only its own errors, and to use the plugin's mapping, register it from a module of the app (m.map_error)"
    );
    assert!(
        error.contains(&expected),
        "expected `{expected}` in:\n{error}"
    );
}
