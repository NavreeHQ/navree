//! `url::Url` as a field type: URLs stored as text.

use std::borrow::Cow;

use sea_orm::QueryResult;
use url::Url;

use crate::value::{Field, FieldError, FieldKind, Value, sql_get};

fn parse(text: &str) -> Result<Url, FieldError> {
    Url::parse(text.trim()).map_err(|error| FieldError::Invalid(format!("`{text}` is not a URL: {error}")))
}

/// A URL, stored as its text; text that does not parse is a decode error
/// naming the column.
///
/// ```
/// use navree_orm::{Field, Value};
/// use url::Url;
///
/// let url = Url::parse("https://navree.com/docs?page=1").unwrap();
/// assert_eq!(url.to_value(), Value::from("https://navree.com/docs?page=1"));
/// assert_eq!(Url::from_value(Value::from("https://navree.com/docs?page=1")).unwrap(), url);
/// assert!(Url::from_value(Value::from("not a url")).is_err());
/// ```
impl Field for Url {
    const KIND: FieldKind = FieldKind::Url;

    fn to_value(&self) -> Value {
        Value::String(Some(self.as_str().to_owned()))
    }

    fn null() -> Value {
        Value::String(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        sql_get::<String>(row, column).and_then(|text| parse(&text))
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        String::from_value(value).and_then(|text| parse(&text))
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Url")
    }
}
