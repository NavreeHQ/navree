//! Accessors: computed values of a model that serialized records can
//! append (`#[accessors]` impl blocks, `#[model(appends(..))]`).
//!
//! An accessor is a plain method of the model; marking it `#[accessor]` in
//! an `#[accessors]` impl block registers it under its name, so records can
//! append it to their JSON, by default for every record
//! (`#[model(appends(name))]`, checked when the model compiles) or per
//! record (`record.append(["name"])`):
//!
//! ```
//! use navree_orm::prelude::*;
//! use serde::Serialize;
//!
//! #[derive(Model, Clone, Debug, Default, Serialize)]
//! #[model(table = "people", appends(full_name))]
//! struct Person {
//!     id: i64,
//!     first_name: String,
//!     last_name: String,
//! }
//!
//! #[accessors]
//! impl Person {
//!     /// "Ada Lovelace".
//!     #[accessor]
//!     fn full_name(&self) -> String {
//!         format!("{} {}", self.first_name, self.last_name)
//!     }
//! }
//!
//! let ada = Record::new(Person { id: 1, first_name: "Ada".into(), last_name: "Lovelace".into() });
//! assert_eq!(ada.full_name(), "Ada Lovelace"); // still a plain method
//! assert_eq!(ada.to_value()?["full_name"], "Ada Lovelace");
//! # Ok::<(), navree_orm::OrmError>(())
//! ```

use std::fmt;

use crate::cast::CastError;
use crate::model::PartialSchemaFn;

/// A computed value of model `M`: its name, how to read it as JSON, and
/// its Rust type (for OpenAPI schemas). `#[accessors]` writes these.
pub struct Accessor<M> {
    name: &'static str,
    get: fn(&M) -> Result<serde_json::Value, CastError>,
    rust_type: &'static str,
    schema: Option<PartialSchemaFn>,
}

impl<M> Accessor<M> {
    /// The accessor `name`, read with `get`, returning Rust type
    /// `rust_type` (as written).
    pub const fn new(
        name: &'static str,
        get: fn(&M) -> Result<serde_json::Value, CastError>,
        rust_type: &'static str,
    ) -> Self {
        Accessor {
            name,
            get,
            rust_type,
            schema: None,
        }
    }

    /// The accessor's OpenAPI schema (`#[accessor(schema = Type)]`).
    pub const fn with_schema(self, schema: PartialSchemaFn) -> Self {
        Accessor {
            schema: Some(schema),
            ..self
        }
    }

    /// The accessor's name: its key in serialized records.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The Rust type the accessor returns, as written.
    pub const fn rust_type(&self) -> &'static str {
        self.rust_type
    }

    /// The accessor's OpenAPI schema override, if any.
    pub const fn schema_override(&self) -> Option<PartialSchemaFn> {
        self.schema
    }

    /// The accessor's value for `model`, as JSON.
    pub fn value(&self, model: &M) -> Result<serde_json::Value, CastError> {
        (self.get)(model)
    }
}

impl<M> Clone for Accessor<M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M> Copy for Accessor<M> {}

impl<M> fmt::Debug for Accessor<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accessor")
            .field("name", &self.name)
            .field("rust_type", &self.rust_type)
            .finish_non_exhaustive()
    }
}

/// The accessors of a model without an `#[accessors]` block: none. An
/// `#[accessors]` block defines an inherent `__NAVREE_ACCESSORS` constant,
/// which takes precedence over this one where `#[derive(Model)]` reads it.
#[doc(hidden)]
pub trait NoAccessors: Sized + 'static {
    /// The model's accessors.
    const __NAVREE_ACCESSORS: &'static [Accessor<Self>] = &[];
}

impl<T: 'static> NoAccessors for T {}

/// Whether `accessors` has one named `name` (a `const fn`, so
/// `#[derive(Model)]` checks `appends(..)` when the model compiles).
#[doc(hidden)]
pub const fn has_accessor<M>(accessors: &[Accessor<M>], name: &str) -> bool {
    let mut index = 0;
    while index < accessors.len() {
        if same_str(accessors[index].name, name) {
            return true;
        }
        index += 1;
    }
    false
}

const fn same_str(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if a[index] != b[index] {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Point {
        x: i32,
    }

    const ACCESSORS: &[Accessor<Point>] = &[Accessor::new(
        "doubled",
        |point: &Point| Ok(serde_json::json!(point.x * 2)),
        "i32",
    )];

    #[test]
    fn accessors_are_found_by_name_at_compile_time() {
        const {
            assert!(has_accessor(ACCESSORS, "doubled"));
            assert!(!has_accessor(ACCESSORS, "double"));
        }
        assert_eq!(
            ACCESSORS[0].value(&Point { x: 21 }).unwrap(),
            serde_json::json!(42)
        );
        assert_eq!(ACCESSORS[0].rust_type(), "i32");
        assert_eq!(
            format!("{:?}", ACCESSORS[0]),
            r#"Accessor { name: "doubled", rust_type: "i32", .. }"#
        );
        assert!(<Point as NoAccessors>::__NAVREE_ACCESSORS.is_empty());
    }
}
