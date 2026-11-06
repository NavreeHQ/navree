//! Payload redaction for the dashboard: values whose names contain a
//! fragment of `log.redact` are shown as `[redacted]`.

use serde_json::Value;

/// What a redacted value is shown as.
pub const REDACTED: &str = "[redacted]";

/// Redacts payload values by key name (ASCII case-insensitive substring).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Redactor {
    fragments: Vec<String>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new(navree_contracts::Redactor::DEFAULT_FRAGMENTS)
    }
}

impl Redactor {
    pub(crate) fn new<I, S>(fragments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            fragments: fragments
                .into_iter()
                .map(|f| f.as_ref().trim().to_ascii_lowercase())
                .filter(|f| !f.is_empty())
                .collect(),
        }
    }

    /// The fragments of a `log.redact` setting: a list, or one
    /// comma-separated string.
    pub(crate) fn from_setting(setting: Option<&Value>) -> Self {
        match setting {
            Some(Value::Array(items)) => Self::new(items.iter().filter_map(Value::as_str)),
            Some(Value::String(joined)) => Self::new(joined.split(',')),
            _ => Self::default(),
        }
    }

    fn hides(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.fragments.iter().any(|f| name.contains(f.as_str()))
    }

    /// `value` with every matching field replaced, at any depth.
    pub(crate) fn redact(&self, value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let shown = if self.hides(key) {
                            Value::String(REDACTED.to_owned())
                        } else {
                            self.redact(value)
                        };
                        (key.clone(), shown)
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(|v| self.redact(v)).collect()),
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn matching_names_are_redacted_at_any_depth() {
        let redactor = Redactor::default();
        let payload = json!({
            "user_id": 7,
            "Password": "hunter2",
            "nested": { "api_token": "t", "list": [{ "client_secret": "s", "ok": 1 }] }
        });
        assert_eq!(
            redactor.redact(&payload),
            json!({
                "user_id": 7,
                "Password": REDACTED,
                "nested": { "api_token": REDACTED, "list": [{ "client_secret": REDACTED, "ok": 1 }] }
            })
        );
    }

    #[test]
    fn the_setting_may_be_a_list_or_a_comma_separated_string() {
        let list = Redactor::from_setting(Some(&json!(["pin"])));
        assert_eq!(
            list.redact(&json!({"pin": 1, "password": 2})),
            json!({"pin": REDACTED, "password": 2})
        );
        let joined = Redactor::from_setting(Some(&json!("pin, iban")));
        assert_eq!(
            joined.redact(&json!({"iban": "x"})),
            json!({"iban": REDACTED})
        );
        assert_eq!(Redactor::from_setting(None), Redactor::default());
    }
}
