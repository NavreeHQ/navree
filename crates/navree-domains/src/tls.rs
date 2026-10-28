//! [`HttpsServer`]: the app's router over TLS with the certificates of
//! [`DomainCertificates`](crate::DomainCertificates)
//! (`domains.tls = "acme"` with `domains.acme.https_addr`).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::serve::{Listener, ListenerExt as _};
use navree_kernel::{AppContext, BoxError};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tokio_util::sync::CancellationToken;

use crate::Domains;

/// How long a client has for its TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The app's routes over HTTPS: the same router `serve` runs (every
/// layer, route and guard), behind rustls with the certificate of the
/// server name each client asks for. Certificates are loaded from the
/// database at start and every `domains.acme.reload_secs`, so renewals
/// made by any server are picked up.
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
        let config = self.domains.certificates().server_config()?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let token = self.ctx.shutdown_token();
        let (sender, receiver) = mpsc::channel(256);
        tokio::spawn(accept(self.listener, acceptor, sender, token.clone()));
        tokio::spawn(reload(
            self.domains.clone(),
            Duration::from_secs(self.domains.options().acme.reload_secs.max(1)),
            token.clone(),
        ));
        tracing::info!(target: "navree::domains", addr = %self.local, "listening on https://{}", self.local);
        let listener = Handshaken {
            receiver,
            local: self.local,
        }
        .tap_io(|_| {});
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(token.cancelled_owned())
        .await?;
        tracing::info!(target: "navree::domains", "HTTPS server stopped");
        Ok(())
    }
}

/// Accepts connections and runs their handshakes concurrently, so a slow
/// client never holds up the others.
async fn accept(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    sender: mpsc::Sender<(TlsStream<TcpStream>, SocketAddr)>,
    token: CancellationToken,
) {
    loop {
        let accepted = tokio::select! {
            () = token.cancelled() => return,
            accepted = listener.accept() => accepted,
        };
        let (stream, remote) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::debug!(target: "navree::domains", %error, "accept failed");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let sender = sender.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                Ok(Ok(tls)) => {
                    let _ = sender.send((tls, remote)).await;
                }
                Ok(Err(error)) => {
                    tracing::debug!(target: "navree::domains", %remote, %error, "TLS handshake failed");
                }
                Err(_) => {
                    tracing::debug!(target: "navree::domains", %remote, "TLS handshake timed out");
                }
            }
        });
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

/// The connections whose handshake succeeded.
struct Handshaken {
    receiver: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

impl Listener for Handshaken {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.receiver.recv().await {
            Some(connection) => connection,
            // The accept loop stopped (shutdown): wait for the server to end.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}
