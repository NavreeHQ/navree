//! The MCP server spoken to as raw JSON-RPC lines over an in-memory pipe.

use std::sync::Arc;
use std::time::Duration;

use navree_navigator::{ServeError, serve};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio::task::JoinHandle;

use crate::support::Fixture;

const WAIT: Duration = Duration::from_secs(30);

/// A raw line client of the server.
struct Client {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
    server: JoinHandle<Result<(), ServeError>>,
}

impl Client {
    fn connect(fixture: &Fixture) -> Client {
        let (client_side, server_side) = tokio::io::duplex(1 << 16);
        let (server_read, server_write) = tokio::io::split(server_side);
        let navigator = Arc::new(fixture.navigator());
        let server = tokio::spawn(serve(navigator, server_read, server_write));
        let (reader, writer) = tokio::io::split(client_side);
        Client {
            reader: BufReader::new(reader),
            writer,
            server,
        }
    }

    async fn write_line(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.write_all(b"\n").await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn next(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(WAIT, self.reader.read_line(&mut line))
            .await
            .expect("an answer in time")
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    async fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_line(&message.to_string()).await;
        loop {
            let answer = self.next().await;
            if answer["id"] == id {
                return answer;
            }
        }
    }

    async fn initialize(&mut self) -> Value {
        let answer = self
            .request(
                0,
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "1" },
                }),
            )
            .await;
        self.write_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await;
        answer["result"].clone()
    }

    async fn call(&mut self, id: i64, tool: &str, arguments: Value) -> Value {
        self.request(
            id,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
        .await
    }

    async fn close(self) {
        let Client {
            reader,
            mut writer,
            server,
        } = self;
        writer.shutdown().await.unwrap();
        drop((reader, writer));
        tokio::time::timeout(WAIT, server)
            .await
            .expect("the server ends with the client")
            .unwrap()
            .expect("the server ends cleanly");
    }
}

#[tokio::test]
async fn the_server_lists_fourteen_read_only_tools() {
    let fixture = Fixture::new("[app]\nname = \"crm\"\n");
    let mut client = Client::connect(&fixture);

    let info = client.initialize().await;
    assert_eq!(info["protocolVersion"], "2025-11-25");
    assert_eq!(info["serverInfo"]["name"], "navree");
    assert_eq!(info["serverInfo"]["title"], "Navigator");
    assert_eq!(info["capabilities"], json!({ "tools": {} }));
    assert!(
        info["instructions"]
            .as_str()
            .unwrap_or_default()
            .contains("app_info")
    );

    let tools = client.request(1, "tools/list", json!({})).await["result"]["tools"].clone();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "app_info",
            "list_routes",
            "module_graph",
            "list_providers",
            "list_commands",
            "describe_schema",
            "query_database",
            "read_logs",
            "last_error",
            "read_config",
            "search_docs",
            "read_doc",
            "list_components",
            "render_page"
        ]
    );
    let list_routes = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "list_routes")
        .unwrap();
    assert_eq!(
        list_routes["inputSchema"]["properties"]["pages_only"]["type"],
        "boolean"
    );
    let render_page = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "render_page")
        .unwrap();
    assert_eq!(render_page["inputSchema"]["required"], json!(["path"]));
    for tool in tools.as_array().unwrap() {
        assert_eq!(
            tool["annotations"]["readOnlyHint"], true,
            "{}",
            tool["name"]
        );
        assert_eq!(
            tool["annotations"]["destructiveHint"], false,
            "{}",
            tool["name"]
        );
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    client.close().await;
}

#[tokio::test]
async fn an_app_with_lighthouse_gets_its_five_tools() {
    let fixture = Fixture::new(
        "[app]
name = \"crm\"
",
    );
    crate::support::write(
        &fixture.root().join("src/app.rs"),
        "Navree::app().module(LighthouseModule::from_config())",
    );
    let mut client = Client::connect(&fixture);
    let instructions = client.initialize().await["instructions"].clone();
    assert!(
        instructions
            .as_str()
            .unwrap()
            .contains("lighthouse_list_issues"),
        "{instructions}"
    );
    let tools = client.request(1, "tools/list", json!({})).await["result"]["tools"].clone();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names.len(), 19);
    assert_eq!(names[14..], navree_navigator::lighthouse::LIGHTHOUSE_TOOLS);
    let issue = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "lighthouse_get_issue")
        .unwrap();
    assert_eq!(issue["inputSchema"]["required"], json!(["id"]));
    assert_eq!(issue["annotations"]["readOnlyHint"], true);

    let answer = client.call(2, "lighthouse_list_issues", json!({})).await;
    assert_eq!(answer["result"]["isError"], true, "no database configured");
    client.close().await;
}

#[tokio::test]
async fn tools_answer_with_text_and_structured_content() {
    let fixture = Fixture::new("[app]\nname = \"crm\"\n[mail]\npassword = \"hunter2\"\n");
    let mut client = Client::connect(&fixture);
    client.initialize().await;

    let config = client.call(1, "read_config", json!({})).await["result"].clone();
    assert_eq!(config["isError"], false);
    let structured = &config["structuredContent"];
    assert_eq!(structured["app"]["name"], "crm");
    assert_eq!(structured["mail"]["password"], "[redacted]");
    let text = config["content"][0]["text"].as_str().expect("text");
    assert!(text.contains("\"name\": \"crm\""), "{text}");

    let doc = client
        .call(2, "read_doc", json!({ "slug": "ai/agent-skills" }))
        .await["result"]
        .clone();
    assert_eq!(doc["isError"], false);
    assert!(
        doc["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("# Agent skills")
    );
    assert!(
        doc.get("structuredContent").is_none(),
        "Markdown is text only"
    );

    let refused = client
        .call(3, "query_database", json!({ "sql": "DELETE FROM deals" }))
        .await;
    assert_eq!(
        refused["result"]["isError"], true,
        "a failed tool is a tool error"
    );

    let unknown = client.call(4, "drop_database", json!({})).await;
    assert_eq!(
        unknown["error"]["code"], -32602,
        "an unknown tool is a protocol error"
    );

    client.close().await;
}

#[tokio::test]
async fn the_protocol_refuses_what_it_does_not_speak() {
    let fixture = Fixture::new("[app]\nname = \"crm\"\n");
    let mut client = Client::connect(&fixture);
    // Only ping and discovery before the handshake.
    assert_eq!(
        client.request(1, "ping", json!({})).await["result"],
        json!({})
    );
    let early = client.request(2, "tools/list", json!({})).await;
    assert_eq!(early["error"]["code"], -32602, "{early}");
    client.initialize().await;

    let unknown = client.request(3, "tools/delete", json!({})).await;
    assert_eq!(unknown["error"]["code"], -32601);
    // Batches are not allowed on 2025-11-25.
    client
        .write_line(r#"[{"jsonrpc":"2.0","id":4,"method":"ping"}]"#)
        .await;
    assert_eq!(client.next().await["error"]["code"], -32600);
    // Neither is deep nesting, nor invalid JSON.
    client
        .write_line(&format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"ping","params":{{"x":{}{}}}}}"#,
            "[".repeat(70),
            "]".repeat(70)
        ))
        .await;
    assert_eq!(client.next().await["error"]["code"], -32600);
    client.write_line("{oops").await;
    assert_eq!(client.next().await["error"]["code"], -32700);
    assert_eq!(
        client.request(6, "ping", json!({})).await["result"],
        json!({})
    );
    client.close().await;
}

#[tokio::test]
async fn the_stateless_protocol_needs_its_metadata() {
    let fixture = Fixture::new("[app]\nname = \"crm\"\n");
    let mut client = Client::connect(&fixture);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let discovered = client
        .request(1, "server/discover", json!({ "_meta": meta }))
        .await;
    assert_eq!(
        discovered["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "navree"
    );
    let listed = client
        .request(2, "tools/list", json!({ "_meta": meta }))
        .await;
    assert_eq!(listed["result"]["resultType"], "complete");
    assert_eq!(listed["result"]["ttlMs"], 0);
    let called = client
        .request(
            3,
            "tools/call",
            json!({ "name": "read_config", "arguments": {}, "_meta": meta }),
        )
        .await;
    assert_eq!(called["result"]["resultType"], "complete");
    let refused = client.request(4, "tools/list", json!({})).await;
    assert_eq!(refused["error"]["code"], -32602);
    client.close().await;
}
