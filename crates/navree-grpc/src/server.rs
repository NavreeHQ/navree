//! [`GrpcServer`]: the registered services, checked and assembled at boot,
//! and the separate listener (`grpc.addr`).

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

use navree_contracts::{AuditSink, HealthCheck};
use navree_db::Db;
use navree_http::body::Body;
use navree_http::{
    AccessSpec, Authenticator, ErrorMapper, HttpOptions, HttpServer, RouteAuthorizer,
};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Lifecycle, ProviderKey, Resolver,
};
use prost_reflect::DescriptorPool;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::call::{Runtime, ServiceRuntime, UndeclaredAudits};
use crate::connect::Connect;
use crate::edge::{Edge, EdgeParts, GrpcCore};
use crate::health::ReadinessHealth;
use crate::service::{DescriptorSet, ServiceLimits, ServiceRegistration};
use crate::tonic_service::{TonicRegistration, boot_problems};
use crate::{GrpcOptions, Routes};

/// The name of the health service.
const HEALTH: &str = "grpc.health.v1.Health";

/// The gRPC server state: the options, the services assembled at boot and
/// the address of the separate listener while it runs. Registered and
/// exported by [`GrpcModule`](crate::GrpcModule).
///
/// At boot it resolves every service registered with
/// `m.grpc_service::<T>()` and checks each method, failing the boot (with
/// every problem listed) when a method declares no access, is public and
/// restricted at once, needs an authenticator or a route authorizer the
/// app does not register, or when two modules register the same service.
/// It then checks every tonic server wrapped with `m.tonic_service(..)`
/// ([`TonicService`](crate::TonicService)) by the same rule, its service
/// default and each method declared with `.method(..)`.
pub struct GrpcServer {
    options: Arc<GrpcOptions>,
    edge: OnceLock<Arc<Edge>>,
    services: OnceLock<Vec<&'static str>>,
    bound: watch::Sender<Option<SocketAddr>>,
}

impl GrpcServer {
    /// The gRPC options.
    pub fn options(&self) -> &GrpcOptions {
        &self.options
    }

    /// The names of the registered services (after boot), e.g.
    /// `["crm.deals.v1.Deals"]`: the `#[grpc_service]` services, then the
    /// wrapped tonic servers, each in registration order.
    pub fn services(&self) -> &[&'static str] {
        self.services.get().map_or(&[], Vec::as_slice)
    }

    /// The address of the separate listener (`grpc.addr`), while it runs.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *self.bound.borrow()
    }

    /// Waits until the separate listener is listening and returns its
    /// address (useful with `addr = "127.0.0.1:0"`).
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

    pub(crate) fn edge(&self) -> Option<&Arc<Edge>> {
        self.edge.get()
    }

    /// Serves one request in-process (what `TestChannel` sends).
    #[cfg_attr(not(feature = "testing"), allow(dead_code))]
    pub(crate) async fn serve_in_process(
        &self,
        request: http::Request<Body>,
    ) -> http::Response<Body> {
        match self.edge() {
            Some(edge) => edge.handle(request).await,
            None => {
                tonic::Status::unavailable("the gRPC server has not booted").into_http::<Body>()
            }
        }
    }
}

impl fmt::Debug for GrpcServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrpcServer")
            .field("options", &self.options)
            .field("services", &self.services())
            .field("local_addr", &self.local_addr())
            .finish_non_exhaustive()
    }
}

impl Injectable for GrpcServer {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<GrpcOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r.get::<GrpcOptions>().await?;
            if let Err(problems) = options.check() {
                return Err(KernelError::Construction {
                    provider: std::any::type_name::<GrpcOptions>().to_owned(),
                    source: problems.join("; ").into(),
                });
            }
            Ok(Self {
                options,
                edge: OnceLock::new(),
                services: OnceLock::new(),
                bound: watch::Sender::new(None),
            })
        })
    }
}

/// Every problem found while assembling the services.
#[derive(Debug)]
pub struct GrpcErrors(pub Vec<String>);

impl fmt::Display for GrpcErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

impl std::error::Error for GrpcErrors {}

/// Assembles the services at boot (so mistakes fail it) and, with
/// `grpc.addr`, starts the separate listener once `serve` is listening.
impl Lifecycle for GrpcServer {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let (edge, names) = build(&ctx, &self.options).await?;
            let edge = Arc::new(edge);
            let _ = self.services.set(names);
            if self.edge.set(edge.clone()).is_err() {
                return Ok(());
            }
            if let Some(addr) = self.options.addr {
                let server = ctx.get::<GrpcServer>().await?;
                let http = ctx.try_get::<HttpServer>().await;
                let token = ctx.shutdown_token();
                let http_options = ctx
                    .try_get::<HttpOptions>()
                    .await
                    .map_or_else(HttpOptions::default, |options| (*options).clone());
                ctx.tracker().spawn(async move {
                    // Only `serve` binds the HTTP port; other commands
                    // (migrate, routes, ...) never open the gRPC one.
                    let Some(http) = http else { return };
                    tokio::select! {
                        _ = http.wait_bound() => {}
                        () = token.cancelled() => return,
                    }
                    if let Err(error) =
                        listen(&server, edge, addr, token.clone(), http_options).await
                    {
                        tracing::error!(target: "navree::grpc", %addr, %error, "the gRPC listener failed; stopping the app");
                        token.cancel();
                    }
                });
            }
            Ok(())
        })
    }
}

/// Resolves and checks every service and builds the edge.
async fn build(
    ctx: &AppContext,
    options: &GrpcOptions,
) -> Result<(Edge, Vec<&'static str>), BoxError> {
    let http = ctx
        .get::<HttpOptions>()
        .await
        .map_err(|error| format!("GrpcModule needs HttpModule in the app: {error}"))?;
    let authenticator = ctx.try_get::<dyn Authenticator>().await;
    let authorizer = ctx.try_get::<dyn RouteAuthorizer>().await;
    let limits = ServiceLimits {
        max_message_bytes: options.max_message_bytes,
    };

    let mut problems = Vec::new();
    let mut routes = Routes::default();
    let mut services: HashMap<&'static str, Arc<ServiceRuntime>> = HashMap::new();
    let mut owners: HashMap<&'static str, &'static str> = HashMap::new();
    let mut names = Vec::new();
    for registration in ctx.contributions::<ServiceRegistration>() {
        let module = registration.module.name;
        let resolver = ctx.resolver_for(registration.value.module);
        let resolved = (registration.value.resolve)(resolver).await?;
        if let Some(first) = owners.insert(resolved.name, resolved.type_name) {
            problems.push(format!(
                "gRPC service `{}` is registered twice ({} in module `{module}`, and {first})",
                resolved.name, resolved.type_name
            ));
            continue;
        }
        for method in &resolved.methods {
            let label = format!(
                "gRPC method `{}` of `{}` (module `{module}`)",
                method.name, resolved.name
            );
            problems.extend(access_problems(
                &label,
                &method.meta.access,
                "#[public], #[authenticated], #[roles], #[permissions] or #[can]",
                "method",
                authenticator.is_some(),
                authorizer.is_some(),
            ));
        }
        names.push(resolved.name);
        services.insert(
            resolved.name,
            Arc::new(ServiceRuntime::new(
                resolved.name,
                registration.value.module,
                module,
                resolved.methods,
            )),
        );
        routes = (resolved.add)(routes, &limits);
    }
    // Tonic servers built elsewhere, each behind the call pipeline.
    for registration in ctx.contributions::<TonicRegistration>() {
        let module = registration.module.name;
        let module_id = registration.module.id;
        let wrapped = &*registration.value;
        if let Some(first) = owners.insert(wrapped.name, wrapped.type_name) {
            problems.push(format!(
                "gRPC service `{}` is registered twice ({} in module `{module}`, and {first})",
                wrapped.name, wrapped.type_name
            ));
            continue;
        }
        problems.extend(boot_problems(
            wrapped,
            module,
            authenticator.is_some(),
            authorizer.is_some(),
            options.health,
            options.reflection,
        ));
        names.push(wrapped.name);
        services.insert(
            wrapped.name,
            Arc::new(
                ServiceRuntime::new(wrapped.name, module_id, module, wrapped.methods.clone())
                    .with_default(wrapped.default.clone(), module_id, module),
            ),
        );
        routes = (wrapped.add)(routes);
    }
    if !problems.is_empty() {
        return Err(GrpcErrors(problems).into());
    }

    let mut known: HashSet<String> = names.iter().map(|name| (*name).to_owned()).collect();
    if options.health {
        let checks = ctx.get_all::<dyn HealthCheck>().await?;
        let health = ReadinessHealth {
            checks,
            services: known.clone(),
        };
        routes = routes.add_service(
            tonic_health::pb::health_server::HealthServer::new(health)
                .max_decoding_message_size(limits.max_message_bytes),
        );
        known.insert(HEALTH.to_owned());
    }

    let sets: Vec<&'static [u8]> = ctx
        .contributions::<DescriptorSet>()
        .iter()
        .map(|c| c.value.0)
        .collect();
    let mut pool = DescriptorPool::new();
    let mut described = sets.clone();
    if options.health {
        described.push(tonic_health::pb::FILE_DESCRIPTOR_SET);
    }
    for set in &described {
        pool.decode_file_descriptor_set(*set).map_err(|error| {
            format!("a descriptor set registered with grpc_descriptors is invalid: {error}")
        })?;
    }
    if options.reflection {
        let mut v1 = tonic_reflection::server::Builder::configure();
        let mut v1alpha = tonic_reflection::server::Builder::configure();
        for set in &described {
            v1 = v1.register_encoded_file_descriptor_set(set);
            v1alpha = v1alpha.register_encoded_file_descriptor_set(set);
        }
        routes = routes
            .add_service(
                v1.build_v1()
                    .map_err(|error| format!("server reflection: {error}"))?,
            )
            .add_service(
                v1alpha
                    .build_v1alpha()
                    .map_err(|error| format!("server reflection: {error}"))?,
            );
        known.insert("grpc.reflection.v1.ServerReflection".to_owned());
        known.insert("grpc.reflection.v1alpha.ServerReflection".to_owned());
    }

    let runtime = Arc::new(Runtime {
        ctx: ctx.downgrade(),
        authenticator,
        authorizer,
        audit: ctx.try_get::<dyn AuditSink>().await,
        db: ctx.try_get::<Db>().await,
        mappers: ctx
            .contributions::<ErrorMapper>()
            .into_iter()
            .map(|c| (*c.value).clone())
            .collect(),
        default_timeout: options.default_timeout(http.timeout_secs),
        shutdown: ctx.shutdown_token(),
        undeclared_audits: UndeclaredAudits::default(),
    });
    let core = GrpcCore::new(runtime.clone(), services, routes);
    let connect = options.connect.then(|| {
        Arc::new(Connect {
            pool,
            max_message_bytes: options.max_message_bytes,
        })
    });
    let edge = Edge::new(EdgeParts {
        runtime,
        core,
        http,
        known,
        web: options.web,
        connect,
        shares_http_port: options.addr.is_none(),
    })?;
    Ok((edge, names))
}

/// The access rule a `#[grpc_service]` method and a wrapped tonic server
/// are both checked by, in this order: no access declaration (and then
/// nothing else), public with another declaration, no [`Authenticator`] for
/// a declaration that needs a user, and no [`RouteAuthorizer`] for roles,
/// permissions or an ability. `label` names the method or service,
/// `declare` lists what declares access (`#[public], ..` or
/// `.public(), ..`), and `noun` (`method`, `service`) is what the
/// authenticator text tells the developer to mark public.
pub(crate) fn access_problems(
    label: &str,
    access: &AccessSpec,
    declare: &str,
    noun: &str,
    authenticator: bool,
    authorizer: bool,
) -> Vec<String> {
    if !access.is_declared() {
        return vec![format!(
            "{label} has no access declaration: add {declare} (deny by default)"
        )];
    }
    let mut problems = Vec::new();
    if access.public && (access.authenticated || access.needs_authorizer()) {
        problems.push(format!(
            "{label} is public but also declares {}; remove one of them",
            access.describe()
        ));
    }
    if !access.public && !authenticator {
        problems.push(format!(
            "{label} requires authentication but no `dyn Authenticator` is registered (import the auth module, or mark the {noun} public)"
        ));
    }
    if access.needs_authorizer() && !authorizer {
        problems.push(format!(
            "{label} declares {} but no `dyn RouteAuthorizer` is registered (import the access module)",
            access.describe()
        ));
    }
    problems
}

/// Serves the edge on its own listener, on the HTTP server with the
/// `http.server` limits, until `token` is cancelled, then lets open calls
/// finish for up to `http.shutdown_grace_secs`.
async fn listen(
    server: &GrpcServer,
    edge: Arc<Edge>,
    addr: SocketAddr,
    token: CancellationToken,
    mut options: HttpOptions,
) -> Result<(), BoxError> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("cannot listen on {addr}: {error}"))?;
    let local = listener.local_addr()?;
    server.bound.send_replace(Some(local));
    tracing::info!(target: "navree::grpc", addr = %local, "gRPC listening on {local}");
    options.server.http2_max_concurrent_streams = server.options.concurrency_per_connection;
    let service = tower::service_fn(move |request: http::Request<Body>| {
        let edge = edge.clone();
        async move { Ok::<_, Infallible>(edge.handle_from(request).await) }
    });
    let served = navree_http::serve(listener, service)
        .options(&options)
        .with_graceful_shutdown(token.cancelled_owned())
        .await;
    server.bound.send_replace(None);
    served?;
    tracing::info!(target: "navree::grpc", "gRPC listener stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use navree_http::AccessSpec;

    use super::access_problems;

    const ATTRIBUTES: &str = "#[public], #[authenticated], #[roles], #[permissions] or #[can]";

    #[test]
    fn the_access_rule_reads_the_same_for_a_method_and_a_wrapped_service() {
        let label = "gRPC method `get_deal` of `crm.deals.v1.Deals` (module `DealsModule`)";
        assert_eq!(
            access_problems(
                label,
                &AccessSpec::default(),
                ATTRIBUTES,
                "method",
                true,
                true
            ),
            [format!(
                "{label} has no access declaration: add #[public], #[authenticated], #[roles], #[permissions] or #[can] (deny by default)"
            )]
        );
        let confused = AccessSpec {
            public: true,
            roles: vec!["munshi".into()],
            ..AccessSpec::default()
        };
        assert_eq!(
            access_problems(label, &confused, ATTRIBUTES, "method", true, false),
            [
                format!(
                    "{label} is public but also declares public roles:munshi; remove one of them"
                ),
                format!(
                    "{label} declares public roles:munshi but no `dyn RouteAuthorizer` is registered (import the access module)"
                ),
            ]
        );
        let signed_in = AccessSpec {
            authenticated: true,
            ..AccessSpec::default()
        };
        assert_eq!(
            access_problems(label, &signed_in, ATTRIBUTES, "method", false, true),
            [format!(
                "{label} requires authentication but no `dyn Authenticator` is registered (import the auth module, or mark the method public)"
            )]
        );
        assert!(access_problems(label, &signed_in, ATTRIBUTES, "method", true, false).is_empty());

        // A wrapped server's default, with the builder's words.
        let service = "gRPC service `crm.deals.v1.Deals` in `Wrapped`";
        let builder = ".public(), .authenticated(), .roles(..), .permissions(..) or .can(..)";
        assert_eq!(
            access_problems(
                service,
                &AccessSpec::default(),
                builder,
                "service",
                true,
                true
            )[0],
            format!(
                "{service} has no access declaration: add .public(), .authenticated(), .roles(..), .permissions(..) or .can(..) (deny by default)"
            )
        );
        assert_eq!(
            access_problems(service, &signed_in, builder, "service", false, true)[0],
            format!(
                "{service} requires authentication but no `dyn Authenticator` is registered (import the auth module, or mark the service public)"
            )
        );
    }
}
