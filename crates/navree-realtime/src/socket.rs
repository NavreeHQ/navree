//! One WebSocket connection's task: the handshake answer, reading client
//! messages, writing queued frames, activity pings and closing.

use std::sync::Arc;
use std::time::Duration;

use navree_http::ws::{CloseFrame, Message, Utf8Bytes, WebSocket};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::Realtime;
use crate::protocol::{self, frame};
use crate::realtime::{CloseReason, Connection};

/// How long a single write may take before the client counts as gone.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest a client gets to answer the server's ping.
const PONG_GRACE: Duration = Duration::from_secs(30);

/// The largest message a client may send (Pusher's limit is 10 KiB for
/// client events; subscriptions are far smaller).
pub(crate) const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// What the handshake decided.
#[derive(Debug)]
pub(crate) enum Admission {
    /// Serve the connection, owned by `user` if the handshake was authenticated.
    Accept { user: Option<String> },
    /// Send `pusher:error` with `code` and `message`, then close.
    Reject { code: u16, message: &'static str },
}

/// Serves one upgraded connection until it closes.
pub(crate) async fn run(realtime: Arc<Realtime>, mut socket: WebSocket, admission: Admission) {
    let user = match admission {
        Admission::Accept { user } => user,
        Admission::Reject { code, message } => {
            close(&mut socket, CloseReason { code, message }).await;
            return;
        }
    };
    let (connection, mut outbox) = realtime.connect(user);
    tracing::debug!(target: "navree::realtime", socket_id = %connection.socket_id, "connection opened");
    let reason = serve(&realtime, &mut socket, &connection, &mut outbox).await;
    realtime.disconnect(&connection);
    if let Some(reason) = reason {
        close(&mut socket, reason).await;
    }
    tracing::debug!(
        target: "navree::realtime",
        socket_id = %connection.socket_id,
        close_code = reason.map(|r| r.code),
        "connection closed"
    );
}

/// Greets the client, then reads its messages and writes queued frames
/// until either side ends the connection. Returns why the server closes it
/// (`None` when the client left or the socket failed).
async fn serve(
    realtime: &Realtime,
    socket: &mut WebSocket,
    connection: &Arc<Connection>,
    outbox: &mut mpsc::Receiver<Utf8Bytes>,
) -> Option<CloseReason> {
    let timeout_secs = realtime.options().activity_timeout_secs;
    let established =
        json!({ "socket_id": connection.socket_id, "activity_timeout": timeout_secs });
    let greeting = frame(
        "pusher:connection_established",
        None,
        &established.to_string(),
    );
    if !send(socket, greeting.into()).await {
        return None;
    }

    let shutdown = realtime.shutdown_token();
    let activity = Duration::from_secs(timeout_secs);
    let grace = activity.min(PONG_GRACE);
    let mut last_seen = Instant::now();
    let mut pinged = false;
    loop {
        let deadline = last_seen + if pinged { activity + grace } else { activity };
        tokio::select! {
            () = shutdown.cancelled() => return Some(CloseReason::SHUTTING_DOWN),
            () = connection.closing().cancelled() => return connection.close_reason(),
            Some(message) = outbox.recv() => {
                if !send(socket, message).await {
                    return None;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(message)) => {
                    last_seen = Instant::now();
                    pinged = false;
                    match message {
                        Message::Text(text) => realtime.handle(connection, text.as_str()),
                        Message::Close(_) => return None,
                        Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => {}
                    }
                }
                Some(Err(_)) | None => return None,
            },
            () = tokio::time::sleep_until(deadline) => {
                if pinged {
                    return Some(CloseReason::PONG_TIMEOUT);
                }
                // Silent for the activity timeout: ask for a sign of life.
                pinged = true;
                connection.push(frame("pusher:ping", None, "{}").into());
            }
        }
    }
}

/// Writes one text frame; `false` when the client is gone or too slow.
async fn send(socket: &mut WebSocket, message: Utf8Bytes) -> bool {
    matches!(
        tokio::time::timeout(WRITE_TIMEOUT, socket.send(Message::Text(message))).await,
        Ok(Ok(()))
    )
}

/// Sends `pusher:error` and a close frame with the reason's code.
async fn close(socket: &mut WebSocket, reason: CloseReason) {
    let _ = tokio::time::timeout(WRITE_TIMEOUT, async {
        socket
            .send(Message::Text(
                protocol::error_frame(Some(reason.code), reason.message).into(),
            ))
            .await?;
        socket
            .send(Message::Close(Some(CloseFrame {
                code: reason.code,
                reason: reason.message.into(),
            })))
            .await
    })
    .await;
}
