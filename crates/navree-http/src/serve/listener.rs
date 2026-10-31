//! [`Listener`]: where a server's connections come from.

use std::fmt::Debug;
use std::future::Future;
use std::io;
use std::mem::MaybeUninit;
use std::net::{IpAddr, SocketAddr};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

/// How long the accept loop waits after an error that is not one client's
/// (out of file descriptors, usually), so it does not spin.
const ACCEPT_BACKOFF: Duration = Duration::from_secs(1);

/// A source of connections for [`serve`](super::serve()).
///
/// [`accept`](Listener::accept) hands over a connection as it arrives;
/// [`handshake`](Listener::handshake) makes it ready to serve (a TLS
/// handshake, say) on the connection's own task, so a slow client never
/// holds up the accept loop, and a handshake in progress counts toward
/// `http.server.max_connections` and is timed by
/// `http.server.header_read_timeout_ms`.
pub trait Listener: Send + 'static {
    /// A connection as accepted.
    type Stream: Send + 'static;
    /// A connection ready to serve.
    type Io: AsyncRead + AsyncWrite + Unpin + Send + 'static;
    /// The peer's address, available to handlers as
    /// [`ConnectInfo`](super::ConnectInfo).
    type Addr: Clone + Debug + Send + Sync + 'static;

    /// The next connection. Errors are retried here: an implementation logs
    /// them and keeps waiting. A server that fills up drops this future to
    /// wait for a slot instead, so dropping it must lose no connection.
    fn accept(&mut self) -> impl Future<Output = (Self::Stream, Self::Addr)> + Send;

    /// The address `http.server.max_connections_per_ip` counts a peer under.
    /// `None`, the default, leaves it uncounted, for a listener whose peers
    /// have no address.
    fn peer_ip(addr: &Self::Addr) -> Option<IpAddr> {
        let _ = addr;
        None
    }

    /// Whether an accepted connection has sent anything, without reading it,
    /// for a full server: only a client known to want something has an idle
    /// connection closed for it. `Ready(true)` when it has sent something,
    /// `Ready(false)` when it has gone, `Pending` until one or the other,
    /// waking `cx`. The default, `Ready(true)`, is for a listener that cannot
    /// tell: every client that waits counts as wanting a slot.
    fn poll_sent(stream: &Self::Stream, cx: &mut Context<'_>) -> Poll<bool> {
        let _ = (stream, cx);
        Poll::Ready(true)
    }

    /// Makes an accepted connection ready to serve. An error drops the
    /// connection.
    fn handshake(
        &self,
        stream: Self::Stream,
    ) -> impl Future<Output = io::Result<Self::Io>> + Send + 'static;
}

impl Listener for TcpListener {
    type Stream = TcpStream;
    type Io = TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (TcpStream, SocketAddr) {
        loop {
            match TcpListener::accept(self).await {
                Ok(accepted) => return accepted,
                Err(error) => accept_failed(&error).await,
            }
        }
    }

    fn peer_ip(addr: &SocketAddr) -> Option<IpAddr> {
        Some(addr.ip())
    }

    fn poll_sent(stream: &TcpStream, cx: &mut Context<'_>) -> Poll<bool> {
        poll_tcp_sent(stream, cx)
    }

    fn handshake(
        &self,
        stream: TcpStream,
    ) -> impl Future<Output = io::Result<TcpStream>> + Send + 'static {
        std::future::ready(Ok(stream))
    }
}

/// [`Listener::poll_sent`] of a TCP connection: peeks at its first byte, for a
/// listener of accepted [`TcpStream`]s.
pub fn poll_tcp_sent(stream: &TcpStream, cx: &mut Context<'_>) -> Poll<bool> {
    let mut byte = [MaybeUninit::uninit()];
    let mut buffer = ReadBuf::uninit(&mut byte);
    stream
        .poll_peek(cx, &mut buffer)
        .map(|peeked| matches!(peeked, Ok(1..)))
}

/// Waits out an accept error: an error of one connection is ignored, anything
/// else is logged and retried after a pause.
async fn accept_failed(error: &io::Error) {
    if matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    ) {
        return;
    }
    tracing::error!(target: "navree::http", %error, "accepting a connection failed");
    tokio::time::sleep(ACCEPT_BACKOFF).await;
}
