//! Matching related records to their parents: key values normalized for
//! dictionaries, and the `IN` lists of batched loads.

use std::borrow::Cow;

use uuid::Uuid;

use crate::ir::{ColumnRef, Expr, InValues, Where};
use crate::value::{Decimal, Value, is_null, to_json};

/// A key value as a dictionary key: equal for the same key whatever its
/// representation (any integer width, a UUID as `Uuid`, text or 16 bytes,
/// an integral decimal or float as an integer).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationKey {
    Int(i128),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
}

/// The dictionary key of `value`; `None` for a NULL (it matches nothing).
pub(crate) fn key_of(value: &Value) -> Option<RelationKey> {
    if is_null(value) {
        return None;
    }
    Some(match value {
        Value::TinyInt(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::SmallInt(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::Int(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::BigInt(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::TinyUnsigned(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::SmallUnsigned(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::Unsigned(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::BigUnsigned(Some(v)) => RelationKey::Int(i128::from(*v)),
        Value::Bool(Some(v)) => RelationKey::Bool(*v),
        Value::Float(Some(v)) => float_key(f64::from(*v)),
        Value::Double(Some(v)) => float_key(*v),
        Value::Decimal(Some(v)) => decimal_key(v),
        Value::String(Some(v)) => text_key(v),
        Value::Char(Some(v)) => RelationKey::Text(v.to_string()),
        Value::Uuid(Some(v)) => RelationKey::Text(v.hyphenated().to_string()),
        Value::Bytes(Some(v)) => match Uuid::from_slice(v) {
            Ok(uuid) => RelationKey::Text(uuid.hyphenated().to_string()),
            Err(_) => RelationKey::Bytes(v.clone()),
        },
        other => RelationKey::Text(to_json(other).to_string()),
    })
}

fn float_key(value: f64) -> RelationKey {
    // An integral float within the exactly representable range is an
    // integer key (SQLite and aggregates can hand integers back as REAL).
    if value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
        // Exact: the value is integral and within 2^53.
        #[allow(clippy::cast_possible_truncation)]
        return RelationKey::Int(value as i128);
    }
    RelationKey::Text(value.to_string())
}

fn decimal_key(value: &Decimal) -> RelationKey {
    if value.fract().is_zero()
        && let Ok(int) = value.trunc().to_string().parse::<i128>()
    {
        return RelationKey::Int(int);
    }
    RelationKey::Text(value.normalize().to_string())
}

fn text_key(text: &str) -> RelationKey {
    // A UUID stored as text matches the same UUID stored natively.
    if text.len() == 36
        && let Ok(uuid) = Uuid::parse_str(text)
    {
        return RelationKey::Text(uuid.hyphenated().to_string());
    }
    RelationKey::Text(text.to_owned())
}

/// The distinct non-NULL values of `values`, sorted by key (so batched
/// queries are stable), each with the first value seen for its key.
pub(crate) fn distinct_keys(values: impl IntoIterator<Item = Value>) -> Vec<(RelationKey, Value)> {
    let mut keys: Vec<(RelationKey, Value)> = Vec::new();
    for value in values {
        if let Some(key) = key_of(&value)
            && !keys.iter().any(|(existing, _)| *existing == key)
        {
            keys.push((key, value));
        }
    }
    keys.sort_by(|(a, _), (b, _)| a.cmp(b));
    keys
}

/// `column IN (values)`: integer keys are written into the SQL (they are
/// numbers, and large batches stay clear of bound-parameter limits), the
/// others are bound.
pub(crate) fn in_keys(column: ColumnRef, keys: &[(RelationKey, Value)]) -> Where {
    let values = keys
        .iter()
        .map(|(key, value)| match key {
            RelationKey::Int(int) if is_integer(value) => Expr::Raw {
                sql: Cow::Owned(int.to_string()),
                values: Vec::new(),
            },
            _ => Expr::Value(value.clone()),
        })
        .collect();
    Where::In {
        expr: Expr::Column(column),
        values: InValues::List(values),
        not: false,
    }
}

fn is_integer(value: &Value) -> bool {
    matches!(
        value,
        Value::TinyInt(_)
            | Value::SmallInt(_)
            | Value::Int(_)
            | Value::BigInt(_)
            | Value::TinyUnsigned(_)
            | Value::SmallUnsigned(_)
            | Value::Unsigned(_)
            | Value::BigUnsigned(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_ignore_representation() {
        assert_eq!(
            key_of(&Value::Int(Some(7))),
            key_of(&Value::BigUnsigned(Some(7)))
        );
        assert_eq!(
            key_of(&Value::Double(Some(7.0))),
            key_of(&Value::BigInt(Some(7)))
        );
        assert_eq!(
            key_of(&Value::Decimal(Some(Decimal::new(700, 2)))),
            key_of(&Value::BigInt(Some(7)))
        );
        let uuid = Uuid::now_v7();
        assert_eq!(
            key_of(&Value::Uuid(Some(uuid))),
            key_of(&Value::from(uuid.to_string().to_uppercase()))
        );
        assert_eq!(
            key_of(&Value::Uuid(Some(uuid))),
            key_of(&Value::Bytes(Some(uuid.as_bytes().to_vec())))
        );
        assert_ne!(key_of(&Value::from("7")), key_of(&Value::BigInt(Some(7))));
        assert_eq!(key_of(&Value::BigInt(None)), None);
    }

    #[test]
    fn distinct_keys_are_sorted_and_unique() {
        let keys = distinct_keys([
            Value::BigInt(Some(3)),
            Value::BigInt(None),
            Value::Int(Some(1)),
            Value::BigInt(Some(3)),
        ]);
        let ints: Vec<RelationKey> = keys.into_iter().map(|(key, _)| key).collect();
        assert_eq!(ints, [RelationKey::Int(1), RelationKey::Int(3)]);
    }

    #[test]
    fn integer_keys_are_inlined_others_bound() {
        let keys = distinct_keys([Value::BigInt(Some(2)), Value::BigInt(Some(1))]);
        let Where::In {
            values: InValues::List(list),
            ..
        } = in_keys(ColumnRef::new(Some("t"), "id"), &keys)
        else {
            panic!("an IN list");
        };
        assert!(matches!(&list[0], Expr::Raw { sql, .. } if sql == "1"));
        let keys = distinct_keys([Value::from("a")]);
        let Where::In {
            values: InValues::List(list),
            ..
        } = in_keys(ColumnRef::new(Some("t"), "id"), &keys)
        else {
            panic!("an IN list");
        };
        assert!(matches!(&list[0], Expr::Value(_)));
    }
}
