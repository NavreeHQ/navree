//! `cast = "vector"`: embeddings in the database's vector column type.

use std::borrow::Cow;

use sea_orm::QueryResult;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::sql::Dialect;
use crate::value::{Field, FieldError, FieldKind, Value, sql_get};

/// How a vector travels to its database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wire {
    /// pgvector's `vector` (binary protocol, with navree-orm's `postgres`
    /// feature).
    Postgres,
    /// Little-endian 32-bit floats: MySQL 9's and MariaDB 11.7's binary
    /// vector format, and the blob SQLite extensions such as sqlite-vec read.
    Binary,
    /// pgvector's text form, `[1,2,3]` (outside a database operation).
    Text,
}

impl Wire {
    fn of(context: &CastContext) -> Wire {
        match context.dialect() {
            Some(Dialect::Postgres) => Wire::Postgres,
            Some(Dialect::MySql | Dialect::Sqlite) => Wire::Binary,
            None => Wire::Text,
        }
    }
}

/// The stored form of a [`Vector`] cast: the floats, and the form the
/// database the record is written to takes them in.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredVector {
    floats: Vec<f32>,
    wire: Wire,
}

impl StoredVector {
    /// The vector's floats.
    pub fn floats(&self) -> &[f32] {
        &self.floats
    }

    fn text(floats: &[f32]) -> String {
        let items: Vec<String> = floats.iter().map(f32::to_string).collect();
        format!("[{}]", items.join(","))
    }

    fn parse_text(text: &str) -> Result<Vec<f32>, FieldError> {
        let inner = text
            .trim()
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .ok_or_else(|| FieldError::Invalid(format!("`{text}` is not a vector (`[1,2,3]`)")))?;
        if inner.trim().is_empty() {
            return Ok(Vec::new());
        }
        inner
            .split(',')
            .map(|item| {
                item.trim().parse::<f32>().map_err(|_| {
                    FieldError::Invalid(format!("`{}` in a vector is not a number", item.trim()))
                })
            })
            .collect()
    }

    fn from_bytes(bytes: &[u8]) -> Result<Vec<f32>, FieldError> {
        if bytes.len() % 4 != 0 {
            return Err(FieldError::Invalid(format!(
                "{} bytes are not a vector of 32-bit floats",
                bytes.len()
            )));
        }
        Ok(bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect())
    }

    fn to_bytes(floats: &[f32]) -> Vec<u8> {
        floats.iter().flat_map(|f| f.to_le_bytes()).collect()
    }
}

impl Field for StoredVector {
    const KIND: FieldKind = FieldKind::Vector;

    fn to_value(&self) -> Value {
        match self.wire {
            #[cfg(feature = "postgres")]
            Wire::Postgres => {
                Value::Vector(Some(sea_orm::prelude::PgVector::from(self.floats.clone())))
            }
            #[cfg(not(feature = "postgres"))]
            Wire::Postgres => Value::String(Some(StoredVector::text(&self.floats))),
            Wire::Binary => Value::Bytes(Some(StoredVector::to_bytes(&self.floats))),
            Wire::Text => Value::String(Some(StoredVector::text(&self.floats))),
        }
    }

    fn null() -> Value {
        Value::Bytes(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        #[cfg(feature = "postgres")]
        if let Ok(vector) = sql_get::<sea_orm::prelude::PgVector>(row, column) {
            return Ok(StoredVector {
                floats: vector.to_vec(),
                wire: Wire::Postgres,
            });
        }
        match sql_get::<Vec<u8>>(row, column) {
            Ok(bytes) => {
                // Text read back as bytes (`[1,2]`) is the text form.
                if bytes.first() == Some(&b'[') {
                    let text = String::from_utf8(bytes).map_err(|_| {
                        FieldError::Invalid("a vector's text is not UTF-8".to_owned())
                    })?;
                    return StoredVector::from_value(Value::String(Some(text)));
                }
                Ok(StoredVector {
                    floats: StoredVector::from_bytes(&bytes)?,
                    wire: Wire::Binary,
                })
            }
            Err(FieldError::Invalid(_)) => sql_get::<String>(row, column)
                .and_then(|text| StoredVector::from_value(Value::String(Some(text)))),
            Err(other) => Err(other),
        }
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        match value {
            #[cfg(feature = "postgres")]
            Value::Vector(Some(vector)) => Ok(StoredVector {
                floats: vector.to_vec(),
                wire: Wire::Postgres,
            }),
            Value::Bytes(Some(bytes)) => Ok(StoredVector {
                floats: StoredVector::from_bytes(&bytes)?,
                wire: Wire::Binary,
            }),
            Value::String(Some(text)) => Ok(StoredVector {
                floats: StoredVector::parse_text(&text)?,
                wire: Wire::Text,
            }),
            Value::Json(Some(json)) => match *json {
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(|item| {
                        item.as_f64()
                            .map(|f| {
                                // A vector's components are 32-bit floats.
                                #[allow(clippy::cast_possible_truncation)]
                                let narrow = f as f32;
                                narrow
                            })
                            .ok_or_else(|| {
                                FieldError::Invalid(format!("{item} in a vector is not a number"))
                            })
                    })
                    .collect::<Result<Vec<f32>, FieldError>>()
                    .map(|floats| StoredVector {
                        floats,
                        wire: Wire::Text,
                    }),
                other => Err(FieldError::Invalid(format!("{other} is not a vector"))),
            },
            other if crate::value::is_null(&other) => Err(FieldError::Null),
            other => Err(FieldError::Invalid(format!(
                "expected a vector, found {}",
                crate::value::describe(&other)
            ))),
        }
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Vector")
    }
}

/// Embeddings (`Vec<f32>`) in the database's vector type
/// (`#[model(cast = "vector")]`):
///
/// | Database | Column | Stored as |
/// |---|---|---|
/// | PostgreSQL | pgvector's `vector(N)` | pgvector's binary value (navree-orm's `postgres` feature) |
/// | MySQL 9 | `VECTOR(N)` | little-endian 32-bit floats, its binary vector format |
/// | MariaDB 11.7 | `VECTOR(N)` | the same bytes |
/// | SQLite | `BLOB` | the same bytes, which sqlite-vec reads |
///
/// Serialized records and `fill` use a JSON array of numbers. Query by
/// similarity with raw expressions (`order_by_raw("embedding <-> ?",
/// [..])` on PostgreSQL), which the database's vector index serves.
///
/// ```
/// use navree_orm::attributes::CastContext;
/// use navree_orm::cast::{Cast, Vector};
/// use navree_orm::{Dialect, Field, Value};
///
/// let embedding = vec![0.5_f32, -1.0, 2.25];
/// let mysql = CastContext::new().with_dialect(Dialect::MySql);
/// let stored = Vector::set_with(&embedding, &mysql)?;
/// assert_eq!(stored.to_value(), Value::Bytes(Some(vec![0, 0, 0, 63, 0, 0, 128, 191, 0, 0, 16, 64])));
/// let read: Vec<f32> = Vector::get_with(stored, &mysql)?;
/// assert_eq!(read, embedding);
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct Vector;

impl Cast<Vec<f32>> for Vector {
    type Stored = StoredVector;

    const CONTEXT: bool = true;

    const KIND: FieldKind = FieldKind::Vector;

    fn get(stored: StoredVector) -> Result<Vec<f32>, CastError> {
        Ok(stored.floats)
    }

    fn set(value: &Vec<f32>) -> Result<StoredVector, CastError> {
        CastContext::with_current(|context| Self::set_with(value, context))
    }

    fn set_with(value: &Vec<f32>, context: &CastContext) -> Result<StoredVector, CastError> {
        if let Some(bad) = value.iter().find(|f| !f.is_finite()) {
            return Err(CastError::new(format!(
                "a vector's components must be finite numbers, found {bad}"
            )));
        }
        Ok(StoredVector {
            floats: value.clone(),
            wire: Wire::of(context),
        })
    }

    fn null_with(context: &CastContext) -> Value {
        match Wire::of(context) {
            #[cfg(feature = "postgres")]
            Wire::Postgres => Value::Vector(None),
            #[cfg(not(feature = "postgres"))]
            Wire::Postgres => Value::String(None),
            Wire::Binary => Value::Bytes(None),
            Wire::Text => Value::String(None),
        }
    }

    fn same(a: &Vec<f32>, b: &Vec<f32>, context: &CastContext) -> bool {
        let _ = context;
        a == b
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<Vec<f32>, CastError> {
        let _ = context;
        StoredVector::from_value(Value::Json(Some(Box::new(input))))
            .map(|stored| stored.floats)
            .map_err(CastError::from)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_database_form_reads_back() {
        let floats = vec![1.5_f32, -2.0, 0.0];
        for dialect in [Dialect::MySql, Dialect::Sqlite, Dialect::Postgres] {
            let context = CastContext::new().with_dialect(dialect);
            let stored = Vector::set_with(&floats, &context).unwrap();
            let read = StoredVector::from_value(stored.to_value()).unwrap();
            assert_eq!(read.floats(), floats, "{dialect:?}");
        }
        let text = Vector::set_with(&floats, &CastContext::new()).unwrap();
        assert_eq!(text.to_value(), Value::from("[1.5,-2,0]"));
    }

    #[test]
    fn malformed_vectors_are_errors() {
        assert!(StoredVector::from_value(Value::Bytes(Some(vec![1, 2, 3]))).is_err());
        assert!(StoredVector::from_value(Value::from("1,2")).is_err());
        assert!(StoredVector::from_value(Value::from("[1,x]")).is_err());
        assert_eq!(
            StoredVector::from_value(Value::from("[]"))
                .unwrap()
                .floats(),
            [] as [f32; 0]
        );
        assert_eq!(
            StoredVector::from_value(Value::Bytes(None)).unwrap_err(),
            FieldError::Null
        );
        let error = Vector::set_with(&vec![f32::NAN], &CastContext::new()).unwrap_err();
        assert!(error.to_string().contains("finite"), "{error}");
    }

    #[test]
    fn nulls_match_the_column_type_of_each_database() {
        let mysql = CastContext::new().with_dialect(Dialect::MySql);
        assert_eq!(
            <Vector as Cast<Vec<f32>>>::null_with(&mysql),
            Value::Bytes(None)
        );
    }

    #[test]
    fn input_is_an_array_of_numbers() {
        let context = CastContext::new();
        let read: Vec<f32> = Vector::from_json(json!([0.25, 1, -3]), &context).unwrap();
        assert_eq!(read, [0.25, 1.0, -3.0]);
        assert!(<Vector as Cast<Vec<f32>>>::from_json(json!(["a"]), &context).is_err());
        assert!(<Vector as Cast<Vec<f32>>>::from_json(json!({"x": 1}), &context).is_err());
    }
}
