//! Reading a row's cells into the import's row type with serde, and the
//! messages for cells that do not fit.
//!
//! Text cells are parsed into what each field asks for: numbers (with any
//! script's digits, and a decimal comma), booleans (`yes`, `no`, `1`, `0`,
//! `true`, `false`, `y`, `n`, `on`, `off`), dates, enums by name, lists
//! split on `,`, `;` or `|`. An empty cell is `None` for an `Option` and
//! "should not be empty" for anything else but text.

use std::fmt;

#[cfg(test)]
use garde::Validate;
use navree_validation::{MessageTranslator, Messages, fold_digits};
#[cfg(test)]
use navree_validation::{ValidationErrors, validate};
use serde::de::value::{SeqDeserializer, StringDeserializer};
use serde::de::{self, DeserializeOwned, DeserializeSeed, IntoDeserializer, MapAccess, Visitor};

use crate::Cell;

/// Why a cell did not fit its field: a validation rule name, with
/// parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    /// `required`: an empty cell (or a missing column) for a field that
    /// needs a value.
    Required,
    /// `integer`.
    Integer,
    /// `numeric`.
    Number,
    /// `boolean`.
    Boolean,
    /// `date`.
    Date,
    /// `uuid`.
    Uuid,
    /// `in`, with the allowed values.
    OneOf(String),
    /// `invalid`.
    Invalid,
}

impl Rule {
    fn key(&self) -> &'static str {
        match self {
            Rule::Required => "required",
            Rule::Integer => "integer",
            Rule::Number => "numeric",
            Rule::Boolean => "boolean",
            Rule::Date => "date",
            Rule::Uuid => "uuid",
            Rule::OneOf(_) => "in",
            Rule::Invalid => "invalid",
        }
    }
}

/// A cell that did not fit, and the field it was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReadError {
    pub(crate) field: Option<String>,
    pub(crate) rule: Rule,
    message: String,
}

impl ReadError {
    fn rule(rule: Rule) -> Self {
        ReadError {
            field: None,
            message: rule.key().to_owned(),
            rule,
        }
    }

    /// The field and its message, in the app's validation wording.
    #[cfg(test)]
    pub(crate) fn render(&self, messages: &Messages) -> (String, String) {
        self.render_with(messages, None)
    }

    /// The field and its message, through the app's message translator
    /// when it has one (navree-i18n's), else its message table.
    pub(crate) fn render_with(
        &self,
        messages: &Messages,
        translator: Option<&dyn MessageTranslator>,
    ) -> (String, String) {
        let field = self.field.clone().unwrap_or_else(|| "row".to_owned());
        let params: Vec<(&str, String)> = match &self.rule {
            Rule::OneOf(values) => vec![("values", values.clone())],
            _ => Vec::new(),
        };
        let message = match translator {
            Some(translator) => translator.message(messages, &field, self.rule.key(), &params),
            None => messages.render(&field, self.rule.key(), &params),
        };
        (field, message)
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(f, "{field}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ReadError {}

impl de::Error for ReadError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        let message = msg.to_string();
        ReadError {
            field: None,
            rule: classify(&message),
            message,
        }
    }

    fn missing_field(field: &'static str) -> Self {
        ReadError {
            field: Some(field.to_owned()),
            rule: Rule::Required,
            message: format!("missing field `{field}`"),
        }
    }

    fn unknown_variant(variant: &str, expected: &'static [&'static str]) -> Self {
        ReadError {
            field: None,
            rule: Rule::OneOf(expected.join(", ")),
            message: format!("unknown variant `{variant}`"),
        }
    }

    fn invalid_type(unexp: de::Unexpected<'_>, exp: &dyn de::Expected) -> Self {
        let expected = exp.to_string();
        ReadError {
            field: None,
            rule: expected_rule(&expected),
            message: format!("invalid type: {unexp}, expected {expected}"),
        }
    }

    fn invalid_value(unexp: de::Unexpected<'_>, exp: &dyn de::Expected) -> Self {
        let expected = exp.to_string();
        ReadError {
            field: None,
            rule: expected_rule(&expected),
            message: format!("invalid value: {unexp}, expected {expected}"),
        }
    }
}

/// The rule of an error another type's `Deserialize` raised.
fn classify(message: &str) -> Rule {
    const DATE_ERRORS: &[&str] = &[
        "input contains invalid characters",
        "premature end of input",
        "trailing input",
        "input is out of range",
        "no possible date and time matching input",
        "input is not enough for unique date and time",
    ];
    if message.contains("UUID parsing failed") || message.starts_with("invalid character: expected an optional prefix of `urn:uuid:`") {
        Rule::Uuid
    } else if DATE_ERRORS.iter().any(|e| message.contains(e)) {
        Rule::Date
    } else {
        Rule::Invalid
    }
}

/// The rule for "expected ...".
fn expected_rule(expected: &str) -> Rule {
    let expected = expected.to_ascii_lowercase();
    if expected.contains("integer") || expected.starts_with("u8") || expected.starts_with("i") && expected.len() <= 4 {
        Rule::Integer
    } else if expected.contains("float") || expected.contains("number") {
        Rule::Number
    } else if expected.contains("bool") {
        Rule::Boolean
    } else if expected.contains("date") || expected.contains("time") {
        Rule::Date
    } else if expected.contains("uuid") {
        Rule::Uuid
    } else {
        Rule::Invalid
    }
}

/// Reads `fields` (field name and cell) into `R`.
pub(crate) fn read_row<R: DeserializeOwned>(fields: Vec<(String, Cell)>) -> Result<R, ReadError> {
    R::deserialize(RowDeserializer { fields })
}

/// Reads `fields` into `R` and runs its garde rules; the errors are keyed by
/// field, in the app's validation wording.
#[cfg(test)]
pub(crate) fn check_row<R>(fields: Vec<(String, Cell)>, messages: &Messages) -> Result<R, ValidationErrors>
where
    R: DeserializeOwned + Validate<Context = ()>,
{
    let row: R = read_row(fields).map_err(|error| {
        let mut errors = ValidationErrors::new();
        let (field, message) = error.render(messages);
        errors.add(field, message);
        errors
    })?;
    validate(&row, None, messages, "row")?;
    Ok(row)
}

struct RowDeserializer {
    fields: Vec<(String, Cell)>,
}

impl<'de> de::Deserializer<'de> for RowDeserializer {
    type Error = ReadError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_map(RowAccess {
            fields: self.fields.into_iter(),
            current: None,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct RowAccess {
    fields: std::vec::IntoIter<(String, Cell)>,
    current: Option<(String, Cell)>,
}

impl<'de> MapAccess<'de> for RowAccess {
    type Error = ReadError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, ReadError> {
        match self.fields.next() {
            Some((field, cell)) => {
                let key = seed.deserialize(StringDeserializer::<ReadError>::new(field.clone()))?;
                self.current = Some((field, cell));
                Ok(Some(key))
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, ReadError> {
        let (field, cell) = self
            .current
            .take()
            .ok_or_else(|| ReadError::rule(Rule::Invalid))?;
        seed.deserialize(CellDeserializer(cell)).map_err(|mut error| {
            if error.field.is_none() {
                error.field = Some(field);
            }
            error
        })
    }
}

/// One cell, read as whatever the field asks for.
struct CellDeserializer(Cell);

impl CellDeserializer {
    /// The cell as a whole number.
    fn integer(&self) -> Result<i128, ReadError> {
        match &self.0 {
            Cell::Int(n) => Ok(i128::from(*n)),
            Cell::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e38 => Ok(*f as i128),
            Cell::Text(text) => {
                let folded = fold_digits(text.trim());
                let digits = folded.strip_prefix('+').unwrap_or(&folded);
                digits.parse::<i128>().or_else(|_| {
                    // `12.0` from a spreadsheet's number formatting.
                    digits
                        .parse::<f64>()
                        .ok()
                        .filter(|f| f.is_finite() && f.fract() == 0.0 && f.abs() < 1e38)
                        .map(|f| f as i128)
                        .ok_or_else(|| ReadError::rule(Rule::Integer))
                })
            }
            Cell::Empty => Err(ReadError::rule(Rule::Required)),
            _ => Err(ReadError::rule(Rule::Integer)),
        }
    }

    /// The cell as a number.
    fn number(&self) -> Result<f64, ReadError> {
        match &self.0 {
            Cell::Int(n) => Ok(*n as f64),
            Cell::Float(f) => Ok(*f),
            Cell::Text(text) => {
                let folded = fold_digits(text.trim());
                let folded = folded.strip_prefix('+').unwrap_or(&folded);
                // A decimal comma (`3,5`) when there is no point.
                let normalized = if !folded.contains('.') && folded.matches(',').count() == 1 {
                    folded.replace(',', ".")
                } else {
                    folded.to_owned()
                };
                normalized
                    .parse::<f64>()
                    .ok()
                    .filter(|f| f.is_finite())
                    .ok_or_else(|| ReadError::rule(Rule::Number))
            }
            Cell::Empty => Err(ReadError::rule(Rule::Required)),
            _ => Err(ReadError::rule(Rule::Number)),
        }
    }

    /// The cell as a boolean.
    fn boolean(&self) -> Result<bool, ReadError> {
        match &self.0 {
            Cell::Bool(b) => Ok(*b),
            Cell::Int(0) => Ok(false),
            Cell::Int(1) => Ok(true),
            Cell::Float(f) if *f == 0.0 => Ok(false),
            Cell::Float(f) if *f == 1.0 => Ok(true),
            Cell::Text(text) => match fold_digits(text.trim()).to_lowercase().as_str() {
                "true" | "yes" | "y" | "1" | "on" | "t" => Ok(true),
                "false" | "no" | "n" | "0" | "off" | "f" => Ok(false),
                _ => Err(ReadError::rule(Rule::Boolean)),
            },
            Cell::Empty => Err(ReadError::rule(Rule::Required)),
            _ => Err(ReadError::rule(Rule::Boolean)),
        }
    }

    /// The cell as text (dates in ISO 8601).
    fn text(self) -> String {
        match self.0 {
            Cell::Text(text) => text,
            Cell::DateTime(at) => at.format("%Y-%m-%dT%H:%M:%S").to_string(),
            other => other.to_text().into_owned(),
        }
    }
}

macro_rules! integers {
    ($($method:ident => $visit:ident as $t:ty),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
            let n = self.integer()?;
            let n = <$t>::try_from(n).map_err(|_| ReadError::rule(Rule::Integer))?;
            visitor.$visit(n)
        }
    )*};
}

impl<'de> de::Deserializer<'de> for CellDeserializer {
    type Error = ReadError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        match self.0 {
            Cell::Empty => visitor.visit_none(),
            Cell::Text(text) => visitor.visit_string(text),
            Cell::Int(n) => visitor.visit_i64(n),
            Cell::Float(f) => visitor.visit_f64(f),
            Cell::Bool(b) => visitor.visit_bool(b),
            other @ (Cell::Date(_) | Cell::DateTime(_)) => {
                visitor.visit_string(CellDeserializer(other).text())
            }
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_bool(self.boolean()?)
    }

    integers! {
        deserialize_i8 => visit_i8 as i8,
        deserialize_i16 => visit_i16 as i16,
        deserialize_i32 => visit_i32 as i32,
        deserialize_i64 => visit_i64 as i64,
        deserialize_i128 => visit_i128 as i128,
        deserialize_u8 => visit_u8 as u8,
        deserialize_u16 => visit_u16 as u16,
        deserialize_u32 => visit_u32 as u32,
        deserialize_u64 => visit_u64 as u64,
        deserialize_u128 => visit_u128 as u128,
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_f32(self.number()? as f32)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_f64(self.number()?)
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        let text = self.text();
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => visitor.visit_char(c),
            _ => Err(ReadError::rule(Rule::Invalid)),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_string(self.text())
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_string(self.text())
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_byte_buf(self.text().into_bytes())
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_byte_buf(self.text().into_bytes())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        if self.0.is_empty() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ReadError> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ReadError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        let items: Vec<CellDeserializer> = match self.0 {
            Cell::Empty => Vec::new(),
            Cell::Text(text) => text
                .split([',', ';', '|', '\n'])
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| CellDeserializer(Cell::Text(item.to_owned())))
                .collect(),
            other => vec![CellDeserializer(other)],
        };
        visitor.visit_seq(SeqDeserializer::new(items.into_iter()))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, ReadError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ReadError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, ReadError> {
        Err(ReadError::rule(Rule::Invalid))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, ReadError> {
        Err(ReadError::rule(Rule::Invalid))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ReadError> {
        if self.0.is_empty() {
            return Err(ReadError::rule(Rule::Required));
        }
        let text = self.text();
        // A variant matches whatever its case in the file.
        let name = variants
            .iter()
            .find(|v| v.eq_ignore_ascii_case(&text))
            .map_or(text, |v| (*v).to_owned());
        visitor.visit_enum(name.into_deserializer())
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_string(self.text())
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, ReadError> {
        visitor.visit_unit()
    }
}

impl<'de> IntoDeserializer<'de, ReadError> for CellDeserializer {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde::Deserialize;
    use uuid::Uuid;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum Status {
        Lead,
        Customer,
    }

    #[derive(Debug, Deserialize, PartialEq, Validate)]
    struct Row {
        #[garde(email)]
        email: String,
        #[garde(skip)]
        age: Option<u8>,
        #[garde(skip)]
        score: f64,
        #[garde(skip)]
        active: bool,
        #[garde(skip)]
        joined: Option<NaiveDate>,
        #[garde(skip)]
        status: Status,
        #[garde(skip)]
        #[serde(default)]
        tags: Vec<String>,
        #[garde(skip)]
        id: Option<Uuid>,
    }

    fn fields(cells: &[(&str, Cell)]) -> Vec<(String, Cell)> {
        cells
            .iter()
            .map(|(f, c)| ((*f).to_owned(), c.clone()))
            .collect()
    }

    fn text(s: &str) -> Cell {
        Cell::Text(s.into())
    }

    fn base() -> Vec<(&'static str, Cell)> {
        vec![
            ("email", text("ada@example.com")),
            ("age", text("٣٦")),
            ("score", text("3,5")),
            ("active", text("Yes")),
            ("joined", Cell::Date(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap())),
            ("status", text("Customer")),
            ("tags", text("vip; new |  ")),
            ("id", Cell::Empty),
        ]
    }

    #[test]
    fn text_is_read_as_each_field_asks() {
        let row: Row = read_row(fields(&base())).unwrap();
        assert_eq!(
            row,
            Row {
                email: "ada@example.com".into(),
                age: Some(36),
                score: 3.5,
                active: true,
                joined: NaiveDate::from_ymd_opt(2026, 1, 2),
                status: Status::Customer,
                tags: vec!["vip".into(), "new".into()],
                id: None,
            }
        );
    }

    #[test]
    fn workbook_numbers_fill_integer_and_text_fields() {
        let mut cells = base();
        cells[1].1 = Cell::Float(36.0);
        cells[0].1 = Cell::Float(5551234.0);
        let row: Result<Row, _> = read_row(fields(&cells));
        // The email is text "5551234", which garde rejects later, not serde.
        assert_eq!(row.unwrap().email, "5551234");
    }

    fn error_for(field: &str, cell: Cell) -> (String, String) {
        let mut cells = base();
        match cells.iter_mut().find(|(f, _)| *f == field) {
            Some(entry) => entry.1 = cell,
            None => unreachable!(),
        }
        let error = read_row::<Row>(fields(&cells)).unwrap_err();
        error.render(&Messages::default())
    }

    #[test]
    fn cells_that_do_not_fit_name_their_field() {
        assert_eq!(
            error_for("age", text("old")),
            ("age".into(), "age must be an integer number".into())
        );
        assert_eq!(
            error_for("age", text("300")),
            ("age".into(), "age must be an integer number".into())
        );
        assert_eq!(
            error_for("score", text("high")),
            ("score".into(), "score must be a number".into())
        );
        assert_eq!(
            error_for("active", text("maybe")),
            ("active".into(), "active must be a boolean value".into())
        );
        assert_eq!(
            error_for("active", Cell::Empty),
            ("active".into(), "active should not be empty".into())
        );
        assert_eq!(
            error_for("joined", text("02/01/2026")),
            (
                "joined".into(),
                "joined must be a valid ISO 8601 date string".into()
            )
        );
        assert_eq!(
            error_for("status", text("partner")),
            (
                "status".into(),
                "status must be one of the following values: lead, customer".into()
            )
        );
        assert_eq!(
            error_for("id", text("nope")),
            ("id".into(), "id must be a UUID".into())
        );
    }

    #[test]
    fn a_missing_column_is_required() {
        let cells: Vec<_> = base().into_iter().filter(|(f, _)| *f != "score").collect();
        let error = read_row::<Row>(fields(&cells)).unwrap_err();
        assert_eq!(
            error.render(&Messages::default()),
            ("score".into(), "score should not be empty".into())
        );
    }

    #[test]
    fn garde_rules_run_after_reading() {
        let mut cells = base();
        cells[0].1 = text("not-an-email");
        let errors = check_row::<Row>(fields(&cells), &Messages::default()).unwrap_err();
        assert_eq!(
            errors.get("email"),
            Some(&["email must be a valid email address".to_owned()][..])
        );
        assert!(check_row::<Row>(fields(&base()), &Messages::default()).is_ok());
    }
}
