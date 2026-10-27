//! The MCP server: [`Navigator`]'s tools over the Model Context Protocol
//! (rmcp), on stdio for `navree mcp` or any byte stream.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::tools::{Navigator, NavigatorOptions, ToolDef, ToolOutput, tools};

/// What the server tells the agent when it connects.
pub const INSTRUCTIONS: &str = "Navigator answers questions about this Navree app from the app itself. \
Call app_info first in a new project. Check list_routes before adding a route, module_graph or \
list_providers before wiring a provider, describe_schema before writing a migration or a query, and \
last_error or read_logs when a request fails. search_docs and read_doc read the docs of the Navree \
version in Cargo.lock. Every tool is read-only.";

/// The MCP handler around a [`Navigator`].
#[derive(Clone, Debug)]
pub struct NavigatorServer {
    navigator: Arc<Navigator>,
}

impl NavigatorServer {
    /// A server answering with `navigator`.
    pub fn new(navigator: Arc<Navigator>) -> NavigatorServer {
        NavigatorServer { navigator }
    }
}

/// The protocol form of a tool definition.
fn tool(def: &ToolDef) -> Tool {
    let value = json!({
        "name": def.name,
        "title": def.title,
        "description": def.description,
        "inputSchema": def.input_schema,
        "annotations": {
            "title": def.title,
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": def.open_world,
        },
    });
    serde_json::from_value(value).expect("a valid tool definition")
}

/// A tool's answer as a tool result: JSON as pretty text (objects also as
/// structured content), Markdown as text, errors as error results.
fn result(output: Result<ToolOutput, crate::tools::ToolError>) -> CallToolResult {
    match output {
        Ok(ToolOutput::Json(value)) => {
            let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
            if value.is_object() {
                result.structured_content = Some(value);
            }
            result
        }
        Ok(ToolOutput::Markdown(markdown)) => {
            CallToolResult::success(vec![ContentBlock::text(markdown)])
        }
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error.0)]),
    }
}

impl ServerHandler for NavigatorServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("navree", env!("CARGO_PKG_VERSION"))
                    .with_title("Navigator")
                    .with_website_url("https://navree.com/docs/ai/navigator/"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tools().iter().map(tool).collect()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if !tools().iter().any(|t| t.name == name) {
            return Err(ErrorData::invalid_params(format!("Unknown tool: {name}"), None));
        }
        let args = request.arguments.map_or(Value::Null, Value::Object);
        Ok(result(self.navigator.call(&name, args).await).into())
    }
}

/// Why the server stopped with an error.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The client never completed the MCP handshake.
    #[error("the MCP client did not initialize: {0}")]
    Initialize(String),
    /// The session ended with an error.
    #[error("the MCP session failed: {0}")]
    Session(String),
    /// The async runtime could not start.
    #[error("cannot start the async runtime: {0}")]
    Runtime(#[from] std::io::Error),
}

/// Serves `navigator` on `reader` and `writer` (JSON-RPC messages, one per
/// line) until the client closes the stream.
pub async fn serve<R, W>(navigator: Arc<Navigator>, reader: R, writer: W) -> Result<(), ServeError>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let service = NavigatorServer::new(navigator)
        .serve((reader, writer))
        .await
        .map_err(|e| ServeError::Initialize(e.to_string()))?;
    service
        .waiting()
        .await
        .map_err(|e| ServeError::Session(e.to_string()))?;
    Ok(())
}

/// `navree mcp`: serves Navigator for the app at `options.path` on stdin and
/// stdout until the client goes away. Nothing else is written to stdout.
pub fn run_stdio(options: NavigatorOptions) -> Result<(), ServeError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let navigator = Arc::new(Navigator::new(options));
        serve(navigator, tokio::io::stdin(), tokio::io::stdout()).await
    })
}
