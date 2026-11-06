//! [`serve()`]: an app on a [`Listener`], over HTTP/1 and HTTP/2 on hyper,
//! with graceful shutdown and the limits of [`ServerOptions`].
//!
//! ```no_run
//! # async fn demo(ctx: &navree_kernel::AppContext) -> Result<(), navree_kernel::BoxError> {
//! use navree_http::{HttpOptions, build_router};
//!
//! let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
//! let app = build_router(ctx).await?;
//! navree_http::serve(listener, app)
//!     .options(&HttpOptions::default())
//!     .with_graceful_shutdown(ctx.shutdown_token().cancelled_owned())
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! The `serve` command runs this; call it yourself for a listener of your
//! own. The service is anything Tower can call with a `Request<Body>`, and
//! handlers find the peer in [`ConnectInfo`].
//!
//! # What a client can hold
//!
//! Every connection stays inside these bounds, on both protocols:
//!
//! - **The first request.** A new connection has
//!   [`header_read_timeout_ms`](ServerOptions::header_read_timeout_ms) to
//!   finish a request head, counted from the accept, so sending nothing,
//!   sending half a protocol preface and sending one byte at a time all end
//!   the same way. A TLS handshake gets the same timeout before that.
//! - **Between requests.** [`idle_timeout_ms`](ServerOptions::idle_timeout_ms)
//!   closes a connection with nothing in flight (a GOAWAY first on HTTP/2,
//!   which nothing the client sends can postpone, a flood of frames included),
//!   and the next head on an HTTP/1 connection has the header timeout from its
//!   first byte. A head that starts in the same read as a request without a
//!   body is timed from the end of that response; one that follows a request
//!   with a body has the idle timeout instead, because the server cannot see
//!   where the body ended.
//! - **Request bodies.** A handler reading a body waits on the client for at
//!   most [`body_read_timeout_ms`](ServerOptions::body_read_timeout_ms), and
//!   the body must average
//!   [`body_min_bytes_per_sec`](ServerOptions::body_min_bytes_per_sec) (408,
//!   then the connection closes, or on HTTP/2 only the stream). A body that is
//!   a conversation rather than an upload lifts this with [`BodyReadLimit`]:
//!   gRPC does for client and bidirectional streams, once a call has passed
//!   its access check.
//! - **A body its answer did not wait for.** A 401, a 404, a 413 for a body
//!   declared over its route's limit, or a handler that does not need the
//!   body answers before the body has arrived, and a client sends its whole
//!   body before it reads the answer. On HTTP/1 the answer goes out first and
//!   the server then reads the rest of the body and throws it away, so the
//!   connection can take the next request: at most the route's body limit
//!   (`http.body_limit_bytes` outside a route), for at most 5 s, at the pace
//!   above. A body that declares more, one not read to its end in time and one
//!   from a client waiting on `100 Continue` close the connection after the
//!   answer (`Connection: close`, when the answer has not gone out yet). That
//!   close lingers, so the client reads the answer and not a reset: the server
//!   ends its side, then reads what the client is still sending, until the
//!   client closes its side, sends nothing for 2 s, or 5 s have passed. What
//!   has already arrived is thrown away at no cost, so a body hyper has read
//!   whole leaves the connection open whatever its size. On HTTP/2 the answer
//!   ends the stream, and the connection goes on.
//! - **Heads.** [`max_header_bytes`](ServerOptions::max_header_bytes) and
//!   [`max_headers`](ServerOptions::max_headers) (431).
//! - **Responses.** A response its client stops taking ends after
//!   [`write_stall_timeout_ms`](ServerOptions::write_stall_timeout_ms): the
//!   connection, when the socket stops taking bytes, or only the stream when
//!   an HTTP/2 client stops one with flow control. A client that keeps taking
//!   bytes, however slowly, is not cut off. This does not reach an HTTP/2
//!   WebSocket that flow control holds back: hyper keeps the send half of an
//!   upgraded stream to itself, so a write waiting for the client's window
//!   cannot be told from a socket with nothing to say.
//! - **Dead peers.** HTTP/2 connections that go quiet are pinged, including
//!   while a request is in flight.
//! - **Connections.** At
//!   [`max_connections`](ServerOptions::max_connections) new clients wait,
//!   nearly all of them in the listen backlog, where they cost almost
//!   nothing, and are served as slots free up. For a client that has sent
//!   something, the connection idle the longest closes, one for each such
//!   client, and a connection that has only started its next head counts as
//!   idle, so keep-alive connections cannot keep anyone out; slots held by
//!   clients that have not finished a request come back at the limits above.
//!   A client that has sent nothing closes nothing. One address holds at most
//!   [`max_connections_per_ip`](ServerOptions::max_connections_per_ip). That
//!   limits what one address can hold; it does not stop a client with many
//!   addresses. At that limit a new connection takes the place of the
//!   address's own connection idle the longest, by the same rules, so many
//!   users behind one address (an office, a carrier's NAT) are not turned
//!   away by their idle connections; only when none of them is idle is the
//!   new one closed. A connection that makes way counts until its socket has
//!   closed, and at most 8 of an address's connections (no more than its
//!   limit) close for newer ones at a time, so whatever a client sends on a
//!   connection that is closing, one address holds at most its limit and
//!   those. Everyone behind an address shares its limit, so one of them can
//!   close the others' idle connections by opening new ones, which they then
//!   open again. TLS handshakes hold a slot too.
//! - **Upgrades.** A WebSocket holds its slot for as long as it is open, and
//!   no limit above ends a quiet one: it can be perfectly healthy, and the
//!   server cannot tell it from a client doing nothing. So unlike the others
//!   this bound is off by default: set
//!   [`upgraded_idle_timeout_ms`](ServerOptions::upgraded_idle_timeout_ms) for
//!   an app whose sockets are expected to talk, or that takes them from
//!   anyone. Until then, the handler's own authentication and its heartbeat
//!   are what bound them, and `max_connections_per_ip` what limits one client.
//!
//! The time a handler takes is not in this list: `http.timeout_secs` bounds
//! it in the pipeline. Neither is a client moving a body or a response at a
//! crawl above these limits, or the frames it sends while a request is in
//! flight. Those frames are read on the connection's own task, which yields
//! to the others when its budget runs out.

use std::future::{Future, IntoFuture, poll_fn};
use std::io;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::CONNECTION;
use http::{HeaderValue, Method, Request, Response, StatusCode, Version};
use http_body::Body as HttpBody;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use navree_kernel::{BoxError, BoxFuture};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tower::{Service, ServiceExt as _};

use crate::body::{Body, DEFAULT_BODY_LIMIT};
use crate::extract::rejection::ExtensionRejection;
use crate::extract::{FromRequestParts, ProxyTrust};
use crate::network::DEFAULT_IPV6_PREFIX;
use crate::{HttpOptions, ServerOptions};

mod body;
mod clock;
mod frames;
mod gate;
mod listener;
mod streams;
mod unread;

pub use body::{BodyReadLimit, BodyReadTimeout};
pub use listener::{Listener, poll_tcp_sent};
pub(crate) use unread::UnreadLimit;

use body::{Pace, PacedBody, ResponseBody};
use clock::{Clock, InFlight, Timer, Watched};
use gate::{Gate, Slot};
use streams::{Progress, Streams};
use unread::{Drain, Kept, Unread, waits_to_continue};

/// The smallest `http.server.max_header_bytes`: hyper's smallest HTTP/1
/// buffer.
pub(crate) const MIN_HEADER_BYTES: usize = 8 * 1024;

/// hyper keeps this many HTTP/1 headers on the stack. Any other limit moves
/// them to the heap for every request, so this one is not set explicitly.
const HYPER_STACK_HEADERS: usize = 100;

/// The longest time limit. `HttpOptions` takes any number, and a deadline
/// this far off already means never.
const LONGEST_LIMIT: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// The peer of a connection, in the extensions of each of its requests:
/// `ConnectInfo<SocketAddr>` for a TCP listener. It is also an extractor.
/// [`ClientIp`](crate::ClientIp) is usually what a handler wants: it knows
/// about trusted proxies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectInfo<T>(pub T);

impl<T: Clone + Send + Sync + 'static> FromRequestParts for ConnectInfo<T> {
    type Rejection = ExtensionRejection;

    async fn from_request_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(ExtensionRejection(std::any::type_name::<Self>()))
    }
}

/// Serves `service` on `listener` until the future ends, with
/// [default limits](ServerOptions): set yours with
/// [`options`](Serve::options). Without
/// [`with_graceful_shutdown`](Serve::with_graceful_shutdown) it never ends,
/// and dropping it closes every connection.
///
/// `service` takes any tower service for `Request<Body>`:
/// [`build_router`](crate::build_router)'s app, a
/// [`routing::Router`](crate::routing::Router), or a stack of layers.
pub fn serve<L, S, B>(listener: L, service: S) -> Serve<L, S>
where
    L: Listener,
    S: Service<Request<Body>, Response = Response<B>> + Clone + Send + 'static,
{
    Serve {
        listener,
        service,
        options: ServerOptions::default(),
        grace: Duration::from_secs(HttpOptions::default().shutdown_grace_secs),
        proxies: ProxyTrust::none(),
        ipv6_prefix: DEFAULT_IPV6_PREFIX,
        body_limit: DEFAULT_BODY_LIMIT,
        signal: None,
    }
}

/// A server, returned by [`serve()`]: configure it, then await it.
#[must_use = "a server does nothing until it is awaited"]
pub struct Serve<L, S> {
    listener: L,
    service: S,
    options: ServerOptions,
    grace: Duration,
    proxies: ProxyTrust,
    ipv6_prefix: u8,
    body_limit: usize,
    signal: Option<BoxFuture<'static, ()>>,
}

impl<L, S> Serve<L, S> {
    /// The limits of `options.server`, `options.shutdown_grace_secs`, what
    /// `http.server.max_connections_per_ip` goes by: who is a proxy
    /// (`options.trusted_proxies`, `options.trust_proxy`) and how much of an
    /// IPv6 address names a client (`options.rate_limit_ipv6_prefix`), and
    /// the most of a body read after an answer outside a route
    /// (`options.body_limit_bytes`).
    pub fn options(mut self, options: &HttpOptions) -> Self {
        self.options = options.server.clone();
        self.grace = Duration::from_secs(options.shutdown_grace_secs);
        self.proxies = ProxyTrust::new(options.trust_proxy, &options.trusted_proxies);
        self.ipv6_prefix = options.rate_limit_ipv6_prefix;
        self.body_limit = options.body_limit_bytes;
        self
    }

    /// When `signal` ends the server stops accepting, tells its connections
    /// to close when their requests are done (`Connection: close`, or a
    /// GOAWAY), and waits for them for up to `http.shutdown_grace_secs`.
    /// After that it closes them and ends anyway. An HTTP/1 WebSocket is not
    /// waited for: the socket left the server when it upgraded, and the app
    /// closes it through its own shutdown. An HTTP/2 WebSocket is a stream of
    /// its connection, and is waited for like any other.
    pub fn with_graceful_shutdown<F>(mut self, signal: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.signal = Some(Box::pin(signal));
        self
    }
}

impl<L, S> std::fmt::Debug for Serve<L, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Serve")
            .field("options", &self.options)
            .field("grace", &self.grace)
            .finish_non_exhaustive()
    }
}

impl<L, S, B> IntoFuture for Serve<L, S>
where
    L: Listener,
    S: Service<Request<Body>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    type Output = io::Result<()>;
    type IntoFuture = BoxFuture<'static, io::Result<()>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

impl<L, S, B> Serve<L, S>
where
    L: Listener,
    S: Service<Request<Body>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    async fn run(self) -> io::Result<()> {
        let Self {
            mut listener,
            service,
            options,
            grace,
            proxies,
            ipv6_prefix,
            body_limit,
            signal,
        } = self;
        let mut gate = Gate::new(&options, proxies, ipv6_prefix);
        let limits = Arc::new(Limits::new(options, body_limit));
        let mut signal = signal.unwrap_or_else(|| Box::pin(std::future::pending()));
        let stopping = CancellationToken::new();
        let mut connections = JoinSet::new();
        loop {
            let (stream, remote, slot) = tokio::select! {
                biased;
                () = &mut signal => break,
                next = gate.next(&mut listener) => next,
            };
            let clock = Arc::new(Clock::new(limits.header_read, limits.idle));
            gate.track(&clock, &slot);
            connections.spawn(connection(
                listener.handshake(stream),
                remote,
                slot,
                clock,
                service.clone(),
                limits.clone(),
                stopping.clone(),
            ));
            while connections.try_join_next().is_some() {}
        }
        // Clients still waiting for a slot get an answer now, not at the end
        // of the grace period.
        drop(gate);
        drop(listener);
        stopping.cancel();
        let drained = tokio::time::timeout(grace, async {
            while connections.join_next().await.is_some() {}
        });
        if drained.await.is_err() {
            tracing::warn!(target: "navree::http", ?grace, "in-flight requests did not finish within the grace period; closing them");
            connections.shutdown().await;
        }
        Ok(())
    }
}

/// The options, and the limits a connection needs as durations.
#[derive(Debug)]
struct Limits {
    server: ServerOptions,
    header_read: Option<Duration>,
    idle: Option<Duration>,
    body: Option<Pace>,
    write_stall: Option<Duration>,
    upgraded_idle: Option<Duration>,
    /// The most of a body read after an answer outside a route.
    body_limit: usize,
}

impl Limits {
    fn new(server: ServerOptions, body_limit: usize) -> Self {
        Self {
            header_read: server.header_read_timeout_ms.map(limit),
            idle: server.idle_timeout_ms.map(limit),
            body: server
                .body_read_timeout_ms
                .map(|timeout| Pace::new(limit(timeout), server.body_min_bytes_per_sec)),
            write_stall: server.write_stall_timeout_ms.map(limit),
            upgraded_idle: server.upgraded_idle_timeout_ms.map(limit),
            body_limit,
            server,
        }
    }
}

/// A limit in milliseconds, up to [`LONGEST_LIMIT`].
fn limit(ms: u64) -> Duration {
    Duration::from_millis(ms).min(LONGEST_LIMIT)
}

/// The connection builder for `options`, one per connection for its
/// `executor`. hyper's HTTP/1 header read timeout stays off: the [`Clock`]
/// does that job. The timer is for HTTP/2's pings.
fn builder(options: &ServerOptions, executor: Streams) -> Builder<Streams> {
    let mut builder = Builder::new(executor);
    let header_bytes = options.max_header_bytes.max(MIN_HEADER_BYTES);
    {
        let mut http1 = builder.http1();
        http1
            .timer(TokioTimer::new())
            .header_read_timeout(None)
            // What is read before a head is complete stays in this buffer,
            // so its size is the limit on a head.
            .max_buf_size(header_bytes);
        if options.max_headers != HYPER_STACK_HEADERS {
            http1.max_headers(options.max_headers);
        }
    }
    builder
        .http2()
        .timer(TokioTimer::new())
        .keep_alive_interval(options.http2_keep_alive_interval_ms.map(limit))
        .keep_alive_timeout(limit(options.http2_keep_alive_timeout_ms))
        .max_concurrent_streams(options.http2_max_concurrent_streams)
        .max_header_list_size(u32::try_from(header_bytes).unwrap_or(u32::MAX))
        // WebSockets over HTTP/2 (RFC 8441).
        .enable_connect_protocol();
    builder
}

/// Serves one connection from its handshake to its end.
async fn connection<H, I, A, S, B>(
    handshake: H,
    remote: A,
    slot: Slot,
    clock: Arc<Clock>,
    service: S,
    limits: Arc<Limits>,
    stopping: CancellationToken,
) where
    H: Future<Output = io::Result<I>> + Send,
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    A: Clone + std::fmt::Debug + Send + Sync + 'static,
    S: Service<Request<Body>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    let io = tokio::select! {
        biased;
        () = stopping.cancelled() => return,
        io = within(limits.header_read, handshake) => io,
    };
    let io = match io {
        Ok(io) => io,
        Err(error) => {
            tracing::debug!(target: "navree::http", peer = ?remote, %error, "handshake failed");
            return;
        }
    };

    clock.handshaken();
    let unread = Arc::new(Unread::new(clock.clone(), limits.body_limit));
    let mut drain = Drain::new(unread.clone());
    let hyper_service = {
        let clock = clock.clone();
        let remote = remote.clone();
        let pace = limits.body;
        let max_headers = limits.server.max_headers;
        service_fn(move |request: Request<Incoming>| {
            let bodiless = HttpBody::is_end_stream(request.body());
            let in_flight = clock.request(request.version(), bodiless);
            let progress = (request.version() == Version::HTTP_2).then(Progress::announce);
            let upgrades = request.method() == Method::CONNECT;
            // hyper holds HTTP/1 to this itself.
            let refused = progress.is_some() && request.headers().len() > max_headers;
            // One request at a time on HTTP/1: what the last one left is settled.
            let unread = progress.is_none().then(|| {
                unread.next_request();
                unread.clone()
            });
            let served = (!refused).then(|| {
                let request = prepare(request, &remote, pace, unread.as_ref());
                service.clone().oneshot(request)
            });
            async move {
                let Some(served) = served else {
                    let mut response = Response::new(None);
                    *response.status_mut() = StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE;
                    return Ok(finishing(response, in_flight, progress));
                };
                let mut response = served.await?;
                if unread.is_some_and(|unread| unread.closes()) {
                    response
                        .headers_mut()
                        .insert(CONNECTION, HeaderValue::from_static("close"));
                }
                if upgraded(upgrades, response.status()) {
                    in_flight.upgraded();
                }
                Ok::<_, S::Error>(finishing(response.map(Some), in_flight, progress))
            }
        })
    };
    let io = TokioIo::new(Watched::new(
        io,
        clock.clone(),
        slot,
        limits.write_stall,
        limits.upgraded_idle,
    ));
    let builder = builder(
        &limits.server,
        Streams::new(clock.clone(), limits.write_stall),
    );
    let mut conn = pin!(builder.serve_connection_with_upgrades(io, hyper_service));

    let mut timer: Timer = None;
    let mut closing = false;
    loop {
        // Biased, hyper first: see `Clock::poll_expired`.
        tokio::select! {
            biased;
            result = conn.as_mut() => {
                if let Err(error) = result {
                    tracing::debug!(target: "navree::http", peer = ?remote, %error, "connection closed with an error");
                }
                return;
            }
            () = stopping.cancelled(), if !closing => {
                closing = true;
                drain.give_up();
                conn.as_mut().graceful_shutdown();
                clock.close_soon();
            }
            () = poll_fn(|cx| clock.poll_expired(&mut timer, cx)) => {
                if closing || !clock.says_goodbye() {
                    tracing::debug!(target: "navree::http", peer = ?remote, "connection timed out");
                    return;
                }
                // HTTP/2 can say it is closing, and a request racing that
                // still gets an answer.
                closing = true;
                conn.as_mut().graceful_shutdown();
                clock.say_goodbye();
            }
            () = poll_fn(|cx| drain.poll(cx)) => {}
        }
    }
}

/// `future`, for at most `limit`.
async fn within<T>(
    limit: Option<Duration>,
    future: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    match limit {
        Some(limit) => tokio::time::timeout(limit, future)
            .await
            .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into())),
        None => future.await,
    }
}

/// The request as a service sees it: its peer, its body at a pace, and on
/// HTTP/1 (with `unread`) a body that hands what is not read back to the
/// connection.
fn prepare<A>(
    request: Request<Incoming>,
    remote: &A,
    pace: Option<Pace>,
    unread: Option<&Arc<Unread<Body>>>,
) -> Request<Body>
where
    A: Clone + Send + Sync + 'static,
{
    let (mut parts, incoming) = request.into_parts();
    parts.extensions.insert(ConnectInfo(remote.clone()));
    if HttpBody::is_end_stream(&incoming) {
        return Request::from_parts(parts, Body::new(incoming));
    }
    let body = match pace {
        Some(pace) => {
            let limit = BodyReadLimit::default();
            parts.extensions.insert(limit.clone());
            Body::new(PacedBody::new(incoming, pace, limit))
        }
        None => Body::new(incoming),
    };
    let body = match unread {
        Some(unread) => {
            let limit = unread.limit();
            parts.extensions.insert(limit.clone());
            let waits = waits_to_continue(&parts);
            Body::new(Kept::new(body, unread.clone(), limit, waits))
        }
        None => body,
    };
    Request::from_parts(parts, body)
}

/// Whether a response ends HTTP on an HTTP/1 connection: a 101, or a success
/// to `CONNECT`. Past it hyper gives the socket to the upgrade, or closes it
/// if nobody is waiting. See [`InFlight::upgraded`] for HTTP/2.
fn upgraded(connect: bool, status: StatusCode) -> bool {
    status == StatusCode::SWITCHING_PROTOCOLS || (connect && status.is_success())
}

/// The response as hyper sees it, in flight until its body ends. `None` is an
/// empty body.
fn finishing<B>(
    response: Response<Option<B>>,
    in_flight: InFlight,
    progress: Option<Arc<Progress>>,
) -> Response<ResponseBody<B>> {
    response.map(|body| ResponseBody::new(body, in_flight, progress))
}
