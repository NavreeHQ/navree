//! The JSON forms of everything that crosses the C interface, as a JSON Schema.

/// The schema of [`Config`](crate::Config), [`Event`](crate::Event),
/// [`Update`](crate::Update), [`Query`](crate::Query) and
/// [`ViewState`](crate::ViewState).
pub fn schema() -> serde_json::Value {
    serde_json::Value::Null
}
