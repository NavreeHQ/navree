//! [`Document`]: what goes into an index for one record.

use std::collections::BTreeMap;
use std::fmt::Display;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The indexed form of one record: its primary key, its workspace (in apps
/// with workspaces) and the fields the index holds.
///
/// ```
/// use navree_search::Document;
///
/// let doc = Document::new(42)
///     .field("title", "Lovelace renewal")
///     .field("amount_cents", 1_200_000)
///     .field("company_name", None::<String>);
/// assert_eq!(doc.id(), "42");
/// assert_eq!(doc.get("title"), Some(&serde_json::json!("Lovelace renewal")));
/// assert_eq!(doc.get("company_name"), Some(&serde_json::Value::Null));
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
    #[serde(default)]
    fields: BTreeMap<String, Value>,
}

impl Document {
    /// A document for the record with primary key `id`.
    pub fn new(id: impl Display) -> Self {
        Self {
            id: id.to_string(),
            workspace: None,
            fields: BTreeMap::new(),
        }
    }

    /// Sets field `name` to `value` (anything `Serialize`; `None` is null).
    /// A value that cannot be serialized is stored as null.
    pub fn field(mut self, name: &str, value: impl Serialize) -> Self {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.fields.insert(name.to_owned(), value);
        self
    }

    /// Scopes the document to a workspace (the derive and the sync fill it
    /// from the entity's workspace column).
    pub fn workspace(mut self, id: impl Display) -> Self {
        self.workspace = Some(id.to_string());
        self
    }

    /// The primary key.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The workspace the document belongs to, if any.
    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace.as_deref()
    }

    /// Every field.
    pub fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }

    /// Field `name`.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    pub(crate) fn set_workspace(&mut self, workspace: Option<String>) {
        self.workspace = workspace;
    }

    pub(crate) fn fields_mut(&mut self) -> &mut BTreeMap<String, Value> {
        &mut self.fields
    }
}

/// The value an index stores for a filter or sort field: dates
/// (`2026-10-25`) and RFC 3339 timestamps become Unix milliseconds, so
/// ranges and sorts compare them as numbers on every engine. Other values
/// are kept.
pub(crate) fn index_value(value: &Value) -> Value {
    if let Value::String(text) = value
        && let Some(millis) = date_millis(text)
    {
        return Value::from(millis);
    }
    value.clone()
}

/// `text` as Unix milliseconds when it is an RFC 3339 timestamp or a
/// `YYYY-MM-DD` date.
fn date_millis(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 10 || !bytes[0].is_ascii_digit() || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if bytes.len() == 10 {
        let date = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn documents_hold_serialized_fields() {
        let id = uuid::Uuid::nil();
        let doc = Document::new(id)
            .field("name", "Ada")
            .field("count", 3)
            .workspace("w1");
        assert_eq!(doc.id(), id.to_string());
        assert_eq!(doc.workspace_id(), Some("w1"));
        assert_eq!(doc.get("count"), Some(&json!(3)));
        let round_trip: Document = serde_json::from_value(serde_json::to_value(&doc).unwrap())
            .expect("deserializes");
        assert_eq!(round_trip, doc);
    }

    #[test]
    fn dates_become_milliseconds_in_the_index() {
        assert_eq!(index_value(&json!("1970-01-02")), json!(86_400_000));
        assert_eq!(
            index_value(&json!("1970-01-01T00:00:01.500Z")),
            json!(1_500)
        );
        assert_eq!(
            index_value(&json!("1970-01-01T01:00:00+01:00")),
            json!(0),
            "offsets are honoured"
        );
        assert_eq!(index_value(&json!("customer")), json!("customer"));
        assert_eq!(index_value(&json!("2026-13-45")), json!("2026-13-45"));
        assert_eq!(index_value(&json!(7)), json!(7));
    }
}
