//! # navree-macros
//!
//! Attribute macros for Navree applications: `#[module]`, `#[injectable]`,
//! `#[controller]` and the route attributes, `#[flag]` (feature flags),
//! `#[resolver]` (GraphQL root fields, for navree-graphql), and
//! `#[grpc_service]` (gRPC services, for navree-grpc).
//!
//! Every attribute expands into plain Rust that calls the runtime crates; the
//! hand-written `impl Module` / `impl Injectable` is always available as the
//! escape hatch (framework crates use it and never these macros).
//!
//! ## Paths
//!
//! The generated code names the runtime crates through the `navree` facade when
//! the crate being compiled depends on it (`::navree::kernel::…`,
//! `::navree::http::…`, `::navree::access::…`, `::navree::flags::…`,
//! `::navree::graphql::…`, `::navree::grpc::…`, honouring a rename in
//! `Cargo.toml`), and otherwise through the individual crates
//! (`::navree_kernel::…`, `::navree_http::…`, `::navree_access::…`,
//! `::navree_flags::…`, `::navree_graphql::…`, `::navree_grpc::…`). Depending on
//! neither is a compile error that names the missing crate.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod attr;
mod controller;
mod duration;
mod flag;
mod grpc;
mod injectable;
mod micro;
mod module;
mod path;
mod paths;
mod resolver;
mod route;

/// Implements `navree_micro::MessageHandlers` for an impl block whose
/// methods answer message and event patterns.
///
/// ```text
/// #[message_handlers]
/// #[authenticated]                                   // impl: applies to its message patterns
/// impl DealsHandlers {
///     #[message_pattern("deals.get")]                // request-response
///     #[can(Read, Deal, from = "id")]                // the record id is the payload's `id`
///     async fn get(&self, uow: Uow, deal: Authorized<Deal, Read>) -> Result<DealView> { .. }
///
///     #[event_pattern("deal.won", max_attempts = 10)] // an event
///     async fn won(&self, uow: Uow, Payload(event): Payload<DealWon>) -> Result<()> { .. }
/// }
/// ```
///
/// Each method marked `#[message_pattern("..")]` or `#[event_pattern("..")]`
/// becomes one `PatternDef`. The method must be a non-generic `async fn`
/// taking `&self`; its other arguments are extractors (`Payload<T>` or
/// `Valid<Payload<T>>` last) and it returns `Result<T>` with `T:
/// Serialize` (the reply; `()` for events). The generated handler captures
/// the struct (`Arc<Self>`), so handlers use injected fields through
/// `self`.
///
/// | Attribute | On | Becomes |
/// |---|---|---|
/// | `#[message_pattern("deals.get")]` | fn | a message pattern; options `dedupe` and `concurrency = n` |
/// | `#[event_pattern("deal.won")]` | fn | an event pattern (may end in `*` or `>`); options `max_attempts = n`, `ordered`, `broadcast` and `concurrency = n` |
/// | `#[public]`, `#[authenticated]`, `#[roles(..)]`, `#[permissions(..)]`, `#[can(..)]` | fn, impl | the access declaration of message patterns |
/// | `#[guard(expr, ..)]` | fn, impl | `.guard(expr)`; `expr` may use `self` |
/// | `#[timeout("2s")]` | fn | the handler's own time limit |
///
/// **Deny by default:** a message pattern without an access declaration
/// (on itself or on the impl) does not compile. An access attribute on an
/// event pattern does not compile either: events are facts, and the access
/// check ran where the change was made. Invalid patterns, wildcards in
/// message patterns, two handlers for one pattern and route-only attributes
/// (`#[throttle]`, `#[get]`, ...) are compile errors too.
///
/// Register the struct with `m.message_handlers::<T>()`; it must also be
/// `Injectable` (usually `#[injectable]`). The expansion names
/// `navree::micro` (or `navree_micro`), plus `navree::http` for guards and
/// `navree::access` for `#[can]` with marker types.
#[proc_macro_attribute]
pub fn message_handlers(args: TokenStream, item: TokenStream) -> TokenStream {
    micro::expand(args.into(), item.into()).into()
}

/// Declares a message pattern on a method of a [`macro@message_handlers`]
/// impl block: `#[message_pattern("deals.get")]`. `#[message_handlers]`
/// consumes it; anywhere else it is a compile error.
#[proc_macro_attribute]
pub fn message_pattern(_args: TokenStream, item: TokenStream) -> TokenStream {
    micro::outside("message_pattern", item.into()).into()
}

/// Declares an event pattern on a method of a [`macro@message_handlers`]
/// impl block: `#[event_pattern("deal.won")]`. `#[message_handlers]`
/// consumes it; anywhere else it is a compile error.
#[proc_macro_attribute]
pub fn event_pattern(_args: TokenStream, item: TokenStream) -> TokenStream {
    micro::outside("event_pattern", item.into()).into()
}

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
/// | `#[requires_flag(NewPipeline)]`, `#[requires_flag(NewPipeline, status = 403)]` | fn | `navree_flags::requires_flag::<NewPipeline>(route, 404)`: while the flag is off the route answers 404 (or 403) before its authorization, extractors and handler; OpenAPI shows `x-navree-flag` |
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
/// dependencies, `#[requires_flag]` needs `navree` with the `flags` feature
/// (or `navree-flags`); everything else needs `navree` (or `navree-http`). Register the
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

/// Implements `navree_flags::Flag` for a unit struct: a feature flag.
///
/// ```text
/// #[flag]                                             // name: the type in kebab case
/// #[flag(name = "ai-summaries", default = true, expose)]
/// ```
///
/// | Argument | Default | Meaning |
/// |---|---|---|
/// | `name = ".."` | the type name in kebab case (`NewPipeline` → `new-pipeline`) | the stored name: lower-case letters, digits, `-`, `_` and `.` |
/// | `default = true` | `false` | the value when nothing else decides |
/// | `expose` (or `expose = true`) | off | `GET /api/flags` serves the flag to the front end |
///
/// The doc comment becomes the flag's description. The struct must be a
/// unit struct without generics; anything else, an unknown or duplicate
/// key or an invalid name is a compile error. The expansion names
/// `::navree::flags::Flag` (the facade's `flags` feature) or
/// `::navree_flags::Flag`.
///
/// ```text
/// /// The redesigned deal pipeline board.
/// #[flag]
/// pub struct NewPipeline;
/// // impl navree::flags::Flag for NewPipeline {
/// //     const NAME: &'static str = "new-pipeline";
/// //     const DEFAULT: bool = false;
/// //     const EXPOSE: bool = false;
/// //     const DESCRIPTION: &'static str = "The redesigned deal pipeline board.";
/// // }
/// ```
#[proc_macro_attribute]
pub fn flag(args: TokenStream, item: TokenStream) -> TokenStream {
    flag::expand(args.into(), item.into()).into()
}

/// Implements `navree_graphql::GraphqlResolver` for an inherent impl block
/// whose methods are GraphQL root fields: `#[query]`, `#[mutation]` or
/// `#[subscription]`.
///
/// ```text
/// #[injectable]
/// pub struct DealResolver { deals: Arc<DealService> }
///
/// #[resolver]
/// impl DealResolver {
///     #[query]
///     #[can(Read, Deal, from = "id")]
///     async fn deal(&self, ctx: &Context<'_>, id: Uuid) -> Result<DealObject> { .. }
///
///     #[mutation]
///     #[authenticated]
///     async fn move_deal(&self, ctx: &Context<'_>, id: Uuid, input: Valid<MoveDealInput>)
///         -> Result<DealObject> { .. }
///
///     #[subscription]
///     #[can(Read, Deal)]
///     async fn deal_updated(&self, ctx: &Context<'_>) -> Result<impl Stream<Item = DealObject>> { .. }
/// }
/// ```
///
/// A field method is a non-generic `async fn` taking `&self`, optionally
/// the async-graphql `&Context<'_>` next, then its GraphQL arguments as
/// plain names (`id: Uuid`, with async-graphql's `#[graphql(..)]` argument
/// attributes). It returns a value or a `Result` whose error converts into
/// `async_graphql::Error`. Doc comments become field descriptions and
/// `#[graphql(..)]` on the method (name, deprecation, complexity, ...)
/// applies to the field. Methods without a field attribute stay plain
/// methods.
///
/// **Deny by default.** Every field declares its access with `#[public]`,
/// `#[authenticated]`, `#[roles(..)]`, `#[permissions(..)]` or `#[can(..)]`,
/// on the method or on the impl block (below `#[resolver]`); otherwise it
/// does not compile. `#[can(Action, Subject, from = "id")]` names a field
/// argument holding the record id. The checks run when the field resolves,
/// before the method: guests get `UNAUTHENTICATED`, refusals `FORBIDDEN`.
///
/// An argument typed `Valid<T>` (navree-validation's) is parsed as `T` and
/// validated with `T`'s garde rules before the method runs. Mutation fields
/// run in a unit of work of their own (committed when the method returns
/// `Ok`). The expansion names the runtime through `::navree::graphql` (or
/// `::navree_graphql`), so it needs `navree` with the `graphql` feature (or
/// `navree-graphql`) among the dependencies. Register the resolver with
/// `ModuleBuilderGraphqlExt::resolver`; it must also be `Injectable`.
#[proc_macro_attribute]
pub fn resolver(args: TokenStream, item: TokenStream) -> TokenStream {
    resolver::expand(args.into(), item.into()).into()
}

/// Serves a tonic service impl through navree-grpc's call pipeline and
/// implements `navree_grpc::GrpcService` for the struct, so a module
/// registers it with `m.grpc_service::<DealsGrpc>()`.
///
/// ```text
/// #[grpc_service(server = DealsServer)]                                  // the generated server type
/// #[grpc_service(server = DealsServer, interceptor = check_version)]     // a tonic interceptor
/// ```
///
/// It goes on the impl of a generated service trait (`impl Deals for
/// DealsGrpc`). Each method is the trait's, with three additions:
///
/// - the first parameter is tonic's `Request<T>`, or `Valid<Request<T>>`
///   to run the message's garde rules first (`INVALID_ARGUMENT` with a
///   `google.rpc.BadRequest` detail when one breaks);
/// - Navree extractors may follow it (`Uow`, `CurrentPrincipal`,
///   `Authorized<E, A>`, `Accessible<E, A>`, `Scoped<T>`, ...), run on the
///   call's metadata and extensions;
/// - the result may be `Result<Response<T>>` with an `AppError` (mapped to
///   the gRPC status of the same meaning) or `Result<Response<T>, Status>`.
///
/// Access attributes work as on `#[controller]` routes and may sit on the
/// methods or on the impl block (below `#[grpc_service]`): `#[public]`,
/// `#[authenticated]`, `#[roles(..)]`, `#[permissions(..)]`, `#[can(Action,
/// Subject)]` and `#[guard(expr)]`. **Deny by default:** a method that
/// declares no access does not compile. `#[can(.., from = "id")]` reads the
/// record id from the request message's field `id` (a string or an
/// integer, optional or not), so it needs a unary request. The HTTP-only
/// attributes (`#[get]`, `#[throttle]`, `#[timeout]`, `#[audit]`,
/// `#[name]`, `#[api]`, `#[transactional]`) are errors here.
///
/// The expansion names `navree::grpc` (or `navree_grpc`) and, for
/// `#[can]` with marker types, `navree::access` (or `navree_access`).
#[proc_macro_attribute]
pub fn grpc_service(args: TokenStream, item: TokenStream) -> TokenStream {
    grpc::expand(args.into(), item.into()).into()
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
