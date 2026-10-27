//! [`Cell`], one spreadsheet value, and [`IntoCell`], what turns a field
//! into one.

use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde_json::Value;
use uuid::Uuid;

/// The largest integer a spreadsheet keeps exactly (2^53): larger ones are
/// written as text so no digit is lost.
pub(crate) const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_992;

/// One spreadsheet value: what an export writes and what an import reads.
///
/// - CSV writes text as text (escaped against formulas, see
///   [`CsvOptions::escape_formulas`](crate::CsvOptions::escape_formulas)),
///   numbers and booleans as they print, dates as `2026-10-27` and date-times
///   as `2026-10-27 14:30:00`.
/// - XLSX writes numbers as numbers, booleans as booleans, dates and
///   date-times as Excel dates with a date format, and text as text (never
///   as a formula).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Cell {
    /// Nothing.
    #[default]
    Empty,
    /// Text.
    Text(String),
    /// A whole number.
    Int(i64),
    /// A number with a fraction.
    Float(f64),
    /// `true` or `false`.
    Bool(bool),
    /// A calendar date.
    Date(NaiveDate),
    /// A date and time (UTC for values that had a zone).
    DateTime(NaiveDateTime),
}

impl Cell {
    /// Whether the cell holds nothing (or only whitespace).
    pub fn is_empty(&self) -> bool {
        match self {
            Cell::Empty => true,
            Cell::Text(text) => text.trim().is_empty(),
            _ => false,
        }
    }

    /// The cell as text, the way CSV writes it (without formula escaping).
    pub fn to_text(&self) -> Cow<'_, str> {
        match self {
            Cell::Empty => Cow::Borrowed(""),
            Cell::Text(text) => Cow::Borrowed(text),
            Cell::Int(n) => Cow::Owned(n.to_string()),
            Cell::Float(n) => Cow::Owned(float_text(*n)),
            Cell::Bool(b) => Cow::Borrowed(if *b { "true" } else { "false" }),
            Cell::Date(date) => Cow::Owned(date.format("%Y-%m-%d").to_string()),
            Cell::DateTime(at) => Cow::Owned(datetime_text(at)),
        }
    }

    /// The cell as JSON: text as a string, numbers and booleans as
    /// themselves, dates as ISO 8601 strings, nothing as `null`.
    pub fn to_json(&self) -> Value {
        match self {
            Cell::Empty => Value::Null,
            Cell::Text(text) => Value::String(text.clone()),
            Cell::Int(n) => Value::from(*n),
            Cell::Float(n) => serde_json::Number::from_f64(*n)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(float_text(*n))),
            Cell::Bool(b) => Value::Bool(*b),
            Cell::Date(date) => Value::String(date.format("%Y-%m-%d").to_string()),
            Cell::DateTime(at) => Value::String(at.format("%Y-%m-%dT%H:%M:%S").to_string()),
        }
    }

    /// The cell [`to_json`](Cell::to_json) wrote (dates come back as text).
    pub fn from_json(value: &Value) -> Cell {
        match value {
            Value::Null => Cell::Empty,
            Value::Bool(b) => Cell::Bool(*b),
            Value::Number(n) => match n.as_i64() {
                Some(i) => Cell::Int(i),
                None => n.as_f64().map_or(Cell::Empty, Cell::Float),
            },
            Value::String(text) => Cell::Text(text.clone()),
            other => Cell::Text(other.to_string()),
        }
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

/// A float the way people write it: no trailing `.0` on whole numbers.
pub(crate) fn float_text(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < MAX_SAFE_INTEGER as f64 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// `2026-10-27 14:30:00`, with the fraction of a second when there is one.
fn datetime_text(at: &NaiveDateTime) -> String {
    if at.and_utc().timestamp_subsec_nanos() == 0 {
        at.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        at.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
    }
}

/// Turns a value into a [`Cell`]: what `#[derive(Exportable)]` calls on
/// every field. Implemented for text, numbers, booleans, `chrono` dates and
/// times, `Uuid`, `serde_json::Value`, `Option<T>` (`None` is empty) and
/// references; implement it for your own types, or point a field at a
/// function with `#[export(with = "path")]`.
pub trait IntoCell {
    /// The cell for this value.
    fn to_cell(&self) -> Cell;
}

impl IntoCell for Cell {
    fn to_cell(&self) -> Cell {
        self.clone()
    }
}

impl<T: IntoCell + ?Sized> IntoCell for &T {
    fn to_cell(&self) -> Cell {
        (**self).to_cell()
    }
}

impl<T: IntoCell> IntoCell for Option<T> {
    fn to_cell(&self) -> Cell {
        match self {
            Some(value) => value.to_cell(),
            None => Cell::Empty,
        }
    }
}

impl<T: IntoCell + ?Sized> IntoCell for Box<T> {
    fn to_cell(&self) -> Cell {
        (**self).to_cell()
    }
}

impl<T: IntoCell + ?Sized> IntoCell for Arc<T> {
    fn to_cell(&self) -> Cell {
        (**self).to_cell()
    }
}

impl<T: IntoCell + ?Sized> IntoCell for Rc<T> {
    fn to_cell(&self) -> Cell {
        (**self).to_cell()
    }
}

impl IntoCell for str {
    fn to_cell(&self) -> Cell {
        Cell::Text(self.to_owned())
    }
}

impl IntoCell for String {
    fn to_cell(&self) -> Cell {
        Cell::Text(self.clone())
    }
}

impl IntoCell for Cow<'_, str> {
    fn to_cell(&self) -> Cell {
        Cell::Text(self.to_string())
    }
}

impl IntoCell for char {
    fn to_cell(&self) -> Cell {
        Cell::Text(self.to_string())
    }
}

impl IntoCell for bool {
    fn to_cell(&self) -> Cell {
        Cell::Bool(*self)
    }
}

macro_rules! small_ints {
    ($($t:ty),*) => {$(
        impl IntoCell for $t {
            fn to_cell(&self) -> Cell {
                Cell::Int(i64::from(*self))
            }
        }
    )*};
}

small_ints!(i8, i16, i32, i64, u8, u16, u32);

macro_rules! large_ints {
    ($($t:ty),*) => {$(
        impl IntoCell for $t {
            fn to_cell(&self) -> Cell {
                match i64::try_from(*self) {
                    Ok(n) => Cell::Int(n),
                    Err(_) => Cell::Text(self.to_string()),
                }
            }
        }
    )*};
}

large_ints!(u64, i128, u128, isize, usize);

impl IntoCell for f32 {
    fn to_cell(&self) -> Cell {
        Cell::Float(f64::from(*self))
    }
}

impl IntoCell for f64 {
    fn to_cell(&self) -> Cell {
        Cell::Float(*self)
    }
}

impl IntoCell for NaiveDate {
    fn to_cell(&self) -> Cell {
        Cell::Date(*self)
    }
}

impl IntoCell for NaiveDateTime {
    fn to_cell(&self) -> Cell {
        Cell::DateTime(*self)
    }
}

impl IntoCell for NaiveTime {
    fn to_cell(&self) -> Cell {
        Cell::Text(self.format("%H:%M:%S").to_string())
    }
}

impl<Tz: TimeZone> IntoCell for DateTime<Tz> {
    fn to_cell(&self) -> Cell {
        Cell::DateTime(self.naive_utc())
    }
}

impl IntoCell for Uuid {
    fn to_cell(&self) -> Cell {
        Cell::Text(self.to_string())
    }
}

impl IntoCell for Value {
    fn to_cell(&self) -> Cell {
        match self {
            Value::Null => Cell::Empty,
            Value::Bool(b) => Cell::Bool(*b),
            Value::Number(n) => match n.as_i64() {
                Some(i) => Cell::Int(i),
                None => n.as_f64().map_or(Cell::Empty, Cell::Float),
            },
            Value::String(text) => Cell::Text(text.clone()),
            other => Cell::Text(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn values_become_cells() {
        assert_eq!("Ada".to_cell(), Cell::Text("Ada".into()));
        assert_eq!(String::from("x").to_cell(), Cell::Text("x".into()));
        assert_eq!(42u32.to_cell(), Cell::Int(42));
        assert_eq!((-7i64).to_cell(), Cell::Int(-7));
        assert_eq!(u64::MAX.to_cell(), Cell::Text(u64::MAX.to_string()));
        assert_eq!(1.5f64.to_cell(), Cell::Float(1.5));
        assert_eq!(true.to_cell(), Cell::Bool(true));
        assert_eq!(None::<String>.to_cell(), Cell::Empty);
        assert_eq!(Some(3u8).to_cell(), Cell::Int(3));
        let date = NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();
        assert_eq!(date.to_cell(), Cell::Date(date));
        let at = Utc.with_ymd_and_hms(2026, 10, 27, 14, 30, 0).unwrap();
        assert_eq!(at.to_cell(), Cell::DateTime(at.naive_utc()));
        assert_eq!(json!(5).to_cell(), Cell::Int(5));
        assert_eq!(json!(null).to_cell(), Cell::Empty);
        assert_eq!(json!(["a"]).to_cell(), Cell::Text("[\"a\"]".into()));
    }

    #[test]
    fn cells_print_like_people_write_them() {
        assert_eq!(Cell::Float(3.0).to_text(), "3");
        assert_eq!(Cell::Float(2.25).to_text(), "2.25");
        assert_eq!(Cell::Bool(false).to_text(), "false");
        let at = NaiveDate::from_ymd_opt(2026, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 5)
            .unwrap();
        assert_eq!(Cell::DateTime(at).to_text(), "2026-01-02 03:04:05");
        assert_eq!(Cell::Date(at.date()).to_text(), "2026-01-02");
        assert_eq!(Cell::Empty.to_text(), "");
    }

    #[test]
    fn json_round_trips_values() {
        for cell in [
            Cell::Empty,
            Cell::Text("x".into()),
            Cell::Int(9),
            Cell::Float(1.25),
            Cell::Bool(true),
        ] {
            assert_eq!(Cell::from_json(&cell.to_json()), cell);
        }
        let date = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        assert_eq!(
            Cell::from_json(&Cell::Date(date).to_json()),
            Cell::Text("2026-01-02".into())
        );
    }

    #[test]
    fn whitespace_only_text_is_empty() {
        assert!(Cell::Text("  ".into()).is_empty());
        assert!(!Cell::Int(0).is_empty());
    }
}
