//! [`TextStream`] and [`StreamEvent`]: answers that arrive piece by piece,
//! as a `Stream` in Rust and as server-sent events over HTTP.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{Stream, StreamExt};
use http::HeaderValue;
use navree_kernel::BoxFuture;
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::agent::PendingApproval;
use crate::ai::OpenStream;
use crate::error::AiError;
use crate::provider::StreamChunk;
use crate::response::{Completion, TextResponse};

/// One event of a stream.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum StreamEvent {
    /// A piece of the answer.
    Text(String),
    /// An agent calls a tool.
    ToolCall {
        /// The call id.
        id: String,
        /// The tool name.
        tool: String,
        /// The arguments.
        arguments: Value,
    },
    /// An agent's tool ran (its result stays on the server).
    ToolResult {
        /// The call id.
        id: String,
        /// The tool name.
        tool: String,
        /// Whether it succeeded.
        ok: bool,
    },
    /// An agent's run stopped for the user to approve a tool call.
    Approval(PendingApproval),
    /// Last: the model, usage and finish of the whole answer.
    Done(Completion),
}

impl StreamEvent {
    /// The SSE event name: `text`, `tool_call`, `tool_result`, `approval`
    /// or `done`.
    pub fn name(&self) -> &'static str {
        match self {
            StreamEvent::Text(_) => "text",
            StreamEvent::ToolCall { .. } => "tool_call",
            StreamEvent::ToolResult { .. } => "tool_result",
            StreamEvent::Approval(_) => "approval",
            StreamEvent::Done(_) => "done",
        }
    }

    /// The SSE event data.
    pub fn data(&self) -> Value {
        match self {
            StreamEvent::Text(text) => json!({ "text": text }),
            StreamEvent::ToolCall { id, tool, arguments } => {
                json!({ "id": id, "tool": tool, "arguments": arguments })
            }
            StreamEvent::ToolResult { id, tool, ok } => json!({ "id": id, "tool": tool, "ok": ok }),
            StreamEvent::Approval(pending) => serde_json::to_value(pending).unwrap_or_default(),
            StreamEvent::Done(completion) => serde_json::to_value(completion).unwrap_or_default(),
        }
    }
}

/// The events of a stream.
pub(crate) type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, AiError>> + Send>>;

/// What runs after the last event.
type OnFinish = Box<dyn FnOnce(TextResponse) -> BoxFuture<'static, ()> + Send>;

/// A streamed answer: a `Stream` of [`StreamEvent`]s (each a
/// `Result<StreamEvent, AiError>`), and a response that sends them as
/// server-sent events.
///
/// ```no_run
/// use futures_util::StreamExt;
/// use navree_ai::StreamEvent;
///
/// # async fn run(ai: navree_ai::Ai) -> Result<(), navree_ai::AiError> {
/// let mut stream = ai.stream("Write a haiku").model("fast").await?;
/// while let Some(event) = stream.next().await {
///     match event? {
///         StreamEvent::Text(text) => print!("{text}"),
///         StreamEvent::Done(done) => println!("\n{} tokens", done.usage.output_tokens),
///         _ => {}
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct TextStream {
    events: EventStream,
    on_finish: Option<OnFinish>,
    text: String,
    done: Option<Completion>,
}

impl TextStream {
    /// A stream from an open provider stream.
    pub(crate) fn from_open(open: OpenStream, log_content: bool) -> Self {
        Self::new(text_events(open, log_content))
    }

    pub(crate) fn new(events: EventStream) -> Self {
        Self {
            events,
            on_finish: None,
            text: String::new(),
            done: None,
        }
    }

    /// Merges the text that arrives within each `window` into one event,
    /// which keeps the event rate steady whatever the model's speed. Other
    /// events pass through in order.
    pub fn chunked(mut self, window: Duration) -> Self {
        self.events = chunk_events(self.events, window);
        self
    }

    /// Runs `f` with the whole answer once the last event is sent (not when
    /// the client disconnects first, since the generation stopped). An
    /// error it returns is logged at `warn`.
    pub fn on_finish<F, Fut, T, E>(mut self, f: F) -> Self
    where
        F: FnOnce(TextResponse) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        E: fmt::Debug + 'static,
    {
        self.on_finish = Some(Box::new(move |response| {
            Box::pin(async move {
                if let Err(error) = f(response).await {
                    tracing::warn!(target: "navree::ai", ?error, "the stream's on_finish failed");
                }
            })
        }));
        self
    }
}

impl Stream for TextStream {
    type Item = Result<StreamEvent, AiError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.events.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                match &event {
                    StreamEvent::Text(text) => {
                        let text = text.clone();
                        self.text.push_str(&text);
                    }
                    StreamEvent::Done(completion) => self.done = Some(completion.clone()),
                    _ => {}
                }
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(None) => {
                if let (Some(done), Some(f)) = (self.done.take(), self.on_finish.take()) {
                    let response = TextResponse {
                        text: std::mem::take(&mut self.text),
                        model: done.model,
                        provider: done.provider,
                        usage: done.usage,
                        finish: done.finish,
                    };
                    tokio::spawn(f(response));
                }
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

impl fmt::Debug for TextStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextStream").finish_non_exhaustive()
    }
}

impl IntoResponse for TextStream {
    fn into_response(self) -> Response {
        sse_response(self)
    }
}

/// The text events of an open provider stream: text pieces, then `Done`
/// once the provider finishes (its usage recorded), or an error. Each gap
/// between two chunks is limited by the model's timeout.
fn text_events(open: OpenStream, log_content: bool) -> EventStream {
    struct State {
        open: Option<OpenStream>,
        ended: bool,
        text: String,
    }
    let state = State {
        open: Some(open),
        ended: false,
        text: String::new(),
    };
    Box::pin(futures_util::stream::unfold(state, move |mut state| async move {
        loop {
            if state.ended {
                return None;
            }
            let open = state.open.as_mut()?;
            let gap = open.gap;
            let next = tokio::time::timeout(gap, open.chunks.next()).await;
            let provider = open.provider.clone();
            match next {
                Ok(Some(Ok(StreamChunk::Text(text)))) => {
                    if log_content {
                        state.text.push_str(&text);
                    }
                    return Some((Ok(StreamEvent::Text(text)), state));
                }
                Ok(Some(Ok(StreamChunk::Done { usage, finish }))) => {
                    state.ended = true;
                    let open = state.open.take()?;
                    if log_content {
                        tracing::debug!(target: "navree::ai", model = %open.model, output = %state.text, "AI content");
                    }
                    let completion = Completion {
                        model: open.model.clone(),
                        provider: open.provider.clone(),
                        usage,
                        finish,
                        conversation_id: None,
                    };
                    open.finisher.finish(usage).await;
                    return Some((Ok(StreamEvent::Done(completion)), state));
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(error))) => {
                    state.ended = true;
                    let error = error.into_ai_error(&provider);
                    tracing::warn!(target: "navree::ai", %error, "AI stream failed");
                    return Some((Err(error), state));
                }
                Ok(None) => {
                    state.ended = true;
                    return Some((
                        Err(AiError::Unavailable {
                            provider,
                            message: "the stream ended before the answer finished".to_owned(),
                        }),
                        state,
                    ));
                }
                Err(_) => {
                    state.ended = true;
                    return Some((
                        Err(AiError::Unavailable {
                            provider,
                            message: format!("no event within {} s", gap.as_secs_f64()),
                        }),
                        state,
                    ));
                }
            }
        }
    }))
}

/// Merges text within each `window`.
pub(crate) fn chunk_events(events: EventStream, window: Duration) -> EventStream {
    struct State {
        events: EventStream,
        buffer: String,
        deadline: Option<Instant>,
        pending: VecDeque<Result<StreamEvent, AiError>>,
        ended: bool,
    }
    let state = State {
        events,
        buffer: String::new(),
        deadline: None,
        pending: VecDeque::new(),
        ended: false,
    };
    Box::pin(futures_util::stream::unfold(state, move |mut state| async move {
        loop {
            if let Some(item) = state.pending.pop_front() {
                return Some((item, state));
            }
            if state.ended {
                if state.buffer.is_empty() {
                    return None;
                }
                let text = std::mem::take(&mut state.buffer);
                return Some((Ok(StreamEvent::Text(text)), state));
            }
            let next = match state.deadline {
                Some(deadline) => match tokio::time::timeout_at(deadline, state.events.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        state.deadline = None;
                        let text = std::mem::take(&mut state.buffer);
                        return Some((Ok(StreamEvent::Text(text)), state));
                    }
                },
                None => state.events.next().await,
            };
            match next {
                Some(Ok(StreamEvent::Text(text))) => {
                    if state.buffer.is_empty() {
                        state.deadline = Some(Instant::now() + window);
                    }
                    state.buffer.push_str(&text);
                }
                Some(other) => {
                    state.deadline = None;
                    if state.buffer.is_empty() {
                        return Some((other, state));
                    }
                    state.pending.push_back(other);
                    let text = std::mem::take(&mut state.buffer);
                    return Some((Ok(StreamEvent::Text(text)), state));
                }
                None => state.ended = true,
            }
        }
    }))
}

/// Server-sent events for `events`: `event: <name>` and JSON `data:`; an
/// error is `event: error` with Navree's error body and ends the stream.
/// Sets `Cache-Control: no-cache` and `X-Accel-Buffering: no`, with a
/// keep-alive comment every 15 seconds.
pub(crate) fn sse_response<S>(events: S) -> Response
where
    S: Stream<Item = Result<StreamEvent, AiError>> + Send + 'static,
{
    let events = Box::pin(events);
    let sse = futures_util::stream::unfold((events, false), |(mut events, ended)| async move {
        if ended {
            return None;
        }
        match events.next().await {
            Some(Ok(event)) => {
                let sse = Event::default().event(event.name()).data(event.data().to_string());
                Some((Ok::<Event, Infallible>(sse), (events, false)))
            }
            Some(Err(error)) => {
                let body = error.to_app_error().body();
                let sse = Event::default().event("error").data(body.to_string());
                Some((Ok(sse), (events, true)))
            }
            None => None,
        }
    });
    let mut response = Sse::new(sse)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response();
    let headers = response.headers_mut();
    headers.insert("cache-control", HeaderValue::from_static("no-cache"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::{Finish, Usage};

    fn completion() -> Completion {
        Completion {
            model: "fast".into(),
            provider: "anthropic".into(),
            usage: Usage::new(3, 2),
            finish: Finish::Stop,
            conversation_id: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn chunked_merges_text_within_the_window() {
        let events: EventStream = Box::pin(
            futures_util::stream::iter([
                Ok(StreamEvent::Text("a".into())),
                Ok(StreamEvent::Text("b".into())),
                Ok(StreamEvent::Text("c".into())),
                Ok(StreamEvent::Done(completion())),
            ])
            .then(|event| async move {
                tokio::time::sleep(Duration::from_millis(40)).await;
                event
            }),
        );
        let out: Vec<StreamEvent> = chunk_events(events, Duration::from_millis(100))
            .map(Result::unwrap)
            .collect()
            .await;
        assert_eq!(
            out,
            [
                StreamEvent::Text("abc".into()),
                StreamEvent::Done(completion())
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn chunked_flushes_when_the_window_closes() {
        let events: EventStream = Box::pin(
            futures_util::stream::iter([
                (10, Ok(StreamEvent::Text("a".into()))),
                (10, Ok(StreamEvent::Text("b".into()))),
                (200, Ok(StreamEvent::Text("c".into()))),
                (10, Ok(StreamEvent::Done(completion()))),
            ])
            .then(|(delay, event)| async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                event
            }),
        );
        let out: Vec<StreamEvent> = chunk_events(events, Duration::from_millis(100))
            .map(Result::unwrap)
            .collect()
            .await;
        assert_eq!(
            out,
            [
                StreamEvent::Text("ab".into()),
                StreamEvent::Text("c".into()),
                StreamEvent::Done(completion())
            ]
        );
    }

    #[test]
    fn events_have_names_and_data() {
        assert_eq!(StreamEvent::Text("x".into()).data(), json!({ "text": "x" }));
        let done = StreamEvent::Done(completion());
        assert_eq!(done.name(), "done");
        assert_eq!(done.data()["usage"]["output_tokens"], 2);
        assert_eq!(done.data()["finish"], "stop");
        assert!(done.data().get("conversation_id").is_none());
        let result = StreamEvent::ToolResult {
            id: "c1".into(),
            tool: "search_deals".into(),
            ok: true,
        };
        assert_eq!(result.name(), "tool_result");
        assert_eq!(result.data(), json!({ "id": "c1", "tool": "search_deals", "ok": true }));
    }
}
