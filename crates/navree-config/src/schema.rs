//! What a configuration section says about itself: its fields, their types,
//! the first paragraph of their rustdoc, which of them are secrets, and the
//! environment variables they fall back to.
//!
//! `#[derive(ConfigSection)]` describes a section, and `#[derive(ConfigTable)]`
//! a table nested in one. The description is data, so tools can list the
//! sections of an app, print their defaults, draw a JSON Schema for editors
//! and write the reference without booting anything.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::{self, Debug};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use navree_contracts::Secret;
use serde_json::{Map, Value as Json, json};

/// The type of a field, reduced to what a person needs to write a value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldType {
    /// `true` or `false`.
    Bool,
    /// A whole number.
    Integer,
    /// A number with a fraction.
    Float,
    /// Text.
    String,
    /// A length of time.
    Duration,
    /// Any value.
    Any,
    /// One of a few names.
    Choice(Vec<String>),
    /// A list of values of one type.
    List(Box<FieldType>),
    /// A table of values of one type, by name.
    Map(Box<FieldType>),
    /// A table with fields of its own (see [`FieldSchema::fields`]).
    Table,
    /// A type this description does not reduce: its Rust name.
    Other(String),
}

impl FieldType {
    /// A list of `item`.
    pub fn list(item: FieldType) -> Self {
        FieldType::List(Box::new(item))
    }

    /// A table of `value`, by name.
    pub fn map(value: FieldType) -> Self {
        FieldType::Map(Box::new(value))
    }

    /// One of `values`.
    pub fn choice(values: &[&str]) -> Self {
        FieldType::Choice(values.iter().map(|value| (*value).to_owned()).collect())
    }

    /// A type known by its Rust name only.
    pub fn other(name: &str) -> Self {
        FieldType::Other(name.to_owned())
    }

    /// The JSON Schema of the type. Types a schema cannot say much about
    /// (durations, which accept several forms, and `Other`) accept anything,
    /// so an editor never underlines a valid value.
    fn json_schema(&self) -> Json {
        match self {
            FieldType::Bool => json!({ "type": "boolean" }),
            FieldType::Integer => json!({ "type": "integer" }),
            FieldType::Float => json!({ "type": "number" }),
            FieldType::String => json!({ "type": "string" }),
            FieldType::Choice(values) => json!({ "type": "string", "enum": values }),
            FieldType::List(item) => json!({ "type": "array", "items": item.json_schema() }),
            FieldType::Map(value) => {
                json!({ "type": "object", "additionalProperties": value.json_schema() })
            }
            FieldType::Table => json!({ "type": "object" }),
            FieldType::Duration | FieldType::Any | FieldType::Other(_) => json!({}),
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldType::Bool => f.write_str("boolean"),
            FieldType::Integer => f.write_str("integer"),
            FieldType::Float => f.write_str("number"),
            FieldType::String => f.write_str("string"),
            FieldType::Duration => f.write_str("duration"),
            FieldType::Any => f.write_str("any"),
            FieldType::Choice(values) => f.write_str(&values.join(" | ")),
            FieldType::List(item) => write!(f, "list of {item}"),
            FieldType::Map(value) => write!(f, "table of {value}"),
            FieldType::Table => f.write_str("table"),
            FieldType::Other(name) => f.write_str(name),
        }
    }
}

/// One field of a section or of a table inside one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSchema {
    /// The key a person writes (after `serde` renames).
    pub name: String,
    /// The type of the value.
    pub ty: FieldType,
    /// The first paragraph of the field's rustdoc, on one line.
    pub doc: String,
    /// The value is a credential: it is never printed, logged or serialized.
    pub secret: bool,
    /// The key must be set: it has no default.
    pub required: bool,
    /// The name looks like a credential's, and the value is not one.
    pub public: bool,
    /// The environment variable that fills the field when nothing else sets
    /// it (`REDIS_URL`).
    pub env_fallback: Option<String>,
    /// The fields of a [`FieldType::Table`].
    pub fields: Vec<FieldSchema>,
}

impl FieldSchema {
    /// A field called `name`.
    pub fn new(name: &str, ty: FieldType, doc: &str) -> Self {
        FieldSchema {
            name: name.to_owned(),
            ty,
            doc: doc.to_owned(),
            secret: false,
            required: false,
            public: false,
            env_fallback: None,
            fields: Vec::new(),
        }
    }

    /// Marks the value as a credential.
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    /// Marks the key as one that must be set.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Says the value is not a credential, whatever the name suggests.
    pub fn public(mut self) -> Self {
        self.public = true;
        self
    }

    /// Names the environment variable the field falls back to.
    pub fn env_fallback(mut self, variable: &str) -> Self {
        self.env_fallback = Some(variable.to_owned());
        self
    }

    /// Gives a table its fields.
    pub fn with_fields(mut self, fields: Vec<FieldSchema>) -> Self {
        self.fields = fields;
        self
    }

    fn json_schema(&self) -> Json {
        let mut schema = if self.ty == FieldType::Table && !self.fields.is_empty() {
            fields_schema(&self.fields)
        } else {
            self.ty.json_schema()
        };
        if !self.doc.is_empty() {
            put(&mut schema, "description", Json::String(self.doc.clone()));
        }
        if self.secret {
            put(&mut schema, "x-secret", Json::Bool(true));
        }
        if let Some(variable) = &self.env_fallback {
            put(
                &mut schema,
                "x-env-fallback",
                Json::String(variable.clone()),
            );
        }
        schema
    }
}

/// An environment variable a field falls back to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvFallback {
    /// The key of the field (`redis.url`).
    pub key: String,
    /// The variable (`REDIS_URL`).
    pub variable: String,
}

/// A section: its key, what it is for, and its fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionSchema {
    /// The key of the section (`mail`, `database.mongodb`).
    pub key: String,
    /// The first paragraph of the rustdoc of the section's type.
    pub doc: String,
    /// The page of the documentation that explains the section.
    pub docs: Option<String>,
    /// The fields.
    pub fields: Vec<FieldSchema>,
}

impl SectionSchema {
    /// A section with `fields`.
    pub fn new(key: &str, doc: &str, fields: Vec<FieldSchema>) -> Self {
        SectionSchema {
            key: key.to_owned(),
            doc: doc.to_owned(),
            docs: None,
            fields,
        }
    }

    /// Names the page of the documentation about the section.
    pub fn with_docs(mut self, path: &str) -> Self {
        self.docs = Some(path.to_owned());
        self
    }

    /// Every field of the section, tables included and followed by their
    /// own fields, with its full key (`mail.smtp.host`).
    pub fn leaves(&self) -> Vec<(String, &FieldSchema)> {
        let mut out = Vec::new();
        collect_leaves(&self.key, &self.fields, &mut out);
        out
    }

    /// The keys of the fields that are credentials.
    pub fn secret_paths(&self) -> Vec<String> {
        self.leaves()
            .into_iter()
            .filter(|(_, field)| field.secret)
            .map(|(path, _)| path)
            .collect()
    }

    /// The environment variables the fields fall back to.
    pub fn env_fallbacks(&self) -> Vec<EnvFallback> {
        self.leaves()
            .into_iter()
            .filter_map(|(key, field)| {
                field.env_fallback.as_ref().map(|variable| EnvFallback {
                    key,
                    variable: variable.clone(),
                })
            })
            .collect()
    }

    /// The keys of the text fields that are named like a credential
    /// (`password`, `api_key`, `token`) but are neither a `Secret` nor marked
    /// `#[config(secret)]` or `#[config(public)]`. A section with such a
    /// field would print a credential in `config:show`.
    pub fn unmarked_secrets(&self) -> Vec<String> {
        self.leaves()
            .into_iter()
            .filter(|(_, field)| {
                !field.secret
                    && !field.public
                    && looks_secret(&field.name)
                    && matches!(
                        field.ty,
                        FieldType::String
                            | FieldType::Any
                            | FieldType::Other(_)
                            | FieldType::List(_)
                            | FieldType::Map(_)
                    )
            })
            .map(|(path, _)| path)
            .collect()
    }

    /// The JSON Schema of the section's table.
    pub fn json_schema(&self) -> Json {
        let mut schema = fields_schema(&self.fields);
        if !self.doc.is_empty() {
            put(&mut schema, "description", Json::String(self.doc.clone()));
        }
        schema
    }
}

fn collect_leaves<'a>(
    prefix: &str,
    fields: &'a [FieldSchema],
    out: &mut Vec<(String, &'a FieldSchema)>,
) {
    for field in fields {
        let path = format!("{prefix}.{}", field.name);
        out.push((path.clone(), field));
        collect_leaves(&path, &field.fields, out);
    }
}

fn fields_schema(fields: &[FieldSchema]) -> Json {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in fields {
        properties.insert(field.name.clone(), field.json_schema());
        if field.required {
            required.push(Json::String(field.name.clone()));
        }
    }
    let mut object = Map::new();
    object.insert("type".to_owned(), Json::String("object".to_owned()));
    object.insert("properties".to_owned(), Json::Object(properties));
    if !required.is_empty() {
        object.insert("required".to_owned(), Json::Array(required));
    }
    Json::Object(object)
}

/// Sets `key` in `schema` when it is an object.
fn put(schema: &mut Json, key: &str, value: Json) {
    if let Json::Object(map) = schema {
        map.insert(key.to_owned(), value);
    }
}

/// Fragments that make a field name look like a credential's.
const SECRET_FRAGMENTS: [&str; 11] = [
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "api_key",
    "apikey",
    "private_key",
    "credential",
    "dsn",
    "access_key",
];

/// Endings that say a name with such a fragment holds something else: where
/// a credential is kept, the name of a header, a lifetime.
const NOT_SECRET_ENDINGS: [&str; 22] = [
    "_path",
    "_file",
    "_env",
    "_name",
    "_header",
    "_param",
    "_field",
    "_claim",
    "_prefix",
    "_type",
    "_kind",
    "_algorithm",
    "_id",
    "_ttl",
    "_secs",
    "_seconds",
    "_minutes",
    "_days",
    "_hours",
    "_length",
    "_bytes",
    "_endpoint",
];

fn looks_secret(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SECRET_FRAGMENTS.iter().any(|part| name.contains(part))
        && !NOT_SECRET_ENDINGS.iter().any(|end| name.ends_with(end))
}

/// A struct whose fields are described. `#[derive(ConfigSection)]` and
/// `#[derive(ConfigTable)]` write the impl.
pub trait ConfigTable {
    /// The fields, in the order they are declared.
    fn fields() -> Vec<FieldSchema>;

    /// The value as JSON, field by field, for the defaults: credentials are
    /// `null`, a value of a type the description does not know is its
    /// `Debug` text.
    fn to_json(&self) -> Json;
}

/// A value that can show itself as JSON in the defaults. Implemented for the
/// types configuration fields use; never for credentials ([`Secret`] is
/// `null`).
pub trait DescribeValue {
    /// The value as JSON.
    fn describe(&self) -> Json;
}

/// A value of a type that has no [`DescribeValue`]: its `Debug` text, and for
/// a bare name such as an enum's variant, the name in snake case.
pub fn debug_value<T: Debug + ?Sized>(value: &T) -> Json {
    let text = format!("{value:?}");
    let bare = text.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && text.chars().all(|c| c.is_ascii_alphanumeric());
    if !bare {
        return Json::String(text);
    }
    let mut out = String::with_capacity(text.len() + 2);
    let mut previous_lower = false;
    for c in text.chars() {
        if c.is_ascii_uppercase() {
            if previous_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            previous_lower = false;
        } else {
            out.push(c);
            previous_lower = true;
        }
    }
    Json::String(out)
}

impl DescribeValue for bool {
    fn describe(&self) -> Json {
        Json::Bool(*self)
    }
}

macro_rules! describe_number {
    ($($ty:ty),* $(,)?) => {
        $(impl DescribeValue for $ty {
            fn describe(&self) -> Json {
                Json::from(*self)
            }
        })*
    };
}

describe_number!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, f32, f64);

macro_rules! describe_text {
    ($($ty:ty),* $(,)?) => {
        $(impl DescribeValue for $ty {
            fn describe(&self) -> Json {
                Json::String(self.to_string())
            }
        })*
    };
}

describe_text!(str, String, char, SocketAddr, IpAddr, Ipv4Addr, Ipv6Addr);

impl DescribeValue for Path {
    fn describe(&self) -> Json {
        Json::String(self.display().to_string())
    }
}

impl DescribeValue for PathBuf {
    fn describe(&self) -> Json {
        Json::String(self.display().to_string())
    }
}

impl DescribeValue for Duration {
    fn describe(&self) -> Json {
        Json::String(format!("{self:?}"))
    }
}

impl DescribeValue for Json {
    fn describe(&self) -> Json {
        self.clone()
    }
}

impl<T> DescribeValue for Secret<T> {
    fn describe(&self) -> Json {
        Json::Null
    }
}

impl<T: DescribeValue> DescribeValue for Option<T> {
    fn describe(&self) -> Json {
        match self {
            Some(value) => value.describe(),
            None => Json::Null,
        }
    }
}

impl<T: DescribeValue> DescribeValue for [T] {
    fn describe(&self) -> Json {
        Json::Array(self.iter().map(DescribeValue::describe).collect())
    }
}

impl<T: DescribeValue> DescribeValue for Vec<T> {
    fn describe(&self) -> Json {
        Json::Array(self.iter().map(DescribeValue::describe).collect())
    }
}

impl<T: DescribeValue> DescribeValue for VecDeque<T> {
    fn describe(&self) -> Json {
        Json::Array(self.iter().map(DescribeValue::describe).collect())
    }
}

impl<T: DescribeValue> DescribeValue for BTreeSet<T> {
    fn describe(&self) -> Json {
        Json::Array(self.iter().map(DescribeValue::describe).collect())
    }
}

impl<T: DescribeValue, S> DescribeValue for HashSet<T, S> {
    fn describe(&self) -> Json {
        Json::Array(self.iter().map(DescribeValue::describe).collect())
    }
}

impl<K: ToString, V: DescribeValue> DescribeValue for BTreeMap<K, V> {
    fn describe(&self) -> Json {
        Json::Object(
            self.iter()
                .map(|(key, value)| (key.to_string(), value.describe()))
                .collect(),
        )
    }
}

impl<K: ToString, V: DescribeValue, S> DescribeValue for HashMap<K, V, S> {
    fn describe(&self) -> Json {
        Json::Object(
            self.iter()
                .map(|(key, value)| (key.to_string(), value.describe()))
                .collect(),
        )
    }
}

impl<T: DescribeValue + ?Sized> DescribeValue for Box<T> {
    fn describe(&self) -> Json {
        (**self).describe()
    }
}

impl<T: DescribeValue + ?Sized> DescribeValue for Arc<T> {
    fn describe(&self) -> Json {
        (**self).describe()
    }
}

impl<T: DescribeValue + ?Sized> DescribeValue for &T {
    fn describe(&self) -> Json {
        (**self).describe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail() -> SectionSchema {
        SectionSchema::new(
            "mail",
            "Mail settings.",
            vec![
                FieldSchema::new("from", FieldType::String, "The sender.").required(),
                FieldSchema::new("api_key", FieldType::String, "Sends mail.").secret(),
                FieldSchema::new("password", FieldType::String, "Unmarked."),
                FieldSchema::new("token_ttl_secs", FieldType::Integer, "A lifetime."),
                FieldSchema::new("key_path", FieldType::String, "A file."),
                FieldSchema::new("host", FieldType::String, "").env_fallback("SMTP_HOST"),
                FieldSchema::new("smtp", FieldType::Table, "The server.").with_fields(vec![
                    FieldSchema::new("port", FieldType::Integer, "The port."),
                    FieldSchema::new("client_secret", FieldType::String, "").secret(),
                ]),
            ],
        )
    }

    #[test]
    fn leaves_carry_the_full_key() {
        let schema = mail();
        let keys: Vec<String> = schema.leaves().into_iter().map(|(key, _)| key).collect();
        assert_eq!(
            keys,
            [
                "mail.from",
                "mail.api_key",
                "mail.password",
                "mail.token_ttl_secs",
                "mail.key_path",
                "mail.host",
                "mail.smtp",
                "mail.smtp.port",
                "mail.smtp.client_secret"
            ]
        );
    }

    #[test]
    fn secrets_and_fallbacks_are_listed() {
        let schema = mail();
        assert_eq!(
            schema.secret_paths(),
            ["mail.api_key", "mail.smtp.client_secret"]
        );
        assert_eq!(
            schema.env_fallbacks(),
            [EnvFallback {
                key: "mail.host".to_owned(),
                variable: "SMTP_HOST".to_owned()
            }]
        );
    }

    #[test]
    fn a_credential_named_field_that_is_not_marked_is_found() {
        // `token_ttl_secs` (a number) and `key_path` (a path) are not
        // credentials; `password` is one that nobody marked.
        assert_eq!(mail().unmarked_secrets(), ["mail.password"]);
        let public = SectionSchema::new(
            "x",
            "",
            vec![FieldSchema::new("password_hint", FieldType::String, "").public()],
        );
        assert!(public.unmarked_secrets().is_empty());
    }

    #[test]
    fn the_json_schema_describes_fields_tables_secrets_and_required_keys() {
        let schema = mail().json_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["description"], "Mail settings.");
        assert_eq!(schema["required"], json!(["from"]));
        assert_eq!(schema["properties"]["from"]["type"], "string");
        assert_eq!(schema["properties"]["from"]["description"], "The sender.");
        assert_eq!(schema["properties"]["api_key"]["x-secret"], true);
        assert_eq!(schema["properties"]["host"]["x-env-fallback"], "SMTP_HOST");
        assert_eq!(schema["properties"]["smtp"]["type"], "object");
        assert_eq!(
            schema["properties"]["smtp"]["properties"]["port"]["type"],
            "integer"
        );
        assert!(schema["properties"]["host"].get("description").is_none());
    }

    #[test]
    fn types_print_as_a_person_would_write_them() {
        assert_eq!(FieldType::Bool.to_string(), "boolean");
        assert_eq!(
            FieldType::list(FieldType::String).to_string(),
            "list of string"
        );
        assert_eq!(
            FieldType::map(FieldType::Integer).to_string(),
            "table of integer"
        );
        assert_eq!(
            FieldType::choice(&["memory", "redis"]).to_string(),
            "memory | redis"
        );
        assert_eq!(
            FieldType::other("CacheStoreKind").to_string(),
            "CacheStoreKind"
        );
        assert_eq!(
            FieldType::choice(&["a", "b"]).json_schema(),
            json!({ "type": "string", "enum": ["a", "b"] })
        );
    }

    #[test]
    fn values_describe_themselves_and_secrets_stay_null() {
        assert_eq!(true.describe(), json!(true));
        assert_eq!(3600u64.describe(), json!(3600));
        assert_eq!(0.5f64.describe(), json!(0.5));
        assert_eq!("a".to_owned().describe(), json!("a"));
        assert_eq!(Some(7u16).describe(), json!(7));
        assert_eq!(None::<u16>.describe(), Json::Null);
        assert_eq!(
            vec!["a".to_owned(), "b".to_owned()].describe(),
            json!(["a", "b"])
        );
        assert_eq!(Duration::from_secs(30).describe(), json!("30s"));
        assert_eq!(Secret::new("hunter2".to_owned()).describe(), Json::Null);
        assert_eq!(Some(Secret::new(1u8)).describe(), Json::Null);
        let mut map = BTreeMap::new();
        map.insert("a".to_owned(), 1u8);
        assert_eq!(map.describe(), json!({ "a": 1 }));
    }

    #[test]
    fn a_type_without_a_description_prints_its_debug_text() {
        #[derive(Debug)]
        enum Store {
            PerRequest,
            Redis,
        }
        assert_eq!(debug_value(&Store::PerRequest), json!("per_request"));
        assert_eq!(debug_value(&Store::Redis), json!("redis"));
        assert_eq!(debug_value(&vec![1, 2]), json!("[1, 2]"));
    }
}
