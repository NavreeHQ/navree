//! # navree-macros
//!
//! Attribute macros for Navree applications: `#[module]`, `#[injectable]`,
//! `#[controller]` and the route attributes, `#[flag]` (feature flags),
//! `#[notification]` (a notification's category, digest and urgency, for
//! navree-notify), `#[resolver]` (GraphQL root fields, for navree-graphql),
//! `#[grpc_service]` (gRPC services, for navree-grpc), the
//! `ToSchema` derive of navree-openapi, the
//! `Searchable` derive of navree-search, the `Settings` derive of
//! navree-settings, the `Exportable` and `Importable` derives of
//! navree-sheets, `#[agent]` (navree-ai's agents and their tools) and
//! `#[workflow]`, `#[trigger]`, `#[condition]` and `#[action]`
//! (navree-workflows' durable workflows and automation blocks).
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

mod agent;
mod attr;
mod controller;
mod duration;
mod flag;
mod grpc;
mod injectable;
mod mcp;
mod micro;
mod module;
mod notification;
mod path;
mod paths;
mod resolver;
mod route;
mod searchable;
mod settings;
mod sheets;
mod to_schema;
mod workflows;

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
/// starting with `!` is absolute). Paths use Navree's route patterns (`{id}`,
/// `{*rest}`). The method must be a non-generic `async fn` taking `&self`;
/// its other arguments are extractors (the body extractor last) and its
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
/// | `#[no_compression]` | fn | `.no_compression()`: responses go out uncompressed |
/// | `#[audit(action = "deal.won")]` | fn | `.audit("deal.won")` |
/// | `#[name("deals.show")]` | fn | `.name("deals.show")` |
/// | `#[api(summary = "..", description = "..", tag = "Deals", tags("A", "B"), request = T, response = U, responses(400, 404), status = 201)]` | fn | `.api(ApiMeta { .. })`, schemas via `navree_http::schema_fn::<T>` |
/// | `#[requires_flag(NewPipeline)]`, `#[requires_flag(NewPipeline, status = 403)]` | fn | `navree_flags::requires_flag::<NewPipeline>(route, 404)`: while the flag is off the route answers 404 (or 403) before its authorization, extractors and handler; OpenAPI shows `x-navree-flag` |
/// | `#[not_while_impersonating]` | fn, impl | `.guard(navree_auth::not_while_impersonating())`: 403 `impersonation_forbidden` in an impersonation session |
/// | `#[recent_auth(minutes = 10)]` | fn, impl | `.guard(navree_auth::recent_auth(10 min))`: 403 `reauthentication_required` unless the user signed in or confirmed (`POST /auth/confirm`) within the last 10 minutes |
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
/// (or `navree-flags`), `#[not_while_impersonating]` and `#[recent_auth]`
/// need `navree` with the `auth` feature (or `navree-auth`); everything
/// else needs `navree` (or `navree-http`). Register the
/// controller with `#[module(controllers = [DealController])]` or
/// `ModuleBuilderHttpExt::controller`; it must also be `Injectable` (usually
/// `#[injectable]`).
///
/// ```
/// use std::sync::Arc;
///
/// use navree_http::Path;
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

/// Sets the category, digest and urgency of a
/// `navree_notify::Notification` implementation (re-exported as
/// `navree::notify::notification`): it goes on the `impl Notification for
/// ..` block and adds the constants its arguments name.
///
/// ```text
/// #[notification(category = "deals", digest = "daily")]
/// impl Notification for DealWon {
///     const TYPE: &'static str = "deal_won";
///     fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> { vec![Channel::Database, Channel::Mail] }
/// }
/// // const CATEGORY: &'static str = "deals";
/// // const DIGEST: navree::notify::Digest = navree::notify::Digest::Daily;
/// ```
///
/// | Argument | Constant | Meaning |
/// |---|---|---|
/// | `type = ".."` | `TYPE` | the stored type name, when the impl does not set it |
/// | `category = ".."` | `CATEGORY` | the category preferences switch channels for (default: the type) |
/// | `digest = "daily"` | `DIGEST` | `off`, `hourly`, `daily` or `weekly`: its mail is batched into one digest mail |
/// | `urgent` | `URGENT` | delivered at once, whatever the quiet hours and digest |
///
/// Anything but an `impl Notification for ..` block, an unknown or
/// duplicate key, an unknown digest, an invalid category, a constant also
/// set in the impl, or `urgent` with a digest is a compile error. The
/// expansion names `::navree::notify` (or `::navree_notify`).
#[proc_macro_attribute]
pub fn notification(args: TokenStream, item: TokenStream) -> TokenStream {
    notification::expand(args.into(), item.into()).into()
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
/// validated with `T`'s validation rules before the method runs. Mutation fields
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
///   to run the message's validation rules first (`INVALID_ARGUMENT` with a
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

/// Implements `navree_search::Searchable` for the navree-orm model it is
/// derived on, next to `#[derive(Model)]` (re-exported as
/// `navree::search::Searchable`).
///
/// ```text
/// #[derive(Model, Searchable, Clone, Debug, Default, Serialize)]
/// #[model(table = "contacts", soft_deletes)]
/// #[searchable(name = "Contact", soft_delete = "deleted_at")]
/// pub struct Contact {
///     pub id: Uuid,
///     #[searchable(boost = 3)]
///     pub last_name: String,
///     #[model(cast = "string_enum")]
///     #[searchable(filter)]
///     pub status: ContactStatus,
///     #[searchable(sort)]
///     pub created_at: DateTime<Utc>,
///     pub deleted_at: Option<DateTime<Utc>>,
/// }
/// ```
///
/// | Field attribute | Effect |
/// |---|---|
/// | `#[searchable]` | full-text field of weight 1 |
/// | `#[searchable(boost = 3)]` | full-text field of weight 3 (1 to 10) |
/// | `#[searchable(filter)]` | exact-match field for filters |
/// | `#[searchable(sort)]` | field to sort on |
/// | `#[searchable(embed)]` | text for the record's embedding |
/// | `#[searchable(rename = "x")]` | the field's name in the index |
///
/// Model attributes: `name` and `index` (both default to the table name),
/// `soft_delete` (a nullable column; rows where it is set are removed from
/// the index) and `workspace` (default: the model's
/// `navree::workspace::WorkspaceScoped` column when it implements that
/// trait, else its `#[model(workspace)]` column, if any). The document
/// holds the primary key and the marked fields: text and embedded fields
/// serialized with serde, filter and sort fields as the database holds them
/// (the form access rules compare). A field's column is its name or its
/// `#[model(column = "..")]`; a `#[model(skip)]` or `#[model(columns(..))]`
/// field can be a text field but not a filter or sort field. The generated
/// code names `::navree::search` (or `::navree_search`).
#[proc_macro_derive(Searchable, attributes(searchable))]
pub fn derive_searchable(input: TokenStream) -> TokenStream {
    searchable::expand(input.into()).into()
}

/// Implements `navree_settings::SettingsGroup` for a struct whose fields
/// are settings: typed settings stored per scope and inherited down the
/// chain global, organization, workspace, user.
///
/// ```text
/// /// What the member portal shows.
/// #[derive(Settings, Serialize, Deserialize, Validate, Default)]
/// #[settings(key = "portal", scopes(global, organization, workspace))]
/// pub struct PortalSettings {
///     #[validate(skip)]
///     pub show_leaderboard: bool,
///     #[validate(length(max = 500))]
///     pub welcome_text: String,
/// }
/// ```
///
/// | Argument | Default | Meaning |
/// |---|---|---|
/// | `key = ".."` | the type name without `Settings`, in kebab case (`portal`) | The stored key and the routes' path segment |
/// | `scopes(..)` | `global, organization, workspace` | The levels the settings may be stored at: `global`, `organization`, `workspace`, `user` |
/// | `permission = ".."` | per level (`settings.manage`, `organization.update`, `workspace.update`) | The permission a write needs |
/// | `read_permission = ".."` | none | The permission a read through the routes needs |
///
/// The doc comment becomes the description. A struct without named
/// fields, generics, an unknown or duplicate argument, an unknown level or
/// an invalid key is a compile error. The generated code names
/// `::navree::settings` (or `::navree_settings`).
#[proc_macro_derive(Settings, attributes(settings))]
pub fn derive_settings(input: TokenStream) -> TokenStream {
    settings::expand(input.into()).into()
}

/// Implements `navree_sheets::Exportable` for a struct with named fields:
/// its columns (field order) and its cells (each field through
/// `navree_sheets::IntoCell`). Re-exported as `navree::sheets::Exportable`.
///
/// ```text
/// #[derive(Exportable)]
/// #[export(sheet = "Contacts")]
/// struct ContactRow {
///     #[export(header = "contacts.name")]
///     name: String,
///     #[export(format = "#,##0.00", width = 14)]
///     balance: f64,
///     #[export(with = "cents_to_cell")]
///     amount_cents: i64,
///     #[export(skip)]
///     internal_note: String,
/// }
/// ```
///
/// | Attribute | Effect |
/// |---|---|
/// | `#[export(sheet = "..")]` (struct) | the XLSX sheet name |
/// | `#[export(header = "..")]` | the header or its translation key (default: the field in sentence case) |
/// | `#[export(format = "..")]` | the column's Excel number format |
/// | `#[export(width = N)]` | the column width in characters (1 to 255) |
/// | `#[export(with = "path")]` | `fn(&FieldType) -> Cell` instead of `IntoCell` |
/// | `#[export(skip)]` | leaves the field out |
///
/// The generated code names `::navree::sheets` (or `::navree_sheets`).
#[proc_macro_derive(Exportable, attributes(export))]
pub fn derive_exportable(input: TokenStream) -> TokenStream {
    sheets::expand_exportable(input.into()).into()
}

/// Implements `navree_sheets::ImportRow` for an import's row type: the
/// columns an upload is mapped to, read from the struct's fields.
/// Re-exported as `navree::sheets::Importable`.
///
/// ```text
/// #[derive(Deserialize, Validate, Importable)]
/// struct RecipientRow {
///     #[import(header = "Email", aliases = ["E-mail", "Email address"])]
///     #[validate(email)]
///     email: String,
///     #[validate(length(max = 100))]
///     first_name: Option<String>,
/// }
/// ```
///
/// | Attribute | Effect |
/// |---|---|
/// | `#[import(header = "..")]` | the header the template shows and automatic mapping matches (default: the field in sentence case) |
/// | `#[import(aliases = [".."])]` | other headers automatic mapping accepts |
/// | `#[import(required)]` / `#[import(optional)]` | whether the column must be mapped (default: required unless the field is an `Option` or has `#[serde(default)]`) |
/// | `#[import(skip)]` | not a column (also `#[serde(skip)]`) |
///
/// A column's field is the Rust field name, or its `#[serde(rename = "..")]`;
/// `#[serde(rename_all)]` is a compile error. The generated code names
/// `::navree::sheets` (or `::navree_sheets`).
#[proc_macro_derive(Importable, attributes(import))]
pub fn derive_importable(input: TokenStream) -> TokenStream {
    sheets::expand_importable(input.into()).into()
}

/// Implements `navree_mcp::McpServer` for an inherent impl block whose
/// methods are MCP tools, resources and prompts.
///
/// ```text
/// #[mcp_server]
/// impl CrmMcp {
///     /// Lists the deals the user can see.            // the description
///     #[mcp_tool(read_only)]
///     #[can(Read, Deal)]
///     async fn list_deals(&self, cx: &McpContext, args: ListDeals) -> Result<Vec<DealResource>> { .. }
///
///     /// A deal with its company and contact.
///     #[mcp_resource("crm://deals/{id}", mime = "application/json")]
///     #[can(Read, Deal, from = "id")]
///     async fn deal(&self, cx: &McpContext, deal: Authorized<Deal, Read>) -> Result<DealResource> { .. }
///
///     #[mcp_prompt(name = "follow-up-email", title = "Draft a follow-up email")]
///     #[can(Read, Deal)]
///     async fn follow_up(&self, cx: &McpContext, args: FollowUpArgs) -> Result<Vec<PromptMessage>> { .. }
/// }
/// ```
///
/// | Attribute | On | Becomes |
/// |---|---|---|
/// | `#[mcp_tool]` | fn | a tool; keys `name = ".."`, `title = ".."` and the hints `read_only`, `destructive`, `idempotent`, `open_world` |
/// | `#[mcp_resource("uri")]` | fn | a resource, or a resource template when the URI has `{variables}`; keys `name`, `title`, `mime` |
/// | `#[mcp_prompt]` | fn | a prompt; keys `name`, `title` |
/// | `#[public]`, `#[authenticated]`, `#[roles(..)]`, `#[permissions(..)]`, `#[can(..)]` | fn, impl | who may list and use it, as on a route |
///
/// Each item is a non-generic `async fn` taking `&self` and returning a
/// `Result`; the method name is its default name and its doc comment its
/// description. Its other parameters are, in any order:
///
/// - `&McpContext` (the caller);
/// - for tools and prompts, one arguments value (a struct deriving
///   `Deserialize`, `navree_openapi::ToSchema` and `navree_validation::Validate`; its schema is
///   the tool's input schema or the prompt's arguments, and it is validated
///   before the method runs);
/// - for resource templates, parameters named like the URI's variables
///   (any `FromStr` type, a 404 when the URI carries no valid value);
/// - `Authorized<E, A>`, the record of the item's `#[can(A, E, from =
///   "..")]` check (a tool argument, a prompt argument or a URI variable).
///
/// Tools and resources return anything that serializes; a resource may
/// also return `ResourceContents` (or a `Vec` of them) and a prompt returns
/// `Vec<PromptMessage>`, a `PromptMessage` or a `String`.
///
/// **Deny by default.** An item without an access declaration (on itself
/// or on the impl block) does not compile. So do a URI without a scheme, a
/// `from` naming no URI variable, a second arguments value, two items of a
/// kind with the same name and a tool name with characters MCP does not
/// allow. Needs `navree` (feature `mcp`) or `navree-mcp` among the
/// dependencies; register the server with `m.mcp_server::<CrmMcp>()`.
#[proc_macro_attribute]
pub fn mcp_server(args: TokenStream, item: TokenStream) -> TokenStream {
    mcp::expand(args.into(), item.into()).into()
}

/// Declares an MCP tool on a method of a [`macro@mcp_server`] impl block.
/// `#[mcp_server]` consumes it; anywhere else it is a compile error.
#[proc_macro_attribute]
pub fn mcp_tool(_args: TokenStream, item: TokenStream) -> TokenStream {
    mcp::outside("mcp_tool", item.into()).into()
}

/// Declares an MCP resource (`#[mcp_resource("crm://pipeline")]`) or
/// resource template (`#[mcp_resource("crm://deals/{id}")]`) on a method of
/// a [`macro@mcp_server`] impl block. `#[mcp_server]` consumes it; anywhere
/// else it is a compile error.
#[proc_macro_attribute]
pub fn mcp_resource(_args: TokenStream, item: TokenStream) -> TokenStream {
    mcp::outside("mcp_resource", item.into()).into()
}

/// Declares an MCP prompt on a method of a [`macro@mcp_server`] impl block.
/// `#[mcp_server]` consumes it; anywhere else it is a compile error.
#[proc_macro_attribute]
pub fn mcp_prompt(_args: TokenStream, item: TokenStream) -> TokenStream {
    mcp::outside("mcp_prompt", item.into()).into()
}

/// Implements an agent's tools: on an `impl Agent for X` block (navree-ai),
/// collects the methods marked `#[tool]` into `Agent::tool_specs` and
/// `Agent::dispatch`, and moves them to an inherent `impl X`.
///
/// ```text
/// #[agent]
/// impl Agent for DealsAssistant {
///     const NAME: &'static str = "deals-assistant";
///     fn instructions(&self, run: &AgentRun) -> String { .. }
///
///     /// Finds the deals the user can see.        <- the tool's description
///     #[tool(read_only)]                          // or #[tool], #[tool(needs_approval)]
///     async fn search_deals(&self, run: &AgentRun, args: SearchDeals) -> Result<Vec<DealResource>> { .. }
///
///     /// Gives a deal to another member of the team.
///     #[tool(needs_approval)]
///     #[can(Manage, Deal)]                        // or #[permissions(..)], #[roles(..)]
///     async fn reassign_deal(&self, run: &AgentRun, args: ReassignDeal) -> Result<DealResource> { .. }
/// }
/// ```
///
/// A tool is an `async fn` taking `&self`, the run (`&AgentRun`) and at
/// most one arguments value, whose type derives `Deserialize`,
/// `ToSchema` (its JSON Schema, field doc comments included) and
/// `navree_validation::Validate`; it returns a `Result` of anything that serializes,
/// with an error that converts into `AppError`. The method name is the tool
/// name; its doc comment, required, is the description. `#[can]` with
/// marker types needs `navree` (or `navree-access`) among the dependencies;
/// everything else needs `navree` (or `navree-ai`). Methods without
/// `#[tool]` stay trait items.
#[proc_macro_attribute]
pub fn agent(args: TokenStream, item: TokenStream) -> TokenStream {
    agent::expand(args.into(), item.into()).into()
}

/// Implements `navree_workflows::Workflow` on a workflow's input type from
/// an `async fn` (re-exported as `navree::workflows::workflow`).
///
/// ```text
/// /// Welcomes a new user, then nudges them after three days.
/// #[workflow]
/// async fn onboarding(ctx: WorkflowCtx, input: Onboarding) -> Result<(), BoxError> {
///     ctx.step("send-welcome", || send_welcome(input.user_id)).await?;
///     ctx.sleep(Duration::from_secs(3 * 86_400)).await;
///     ctx.step("send-nudge", || send_nudge(input.user_id)).await?;
///     Ok(())
/// }
/// // impl navree::workflows::Workflow for Onboarding {
/// //     const NAME: &'static str = "onboarding";
/// //     const DESCRIPTION: &'static str = "Welcomes a new user, then nudges them after three days.";
/// //     type Output = ();
/// //     fn run(self, ctx: WorkflowCtx) -> impl Future<..> + Send { /* onboarding(ctx, self) */ }
/// // }
/// ```
///
/// | Argument | Default | Meaning |
/// |---|---|---|
/// | `name = ".."` | the fn name in kebab case (`deal_follow_up` → `deal-follow-up`) | the stable name stored with runs |
/// | `version = N` | 1 | the version new runs start on |
/// | `min_version = N` | `version` | the oldest version whose runs this code resumes |
///
/// The fn takes `(ctx: WorkflowCtx, input: In)` (the input by value; `In`
/// is a local type deriving `Serialize` and `Deserialize`) and returns
/// `Result<T, E>`: `T` is stored as the run's output and `E` converts into
/// `BoxError`. The fn stays callable as written. The doc comment becomes
/// the description. The expansion names `::navree::workflows` (the facade's
/// `workflows` feature) or `::navree_workflows`.
#[proc_macro_attribute]
pub fn workflow(args: TokenStream, item: TokenStream) -> TokenStream {
    workflows::expand_workflow(args.into(), item.into()).into()
}

/// Implements `navree_workflows::Trigger` on an automation trigger's
/// configuration type (re-exported as `navree::workflows::trigger`): on a
/// fn that decides which events fire it, or on the configuration struct.
///
/// ```text
/// /// A deal moved to won.
/// #[trigger(type = "deal.won", label = "Deal won", subject = "owner_id")]
/// fn deal_won(config: &DealWonTrigger, event: &DealWon) -> bool {
///     config.min_amount_cents.is_none_or(|min| event.amount_cents >= min)
/// }
///
/// /// A task was completed.
/// #[trigger(type = "task.completed", event = TaskCompleted, workspace = false)]
/// #[derive(Serialize, Deserialize, ToSchema)]
/// struct TaskCompletedTrigger {}
/// ```
///
/// | Argument | Default | Meaning |
/// |---|---|---|
/// | `type = ".."` | required | the stable type name stored in definitions |
/// | `label = ".."` | the type name | the name the editor shows |
/// | `subject = ".."` | none | the event field naming who or what is enrolled |
/// | `workspace = ".."` or `false` | `"workspace_id"` | the event field holding the workspace id, or none |
/// | `event = Type` | the fn's second argument | the event, for the struct form |
///
/// The configuration type derives `Serialize`, `Deserialize` and
/// `ToSchema` (its JSON Schema is the editor's form); the event implements
/// `navree_events::Event`. The doc comment becomes the description.
#[proc_macro_attribute]
pub fn trigger(args: TokenStream, item: TokenStream) -> TokenStream {
    workflows::expand_trigger(args.into(), item.into()).into()
}

/// Implements `navree_workflows::Condition` on an automation condition's
/// configuration type from a fn (re-exported as
/// `navree::workflows::condition`).
///
/// ```text
/// /// The won deal is worth at least the amount.
/// #[condition(type = "deal.amount_at_least", label = "Deal amount")]
/// async fn amount_at_least(config: &AmountAtLeast, ctx: &AutomationCtx) -> Result<bool, BoxError> {
///     let deal: DealWon = ctx.event_as()?;
///     Ok(deal.amount_cents >= config.cents)
/// }
/// ```
///
/// The fn (async or not) takes `(config: &Config, ctx: &AutomationCtx)`
/// and returns `bool` or `Result<bool, E>`. Keys: `type = ".."` (required)
/// and `label = ".."`. Keep it free of side effects: simulations evaluate
/// it too.
#[proc_macro_attribute]
pub fn condition(args: TokenStream, item: TokenStream) -> TokenStream {
    workflows::expand_condition(args.into(), item.into()).into()
}

/// Implements `navree_workflows::Action` on an automation action's
/// configuration type from a fn (re-exported as
/// `navree::workflows::action`).
///
/// ```text
/// /// Sends a mail from a template.
/// #[action(type = "mail.send", label = "Send a mail", preview = mail_preview)]
/// async fn send_mail(config: SendMail, ctx: AutomationCtx) -> Result<MailSent, BoxError> {
///     let mailer = ctx.app().get::<Mailer>().await?;
///     ..
/// }
///
/// fn mail_preview(config: &SendMail, ctx: &AutomationCtx) -> serde_json::Value { .. }
/// ```
///
/// The fn (async or not) takes `(config: Config, ctx: AutomationCtx)` by
/// value and returns `Result<T, E>`: `T` serializes and later nodes read it
/// as `nodes.<id>`. Keys: `type = ".."` (required), `label = ".."` and
/// `preview = path` (a `fn(&Config, &AutomationCtx) -> Value` a
/// simulation shows instead of running the action; default: the
/// configuration).
#[proc_macro_attribute]
pub fn action(args: TokenStream, item: TokenStream) -> TokenStream {
    workflows::expand_action(args.into(), item.into()).into()
}

/// Implements `navree_openapi::ToSchema` for a struct or an enum: its JSON
/// Schema, for the OpenAPI document, AI tools and structured output, MCP tools
/// and automation blocks.
///
/// The schema follows the type's serde attributes (`rename`, `rename_all`,
/// `skip`, `default`, `skip_serializing_if`, `flatten`, `tag`, `content`,
/// `untagged`, `deny_unknown_fields`), so it describes the JSON serde reads and
/// writes. Doc comments become descriptions. A field is required unless it's
/// an `Option`, has a serde default or may be skipped when serializing.
///
/// | Attribute | On | What it does |
/// |---|---|---|
/// | `#[schema(as = Name)]` | type | the component name (default: the Rust name) |
/// | `#[schema(description = "..")]` | type | the description, in place of the doc comment |
/// | `#[schema(value_type = T)]` | field | describes the field as `T`; `Object` is `{"type": "object"}` |
/// | `#[schema(inline)]` | field | the type's schema in place of a `$ref` |
/// | `#[schema(nullable = false)]` | field | an `Option` that's never `null` when present |
/// | `#[schema(required = true)]` | field | overrides whether the field is required |
/// | `#[schema(rename = "..")]` | field, variant | the name, unless serde renames it |
/// | `#[schema(format = Uuid)]` | field | `format`, a known name or a string |
/// | `#[schema(example = "..")]` | field | `example`: a literal or `json!(..)` |
/// | `#[schema(title = "..")]` | field | `title` |
/// | `#[schema(minimum = 0, maximum = 100)]` | field | the numeric limits |
/// | `#[schema(schema_with = function)]` | field | the schema a `fn() -> Schema` returns |
///
/// The generated code names `navree_openapi` through the `navree` facade when
/// the crate depends on it (`::navree::openapi`).
///
/// ```ignore
/// use navree::openapi::ToSchema;
///
/// /// A note on a deal.
/// #[derive(Serialize, ToSchema)]
/// #[schema(as = Note)]
/// #[serde(rename_all = "camelCase")]
/// pub struct NoteResource {
///     /// Always `"Note"`.
///     #[schema(example = "Note")]
///     pub resource_type: &'static str,
///     pub id: Uuid,
///     #[schema(format = Password)]
///     pub secret: Option<String>,
/// }
/// ```
#[proc_macro_derive(ToSchema, attributes(schema))]
pub fn derive_to_schema(input: TokenStream) -> TokenStream {
    to_schema::expand(input.into()).into()
}
