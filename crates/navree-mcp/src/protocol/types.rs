//! The protocol's descriptions of tools, resources, templates and prompts,
//! as `tools/list`, `resources/list`, `resources/templates/list` and
//! `prompts/list` send them.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A JSON object schema, shared (`Arc`) like the lists that carry it.
pub type JsonSchema = Arc<Map<String, Value>>;

fn serialize_schema<S: Serializer>(schema: &JsonSchema, serializer: S) -> Result<S::Ok, S::Error> {
    schema.as_ref().serialize(serializer)
}

fn deserialize_schema<'de, D: Deserializer<'de>>(deserializer: D) -> Result<JsonSchema, D::Error> {
    Map::deserialize(deserializer).map(Arc::new)
}

#[expect(
    clippy::ref_option,
    reason = "serde's `serialize_with` passes the field by reference"
)]
fn serialize_optional_schema<S: Serializer>(
    schema: &Option<JsonSchema>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    schema.as_deref().serialize(serializer)
}

fn deserialize_optional_schema<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<JsonSchema>, D::Error> {
    Option::<Map<String, Value>>::deserialize(deserializer).map(|schema| schema.map(Arc::new))
}

/// A tool as `tools/list` describes it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Tool {
    /// The name clients call it by.
    pub name: String,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What it does (the method's doc comment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The JSON Schema of its arguments (always an object schema).
    #[serde(
        default,
        serialize_with = "serialize_schema",
        deserialize_with = "deserialize_schema"
    )]
    pub input_schema: JsonSchema,
    /// The JSON Schema of its structured result, when it declares one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_schema",
        deserialize_with = "deserialize_optional_schema"
    )]
    pub output_schema: Option<JsonSchema>,
    /// Hints about its behaviour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

/// Hints about a tool's behaviour (`readOnlyHint`, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ToolAnnotations {
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// It changes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// It may destroy or overwrite data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Calling it twice with the same arguments does nothing more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// It reaches outside the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// A resource at a fixed URI, as `resources/list` describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Resource {
    /// Its URI.
    pub uri: String,
    /// Its name.
    pub name: String,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The MIME type of its contents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A resource template (a URI with `{variables}`), as
/// `resources/templates/list` describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ResourceTemplate {
    /// The RFC 6570 URI template.
    pub uri_template: String,
    /// Its name.
    pub name: String,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What its resources hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The MIME type of its resources' contents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A prompt as `prompts/list` describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Prompt {
    /// The name clients get it by.
    pub name: String,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What it is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Its arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgumentInfo>>,
}

/// One argument of a [`Prompt`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PromptArgumentInfo {
    /// Its name.
    pub name: String,
    /// What it means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the prompt needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// A server's (or client's) name, title and version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Implementation {
    /// Its name.
    pub name: String,
    /// A display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Its version.
    pub version: String,
    /// Its website.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
}

impl Implementation {
    /// `name` at `version`.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Self::default()
        }
    }

    /// With a display name.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}
