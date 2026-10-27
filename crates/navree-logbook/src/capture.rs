//! The HTTP layer that captures request and response headers and bodies
//! for the `requests` watcher.

use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use http::HeaderMap;
use http::header::CONTENT_TYPE;
use http_body::Body as _;
use http_body_util::BodyExt;
use navree_contracts::RequestId;
use navree_http::{GlobalLayer, LayerSlot};

use crate::logbook::{Active, HttpCapture};
use crate::{CapturedBody, HttpMessage, Logbook};

/// Bodies larger than this are never read into memory (only their size
/// and type are kept).
const MAX_BUFFERED: u64 = 4 * 1024 * 1024;

/// The layer, in the outermost [`LayerSlot::Session`] position. It wraps
/// the router only when Logbook is on and records requests; otherwise the
/// router is left as it is.
pub(crate) fn layer(logbook: Arc<OnceLock<Arc<Logbook>>>) -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Session, "logbook", move |_ctx, router| {
        let Some(active) = logbook.get().and_then(|logbook| logbook.active().cloned()) else {
            return Ok(router);
        };
        if !active.options.watchers.requests {
            return Ok(router);
        }
        Ok(
            router.layer(middleware::from_fn(move |request: Request, next: Next| {
                capture(active.clone(), request, next)
            })),
        )
    })
}

/// Captures one exchange (or marks an ignored path) and hands it to the
/// Logbook, which joins it to the request's `RequestHandled` event.
async fn capture(active: Arc<Active>, request: Request, next: Next) -> Response {
    let Some(request_id) = request
        .extensions()
        .get::<RequestId>()
        .map(|id| id.as_str().to_owned())
    else {
        return next.run(request).await;
    };
    if active.ignores_path(request.uri().path()) {
        active.mark_ignored(&request_id);
        return next.run(request).await;
    }
    let options = &active.options.capture;
    let max = options.max_body_kb.saturating_mul(1024);
    let (parts, body) = request.into_parts();
    let (body, request_body) = if options.request_body {
        read_body(&parts.headers, body, max, &active).await
    } else {
        (body, None)
    };
    let request_message = HttpMessage {
        headers: active.redactor.headers(&parts.headers),
        query: parts.uri.query().map(|query| active.redactor.query(query)),
        body: request_body,
    };

    let response = next.run(Request::from_parts(parts, body)).await;

    let (parts, body) = response.into_parts();
    let streaming = parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));
    let (body, response_body) = if options.response_body && !streaming {
        read_body(&parts.headers, body, max, &active).await
    } else {
        (body, None)
    };
    let response_message = HttpMessage {
        headers: active.redactor.headers(&parts.headers),
        query: None,
        body: response_body,
    };
    active.store_capture(
        request_id,
        HttpCapture {
            request: request_message,
            response: response_message,
        },
    );
    Response::from_parts(parts, body)
}

/// Whether bodies of this content type are text worth storing.
pub(crate) fn is_textual(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    essence.starts_with("text/")
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
        || matches!(
            essence.as_str(),
            "application/json"
                | "application/xml"
                | "application/x-www-form-urlencoded"
                | "application/javascript"
                | "application/graphql"
                | "application/x-ndjson"
        )
}

/// Reads a body of known size (up to [`MAX_BUFFERED`]) and gives it back
/// whole, with what Logbook keeps of it: text cut at `max` bytes and
/// secrets masked; binary bodies keep only their size and type.
async fn read_body(
    headers: &HeaderMap,
    body: Body,
    max: usize,
    active: &Active,
) -> (Body, Option<CapturedBody>) {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let size = body.size_hint().exact();
    if size == Some(0) {
        return (body, None);
    }
    let textual = content_type.as_deref().is_none_or(is_textual);
    let readable = size.is_some_and(|size| size <= MAX_BUFFERED);
    if !textual || !readable {
        return (
            body,
            Some(CapturedBody {
                content_type,
                size,
                text: None,
                truncated: false,
            }),
        );
    }
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return (Body::empty(), None),
    };
    let text = std::str::from_utf8(&bytes)
        .ok()
        .map(|text| active.redactor.body(content_type.as_deref(), text));
    let (text, truncated) = match text {
        Some(text) => {
            let (cut, truncated) = cut(text, max);
            (Some(cut), truncated)
        }
        None => (None, false),
    };
    let captured = CapturedBody {
        content_type,
        size: Some(bytes.len() as u64),
        text,
        truncated,
    };
    (Body::from(bytes), Some(captured))
}

/// `text` cut to at most `max` bytes, at a character boundary.
pub(crate) fn cut(mut text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_types_are_captured_and_binary_ones_are_not() {
        assert!(is_textual("application/json; charset=utf-8"));
        assert!(is_textual("text/html"));
        assert!(is_textual("application/problem+json"));
        assert!(is_textual("application/x-www-form-urlencoded"));
        assert!(!is_textual("image/png"));
        assert!(!is_textual("multipart/form-data; boundary=x"));
        assert!(!is_textual("application/octet-stream"));
    }

    #[test]
    fn cuts_happen_at_character_boundaries() {
        assert_eq!(cut("hello".into(), 10), ("hello".into(), false));
        assert_eq!(cut("hello".into(), 3), ("hel".into(), true));
        assert_eq!(cut("héllo".into(), 2), ("h".into(), true));
    }
}
