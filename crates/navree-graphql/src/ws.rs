//! Subscriptions over WebSockets at `<path>/ws`, speaking
//! `graphql-transport-ws` and the older `graphql-ws`.
//!
//! - The client authenticates in the `connection_init` payload
//!   (`{"authorization": "Bearer <token>"}`) with the app's authenticator;
//!   without one, the upgrade request's own authentication is used, and a
//!   session cookie only counts when the `Origin` is one of
//!   `http.cors.allowed_origins`.
//! - A client that sends no `connection_init` within
//!   `graphql.connection_init_timeout_secs` is closed with 4408; a refused
//!   one with 4403; when the token's `exp` passes, the socket closes with
//!   4401 so the client reconnects with a fresh token.
//! - The server pings every `graphql.keep_alive_secs`.

use std::future::{pending, ready};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_graphql::Data;
use async_graphql::http::{WebSocket as GraphqlSocket, WebSocketProtocols as Protocols, WsMessage};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::{SinkExt, StreamExt};
use http::header::{AUTHORIZATION, COOKIE, ORIGIN, SEC_WEBSOCKET_PROTOCOL};
use http::request::Parts;
use http::{HeaderMap, HeaderValue};
use navree_contracts::{AuthState, AuthVia, Principal};
use navree_http::response::Response as HttpResponse;
use navree_http::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use tokio::sync::watch;
use tokio::time::{Instant, Interval, Sleep};

use crate::endpoint::origin;
use crate::operation::Origin;
use crate::schema::{Connection, ConnectionViewer, GraphqlSchema, Shared};

/// Close code: the token expired (the client reconnects with a fresh one).
pub(crate) const CLOSE_UNAUTHORIZED: u16 = 4401;
/// Close code: `connection_init` was refused.
pub(crate) const CLOSE_FORBIDDEN: u16 = 4403;
/// Close code: no `connection_init` in time.
pub(crate) const CLOSE_INIT_TIMEOUT: u16 = 4408;

/// `GET <path>/ws`: upgrades to a GraphQL WebSocket.
pub(crate) fn upgrade(
    schema: &GraphqlSchema,
    parts: Parts,
    upgrade: WebSocketUpgrade,
) -> HttpResponse {
    let protocol = offered_protocol(&parts.headers).unwrap_or(Protocols::GraphQLWS);
    let shared = schema.shared().clone();
    let mut origin = origin(&parts);
    let via_session = origin
        .principal
        .as_ref()
        .is_some_and(|principal| principal.via == AuthVia::Session);
    if via_session && !origin_allowed(&parts.headers, &shared.allowed_origins) {
        origin.principal = None;
        origin.extensions.insert(AuthState(None));
    }
    let expires = origin
        .principal
        .as_ref()
        .and(parts.headers.get(AUTHORIZATION))
        .and_then(|value| value.to_str().ok())
        .and_then(token_expiry);
    let schema = schema.clone();
    upgrade
        .protocols(["graphql-transport-ws", "graphql-ws"])
        .on_upgrade(move |socket| serve(socket, schema, shared, protocol, origin, expires))
}

/// The first protocol of `Sec-WebSocket-Protocol` this endpoint speaks.
fn offered_protocol(headers: &HeaderMap) -> Option<Protocols> {
    headers
        .get_all(SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .find_map(|protocol| Protocols::from_str(protocol.trim()).ok())
}

/// Whether the request's `Origin` is one of `allowed`.
fn origin_allowed(headers: &HeaderMap, allowed: &[String]) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| allowed.iter().any(|allowed| allowed == origin))
}

/// When the bearer token in `authorization` (a JWT) expires: its `exp`
/// claim. `None` for other tokens.
pub(crate) fn token_expiry(authorization: &str) -> Option<SystemTime> {
    let token = authorization
        .strip_prefix("Bearer ")
        .or_else(|| authorization.strip_prefix("bearer "))
        .unwrap_or(authorization)
        .trim();
    let mut segments = token.split('.');
    let (_, payload, _, None) = (
        segments.next()?,
        segments.next()?,
        segments.next()?,
        segments.next(),
    ) else {
        return None;
    };
    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    let exp = claims.get("exp")?.as_u64()?;
    Some(UNIX_EPOCH + Duration::from_secs(exp))
}

/// The `authorization` member of a `connection_init` payload (any case).
fn authorization(payload: &serde_json::Value) -> Option<&str> {
    payload
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, value)| value.as_str())
}

/// How `connection_init` went.
#[derive(Clone, Copy, Debug)]
enum Init {
    Accepted { expires: Option<SystemTime> },
    Refused,
}

/// Authenticates a `connection_init` payload: its token with the app's
/// authenticator, else the upgrade request's viewer.
async fn authenticate(
    shared: &Shared,
    origin: &Origin,
    payload: &serde_json::Value,
    upgrade_expiry: Option<SystemTime>,
) -> Result<(Option<Principal>, Option<SystemTime>), String> {
    let Some(header) = authorization(payload) else {
        return Ok((origin.principal.clone(), upgrade_expiry));
    };
    let authenticator = shared
        .authenticator
        .as_ref()
        .ok_or_else(|| "Forbidden".to_owned())?;
    let value = HeaderValue::from_str(header).map_err(|_| "Invalid authorization".to_owned())?;
    let (mut parts, ()) = http::Request::new(()).into_parts();
    parts.uri = origin.uri.clone();
    parts.headers = origin.headers.clone();
    parts.headers.remove(COOKIE);
    parts.headers.insert(AUTHORIZATION, value);
    parts.extensions = origin.extensions.clone();
    parts.extensions.remove::<AuthState>();
    match authenticator.authenticate(&mut parts).await {
        Ok(Some(principal)) => Ok((Some(principal), token_expiry(header))),
        Ok(None) => Err("Forbidden".to_owned()),
        Err(error) => Err(error.detail.unwrap_or(error.title)),
    }
}

/// Runs one connection until either side closes it.
async fn serve(
    socket: WebSocket,
    schema: GraphqlSchema,
    shared: Arc<Shared>,
    protocol: Protocols,
    origin: Origin,
    upgrade_expiry: Option<SystemTime>,
) {
    let options = shared.options.clone();
    let (mut sink, source) = socket.split();
    let input = source
        .take_while(|message| ready(matches!(message, Ok(m) if !matches!(m, Message::Close(_)))))
        .filter_map(|message| {
            ready(match message {
                Ok(Message::Text(text)) => Some(text.as_str().as_bytes().to_vec()),
                Ok(Message::Binary(bytes)) => Some(bytes.to_vec()),
                _ => None,
            })
        });
    let (init_tx, mut init_rx) = watch::channel::<Option<Init>>(None);
    let mut data = Data::default();
    data.insert(Connection(origin.clone()));
    let init_shared = shared.clone();
    let graphql = GraphqlSocket::new(schema, input, protocol)
        .connection_data(data)
        .on_connection_init(move |payload| async move {
            match authenticate(&init_shared, &origin, &payload, upgrade_expiry).await {
                Ok((principal, expires)) => {
                    let _ = init_tx.send(Some(Init::Accepted { expires }));
                    let mut data = Data::default();
                    data.insert(ConnectionViewer(principal));
                    Ok(data)
                }
                Err(message) => {
                    let _ = init_tx.send(Some(Init::Refused));
                    Err(async_graphql::Error::new(message))
                }
            }
        });
    let mut graphql = Box::pin(graphql);
    let init_deadline =
        tokio::time::sleep(Duration::from_secs(options.connection_init_timeout_secs));
    tokio::pin!(init_deadline);
    let mut keep_alive = (options.keep_alive_secs > 0).then(|| {
        let period = Duration::from_secs(options.keep_alive_secs);
        tokio::time::interval_at(Instant::now() + period, period)
    });
    let mut expiry: Option<Pin<Box<Sleep>>> = None;
    let mut init: Option<Init> = None;
    loop {
        tokio::select! {
            message = graphql.next() => match message {
                Some(WsMessage::Text(text)) => {
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Some(WsMessage::Close(code, reason)) => {
                    // The init outcome may not have reached `init` yet.
                    let refused = matches!(*init_rx.borrow(), Some(Init::Refused));
                    let code = if refused && code == 1002 { CLOSE_FORBIDDEN } else { code };
                    close(&mut sink, code, &reason).await;
                    break;
                }
                None => {
                    let _ = sink.close().await;
                    break;
                }
            },
            changed = init_rx.changed(), if init.is_none() => {
                if changed.is_err() {
                    continue;
                }
                let outcome = *init_rx.borrow();
                if let Some(Init::Accepted { expires: Some(at) }) = outcome {
                    expiry = Some(Box::pin(tokio::time::sleep_until(instant_at(at))));
                }
                init = outcome;
            }
            () = &mut init_deadline, if init.is_none() => {
                close(&mut sink, CLOSE_INIT_TIMEOUT, "Connection initialisation timeout").await;
                break;
            }
            () = tick(keep_alive.as_mut()) => {
                let ping = match protocol {
                    Protocols::GraphQLWS => r#"{"type":"ping"}"#,
                    Protocols::SubscriptionsTransportWS => r#"{"type":"ka"}"#,
                };
                if sink.send(Message::Text(ping.into())).await.is_err() {
                    break;
                }
            }
            () = wait(expiry.as_mut()) => {
                close(&mut sink, CLOSE_UNAUTHORIZED, "Unauthorized").await;
                break;
            }
        }
    }
}

async fn close<S>(sink: &mut S, code: u16, reason: &str)
where
    S: futures_util::Sink<Message> + Unpin,
{
    let frame = CloseFrame {
        code,
        reason: reason.to_owned().into(),
    };
    let _ = sink.send(Message::Close(Some(frame))).await;
    let _ = sink.close().await;
}

/// The next tick of `interval`; never without one.
async fn tick(interval: Option<&mut Interval>) {
    match interval {
        Some(interval) => {
            interval.tick().await;
        }
        None => pending::<()>().await,
    }
}

/// `sleep`'s deadline; never without one.
async fn wait(sleep: Option<&mut Pin<Box<Sleep>>>) {
    match sleep {
        Some(sleep) => sleep.as_mut().await,
        None => pending::<()>().await,
    }
}

/// The `Instant` of the wall-clock time `at`.
fn instant_at(at: SystemTime) -> Instant {
    match at.duration_since(SystemTime::now()) {
        Ok(left) => Instant::now() + left,
        Err(_) => Instant::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(claims: &serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        format!("{header}.{payload}.signature")
    }

    #[test]
    fn expiry_comes_from_the_exp_claim() {
        let token = jwt(&serde_json::json!({ "sub": "u", "exp": 2_000_000_000_u64 }));
        assert_eq!(
            token_expiry(&format!("Bearer {token}")),
            Some(UNIX_EPOCH + Duration::from_secs(2_000_000_000))
        );
        assert_eq!(token_expiry("Bearer opaque-token"), None);
        let no_exp = jwt(&serde_json::json!({ "sub": "u" }));
        assert_eq!(token_expiry(&no_exp), None);
    }

    #[test]
    fn the_init_payload_names_authorization_in_any_case() {
        let payload = serde_json::json!({ "Authorization": "Bearer x" });
        assert_eq!(authorization(&payload), Some("Bearer x"));
        assert_eq!(authorization(&serde_json::json!({})), None);
        assert_eq!(authorization(&serde_json::Value::Null), None);
    }

    #[test]
    fn protocols_are_negotiated_from_the_offer() {
        let mut headers = HeaderMap::new();
        assert!(offered_protocol(&headers).is_none());
        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("chat, graphql-ws"),
        );
        assert_eq!(
            offered_protocol(&headers),
            Some(Protocols::SubscriptionsTransportWS)
        );
        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("graphql-transport-ws"),
        );
        assert_eq!(offered_protocol(&headers), Some(Protocols::GraphQLWS));
    }

    #[test]
    fn session_viewers_need_an_allowed_origin() {
        let allowed = vec!["http://localhost:3000".to_owned()];
        let mut headers = HeaderMap::new();
        assert!(!origin_allowed(&headers, &allowed));
        headers.insert(ORIGIN, HeaderValue::from_static("http://evil.example"));
        assert!(!origin_allowed(&headers, &allowed));
        headers.insert(ORIGIN, HeaderValue::from_static("http://localhost:3000"));
        assert!(origin_allowed(&headers, &allowed));
    }
}
