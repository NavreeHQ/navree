//! [`EventMatch`]: which events end a wait.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which events of a type end a wait: every field named must equal the
/// given value in the event's JSON (paths are dotted: `deal.owner_id`).
/// Stored with the waiting run, so the match is decided when the event
/// arrives, without running the workflow.
///
/// ```
/// use navree_workflows::EventMatch;
/// use serde_json::json;
///
/// let filter = EventMatch::field("course_id", 7).and("user.id", "u-1");
/// assert!(filter.matches(&json!({"course_id": 7, "user": {"id": "u-1"}, "score": 90})));
/// assert!(!filter.matches(&json!({"course_id": 8, "user": {"id": "u-1"}})));
/// assert!(EventMatch::any().matches(&json!({"anything": true})));
/// ```
///
/// Values compare as JSON: `7` and `7.0` differ, and a UUID matches its
/// string form.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventMatch {
    /// Dotted path → the value it must hold.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) fields: BTreeMap<String, Value>,
    /// An automation wait: the trigger whose `matches` must also accept the
    /// event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) trigger: Option<TriggerRef>,
}

/// A trigger block and its configuration, inside an [`EventMatch`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TriggerRef {
    /// The trigger's type name.
    #[serde(rename = "type")]
    pub(crate) kind: String,
    /// Its configuration.
    #[serde(default)]
    pub(crate) config: Value,
    /// Only events of this subject (by the trigger's `SUBJECT` path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) subject: Option<String>,
}

impl EventMatch {
    /// Every event of the type.
    pub fn any() -> Self {
        Self::default()
    }

    /// Events whose field at `path` equals `value`.
    pub fn field(path: impl Into<String>, value: impl Serialize) -> Self {
        Self::any().and(path, value)
    }

    /// Also requires the field at `path` to equal `value`.
    pub fn and(mut self, path: impl Into<String>, value: impl Serialize) -> Self {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.fields.insert(path.into(), value);
        self
    }

    /// The fields and the values they must hold.
    pub fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }

    /// Whether `payload` (an event's JSON) has every field.
    pub fn matches(&self, payload: &Value) -> bool {
        self.fields
            .iter()
            .all(|(path, expected)| lookup(payload, path) == Some(expected))
    }
}

/// The value at dotted `path` in `value` (array items by index: `items.0`).
pub(crate) fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    path.split('.')
        .try_fold(value, |current, segment| match current {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn lookup_follows_objects_and_arrays() {
        let value = json!({"a": {"b": [10, {"c": true}]}});
        assert_eq!(lookup(&value, "a.b.0"), Some(&json!(10)));
        assert_eq!(lookup(&value, "a.b.1.c"), Some(&json!(true)));
        assert_eq!(lookup(&value, "a.x"), None);
        assert_eq!(lookup(&value, ""), Some(&value));
    }

    #[test]
    fn filters_round_trip_as_json() {
        let filter = EventMatch::field("deal_id", "d-1");
        let stored = serde_json::to_value(&filter).unwrap();
        assert_eq!(stored, json!({"fields": {"deal_id": "d-1"}}));
        let back: EventMatch = serde_json::from_value(stored).unwrap();
        assert_eq!(back, filter);
        assert_eq!(serde_json::to_value(EventMatch::any()).unwrap(), json!({}));
    }
}
