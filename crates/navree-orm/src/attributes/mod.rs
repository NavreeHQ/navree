//! The attribute layer of models: how values get in (mass assignment,
//! mutators, casts), what they are stored as (casts that need the app: the
//! encrypter, the hasher), and how records go out (accessors, visibility,
//! serialization, API resources, OpenAPI schemas, references in queued
//! jobs).
//!
//! - [`Attributes`], [`attrs!`](crate::attrs) and
//!   [`Record::fill`](crate::Record::fill): mass assignment under the
//!   model's `fillable` / `guarded` columns ([`fill`]).
//! - [`CastContext`]: the services casts work with, resolved from the
//!   executor's app.
//! - [`Accessor`]: computed values records can append
//!   (`#[accessors]`, `#[model(appends(..))]`).

mod accessors;
mod arith;
pub(crate) mod casts;
mod context;
pub(crate) mod extras;
pub mod fill;
mod model_ref;
mod resource;
pub mod schema;
pub mod serialize;
pub(crate) mod state;
mod visibility;

pub use accessors::Accessor;
pub use extras::ExtraCast;
pub use model_ref::{ModelRef, ModelRefs};
pub use resource::{HasResource, Resource};
pub use schema::{ModelSchema, cursor_page_schema, page_schema, simple_page_schema};
pub use visibility::Condition;
#[doc(hidden)]
pub use accessors::{NoAccessors, has_accessor};
pub use context::CastContext;
pub(crate) use context::in_model_scope;
pub use fill::{
    Attributes, DiscardHandler, DiscardPolicy, discard_policy, is_unguarded, set_discard_policy,
    unguarded, with_discard_policy,
};
