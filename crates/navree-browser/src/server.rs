//! [`TestServer`]: an app's real router on a real port.

use std::net::SocketAddr;

use navree_http::HttpOptions;
use navree_kernel::AppContext;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::BrowserError;

/// Serves an app's routes on `127.0.0.1` with a free port until dropped, so
/// a browser (or a front end) can reach it.
///
/// It runs the app's full router (`navree_http::build_router`) with every
/// layer, so CORS, cookies, CSRF and rate limits behave as in production,
/// and pages the app renders itself can be visited at [`url`](Self::url).
/// The server runs on the calling test's runtime.
///
/// ```no_run
/// # async fn demo(ctx: &navree_kernel::AppContext) -> Result<(), navree_browser::BrowserError> {
/// use navree_browser::TestServer;
///
/// let server = TestServer::start(ctx).await?;
/// let url = server.url("/api/v1/deals"); // "http://127.0.0.1:52417/api/v1/deals"
/// # Ok(())
/// # }
/// ```
pub struct TestServer {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestServer {
    /// Builds the app's router and serves it on `127.0.0.1:0`.
    pub async fn start(ctx: &AppContext) -> Result<TestServer, BrowserError> {
        let router = navree_http::build_router(ctx).await.map_err(|error| {
            BrowserError::Server(format!("cannot build the app's router: {error}"))
        })?;
        let options = ctx.get::<HttpOptions>().await.map_err(|error| {
            BrowserError::Server(format!("cannot read the HTTP options: {error}"))
        })?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| BrowserError::io("cannot listen on 127.0.0.1", e))?;
        let addr = listener
            .local_addr()
            .map_err(|e| BrowserError::io("cannot read the listening address", e))?;
        let (shutdown, stop) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = navree_http::serve(listener, router)
                .options(&options)
                .with_graceful_shutdown(async {
                    let _ = stop.await;
                })
                .await;
        });
        Ok(TestServer {
            addr,
            shutdown: Some(shutdown),
            task,
        })
    }

    /// `http://127.0.0.1:<port><path>`.
    pub fn url(&self, path: &str) -> String {
        join_url(&format!("http://{}", self.addr), path)
    }

    /// The address it listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.abort();
    }
}

impl std::fmt::Debug for TestServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestServer")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

/// `base` followed by `path`, with exactly one `/` between them.
pub(crate) fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    if path.is_empty() {
        base.to_owned()
    } else if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_join_with_one_slash() {
        assert_eq!(join_url("http://h:1", "/a"), "http://h:1/a");
        assert_eq!(join_url("http://h:1/", "/a?b=1"), "http://h:1/a?b=1");
        assert_eq!(join_url("http://h:1", "a"), "http://h:1/a");
        assert_eq!(join_url("http://h:1/", ""), "http://h:1");
    }
}
