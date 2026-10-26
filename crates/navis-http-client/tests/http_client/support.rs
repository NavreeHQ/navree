//! A tiny local HTTP/1.1 server for tests that need a real socket.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use navis_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A server on `127.0.0.1` that answers every request with one canned
/// response and remembers the raw request heads it received.
pub struct TestServer {
    pub addr: SocketAddr,
    heads: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    /// Starts a server answering `response` (a full HTTP/1.1 response).
    pub async fn start(response: &'static str) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = heads.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    if let Some(head) = answer(stream, response).await {
                        seen.lock().unwrap().push(head);
                    }
                });
            }
        });
        TestServer { addr, heads }
    }

    /// `http://127.0.0.1:<port><path>`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// The request heads received so far (lower-cased).
    pub fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

/// Reads one request head, writes `response`, closes.
async fn answer(mut stream: TcpStream, response: &str) -> Option<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    stream.write_all(response.as_bytes()).await.ok()?;
    stream.shutdown().await.ok()?;
    Some(String::from_utf8_lossy(&buffer).to_ascii_lowercase())
}

/// `200 OK` with body `ok`.
pub const OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";

/// A real client whose SSRF allow list is `allow`.
pub fn client_allowing(allow: &[&str]) -> HttpClient {
    HttpClient::new(HttpClientOptions {
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: allow.iter().map(|s| s.to_string()).collect(),
        },
        ..HttpClientOptions::default()
    })
    .unwrap()
}
