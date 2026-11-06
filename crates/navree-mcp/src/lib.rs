//! # navree-mcp
//!
//! `McpModule`: the app's own MCP server. Methods on services become the
//! tools, resources and prompts AI clients (Claude, ChatGPT, Cursor, any
//! MCP client) use, served over **streamable HTTP** at `/mcp` on the app's
//! server and over **stdio** (`mcp:serve --user <email>`). The protocol
//! layer is Navree's own ([`protocol`]): JSON-RPC 2.0, the lifecycle and
//! version negotiation (protocols `2024-11-05` to `2025-11-25` with the
//! `initialize` handshake, and the stateless `2026-07-28`), and both
//! transports.
//!
//! - An MCP server is an injectable whose `impl` block is marked
//!   `#[mcp_server]` (navree-macros); its methods marked `#[mcp_tool]`,
//!   `#[mcp_resource("crm://deals/{id}")]` and `#[mcp_prompt]` become what
//!   the server offers ([`McpServer`], [`McpItem`]; hand-written servers use
//!   [`ToolDef`], [`ResourceDef`] and [`PromptDef`]). Modules register
//!   servers with [`ModuleBuilderMcpExt::mcp_server`]; they merge into one
//!   endpoint, and two items with the same name fail the boot.
//! - **Every request is authenticated** by the app's authenticators
//!   (`mcp.auth = "bearer"`, or `"oauth"` with the RFC 9728 metadata), and
//!   every item declares who may use it, like a route: `#[public]`,
//!   `#[authenticated]`, `#[roles]`, `#[permissions]` or `#[can]` (deny by
//!   default). Lists show only what the user may use; calls are checked,
//!   `from = "id"` record checks included.
//! - Tool arguments are a struct deriving `Deserialize`, `navree_openapi::ToSchema`
//!   and `navree_validation::Validate`: the input schema comes from `ToSchema`
//!   ([`input_schema`]) and invalid arguments are refused with field
//!   messages before the method runs.
//! - A call runs in its own unit of work (committed when it returns `Ok`)
//!   with an [`McpContext`]: the user, their access, list filters, record
//!   checks, the clock, the session, the client and progress
//!   notifications. Errors become tool results marked `isError` whose text
//!   is Navree's error body.
//! - [`McpNotifier`] tells subscribed sessions that a resource changed and
//!   every session that a list changed.
//! - The HTTP transport refuses browser origins not in
//!   `mcp.allowed_origins` (DNS rebinding), binds each session (256 random
//!   bits) to the user who opened it, caps bodies at `mcp.max_request_bytes`
//!   and JSON nesting at [`protocol::MAX_JSON_DEPTH`], and queues at most a
//!   few dozen messages for a client that reads slowly.
//! - [`testing::McpTestClient`] speaks the protocol to a `TestApp` in
//!   process, as whoever its `TestClient` acts as.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use navree_validation::Validate;
//! use navree_http::AppError;
//! use navree_kernel::{BoxFuture, Injectable, KernelError, Resolver};
//! use navree_mcp::{Call, McpItem, McpOutput, McpServer, ToolDef};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, Validate, navree_openapi::ToSchema)]
//! struct Greet {
//!     /// Who to greet.
//!     #[validate(length(min = 1, max = 50))]
//!     name: String,
//! }
//!
//! struct Greeter;
//!
//! impl Injectable for Greeter {
//!     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
//!         Box::pin(async { Ok(Greeter) })
//!     }
//! }
//!
//! // What `#[mcp_server]` writes for a `greet` method.
//! impl McpServer for Greeter {
//!     fn items(self: Arc<Self>) -> Vec<McpItem> {
//!         vec![McpItem::tool(
//!             ToolDef::new("greet")
//!                 .description("Greets someone.")
//!                 .read_only()
//!                 .authenticated()
//!                 .input::<Greet>(),
//!             |call: Call| async move {
//!                 let args: Greet = call.args().await?;
//!                 Ok::<_, AppError>(McpOutput::Json(format!("Hello, {}!", args.name).into()))
//!             },
//!         )]
//!     }
//! }
//! ```
#![forbid(unsafe_code)]

// The `#[mcp_server]` expansion names `::navree_mcp` when the facade is not
// a dependency, this crate's own tests included.
extern crate self as navree_mcp;

mod content;
mod context;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
mod handler;
mod http;
mod item;
mod module;
mod notifier;
mod observe;
mod options;
pub mod protocol;
mod registry;
mod schema;
mod session;
mod sessions;
mod stdio;
mod uri;

#[cfg(feature = "testing")]
pub mod testing;

#[doc(hidden)]
#[path = "private.rs"]
pub mod __private;

pub use content::{PromptMessage, ResourceContents};
pub use context::{McpClient, McpContext};
pub use item::{
    Call, Handler, HandlerFuture, McpItem, McpOutput, McpServer, PromptDef, ResourceDef, ToolDef,
};
pub use module::{McpModule, ModuleBuilderMcpExt};
pub use notifier::McpNotifier;
pub use options::{McpAuth, McpOAuthOptions, McpOptions, McpSettings, OAuthSettings};
pub use registry::McpRegistry;
pub use schema::{PromptArgument, input_schema};
pub use stdio::serve_stdio;
