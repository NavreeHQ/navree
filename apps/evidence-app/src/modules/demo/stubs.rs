//! The routes the app calls itself, which need no sign-in.
//!
//! - `GET /api/demo/stub/{status}?delay_ms=N`: answers `status` after
//!   `delay_ms` milliseconds (at most 5000). The upstream the outgoing
//!   demo calls hit. `POST` answers the same, for endpoints that receive
//!   posts.
//! - `POST /api/demo/sink/alerts` and `POST /api/demo/sink/slack`: accept
//!   any body, answer 200 and log one line. Where alert channels and
//!   webhook endpoints are pointed.

use std::time::Duration;

use navree::http::{IntoResponse, Response};
use navree::openapi::ToSchema;
use navree::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The longest a stub waits before it answers, in milliseconds.
const MAX_DELAY_MS: u64 = 5_000;

/// The query of the stub routes.
#[derive(Debug, Default, Deserialize)]
pub struct DemoStubQuery {
    /// How long to wait before answering, in milliseconds (at most 5000).
    pub delay_ms: Option<u64>,
}

/// What a sink answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoSinkReceipt {
    /// Always `true`.
    pub received: bool,
}

/// The stub and the sinks.
#[injectable]
pub struct StubController;

#[controller("/demo")]
#[public]
impl StubController {
    /// `GET /api/demo/stub/{status}`
    #[get("/stub/{status}")]
    #[api(
        summary = "Answer with a chosen status, after a chosen delay",
        tag = "Demo",
        responses(400)
    )]
    async fn stub(
        &self,
        Path(status): Path<u16>,
        Query(query): Query<DemoStubQuery>,
    ) -> Result<Response> {
        answer(status, query).await
    }

    /// `POST /api/demo/stub/{status}`
    #[post("/stub/{status}")]
    #[api(
        summary = "Answer a post with a chosen status, after a chosen delay",
        tag = "Demo",
        responses(400)
    )]
    async fn stub_post(
        &self,
        Path(status): Path<u16>,
        Query(query): Query<DemoStubQuery>,
    ) -> Result<Response> {
        answer(status, query).await
    }

    /// `POST /api/demo/sink/alerts`
    #[post("/sink/alerts")]
    #[api(summary = "Receive an alert", tag = "Demo", response = DemoSinkReceipt)]
    async fn alerts(&self) -> Json<DemoSinkReceipt> {
        tracing::info!(target: "evidence::sink", channel = "alerts", "alert received");
        Json(DemoSinkReceipt { received: true })
    }

    /// `POST /api/demo/sink/slack`
    #[post("/sink/slack")]
    #[api(summary = "Receive a chat message", tag = "Demo", response = DemoSinkReceipt)]
    async fn slack(&self) -> Json<DemoSinkReceipt> {
        tracing::info!(target: "evidence::sink", channel = "slack", "chat message received");
        Json(DemoSinkReceipt { received: true })
    }
}

/// Waits, then answers `status`: a small JSON body, none for the statuses
/// that carry none.
async fn answer(status: u16, query: DemoStubQuery) -> Result<Response> {
    if !(200..=599).contains(&status) {
        return Err(AppError::bad_request(
            "The status must be a code from 200 to 599.",
        ));
    }
    let delay_ms = query.delay_ms.unwrap_or(0).min(MAX_DELAY_MS);
    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
    let mut response = if matches!(status, 204 | 205 | 304) {
        ().into_response()
    } else {
        Json(json!({ "stub": true, "status": status, "delay_ms": delay_ms })).into_response()
    };
    *response.status_mut() = status
        .try_into()
        .map_err(|_| AppError::bad_request("The status must be a valid HTTP status code."))?;
    Ok(response)
}
