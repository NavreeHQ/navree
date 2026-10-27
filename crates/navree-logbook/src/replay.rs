//! Replaying recorded requests through the app's router, and "Copy as
//! curl".

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use http::header::{AUTHORIZATION, HeaderName, HeaderValue};
use http::{Method, Request};
use http_body_util::BodyExt;
use navree_auth::{JwtService, TokenType};
use navree_contracts::RequestId;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use uuid::Uuid;

use crate::{Content, Logbook, LogbookError, RequestEntry};

/// How long the token of a replay "as the same user" is valid.
pub const REPLAY_TOKEN_TTL: Duration = Duration::from_secs(5 * 60);

/// Headers a replay never copies from the recording (the pipeline sets
/// them again).
const NOT_REPLAYED: [&str; 6] = [
    "host",
    "content-length",
    "x-request-id",
    "traceparent",
    "tracestate",
    "connection",
];

/// The largest response body a replay reads back.
const MAX_REPLY: usize = 8 * 1024 * 1024;

/// A replay: the recorded request, with any field edited. Every field is
/// optional; `ReplayRequest::default()` sends the request as recorded, as
/// the same user.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplayRequest {
    /// The method (`POST`).
    pub method: Option<String>,
    /// The path with its query string (`/api/v1/deals?page=2`).
    pub path: Option<String>,
    /// The headers, replacing the recorded ones.
    pub headers: Option<Vec<(String, String)>>,
    /// The body, replacing the recorded one.
    pub body: Option<String>,
    /// Send it as a guest instead of the user who made it.
    pub as_guest: bool,
    /// Confirms a `POST`, `PUT`, `PATCH` or `DELETE`, which changes data
    /// again.
    pub confirm: bool,
}

/// What a replay got back. The replay itself is recorded as a new batch,
/// tagged `replay`, whose request links to the original.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Replayed {
    /// The new batch (the replay's request id).
    pub batch_id: String,
    /// The replay's request entry, once recorded.
    pub entry_id: Option<String>,
    /// The response status.
    pub status: u16,
    /// The response headers.
    pub headers: Vec<(String, String)>,
    /// The response body, as text (lossy for binary bodies).
    pub body: String,
}

/// Whether replaying `method` changes data (it is not GET, HEAD, OPTIONS or
/// TRACE).
pub fn is_unsafe(method: &Method) -> bool {
    !matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

impl Logbook {
    /// Replays request `id` through the app's router, in-process, with the
    /// edits in `edit`, and records the result as a new batch tagged
    /// `replay`.
    ///
    /// When the app includes `AuthModule` and the request had a user, the
    /// replay carries a fresh access token for that user, valid for five
    /// minutes ([`REPLAY_TOKEN_TTL`]), unless `as_guest`. Unsafe methods
    /// need `confirm`.
    pub async fn replay(&self, id: &str, edit: ReplayRequest) -> Result<Replayed, LogbookError> {
        let active = self.active().ok_or(LogbookError::Disabled)?.clone();
        if !self.options().replay {
            return Err(LogbookError::ReplayOff);
        }
        let entry = self
            .entry(id)
            .await?
            .ok_or_else(|| LogbookError::NotFound(id.to_owned()))?;
        let Content::Request(original) = &entry.content else {
            return Err(LogbookError::NotARequest(id.to_owned()));
        };
        let method_name = edit
            .method
            .clone()
            .unwrap_or_else(|| original.method.clone());
        let method = Method::from_bytes(method_name.trim().to_ascii_uppercase().as_bytes())
            .map_err(|_| LogbookError::InvalidReplay(format!("`{method_name}` is not a method")))?;
        if is_unsafe(&method) && !edit.confirm {
            return Err(LogbookError::Unconfirmed {
                method: method.to_string(),
            });
        }
        let uri = edit.path.clone().unwrap_or_else(|| recorded_uri(original));
        if !uri.starts_with('/') {
            return Err(LogbookError::InvalidReplay(format!(
                "the path must start with `/` (got `{uri}`)"
            )));
        }
        let headers = edit
            .headers
            .clone()
            .unwrap_or_else(|| recorded_headers(original));
        let body = edit.body.clone().or_else(|| {
            original
                .request
                .as_ref()
                .and_then(|request| request.body.as_ref())
                .and_then(|body| body.text.clone())
        });

        let mut request = Request::builder()
            .method(method)
            .uri(&uri)
            .body(Body::from(body.unwrap_or_default()))
            .map_err(|error| LogbookError::InvalidReplay(error.to_string()))?;
        for (name, value) in &headers {
            if NOT_REPLAYED.contains(&name.to_ascii_lowercase().as_str()) {
                continue;
            }
            let name = HeaderName::from_bytes(name.trim().as_bytes()).map_err(|_| {
                LogbookError::InvalidReplay(format!("`{name}` is not a header name"))
            })?;
            let value = HeaderValue::from_str(value.trim()).map_err(|_| {
                LogbookError::InvalidReplay(format!("the value of `{name}` is not a header value"))
            })?;
            request.headers_mut().append(name, value);
        }
        let app = active
            .app
            .get()
            .and_then(navree_kernel::WeakAppContext::upgrade)
            .ok_or_else(|| LogbookError::Replay("the app has shut down".to_owned()))?;
        if !edit.as_guest
            && !request.headers().contains_key(AUTHORIZATION)
            && let Some(user) = entry
                .user_id
                .as_deref()
                .and_then(|u| Uuid::parse_str(u).ok())
            && let Some(jwt) = app.try_get::<JwtService>().await
        {
            let token = jwt
                .sign(user, TokenType::Access, REPLAY_TOKEN_TTL)
                .map_err(|error| LogbookError::Replay(error.to_string()))?;
            let value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| LogbookError::Replay(error.to_string()))?;
            request.headers_mut().insert(AUTHORIZATION, value);
        }
        let replay_id = RequestId::generate();
        request.headers_mut().insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_str(replay_id.as_str())
                .map_err(|error| LogbookError::Replay(error.to_string()))?,
        );
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))));
        active
            .replays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(replay_id.as_str().to_owned(), entry.id.clone());

        let router = active
            .router
            .get_or_try_init(|| async {
                navree_http::build_router(&app)
                    .await
                    .map_err(|error| LogbookError::Replay(error.to_string()))
            })
            .await?
            .clone();
        let response = router
            .oneshot(request)
            .await
            .unwrap_or_else(|never| match never {});
        let (parts, body) = response.into_parts();
        let bytes = http_body_util::Limited::new(body, MAX_REPLY)
            .collect()
            .await
            .map_err(|error| LogbookError::Replay(error.to_string()))?
            .to_bytes();
        let entry_id = self
            .batch(replay_id.as_str())
            .await
            .and_then(|batch| batch.head().map(|head| head.id.clone()));
        Ok(Replayed {
            batch_id: replay_id.as_str().to_owned(),
            entry_id,
            status: parts.status.as_u16(),
            headers: active.redactor.headers(&parts.headers),
            body: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }

    /// The request `id` as a `curl` command against the app's address
    /// (`http.addr`), without credentials.
    pub async fn curl(&self, id: &str) -> Result<String, LogbookError> {
        let active = self.active().ok_or(LogbookError::Disabled)?;
        let entry = self
            .entry(id)
            .await?
            .ok_or_else(|| LogbookError::NotFound(id.to_owned()))?;
        let Content::Request(request) = &entry.content else {
            return Err(LogbookError::NotARequest(id.to_owned()));
        };
        let base = active
            .base_url
            .get()
            .map_or("http://127.0.0.1:8080", String::as_str);
        Ok(curl_command(base, request))
    }
}

/// The recorded path and query string.
fn recorded_uri(request: &RequestEntry) -> String {
    match request
        .request
        .as_ref()
        .and_then(|message| message.query.as_deref())
    {
        Some(query) if !query.is_empty() => format!("{}?{query}", request.path),
        _ => request.path.clone(),
    }
}

/// The recorded request headers worth sending again.
fn recorded_headers(request: &RequestEntry) -> Vec<(String, String)> {
    request
        .request
        .as_ref()
        .map(|message| {
            message
                .headers
                .iter()
                .filter(|(name, _)| !NOT_REPLAYED.contains(&name.to_ascii_lowercase().as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// `curl` for `request` against `base` (`http://127.0.0.1:8080`).
pub(crate) fn curl_command(base: &str, request: &RequestEntry) -> String {
    let mut parts = vec!["curl".to_owned()];
    if request.method != "GET" {
        parts.push(format!("-X {}", request.method));
    }
    parts.push(quote(&format!(
        "{}{}",
        base.trim_end_matches('/'),
        recorded_uri(request)
    )));
    for (name, value) in recorded_headers(request) {
        if name.eq_ignore_ascii_case("accept-encoding") {
            continue;
        }
        parts.push(format!("-H {}", quote(&format!("{name}: {value}"))));
    }
    if let Some(text) = request
        .request
        .as_ref()
        .and_then(|message| message.body.as_ref())
        .and_then(|body| body.text.as_deref())
    {
        parts.push(format!("--data-raw {}", quote(text)));
    }
    parts.join(" \\\n  ")
}

/// A single-quoted shell word.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BatchSummary, CapturedBody, HttpMessage};

    fn post() -> RequestEntry {
        RequestEntry {
            method: "POST".into(),
            path: "/api/v1/deals".into(),
            route: Some("/api/v1/deals".into()),
            status: 201,
            request_id: "r".into(),
            client_ip: None,
            duration_ms: 3.0,
            request: Some(HttpMessage {
                headers: vec![
                    ("content-type".into(), "application/json".into()),
                    ("host".into(), "localhost".into()),
                ],
                query: Some("draft=1".into()),
                body: Some(CapturedBody {
                    content_type: Some("application/json".into()),
                    size: Some(15),
                    text: Some(r#"{"title":"O'Neil"}"#.into()),
                    truncated: false,
                }),
            }),
            response: None,
            replay_of: None,
            summary: BatchSummary::default(),
        }
    }

    #[test]
    fn curl_commands_quote_and_leave_out_transport_headers() {
        assert_eq!(
            curl_command("http://127.0.0.1:8080/", &post()),
            "curl \\\n  -X POST \\\n  'http://127.0.0.1:8080/api/v1/deals?draft=1' \\\n  \
             -H 'content-type: application/json' \\\n  --data-raw '{\"title\":\"O'\\''Neil\"}'"
        );
    }

    #[test]
    fn only_safe_methods_replay_without_confirmation() {
        assert!(!is_unsafe(&Method::GET));
        assert!(!is_unsafe(&Method::HEAD));
        assert!(is_unsafe(&Method::POST));
        assert!(is_unsafe(&Method::DELETE));
        assert!(is_unsafe(&Method::PATCH));
    }

    #[test]
    fn replay_requests_default_to_the_recording() {
        let edit: ReplayRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(edit, ReplayRequest::default());
        assert!(!edit.as_guest);
        assert!(!edit.confirm);
    }
}
