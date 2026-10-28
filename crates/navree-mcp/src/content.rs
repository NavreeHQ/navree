//! [`PromptMessage`] and [`ResourceContents`]: what prompts and resources
//! return besides plain values.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde_json::{Value, json};

/// One message of a prompt: what the user picks in the client starts the
/// conversation with these.
///
/// ```
/// use navree_mcp::PromptMessage;
///
/// let messages = vec![
///     PromptMessage::resource("crm://deals/0199"),
///     PromptMessage::user("Draft a short follow-up email from me."),
/// ];
/// assert_eq!(messages[1].role(), "user");
/// assert_eq!(messages[0].uri(), Some("crm://deals/0199"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptMessage {
    role: Role,
    body: MessageBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MessageBody {
    Text(String),
    Resource(String),
}

impl PromptMessage {
    /// A message from the user.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            body: MessageBody::Text(text.into()),
        }
    }

    /// A message from the assistant (an example answer, a draft to go on
    /// from).
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            body: MessageBody::Text(text.into()),
        }
    }

    /// A user message embedding the resource at `uri`: its current contents
    /// are read (with the same access checks as `resources/read`) when the
    /// prompt is rendered.
    pub fn resource(uri: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            body: MessageBody::Resource(uri.into()),
        }
    }

    /// `user` or `assistant`.
    pub fn role(&self) -> &'static str {
        match self.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    /// The text of a text message.
    pub fn text(&self) -> Option<&str> {
        match &self.body {
            MessageBody::Text(text) => Some(text),
            MessageBody::Resource(_) => None,
        }
    }

    /// The URI of a resource message.
    pub fn uri(&self) -> Option<&str> {
        match &self.body {
            MessageBody::Resource(uri) => Some(uri),
            MessageBody::Text(_) => None,
        }
    }

    /// The protocol message, with `embedded` as the contents of a resource
    /// message.
    pub(crate) fn into_mcp(self, embedded: Option<Value>) -> Value {
        let role = self.role();
        let content = match (self.body, embedded) {
            (MessageBody::Text(text), _) => text_block(text),
            (MessageBody::Resource(_), Some(contents)) => {
                json!({ "type": "resource", "resource": contents })
            }
            (MessageBody::Resource(uri), None) => text_block(uri),
        };
        json!({ "role": role, "content": content })
    }
}

/// A text content block.
pub(crate) fn text_block(text: impl Into<String>) -> Value {
    json!({ "type": "text", "text": text.into() })
}

/// Contents of a resource: text with a MIME type, or binary data such as a
/// PDF from storage (sent base64-encoded).
///
/// A resource method may return it, or a `Vec` of them, instead of a
/// serializable value (sent as JSON) or a `String` (sent as text).
///
/// ```
/// use navree_mcp::ResourceContents;
///
/// let csv = ResourceContents::text("id,title\n1,Acme", "text/csv");
/// assert_eq!(csv.mime_type(), "text/csv");
/// let pdf = ResourceContents::blob(vec![0x25, 0x50, 0x44, 0x46], "application/pdf");
/// assert_eq!(pdf.as_bytes(), Some(&[0x25, 0x50, 0x44, 0x46][..]));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceContents {
    mime_type: String,
    body: ContentsBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ContentsBody {
    Text(String),
    Blob(Vec<u8>),
}

impl ResourceContents {
    /// Text contents with a MIME type (`text/plain`, `text/markdown`, ...).
    pub fn text(text: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            body: ContentsBody::Text(text.into()),
        }
    }

    /// Binary contents with a MIME type, sent base64-encoded.
    pub fn blob(bytes: impl Into<Vec<u8>>, mime_type: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            body: ContentsBody::Blob(bytes.into()),
        }
    }

    /// `value` as JSON text (`application/json`).
    pub fn json<T: Serialize + ?Sized>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(Self::text(
            serde_json::to_string(value)?,
            "application/json",
        ))
    }

    /// The MIME type.
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// The text of text contents.
    pub fn as_text(&self) -> Option<&str> {
        match &self.body {
            ContentsBody::Text(text) => Some(text),
            ContentsBody::Blob(_) => None,
        }
    }

    /// The bytes of binary contents.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.body {
            ContentsBody::Blob(bytes) => Some(bytes),
            ContentsBody::Text(_) => None,
        }
    }

    /// The protocol contents of the resource at `uri`.
    pub(crate) fn into_mcp(self, uri: &str) -> Value {
        match self.body {
            ContentsBody::Text(text) => {
                json!({ "uri": uri, "mimeType": self.mime_type, "text": text })
            }
            ContentsBody::Blob(bytes) => {
                json!({ "uri": uri, "mimeType": self.mime_type, "blob": STANDARD.encode(bytes) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_render_as_protocol_messages() {
        let user = PromptMessage::user("hi").into_mcp(None);
        assert_eq!(
            user,
            json!({ "role": "user", "content": { "type": "text", "text": "hi" } })
        );
        let assistant = PromptMessage::assistant("sure").into_mcp(None);
        assert_eq!(assistant["role"], "assistant");
        let contents = ResourceContents::text("{}", "application/json").into_mcp("crm://x");
        let embedded = PromptMessage::resource("crm://x").into_mcp(Some(contents));
        assert_eq!(
            embedded["content"],
            json!({
                "type": "resource",
                "resource": { "uri": "crm://x", "mimeType": "application/json", "text": "{}" },
            })
        );
    }

    #[test]
    fn blobs_are_base64() {
        let contents =
            ResourceContents::blob(b"%PDF".to_vec(), "application/pdf").into_mcp("x://doc");
        assert_eq!(
            contents,
            json!({ "uri": "x://doc", "mimeType": "application/pdf", "blob": "JVBERg==" })
        );
        let json = ResourceContents::json(&json!({ "a": 1 })).unwrap();
        assert_eq!(
            (json.mime_type(), json.as_text()),
            ("application/json", Some("{\"a\":1}"))
        );
    }
}
