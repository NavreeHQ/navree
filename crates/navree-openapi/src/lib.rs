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
#[doc(hidden)]
pub use schema::RustType;
pub use schema::{NotASchema, Schema, SchemaType, Subschema, Type};
pub use to_schema::{Object, ToSchema, components_of};

/// `#[derive(ToSchema)]`: see the macro's documentation in navree-macros.
pub use navree_macros::ToSchema;

/// What the derive's expansion names.
#[doc(hidden)]
pub mod __private {
    use std::cell::RefCell;
    use std::marker::PhantomData;

    use serde::Serialize;
    pub use serde_json;
    use serde_json::{Map, Value};

    use crate::Schema;

    /// The member of a schema that carries the schemas it refers to.
    pub const COMPONENTS: &str = "x-navree-components";
    /// The member that says which Rust type each of those came from.
    pub const COMPONENT_TYPES: &str = "x-navree-component-types";
    /// The member listing different schemas that share a name:
    /// `[name, first type, second type]`.
    pub const COMPONENT_CONFLICTS: &str = "x-navree-component-conflicts";

    /// Puts `referenced` (what [`ToSchema::schemas`](crate::ToSchema::schemas)
    /// collected) in `schema`, for the document to read back. A different
    /// schema of a name already there isn't lost: it is listed as a conflict.
    pub fn attach_components(schema: &mut Value, referenced: Vec<(String, Schema)>) {
        let Some(object) = schema.as_object_mut() else {
            return;
        };
        if referenced.is_empty() {
            return;
        }
        let mut components = Map::new();
        let mut types = Map::new();
        let mut conflicts = Vec::new();
        let mut first_of: Vec<(String, Value, Option<&'static str>)> = Vec::new();
        for (name, schema) in referenced {
            let rust_type = schema.rust_type.0;
            let value = schema.to_value();
            match first_of.iter().find(|(first, _, _)| *first == name) {
                Some((_, first, _)) if *first == value => {}
                Some((_, _, first_type)) => {
                    conflicts.push(serde_json::json!([name, first_type, rust_type]));
                }
                None => {
                    if let Some(rust_type) = rust_type {
                        types.insert(name.clone(), Value::from(rust_type));
                    }
                    components.insert(name.clone(), value.clone());
                    first_of.push((name, value, rust_type));
                }
            }
        }
        object.insert(COMPONENTS.to_owned(), Value::Object(components));
        if !types.is_empty() {
            object.insert(COMPONENT_TYPES.to_owned(), Value::Object(types));
        }
        if !conflicts.is_empty() {
            object.insert(COMPONENT_CONFLICTS.to_owned(), Value::Array(conflicts));
        }
    }

    thread_local! {
        static INLINING: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    /// Held while a type with `#[schema(inline)]` fields builds its schema, so
    /// types that inline each other stop with a message.
    pub struct InlineGuard;

    impl InlineGuard {
        /// Notes that `rust_type` is building its schema.
        ///
        /// # Panics
        ///
        /// When it is already: the schema would never end, and the document is
        /// built at boot.
        pub fn enter(rust_type: &'static str) -> InlineGuard {
            INLINING.with(|inlining| {
                let mut inlining = inlining.borrow_mut();
                if inlining.contains(&rust_type) {
                    let path = inlining.join(" -> ");
                    inlining.clear();
                    panic!(
                        "#[schema(inline)] on a field of `{rust_type}` makes it contain itself ({path} -> {rust_type}): remove `inline` from one of the fields"
                    );
                }
                inlining.push(rust_type);
            });
            InlineGuard
        }
    }

    impl Drop for InlineGuard {
        fn drop(&mut self) {
            INLINING.with(|inlining| {
                inlining.borrow_mut().pop();
            });
        }
    }

    /// `T::default()` when `T` has one: `(&DefaultOf::<T>::new()).get()`, with
    /// both traits in scope, picks [`HasDefault`] if it applies.
    pub struct DefaultOf<T>(PhantomData<T>);

    impl<T> DefaultOf<T> {
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            DefaultOf(PhantomData)
        }
    }

    pub trait HasDefault<T> {
        fn get(&self) -> Option<T>;
    }

    impl<T: Default> HasDefault<T> for DefaultOf<T> {
        fn get(&self) -> Option<T> {
            Some(T::default())
        }
    }

    pub trait NoDefault<T> {
        fn get(&self) -> Option<T> {
            None
        }
    }

    impl<T> NoDefault<T> for &DefaultOf<T> {}

    /// `value` as JSON when it is `Serialize`:
    /// `(&JsonOf(&value)).get()`, with both traits in scope.
    pub struct JsonOf<'a, T: ?Sized>(pub &'a T);

    pub trait HasJson {
        fn get(&self) -> Option<Value>;
    }

    impl<T: Serialize + ?Sized> HasJson for JsonOf<'_, T> {
        fn get(&self) -> Option<Value> {
            Some(serde_json::to_value(self.0).unwrap_or_default())
        }
    }

    pub trait NoJson {
        fn get(&self) -> Option<Value> {
            None
        }
    }

    impl<T: ?Sized> NoJson for &JsonOf<'_, T> {}
}
