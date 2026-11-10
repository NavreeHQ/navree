//! [`Message`], [`Attachment`] and the prompt types.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use bytes::Bytes;
use navree_storage::Disk;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::AiError;

/// Who wrote a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    /// The user (and the tool results sent back to the model).
    User,
    /// The model.
    Assistant,
}

/// One message of a conversation: text, attachments, and for agents the
/// tool calls and their results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Who wrote it.
    pub role: Role,
    /// Its content, in order.
    pub parts: Vec<Part>,
}

impl Message {
    /// A user message.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            parts: vec![Part::Text { text: text.into() }],
        }
    }

    /// A model message (an earlier answer, to continue a conversation).
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            parts: vec![Part::Text { text: text.into() }],
        }
    }

    /// A message of `role` with `parts`.
    pub fn new(role: Role, parts: Vec<Part>) -> Self {
        Self { role, parts }
    }

    /// The text parts, joined.
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The tool calls in this (model) message.
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.parts.iter().filter_map(|part| match part {
            Part::ToolCall(call) => Some(call),
            _ => None,
        })
    }

    /// The tool results in this (user) message.
    pub fn tool_results(&self) -> impl Iterator<Item = &ToolResult> {
        self.parts.iter().filter_map(|part| match part {
            Part::ToolResult(result) => Some(result),
            _ => None,
        })
    }

    /// The attachments in this message.
    pub fn attachments(&self) -> impl Iterator<Item = &Attachment> {
        self.parts.iter().filter_map(|part| match part {
            Part::Attachment(attachment) => Some(attachment),
            _ => None,
        })
    }
}

/// A piece of a [`Message`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Part {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// An image or a document.
    Attachment(Attachment),
    /// The model asks to run a tool.
    ToolCall(ToolCall),
    /// What a tool returned, sent back to the model.
    ToolResult(ToolResult),
    /// Provider-specific content the provider needs back unchanged on the
    /// next request (a reasoning block and its signature); other providers
    /// skip it.
    Reasoning {
        /// The provider driver that produced it (`anthropic`).
        provider: String,
        /// The provider's own block.
        data: Value,
    },
}

/// A tool call made by the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The call id, which its result refers to.
    pub id: String,
    /// The tool name.
    pub name: String,
    /// The arguments (a JSON object).
    pub arguments: Value,
    /// Provider-specific data the provider needs back with the call (a
    /// thought signature).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// A tool's result, as the model reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The id of the call it answers.
    pub call_id: String,
    /// The tool name.
    pub name: String,
    /// The result: the tool's JSON output, or `{"error": {...}}`.
    pub content: Value,
    /// Whether the tool failed.
    #[serde(default)]
    pub is_error: bool,
}

/// What kind of file an [`Attachment`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentKind {
    /// An image (`image/png`, `image/jpeg`, `image/gif`, `image/webp`).
    Image,
    /// A PDF document.
    Pdf,
    /// Anything else (no provider reads it).
    Other,
}

/// An image or a document attached to a request.
#[derive(Clone, PartialEq)]
pub struct Attachment {
    media_type: String,
    data: Bytes,
    name: Option<String>,
}

impl Attachment {
    /// An image from its bytes and media type (`image/png`).
    pub fn image(bytes: impl Into<Bytes>, media_type: impl Into<String>) -> Self {
        Self {
            media_type: media_type.into(),
            data: bytes.into(),
            name: None,
        }
    }

    /// A PDF document from its bytes.
    pub fn pdf(bytes: impl Into<Bytes>) -> Self {
        Self {
            media_type: "application/pdf".to_owned(),
            data: bytes.into(),
            name: None,
        }
    }

    /// A file read from a storage disk, its type taken from the content
    /// (magic bytes); a file of unknown type is `application/octet-stream`.
    pub async fn from_disk(disk: &Disk, path: &str) -> Result<Self, AiError> {
        let bytes = disk
            .get(path)
            .await
            .map_err(|error| AiError::internal(format!("cannot read `{path}`: {error}")))?;
        let media_type = infer::get(&bytes)
            .map(|kind| kind.mime_type().to_owned())
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let name = path.rsplit('/').next().map(str::to_owned);
        Ok(Self {
            media_type,
            data: bytes,
            name,
        })
    }

    /// Sets the file name some providers show the model.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The media type.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    /// The bytes.
    pub fn data(&self) -> &Bytes {
        &self.data
    }

    /// The file name, if known.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The bytes as standard base64.
    pub fn base64(&self) -> String {
        BASE64.encode(&self.data)
    }

    /// Image, PDF or other.
    pub fn kind(&self) -> AttachmentKind {
        if self.media_type.starts_with("image/") {
            AttachmentKind::Image
        } else if self.media_type == "application/pdf" {
            AttachmentKind::Pdf
        } else {
            AttachmentKind::Other
        }
    }
}

impl fmt::Debug for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachment")
            .field("media_type", &self.media_type)
            .field("bytes", &self.data.len())
            .field("name", &self.name)
            .finish()
    }
}

/// How an attachment is stored: its bytes as base64.
#[derive(Serialize, Deserialize)]
struct AttachmentRepr {
    media_type: String,
    data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl Serialize for Attachment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AttachmentRepr {
            media_type: self.media_type.clone(),
            data: self.base64(),
            name: self.name.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Attachment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let repr = AttachmentRepr::deserialize(deserializer)?;
        let data = BASE64
            .decode(repr.data.as_bytes())
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            media_type: repr.media_type,
            data: Bytes::from(data),
            name: repr.name,
        })
    }
}

/// What a request asks: a string (one user message) or a list of messages
/// with the earlier turns.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt(pub(crate) Vec<Message>);

impl Prompt {
    /// The messages.
    pub fn messages(&self) -> &[Message] {
        &self.0
    }
}

impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        Prompt(vec![Message::user(text)])
    }
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Prompt(vec![Message::user(text)])
    }
}

impl From<&String> for Prompt {
    fn from(text: &String) -> Self {
        Prompt(vec![Message::user(text.as_str())])
    }
}

impl From<Message> for Prompt {
    fn from(message: Message) -> Self {
        Prompt(vec![message])
    }
}

impl From<Vec<Message>> for Prompt {
    fn from(messages: Vec<Message>) -> Self {
        Prompt(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_from_strings_are_one_user_message() {
        let prompt = Prompt::from("hello");
        assert_eq!(prompt.messages(), [Message::user("hello")]);
        let prompt = Prompt::from(vec![Message::user("a"), Message::assistant("b")]);
        assert_eq!(prompt.messages().len(), 2);
        assert_eq!(prompt.messages()[1].role, Role::Assistant);
    }

    #[test]
    fn message_text_joins_the_text_parts() {
        let message = Message::new(
            Role::Assistant,
            vec![
                Part::Text { text: "a".into() },
                Part::ToolCall(ToolCall {
                    id: "1".into(),
                    name: "t".into(),
                    arguments: serde_json::json!({}),
                    meta: None,
                }),
                Part::Text { text: "b".into() },
            ],
        );
        assert_eq!(message.text(), "ab");
        assert_eq!(message.tool_calls().count(), 1);
    }

    #[test]
    fn attachments_round_trip_through_json() {
        let attachment = Attachment::image(vec![1u8, 2, 3], "image/png").named("dot.png");
        let json = serde_json::to_value(Part::Attachment(attachment.clone())).unwrap();
        assert_eq!(json["type"], "attachment");
        assert_eq!(json["data"], "AQID");
        let back: Part = serde_json::from_value(json).unwrap();
        assert_eq!(back, Part::Attachment(attachment));
    }

    #[test]
    fn attachment_kinds() {
        assert_eq!(Attachment::pdf(vec![0u8]).kind(), AttachmentKind::Pdf);
        assert_eq!(
            Attachment::image(vec![0u8], "image/webp").kind(),
            AttachmentKind::Image
        );
        assert_eq!(
            Attachment::image(vec![0u8], "text/csv").kind(),
            AttachmentKind::Other
        );
        assert!(format!("{:?}", Attachment::pdf(vec![0u8; 10])).contains("bytes: 10"));
    }
}
