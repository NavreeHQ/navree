//! Server-sent events: [`Sse`], a response that streams [`Event`]s, with
//! [`KeepAlive`] comments while there is nothing to send.
//!
//! ```
//! use std::convert::Infallible;
//! use std::time::Duration;
//!
//! use futures_util::stream::{self, Stream};
//! use navree_http::response::sse::{Event, KeepAlive, Sse};
//!
//! async fn ticks() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
//!     let events = stream::iter([Ok(Event::default().event("tick").data("1"))]);
//!     Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
//! }
//! ```
//!
//! The response has `Content-Type: text/event-stream` and `Cache-Control:
//! no-cache`, each event goes out as soon as it is ready, and compression
//! leaves it alone. An error from the stream ends the response without a
//! final chunk, so the client sees a broken connection and reconnects.

use std::fmt;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use futures_util::stream::{Stream, TryStream};
use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http_body::Frame;
use navree_kernel::BoxError;
use pin_project_lite::pin_project;
use serde::Serialize;

use super::{IntoResponse, Response};
use crate::body::{Body, HttpBody};

/// A response that streams `S`'s events.
#[derive(Clone)]
#[must_use]
pub struct Sse<S> {
    stream: S,
}

impl<S> Sse<S> {
    /// A response with the events of `stream`.
    pub fn new(stream: S) -> Self
    where
        S: TryStream<Ok = Event> + Send + 'static,
        S::Error: Into<BoxError>,
    {
        Sse { stream }
    }

    /// Sends `keep_alive`'s event when the stream has been quiet for its
    /// interval. Without this nothing is sent between events.
    pub fn keep_alive(self, keep_alive: KeepAlive) -> Sse<KeepAliveStream<S>> {
        Sse {
            stream: KeepAliveStream::new(keep_alive, self.stream),
        }
    }
}

impl<S> fmt::Debug for Sse<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sse")
            .field("stream", &format_args!("{}", std::any::type_name::<S>()))
            .finish()
    }
}

impl<S, E> IntoResponse for Sse<S>
where
    S: Stream<Item = Result<Event, E>> + Send + 'static,
    E: Into<BoxError>,
{
    fn into_response(self) -> Response {
        (
            [
                (CONTENT_TYPE, "text/event-stream"),
                (CACHE_CONTROL, "no-cache"),
            ],
            Body::new(SseBody {
                events: self.stream,
            }),
        )
            .into_response()
    }
}

pin_project! {
    /// One data frame per event.
    struct SseBody<S> {
        #[pin]
        events: S,
    }
}

impl<S, E> HttpBody for SseBody<S>
where
    S: Stream<Item = Result<Event, E>>,
{
    type Data = Bytes;
    type Error = E;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, E>>> {
        match ready!(self.project().events.poll_next(cx)) {
            Some(Ok(event)) => Poll::Ready(Some(Ok(Frame::data(event.finalize())))),
            Some(Err(error)) => Poll::Ready(Some(Err(error))),
            None => Poll::Ready(None),
        }
    }
}

/// An event's bytes: still being written, or complete and cheap to clone (so
/// a keep-alive can be a `const`).
#[derive(Clone, Debug)]
enum Buffer {
    Active(BytesMut),
    Finalized(Bytes),
}

impl Buffer {
    /// The bytes to write to; complete ones are written to again.
    fn as_mut(&mut self) -> &mut BytesMut {
        if let Buffer::Finalized(bytes) = self {
            *self = Buffer::Active(BytesMut::from(mem::take(bytes)));
        }
        match self {
            Buffer::Active(bytes) => bytes,
            Buffer::Finalized(_) => unreachable!("it was made active above"),
        }
    }
}

/// One server-sent event.
///
/// A field set twice and a value that would break the framing (a newline in
/// an event name or an id, a NUL in an id) panic: these are programming
/// errors. `data` may hold newlines, which become separate `data:` lines.
#[derive(Clone, Debug)]
#[must_use]
pub struct Event {
    buffer: Buffer,
    has_data: bool,
    has_event: bool,
    has_retry: bool,
    has_id: bool,
}

impl Default for Event {
    fn default() -> Self {
        Self {
            buffer: Buffer::Active(BytesMut::new()),
            has_data: false,
            has_event: false,
            has_retry: false,
            has_id: false,
        }
    }
}

impl Event {
    /// The default keep-alive: an empty comment.
    pub const DEFAULT_KEEP_ALIVE: Self = Self::finalized(Bytes::from_static(b":\n\n"));

    const fn finalized(bytes: Bytes) -> Self {
        Self {
            buffer: Buffer::Finalized(bytes),
            has_data: false,
            has_event: false,
            has_retry: false,
            has_id: false,
        }
    }

    /// A writer for the event's data, for data that is not one string.
    pub fn into_data_writer(self) -> EventDataWriter {
        EventDataWriter {
            event: self,
            data_written: false,
        }
    }

    /// The event's data (`data: ...`). Newlines start a new `data:` line.
    ///
    /// # Panics
    ///
    /// When the event already has data.
    pub fn data(self, data: impl AsRef<str>) -> Self {
        let mut writer = self.into_data_writer();
        writer.write_bytes(data.as_ref().as_bytes());
        writer.into_event()
    }

    /// The event's data: `data` as compact JSON.
    ///
    /// # Panics
    ///
    /// When the event already has data.
    pub fn json_data(self, data: impl Serialize) -> Result<Self, serde_json::Error> {
        struct JsonWriter<'a>(&'a mut EventDataWriter);

        impl std::io::Write for JsonWriter<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.write_bytes(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut writer = self.into_data_writer();
        serde_json::to_writer(JsonWriter(&mut writer), &data)?;
        Ok(writer.into_event())
    }

    /// Adds a comment (`: ...`), which clients ignore. May be called more
    /// than once.
    ///
    /// # Panics
    ///
    /// When `comment` holds a newline or a carriage return.
    pub fn comment(mut self, comment: impl AsRef<str>) -> Self {
        self.field("", comment.as_ref().as_bytes());
        self
    }

    /// The event's name (`event: ...`), what a client listens for. Without
    /// one it is a `message` event.
    ///
    /// # Panics
    ///
    /// When `event` holds a newline or a carriage return, or the event
    /// already has a name.
    pub fn event(mut self, event: impl AsRef<str>) -> Self {
        assert!(!self.has_event, "Called `Event::event` multiple times");
        self.has_event = true;
        self.field("event", event.as_ref().as_bytes());
        self
    }

    /// How long a client should wait before reconnecting (`retry: ...`, in
    /// milliseconds). A hint: clients may wait longer.
    ///
    /// # Panics
    ///
    /// When the event already has a retry.
    pub fn retry(mut self, duration: Duration) -> Self {
        assert!(!self.has_retry, "Called `Event::retry` multiple times");
        self.has_retry = true;
        let secs = duration.as_secs();
        let millis = duration.subsec_millis();
        let buffer = self.buffer.as_mut();
        let line = if secs > 0 {
            format!("retry: {secs}{millis:03}\n")
        } else {
            format!("retry: {millis}\n")
        };
        buffer.extend_from_slice(line.as_bytes());
        self
    }

    /// The event's id (`id: ...`), which a client sends back as
    /// `Last-Event-ID` when it reconnects.
    ///
    /// # Panics
    ///
    /// When `id` holds a newline, a carriage return or a NUL, or the event
    /// already has an id.
    pub fn id(mut self, id: impl AsRef<str>) -> Self {
        assert!(!self.has_id, "Called `Event::id` multiple times");
        self.has_id = true;
        let id = id.as_ref().as_bytes();
        assert!(!id.contains(&0), "Event ID cannot contain null characters");
        self.field("id", id);
        self
    }

    fn field(&mut self, name: &str, value: &[u8]) {
        assert!(
            !value.iter().any(|byte| matches!(byte, b'\r' | b'\n')),
            "SSE field value cannot contain newlines or carriage returns",
        );
        let buffer = self.buffer.as_mut();
        buffer.extend_from_slice(name.as_bytes());
        buffer.put_u8(b':');
        buffer.put_u8(b' ');
        buffer.extend_from_slice(value);
        buffer.put_u8(b'\n');
    }

    fn finalize(self) -> Bytes {
        match self.buffer {
            Buffer::Finalized(bytes) => bytes,
            Buffer::Active(mut bytes) => {
                bytes.put_u8(b'\n');
                bytes.freeze()
            }
        }
    }
}

/// An [`Event`] as a [`fmt::Write`], from [`Event::into_data_writer`], so any
/// text can be written as its data, in pieces. `\r` and `\n` start a new
/// `data:` line.
///
/// # Panics
///
/// The first write panics when the event already has data.
#[derive(Debug)]
#[must_use]
pub struct EventDataWriter {
    event: Event,
    /// Whether this writer has written, whatever the event had.
    data_written: bool,
}

impl EventDataWriter {
    /// The event, with its data line closed if anything was written.
    pub fn into_event(self) -> Event {
        let mut event = self.event;
        if self.data_written {
            event.buffer.as_mut().put_u8(b'\n');
        }
        event
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if !mem::replace(&mut self.data_written, true) {
            assert!(!self.event.has_data, "Called `Event::data*` multiple times");
            self.event.has_data = true;
            self.event.buffer.as_mut().extend_from_slice(b"data: ");
        }
        let buffer = self.event.buffer.as_mut();
        let mut line_start = 0;
        for (at, byte) in bytes.iter().enumerate() {
            if matches!(byte, b'\n' | b'\r') {
                buffer.extend_from_slice(&bytes[line_start..=at]);
                buffer.extend_from_slice(b"data: ");
                line_start = at + 1;
            }
        }
        buffer.extend_from_slice(&bytes[line_start..]);
    }
}

impl fmt::Write for EventDataWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.write_bytes(text.as_bytes());
        Ok(())
    }
}

/// What to send while a stream is quiet, and how often.
#[derive(Clone, Debug)]
#[must_use]
pub struct KeepAlive {
    event: Event,
    max_interval: Duration,
}

impl KeepAlive {
    /// An empty comment every 15 seconds.
    pub fn new() -> Self {
        Self {
            event: Event::DEFAULT_KEEP_ALIVE,
            max_interval: Duration::from_secs(15),
        }
    }

    /// The longest quiet time.
    pub fn interval(mut self, interval: Duration) -> Self {
        self.max_interval = interval;
        self
    }

    /// The keep-alive as a comment with `text`.
    ///
    /// # Panics
    ///
    /// When `text` holds a newline or a carriage return.
    pub fn text(self, text: impl AsRef<str>) -> Self {
        self.event(Event::default().comment(text))
    }

    /// The keep-alive as `event`.
    pub fn event(mut self, event: Event) -> Self {
        self.event = Event::finalized(event.finalize());
        self
    }
}

impl Default for KeepAlive {
    fn default() -> Self {
        Self::new()
    }
}

pin_project! {
    /// A stream of events with keep-alives between them.
    #[derive(Debug)]
    pub struct KeepAliveStream<S> {
        #[pin]
        timer: tokio::time::Sleep,
        #[pin]
        inner: S,
        keep_alive: KeepAlive,
    }
}

impl<S> KeepAliveStream<S> {
    fn new(keep_alive: KeepAlive, inner: S) -> Self {
        Self {
            timer: tokio::time::sleep(keep_alive.max_interval),
            inner,
            keep_alive,
        }
    }

    fn reset(self: Pin<&mut Self>) {
        let this = self.project();
        this.timer
            .reset(tokio::time::Instant::now() + this.keep_alive.max_interval);
    }
}

impl<S, E> Stream for KeepAliveStream<S>
where
    S: Stream<Item = Result<Event, E>>,
{
    type Item = Result<Event, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                self.reset();
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(other) => Poll::Ready(other),
            Poll::Pending => {
                ready!(this.timer.poll(cx));
                let event = this.keep_alive.event.clone();
                self.reset();
                Poll::Ready(Some(Ok(event)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::fmt::Write as _;

    use futures_util::StreamExt as _;
    use futures_util::stream;
    use http_body_util::BodyExt;

    use super::*;

    #[test]
    fn events_are_framed() {
        let event = Event::default()
            .event("deal.updated")
            .id("42")
            .retry(Duration::from_millis(1500))
            .comment("note")
            .data("one\ntwo");
        assert_eq!(
            &*event.finalize(),
            b"event: deal.updated\nid: 42\nretry: 1500\n: note\ndata: one\ndata: two\n\n"
        );
    }

    #[test]
    fn retries_are_in_milliseconds() {
        let line = |duration| Event::default().retry(duration).finalize();
        assert_eq!(&*line(Duration::from_millis(7)), b"retry: 7\n\n");
        assert_eq!(&*line(Duration::from_millis(0)), b"retry: 0\n\n");
        assert_eq!(&*line(Duration::from_millis(30_000)), b"retry: 30000\n\n");
        assert_eq!(&*line(Duration::from_millis(3_007)), b"retry: 3007\n\n");
    }

    #[test]
    fn data_is_written_in_pieces() {
        let mut writer = Event::default()
            .into_data_writer()
            .into_event()
            .into_data_writer();
        writer.write_str("").unwrap();
        let mut writer = writer.into_event().into_data_writer();
        writer.write_str("moon ").unwrap();
        writer.write_str("star\nsun").unwrap();
        writer.write_str("").unwrap();
        writer.write_str(" bye\r").unwrap();
        assert_eq!(
            &*writer.into_event().finalize(),
            b"data: moon star\ndata: sun bye\rdata: \n\n"
        );
    }

    #[test]
    fn json_data_is_compact() {
        let event = Event::default()
            .json_data(serde_json::json!({ "a": 1 }))
            .unwrap();
        assert_eq!(&*event.finalize(), b"data: {\"a\":1}\n\n");
    }

    #[test]
    #[should_panic(expected = "Called `Event::data*` multiple times")]
    fn data_may_only_be_set_once() {
        let _ = Event::default().data("a").data("b");
    }

    #[test]
    #[should_panic(expected = "SSE field value cannot contain newlines")]
    fn a_newline_cannot_forge_a_field() {
        let _ = Event::default().event("a\ndata: forged");
    }

    #[test]
    #[should_panic(expected = "Event ID cannot contain null characters")]
    fn an_id_cannot_hold_a_nul() {
        let _ = Event::default().id("a\0b");
    }

    async fn next_text(body: &mut Body) -> Option<String> {
        let frame = body.frame().await?.unwrap();
        let data = frame.into_data().unwrap();
        Some(String::from_utf8(data.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn the_response_streams_one_frame_per_event() {
        let events =
            stream::iter(["1", "2"]).map(|data| Ok::<_, Infallible>(Event::default().data(data)));
        let response = Sse::new(events).into_response();
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert_eq!(response.headers()["cache-control"], "no-cache");
        let mut body = response.into_body();
        assert_eq!(next_text(&mut body).await.unwrap(), "data: 1\n\n");
        assert_eq!(next_text(&mut body).await.unwrap(), "data: 2\n\n");
        assert!(next_text(&mut body).await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn keep_alives_fill_the_quiet_and_events_push_them_back() {
        let (events, receiver) = futures_channel_pair();
        let response = Sse::new(receiver)
            .keep_alive(
                KeepAlive::new()
                    .interval(Duration::from_secs(10))
                    .text("waiting"),
            )
            .into_response();
        let mut body = response.into_body();

        let start = tokio::time::Instant::now();
        assert_eq!(next_text(&mut body).await.unwrap(), ": waiting\n\n");
        assert_eq!(start.elapsed(), Duration::from_secs(10));

        tokio::time::sleep(Duration::from_secs(4)).await;
        events.send(Ok(Event::default().data("x"))).await.unwrap();
        assert_eq!(next_text(&mut body).await.unwrap(), "data: x\n\n");
        assert_eq!(start.elapsed(), Duration::from_secs(14));

        // The quiet is counted from the event, not from the last keep-alive.
        assert_eq!(next_text(&mut body).await.unwrap(), ": waiting\n\n");
        assert_eq!(start.elapsed(), Duration::from_secs(24));

        // The end of the stream ends the keep-alives.
        drop(events);
        assert!(next_text(&mut body).await.is_none());
    }

    #[tokio::test]
    async fn a_stream_error_ends_the_body_with_it() {
        let events = stream::iter([
            Ok(Event::default().data("1")),
            Err(std::io::Error::other("gone")),
        ]);
        let mut body = Sse::new(events).into_response().into_body();
        assert_eq!(next_text(&mut body).await.unwrap(), "data: 1\n\n");
        let error = body.frame().await.unwrap().unwrap_err();
        assert_eq!(error.to_string(), "gone");
    }

    /// A sender and the stream of what it sends.
    fn futures_channel_pair() -> (
        tokio::sync::mpsc::Sender<Result<Event, Infallible>>,
        impl Stream<Item = Result<Event, Infallible>> + Send + 'static,
    ) {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let stream = stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });
        (sender, stream)
    }
}
