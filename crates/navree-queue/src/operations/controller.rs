//! The routes of operations (`/api/operations...`), their SSE progress
//! stream and the authorizer of the private channel `operations.{id}`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use http::Method;
use navree_contracts::{BoxError, BoxFuture, Principal};
use navree_http::{
    ApiMeta, AppError, Authenticator, Controller, CurrentPrincipal, Json, RouteDef, SchemaFn,
    schema_fn,
};
use navree_kernel::{AppContext, Injectable, KernelError, ProviderKey, Resolver};
use navree_realtime::ChannelDecision;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::{OperationRecord, Operations};
use crate::QueueOptions;

/// The SSE event name of an operation snapshot.
pub const OPERATION_EVENT: &str = "operation";

/// How often an idle SSE stream sends a keep-alive comment.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// `GET /api/operations?limit=20`.
#[derive(Clone, Debug, Default, Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
struct DataList<T> {
    data: Vec<T>,
}

/// The routes of operations, under `{http.api_prefix}/operations`
/// (default `/api/operations`). They mount when `queue.operations.routes`
/// is true (the default) and the app has `DatabaseModule` and an
/// authenticator. Every route needs a signed-in user; an operation is
/// visible to its owner and to holders of `operations.view` (app-wide or
/// in its workspace), and is a 404 to everyone else.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /operations?limit=20` | `{data: [Operation]}`: the user's own operations, newest first |
/// | `GET /operations/{id}` | the operation |
/// | `GET /operations/{id}/events` | `text/event-stream`: an `operation` event now and after every change; ends after the finished state |
/// | `DELETE /operations/{id}` | the operation, cancelled (owner or `operations.cancel`; 409 `operation_finished` once finished) |
pub struct OperationsController {
    operations: Option<Arc<Operations>>,
    poll: Duration,
}

impl Injectable for OperationsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<QueueOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r.get::<QueueOptions>().await?;
            let app = r.app();
            let mounted = options.operations.routes
                && app.try_get::<dyn Authenticator>().await.is_some()
                && app.try_get::<navree_db::Db>().await.is_some();
            let operations = if mounted {
                app.try_get::<Operations>().await
            } else {
                None
            };
            Ok(OperationsController {
                operations,
                poll: options.operations.stream_poll_interval(),
            })
        })
    }
}

fn api(summary: &str, responses: &[u16], response: Option<SchemaFn>) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Operations".to_owned()],
        responses: responses.to_vec(),
        response,
        ..ApiMeta::default()
    }
}

/// The operation `id` when `principal` may see it, else a 404.
async fn visible(
    operations: &Operations,
    principal: &Principal,
    id: &str,
) -> Result<OperationRecord, AppError> {
    let id = Uuid::parse_str(id).map_err(|_| AppError::not_found())?;
    let record = operations
        .find(id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(AppError::not_found)?;
    if operations
        .can_view(principal, &record)
        .await
        .map_err(AppError::internal)?
    {
        Ok(record)
    } else {
        Err(AppError::not_found())
    }
}

impl Controller for OperationsController {
    fn prefix(&self) -> &'static str {
        "/operations"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let Some(operations) = self.operations.clone() else {
            return Vec::new();
        };
        let poll = self.poll;

        let ops = operations.clone();
        let index = move |CurrentPrincipal(principal): CurrentPrincipal,
                          Query(query): Query<ListQuery>| {
            let ops = ops.clone();
            async move {
                let limit = query.limit.unwrap_or(20).clamp(1, 100);
                let data = ops
                    .for_owner(principal.user_id, limit)
                    .await
                    .map_err(AppError::internal)?;
                Ok::<_, AppError>(Json(DataList { data }))
            }
        };

        let ops = operations.clone();
        let show = move |CurrentPrincipal(principal): CurrentPrincipal, Path(id): Path<String>| {
            let ops = ops.clone();
            async move {
                let record = visible(&ops, &principal, &id).await?;
                Ok::<_, AppError>(Json(record))
            }
        };

        let ops = operations.clone();
        let events = move |CurrentPrincipal(principal): CurrentPrincipal,
                           Path(id): Path<String>| {
            let ops = ops.clone();
            async move {
                let record = visible(&ops, &principal, &id).await?;
                let stream = snapshots(ops, record.id, poll);
                Ok::<_, AppError>(
                    Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)),
                )
            }
        };

        let ops = operations;
        let cancel = move |CurrentPrincipal(principal): CurrentPrincipal,
                           Path(id): Path<String>| {
            let ops = ops.clone();
            async move {
                let record = visible(&ops, &principal, &id).await?;
                if !ops
                    .can_cancel(&principal, &record)
                    .await
                    .map_err(AppError::internal)?
                {
                    return Err(AppError::forbidden());
                }
                if record.is_finished() {
                    return Err(AppError::conflict("The operation has already finished.")
                        .with_code("operation_finished"));
                }
                let record = ops
                    .cancel(record.id)
                    .await
                    .map_err(AppError::internal)?
                    .ok_or_else(AppError::not_found)?;
                Ok::<_, AppError>(Json(record))
            }
        };

        let record = Some(schema_fn::<OperationRecord> as SchemaFn);
        vec![
            RouteDef::on(Method::GET, "/", index)
                .authenticated()
                .name("operations.index")
                .api(api("The signed-in user's operations", &[401], None)),
            RouteDef::on(Method::GET, "/{id}", show)
                .authenticated()
                .name("operations.show")
                .api(api("An operation", &[401, 404], record)),
            RouteDef::on(Method::GET, "/{id}/events", events)
                .authenticated()
                .name("operations.events")
                .api(ApiMeta {
                    description: Some(
                        "Server-sent events: an `operation` event with the operation now and after \
                         every change. The stream ends after the finished state."
                            .to_owned(),
                    ),
                    ..api("Follow an operation's progress", &[401, 404], None)
                }),
            RouteDef::on(Method::DELETE, "/{id}", cancel)
                .authenticated()
                .name("operations.cancel")
                .api(api("Cancel an operation", &[401, 403, 404, 409], record)),
        ]
    }
}

/// The snapshots of operation `id`: one now, then one after every change
/// (announced in this process, or seen by polling every `poll`, for
/// changes made by workers in other processes); ends after the finished
/// state, or when the operation is gone.
fn snapshots(
    operations: Arc<Operations>,
    id: Uuid,
    poll: Duration,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    struct State {
        operations: Arc<Operations>,
        changes: broadcast::Receiver<Uuid>,
        id: Uuid,
        poll: Duration,
        version: Option<i64>,
        done: bool,
    }
    let state = State {
        changes: operations.subscribe(),
        operations,
        id,
        poll,
        version: None,
        done: false,
    };
    futures_util::stream::unfold(state, |mut state| async move {
        if state.done {
            return None;
        }
        loop {
            let record = match state.operations.find(state.id).await {
                Ok(Some(record)) => record,
                Ok(None) => return None,
                Err(error) => {
                    tracing::warn!(target: "navree::job", operation_id = %state.id, %error, "operation stream stopped");
                    return None;
                }
            };
            if state.version != Some(record.version) {
                state.version = Some(record.version);
                state.done = record.is_finished();
                let event = Event::default()
                    .event(OPERATION_EVENT)
                    .id(record.version.to_string())
                    .json_data(&record)
                    .unwrap_or_else(|_| Event::default().event(OPERATION_EVENT));
                return Some((Ok(event), state));
            }
            wait_for_change(&mut state.changes, state.id, state.poll).await;
        }
    })
}

/// Waits until operation `id` changes in this process, or `poll` passes.
async fn wait_for_change(changes: &mut broadcast::Receiver<Uuid>, id: Uuid, poll: Duration) {
    let deadline = tokio::time::sleep(poll);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            () = &mut deadline => return,
            changed = changes.recv() => match changed {
                Ok(changed) if changed == id => return,
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => return,
                Err(broadcast::error::RecvError::Closed) => {
                    (&mut deadline).await;
                    return;
                }
            },
        }
    }
}

/// The authorizer of the private channel `operations.{id}`: the users who
/// may see the operation.
pub(crate) fn authorize_channel(
    principal: &Principal,
    params: &HashMap<String, String>,
    ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    let principal = principal.clone();
    let id = params.get("id").cloned().unwrap_or_default();
    let ctx = ctx.clone();
    Box::pin(async move {
        let Ok(id) = Uuid::parse_str(&id) else {
            return Ok(ChannelDecision::Deny);
        };
        let Some(operations) = ctx.try_get::<Operations>().await else {
            return Ok(ChannelDecision::Deny);
        };
        let Some(record) = operations.find(id).await? else {
            return Ok(ChannelDecision::Deny);
        };
        if operations.can_view(&principal, &record).await? {
            Ok(ChannelDecision::Allow)
        } else {
            Ok(ChannelDecision::Deny)
        }
    })
}
