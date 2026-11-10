//! WebSockets: the upgrade handshake, here, and frames on `tokio-tungstenite`.
//!
//! ```
//! use navree_http::ws::{Message, WebSocket, WebSocketUpgrade};
//! use navree_http::response::Response;
//!
//! async fn connect(upgrade: WebSocketUpgrade) -> Response {
//!     upgrade.on_upgrade(echo)
//! }
//!
//! async fn echo(mut socket: WebSocket) {
//!     while let Some(Ok(message)) = socket.recv().await {
//!         if matches!(message, Message::Text(_)) && socket.send(message).await.is_err() {
//!             return;
//!         }
//!     }
//! }
//! ```
//!
//! Over HTTP/1.1 an upgrade is a `GET`; over HTTP/2 (RFC 8441) it is a
//! `CONNECT`, so a route that takes both declares both methods.
//!
//! # What a client can send
//!
//! The handshake is checked before a handler runs: anything but a well-formed
//! one is a 4xx and stays HTTP. After it, a frame longer than
//! [`max_frame_size`](WebSocketUpgrade::max_frame_size) or a message longer
//! than [`max_message_size`](WebSocketUpgrade::max_message_size) (1 MiB each
//! by default) ends the connection with a [`WebSocketError`] before the
//! payload is buffered; frames that are not masked, and text that is not
//! UTF-8, end it too. The server's own limits on a socket are in
//! [`serve`](fn@crate::serve): `upgraded_idle_timeout_ms` and the connection
//! caps.
//!
//! A socket is not a request: browsers send cookies with an upgrade from any
//! page, and the pipeline's CORS check does not apply to one. A handler that
//! trusts a cookie checks the origin with
//! [`origin_allowed`](WebSocketUpgrade::origin_allowed).

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use futures_util::stream::FusedStream;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use http::StatusCode;
use http::header::{self, HeaderValue};
use http::request::Parts;
use hyper::upgrade::{OnUpgrade, Upgraded};
use hyper_util::rt::TokioIo;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::{self, error::CapacityError};

use crate::body::Body;
use crate::extract::FromRequestParts;
use crate::response::Response;

mod handshake;
mod message;

pub use crate::extract::rejection::WebSocketRejection;
pub use handshake::{Handshake, origin_allowed};
pub use message::{CloseCode, CloseFrame, Message, Utf8Bytes};

/// The longest frame a client may send unless the handler says otherwise
/// (1 MiB).
pub const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;

/// The longest message a client may send, across its frames, unless the
/// handler says otherwise (1 MiB).
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// A request to open a WebSocket, as an extractor. It answers with
/// [`on_upgrade`](Self::on_upgrade).
///
/// Rejected, as a 4xx in the JSON error format, when the request is not a
/// well-formed handshake ([`WebSocketRejection`]), or when the connection
/// cannot be upgraded (426), such as in a test without a server.
#[must_use = "a WebSocket opens when the response of `on_upgrade` is returned"]
#[derive(Debug)]
pub struct WebSocketUpgrade {
    handshake: Handshake,
    config: WebSocketConfig,
    protocol: Option<HeaderValue>,
    on_upgrade: OnUpgrade,
}

impl WebSocketUpgrade {
    /// The longest frame this socket takes, in bytes
    /// ([`DEFAULT_MAX_FRAME_BYTES`] by default).
    pub fn max_frame_size(mut self, bytes: usize) -> Self {
        self.config = self.config.max_frame_size(Some(bytes));
        self
    }

    /// The longest message this socket takes, in bytes, across its frames
    /// ([`DEFAULT_MAX_MESSAGE_BYTES`] by default).
    pub fn max_message_size(mut self, bytes: usize) -> Self {
        self.config = self.config.max_message_size(Some(bytes));
        self
    }

    /// The size of the socket's read buffer (128 KiB by default), held for
    /// as long as the socket is open.
    pub fn read_buffer_size(mut self, bytes: usize) -> Self {
        self.config = self.config.read_buffer_size(bytes);
        self
    }

    /// How much is buffered before a write reaches the connection (128 KiB
    /// by default; 0 writes each message at once).
    pub fn write_buffer_size(mut self, bytes: usize) -> Self {
        self.config = self.config.write_buffer_size(bytes);
        self
    }

    /// The most that may wait to be written once writes start failing (no
    /// limit by default). At least `write_buffer_size` plus one message.
    pub fn max_write_buffer_size(mut self, bytes: usize) -> Self {
        self.config = self.config.max_write_buffer_size(bytes);
        self
    }

    /// Chooses the subprotocol: the first of `protocols`, in the server's
    /// order of preference, that the client offered. With none in common the
    /// response names no protocol, and the client decides whether to go on.
    pub fn protocols<I>(mut self, protocols: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        self.protocol = protocols.into_iter().find_map(|protocol| {
            let protocol = protocol.as_ref();
            self.handshake
                .requested_protocols()
                .iter()
                .find(|offered| offered.as_bytes() == protocol.as_bytes())
                .cloned()
        });
        self
    }

    /// The subprotocols the client offered, in its order.
    pub fn requested_protocols(&self) -> &[HeaderValue] {
        self.handshake.requested_protocols()
    }

    /// The subprotocol chosen by [`protocols`](Self::protocols).
    pub fn selected_protocol(&self) -> Option<&HeaderValue> {
        self.protocol.as_ref()
    }

    /// The `Origin` of the handshake: the page that opened the socket, when
    /// a browser did.
    pub fn origin(&self) -> Option<&str> {
        self.handshake.origin()
    }

    /// Whether a page from this request's origin may connect: see
    /// [`origin_allowed`].
    pub fn origin_allowed<S: AsRef<str>>(&self, allowed: &[S]) -> bool {
        origin_allowed(self.handshake.origin(), self.handshake.host(), allowed)
    }

    /// The response that finishes the handshake. Once the client has it,
    /// `callback` runs in a task of its own with the socket.
    #[must_use = "the WebSocket opens when this response is returned"]
    pub fn on_upgrade<C, F>(self, callback: C) -> Response
    where
        C: FnOnce(WebSocket) -> F + Send + 'static,
        F: Future<Output = ()> + Send + 'static,
    {
        let Self {
            handshake,
            config,
            protocol,
            on_upgrade,
        } = self;
        let chosen = protocol.clone();
        tokio::spawn(async move {
            let upgraded = match on_upgrade.await {
                Ok(upgraded) => upgraded,
                Err(error) => {
                    tracing::debug!(target: "navree::http", %error, "the WebSocket upgrade failed");
                    return;
                }
            };
            let inner = WebSocketStream::from_raw_socket(
                TokioIo::new(upgraded),
                Role::Server,
                Some(config),
            )
            .await;
            callback(WebSocket {
                inner,
                protocol: chosen,
            })
            .await;
        });

        let mut response = Response::new(Body::empty());
        // Over HTTP/2 a 200 is the whole answer (RFC 8441).
        if let Some(accept) = handshake.accept_key() {
            *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
            let headers = response.headers_mut();
            headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
            headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
            headers.insert(header::SEC_WEBSOCKET_ACCEPT, accept);
        }
        if let Some(protocol) = protocol {
            response
                .headers_mut()
                .insert(header::SEC_WEBSOCKET_PROTOCOL, protocol);
        }
        response
    }
}

impl FromRequestParts for WebSocketUpgrade {
    type Rejection = WebSocketRejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let handshake = Handshake::read(parts)?;
        let on_upgrade = parts
            .extensions
            .remove::<OnUpgrade>()
            .ok_or(WebSocketRejection::NotUpgradable)?;
        Ok(Self {
            handshake,
            config: WebSocketConfig::default()
                .max_frame_size(Some(DEFAULT_MAX_FRAME_BYTES))
                .max_message_size(Some(DEFAULT_MAX_MESSAGE_BYTES)),
            protocol: None,
            on_upgrade,
        })
    }
}

/// Why a socket failed.
#[derive(Debug)]
pub struct WebSocketError(Box<tungstenite::Error>);

impl WebSocketError {
    fn new(error: tungstenite::Error) -> Self {
        Self(Box::new(error))
    }

    /// Whether the client sent a frame or a message over the limit.
    pub fn is_too_large(&self) -> bool {
        matches!(
            *self.0,
            tungstenite::Error::Capacity(CapacityError::MessageTooLong { .. })
        )
    }
}

impl std::fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for WebSocketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.0)
    }
}

/// An open WebSocket: a [`Stream`] of messages and a [`Sink`] for them, so
/// `StreamExt::split` reads and writes it from two tasks.
#[derive(Debug)]
pub struct WebSocket {
    inner: WebSocketStream<TokioIo<Upgraded>>,
    protocol: Option<HeaderValue>,
}

impl WebSocket {
    /// The next message; `None` once the socket has closed.
    pub async fn recv(&mut self) -> Option<Result<Message, WebSocketError>> {
        self.next().await
    }

    /// Sends a message.
    pub async fn send(&mut self, message: Message) -> Result<(), WebSocketError> {
        self.inner
            .send(message.into_tungstenite())
            .await
            .map_err(WebSocketError::new)
    }

    /// The chosen subprotocol.
    pub fn protocol(&self) -> Option<&HeaderValue> {
        self.protocol.as_ref()
    }
}

impl Stream for WebSocket {
    type Item = Result<Message, WebSocketError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            return Poll::Ready(match ready!(self.inner.poll_next_unpin(cx)) {
                Some(Ok(message)) => match Message::from_tungstenite(message) {
                    Some(message) => Some(Ok(message)),
                    None => continue,
                },
                Some(Err(error)) => Some(Err(WebSocketError::new(error))),
                None => None,
            });
        }
    }
}

impl FusedStream for WebSocket {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}

impl Sink<Message> for WebSocket {
    type Error = WebSocketError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_ready(cx)
            .map_err(WebSocketError::new)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner)
            .start_send(item.into_tungstenite())
            .map_err(WebSocketError::new)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_flush(cx)
            .map_err(WebSocketError::new)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_close(cx)
            .map_err(WebSocketError::new)
    }
}
