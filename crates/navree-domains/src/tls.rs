//! [`HttpsServer`]: the app's router over TLS with the certificates of
//! [`DomainCertificates`](crate::DomainCertificates)
//! (`domains.tls = "acme"` with `domains.acme.https_addr`).

use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use navree_http::HttpOptions;
use navree_http::serve::{Listener, poll_tcp_sent};
use navree_kernel::{AppContext, BoxError};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tokio_util::sync::CancellationToken;

use crate::Domains;

/// How long a client has for its TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The app's routes over HTTPS: the same router `serve` runs (every
/// layer, route and guard), on the same server with the same `http.server`
/// limits, behind rustls with the certificate of the server name each client
/// asks for. Certificates are loaded from the database at start and every
/// `domains.acme.reload_secs`, so renewals made by any server are picked up.
///
/// `DomainsModule` starts one on `domains.acme.https_addr` when the app
/// runs `serve` with `domains.tls = "acme"`. Run one yourself (a test, a
/// command of your own) with [`bind`](HttpsServer::bind) and
/// [`serve`](HttpsServer::serve); it stops with the app.
pub struct HttpsServer {
    ctx: AppContext,
    domains: Arc<Domains>,
    listener: TcpListener,
    local: SocketAddr,
}

impl std::fmt::Debug for HttpsServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpsServer")
            .field("local_addr", &self.local)
            .finish_non_exhaustive()
    }
}

impl HttpsServer {
    /// Loads the certificates and listens on `addr` (`0.0.0.0:443`, or
    /// `127.0.0.1:0` for a free port).
    pub async fn bind(ctx: &AppContext, addr: &str) -> Result<HttpsServer, BoxError> {
        let domains = ctx.get::<Domains>().await?;
        domains.reload_certificates().await?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|error| format!("cannot listen on {addr}: {error}"))?;
        let local = listener.local_addr()?;
        Ok(HttpsServer {
            ctx: ctx.clone(),
            domains,
            listener,
            local,
        })
    }

    /// The address it listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Serves until the app shuts down.
    pub async fn serve(self) -> Result<(), BoxError> {
        let router = navree_http::build_router(&self.ctx).await?;
        let options = self.ctx.get::<HttpOptions>().await?;
        let config = self.domains.certificates().server_config()?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let token = self.ctx.shutdown_token();
        tokio::spawn(reload(
            self.domains.clone(),
            Duration::from_secs(self.domains.options().acme.reload_secs.max(1)),
            token.clone(),
        ));
        tracing::info!(target: "navree::domains", addr = %self.local, "listening on https://{}", self.local);
        let listener = TlsListener {
            listener: self.listener,
            acceptor,
        };
        navree_http::serve(listener, router)
            .options(&options)
            .with_graceful_shutdown(token.cancelled_owned())
            .await?;
        tracing::info!(target: "navree::domains", "HTTPS server stopped");
        Ok(())
    }
}

/// Accepts TCP connections and leaves their TLS handshakes to the server,
/// which runs each on the connection's own task, under
/// `http.server.max_connections`, so a slow client never holds up the others.
struct TlsListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl Listener for TlsListener {
    type Stream = TcpStream;
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (TcpStream, SocketAddr) {
        Listener::accept(&mut self.listener).await
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
    ) -> impl Future<Output = io::Result<TlsStream<TcpStream>>> + Send + 'static {
        let handshake = self.acceptor.accept(stream);
        async move {
            tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake)
                .await
                .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))
        }
    }
}

/// Reloads the certificates from the database every `every`.
async fn reload(domains: Arc<Domains>, every: Duration, token: CancellationToken) {
    loop {
        tokio::select! {
            () = token.cancelled() => return,
            () = tokio::time::sleep(every) => {}
        }
        if let Err(error) = domains.reload_certificates().await {
            tracing::warn!(target: "navree::domains", %error, "could not reload the certificates");
        }
    }
}
