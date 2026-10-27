//! Query-time casts: converting the extra columns a query selects
//! (aggregates, expressions, joined columns) as the records are read.

use std::borrow::Cow;
use std::fmt;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::RoundingStrategy;
use uuid::Uuid;

use crate::cast::CastError;
use crate::error::{OrmError, Result};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::value::{Decimal, Field, Value, is_null};

/// How an extra column is converted when records are read
/// ([`Builder::with_casts`]): databases type expressions loosely (SQLite's
/// comparisons are integers, MySQL's sums are decimals, JSON built in SQL
/// is text), and a query-time cast gives the extra its Rust-side type, in
/// typed reads ([`Record::extra`]) and in serialized records alike.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum ExtraCast {
    /// A boolean (`0`/`1`, `"true"`, ...).
    Bool,
    /// A 64-bit integer.
    Integer,
    /// A double-precision float.
    Float,
    /// A decimal, rounded half away from zero to the scale when one is
    /// given.
    Decimal(Option<u32>),
    /// Text.
    String,
    /// JSON (text is parsed).
    Json,
    /// A UTC date-time.
    DateTime,
    /// A date.
    Date,
    /// A UUID.
    Uuid,
    /// Your own conversion.
    Custom(fn(Value) -> std::result::Result<Value, CastError>),
}

impl fmt::Debug for ExtraCast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtraCast::Bool => f.write_str("Bool"),
            ExtraCast::Integer => f.write_str("Integer"),
            ExtraCast::Float => f.write_str("Float"),
            ExtraCast::Decimal(scale) => f.debug_tuple("Decimal").field(scale).finish(),
            ExtraCast::String => f.write_str("String"),
            ExtraCast::Json => f.write_str("Json"),
            ExtraCast::DateTime => f.write_str("DateTime"),
            ExtraCast::Date => f.write_str("Date"),
            ExtraCast::Uuid => f.write_str("Uuid"),
            ExtraCast::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

impl ExtraCast {
    /// Converts `value`; NULL stays NULL.
    pub fn convert(&self, value: Value) -> std::result::Result<Value, CastError> {
        if is_null(&value) {
            return Ok(match self {
                ExtraCast::Bool => Value::Bool(None),
                ExtraCast::Integer => Value::BigInt(None),
                ExtraCast::Float => Value::Double(None),
                ExtraCast::Decimal(_) => Value::Decimal(None),
                ExtraCast::String => Value::String(None),
                ExtraCast::Json => Value::Json(None),
                ExtraCast::DateTime => Value::ChronoDateTimeUtc(None),
                ExtraCast::Date => Value::ChronoDate(None),
                ExtraCast::Uuid => Value::Uuid(None),
                ExtraCast::Custom(_) => value,
            });
        }
        Ok(match self {
            ExtraCast::Bool => Value::Bool(Some(bool::from_value(value)?)),
            ExtraCast::Integer => Value::BigInt(Some(i64::from_value(value)?)),
            ExtraCast::Float => Value::Double(Some(f64::from_value(value)?)),
            ExtraCast::Decimal(scale) => {
                let decimal = Decimal::from_value(value)?;
                Value::Decimal(Some(match scale {
                    Some(scale) => {
                        let mut rounded = decimal
                            .round_dp_with_strategy(*scale, RoundingStrategy::MidpointAwayFromZero);
                        rounded.rescale(*scale);
                        rounded
                    }
                    None => decimal,
                }))
            }
            ExtraCast::String => Value::String(Some(String::from_value(value)?)),
            ExtraCast::Json => Value::Json(Some(Box::new(serde_json::Value::from_value(value)?))),
            ExtraCast::DateTime => Value::ChronoDateTimeUtc(Some(DateTime::<Utc>::from_value(value)?)),
            ExtraCast::Date => Value::ChronoDate(Some(NaiveDate::from_value(value)?)),
            ExtraCast::Uuid => Value::Uuid(Some(Uuid::from_value(value)?)),
            ExtraCast::Custom(convert) => convert(value)?,
        })
    }
}

/// The query-time casts of a query.
#[derive(Clone, Debug, Default)]
pub(crate) struct ExtraCasts(Vec<(Cow<'static, str>, ExtraCast)>);

impl ExtraCasts {
    /// Converts `record`'s extras that have a cast (extras the query did not
    /// select are left alone).
    pub(crate) fn apply<M: Model>(&self, record: &mut Record<M>) -> Result<()> {
        for (name, cast) in &self.0 {
            if let Some((_, value)) = record.extras.iter_mut().find(|(extra, _)| extra == name) {
                let converted = cast.convert(std::mem::replace(value, Value::String(None)));
                *value = converted.map_err(|error| OrmError::Cast {
                    model: M::meta().name().into(),
                    column: name.to_string(),
                    message: error.to_string(),
                })?;
            }
        }
        Ok(())
    }
}

impl<M: Model> Builder<M> {
    /// Converts extra columns this query selects as its records are read:
    /// per-query casts for aggregates and expressions.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::attributes::ExtraCast;
    ///
    /// # async fn demo(db: Db) -> Result<()> {
    /// let deals = Deal::query()
    ///     .select_raw("amount_cents > 100000 AS large", [] as [i64; 0])
    ///     .select_raw("amount_cents / 3.0 AS third", [] as [i64; 0])
    ///     .with_casts([("large", ExtraCast::Bool), ("third", ExtraCast::Decimal(Some(2)))])
    ///     .get(&db)
    ///     .await?;
    /// let large: Option<bool> = deals[0].extra("large")?; // SQLite's 0/1 reads as a bool
    /// # Ok(()) }
    /// ```
    pub fn with_casts<N: Into<Cow<'static, str>>>(
        mut self,
        casts: impl IntoIterator<Item = (N, ExtraCast)>,
    ) -> Self {
        for (name, cast) in casts {
            let name = name.into();
            self.extra_casts.0.retain(|(existing, _)| *existing != name);
            self.extra_casts.0.push((name, cast));
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_and_nulls() {
        assert_eq!(ExtraCast::Bool.convert(Value::BigInt(Some(1))).unwrap(), Value::Bool(Some(true)));
        assert_eq!(
            ExtraCast::Decimal(Some(2)).convert(Value::Double(Some(2.0 / 3.0))).unwrap(),
            Value::Decimal(Some(Decimal::new(67, 2)))
        );
        assert_eq!(
            ExtraCast::Json.convert(Value::from(r#"{"a":1}"#)).unwrap(),
            Value::Json(Some(Box::new(serde_json::json!({"a": 1}))))
        );
        assert_eq!(ExtraCast::Integer.convert(Value::String(None)).unwrap(), Value::BigInt(None));
        assert!(ExtraCast::Uuid.convert(Value::from("nope")).is_err());
        let upper = ExtraCast::Custom(|value| match value {
            Value::String(Some(s)) => Ok(Value::from(s.to_uppercase())),
            _ => Err(CastError::new("text only")),
        });
        assert_eq!(upper.convert(Value::from("ok")).unwrap(), Value::from("OK"));
        assert_eq!(format!("{upper:?}"), "Custom(..)");
    }
}
