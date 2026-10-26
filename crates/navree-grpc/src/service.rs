//! [`GrpcService`], [`GrpcMethod`] and [`ModuleBuilderGrpcExt`]: how
//! modules register tonic services and their descriptor sets.

use std::fmt;
use std::sync::Arc;

use navree_http::{Guard, RouteMeta};
use navree_kernel::{BoxFuture, Injectable, KernelError, ModuleBuilder, ModuleId, Resolver};
use tonic::service::Routes;

/// A tonic service served by [`GrpcModule`](crate::GrpcModule): what
/// `#[grpc_service(server = DealsServer)]` implements for the struct behind
/// the impl block. Register it with
/// [`ModuleBuilderGrpcExt::grpc_service`]; it is a singleton of its module,
/// built with dependency injection.
pub trait GrpcService: Send + Sync + 'static {
    /// The full service name, e.g. `crm.deals.v1.Deals`.
    const NAME: &'static str;

    /// Every method with its access declaration and guards (deny by
    /// default: each must declare its access).
    fn methods(self: &Arc<Self>) -> Vec<GrpcMethod>;

    /// Adds the service's tonic server (with its interceptor, if any) to
    /// `routes`, with the message size `limits`.
    fn add_to(self: Arc<Self>, routes: Routes, limits: &ServiceLimits) -> Routes;
}

/// The limits applied to every registered service's tonic server.
#[derive(Clone, Copy, Debug)]
pub struct ServiceLimits {
    /// `grpc.max_message_bytes`: the largest message received or sent.
    pub max_message_bytes: usize,
}

/// One method of a [`GrpcService`]: its Rust name (`get_deal`) and the
/// [`RouteMeta`] the call pipeline checks (access declaration and guards).
/// The builder methods mirror `navree_http::RouteDef`'s.
#[derive(Clone)]
pub struct GrpcMethod {
    /// The method's Rust name, as the generated trait spells it.
    pub name: &'static str,
    /// Who may call it, and its guards. `method` is `GRPC`; the full path
    /// (`/crm.deals.v1.Deals/GetDeal`) is set on the first call.
    pub meta: RouteMeta,
}

impl GrpcMethod {
    /// A method with no access declaration yet.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            meta: RouteMeta {
                name: Some(name.to_owned()),
                method: "GRPC".to_owned(),
                ..RouteMeta::default()
            },
        }
    }

    /// No authentication or authorization needed.
    pub fn public(mut self) -> Self {
        self.meta.access.public = true;
        self
    }

    /// Any signed-in caller may call the method.
    pub fn authenticated(mut self) -> Self {
        self.meta.access.authenticated = true;
        self
    }

    /// Requires any of the roles.
    pub fn roles(mut self, roles: &[&str]) -> Self {
        self.meta
            .access
            .roles
            .extend(roles.iter().map(|role| (*role).to_owned()));
        self
    }

    /// Requires the permissions.
    pub fn permissions(mut self, permissions: &[&str]) -> Self {
        self.meta
            .access
            .permissions
            .extend(permissions.iter().map(|p| (*p).to_owned()));
        self
    }

    /// Requires the ability `action` on `subject`; with `from`, on the
    /// record whose id is the request message's field `from`.
    pub fn can(mut self, action: &str, subject: &str, from: Option<&str>) -> Self {
        self.meta.access.can = Some(navree_http::CanSpec {
            action: action.to_owned(),
            subject: subject.to_owned(),
            from: from.map(str::to_owned),
        });
        self
    }

    /// Adds a guard, run before authorization and the method.
    pub fn guard(mut self, guard: Arc<dyn Guard>) -> Self {
        self.meta.guards.push(guard);
        self
    }
}

impl fmt::Debug for GrpcMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrpcMethod")
            .field("name", &self.name)
            .field("access", &self.meta.access.describe())
            .field("guards", &self.meta.guards.len())
            .finish()
    }
}

/// Adds a resolved service's server to the routes.
pub(crate) type AddService = Box<dyn FnOnce(Routes, &ServiceLimits) -> Routes + Send>;

/// A resolved service: its name, methods and how to add its server.
pub(crate) struct ResolvedService {
    pub(crate) name: &'static str,
    pub(crate) type_name: &'static str,
    pub(crate) methods: Vec<GrpcMethod>,
    pub(crate) add: AddService,
}

type ResolveService =
    Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<ResolvedService, KernelError>> + Send + Sync>;

/// A service registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct ServiceRegistration {
    pub(crate) module: ModuleId,
    pub(crate) resolve: ResolveService,
}

impl ServiceRegistration {
    fn of<S: GrpcService + Injectable>(module: ModuleId) -> Self {
        Self {
            module,
            resolve: Arc::new(|r: Resolver| {
                Box::pin(async move {
                    let service = r.get::<S>().await?;
                    let methods = service.methods();
                    Ok(ResolvedService {
                        name: S::NAME,
                        type_name: std::any::type_name::<S>(),
                        methods,
                        add: Box::new(move |routes, limits| service.add_to(routes, limits)),
                    })
                })
            }),
        }
    }
}

/// An encoded `FileDescriptorSet` registered with
/// [`ModuleBuilderGrpcExt::grpc_descriptors`] (a contribution).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DescriptorSet(pub(crate) &'static [u8]);

/// gRPC registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderGrpcExt {
    /// Registers service `S` (a `#[grpc_service]` impl on an injectable
    /// struct) as a singleton of this module and serves it.
    fn grpc_service<S: GrpcService + Injectable>(&mut self) -> &mut Self;

    /// Registers an encoded `FileDescriptorSet` (what
    /// `tonic::include_file_descriptor_set!` gives): server reflection
    /// describes it, and Connect's JSON encoding reads the message types
    /// from it.
    fn grpc_descriptors(&mut self, descriptors: &'static [u8]) -> &mut Self;
}

impl ModuleBuilderGrpcExt for ModuleBuilder {
    fn grpc_service<S: GrpcService + Injectable>(&mut self) -> &mut Self {
        let registration = ServiceRegistration::of::<S>(self.module_id());
        self.provider::<S>().contribute(registration)
    }

    fn grpc_descriptors(&mut self, descriptors: &'static [u8]) -> &mut Self {
        self.contribute(DescriptorSet(descriptors))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Always;

    impl Guard for Always {
        fn check<'a>(
            &'a self,
            _parts: &'a mut http::request::Parts,
        ) -> BoxFuture<'a, Result<(), navree_http::AppError>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn builders_fill_the_route_meta() {
        let method = GrpcMethod::new("move_deal")
            .authenticated()
            .roles(&["admin"])
            .permissions(&["deals.update"])
            .can("update", "Deal", Some("id"))
            .guard(Arc::new(Always));
        assert_eq!(method.meta.method, "GRPC");
        assert_eq!(method.meta.name.as_deref(), Some("move_deal"));
        assert_eq!(
            method.meta.access.describe(),
            "auth roles:admin permissions:deals.update can:update Deal from id"
        );
        assert_eq!(method.meta.guards.len(), 1);
        assert!(GrpcMethod::new("x").public().meta.access.public);
        assert!(format!("{method:?}").contains("move_deal"));
    }
}
