//! Live streams to pages over the Redis backplane
//! (`NAVREE_TEST_REDIS_URL`): a nudge published in one `serve` process
//! reaches a page whose stream is held by another, and the page resumes on
//! either with the same id.

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use navree_contracts::Secret;
use navree_http::{Authenticator, HttpModule, HttpOptions, build_router};
use navree_kernel::{App, Navree};
use navree_realtime::{BackplaneKind, Live, RealtimeModule, RealtimeOptions, Topic};
use navree_redis::RedisModule;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::support::{NoAuthenticator, TestRedis};

const WAIT: Duration = Duration::from_secs(5);

/// An app with the realtime module on the Redis backplane, served on
/// 127.0.0.1:0.
struct Node {
    app: App,
    addr: SocketAddr,
}

async fn node(t: &TestRedis) -> Node {
    let app = Navree::app()
        .module(RedisModule::for_root(t.options()))
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .module(RealtimeModule::for_root(RealtimeOptions {
            app_id: "live".to_owned(),
            key: "test-key".to_owned(),
            secret: Secret::new("shared-secret".to_owned()),
            backplane: BackplaneKind::Redis,
            ..RealtimeOptions::default()
        }))
        .override_value::<dyn Authenticator>(Arc::new(NoAuthenticator))
        .boot()
        .await
        .expect("app boots");
    let router = build_router(app.ctx()).await.expect("router builds");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(
        navree_http::serve(listener, router)
            .with_graceful_shutdown(app.ctx().shutdown_token().cancelled_owned())
            .into_future(),
    );
    Node { app, addr }
}

/// A page's stream, read as raw text over HTTP/1.1.
struct Page {
    socket: TcpStream,
    seen: String,
}

impl Page {
    /// Opens `GET /broadcasting/stream?{query}` with `last` as `Last-Event-ID`.
    async fn open(node: &Node, query: &str, last: Option<&str>) -> Page {
        let mut socket = TcpStream::connect(node.addr).await.unwrap();
        let resume = last.map_or_else(String::new, |id| format!("Last-Event-ID: {id}\r\n"));
        let request = format!(
            "GET /broadcasting/stream?{query} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\n{resume}\r\n",
            node.addr
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        Page {
            socket,
            seen: String::new(),
        }
    }

    /// Reads until the text holds `needle`; returns what came before it and
    /// it, and keeps the rest.
    async fn until(&mut self, needle: &str) -> String {
        tokio::time::timeout(WAIT, async {
            loop {
                if let Some(at) = self.seen.find(needle) {
                    let upto: String = self.seen.drain(..at + needle.len()).collect();
                    return upto;
                }
                let mut buffer = [0u8; 4096];
                let read = self.socket.read(&mut buffer).await.unwrap();
                assert!(
                    read > 0,
                    "the stream ended before `{needle}`: {}",
                    self.seen
                );
                self.seen
                    .push_str(&String::from_utf8_lossy(&buffer[..read]));
            }
        })
        .await
        .unwrap_or_else(|_| panic!("`{needle}` within {WAIT:?}; got {}", self.seen))
    }

    /// The id of the next `event: <name>` nudge.
    async fn nudge(&mut self, name: &str) -> String {
        let upto = self.until(&format!("event: {name}\n")).await;
        let id_line = upto
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("id: "))
            .expect("an id before the event");
        id_line.trim().to_owned()
    }
}

#[tokio::test]
async fn a_nudge_published_in_one_process_reaches_a_page_on_another_and_resumes_on_either() {
    let Some(t) = TestRedis::connect(
        "a_nudge_published_in_one_process_reaches_a_page_on_another_and_resumes_on_either",
    )
    .await
    else {
        return;
    };
    let (a, b) = (node(&t).await, node(&t).await);
    let mut on_b = Page::open(&b, "topics=orders", None).await;
    on_b.until("event: ui:live\n").await;
    // A keeps broadcasts once it has served a stream.
    let mut on_a = Page::open(&a, "topics=orders", None).await;
    on_a.until("event: ui:live\n").await;

    let from_a = a.app.ctx().get::<Live>().await.expect("Live");
    from_a.nudge([Topic::public("orders")]).await.unwrap();
    let last = on_b.nudge("orders").await;
    drop(on_b);
    from_a.nudge([Topic::public("orders")]).await.unwrap();
    on_a.nudge("orders").await;
    on_a.nudge("orders").await;

    // The page comes back through the load balancer to A with B's last id.
    let mut back = Page::open(&a, "topics=orders", Some(&last)).await;
    back.until("event: ui:resumed\n").await;
    back.nudge("orders").await;

    a.app.shutdown().await.unwrap();
    b.app.shutdown().await.unwrap();
    t.cleanup().await;
}
