//! # navree-graphql
//!
//! `GraphqlModule`: a GraphQL API served from your modules, on
//! [async-graphql](https://docs.rs/async-graphql).
//!
//! - **Resolvers are providers.** An `#[injectable]` struct whose impl block
//!   carries `#[resolver]` (navree-macros, `navree::graphql::resolver`)
//!   contributes root fields: methods marked `#[query]`, `#[mutation]` or
//!   `#[subscription]`. Modules register them with
//!   [`ModuleBuilderGraphqlExt::resolver`]; at boot, [`GraphqlSchema`]
//!   merges every resolver's fields into one `Query`, `Mutation` and
//!   `Subscription` type (two resolvers defining the same field fail the
//!   boot).
//! - **Access like routes.** Every field declares `#[public]`,
//!   `#[authenticated]`, `#[roles]`, `#[permissions]` or `#[can]` (deny by
//!   default, at compile time); the app's route authorizer checks it when
//!   the field resolves, and `can(.., from = "id")` loads the record named
//!   by the field's `id` argument ([`ContextExt::authorized`]).
//! - **Request state** through [`ContextExt`] on the async-graphql
//!   [`Context`]: the unit of work, the viewer, their abilities and SQL
//!   filters, the workspace, the operation's dataloaders and the
//!   `AppContext`.
//! - **Units of work.** Query fields share one that commits nothing; each
//!   mutation field runs in its own, committed when it returns `Ok`.
//! - **Validation.** A `Valid<T>` argument runs `T`'s validation rules before
//!   the field, with the messages routes use.
//! - **Errors** keep Navree's JSON error shape in `extensions` (`code`,
//!   `statusCode`, `error`, `errors`), with each error's HTTP status.
//! - **Dataloaders**: a [`Loader`] registered by a module gets a
//!   [`RequestLoader`] per operation, which batches the loads of one tick
//!   and caches results for that operation only, so they never cross
//!   viewers.
//! - **Subscriptions** over WebSockets at `<path>/ws`, fed by the event bus
//!   through [`EventStream`].
//! - **Limits and tooling:** depth and complexity limits, introspection and
//!   GraphiQL in debug builds, batching on request, and the command
//!   `graphql:schema [path] [--check]`.
//!
//! ```ignore
//! use async_graphql::{Context, SimpleObject};
//! use navree::graphql::{ContextExt, Result, resolver};
//! use navree::prelude::*;
//!
//! #[derive(SimpleObject)]
//! struct Greeting {
//!     text: String,
//! }
//!
//! #[injectable]
//! pub struct GreetingResolver;
//!
//! #[resolver]
//! impl GreetingResolver {
//!     /// A greeting for the viewer.
//!     #[query]
//!     #[authenticated]
//!     async fn greeting(&self, ctx: &Context<'_>) -> Result<Greeting> {
//!         let me = ctx.principal()?.user_id;
//!         Ok(Greeting { text: format!("Hello, {me}!") })
//!     }
//! }
//!
//! #[module(register = Self::wire)]
//! pub struct GreetingModule;
//!
//! impl GreetingModule {
//!     fn wire(&self, m: &mut ModuleBuilder) {
//!         m.resolver::<GreetingResolver>();
//!     }
//! }
//! ```
#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod __private;

mod command;
mod context;
mod endpoint;
mod error;
mod events;
mod graphiql;
mod loader;
mod module;
mod operation;
mod options;
mod root;
mod schema;
mod ws;

/// async-graphql, the GraphQL engine (`navree::graphql::async_graphql`).
pub use async_graphql;
/// The async-graphql types a resolver signature names.
pub use async_graphql::{Context, Error, Result};
pub use context::ContextExt;
pub use events::EventStream;
pub use loader::{Loader, RequestLoader};
pub use module::{GraphqlModule, ModuleBuilderGraphqlExt};
pub use options::GraphqlOptions;
pub use schema::{GraphqlResolver, GraphqlSchema};
