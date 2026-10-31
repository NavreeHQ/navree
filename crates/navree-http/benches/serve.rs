//! What `navree_http::serve` adds to a request over a bare hyper accept loop,
//! on loopback, over HTTP/1 keep-alive and HTTP/2.
//!
//! One process holds every variant at once, each on its own listener behind
//! the same router and handlers:
//!
//! - `bare`: hyper-util's auto builder over `TokioIo` (with upgrades, as
//!   `serve` runs it), no timers and no limits. `serve` does not set
//!   TCP_NODELAY, so neither does this unless `--nodelay` sets it on every
//!   variant.
//! - `serve`: `navree_http::serve` with the default options.
//! - `serve-nolimits`: `serve` with every timeout and connection limit off,
//!   so the gap to `serve` is what the limits cost and the gap to `bare` is
//!   what the rest of the server costs.
//!
//! The machine is not quiet, so two separate runs are not comparable. Rounds
//! of every variant alternate, in all six orders, and each round yields a
//! ratio to the same pair's `bare`. The report gives the median of those
//! ratios with a 95% bootstrap interval and the quartiles, then the raw
//! medians. A ratio whose interval holds 1.0 is "no measurable difference".
//!
//! The load is a tokio client on its own runtime, with connections kept
//! alive across rounds: 32 connections for HTTP/1 (16 for 1 MiB bodies), and
//! 4 connections of 16 streams (2 of 8 for 1 MiB) for HTTP/2, enough to keep
//! the server's workers busy. The 1 MiB body is one chunk, and `h2-large` has
//! a client flow-control window of 4 MiB per stream (where a browser sits)
//! while `h2-large-64k` keeps hyper's 64 KiB default.
//!
//! Run with `cargo bench -p navree-http --bench serve`. Arguments after `--`:
//! a scenario name to run only that one (`h1-small`, `h2-small`, `h1-large`,
//! `h2-large`, `h2-large-64k`), `--pairs N` (rounds per variant, default 36),
//! `--secs S` (seconds per round, default 0.75), `--server-threads N` and
//! `--client-threads N` (default 4 each), `--nodelay`.
#![allow(missing_docs)]

use std::io;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{Method, Request, Response, header};
use http_body_util::{BodyExt, Empty};
use hyper::body::Incoming;
use hyper::client::conn::{http1, http2};
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use navree_http::body::Body;
use navree_http::routing::Router;
use navree_http::serve::Listener;
use navree_http::{HttpOptions, Json, ServerOptions};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::{Builder as Runtime, Handle};
use tower::ServiceExt;

const LARGE: usize = 1 << 20;

// ---- the app behind every variant ----

fn app() -> Router {
    let large = Bytes::from(vec![b'x'; LARGE]);
    Router::new()
        .route(Method::GET, "/small", || async {
            Json(serde_json::json!({
                "id": "7f0c2d4e",
                "name": "Ada Lovelace",
                "email": "ada@example.com",
                "active": true,
                "tags": ["lead", "enterprise"],
            }))
        })
        .route(Method::GET, "/large", move || {
            let body = large.clone();
            async move {
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/octet-stream")
                    .body(Body::from(body))
                    .expect("a response")
            }
        })
}

// ---- the variants ----

#[derive(Clone, Copy)]
enum Variant {
    Bare,
    Serve,
    ServeNoLimits,
}

impl Variant {
    const ALL: [Self; 3] = [Self::Bare, Self::Serve, Self::ServeNoLimits];

    fn name(self) -> &'static str {
        match self {
            Self::Bare => "bare",
            Self::Serve => "serve",
            Self::ServeNoLimits => "serve-nolimits",
        }
    }
}

fn no_limits() -> ServerOptions {
    ServerOptions {
        header_read_timeout_ms: None,
        idle_timeout_ms: None,
        body_read_timeout_ms: None,
        body_min_bytes_per_sec: None,
        http2_keep_alive_interval_ms: None,
        max_connections: None,
        max_connections_per_ip: None,
        write_stall_timeout_ms: None,
        ..ServerOptions::default()
    }
}

/// Starts `variant` on a free loopback port, on the runtime of `server`, with
/// TCP_NODELAY on its connections if `nodelay`.
fn start(server: &Handle, variant: Variant, nodelay: bool) -> SocketAddr {
    server.block_on(async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let addr = listener.local_addr().expect("an address");
        match variant {
            Variant::Bare => {
                tokio::spawn(bare(listener, app(), nodelay));
            }
            Variant::Serve | Variant::ServeNoLimits => {
                let options = HttpOptions {
                    server: match variant {
                        Variant::Serve => ServerOptions::default(),
                        _ => no_limits(),
                    },
                    ..HttpOptions::default()
                };
                tokio::spawn(async move {
                    navree_http::serve(Tcp { listener, nodelay }, app())
                        .options(&options)
                        .await
                });
            }
        }
        addr
    })
}

/// An accept loop on hyper alone.
async fn bare(listener: TcpListener, app: Router, nodelay: bool) {
    loop {
        let (stream, _) = listener.accept().await.expect("accepts");
        stream.set_nodelay(nodelay).expect("nodelay");
        let app = app.clone();
        tokio::spawn(async move {
            let service = service_fn(move |request: Request<Incoming>| {
                app.clone().oneshot(request.map(Body::new))
            });
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(TokioIo::new(stream), service)
                .await;
        });
    }
}

/// `serve`'s own listener for a `TcpListener`, which leaves Nagle's algorithm
/// on, plus the option to turn it off.
struct Tcp {
    listener: TcpListener,
    nodelay: bool,
}

impl Listener for Tcp {
    type Stream = TcpStream;
    type Io = TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (TcpStream, SocketAddr) {
        let (stream, addr) = Listener::accept(&mut self.listener).await;
        stream.set_nodelay(self.nodelay).expect("nodelay");
        (stream, addr)
    }

    fn peer_ip(addr: &SocketAddr) -> Option<IpAddr> {
        <TcpListener as Listener>::peer_ip(addr)
    }

    fn poll_sent(stream: &TcpStream, cx: &mut Context<'_>) -> Poll<bool> {
        <TcpListener as Listener>::poll_sent(stream, cx)
    }

    fn handshake(
        &self,
        stream: TcpStream,
    ) -> impl Future<Output = io::Result<TcpStream>> + Send + 'static {
        Listener::handshake(&self.listener, stream)
    }
}

// ---- the client ----

/// A socket that counts the bytes through it.
struct Counted {
    inner: TcpStream,
    down: Arc<AtomicU64>,
    up: Arc<AtomicU64>,
}

impl AsyncRead for Counted {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        self.down
            .fetch_add((buf.filled().len() - before) as u64, Ordering::Relaxed);
        polled
    }
}

impl AsyncWrite for Counted {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &polled {
            self.up.fetch_add(*n as u64, Ordering::Relaxed);
        }
        polled
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

type Req = Request<Empty<Bytes>>;

enum Sender {
    H1(http1::SendRequest<Empty<Bytes>>),
    H2(http2::SendRequest<Empty<Bytes>>),
}

impl Sender {
    async fn send(&mut self, request: Req) -> Response<Incoming> {
        match self {
            Self::H1(sender) => {
                sender.ready().await.expect("a ready connection");
                sender.send_request(request).await.expect("a response")
            }
            Self::H2(sender) => sender.send_request(request).await.expect("a response"),
        }
    }
}

struct Connection {
    sender: Option<Sender>,
    down: Arc<AtomicU64>,
    up: Arc<AtomicU64>,
}

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    http2: bool,
    path: &'static str,
    /// The size of the answer, learned from the server before a run.
    bytes: usize,
    connections: usize,
    streams: usize,
    /// The client's flow-control window per stream, `None` for hyper's.
    window: Option<u32>,
}

const SCENARIOS: [Scenario; 5] = [
    scenario("h1-small", false, "/small", 32, 1, None),
    scenario("h2-small", true, "/small", 4, 16, None),
    scenario("h1-large", false, "/large", 16, 1, None),
    scenario("h2-large", true, "/large", 2, 8, Some(4 << 20)),
    scenario("h2-large-64k", true, "/large", 2, 8, None),
];

const fn scenario(
    name: &'static str,
    http2: bool,
    path: &'static str,
    connections: usize,
    streams: usize,
    window: Option<u32>,
) -> Scenario {
    Scenario {
        name,
        http2,
        path,
        bytes: 0,
        connections,
        streams,
        window,
    }
}

async fn connect(scenario: &Scenario, addr: SocketAddr) -> Connection {
    let stream = TcpStream::connect(addr).await.expect("connects");
    stream.set_nodelay(true).expect("nodelay");
    let (down, up) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let io = TokioIo::new(Counted {
        inner: stream,
        down: down.clone(),
        up: up.clone(),
    });
    let sender = if scenario.http2 {
        let mut builder = http2::Builder::new(TokioExecutor::new());
        if let Some(window) = scenario.window {
            builder
                .initial_stream_window_size(window)
                .initial_connection_window_size(window.saturating_mul(4));
        }
        let (sender, connection) = builder.handshake(io).await.expect("an HTTP/2 handshake");
        tokio::spawn(connection);
        Sender::H2(sender)
    } else {
        let (sender, connection) = http1::handshake(io).await.expect("an HTTP/1 handshake");
        tokio::spawn(connection);
        Sender::H1(sender)
    };
    Connection {
        sender: Some(sender),
        down,
        up,
    }
}

fn request(scenario: &Scenario) -> Req {
    let uri = if scenario.http2 {
        format!("http://bench{}", scenario.path)
    } else {
        scenario.path.to_owned()
    };
    Request::get(uri)
        .header(header::HOST, "bench")
        .body(Empty::new())
        .expect("a request")
}

/// What one round measured.
struct Round {
    rps: f64,
    p50_us: f64,
    p99_us: f64,
    down_per_request: f64,
    up_per_request: f64,
}

/// Every stream of every connection asks again as soon as it has the whole
/// answer, for `secs`.
async fn round(scenario: &Scenario, connections: &mut [Connection], secs: f64) -> Round {
    let (down, up) = connections.iter().fold((0, 0), |(down, up), c| {
        (
            down + c.down.load(Ordering::Relaxed),
            up + c.up.load(Ordering::Relaxed),
        )
    });
    let start = Instant::now();
    let deadline = start + Duration::from_secs_f64(secs);
    let mut tasks = Vec::new();
    for (index, connection) in connections.iter_mut().enumerate() {
        for _ in 0..scenario.streams {
            let mut sender = match connection.sender.as_ref().expect("a sender") {
                Sender::H2(sender) => Sender::H2(sender.clone()),
                Sender::H1(_) => connection.sender.take().expect("a sender"),
            };
            let scenario = *scenario;
            tasks.push(tokio::spawn(async move {
                let mut latencies = Vec::new();
                while Instant::now() < deadline {
                    let sent = Instant::now();
                    let response = sender.send(request(&scenario)).await;
                    assert_eq!(response.status(), 200);
                    let mut body = response.into_body();
                    let mut length = 0;
                    while let Some(frame) = body.frame().await {
                        length += frame.expect("a frame").data_ref().map_or(0, Bytes::len);
                    }
                    assert_eq!(length, scenario.bytes);
                    latencies.push(sent.elapsed().as_nanos() as u32);
                }
                (index, sender, latencies)
            }));
        }
    }
    let mut latencies = Vec::new();
    for task in tasks {
        let (index, sender, mut taken) = task.await.expect("a client task");
        if matches!(sender, Sender::H1(_)) {
            connections[index].sender = Some(sender);
        }
        latencies.append(&mut taken);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let (down_after, up_after) = connections.iter().fold((0, 0), |(down, up), c| {
        (
            down + c.down.load(Ordering::Relaxed),
            up + c.up.load(Ordering::Relaxed),
        )
    });
    latencies.sort_unstable();
    let requests = latencies.len() as f64;
    let at = |q: f64| f64::from(latencies[((requests - 1.0) * q) as usize]) / 1e3;
    Round {
        rps: requests / elapsed,
        p50_us: at(0.5),
        p99_us: at(0.99),
        down_per_request: (down_after - down) as f64 / requests,
        up_per_request: (up_after - up) as f64 / requests,
    }
}

// ---- statistics ----

fn quantile(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    quantile(&sorted, 0.5)
}

/// A 95% interval of the median of `values`, by bootstrap.
fn bootstrap(values: &[f64]) -> (f64, f64) {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut medians: Vec<f64> = (0..5000)
        .map(|_| {
            let resample: Vec<f64> = (0..values.len())
                .map(|_| values[(next() % values.len() as u64) as usize])
                .collect();
            median(&resample)
        })
        .collect();
    medians.sort_by(f64::total_cmp);
    (quantile(&medians, 0.025), quantile(&medians, 0.975))
}

/// `numerator / denominator` per pair: its median, 95% interval and quartiles.
fn ratio(numerator: &[f64], denominator: &[f64]) -> String {
    let ratios: Vec<f64> = numerator.iter().zip(denominator).map(|(n, d)| n / d).collect();
    let mut sorted = ratios.clone();
    sorted.sort_by(f64::total_cmp);
    let (low, high) = bootstrap(&ratios);
    let verdict = if low <= 1.0 && 1.0 <= high { "  =" } else { "  !" };
    format!(
        "{:.3} [{low:.3}, {high:.3}] q {:.3}-{:.3}{verdict}",
        median(&ratios),
        quantile(&sorted, 0.25),
        quantile(&sorted, 0.75),
    )
}

// ---- the run ----

struct Settings {
    nodelay: bool,
    pairs: usize,
    secs: f64,
    server_threads: usize,
    client_threads: usize,
}

fn run(scenario: &Scenario, settings: &Settings, addrs: &[SocketAddr], client: &Handle) {
    let mut rounds: Vec<Vec<Round>> = Variant::ALL.iter().map(|_| Vec::new()).collect();
    client.block_on(async {
        // The size of the answer, which every variant must give.
        let mut probe = connect(scenario, addrs[0]).await;
        let sender = probe.sender.as_mut().expect("a sender");
        let answer = sender.send(request(scenario)).await.into_body();
        let bytes = answer.collect().await.expect("a body").to_bytes().len();
        let scenario = &Scenario { bytes, ..*scenario };
        let mut pools = Vec::new();
        for &addr in addrs {
            let mut pool = Vec::new();
            for _ in 0..scenario.connections {
                pool.push(connect(scenario, addr).await);
            }
            round(scenario, &mut pool, 0.4).await;
            pools.push(pool);
        }
        // All six orders of the three variants, over and over.
        const ORDERS: [[usize; 3]; 6] = [
            [0, 1, 2],
            [2, 1, 0],
            [1, 2, 0],
            [0, 2, 1],
            [2, 0, 1],
            [1, 0, 2],
        ];
        for pair in 0..settings.pairs {
            for variant in ORDERS[pair % 6] {
                rounds[variant].push(round(scenario, &mut pools[variant], settings.secs).await);
            }
        }
    });
    report(scenario, settings, &rounds);
}

fn report(scenario: &Scenario, settings: &Settings, rounds: &[Vec<Round>]) {
    println!(
        "\n== {}: GET {} over HTTP/{}, {} connections x {} streams, {} rounds of {} s per variant ==",
        scenario.name,
        scenario.path,
        if scenario.http2 { 2 } else { 1 },
        scenario.connections,
        scenario.streams,
        settings.pairs,
        settings.secs,
    );
    let column = |variant: usize, f: fn(&Round) -> f64| -> Vec<f64> {
        rounds[variant].iter().map(f).collect()
    };
    println!(
        "{:<16}{:>12}{:>11}{:>11}{:>13}{:>11}",
        "median of rounds", "req/s", "p50 us", "p99 us", "down B/req", "up B/req"
    );
    for (index, variant) in Variant::ALL.iter().enumerate() {
        println!(
            "{:<16}{:>12.0}{:>11.1}{:>11.1}{:>13.1}{:>11.1}",
            variant.name(),
            median(&column(index, |r| r.rps)),
            median(&column(index, |r| r.p50_us)),
            median(&column(index, |r| r.p99_us)),
            median(&column(index, |r| r.down_per_request)),
            median(&column(index, |r| r.up_per_request)),
        );
    }
    println!("\nmedian of per-pair ratios [95% bootstrap interval] and quartiles; = holds 1.0, ! does not");
    println!("{:<24}{:<44}{:<44}{}", "", "req/s (below 1 = slower)", "p50 latency (above 1 = slower)", "p99 latency (above 1 = slower)");
    for (numerator, denominator) in [(1, 0), (2, 0), (1, 2)] {
        println!(
            "{:<24}{:<44}{:<44}{}",
            format!("{} / {}", Variant::ALL[numerator].name(), Variant::ALL[denominator].name()),
            ratio(&column(numerator, |r| r.rps), &column(denominator, |r| r.rps)),
            ratio(&column(numerator, |r| r.p50_us), &column(denominator, |r| r.p50_us)),
            ratio(&column(numerator, |r| r.p99_us), &column(denominator, |r| r.p99_us)),
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|at| args.get(at + 1))
    };
    let settings = Settings {
        nodelay: args.iter().any(|arg| arg == "--nodelay"),
        pairs: value("--pairs").map_or(36, |v| v.parse().expect("--pairs N")),
        secs: value("--secs").map_or(0.75, |v| v.parse().expect("--secs S")),
        server_threads: value("--server-threads").map_or(4, |v| v.parse().expect("a number")),
        client_threads: value("--client-threads").map_or(4, |v| v.parse().expect("a number")),
    };
    let filter = args.iter().find(|arg| SCENARIOS.iter().any(|s| s.name == arg.as_str()));
    let server = Runtime::new_multi_thread()
        .worker_threads(settings.server_threads)
        .enable_all()
        .build()
        .expect("a server runtime");
    let client = Runtime::new_multi_thread()
        .worker_threads(settings.client_threads)
        .enable_all()
        .build()
        .expect("a client runtime");
    let addrs: Vec<SocketAddr> = Variant::ALL
        .iter()
        .map(|&variant| start(server.handle(), variant, settings.nodelay))
        .collect();
    for scenario in SCENARIOS.iter().filter(|s| filter.is_none_or(|f| *f == s.name)) {
        run(scenario, &settings, &addrs, client.handle());
    }
}
