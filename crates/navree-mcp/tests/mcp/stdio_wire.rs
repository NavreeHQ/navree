//! The stdio transport on the wire: newline-delimited JSON-RPC through
//! `serve_stdio`, spoken by a raw client over an in-memory pipe.

use std::time::Duration;

use navree_kernel::BoxError;
use navree_testing::TestApp;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::support::{app, insert_deal};

const WAIT: Duration = Duration::from_secs(10);

/// A raw line client of `serve_stdio`.
pub struct LineClient {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
    /// Notifications read while waiting for answers.
    pub notifications: Vec<Value>,
    pub server: JoinHandle<Result<(), BoxError>>,
}

impl LineClient {
    /// Serves `app` over a pipe as `user`.
    pub fn connect(app: &TestApp, user: &str) -> LineClient {
        let (client_side, server_side) = tokio::io::duplex(1 << 16);
        let (server_read, server_write) = tokio::io::split(server_side);
        let ctx = app.ctx().clone();
        let user = user.to_owned();
        let server = tokio::spawn(async move {
            navree_mcp::serve_stdio(&ctx, &user, server_read, server_write).await
        });
        let (reader, writer) = tokio::io::split(client_side);
        LineClient {
            reader: BufReader::new(reader),
            writer,
            notifications: Vec::new(),
            server,
        }
    }

    /// Writes one line.
    pub async fn write_line(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.write_all(b"\n").await.unwrap();
        self.writer.flush().await.unwrap();
    }

    /// Sends one message.
    pub async fn send(&mut self, message: &Value) {
        self.write_line(&message.to_string()).await;
    }

    /// The next message the server writes, or `None` within `wait`.
    pub async fn next(&mut self, wait: Duration) -> Option<Value> {
        let mut line = String::new();
        match tokio::time::timeout(wait, self.reader.read_line(&mut line)).await {
            Ok(Ok(0)) | Err(_) => None,
            Ok(Ok(_)) => Some(serde_json::from_str(&line).expect("a JSON line")),
            Ok(Err(error)) => panic!("read failed: {error}"),
        }
    }

    /// The answer to request `id`, keeping notifications that came first.
    pub async fn answer(&mut self, id: &Value) -> Value {
        loop {
            let message = self
                .next(WAIT)
                .await
                .unwrap_or_else(|| panic!("no answer to {id}"));
            if &message["id"] == id && message.get("method").is_none() {
                return message;
            }
            self.notifications.push(message);
        }
    }

    /// Sends request `id` and waits for its answer.
    pub async fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(&message).await;
        self.answer(&json!(id)).await
    }

    /// The handshake at `version`; returns the `initialize` result.
    pub async fn initialize(&mut self, version: &str) -> Value {
        let answer = self
            .request(
                0,
                "initialize",
                json!({
                    "protocolVersion": version,
                    "capabilities": {},
                    "clientInfo": { "name": "line-client", "version": "1" },
                }),
            )
            .await;
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        answer["result"].clone()
    }

    /// Closes the client's side and waits for the server to end.
    pub async fn close(self) -> Result<(), BoxError> {
        let LineClient {
            reader,
            mut writer,
            server,
            ..
        } = self;
        writer.shutdown().await.unwrap();
        drop(writer);
        drop(reader);
        tokio::time::timeout(WAIT, server)
            .await
            .expect("the server ends with the client")
            .unwrap()
    }
}

#[tokio::test]
async fn the_handshake_lists_and_calls() {
    let app = app().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, rep, "Agra renewal", "lead").await;
    let mut client = LineClient::connect(&app, &rep.to_string());

    let info = client.initialize("2025-06-18").await;
    assert_eq!(info["protocolVersion"], "2025-06-18");
    assert_eq!(info["serverInfo"]["name"], "navree-crm");
    assert_eq!(info["capabilities"]["tools"]["listChanged"], true);

    assert_eq!(
        client.request(1, "ping", Value::Null).await["result"],
        json!({})
    );
    let tools = client.request(2, "tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"list_deals"), "{names:?}");

    let called = client
        .request(
            3,
            "tools/call",
            json!({ "name": "rename_deal", "arguments": { "deal_id": deal.id, "title": "Renamed" } }),
        )
        .await;
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(called["result"]["structuredContent"]["title"], "Renamed");

    let read = client
        .request(
            4,
            "resources/read",
            json!({ "uri": format!("crm://deals/{}", deal.id) }),
        )
        .await;
    assert_eq!(read["result"]["contents"].as_array().unwrap().len(), 1);

    let prompt = client
        .request(
            5,
            "prompts/get",
            json!({ "name": "summarize", "arguments": { "days": "3" } }),
        )
        .await;
    assert_eq!(
        prompt["result"]["messages"][0]["content"]["text"],
        "Summarize the last 3 days."
    );

    let unknown = client.request(6, "no/such/method", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);

    // String ids are answered with the same id.
    let answered = client.request_with_id(json!("abc"), "ping").await;
    assert_eq!(answered["result"], json!({}));

    client.close().await.expect("the server ends cleanly");
}

impl LineClient {
    async fn request_with_id(&mut self, id: Value, method: &str) -> Value {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method }))
            .await;
        self.answer(&id).await
    }
}

#[tokio::test]
async fn progress_arrives_before_the_result() {
    let app = app().await;
    let mut client = LineClient::connect(&app, &Uuid::now_v7().to_string());
    client.initialize("2025-11-25").await;
    let exported = client
        .request(
            1,
            "tools/call",
            json!({ "name": "export_deals", "arguments": {}, "_meta": { "progressToken": "p1" } }),
        )
        .await;
    assert_eq!(exported["result"]["structuredContent"]["result"], 2);
    let progress: Vec<&Value> = client
        .notifications
        .iter()
        .filter(|message| message["method"] == "notifications/progress")
        .collect();
    assert_eq!(progress.len(), 2, "{:?}", client.notifications);
    assert_eq!(progress[0]["params"]["progressToken"], "p1");
    assert_eq!(progress[0]["params"]["progress"], 1.0);
    client.close().await.unwrap();
}

#[tokio::test]
async fn the_stateless_protocol_works_over_stdio() {
    let app = app().await;
    let mut client = LineClient::connect(&app, &Uuid::now_v7().to_string());
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "line-client", "version": "1" },
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let discovered = client
        .request(1, "server/discover", json!({ "_meta": meta }))
        .await;
    assert_eq!(
        discovered["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "navree-crm"
    );
    let listed = client
        .request(2, "tools/list", json!({ "_meta": meta }))
        .await;
    assert_eq!(listed["result"]["resultType"], "complete");
    // Without its metadata a request is refused.
    let refused = client.request(3, "tools/list", json!({})).await;
    assert_eq!(refused["error"]["code"], -32602);
    client.close().await.unwrap();
}
