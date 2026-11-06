//! The OpenAPI 3.1 document: [`Document`] and the objects inside it.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;

use crate::Schema;

/// The operations of one path, by lower-case method (`get`, `post`), in the
/// order they were added.
pub type PathItem = IndexMap<String, Operation>;

/// An OpenAPI document (the root object).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Document {
    /// The OpenAPI version.
    pub openapi: String,
    /// The API's title and version.
    pub info: Info,
    /// The server URLs; empty leaves clients with the URL of the document.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,
    /// The tags, in the order operations first use them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// The paths, in the order they were added.
    pub paths: IndexMap<String, PathItem>,
    /// The component schemas and security schemes.
    pub components: Components,
}

/// `info`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Info {
    /// The API's title.
    pub title: String,
    /// The API's version.
    pub version: String,
}

/// One entry of `servers`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Server {
    /// The server's URL.
    pub url: String,
}

/// One entry of `tags`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tag {
    /// The tag's name.
    pub name: String,
}

/// An operation: what one method on one path does.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Operation {
    /// The groups the operation belongs to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// A one-line summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A longer description (Markdown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The unique operation id.
    #[serde(rename = "operationId", skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// The parameters.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,
    /// The request body.
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    pub request_body: Option<RequestBody>,
    /// The responses, by status, in the order they were added.
    pub responses: IndexMap<String, Response>,
    /// The security requirements; any one of them is enough. Empty means none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub security: Vec<BTreeMap<String, Vec<String>>>,
    /// Specification extensions (`x-navree-flag`).
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

/// A parameter of an operation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Parameter {
    /// The parameter's name.
    pub name: String,
    /// Where it goes: `path`, `query`, `header` or `cookie`.
    #[serde(rename = "in")]
    pub location: String,
    /// Whether the operation needs it.
    pub required: bool,
    /// Its schema.
    pub schema: Schema,
    /// What it means (Markdown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How an array or an object is written: `form`, `deepObject` and so on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Whether each member of an array or object is a parameter of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,
}

impl Parameter {
    /// A required path parameter.
    pub fn path(name: impl Into<String>, schema: Schema) -> Self {
        Self::new(name, "path", true, schema)
    }

    /// An optional query parameter.
    pub fn query(name: impl Into<String>, schema: Schema) -> Self {
        Self::new(name, "query", false, schema)
    }

    fn new(name: impl Into<String>, location: &str, required: bool, schema: Schema) -> Self {
        Self {
            name: name.into(),
            location: location.to_owned(),
            required,
            schema,
            description: None,
            style: None,
            explode: None,
        }
    }

    /// Sets `description`.
    #[must_use]
    pub fn described(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Marks the parameter as required.
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// An object written as `name[key]=value` (`style: deepObject`, `explode: true`).
    #[must_use]
    pub fn deep_object(mut self) -> Self {
        self.style = Some("deepObject".to_owned());
        self.explode = Some(true);
        self
    }

    /// A list written as one comma-separated value (`style: form`, `explode: false`).
    #[must_use]
    pub fn comma_separated(mut self) -> Self {
        self.style = Some("form".to_owned());
        self.explode = Some(false);
        self
    }
}

/// A request body.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RequestBody {
    /// Whether the operation needs it.
    pub required: bool,
    /// The body by content type.
    pub content: IndexMap<String, MediaType>,
}

/// A response.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Response {
    /// What the response means.
    pub description: String,
    /// The body by content type.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub content: IndexMap<String, MediaType>,
}

/// A body of one content type.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MediaType {
    /// The body's schema.
    pub schema: Schema,
}

/// `components`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Components {
    /// The schemas `$ref`s point at, by name.
    pub schemas: BTreeMap<String, Schema>,
    /// The security schemes, by name.
    #[serde(rename = "securitySchemes")]
    pub security_schemes: IndexMap<String, SecurityScheme>,
}

/// A security scheme.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SecurityScheme {
    /// `apiKey` or `http`.
    #[serde(rename = "type")]
    pub kind: String,
    /// For `apiKey`: where the key goes.
    #[serde(rename = "in", skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// For `apiKey`: the name of the cookie or header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For `http`: the authorization scheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// For `http` with bearer tokens: their format.
    #[serde(rename = "bearerFormat", skip_serializing_if = "Option::is_none")]
    pub bearer_format: Option<String>,
    /// What the scheme is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
