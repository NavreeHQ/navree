//! Helpers for the live stream tests: a server-sent event reader over the
//! app's router, and a workspace resolver like the workspace module's.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use navree_contracts::{ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource};
use navree_http::body::Body;
use navree_kernel::{BoxError, BoxFuture, Factory, Module, ModuleBuilder};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use crate::support::Server;

/// How long a reader waits for an expected event.
const WAIT: Duration = Duration::from_secs(5);

/// One event read off a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub id: Option<String>,
    pub event: String,
    pub data: String,
}

/// A stream, read through the app's router.
pub struct SseClient {
    pub status: StatusCode,
    pub headers: HeaderMap,
    body: Body,
    buffer: String,
    ended: bool,
}

impl SseClient {
    /// The next event, skipping comments; `None` once the stream has ended.
    /// Panics when nothing comes within 5 s.
    pub async fn next(&mut self) -> Option<SseEvent> {
        self.next_within(WAIT)
            .await
            .unwrap_or_else(|| panic!("an event within {WAIT:?}"))
    }

    /// The next event within `wait`: `None` when nothing came, `Some(None)`
    /// when the stream ended.
    pub async fn next_within(&mut self, wait: Duration) -> Option<Option<SseEvent>> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(event) = self.parse_one() {
                return Some(Some(event));
            }
            if self.ended {
                return Some(None);
            }
            let frame = tokio::time::timeout_at(deadline, self.body.frame()).await.ok()?;
            match frame {
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        self.buffer.push_str(&String::from_utf8_lossy(&data));
                    }
                }
                Some(Err(_)) | None => self.ended = true,
            }
        }
    }

    /// Takes one complete event out of the buffer; comments are dropped.
    fn parse_one(&mut self) -> Option<SseEvent> {
        loop {
            let end = self.buffer.find("\n\n")?;
            let block: String = self.buffer.drain(..end + 2).collect();
            let mut event = SseEvent {
                id: None,
                event: "message".to_owned(),
                data: String::new(),
            };
            let mut has_data = false;
            for line in block.lines() {
                if line.starts_with(':') || line.is_empty() {
                    continue;
                }
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "id" => event.id = Some(value.to_owned()),
                    "event" => event.event = value.to_owned(),
                    "data" => {
                        if has_data {
                            event.data.push('\n');
                        }
                        event.data.push_str(value);
                        has_data = true;
                    }
                    _ => {}
                }
            }
            if has_data {
                return Some(event);
            }
        }
    }

    /// The next event, which must be `event`.
    pub async fn expect(&mut self, event: &str) -> SseEvent {
        let next = self
            .next()
            .await
            .unwrap_or_else(|| panic!("expected `{event}`, the stream ended"));
        assert_eq!(next.event, event, "{next:?}");
        next
    }

    /// No event within `wait` (keep-alive comments don't count).
    pub async fn expect_silence(&mut self, wait: Duration) {
        if let Some(next) = self.next_within(wait).await {
            panic!("expected silence, got {next:?}");
        }
    }

    /// The stream ends within 5 s, with no event before.
    pub async fn expect_end(&mut self) {
        match self.next_within(WAIT).await {
            Some(None) => {}
            Some(Some(event)) => panic!("expected the stream to end, got {event:?}"),
            None => panic!("expected the stream to end within {WAIT:?}"),
        }
    }

    /// The body of a refused stream, as JSON.
    pub async fn json(self) -> Value {
        let bytes = self.body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }
}

impl Server {
    /// Opens `GET /broadcasting/stream?{query}` as `user` (a guest when
    /// `None`) with `headers` (name, value).
    pub async fn stream(&self, query: &str, user: Option<Uuid>, headers: &[(&str, &str)]) -> SseClient {
        let mut request = Request::get(format!("/broadcasting/stream?{query}"))
            .header("accept", "text/event-stream");
        if let Some(user) = user {
            request = request.header("x-user-id", user.to_string());
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        SseClient {
            status,
            headers,
            body: response.into_body(),
            buffer: String::new(),
            ended: false,
        }
    }

    /// Opens a stream that must be served (200), and reads its first event.
    pub async fn listen(&self, query: &str, user: Option<Uuid>, headers: &[(&str, &str)]) -> (SseClient, SseEvent) {
        let mut stream = self.stream(query, user, headers).await;
        assert_eq!(stream.status, StatusCode::OK, "{query}");
        let first = stream.next().await.expect("a first event");
        (stream, first)
    }
}

/// The request's workspace as the workspace module resolves it, for the
/// tests: the one `x-workspace` names, when the signed-in user is a member.
/// The workspace module answers through the same contract (its
/// workspace-level `dyn ZoneSource`).
#[derive(Clone, Default)]
pub struct Workspaces {
    members: Arc<HashMap<Uuid, Vec<Uuid>>>,
}

impl Workspaces {
    /// Workspaces and their members.
    pub fn new(members: impl IntoIterator<Item = (Uuid, Vec<Uuid>)>) -> Self {
        Self {
            members: Arc::new(members.into_iter().collect()),
        }
    }

    /// A module registering them.
    pub fn module(&self) -> impl Module + use<> {
        WorkspacesModule(self.clone())
    }
}

impl ZoneSource for Workspaces {
    fn level(&self) -> ZoneLevel {
        ZoneLevel::Workspace
    }

    fn zone_of_request<'a>(
        &'a self,
        request: &'a mut ZoneRequest<'_>,
    ) -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            let Some(user) = request.user_id() else {
                return Ok(None);
            };
            let Some(parts) = request.head::<http::request::Parts>() else {
                return Ok(None);
            };
            let Some(named) = parts
                .headers
                .get("x-workspace")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| Uuid::parse_str(value).ok())
            else {
                return Ok(None);
            };
            let member = self
                .members
                .get(&named)
                .is_some_and(|members| members.contains(&user));
            Ok(member.then(|| ZoneOwner::new(named)))
        })
    }

    fn zone_of(&self, _owner: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async { Ok(None) })
    }
}

struct WorkspacesModule(Workspaces);

impl Module for WorkspacesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let source: Arc<dyn ZoneSource> = Arc::new(self.0.clone());
        m.multi(Factory::<dyn ZoneSource>::new(move |_| {
            let source = source.clone();
            async move { Ok(source) }
        }));
    }
}
