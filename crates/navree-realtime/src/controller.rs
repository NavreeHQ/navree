//! The realtime routes: the WebSocket (`GET /app/{key}`) and the channel
//! auth endpoint (`POST /broadcasting/auth`).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Extension;
use axum::body::Bytes;
use axum::extract::Path;
use axum::extract::ws::WebSocketUpgrade;
use axum::response::Response;
use http::header::{CONTENT_TYPE, HOST, ORIGIN};
use http::{HeaderMap, Method};
use navree_contracts::{AuthState, Principal};
use navree_http::{ApiMeta, AppError, Controller, CurrentPrincipal, Json, RouteDef};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::Value;

use crate::Realtime;
use crate::protocol;
use crate::realtime::AuthFailure;
use crate::socket::{self, Admission, MAX_MESSAGE_BYTES};

/// Serves the realtime routes.
pub(crate) struct RealtimeController {
    realtime: Arc<Realtime>,
}

impl Injectable for RealtimeController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Realtime>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                realtime: r.get::<Realtime>().await?,
            })
        })
    }
}

impl Controller for RealtimeController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let realtime = self.realtime.clone();
        let connect = RouteDef::on(
            Method::GET,
            "!/app/{key}",
            move |upgrade: WebSocketUpgrade,
                  Path(key): Path<String>,
                  headers: HeaderMap,
                  auth: Option<Extension<AuthState>>| {
                let realtime = realtime.clone();
                async move { connect(realtime, upgrade, &key, &headers, auth.map(|a| a.0)) }
            },
        )
        .public()
        .name("realtime.connect")
        .api(ApiMeta {
            summary: Some("Pusher protocol 7 WebSocket".to_owned()),
            tags: vec!["realtime".to_owned()],
            ..ApiMeta::default()
        });

        let realtime = self.realtime.clone();
        let auth = RouteDef::on(
            Method::POST,
            "!/broadcasting/auth",
            move |Extension(ctx): Extension<AppContext>,
                  CurrentPrincipal(principal): CurrentPrincipal,
                  headers: HeaderMap,
                  body: Bytes| {
                let realtime = realtime.clone();
                async move { authorize(&realtime, &ctx, &principal, &headers, &body).await }
            },
        )
        .authenticated()
        .name("realtime.auth")
        .api(ApiMeta {
            summary: Some("Authorize a private or presence channel subscription".to_owned()),
            tags: vec!["realtime".to_owned()],
            responses: vec![400, 401, 403],
            ..ApiMeta::default()
        });

        vec![connect, auth]
    }
}

/// `GET /app/{key}`: upgrades, then serves the connection or, for an
/// unknown key (4001) or a disallowed origin (4009), sends `pusher:error`
/// and closes.
fn connect(
    realtime: Arc<Realtime>,
    upgrade: WebSocketUpgrade,
    key: &str,
    headers: &HeaderMap,
    auth: Option<AuthState>,
) -> Response {
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let origin = header(ORIGIN);
    let admission = if key != realtime.options().key {
        tracing::debug!(target: "navree::realtime", "connection for an unknown app key");
        Admission::Reject {
            code: protocol::APP_DOES_NOT_EXIST,
            message: "Application does not exist",
        }
    } else if !realtime.origin_allowed(origin, header(HOST)) {
        tracing::warn!(
            target: "navree::security",
            origin = origin.unwrap_or_default(),
            "realtime connection from a disallowed origin"
        );
        Admission::Reject {
            code: protocol::UNAUTHORIZED,
            message: "Origin not allowed",
        }
    } else {
        Admission::Accept {
            user: auth
                .and_then(|state| state.0)
                .map(|principal| principal.user_id.to_string()),
        }
    };
    // Graceful shutdown waits for the connection's task.
    let token = realtime.tracker().token();
    upgrade
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |websocket| async move {
            let _token = token;
            socket::run(realtime, websocket, admission).await;
        })
}

/// The body of `POST /broadcasting/auth`.
#[derive(Debug, Deserialize)]
struct AuthRequest {
    socket_id: Option<String>,
    channel_name: Option<String>,
}

/// `POST /broadcasting/auth`: `{auth}` / `{auth, channel_data}`, 403 when the
/// channel's authorizer denies (or none matches), 400 for malformed fields.
async fn authorize(
    realtime: &Realtime,
    ctx: &AppContext,
    principal: &Principal,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Json<Value>, AppError> {
    let is_json = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|content_type| {
            let essence = content_type.split(';').next().unwrap_or_default().trim();
            essence.eq_ignore_ascii_case("application/json") || essence.ends_with("+json")
        });
    let request: AuthRequest = if is_json {
        serde_json::from_slice(body).ok()
    } else {
        serde_urlencoded::from_bytes(body).ok()
    }
    .ok_or_else(|| {
        AppError::bad_request("Send socket_id and channel_name as a form or a JSON object.")
    })?;

    let mut errors = BTreeMap::new();
    let mut required = |field: &str, value: Option<String>| {
        let value = value.filter(|v| !v.is_empty());
        if value.is_none() {
            errors.insert(
                field.to_owned(),
                vec![format!("{field} should not be empty")],
            );
        }
        value.unwrap_or_default()
    };
    let socket_id = required("socket_id", request.socket_id);
    let channel = required("channel_name", request.channel_name);
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }

    match realtime
        .authorize(principal, &socket_id, &channel, ctx)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(AuthFailure::Invalid { field, message }) => Err(AppError::validation(BTreeMap::from(
            [(field.to_owned(), vec![message.to_owned()])],
        ))),
        Err(AuthFailure::Forbidden) => Err(AppError::forbidden()),
        Err(AuthFailure::Failed(source)) => Err(AppError::internal(source)),
    }
}
