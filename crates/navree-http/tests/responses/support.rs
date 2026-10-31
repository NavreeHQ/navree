//! What the comparisons share.

use bytes::Bytes;
use http::{HeaderName, HeaderValue, StatusCode};
use http_body_util::BodyExt;
use navree_http::response::Response;

/// All of a response a client or an outer layer can see: its status, its
/// headers in order, its data frames one by one and how the body ended.
#[derive(Debug, PartialEq, Eq)]
pub struct Seen {
    pub status: StatusCode,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub frames: Vec<Bytes>,
    pub error: Option<String>,
    pub size_hint: (u64, Option<u64>),
}

/// Reads a whole response without gluing its frames together.
pub async fn seen<B>(response: http::Response<B>) -> Seen
where
    B: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
    B::Error: std::fmt::Display,
{
    let (parts, mut body) = response.into_parts();
    let hint = http_body::Body::size_hint(&body);
    let mut frames = Vec::new();
    let mut error = None;
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => frames.extend(frame.into_data().ok()),
            Err(failure) => {
                error = Some(failure.to_string());
                break;
            }
        }
    }
    Seen {
        status: parts.status,
        headers: parts
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        frames,
        error,
        size_hint: (hint.lower(), hint.upper()),
    }
}

/// [`seen`] of a response of Navree's.
pub async fn ours(response: Response) -> Seen {
    seen(response).await
}

/// [`seen`] of a response of the previous framework's.
pub async fn theirs(response: axum::response::Response) -> Seen {
    seen(response).await
}

/// One thread's runtime for a property test.
pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .expect("a runtime")
}
