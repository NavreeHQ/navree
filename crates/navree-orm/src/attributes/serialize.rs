//! Serialized records: what `serde_json::to_value(&record)`,
//! [`Record::to_json`] and HTTP responses write.
//!
//! A record serializes as:
//!
//! 1. its model, as the model's own `Serialize` writes it (so serde
//!    attributes keep working: `rename`, `skip_serializing_if`,
//!    `serialize_with`), with each field under serde's key;
//! 2. where a cast decides a field's output
//!    ([`Cast::serialize`](crate::cast::Cast::serialize)), the cast's value;
//! 3. date-times in one format: RFC 3339 in UTC with microseconds
//!    (`2026-10-27T08:30:00.000000Z`), or the model's
//!    `#[model(serialize_dates_with = path)]`;
//! 4. the extra columns the query selected (aggregates, counts);
//! 5. the appended accessors (`#[model(appends(..))]`, `record.append(..)`);
//! 6. the loaded relations, each related record applying its own
//!    visibility (`null` for an empty to-one, `[]` for an empty to-many);
//!
//! keeping only the `visible` names when there are any, and leaving out
//! the `hidden` ones. Names in those lists are field or column names
//! (matched to serde's keys) or output keys (accessors, extras, relations).

use std::borrow::Cow;
use std::fmt;

use chrono::{DateTime, FixedOffset, NaiveDateTime, SecondsFormat};
use serde::ser::{self, Serialize, SerializeMap, Serializer};

use crate::error::{OrmError, Result};
use crate::model::{DateSerializer, Model, ModelMeta};
use crate::record::Record;
use crate::value::{Value, to_json};

/// A serialized record: keys and values, in order.
pub(crate) type Object = Vec<(String, serde_json::Value)>;

fn serialize_error<M: Model>(column: Option<&str>, message: impl Into<String>) -> OrmError {
    OrmError::Serialize {
        model: Cow::Borrowed(M::meta().name()),
        column: column.map(str::to_owned),
        message: message.into(),
    }
}

// -------------------------------------------------------------------------
// Capturing serde's output in order
// -------------------------------------------------------------------------

/// Why capturing a model's fields failed.
#[derive(Debug)]
struct CaptureError {
    column: Option<String>,
    message: String,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CaptureError {}

impl ser::Error for CaptureError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        CaptureError {
            column: None,
            message: message.to_string(),
        }
    }
}

/// A serializer that keeps a struct's (or map's) entries, in order.
struct Capture;

/// The entries of the struct being captured.
struct Entries {
    entries: Object,
    key: Option<String>,
}

fn field_value<T: Serialize + ?Sized>(key: &str, value: &T) -> std::result::Result<serde_json::Value, CaptureError> {
    serde_json::to_value(value).map_err(|error| CaptureError {
        column: Some(key.to_owned()),
        message: error.to_string(),
    })
}

impl ser::SerializeStruct for Entries {
    type Ok = Object;
    type Error = CaptureError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> std::result::Result<(), CaptureError> {
        let value = field_value(key, value)?;
        self.entries.push((key.to_owned(), value));
        Ok(())
    }

    fn end(self) -> std::result::Result<Object, CaptureError> {
        Ok(self.entries)
    }
}

impl ser::SerializeMap for Entries {
    type Ok = Object;
    type Error = CaptureError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> std::result::Result<(), CaptureError> {
        match serde_json::to_value(key) {
            Ok(serde_json::Value::String(key)) => {
                self.key = Some(key);
                Ok(())
            }
            Ok(other) => Err(CaptureError {
                column: None,
                message: format!("a serialized model's keys must be strings, found {other}"),
            }),
            Err(error) => Err(CaptureError {
                column: None,
                message: error.to_string(),
            }),
        }
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> std::result::Result<(), CaptureError> {
        let key = self.key.take().unwrap_or_default();
        let value = field_value(&key, value)?;
        self.entries.push((key, value));
        Ok(())
    }

    fn end(self) -> std::result::Result<Object, CaptureError> {
        Ok(self.entries)
    }
}

fn not_an_object(kind: &str) -> CaptureError {
    CaptureError {
        column: None,
        message: format!(
            "it serializes as {kind}, not an object (a model is serialized field by field: drop #[serde(transparent)] or the custom Serialize)"
        ),
    }
}

macro_rules! not_objects {
    ($($method:ident($($arg:ty),*) => $kind:literal;)*) => {$(
        fn $method(self, $(_: $arg),*) -> std::result::Result<Object, CaptureError> {
            Err(not_an_object($kind))
        }
    )*};
}

impl Serializer for Capture {
    type Ok = Object;
    type Error = CaptureError;
    type SerializeSeq = ser::Impossible<Object, CaptureError>;
    type SerializeTuple = ser::Impossible<Object, CaptureError>;
    type SerializeTupleStruct = ser::Impossible<Object, CaptureError>;
    type SerializeTupleVariant = ser::Impossible<Object, CaptureError>;
    type SerializeMap = Entries;
    type SerializeStruct = Entries;
    type SerializeStructVariant = ser::Impossible<Object, CaptureError>;

    not_objects! {
        serialize_bool(bool) => "a boolean";
        serialize_i8(i8) => "a number";
        serialize_i16(i16) => "a number";
        serialize_i32(i32) => "a number";
        serialize_i64(i64) => "a number";
        serialize_u8(u8) => "a number";
        serialize_u16(u16) => "a number";
        serialize_u32(u32) => "a number";
        serialize_u64(u64) => "a number";
        serialize_f32(f32) => "a number";
        serialize_f64(f64) => "a number";
        serialize_char(char) => "a string";
        serialize_str(&str) => "a string";
        serialize_bytes(&[u8]) => "bytes";
        serialize_none() => "null";
        serialize_unit() => "null";
        serialize_unit_struct(&'static str) => "null";
        serialize_unit_variant(&'static str, u32, &'static str) => "a string";
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> std::result::Result<Object, CaptureError> {
        value.serialize(self)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> std::result::Result<Object, CaptureError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> std::result::Result<Object, CaptureError> {
        Err(not_an_object("an enum variant"))
    }

    fn serialize_seq(self, _len: Option<usize>) -> std::result::Result<Self::SerializeSeq, CaptureError> {
        Err(not_an_object("an array"))
    }

    fn serialize_tuple(self, _len: usize) -> std::result::Result<Self::SerializeTuple, CaptureError> {
        Err(not_an_object("an array"))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> std::result::Result<Self::SerializeTupleStruct, CaptureError> {
        Err(not_an_object("an array"))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> std::result::Result<Self::SerializeTupleVariant, CaptureError> {
        Err(not_an_object("an enum variant"))
    }

    fn serialize_map(self, len: Option<usize>) -> std::result::Result<Entries, CaptureError> {
        Ok(Entries {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> std::result::Result<Entries, CaptureError> {
        Ok(Entries {
            entries: Vec::with_capacity(len),
            key: None,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> std::result::Result<Self::SerializeStructVariant, CaptureError> {
        Err(not_an_object("an enum variant"))
    }
}

// -------------------------------------------------------------------------
// Dates
// -------------------------------------------------------------------------

/// The default date-time output: RFC 3339, UTC, microseconds.
pub(crate) fn default_date(at: &DateTime<FixedOffset>) -> serde_json::Value {
    serde_json::Value::String(at.to_utc().to_rfc3339_opts(SecondsFormat::Micros, true))
}

/// A date-time written by serde (RFC 3339, or a naive `T`-separated
/// date-time read as UTC), parsed.
fn parse_serialized(text: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(text).ok().or_else(|| {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
            .ok()
            .map(|at| at.and_utc().fixed_offset())
    })
}

/// A date-time value of an extra column, if it is one.
fn date_time_value(value: &Value) -> Option<DateTime<FixedOffset>> {
    match value {
        Value::ChronoDateTimeUtc(Some(at)) => Some(at.fixed_offset()),
        Value::ChronoDateTimeWithTimeZone(Some(at)) => Some(*at),
        Value::ChronoDateTimeLocal(Some(at)) => Some(at.fixed_offset()),
        Value::ChronoDateTime(Some(at)) => Some(at.and_utc().fixed_offset()),
        _ => None,
    }
}

fn write_date(at: &DateTime<FixedOffset>, serializer: Option<DateSerializer>) -> serde_json::Value {
    match serializer {
        Some(serializer) => serializer(at),
        None => default_date(at),
    }
}

// -------------------------------------------------------------------------
// Names
// -------------------------------------------------------------------------

/// The output key of a visibility or appends name: a field's or column's
/// serialized key, or the name itself.
pub(crate) fn output_key(meta: &ModelMeta, name: &str) -> Option<String> {
    match meta.column_named(name) {
        Some(column) => column.serialized_name().map(str::to_owned),
        None => Some(name.to_owned()),
    }
}

fn output_keys(meta: &ModelMeta, names: &[Cow<'static, str>]) -> Vec<String> {
    names
        .iter()
        .filter_map(|name| output_key(meta, name))
        .collect()
}

// -------------------------------------------------------------------------
// Records
// -------------------------------------------------------------------------

/// The record's attributes before visibility applies: the model's fields
/// (with cast overrides and date formats), the extras and the appended
/// accessors, with the cast context the record was read or saved under.
pub(crate) fn attributes_object<M: Model>(record: &Record<M>) -> Result<Object> {
    record.in_cast_context(|| collect_attributes(record))
}

fn collect_attributes<M: Model>(record: &Record<M>) -> Result<Object> {
    let meta = M::meta();
    let mut object = record.model.serialize(Capture).map_err(|error| {
        serialize_error::<M>(error.column.as_deref(), error.message)
    })?;

    let overrides = record.model.serialized_overrides()?;
    for (key, json) in &overrides {
        if let Some(entry) = object.iter_mut().find(|(existing, _)| existing == key) {
            entry.1 = json.clone();
        }
    }
    let dates = meta.date_serializer();
    for column in meta.all_columns() {
        let Some(key) = column.serialized_name() else {
            continue;
        };
        if !column.is_date_time() || overrides.iter().any(|(overridden, _)| *overridden == key) {
            continue;
        }
        if let Some(entry) = object.iter_mut().find(|(existing, _)| existing == key)
            && let Some(at) = entry.1.as_str().and_then(parse_serialized)
        {
            entry.1 = write_date(&at, dates);
        }
    }

    for (name, value) in &record.extras {
        let json = match date_time_value(value) {
            Some(at) => write_date(&at, dates),
            None => to_json(value),
        };
        match object.iter_mut().find(|(existing, _)| existing == name) {
            Some(entry) => entry.1 = json,
            None => object.push((name.clone(), json)),
        }
    }

    for name in record.appended_names() {
        let accessor = M::accessors()
            .iter()
            .find(|accessor| accessor.name() == name)
            .ok_or_else(|| {
                serialize_error::<M>(
                    Some(name),
                    format!(
                        "{} has no accessor named `{name}` (mark the method #[accessor] in an #[accessors] impl block)",
                        meta.name()
                    ),
                )
            })?;
        let json = accessor
            .value(&record.model)
            .map_err(|error| serialize_error::<M>(Some(name), error.to_string()))?;
        match object.iter_mut().find(|(existing, _)| existing == name) {
            Some(entry) => entry.1 = json,
            None => object.push((name.to_owned(), json)),
        }
    }
    Ok(object)
}

/// Keeps the visible keys, drops the hidden ones.
fn apply_visibility(object: &mut Object, visible: &[String], hidden: &[String]) {
    if !visible.is_empty() {
        object.retain(|(key, _)| visible.contains(key));
    }
    if !hidden.is_empty() {
        object.retain(|(key, _)| !hidden.contains(key));
    }
}

/// The serialized record: attributes, extras, appended values and loaded
/// relations, after visibility.
pub(crate) fn record_object<M: Model>(record: &Record<M>) -> Result<Object> {
    let meta = M::meta();
    let mut object = attributes_object(record)?;
    let visible = output_keys(meta, &record.visible_names());
    let hidden = output_keys(meta, &record.hidden_names());
    apply_visibility(&mut object, &visible, &hidden);

    if !record.relations.is_empty() {
        let relations = record.relations.to_json().map_err(|error| {
            serialize_error::<M>(None, format!("a loaded relation failed: {error}"))
        })?;
        if let serde_json::Value::Object(relations) = relations {
            let mut relations: Object = relations.into_iter().collect();
            apply_visibility(&mut relations, &visible, &hidden);
            for (name, json) in relations {
                match object.iter_mut().find(|(existing, _)| *existing == name) {
                    Some(entry) => entry.1 = json,
                    None => object.push((name, json)),
                }
            }
        }
    }
    Ok(object)
}

/// An ordered object as JSON.
pub(crate) fn to_value(object: Object) -> serde_json::Value {
    serde_json::Value::Object(object.into_iter().collect())
}

/// Writes `object` with `serializer`, in order.
pub(crate) fn write_object<S: Serializer>(object: &Object, serializer: S) -> std::result::Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(object.len()))?;
    for (key, value) in object {
        map.serialize_entry(key, value)?;
    }
    map.end()
}

/// The record as `serializer` writes it (what `Serialize for Record` does).
pub(crate) fn serialize_record<M: Model, S: Serializer>(
    record: &Record<M>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    let object = record_object(record).map_err(ser::Error::custom)?;
    write_object(&object, serializer)
}

/// Ordered JSON text of `object`.
fn object_text(object: &Object, pretty: bool) -> std::result::Result<String, serde_json::Error> {
    struct Ordered<'a>(&'a Object);

    impl Serialize for Ordered<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
            write_object(self.0, serializer)
        }
    }

    if pretty {
        serde_json::to_string_pretty(&Ordered(object))
    } else {
        serde_json::to_string(&Ordered(object))
    }
}

impl<M: Model> Record<M> {
    /// The record as JSON text (see [the serialization rules](crate::attributes::serialize)):
    /// fields in their order, then extras, appended values and relations.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let tag = Record::new(Tag { id: 1, name: "vip".into(), color: "gold".into() });
    /// assert_eq!(tag.to_json()?, r#"{"id":1,"name":"vip","color":"gold"}"#);
    /// # Ok::<(), navree_orm::OrmError>(())
    /// ```
    pub fn to_json(&self) -> Result<String> {
        let object = record_object(self)?;
        object_text(&object, false).map_err(|error| serialize_error::<M>(None, error.to_string()))
    }

    /// The record as indented JSON text.
    pub fn to_json_pretty(&self) -> Result<String> {
        let object = record_object(self)?;
        object_text(&object, true).map_err(|error| serialize_error::<M>(None, error.to_string()))
    }

    /// The record as a JSON value.
    pub fn to_value(&self) -> Result<serde_json::Value> {
        record_object(self).map(to_value)
    }

    /// The values of `names` (columns, fields, accessors, extras), whatever
    /// the record's visibility, keyed by the names given; a name the record
    /// does not have is `null`.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let tag = Record::new(Tag { id: 1, name: "vip".into(), color: "gold".into() });
    /// assert_eq!(serde_json::Value::Object(tag.only(["name", "rank"])?), serde_json::json!({"name": "vip", "rank": null}));
    /// # Ok::<(), navree_orm::OrmError>(())
    /// ```
    pub fn only<N: AsRef<str>>(
        &self,
        names: impl IntoIterator<Item = N>,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        let meta = M::meta();
        let object = attributes_object(self)?;
        let mut only = serde_json::Map::new();
        for name in names {
            let name = name.as_ref();
            let value = output_key(meta, name)
                .and_then(|key| object.iter().find(|(existing, _)| *existing == key))
                .map_or(serde_json::Value::Null, |(_, value)| value.clone());
            only.insert(name.to_owned(), value);
        }
        Ok(only)
    }

    /// The values of every column but `names`, whatever the record's
    /// visibility, keyed by column name (columns serde never writes are
    /// left out).
    pub fn except<N: AsRef<str>>(
        &self,
        names: impl IntoIterator<Item = N>,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        let meta = M::meta();
        let excluded: Vec<String> = names.into_iter().map(|name| name.as_ref().to_owned()).collect();
        let object = attributes_object(self)?;
        let mut except = serde_json::Map::new();
        for column in meta.all_columns() {
            if excluded.iter().any(|name| name == column.name() || name == column.field()) {
                continue;
            }
            let Some(key) = column.serialized_name() else {
                continue;
            };
            if let Some((_, value)) = object.iter().find(|(existing, _)| existing == key) {
                except.insert(column.name().to_owned(), value.clone());
            }
        }
        Ok(except)
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    #[derive(Serialize)]
    struct Point {
        z: i32,
        a: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    }

    #[test]
    fn capture_keeps_serdes_order_and_skips() {
        let object = Point { z: 1, a: 2, label: None }.serialize(Capture).unwrap();
        assert_eq!(
            object,
            vec![("z".to_owned(), serde_json::json!(1)), ("a".to_owned(), serde_json::json!(2))]
        );
        let map: std::collections::BTreeMap<&str, i32> = [("b", 1), ("a", 2)].into();
        assert_eq!(map.serialize(Capture).unwrap().len(), 2);
        let error = 7_u8.serialize(Capture).unwrap_err();
        assert!(error.message.starts_with("it serializes as a number"), "{}", error.message);
    }

    #[test]
    fn failing_fields_are_named() {
        #[derive(Serialize)]
        struct Bad {
            ok: i32,
            map: std::collections::HashMap<(i32, i32), i32>,
        }
        let bad = Bad {
            ok: 1,
            map: [((1, 2), 3)].into(),
        };
        let error = bad.serialize(Capture).unwrap_err();
        assert_eq!(error.column.as_deref(), Some("map"));
    }

    #[test]
    fn dates_are_written_in_utc_with_microseconds() {
        let at = parse_serialized("2026-10-27T10:30:00.5+02:00").unwrap();
        assert_eq!(default_date(&at), serde_json::json!("2026-10-27T08:30:00.500000Z"));
        let naive = parse_serialized("2026-10-27T08:30:00").unwrap();
        assert_eq!(default_date(&naive), serde_json::json!("2026-10-27T08:30:00.000000Z"));
        assert!(parse_serialized("yesterday").is_none());
    }
}
