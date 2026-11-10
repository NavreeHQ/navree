//! [`GrpcModule`]: options, the server, the channels, and the layers that
//! put gRPC on the HTTP port.

use navree_config::ModuleBuilderConfigExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{ConfiguredModule, Module, ModuleBuilder};

use crate::context::http_context_layer;
use crate::edge::shared_port_layer;
use crate::{GrpcChannels, GrpcOptions, GrpcServer, error_from_status};

/// The gRPC module: serves the tonic services that modules register with
/// `m.grpc_service::<T>()`, on the HTTP port (gRPC, gRPC-Web and Connect
/// next to the routes) or on `grpc.addr`, with health and reflection, and
/// provides [`GrpcChannels`] for calling other services.
///
/// Global; provides and exports [`GrpcOptions`], [`GrpcServer`] and
/// [`GrpcChannels`]. It needs `HttpModule`; with `DatabaseModule` every
/// call gets a unit of work. Register it after `DatabaseModule`, so the
/// gRPC calls an HTTP handler makes inherit its request's context.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_grpc::GrpcModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(GrpcModule::from_config()) // section `grpc`
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(GrpcModule)`) or through
/// [`from_config`](GrpcModule::from_config), it reads the `grpc`
/// configuration section, so the app must include `ConfigModule`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GrpcModule;

impl GrpcModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: GrpcOptions) -> impl Module {
        ConfiguredModule::<GrpcModule, _>::for_root(options, register)
    }

    /// The module configured from the `grpc` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<GrpcModule, GrpcOptions>::from_config(register)
    }
}

impl Module for GrpcModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<GrpcOptions>) {
    m.global();
    match options {
        Some(options) => m.value(options),
        None => m.config::<GrpcOptions>(),
    };
    m.exports::<GrpcOptions>()
        .provider::<GrpcServer>()
        .exports::<GrpcServer>()
        .lifecycle::<GrpcServer>()
        .provider::<GrpcChannels>()
        .exports::<GrpcChannels>()
        .http_layer(shared_port_layer())
        .http_layer(http_context_layer())
        .map_error::<tonic::Status>(|status| Some(error_from_status(status)));
}
