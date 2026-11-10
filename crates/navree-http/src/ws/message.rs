//! The messages of a WebSocket.

use std::fmt;
use std::ops::Deref;
use std::str::Utf8Error;

use bytes::Bytes;
use tokio_tungstenite::tungstenite::protocol::CloseFrame as RawCloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode as RawCloseCode;
use tokio_tungstenite::tungstenite::{self, Message as Raw};

use super::WebSocketError;

/// Bytes that are always valid UTF-8: the payload of a text message.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Utf8Bytes(tungstenite::Utf8Bytes);

impl Utf8Bytes {
    /// From a static string.
    #[must_use]
    pub const fn from_static(text: &'static str) -> Self {
        Self(tungstenite::Utf8Bytes::from_static(text))
    }

    /// As a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Deref for Utf8Bytes {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Utf8Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for Utf8Bytes {
    fn from(text: String) -> Self {
        Self(text.into())
    }
}

impl From<&str> for Utf8Bytes {
    fn from(text: &str) -> Self {
        Self(text.into())
    }
}

impl From<&String> for Utf8Bytes {
    fn from(text: &String) -> Self {
        Self(text.into())
    }
}

impl TryFrom<Bytes> for Utf8Bytes {
    type Error = Utf8Error;

    fn try_from(bytes: Bytes) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}

impl TryFrom<Vec<u8>> for Utf8Bytes {
    type Error = Utf8Error;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Ok(Self(bytes.try_into()?))
    }
}

impl From<Utf8Bytes> for Bytes {
    fn from(text: Utf8Bytes) -> Self {
        text.0.into()
    }
}

impl<T> PartialEq<T> for Utf8Bytes
where
    for<'a> &'a str: PartialEq<T>,
{
    fn eq(&self, other: &T) -> bool {
        self.as_str() == *other
    }
}

/// Why a connection is closing: a status code of RFC 6455 §7.4, or `3000` to
/// `4999` for libraries and apps.
pub type CloseCode = u16;

/// A close code with its reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloseFrame {
    /// The code.
    pub code: CloseCode,
    /// The reason, for people.
    pub reason: Utf8Bytes,
}

/// A WebSocket message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// Text.
    Text(Utf8Bytes),
    /// Binary data.
    Binary(Bytes),
    /// A ping, of at most 125 bytes. The socket answers a client's ping by
    /// itself; a handler sees it too.
    Ping(Bytes),
    /// A pong, of at most 125 bytes.
    Pong(Bytes),
    /// The closing handshake. When a client sends this the socket answers by
    /// itself. Dropping a [`WebSocket`](super::WebSocket) also closes it,
    /// without the handshake.
    Close(Option<CloseFrame>),
}

impl Message {
    /// A text message.
    pub fn text(text: impl Into<Utf8Bytes>) -> Self {
        Self::Text(text.into())
    }

    /// A binary message.
    pub fn binary(bytes: impl Into<Bytes>) -> Self {
        Self::Binary(bytes.into())
    }

    /// The payload as bytes.
    #[must_use]
    pub fn into_data(self) -> Bytes {
        match self {
            Self::Text(text) => text.into(),
            Self::Binary(data) | Self::Ping(data) | Self::Pong(data) => data,
            Self::Close(None) => Bytes::new(),
            Self::Close(Some(frame)) => frame.reason.into(),
        }
    }

    /// The payload as text, when it is UTF-8.
    pub fn into_text(self) -> Result<Utf8Bytes, WebSocketError> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Binary(data) | Self::Ping(data) | Self::Pong(data) => data
                .try_into()
                .map_err(|_| WebSocketError::new(tungstenite::Error::Utf8(utf8_message()))),
            Self::Close(None) => Ok(Utf8Bytes::default()),
            Self::Close(Some(frame)) => Ok(frame.reason),
        }
    }

    /// The payload as a string slice, when it is UTF-8.
    pub fn to_text(&self) -> Result<&str, WebSocketError> {
        match self {
            Self::Text(text) => Ok(text.as_str()),
            Self::Binary(data) | Self::Ping(data) | Self::Pong(data) => std::str::from_utf8(data)
                .map_err(|_| WebSocketError::new(tungstenite::Error::Utf8(utf8_message()))),
            Self::Close(None) => Ok(""),
            Self::Close(Some(frame)) => Ok(frame.reason.as_str()),
        }
    }

    pub(super) fn into_tungstenite(self) -> Raw {
        match self {
            Self::Text(text) => Raw::Text(text.0),
            Self::Binary(data) => Raw::Binary(data),
            Self::Ping(data) => Raw::Ping(data),
            Self::Pong(data) => Raw::Pong(data),
            Self::Close(frame) => Raw::Close(frame.map(|frame| RawCloseFrame {
                code: RawCloseCode::from(frame.code),
                reason: frame.reason.0,
            })),
        }
    }

    /// `None` for a raw frame, which only a client of raw frames sees.
    pub(super) fn from_tungstenite(message: Raw) -> Option<Self> {
        Some(match message {
            Raw::Text(text) => Self::Text(Utf8Bytes(text)),
            Raw::Binary(data) => Self::Binary(data),
            Raw::Ping(data) => Self::Ping(data),
            Raw::Pong(data) => Self::Pong(data),
            Raw::Close(frame) => Self::Close(frame.map(|frame| CloseFrame {
                code: frame.code.into(),
                reason: Utf8Bytes(frame.reason),
            })),
            Raw::Frame(_) => return None,
        })
    }
}

fn utf8_message() -> String {
    "the message is not UTF-8".to_owned()
}

impl From<String> for Message {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

impl From<&str> for Message {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl From<&[u8]> for Message {
    fn from(data: &[u8]) -> Self {
        Self::Binary(Bytes::copy_from_slice(data))
    }
}

impl From<Bytes> for Message {
    fn from(data: Bytes) -> Self {
        Self::Binary(data)
    }
}

impl From<Vec<u8>> for Message {
    fn from(data: Vec<u8>) -> Self {
        Self::Binary(data.into())
    }
}

impl From<Message> for Vec<u8> {
    fn from(message: Message) -> Self {
        message.into_data().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_compares_as_a_string() {
        let text = Utf8Bytes::from_static("hello");
        assert_eq!(text, "hello");
        assert_eq!(text.len(), 5);
        assert_eq!(text.to_string(), "hello");
    }

    #[test]
    fn invalid_utf8_is_not_text() {
        assert!(Utf8Bytes::try_from(vec![0xff]).is_err());
        assert!(Utf8Bytes::try_from(Bytes::from_static(b"\xff")).is_err());
        assert!(Message::binary(vec![0xff]).to_text().is_err());
        assert!(Message::binary(vec![0xff]).into_text().is_err());
    }

    #[test]
    fn every_message_survives_the_trip_to_frames_and_back() {
        for message in [
            Message::text("text"),
            Message::binary(vec![1, 2, 3]),
            Message::Ping(Bytes::from_static(b"ping")),
            Message::Pong(Bytes::from_static(b"pong")),
            Message::Close(None),
            Message::Close(Some(CloseFrame {
                code: 4401,
                reason: "Unauthorized".into(),
            })),
        ] {
            assert_eq!(
                Message::from_tungstenite(message.clone().into_tungstenite()),
                Some(message)
            );
        }
    }

    #[test]
    fn payloads_read_as_data_and_text() {
        assert_eq!(&Message::text("a").into_data()[..], b"a");
        assert_eq!(Message::binary(vec![b'a']).into_text().unwrap(), "a");
        assert_eq!(Message::binary(vec![b'a']).to_text().unwrap(), "a");
        assert_eq!(Message::Close(None).to_text().unwrap(), "");
        let close = Message::Close(Some(CloseFrame {
            code: 1000,
            reason: "bye".into(),
        }));
        assert_eq!(close.to_text().unwrap(), "bye");
        assert_eq!(&close.into_data()[..], b"bye");
    }
}
