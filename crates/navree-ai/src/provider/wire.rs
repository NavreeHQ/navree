//! What the drivers share: sending JSON through the HTTP client, mapping
//! statuses to [`ProviderError`]s, and parsing server-sent events and
//! newline-delimited JSON.

use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt};
use http::{HeaderMap, StatusCode};
use navree_http_client::{HttpClientError, RequestBuilder, StreamingResponse};
use serde_json::Value;

use crate::error::ProviderError;

/// Sends `request` (JSON already set) and parses a JSON answer; a status
/// of 400 or above is classified with `message_of`.
pub(crate) async fn send_json(
    request: RequestBuilder,
    timeout: Duration,
    message_of: fn(&Value) -> Option<String>,
) -> Result<Value, ProviderError> {
    let response = request
        .retries(0)
        .timeout(timeout)
        .send()
        .await
        .map_err(transport)?;
    let status = response.status();
    let body = response.bytes();
    if status.as_u16() >= 400 {
        return Err(classify(status, response.headers(), &body, message_of));
    }
    serde_json::from_slice(&body)
        .map_err(|error| ProviderError::unavailable(format!("unreadable answer: {error}")))
}

/// Sends `request` for a streamed answer and returns the open response; a
/// status of 400 or above is read whole and classified.
pub(crate) async fn open_stream(
    request: RequestBuilder,
    timeout: Duration,
    message_of: fn(&Value) -> Option<String>,
) -> Result<StreamingResponse, ProviderError> {
    let response = request
        .timeout(timeout)
        .send_streaming()
        .await
        .map_err(transport)?;
    let status = response.status();
    if status.as_u16() >= 400 {
        let headers = response.headers().clone();
        let body = response.bytes().await.unwrap_or_default();
        return Err(classify(status, &headers, &body, message_of));
    }
    Ok(response)
}

/// A transport failure: everything but a refused destination or a bad
/// request can be retried.
pub(crate) fn transport(error: HttpClientError) -> ProviderError {
    match error {
        HttpClientError::Blocked { .. } | HttpClientError::Build(_) => {
            ProviderError::request(error.to_string())
        }
        other => ProviderError::unavailable(other.to_string()),
    }
}

/// The [`ProviderError`] of an error status: 429 rate limited, 401 and 403
/// a key problem, 408, 409 and 5xx unavailable, other 4xx an invalid
/// request (`message_of` reads the vendor's message from its error body).
pub(crate) fn classify(
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
    message_of: fn(&Value) -> Option<String>,
) -> ProviderError {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| message_of(&value))
        .unwrap_or_else(|| String::from_utf8_lossy(&body[..body.len().min(500)]).into_owned());
    let message = format!("{status}: {message}");
    match status.as_u16() {
        429 => ProviderError::RateLimited {
            retry_after: retry_after(headers),
        },
        401 | 403 => ProviderError::Auth { message },
        408 | 409 | 425 => ProviderError::Unavailable { message },
        code if code >= 500 => ProviderError::Unavailable { message },
        _ => ProviderError::Request { message },
    }
}

/// `retry-after-ms`, or `retry-after` in seconds.
pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    if let Some(ms) = header("retry-after-ms").and_then(|v| v.trim().parse::<f64>().ok()) {
        return Some(Duration::from_millis(ms.max(0.0) as u64));
    }
    header("retry-after")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map(|secs| Duration::from_millis((secs.max(0.0) * 1000.0) as u64))
}

/// `error.message` (OpenAI, Anthropic, Gemini, Mistral), `message`, or
/// `error` as a string (Ollama).
pub(crate) fn common_message(value: &Value) -> Option<String> {
    value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("error").filter(|e| e.is_string()))
        .or_else(|| value.get("Message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// One server-sent event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SseEvent {
    /// The `event:` field (`None` for the default `message`).
    pub(crate) event: Option<String>,
    /// The `data:` lines, joined with `\n`.
    pub(crate) data: String,
}

/// An incremental server-sent events parser.
#[derive(Debug, Default)]
pub(crate) struct SseParser {
    buffer: BytesMut,
}

impl SseParser {
    /// Feeds bytes; returns the events they complete.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some((end, separator)) = find_blank_line(&self.buffer) {
            let block = self.buffer.split_to(end + separator);
            let text = String::from_utf8_lossy(&block[..end]);
            if let Some(event) = parse_block(&text) {
                events.push(event);
            }
        }
        events
    }

    /// The last event, when the stream ended without a blank line.
    pub(crate) fn finish(&mut self) -> Option<SseEvent> {
        let rest = std::mem::take(&mut self.buffer);
        let text = String::from_utf8_lossy(&rest);
        parse_block(text.trim_end())
    }
}

/// The first blank line: its start and its length (`\n\n`, `\r\n\r\n`).
fn find_blank_line(buffer: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buffer.len() {
        if buffer[i..].starts_with(b"\r\n\r\n") {
            return Some((i, 4));
        }
        if buffer[i..].starts_with(b"\n\n") || buffer[i..].starts_with(b"\r\r") {
            return Some((i, 2));
        }
        i += 1;
    }
    None
}

/// One event block; `None` for comments and empty blocks.
fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event = SseEvent::default();
    let mut data = Vec::new();
    for line in block.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => event.event = Some(value.to_owned()),
            "data" => data.push(value.to_owned()),
            _ => {}
        }
    }
    if data.is_empty() && event.event.is_none() {
        return None;
    }
    event.data = data.join("\n");
    Some(event)
}

/// The response body as server-sent events.
pub(crate) fn sse_events(
    response: StreamingResponse,
) -> impl Stream<Item = Result<SseEvent, ProviderError>> + Send {
    framed(response, SseParser::default(), SseParser::feed, SseParser::finish)
}

/// An incremental newline-delimited JSON parser (Ollama streams).
#[derive(Debug, Default)]
pub(crate) struct LineParser {
    buffer: BytesMut,
}

impl LineParser {
    /// Feeds bytes; returns the complete non-empty lines.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line = self.buffer.split_to(end + 1);
            let text = String::from_utf8_lossy(&line).trim().to_owned();
            if !text.is_empty() {
                lines.push(text);
            }
        }
        lines
    }

    /// The last line, when the stream ended without a newline.
    pub(crate) fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buffer);
        let text = String::from_utf8_lossy(&rest).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }
}

/// The response body as lines.
pub(crate) fn lines(
    response: StreamingResponse,
) -> impl Stream<Item = Result<String, ProviderError>> + Send {
    framed(response, LineParser::default(), LineParser::feed, LineParser::finish)
}

/// The body of `response` cut into frames by `parser`.
pub(crate) fn framed<P, T>(
    response: StreamingResponse,
    parser: P,
    feed: fn(&mut P, &[u8]) -> Vec<T>,
    finish: fn(&mut P) -> Option<T>,
) -> impl Stream<Item = Result<T, ProviderError>> + Send
where
    P: Send + 'static,
    T: Send + 'static,
{
    let body = response.into_stream();
    futures_util::stream::unfold(
        (body, parser, std::collections::VecDeque::new(), false),
        move |(mut body, mut parser, mut pending, mut ended)| async move {
            loop {
                if let Some(item) = pending.pop_front() {
                    return Some((Ok(item), (body, parser, pending, ended)));
                }
                if ended {
                    return None;
                }
                match body.next().await {
                    Some(Ok(chunk)) => {
                        let chunk: Bytes = chunk;
                        pending.extend(feed(&mut parser, &chunk));
                    }
                    Some(Err(error)) => {
                        ended = true;
                        return Some((Err(transport(error)), (body, parser, pending, ended)));
                    }
                    None => {
                        ended = true;
                        pending.extend(finish(&mut parser));
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    #[test]
    fn sse_events_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"event: text\ndata: {\"a\"").is_empty());
        let events = parser.feed(b":1}\n\nevent: done\r\ndata: x\r\n\r\n: comment\n\ndata: tail");
        assert_eq!(
            events,
            [
                SseEvent {
                    event: Some("text".into()),
                    data: "{\"a\":1}".into()
                },
                SseEvent {
                    event: Some("done".into()),
                    data: "x".into()
                },
            ]
        );
        assert_eq!(
            parser.finish(),
            Some(SseEvent {
                event: None,
                data: "tail".into()
            })
        );
    }

    #[test]
    fn multi_line_data_joins_with_newlines() {
        let mut parser = SseParser::default();
        let events = parser.feed(b"data: one\ndata: two\n\n");
        assert_eq!(events[0].data, "one\ntwo");
    }

    #[test]
    fn lines_split_across_chunks() {
        let mut parser = LineParser::default();
        assert_eq!(parser.feed(b"{\"a\":1}\n{\"b\""), ["{\"a\":1}"]);
        assert_eq!(parser.feed(b":2}\n\n"), ["{\"b\":2}"]);
        assert_eq!(parser.feed(b"{\"c\":3}"), Vec::<String>::new());
        assert_eq!(parser.finish().as_deref(), Some("{\"c\":3}"));
    }

    #[test]
    fn statuses_classify() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("7"));
        let body = br#"{"error":{"message":"slow down"}}"#;
        assert_eq!(
            classify(StatusCode::TOO_MANY_REQUESTS, &headers, body, common_message),
            ProviderError::RateLimited {
                retry_after: Some(Duration::from_secs(7))
            }
        );
        assert!(matches!(
            classify(StatusCode::UNAUTHORIZED, &HeaderMap::new(), body, common_message),
            ProviderError::Auth { .. }
        ));
        assert!(matches!(
            classify(StatusCode::from_u16(529).unwrap(), &HeaderMap::new(), body, common_message),
            ProviderError::Unavailable { .. }
        ));
        match classify(StatusCode::BAD_REQUEST, &HeaderMap::new(), body, common_message) {
            ProviderError::Request { message } => assert!(message.contains("slow down")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn retry_after_reads_milliseconds_first() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("2"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(2)));
        headers.insert("retry-after-ms", HeaderValue::from_static("150"));
        assert_eq!(retry_after(&headers), Some(Duration::from_millis(150)));
    }
}
