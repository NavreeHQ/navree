//! [`Redactor`]: masks headers, context values and bodies whose names
//! contain a fragment of `log.redact` or `lighthouse.redact`.

use http::HeaderMap;
use serde_json::{Map, Value};

/// What a masked value is stored as.
pub const REDACTED: &str = "[redacted]";

/// `log.redact`'s default fragments.
pub(crate) const DEFAULT_FRAGMENTS: [&str; 5] =
    ["password", "token", "authorization", "cookie", "secret"];

/// Headers masked whatever the fragments say.
const ALWAYS_MASKED: [&str; 4] = [
    "authorization",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

/// Masks values by name.
#[derive(Clone, Debug)]
pub(crate) struct Redactor {
    fragments: Vec<String>,
}

impl Redactor {
    /// A redactor for these fragments (matched case-insensitively).
    pub(crate) fn new(fragments: &[String]) -> Redactor {
        Redactor {
            fragments: fragments
                .iter()
                .map(|fragment| fragment.trim().to_ascii_lowercase())
                .filter(|fragment| !fragment.is_empty())
                .collect(),
        }
    }

    /// Whether a value named `name` is masked.
    pub(crate) fn masks(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.fragments
            .iter()
            .any(|fragment| name.contains(fragment.as_str()))
    }

    /// The headers as a JSON object, masked (repeated headers joined with
    /// `, `).
    pub(crate) fn headers(&self, headers: &HeaderMap) -> Map<String, Value> {
        let mut out = Map::new();
        for name in headers.keys() {
            let key = name.as_str();
            let value = if ALWAYS_MASKED.contains(&key) || self.masks(key) {
                REDACTED.to_owned()
            } else {
                headers
                    .get_all(name)
                    .iter()
                    .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.insert(key.to_owned(), Value::String(value));
        }
        out
    }

    /// `value` with every object member whose key is masked replaced, at
    /// any depth.
    pub(crate) fn value(&self, value: Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(key, value)| {
                        if self.masks(&key) {
                            (key, Value::String(REDACTED.to_owned()))
                        } else {
                            (key, self.value(value))
                        }
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.into_iter().map(|v| self.value(v)).collect()),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use serde_json::json;

    use super::*;

    fn redactor() -> Redactor {
        let mut fragments: Vec<String> =
            DEFAULT_FRAGMENTS.iter().map(|s| (*s).to_owned()).collect();
        fragments.push("iban".to_owned());
        Redactor::new(&fragments)
    }

    #[test]
    fn headers_are_masked_by_name() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer abc"));
        headers.insert("cookie", HeaderValue::from_static("a=b"));
        headers.insert("x-api-token", HeaderValue::from_static("t0k"));
        headers.insert("accept", HeaderValue::from_static("application/json"));
        headers.append("x-many", HeaderValue::from_static("1"));
        headers.append("x-many", HeaderValue::from_static("2"));
        let masked = Redactor::new(&[]).headers(&headers);
        assert_eq!(masked["authorization"], REDACTED);
        assert_eq!(masked["cookie"], REDACTED);
        assert_eq!(
            masked["x-api-token"], "t0k",
            "no fragments: only the fixed ones"
        );
        let masked = redactor().headers(&headers);
        assert_eq!(masked["x-api-token"], REDACTED);
        assert_eq!(masked["accept"], "application/json");
        assert_eq!(masked["x-many"], "1, 2");
    }

    #[test]
    fn values_are_masked_at_any_depth() {
        let value = json!({
            "email": "ada@example.com",
            "password": "hunter2",
            "billing": { "IBAN": "DE00", "items": [{ "api_token": "x", "sku": "a" }] }
        });
        assert_eq!(
            redactor().value(value),
            json!({
                "email": "ada@example.com",
                "password": REDACTED,
                "billing": { "IBAN": REDACTED, "items": [{ "api_token": REDACTED, "sku": "a" }] }
            })
        );
    }
}
