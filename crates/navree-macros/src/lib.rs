//! # navree-macros
//!
//! Attribute macros for Navree applications: `#[module]`, `#[injectable]`,
//! `#[controller]` and the route attributes.
//!
//! Every attribute expands into plain Rust that calls the runtime crates; the
//! hand-written `impl Module` / `impl Injectable` is always available as the
//! escape hatch (framework crates use it and never these macros).
//!
//! ## Paths
//!
//! The generated code names the runtime crates through the `navree` facade when
//! the crate being compiled depends on it (`::navree::kernel::…`,
//! `::navree::http::…`, `::navree::access::…`, honouring a rename in
//! `Cargo.toml`), and otherwise through the individual crates
//! (`::navree_kernel::…`, `::navree_http::…`, `::navree_access::…`). Depending on
//! neither is a compile error that names the missing crate.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod attr;
mod controller;
mod duration;
mod injectable;
mod module;
mod path;
mod paths;
mod route;

/// Implements `Injectable` for a struct by resolving each field from the
/// container (constructor injection).
///
/// ```text
/// #[injectable]                                  // singleton (the default)
/// #[injectable(scope = "singleton" | "request" | "transient")]
/// ```
///
/// Each named field is filled according to its type:
///
/// | Field | Filled with | Dependency edge |
/// |---|---|---|
/// | `Arc<T>` (also `Arc<dyn Trait>`) | `resolver.get::<T>()` | `ProviderKey::of::<T>()` |
/// | `#[inject(name = "x")] Arc<T>` | `resolver.get_named::<T>("x")` | `ProviderKey::named::<T>("x")` |
/// | `Lazy<T>` | `resolver.lazy::<T>()`, resolved on first `get` after boot | none (it breaks cycles) |
/// | `#[inject(default)] T` | `Default::default()` | none |
///
/// `dependencies()` lists the edges in field order, so the boot-time graph
/// check sees them. Unit structs (and `struct S {}`) are allowed; generic
/// structs keep their parameters and bounds. Any other field type, a tuple
/// struct, an enum, an unknown argument or an unknown scope is a compile error
/// that points at the offending tokens.
///
/// ```
/// use std::sync::Arc;
///
/// use navree_kernel::{Injectable, Navree, Lazy, Module, ModuleBuilder, Scope};
/// use navree_macros::injectable;
///
/// #[injectable]
/// struct Repo;
///
/// #[injectable]
/// struct Mailer;
///
/// #[injectable(scope = "transient")]
/// struct UserService {
///     repo: Arc<Repo>,
///     #[inject(name = "audit")]
///     audit_repo: Arc<Repo>,
///     mailer: Lazy<Mailer>,
///     #[inject(default)]
///     greeting: String,
/// }
///
/// assert_eq!(UserService::scope(), Scope::Transient);
/// assert_eq!(UserService::dependencies().len(), 2); // `mailer` and `greeting` are not edges
/// ```
#[proc_macro_attribute]
pub fn injectable(args: TokenStream, item: TokenStream) -> TokenStream {
    injectable::expand(args.into(), item.into()).into()
}

/// Implements `Module` for a struct from a declarative description.
///
/// ```text
/// #[module(
///     imports = [DatabaseModule, CacheModule::for_feature(opts)],  // expressions
///     providers = [UserRepository, UserService],                    // #[injectable] types
///     controllers = [UserController],                               // needs navree-http
///     exports = [UserService],                                      // types (dyn Trait too)
///     global,                                                       // exports visible everywhere
///     register = Self::extra,                                       // fn(&Self, &mut ModuleBuilder)
/// )]
/// ```
///
/// Every key is optional; order inside the attribute does not matter. The
/// generated `register` calls, in this order: `m.import(expr)` per import,
/// `m.provider::<P>()` per provider,
/// `navree_http::ModuleBuilderHttpExt::controller::<C>(m)` per controller,
/// `m.exports::<E>()` per export, `m.global()`, and finally the `register`
/// function or closure, for everything the attribute does not declare
/// (configuration sections, values, factories, named providers, migrations,
/// jobs, commands, lifecycle hooks). Unknown or duplicate keys are compile
/// errors pointing at the key.
///
/// ```
/// use std::sync::Arc;
///
/// use navree_kernel::{Navree, ModuleBuilder};
/// use navree_macros::{injectable, module};
///
/// #[injectable]
/// struct Repo;
///
/// #[module(providers = [Repo], exports = [Repo])]
/// struct DataModule;
///
/// struct Greeting(&'static str);
///
/// #[injectable]
/// struct UserService {
///     repo: Arc<Repo>,
///     greeting: Arc<Greeting>,
/// }
///
/// #[module(
///     imports = [DataModule],
///     providers = [UserService],
///     exports = [UserService],
///     register = Self::extra,
/// )]
/// struct UsersModule {
///     greeting: &'static str,
/// }
///
/// impl UsersModule {
///     fn extra(&self, m: &mut ModuleBuilder) {
///         m.value(Greeting(self.greeting));
///     }
/// }
///
/// let report = Navree::app()
///     .module(UsersModule { greeting: "hello" })
///     .validate()
///     .expect("the module graph is valid");
/// let users = report.modules.iter().find(|m| m.name == "UsersModule").unwrap();
/// assert_eq!(users.imports, ["DataModule"]);
/// assert_eq!(users.providers.len(), 2); // UserService and the Greeting value
/// ```
#[proc_macro_attribute]
pub fn module(args: TokenStream, item: TokenStream) -> TokenStream {
    module::expand(args.into(), item.into()).into()
}

/// Implements `navree_http::Controller` for an inherent impl block whose
/// methods carry route attributes.
///
/// ```text
/// #[controller("/deals")]                      // routes under {api_prefix}/deals
/// #[controller("/deals", version = "1")]       // under {api_prefix}/v1/deals
/// #[controller("!/sanctum")]                   // absolute: no API prefix, no version
/// ```
///
/// Each method with `#[get("/p")]`, `#[post]`, `#[put]`, `#[patch]` or
/// `#[delete]` becomes one `RouteDef` (a missing path means `"/"`; a path
/// starting with `!` is absolute). Paths use Axum 0.8 captures (`{id}`,
/// `{*rest}`). The method must be a non-generic `async fn` taking `&self`;
/// its other arguments are Axum extractors (the body extractor last) and its
/// return type implements `IntoResponse`. The generated handler captures the
/// controller (`Arc<Self>`), so handlers use injected fields through `self`.
/// Methods without a route attribute stay plain methods.
///
/// The helper attributes below sit on route methods; those marked "impl"
/// may also sit on the impl block, **below** `#[controller]`, and then apply
/// to every route. None of them needs an import: `#[controller]` consumes
/// them.
///
/// | Attribute | On | Becomes |
/// |---|---|---|
/// | `#[public]` | fn, impl | `.public()`: no authentication or authorization |
/// | `#[authenticated]` | fn, impl | `.authenticated()`: any signed-in user |
/// | `#[roles("admin", "manager")]` | fn, impl | `.roles(..)`: any of the roles |
/// | `#[permissions("deals.update")]` | fn, impl | `.permissions(..)` |
/// | `#[can(Update, Deal)]`, `#[can(Update, Deal, from = "id")]` | fn, impl | `.can(<Update as ActionMarker>::NAME, <Deal as Subject>::SUBJECT, from)`; a string (`"publish"`) is used as is |
/// | `#[guard(expr, ..)]` | fn, impl | `.guard(expr)`; `expr` is an `Arc<impl Guard>` or `Arc<dyn Guard>` and may use `self` |
/// | `#[throttle(limit = 10, per = "1m")]` | fn, impl | `.throttle(10, 60s)`; a route's own replaces the impl's |
/// | `#[timeout("5s")]` | fn | `.timeout(5s)` |
/// | `#[transactional]` | fn | `.transactional()` |
/// | `#[audit(action = "deal.won")]` | fn | `.audit("deal.won")` |
/// | `#[name("deals.show")]` | fn | `.name("deals.show")` |
/// | `#[api(summary = "..", description = "..", tag = "Deals", tags("A", "B"), request = T, response = U, responses(400, 404), status = 201)]` | fn | `.api(ApiMeta { .. })`, schemas via `navree_http::schema_fn::<T>` |
///
/// Durations are a whole number followed by `ms`, `s`, `m` or `h` (`"500ms"`,
/// `"5s"`, `"1m"`, `"1h"`). Access declarations on the impl add to a route's
/// own; impl guards run before the route's guards.
///
/// **Deny by default.** A route that declares none of `#[public]`,
/// `#[authenticated]`, `#[roles]`, `#[permissions]` or `#[can]` (on itself
/// or on the impl) does not compile; a `#[guard]` alone does not count.
/// Other mistakes navree-http would only catch at boot are compile errors too:
/// `#[public]` combined with another declaration, two `#[can]` on one route,
/// `from = "id"` naming no path parameter, old `:id` / `*rest` path syntax,
/// and two routes with the same method and path.
///
/// `#[can]` with marker types needs `navree` (or `navree-access`) among the
/// dependencies; everything else needs `navree` (or `navree-http`). Register the
/// controller with `#[module(controllers = [DealController])]` or
/// `ModuleBuilderHttpExt::controller`; it must also be `Injectable` (usually
/// `#[injectable]`).
///
/// ```
/// use std::sync::Arc;
///
/// use axum::extract::Path;
/// use navree_access::{Subject, Update};
/// use navree_http::{Controller, Json};
/// use navree_macros::{controller, injectable};
///
/// struct Deal;
///
/// impl Subject for Deal {
///     const SUBJECT: &'static str = "Deal";
///     const PERMISSION_PREFIX: &'static str = "deals";
/// }
///
/// struct Pipeline(&'static str);
///
/// #[injectable]
/// struct DealController {
///     pipeline: Arc<Pipeline>,
/// }
///
/// #[controller("/deals", version = "1")]
/// #[authenticated]
/// impl DealController {
///     #[get("/")]
///     #[name("deals.index")]
///     async fn index(&self) -> Json<&'static str> {
///         Json(self.pipeline.0)
///     }
///
///     #[patch("/{id}")]
///     #[can(Update, Deal, from = "id")]
///     #[transactional]
///     #[timeout("5s")]
///     async fn update(&self, Path(id): Path<u64>) -> String {
///         format!("updated {id}")
///     }
/// }
///
/// let controller = Arc::new(DealController {
///     pipeline: Arc::new(Pipeline("sales")),
/// });
/// assert_eq!((controller.prefix(), controller.version()), ("/deals", Some("1")));
/// let routes = controller.routes();
/// assert_eq!(routes[0].meta.name.as_deref(), Some("deals.index"));
/// assert_eq!(routes[0].meta.access.describe(), "auth");
/// assert_eq!(
///     routes[1].meta.access.describe(),
///     "auth can:update Deal from id"
/// );
/// assert!(routes[1].meta.transactional);
/// ```
#[proc_macro_attribute]
pub fn controller(args: TokenStream, item: TokenStream) -> TokenStream {
    controller::expand(args.into(), item.into()).into()
}

/// Declares a `GET` route on a method of a [`macro@controller`] impl block:
/// `#[get("/{id}")]`. `#[controller]` consumes it; anywhere else it is a
/// compile error.
#[proc_macro_attribute]
pub fn get(_args: TokenStream, item: TokenStream) -> TokenStream {
    controller::outside_controller("get", item.into()).into()
}

/// Declares a `POST` route on a method of a [`macro@controller`] impl block:
/// `#[post("/")]`. `#[controller]` consumes it; anywhere else it is a compile
/// error.
#[proc_macro_attribute]
pub fn post(_args: TokenStream, item: TokenStream) -> TokenStream {
    controller::outside_controller("post", item.into()).into()
}

/// Declares a `PUT` route on a method of a [`macro@controller`] impl block:
/// `#[put("/{id}")]`. `#[controller]` consumes it; anywhere else it is a
/// compile error.
#[proc_macro_attribute]
pub fn put(_args: TokenStream, item: TokenStream) -> TokenStream {
    controller::outside_controller("put", item.into()).into()
}

/// Declares a `PATCH` route on a method of a [`macro@controller`] impl block:
/// `#[patch("/{id}")]`. `#[controller]` consumes it; anywhere else it is a
/// compile error.
#[proc_macro_attribute]
pub fn patch(_args: TokenStream, item: TokenStream) -> TokenStream {
    controller::outside_controller("patch", item.into()).into()
}

/// Declares a `DELETE` route on a method of a [`macro@controller`] impl
/// block: `#[delete("/{id}")]`. `#[controller]` consumes it; anywhere else it
/// is a compile error.
#[proc_macro_attribute]
pub fn delete(_args: TokenStream, item: TokenStream) -> TokenStream {
    controller::outside_controller("delete", item.into()).into()
}
