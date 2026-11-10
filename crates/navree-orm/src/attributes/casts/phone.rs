//! Phone numbers as field types (feature `phone`): navree-phone's
//! `PhoneNumber` and, with `validation` too, navree-validation's `Phone`,
//! stored as their E.164 text.

use std::borrow::Cow;

use navree_phone::PhoneNumber;

use crate::value::{Field, FieldError, FieldKind, Kind, Value};

/// Reads a stored number: E.164 exactly as written, whatever the plans
/// compiled in now say (a range withdrawn in an update can't make a row
/// unreadable); else any international form, valid or of a possible length.
fn parse(text: &str) -> Result<PhoneNumber, FieldError> {
    PhoneNumber::from_e164(text)
        .or_else(|_| PhoneNumber::parse(text, None))
        .or_else(|_| PhoneNumber::parse_possible(text, None))
        .map_err(|error| FieldError::Invalid(format!("`{text}` is not a phone number: {error}")))
}

/// A phone number, stored as its E.164 text (`+447400123456`). E.164 has no
/// extension: keep one in a column of its own if it matters. What was
/// written reads back exactly, even under newer numbering plans; text that
/// does not read as a number with its country code is a decode error
/// naming the column.
///
/// ```
/// use navree_orm::{Field, Value};
/// use navree_phone::PhoneNumber;
///
/// let number = PhoneNumber::parse("+44 7400 123456", None).unwrap();
/// assert_eq!(number.to_value(), Value::from("+447400123456"));
/// assert_eq!(PhoneNumber::from_value(Value::from("+447400123456")).unwrap(), number);
/// assert!(PhoneNumber::from_value(Value::from("07400 123456")).is_err());
///
/// // A number the plans compiled in don't assign still loads as stored.
/// let stored = PhoneNumber::from_value(Value::from("+441000000000")).unwrap();
/// assert_eq!(stored.to_value(), Value::from("+441000000000"));
/// ```
impl Field for PhoneNumber {
    const KIND: FieldKind = FieldKind::Text;

    fn to_value(&self) -> Value {
        Value::String(self.as_str().to_owned())
    }

    fn null() -> Value {
        Value::Null(Kind::String)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        String::from_value(value).and_then(|text| parse(&text))
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("PhoneNumber")
    }
}

/// A validated phone number from a request, stored as its E.164 text, like
/// [`PhoneNumber`].
///
/// ```
/// use navree_orm::{Field, Value};
/// use navree_validation::Phone;
///
/// let phone = Phone::parse("+1 650 253 0000", None).unwrap();
/// assert_eq!(phone.to_value(), Value::from("+16502530000"));
/// assert_eq!(Phone::from_value(Value::from("+16502530000")).unwrap(), phone);
/// ```
#[cfg(feature = "validation")]
impl Field for navree_validation::Phone {
    const KIND: FieldKind = FieldKind::Text;

    fn to_value(&self) -> Value {
        Value::String(self.as_str().to_owned())
    }

    fn null() -> Value {
        Value::Null(Kind::String)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        String::from_value(value)
            .and_then(|text| parse(&text))
            .map(navree_validation::Phone::from)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Phone")
    }
}
