//! [`TcpTransport`]: direct connections between services, no broker.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use navree_kernel::BoxFuture;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

use crate::options::{TcpOptions, TlsOptions};
use crate::pattern::{route_for, route_matches};
use crate::transport::LocalRetry;
use crate::{
    Delivery, DeliveryActions, Envelope, MicroError, MicroTransport, PatternKind, Subscribed,
    Subscription, pattern_matches,
};

/// Room for the envelope's headers on top of `micro.max_message_bytes`.
const HEADER_ROOM: usize = 64 * 1024;

/// Frames queued per connection before writers wait.
const WRITE_QUEUE: usize = 256;

/// The kind of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Request = 1,
    Reply = 2,
    Event = 3,
    NoResponders = 4,
}

impl Kind {
    fn of(byte: u8) -> Option<Kind> {
        Some(match byte {
            1 => Kind::Request,
            2 => Kind::Reply,
            3 => Kind::Event,
            4 => Kind::NoResponders,
            _ => return None,
        })
    }
}

/// A frame: a kind byte, an 8-byte correlation id, then the envelope
/// ([`Envelope::to_bytes`]); frames are length-prefixed.
fn encode(kind: Kind, correlation: u64, envelope: &Envelope) -> Bytes {
    let body = envelope.to_bytes();
    let mut out = BytesMut::with_capacity(9 + body.len());
    out.put_u8(kind as u8);
    out.put_u64(correlation);
    out.put_slice(&body);
    out.freeze()
}

fn decode(frame: Bytes) -> Result<(Kind, u64, Envelope), MicroError> {
    let malformed = || MicroError::transport("a malformed TCP frame was received");
    if frame.len() < 9 {
        return Err(malformed());
    }
    let kind = Kind::of(frame[0]).ok_or_else(malformed)?;
    let mut correlation = [0u8; 8];
    correlation.copy_from_slice(&frame[1..9]);
    let envelope = Envelope::from_bytes(frame.slice(9..))?;
    Ok((kind, u64::from_be_bytes(correlation), envelope))
}

/// A byte stream: plain TCP or TLS.
trait Io: AsyncRead + AsyncWrite + Send + Unpin + 'static {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Io for T {}

/// TCP connects services directly. Each service listens on `micro.tcp.addr`;
/// a client finds the service of a pattern in `micro.tcp.routes` (the
/// longest matching prefix wins; `micro.url` is the peer of patterns no
/// route covers).
///
/// - Frames are length-prefixed envelopes over long-lived connections, with
///   up to `pool_size` connections to each peer and many requests in flight
///   on each.
/// - A request goes to its route's peer; a peer that does not handle the
///   pattern answers "no responders" at once.
/// - `emit` sends the event to every peer whose route matches, once, and
///   does not wait for it to be handled: TCP has no storage, so events are
///   delivered at most once. A failed handler is retried inside the
///   receiving process.
/// - `micro.tcp.tls.cert` and `.key` turn on TLS (rustls, ring);
///   `micro.tcp.tls.ca` turns on mutual TLS: peers must present a
///   certificate that chains to it, and servers are verified against it
///   (against the public web roots without it).
#[derive(Clone)]
pub struct TcpTransport {
    inner: Arc<Inner>,
}

struct Inner {
    addr: String,
    routes: BTreeMap<String, String>,
    default_peer: Option<String>,
    pool_size: usize,
    max_frame: usize,
    connect_timeout: Duration,
    tls: Option<Tls>,
    peers: Mutex<HashMap<String, Arc<Peer>>>,
    bound: watch::Sender<Option<SocketAddr>>,
}

struct Tls {
    acceptor: TlsAcceptor,
    connector: TlsConnector,
}

impl TcpTransport {
    /// A TCP transport from `micro.tcp`; `default_peer` is the peer of
    /// patterns no route covers (`micro.url`).
    ///
    /// Fails when a TLS file cannot be read.
    pub fn new(
        options: &TcpOptions,
        default_peer: Option<String>,
        max_message_bytes: usize,
        connect_timeout: Duration,
    ) -> Result<Self, MicroError> {
        let tls = match (&options.tls.cert, &options.tls.key) {
            (Some(cert), Some(key)) => Some(tls_setup(&options.tls, cert, key)?),
            (None, None) => None,
            _ => {
                return Err(MicroError::transport(
                    "micro.tcp.tls.cert and micro.tcp.tls.key go together",
                ));
            }
        };
        Ok(Self {
            inner: Arc::new(Inner {
                addr: options.addr.clone(),
                routes: options.routes.clone(),
                default_peer: default_peer.filter(|peer| !peer.is_empty()),
                pool_size: options.pool_size.max(1),
                max_frame: max_message_bytes.saturating_add(HEADER_ROOM),
                connect_timeout,
                tls,
                peers: Mutex::default(),
                bound: watch::Sender::new(None),
            }),
        })
    }

    /// The address the listener is bound to, while it listens.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *self.inner.bound.borrow()
    }

    /// Waits until the listener is bound (useful with port 0) and returns
    /// its address.
    pub async fn wait_bound(&self) -> SocketAddr {
        let mut receiver = self.inner.bound.subscribe();
        loop {
            if let Some(addr) = *receiver.borrow_and_update() {
                return addr;
            }
            if receiver.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }

    fn codec(&self) -> LengthDelimitedCodec {
        LengthDelimitedCodec::builder()
            .max_frame_length(self.inner.max_frame)
            .new_codec()
    }

    fn peer(&self, addr: &str) -> Arc<Peer> {
        lock(&self.inner.peers)
            .entry(addr.to_owned())
            .or_insert_with(|| Arc::new(Peer::new(self.inner.pool_size)))
            .clone()
    }

    /// A connection to `addr`. With a `key`, always the same slot of the
    /// pool, so frames with that key keep their order; otherwise the next
    /// slot in turn. A slot connects on first use and again after its
    /// connection broke.
    async fn connection(&self, addr: &str, key: Option<&str>) -> Result<Arc<Conn>, MicroError> {
        let peer = self.peer(addr);
        let mut slots = peer.slots.lock().await;
        let slot = match key {
            Some(key) => {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(key, &mut hasher);
                usize::try_from(std::hash::Hasher::finish(&hasher) % slots.len() as u64)
                    .unwrap_or_default()
            }
            None => peer.turn.fetch_add(1, Ordering::SeqCst) % slots.len(),
        };
        if let Some(conn) = &slots[slot]
            && conn.alive.load(Ordering::SeqCst)
        {
            return Ok(conn.clone());
        }
        let conn = self.connect(addr).await?;
        slots[slot] = Some(conn.clone());
        Ok(conn)
    }

    async fn connect(&self, addr: &str) -> Result<Arc<Conn>, MicroError> {
        let attempt = async {
            let stream = TcpStream::connect(addr).await?;
            stream.set_nodelay(true)?;
            let io: Box<dyn Io> = match &self.inner.tls {
                Some(tls) => {
                    let host = addr.rsplit_once(':').map_or(addr, |(host, _)| host);
                    let host = host.trim_start_matches('[').trim_end_matches(']');
                    let name = ServerName::try_from(host.to_owned())
                        .map_err(|error| std::io::Error::other(error.to_string()))?;
                    Box::new(tls.connector.connect(name, stream).await?)
                }
                None => Box::new(stream),
            };
            Ok::<_, std::io::Error>(io)
        };
        let io = match tokio::time::timeout(self.inner.connect_timeout, attempt).await {
            Ok(Ok(io)) => io,
            Ok(Err(error)) => {
                return Err(MicroError::transport(format!(
                    "cannot connect to {addr}: {error}"
                )));
            }
            Err(_) => {
                return Err(MicroError::transport(format!(
                    "connecting to {addr} took longer than {:?}",
                    self.inner.connect_timeout
                )));
            }
        };
        Ok(Conn::start(io, self.codec(), addr.to_owned()))
    }

    /// The peers an event for `pattern` goes to: every route that covers
    /// it, each peer once; `micro.url` when no route does.
    fn event_peers(&self, pattern: &str) -> Vec<String> {
        let mut peers: BTreeSet<String> = self
            .inner
            .routes
            .iter()
            .filter(|(key, _)| route_matches(key, pattern))
            .map(|(_, peer)| peer.clone())
            .collect();
        if peers.is_empty()
            && let Some(peer) = &self.inner.default_peer
        {
            peers.insert(peer.clone());
        }
        peers.into_iter().collect()
    }
}

impl fmt::Debug for TcpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TcpTransport")
            .field("addr", &self.inner.addr)
            .field("routes", &self.inner.routes)
            .field("default_peer", &self.inner.default_peer)
            .field("pool_size", &self.inner.pool_size)
            .field("tls", &self.inner.tls.is_some())
            .finish_non_exhaustive()
    }
}

impl MicroTransport for TcpTransport {
    fn name(&self) -> &'static str {
        "tcp"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            let pattern = envelope.pattern().to_owned();
            let peer = route_for(&self.inner.routes, &pattern)
                .cloned()
                .or_else(|| self.inner.default_peer.clone())
                .ok_or_else(|| MicroError::NoResponders {
                    pattern: pattern.clone(),
                })?;
            let conn = self.connection(&peer, None).await?;
            let (id, reply) = conn.register();
            if conn
                .writer
                .send(encode(Kind::Request, id, &envelope))
                .await
                .is_err()
            {
                conn.forget(id);
                return Err(MicroError::transport(format!(
                    "the connection to {peer} closed"
                )));
            }
            let outcome = match envelope.remaining() {
                Some(left) => match tokio::time::timeout(left, reply).await {
                    Ok(outcome) => outcome,
                    Err(_) => {
                        conn.forget(id);
                        return Err(MicroError::Timeout {
                            pattern,
                            after: left,
                        });
                    }
                },
                None => reply.await,
            };
            outcome.unwrap_or_else(|_| {
                Err(MicroError::transport(format!(
                    "the connection to {peer} closed"
                )))
            })
        })
    }

    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            for peer in self.event_peers(envelope.pattern()) {
                // Events with a key share a connection: they arrive in order.
                let sent = match self.connection(&peer, envelope.key()).await {
                    Ok(conn) => conn
                        .writer
                        .send(encode(Kind::Event, 0, &envelope))
                        .await
                        .map_err(|_| MicroError::transport("the connection closed")),
                    Err(error) => Err(error),
                };
                if let Err(error) = sent {
                    // At most once: an event for a peer that is down is lost.
                    tracing::warn!(
                        target: "navree::micro",
                        pattern = envelope.pattern(),
                        peer = %peer,
                        %error,
                        "TCP event not delivered"
                    );
                }
            }
            Ok(())
        })
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        self.listen_subscribed(subscriptions, deliveries, Subscribed::detached())
    }

    /// A peer that connects before the listener is bound is refused, so the listener says it
    /// is subscribed once it is bound, and not before.
    fn listen_subscribed(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
        subscribed: Subscribed,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            let listener = TcpListener::bind(&self.inner.addr).await.map_err(|error| {
                MicroError::transport(format!("cannot listen on {}: {error}", self.inner.addr))
            })?;
            let local = listener
                .local_addr()
                .map_err(|error| MicroError::transport(error.to_string()))?;
            self.inner.bound.send_replace(Some(local));
            subscribed.signal();
            tracing::info!(target: "navree::micro", addr = %local, "TCP transport listening");
            let subscriptions = Arc::new(subscriptions);
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, peer) = match accepted {
                            Ok(accepted) => accepted,
                            Err(error) => {
                                tracing::warn!(target: "navree::micro", %error, "TCP accept failed");
                                continue;
                            }
                        };
                        let _ = stream.set_nodelay(true);
                        let acceptor = self.inner.tls.as_ref().map(|tls| tls.acceptor.clone());
                        let codec = self.codec();
                        let subscriptions = subscriptions.clone();
                        let deliveries = deliveries.clone();
                        tokio::spawn(async move {
                            let io: Box<dyn Io> = match acceptor {
                                Some(acceptor) => match acceptor.accept(stream).await {
                                    Ok(stream) => Box::new(stream),
                                    Err(error) => {
                                        tracing::warn!(target: "navree::micro", %peer, %error, "TLS handshake failed");
                                        return;
                                    }
                                },
                                None => Box::new(stream),
                            };
                            serve(io, codec, subscriptions, deliveries).await;
                        });
                    }
                    () = deliveries.closed() => break,
                }
            }
            self.inner.bound.send_replace(None);
            Ok(())
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let peers: Vec<Arc<Peer>> = lock(&self.inner.peers).drain().map(|(_, p)| p).collect();
            for peer in peers {
                for conn in peer.slots.lock().await.iter_mut().filter_map(Option::take) {
                    conn.alive.store(false, Ordering::SeqCst);
                }
            }
        })
    }
}

/// Serves one inbound connection: requests and events in, replies out,
/// until the connection or the listener closes.
async fn serve(
    io: Box<dyn Io>,
    codec: LengthDelimitedCodec,
    subscriptions: Arc<Vec<Subscription>>,
    deliveries: mpsc::Sender<Delivery>,
) {
    let (read, write) = tokio::io::split(io);
    let mut frames = FramedRead::new(read, codec.clone());
    let (writer, mut outgoing) = mpsc::channel::<Bytes>(WRITE_QUEUE);
    let write_task = tokio::spawn(async move {
        let mut sink = FramedWrite::new(write, codec);
        while let Some(frame) = outgoing.recv().await {
            if sink.send(frame).await.is_err() {
                break;
            }
        }
    });
    loop {
        let frame = tokio::select! {
            frame = frames.next() => frame,
            () = deliveries.closed() => break,
        };
        let Some(Ok(frame)) = frame else {
            break;
        };
        let (kind, id, envelope) = match decode(frame.freeze()) {
            Ok(decoded) => decoded,
            Err(error) => {
                tracing::warn!(target: "navree::micro", %error, "dropping a TCP connection");
                break;
            }
        };
        match kind {
            Kind::Request => {
                let index = subscriptions.iter().position(|s| {
                    s.kind == PatternKind::Message
                        && pattern_matches(&s.pattern, envelope.pattern())
                });
                match index {
                    Some(index) => {
                        let actions = ReplyActions {
                            writer: writer.clone(),
                            id,
                        };
                        if deliveries
                            .send(Delivery::new(envelope, index, 1, actions))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    None => {
                        let refusal = encode(
                            Kind::NoResponders,
                            id,
                            &Envelope::new(envelope.pattern().to_owned(), Bytes::new()),
                        );
                        if writer.send(refusal).await.is_err() {
                            break;
                        }
                    }
                }
            }
            Kind::Event => {
                for (index, _) in subscriptions.iter().enumerate().filter(|(_, s)| {
                    s.kind == PatternKind::Event && pattern_matches(&s.pattern, envelope.pattern())
                }) {
                    // At most once: a failed handler is retried in this process.
                    let delivery =
                        LocalRetry::delivery(deliveries.clone(), envelope.clone(), index);
                    if deliveries.send(delivery).await.is_err() {
                        break;
                    }
                }
            }
            Kind::Reply | Kind::NoResponders => {
                tracing::debug!(target: "navree::micro", "ignoring a reply on an inbound connection");
            }
        }
    }
    drop(writer);
    let _ = write_task.await;
}

/// Replies to a request on its connection.
struct ReplyActions {
    writer: mpsc::Sender<Bytes>,
    id: u64,
}

impl DeliveryActions for ReplyActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, _delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }

    fn reply(self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async move {
            self.writer
                .send(encode(Kind::Reply, self.id, &reply))
                .await
                .map_err(|_| MicroError::transport("the requester's connection closed"))
        })
    }
}

/// The connections to one peer: `pool_size` slots, each connected on
/// first use.
struct Peer {
    slots: tokio::sync::Mutex<Vec<Option<Arc<Conn>>>>,
    turn: AtomicUsize,
}

impl Peer {
    fn new(pool_size: usize) -> Self {
        Self {
            slots: tokio::sync::Mutex::new(vec![None; pool_size.max(1)]),
            turn: AtomicUsize::new(0),
        }
    }
}

type Pending = Mutex<HashMap<u64, oneshot::Sender<Result<Envelope, MicroError>>>>;

/// One outbound connection: requests in flight by correlation id.
struct Conn {
    writer: mpsc::Sender<Bytes>,
    pending: Arc<Pending>,
    next: AtomicU64,
    alive: Arc<AtomicBool>,
}

impl Conn {
    fn start(io: Box<dyn Io>, codec: LengthDelimitedCodec, peer: String) -> Arc<Conn> {
        let (read, write) = tokio::io::split(io);
        let (writer, mut outgoing) = mpsc::channel::<Bytes>(WRITE_QUEUE);
        let pending: Arc<Pending> = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let write_codec = codec.clone();
        let write_alive = alive.clone();
        tokio::spawn(async move {
            let mut sink = FramedWrite::new(write, write_codec);
            while let Some(frame) = outgoing.recv().await {
                if sink.send(frame).await.is_err() {
                    break;
                }
            }
            write_alive.store(false, Ordering::SeqCst);
        });
        let read_pending = pending.clone();
        let read_alive = alive.clone();
        tokio::spawn(async move {
            let mut frames = FramedRead::new(read, codec);
            while let Some(Ok(frame)) = frames.next().await {
                let Ok((kind, id, envelope)) = decode(frame.freeze()) else {
                    break;
                };
                let waiter = lock(&read_pending).remove(&id);
                let Some(waiter) = waiter else {
                    continue;
                };
                let outcome = match kind {
                    Kind::Reply => Ok(envelope),
                    Kind::NoResponders => Err(MicroError::NoResponders {
                        pattern: envelope.pattern().to_owned(),
                    }),
                    Kind::Request | Kind::Event => continue,
                };
                let _ = waiter.send(outcome);
            }
            read_alive.store(false, Ordering::SeqCst);
            for (_, waiter) in lock(&read_pending).drain() {
                let _ = waiter.send(Err(MicroError::transport(format!(
                    "the connection to {peer} closed"
                ))));
            }
        });
        Arc::new(Conn {
            writer,
            pending,
            next: AtomicU64::new(1),
            alive,
        })
    }

    fn register(&self) -> (u64, oneshot::Receiver<Result<Envelope, MicroError>>) {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        lock(&self.pending).insert(id, sender);
        (id, receiver)
    }

    fn forget(&self, id: u64) {
        lock(&self.pending).remove(&id);
    }
}

/// The TLS acceptor and connector from `micro.tcp.tls`.
fn tls_setup(options: &TlsOptions, cert: &Path, key: &Path) -> Result<Tls, MicroError> {
    let read_error = |path: &Path, error: &dyn fmt::Display| {
        MicroError::transport(format!("cannot read {}: {error}", path.display()))
    };
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .map_err(|error| read_error(cert, &error))?
        .collect::<Result<_, _>>()
        .map_err(|error| read_error(cert, &error))?;
    if chain.is_empty() {
        return Err(MicroError::transport(format!(
            "{} holds no certificate",
            cert.display()
        )));
    }
    let private_key = PrivateKeyDer::from_pem_file(key).map_err(|error| read_error(key, &error))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls_error =
        |error: rustls::Error| MicroError::transport(format!("TLS setup failed: {error}"));

    let ca_roots = match &options.ca {
        Some(ca) => {
            let mut roots = RootCertStore::empty();
            for certificate in
                CertificateDer::pem_file_iter(ca).map_err(|error| read_error(ca, &error))?
            {
                roots
                    .add(certificate.map_err(|error| read_error(ca, &error))?)
                    .map_err(tls_error)?;
            }
            Some(Arc::new(roots))
        }
        None => None,
    };

    let server = ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?;
    let server = match &ca_roots {
        Some(roots) => {
            let verifier =
                WebPkiClientVerifier::builder_with_provider(roots.clone(), provider.clone())
                    .build()
                    .map_err(|error| MicroError::transport(format!("TLS setup failed: {error}")))?;
            server.with_client_cert_verifier(verifier)
        }
        None => server.with_no_client_auth(),
    }
    .with_single_cert(chain.clone(), private_key.clone_key())
    .map_err(tls_error)?;

    let client = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?;
    let client = match &ca_roots {
        Some(roots) => client
            .with_root_certificates(roots.clone())
            .with_client_auth_cert(chain, private_key)
            .map_err(tls_error)?,
        None => {
            let roots = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            client.with_root_certificates(roots).with_no_client_auth()
        }
    };
    Ok(Tls {
        acceptor: TlsAcceptor::from(Arc::new(server)),
        connector: TlsConnector::from(Arc::new(client)),
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let envelope = Envelope::new("deals.get", "{\"id\":1}").with_header("x-request-id", "r1");
        let (kind, id, back) = decode(encode(Kind::Request, 42, &envelope)).unwrap();
        assert_eq!((kind, id), (Kind::Request, 42));
        assert_eq!(back.pattern(), "deals.get");
        assert_eq!(back.header("x-request-id"), Some("r1"));
        assert_eq!(back.payload().as_ref(), b"{\"id\":1}");
        assert!(decode(Bytes::from_static(b"\x09\0\0\0\0\0\0\0\0")).is_err());
        assert!(decode(Bytes::from_static(b"\x01")).is_err());
    }

    #[test]
    fn events_go_to_every_matching_route_once() {
        let options = TcpOptions {
            routes: BTreeMap::from([
                ("deal.*".to_owned(), "a:1".to_owned()),
                ("deal.won".to_owned(), "b:1".to_owned()),
                ("deal.>".to_owned(), "a:1".to_owned()),
                ("company.*".to_owned(), "c:1".to_owned()),
            ]),
            ..TcpOptions::default()
        };
        let transport = TcpTransport::new(&options, None, 1024, Duration::from_secs(1)).unwrap();
        assert_eq!(transport.event_peers("deal.won"), ["a:1", "b:1"]);
        assert_eq!(transport.event_peers("company.renamed"), ["c:1"]);
        assert!(transport.event_peers("user.created").is_empty());
        let fallback = TcpTransport::new(
            &TcpOptions::default(),
            Some("d:1".into()),
            1024,
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(fallback.event_peers("user.created"), ["d:1"]);
    }

    /// The listener says it is subscribed once its socket is bound, and not before: a peer
    /// that connects on the signal must find it. A transport that signals when `listen_subscribed`
    /// is called, before the future has run, has said so with no socket yet, which is seen
    /// here by looking before the future is polled, and again after.
    #[tokio::test]
    async fn the_listener_is_subscribed_once_it_is_bound() {
        use std::future::Future;

        let options = TcpOptions {
            addr: "127.0.0.1:0".to_owned(),
            ..TcpOptions::default()
        };
        let transport = TcpTransport::new(&options, None, 1024, Duration::from_secs(1)).unwrap();
        let (sender, _deliveries) = mpsc::channel(1);
        let (signal, mut answered) = Subscribed::channel();
        let mut listening = Box::pin(transport.listen_subscribed(Vec::new(), sender, signal));
        assert!(
            answered.try_recv().is_err(),
            "subscribed before anything was listening"
        );
        assert!(transport.local_addr().is_none());
        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        assert!(listening.as_mut().poll(&mut cx).is_pending(), "it listens");
        assert!(
            answered.try_recv().is_ok(),
            "the listener did not say it was subscribed once bound"
        );
        let addr = transport.local_addr().expect("bound");
        // A peer that connects on the signal finds the socket.
        tokio::net::TcpStream::connect(addr).await.expect("connect");
    }

    #[test]
    fn tls_files_must_exist_and_pair_up() {
        let mut options = TcpOptions::default();
        options.tls.cert = Some("missing-cert.pem".into());
        let error = TcpTransport::new(&options, None, 1024, Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("go together"), "{error}");
        options.tls.key = Some("missing-key.pem".into());
        let error = TcpTransport::new(&options, None, 1024, Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("missing-cert.pem"), "{error}");
    }
}
