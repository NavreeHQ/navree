//! # navree-openapi
//!
//! The OpenAPI 3.1 document model and the [`ToSchema`] trait and derive that
//! describe Rust types as JSON Schema (for the document, for AI tools and
//! structured output, for MCP tools and for the automation editor).
//!
//! ```
//! use navree_openapi::ToSchema;
//! use serde_json::json;
//!
//! /// A note on a deal.
//! #[derive(ToSchema)]
//! #[schema(as = Note)]
//! struct NoteResource {
//!     /// The text.
//!     body: String,
//!     pinned: Option<bool>,
//! }
//!
//! assert_eq!(NoteResource::name(), "Note");
//! assert_eq!(
//!     serde_json::to_value(NoteResource::schema()).unwrap(),
//!     json!({
//!         "type": "object",
//!         "description": "A note on a deal.",
//!         "required": ["body"],
//!         "properties": {
//!             "body": {"type": "string", "description": "The text."},
//!             "pinned": {"type": ["boolean", "null"]},
//!         },
//!     })
//! );
//! ```
#![forbid(unsafe_code)]

// The derive names this crate as `::navree_openapi`, in its own tests too.
extern crate self as navree_openapi;

mod document;
mod schema;
mod to_schema;

pub use document::{
    Components, Document, Info, MediaType, Operation, Parameter, PathItem, RequestBody, Response,
    SecurityScheme, Server, Tag,
};
pub use schema::{NotASchema, Schema, SchemaType, Subschema, Type};
pub use to_schema::{Object, ToSchema};

/// `#[derive(ToSchema)]`: see the macro's documentation in navree-macros.
pub use navree_macros::ToSchema;

/// What the derive's expansion names.
#[doc(hidden)]
pub mod __private {
    pub use serde_json;
}
