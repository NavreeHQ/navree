//! Rule conditions: a conjunction of operators on top-level scalar fields,
//! serialized as a CASL (MongoDB-style) query object.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::Value;

use super::rule::RuleError;

/// One condition operator with its JSON operand.
///
/// Values are JSON scalars: `null`, booleans, numbers and strings. Integers
/// must be JavaScript-safe (|n| ≤ 2^53 − 1); larger integers and decimals
/// travel as strings and should only be used with equality operators.
#[derive(Clone, Debug, PartialEq)]
pub enum Cond {
    /// `$eq`: equal; `$eq: null` matches null (SQL `IS NULL`).
    Eq(Value),
    /// `$ne`: not equal; null fields are "not equal" to any non-null value
    /// (SQL `IS DISTINCT FROM`), and `$ne: null` means `IS NOT NULL`.
    Ne(Value),
    /// `$lt`: less than (numbers or strings; never matches null).
    Lt(Value),
    /// `$lte`: less than or equal.
    Lte(Value),
    /// `$gt`: greater than.
    Gt(Value),
    /// `$gte`: greater than or equal.
    Gte(Value),
    /// `$in`: equal to one of the values (a `null` member matches null).
    In(Vec<Value>),
    /// `$nin`: equal to none of the values (null fields match unless the
    /// list contains `null`).
    Nin(Vec<Value>),
}

impl Cond {
    /// The CASL operator name, e.g. `"$eq"`.
    pub fn operator(&self) -> &'static str {
        match self {
            Cond::Eq(_) => "$eq",
            Cond::Ne(_) => "$ne",
            Cond::Lt(_) => "$lt",
            Cond::Lte(_) => "$lte",
            Cond::Gt(_) => "$gt",
            Cond::Gte(_) => "$gte",
            Cond::In(_) => "$in",
            Cond::Nin(_) => "$nin",
        }
    }

    /// Builds a condition from a CASL operator name and its JSON operand.
    pub(crate) fn from_operator(
        field: &str,
        operator: &str,
        value: Value,
    ) -> Result<Self, RuleError> {
        let list = |value: Value, operator: &'static str| match value {
            Value::Array(items) => Ok(items),
            _ => Err(RuleError::InvalidValue {
                field: field.to_owned(),
                operator,
                reason: "expected an array".to_owned(),
            }),
        };
        Ok(match operator {
            "$eq" => Cond::Eq(value),
            "$ne" => Cond::Ne(value),
            "$lt" => Cond::Lt(value),
            "$lte" => Cond::Lte(value),
            "$gt" => Cond::Gt(value),
            "$gte" => Cond::Gte(value),
            "$in" => Cond::In(list(value, "$in")?),
            "$nin" => Cond::Nin(list(value, "$nin")?),
            other => return Err(RuleError::UnsupportedOperator(other.to_owned())),
        })
    }

    /// The JSON operand (an array for `$in` / `$nin`).
    pub(crate) fn operand(&self) -> Value {
        match self {
            Cond::Eq(v) | Cond::Ne(v) | Cond::Lt(v) | Cond::Lte(v) | Cond::Gt(v) | Cond::Gte(v) => {
                v.clone()
            }
            Cond::In(items) | Cond::Nin(items) => Value::Array(items.clone()),
        }
    }

    /// Whether `value` (the record's field; `Null` when absent) satisfies
    /// this condition.
    pub(crate) fn matches(&self, value: &Value) -> bool {
        match self {
            Cond::Eq(expected) => scalar_eq(value, expected),
            Cond::Ne(expected) => !scalar_eq(value, expected),
            Cond::Lt(bound) => scalar_cmp(value, bound) == Some(Ordering::Less),
            Cond::Lte(bound) => matches!(
                scalar_cmp(value, bound),
                Some(Ordering::Less | Ordering::Equal)
            ),
            Cond::Gt(bound) => scalar_cmp(value, bound) == Some(Ordering::Greater),
            Cond::Gte(bound) => matches!(
                scalar_cmp(value, bound),
                Some(Ordering::Greater | Ordering::Equal)
            ),
            Cond::In(items) => items.iter().any(|item| scalar_eq(value, item)),
            Cond::Nin(items) => !items.iter().any(|item| scalar_eq(value, item)),
        }
    }

    fn validate(&self, field: &str) -> Result<(), RuleError> {
        let invalid = |reason: &str| RuleError::InvalidValue {
            field: field.to_owned(),
            operator: self.operator(),
            reason: reason.to_owned(),
        };
        let scalar = |value: &Value| match value {
            Value::Array(_) | Value::Object(_) => Err(invalid("expected a JSON scalar")),
            Value::Number(n) if !is_safe_number(n) => Err(invalid(
                "integers beyond ±(2^53 − 1) lose precision in JavaScript; pass them as strings",
            )),
            _ => Ok(()),
        };
        match self {
            Cond::Eq(value) | Cond::Ne(value) => scalar(value),
            Cond::Lt(value) | Cond::Lte(value) | Cond::Gt(value) | Cond::Gte(value) => {
                match value {
                    Value::Number(_) | Value::String(_) => scalar(value),
                    _ => Err(invalid("range operators take a number or a string")),
                }
            }
            Cond::In(items) | Cond::Nin(items) => items.iter().try_for_each(scalar),
        }
    }
}

/// Largest integer JavaScript represents exactly (2^53 − 1).
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

fn is_safe_number(n: &serde_json::Number) -> bool {
    match (n.as_i64(), n.as_u64()) {
        (Some(i), _) => i.unsigned_abs() <= MAX_SAFE_INTEGER,
        (None, Some(u)) => u <= MAX_SAFE_INTEGER,
        // A float: JSON numbers are always finite.
        (None, None) => true,
    }
}

/// Equality of JSON scalars as SQL sees them: numbers by value (`1 == 1.0`),
/// strings by code points, `null` only equal to `null`, never across types.
/// Arrays and objects (not scalar columns) equal nothing.
fn scalar_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => compare_numbers(a, b) == Some(Ordering::Equal),
        (Value::String(a), Value::String(b)) => a == b,
        _ => false,
    }
}

/// Order of two numbers or two strings (code point order, which is SQL's
/// binary collation); `None` for `null`, other types and mixed types.
fn scalar_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => compare_numbers(a, b),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// Numeric order: exact for integers, `f64` once either side is a float.
fn compare_numbers(a: &serde_json::Number, b: &serde_json::Number) -> Option<Ordering> {
    match (as_i128(a), as_i128(b)) {
        (Some(a), Some(b)) => Some(a.cmp(&b)),
        _ => a.as_f64()?.partial_cmp(&b.as_f64()?),
    }
}

fn as_i128(n: &serde_json::Number) -> Option<i128> {
    n.as_i64()
        .map(i128::from)
        .or_else(|| n.as_u64().map(i128::from))
}

/// A conjunction of [`Cond`]s on top-level fields (CASL `conditions`).
///
/// Build with [`field`] and [`Conditions::and`]; serialized as CASL's
/// MongoDB-style query object, `{"field": value | {"$op": value, ...}}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conditions {
    fields: BTreeMap<String, Vec<Cond>>,
    /// Problems found while building (e.g. a non-finite float); reported by
    /// [`Conditions::validate`] so the builder API stays infallible.
    errors: Vec<RuleError>,
}

impl Conditions {
    /// No conditions (matches every record).
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `cond` on `field`.
    pub fn with(mut self, field: &str, cond: Cond) -> Self {
        self.fields.entry(field.to_owned()).or_default().push(cond);
        self
    }

    /// Both these conditions and `other` must hold.
    pub fn and(mut self, other: Conditions) -> Conditions {
        for (field, conds) in other.fields {
            self.fields.entry(field).or_default().extend(conds);
        }
        self.errors.extend(other.errors);
        self
    }

    /// The conditions per field, in field-name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[Cond])> {
        self.fields
            .iter()
            .map(|(field, conds)| (field.as_str(), conds.as_slice()))
    }

    /// The conditions on `field`.
    pub fn get(&self, field: &str) -> Option<&[Cond]> {
        self.fields.get(field).map(Vec::as_slice)
    }

    /// Whether there are no conditions.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Whether `record` (a JSON object) satisfies every condition. A field
    /// the record lacks counts as `null`; a non-object record has no fields.
    pub fn matches(&self, record: &Value) -> bool {
        self.iter().all(|(field, conds)| {
            let value = record.get(field).unwrap_or(&Value::Null);
            conds.iter().all(|cond| cond.matches(value))
        })
    }

    /// Checks field names, operand types and duplicate operators.
    pub fn validate(&self) -> Result<(), RuleError> {
        if let Some(error) = self.errors.first() {
            return Err(error.clone());
        }
        for (field, conds) in &self.fields {
            validate_field_name(field)?;
            for (i, cond) in conds.iter().enumerate() {
                cond.validate(field)?;
                if conds[..i].iter().any(|c| c.operator() == cond.operator()) {
                    return Err(RuleError::DuplicateOperator {
                        field: field.clone(),
                        operator: cond.operator(),
                    });
                }
            }
        }
        Ok(())
    }

    fn invalid(field: &str, operator: &'static str, reason: &str) -> Conditions {
        Conditions {
            fields: BTreeMap::new(),
            errors: vec![RuleError::InvalidValue {
                field: field.to_owned(),
                operator,
                reason: reason.to_owned(),
            }],
        }
    }
}

/// Checks that `name` is a top-level column name: non-empty ASCII letters,
/// digits and `_`.
pub(crate) fn validate_field_name(name: &str) -> Result<(), RuleError> {
    if name.contains('.') {
        return Err(RuleError::NestedField(name.to_owned()));
    }
    if name.starts_with('$') {
        return Err(RuleError::UnsupportedOperator(name.to_owned()));
    }
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(RuleError::InvalidField(name.to_owned()));
    }
    Ok(())
}

/// A field to build [`Conditions`] on; see [`field`].
///
/// `FieldRef::new` is `const`, so entity column constants can be declared
/// once and reused: `const AUTHOR_ID: FieldRef = FieldRef::new("author_id");`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldRef(Cow<'static, str>);

/// A field to put conditions on: `field("author_id").eq(user_id)`.
pub fn field(name: &str) -> FieldRef {
    FieldRef(Cow::Owned(name.to_owned()))
}

macro_rules! scalar_ops {
    ($($(#[$doc:meta])* $method:ident => $variant:ident, $operator:literal;)*) => {
        $(
            $(#[$doc])*
            pub fn $method(&self, value: impl IntoCondValue) -> Conditions {
                match value.into_cond_value() {
                    Some(value) => Conditions::new().with(&self.0, Cond::$variant(value)),
                    None => Conditions::invalid(&self.0, $operator, NON_FINITE),
                }
            }
        )*
    };
}

const NON_FINITE: &str = "not representable in JSON (non-finite number)";

impl FieldRef {
    /// A field with a static name (usable in `const` items).
    pub const fn new(name: &'static str) -> Self {
        FieldRef(Cow::Borrowed(name))
    }

    /// The field name.
    pub fn name(&self) -> &str {
        &self.0
    }

    scalar_ops! {
        /// `field == value`; `eq(None::<i64>)` matches null.
        eq => Eq, "$eq";
        /// `field != value`; null fields differ from every non-null value.
        ne => Ne, "$ne";
        /// `field < value`.
        lt => Lt, "$lt";
        /// `field <= value`.
        lte => Lte, "$lte";
        /// `field > value`.
        gt => Gt, "$gt";
        /// `field >= value`.
        gte => Gte, "$gte";
    }

    /// `field` is one of `values`.
    pub fn is_in<I, V>(&self, values: I) -> Conditions
    where
        I: IntoIterator<Item = V>,
        V: IntoCondValue,
    {
        match collect_values(values) {
            Some(values) => Conditions::new().with(&self.0, Cond::In(values)),
            None => Conditions::invalid(&self.0, "$in", NON_FINITE),
        }
    }

    /// `field` is none of `values`.
    pub fn not_in<I, V>(&self, values: I) -> Conditions
    where
        I: IntoIterator<Item = V>,
        V: IntoCondValue,
    {
        match collect_values(values) {
            Some(values) => Conditions::new().with(&self.0, Cond::Nin(values)),
            None => Conditions::invalid(&self.0, "$nin", NON_FINITE),
        }
    }
}

fn collect_values<I, V>(values: I) -> Option<Vec<Value>>
where
    I: IntoIterator<Item = V>,
    V: IntoCondValue,
{
    values
        .into_iter()
        .map(IntoCondValue::into_cond_value)
        .collect()
}

/// A Rust value usable as a condition operand.
///
/// Implemented for booleans, integers, floats, strings, [`uuid::Uuid`]
/// (hyphenated lower case), [`chrono::DateTime<Utc>`](chrono::DateTime)
/// (RFC 3339 UTC with fixed microsecond precision, so text order is time
/// order), JSON values and `Option<T>` (`None` is `null`).
pub trait IntoCondValue {
    /// The JSON operand, or `None` when the value has no JSON form (a
    /// non-finite float); the rule is then rejected when it is built.
    fn into_cond_value(self) -> Option<Value>;
}

macro_rules! into_json {
    ($($ty:ty),*) => {
        $(
            impl IntoCondValue for $ty {
                fn into_cond_value(self) -> Option<Value> {
                    Some(Value::from(self))
                }
            }
        )*
    };
}

into_json!(
    bool, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, String, &str
);

impl IntoCondValue for &String {
    fn into_cond_value(self) -> Option<Value> {
        Some(Value::String(self.clone()))
    }
}

impl IntoCondValue for f64 {
    fn into_cond_value(self) -> Option<Value> {
        serde_json::Number::from_f64(self).map(Value::Number)
    }
}

impl IntoCondValue for f32 {
    fn into_cond_value(self) -> Option<Value> {
        f64::from(self).into_cond_value()
    }
}

impl IntoCondValue for Value {
    fn into_cond_value(self) -> Option<Value> {
        Some(self)
    }
}

impl IntoCondValue for uuid::Uuid {
    fn into_cond_value(self) -> Option<Value> {
        Some(Value::String(self.hyphenated().to_string()))
    }
}

impl IntoCondValue for chrono::DateTime<chrono::Utc> {
    fn into_cond_value(self) -> Option<Value> {
        Some(Value::String(
            self.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        ))
    }
}

impl<T: IntoCondValue> IntoCondValue for Option<T> {
    fn into_cond_value(self) -> Option<Value> {
        match self {
            Some(value) => value.into_cond_value(),
            None => Some(Value::Null),
        }
    }
}
