//! [`Mount`]: any tower service under a path prefix, inside the app's
//! pipeline, and [`ModuleBuilderMountExt`], how a module declares one.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::Method;
use navree_kernel::{AppContext, BoxError, ModuleBuilder};
use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceExt};

use crate::body::{Body, HttpBody};
use crate::extract::Request;
use crate::response::Response;
use crate::router::access_problems;
use crate::routing::{PathPattern, Segment};
use crate::{CanSpec, Guard, RouteMeta, Throttle};

/// A mounted service as the app holds it: the app's request and response,
/// any error.
pub(crate) type MountService = BoxCloneSyncService<Request, Response, BoxError>;

/// What a prefix must look like: the text of every refused prefix.
const PREFIX_RULE: &str = "a prefix starts with `/` but not with `/_`, has no `{`, `}`, `*`, `..` or `//`, and does not end with `/`";

/// Any tower service under a path prefix, inside the app's pipeline.
///
/// A module declares it with
/// [`m.mount(..)`](ModuleBuilderMountExt::mount). It answers the prefix
/// itself and every path below it, for every method unless
/// [`methods`](Mount::methods) names some, behind the pipeline a route has:
/// request id, request context and client address, the app's global layers
/// (session and CSRF among them), authentication, then on every request the
/// mount's access check, guards, throttle, timeout and body limit. One of
/// [`public`](Mount::public), [`authenticated`](Mount::authenticated),
/// [`roles`](Mount::roles), [`permissions`](Mount::permissions) or
/// [`can`](Mount::can) is required, as for a route: a mount without an access
/// declaration fails the boot, and so does one whose prefix is malformed,
/// starts with `/_` (the framework's own paths), or overlaps a route or
/// another mount.
///
/// The service is built in `register`; it reaches the app's providers through
/// the `AppContext` in each request's extensions.
///
/// ```
/// use std::time::Duration;
///
/// use navree_http::extract::Request;
/// use navree_http::{AppRouter, ModuleBuilderMountExt, Mount};
/// use navree_kernel::{Module, ModuleBuilder};
///
/// struct LedgerModule;
///
/// impl Module for LedgerModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // The service of another crate; here, one that answers every path.
///         let khata = AppRouter::from_handler(|request: Request| async move {
///             format!("the khata at {}", request.uri().path())
///         });
///         m.mount(
///             Mount::new("/khata", khata)
///                 .authenticated()
///                 .throttle(60, Duration::from_secs(60)),
///         );
///     }
/// }
/// ```
#[derive(Clone)]
pub struct Mount {
    /// Where the service answers: absolute, not under `http.api_prefix`.
    prefix: String,
    /// The service, its body taken as the app's and its error boxed.
    service: MountService,
    /// The access declaration and the rest of a route's metadata.
    meta: RouteMeta,
    /// The methods it answers; `None` for every method.
    methods: Option<Vec<Method>>,
}

impl Mount {
    /// `service` at `prefix` (`/khata`, absolute: not under
    /// `http.api_prefix`): it sees requests for `prefix` and `prefix/...` with
    /// the prefix taken off the path, raw (`/khata/v1/items%3Fx` arrives as
    /// `/v1/items%3Fx`). Its access is declared next.
    pub fn new<S, B>(prefix: &str, service: S) -> Mount
    where
        S: Service<Request, Response = http::Response<B>> + Clone + Send + Sync + 'static,
        S::Error: Into<BoxError>,
        S::Future: Send + 'static,
        B: HttpBody<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let service = service
            .map_response(|response: http::Response<B>| response.map(Body::new))
            .map_err(|error: S::Error| -> BoxError { error.into() });
        Mount {
            prefix: prefix.to_owned(),
            service: BoxCloneSyncService::new(service),
            meta: RouteMeta::default(),
            methods: None,
        }
    }

    /// No authentication or authorization: every request reaches the service.
    pub fn public(mut self) -> Mount {
        self.meta.access.public = true;
        self
    }

    /// Any signed-in user may call the mount (no further authorization).
    pub fn authenticated(mut self) -> Mount {
        self.meta.access.authenticated = true;
        self
    }

    /// Requires any of the roles (checked by the
    /// [`RouteAuthorizer`](crate::RouteAuthorizer) on every request).
    pub fn roles(mut self, roles: &[&str]) -> Mount {
        self.meta
            .access
            .roles
            .extend(roles.iter().map(|role| (*role).to_owned()));
        self
    }

    /// Requires the permissions (checked by the
    /// [`RouteAuthorizer`](crate::RouteAuthorizer) on every request).
    pub fn permissions(mut self, permissions: &[&str]) -> Mount {
        self.meta
            .access
            .permissions
            .extend(permissions.iter().map(|permission| (*permission).to_owned()));
        self
    }

    /// Requires the ability `action` on the type `subject` (checked by the
    /// [`RouteAuthorizer`](crate::RouteAuthorizer) on every request). A mount
    /// has no path parameter, so no record is named.
    pub fn can(mut self, action: &str, subject: &str) -> Mount {
        self.meta.access.can = Some(CanSpec {
            action: action.to_owned(),
            subject: subject.to_owned(),
            from: None,
        });
        self
    }

    /// Adds a guard, run on every request before authorization and the
    /// service.
    pub fn guard(mut self, guard: Arc<dyn Guard>) -> Mount {
        self.meta.guards.push(guard);
        self
    }

    /// At most `limit` requests per `per`, per user (per client IP for
    /// guests), counted for the whole mount: every path and method under the
    /// prefix takes from one budget.
    pub fn throttle(mut self, limit: u32, per: Duration) -> Mount {
        self.meta.throttle = Some(Throttle { limit, per });
        self
    }

    /// Overrides the app-wide request timeout (shorter or longer).
    pub fn timeout(mut self, d: Duration) -> Mount {
        self.meta.timeout = Some(d);
        self
    }

    /// Overrides the app-wide request body limit: a request that declares a
    /// longer body is refused before the service is called, and a body sent
    /// without a length ends in an error once it passes the limit.
    pub fn body_limit(mut self, bytes: usize) -> Mount {
        self.meta.body_limit = Some(bytes);
        self
    }

    /// The mount's name, for logs and the `routes` command.
    pub fn name(mut self, name: &str) -> Mount {
        self.meta.name = Some(name.to_owned());
        self
    }

    /// Only these methods (default: every method, extension methods such as
    /// `PROPFIND` included; an empty list fails the boot). A method named
    /// twice is kept once. With a list, `HEAD` runs the `GET` answer without
    /// its body and `OPTIONS` lists the methods, as for a route.
    pub fn methods(mut self, methods: &[Method]) -> Mount {
        let mut kept: Vec<Method> = Vec::with_capacity(methods.len());
        for method in methods {
            if !kept.contains(method) {
                kept.push(method.clone());
            }
        }
        self.methods = Some(kept);
        self
    }
}

impl fmt::Debug for Mount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mount")
            .field("prefix", &self.prefix)
            .field("methods", &self.methods)
            .field("access", &self.meta.access.describe())
            .finish_non_exhaustive()
    }
}

/// Mounting tower services on a [`ModuleBuilder`].
///
/// ```
/// use std::convert::Infallible;
///
/// use http::Method;
/// use navree_http::body::Body;
/// use navree_http::extract::Request;
/// use navree_http::{ModuleBuilderMountExt, Mount};
/// use navree_kernel::{Module, ModuleBuilder};
///
/// struct BalanceModule;
///
/// impl Module for BalanceModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // A service of another crate, written with tower alone.
///         let balance = tower::service_fn(|_request: Request| async {
///             Ok::<_, Infallible>(http::Response::new(Body::from("₹1,00,000")))
///         });
///         m.mount(
///             Mount::new("/balance", balance)
///                 .public()
///                 .methods(&[Method::GET]),
///         );
///     }
/// }
/// ```
pub trait ModuleBuilderMountExt {
    /// Serves `mount`'s service under its prefix. The boot checks the mount
    /// with the app's routes and other mounts (see [`Mount`]).
    fn mount(&mut self, mount: Mount) -> &mut Self;
}

impl ModuleBuilderMountExt for ModuleBuilder {
    fn mount(&mut self, mount: Mount) -> &mut Self {
        self.contribute(MountRegistration(mount))
    }
}

/// A mount declared by a module (a contribution).
#[derive(Clone)]
pub(crate) struct MountRegistration(pub(crate) Mount);

/// A mount ready to check and serve: its metadata names it as one route,
/// `{prefix}/{*rest}`, of the module that declared it.
pub(crate) struct MountEntry {
    pub(crate) prefix: String,
    pub(crate) service: MountService,
    pub(crate) meta: RouteMeta,
    pub(crate) methods: Option<Vec<Method>>,
    pub(crate) module: &'static str,
}

/// Every mount of the app, modules in dependency order, each module's in the
/// order declared.
pub(crate) fn collect_mounts(ctx: &AppContext) -> Vec<MountEntry> {
    ctx.contributions::<MountRegistration>()
        .into_iter()
        .map(|contributed| {
            let mount = contributed.value.0.clone();
            let mut meta = mount.meta;
            meta.full_path = format!("{}/{{*rest}}", mount.prefix);
            meta.method = match &mount.methods {
                None => "*".to_owned(),
                Some(methods) => methods
                    .iter()
                    .map(Method::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            };
            meta.module = Some(contributed.module.name);
            meta.module_id = Some(contributed.module.id);
            MountEntry {
                prefix: mount.prefix,
                service: mount.service,
                meta,
                methods: mount.methods,
                module: contributed.module.name,
            }
        })
        .collect()
}

/// The segments of a good prefix (`/khata/reports` is `["khata",
/// "reports"]`). `Err(None)` for a prefix outside the grammar
/// (`PREFIX_RULE`); `Err(Some(..))` with the router's own text for one
/// inside it whose segment a route path may not hold (`%`, a space, a `.`
/// segment, a segment starting with `:`).
pub(crate) fn check_prefix(prefix: &str) -> Result<Vec<&str>, Option<String>> {
    let in_grammar = prefix.starts_with('/')
        && !prefix.starts_with("/_")
        && !prefix.contains(['{', '}', '*'])
        && !prefix.contains("..")
        && !prefix.contains("//")
        && !prefix.ends_with('/');
    if !in_grammar {
        return Err(None);
    }
    PathPattern::parse(prefix).map_err(|error| Some(error.to_string()))?;
    // Parsed without a parameter: every segment is static, as written.
    Ok(prefix.split('/').skip(1).collect())
}

/// Whether `route` matches the path `prefix` names or a path below it: a
/// `{param}` matches any one segment, a `{*rest}` any path.
pub(crate) fn overlaps(route: &PathPattern, prefix: &[&str]) -> bool {
    for (index, segment) in route.segments().iter().enumerate() {
        if index == prefix.len() {
            return true;
        }
        match segment {
            Segment::CatchAll => return true,
            Segment::Param => {}
            Segment::Static(text) => {
                if &**text != prefix[index] {
                    return false;
                }
            }
        }
    }
    route.segments().len() == prefix.len()
}

/// The problems of every mount, in order: a bad prefix (and then nothing
/// else for that mount), an empty method list, the access rule routes are
/// checked by, each route that overlaps it, and each earlier mount at or
/// under its prefix, or above it.
pub(crate) fn check_mounts(
    routes: &[&RouteMeta],
    mounts: &[MountEntry],
    authenticator: bool,
    authorizer: bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    // The earlier mounts with a good prefix: (prefix, segments, module).
    let mut checked: Vec<(&str, Vec<&str>, &str)> = Vec::new();
    for mount in mounts {
        let label = format!("mount `{}` in `{}`", mount.prefix, mount.module);
        let segments = match check_prefix(&mount.prefix) {
            Ok(segments) => segments,
            Err(detail) => {
                let detail = detail
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default();
                errors.push(format!("{label}: {}{detail}", PREFIX_RULE));
                continue;
            }
        };
        if mount.methods.as_ref().is_some_and(Vec::is_empty) {
            errors.push(format!(
                "{label} takes no method: leave out .methods(..) for every method, or name at least one"
            ));
        }
        errors.extend(access_problems(&label, "mount", &mount.meta, authenticator, authorizer));
        for route in routes {
            // A route path that does not parse is reported with the routes.
            let Ok(pattern) = PathPattern::parse(&route.full_path) else {
                continue;
            };
            if overlaps(&pattern, &segments) {
                errors.push(format!(
                    "{label} overlaps the route {} {} of `{}`; give the mount a prefix of its own",
                    route.method,
                    route.full_path,
                    route.module.unwrap_or("?")
                ));
            }
        }
        for (earlier, earlier_segments, earlier_module) in &checked {
            let shared = earlier_segments.len().min(segments.len());
            if earlier_segments[..shared] == segments[..shared] {
                errors.push(format!(
                    "mounts `{earlier}` ({earlier_module}) and `{}` ({}) overlap",
                    mount.prefix, mount.module
                ));
            }
        }
        checked.push((mount.prefix.as_str(), segments, mount.module));
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_overlaps_a_mount_when_it_matches_the_prefix_or_a_path_under_it() {
        let prefix = ["admin"];
        for (route, expected) in [
            ("/admin", true),
            ("/admin/reports", true),
            ("/admin/{*rest}", true),
            ("/{workspace}", true),
            ("/{workspace}/dashboard", true),
            ("/{*path}", true),
            ("/", false),
            ("/adminx", false),
            ("/api/admin", false),
            ("/files/{*path}", false),
        ] {
            assert_eq!(
                overlaps(&PathPattern::parse(route).unwrap(), &prefix),
                expected,
                "{route}"
            );
        }
        let deeper = ["admin", "reports"];
        assert!(
            !overlaps(&PathPattern::parse("/admin").unwrap(), &deeper),
            "a route above the prefix"
        );
        assert!(!overlaps(
            &PathPattern::parse("/admin/{id}").unwrap(),
            &["admin", "reports", "daily"]
        ));
        assert!(overlaps(
            &PathPattern::parse("/admin/{id}/daily").unwrap(),
            &["admin", "reports", "daily"]
        ));
    }
}
