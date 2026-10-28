//! navree-mcp integration tests: `#[mcp_server]` servers in a `TestApp`,
//! spoken to through the full HTTP pipeline with `McpTestClient`, as raw
//! JSON-RPC over HTTP and stdio, and through the security requirements.

mod support;

mod boot;
mod http;
mod notifications;
mod observability;
mod prompts;
mod resources;
mod security;
mod stateless;
mod stdio;
mod stdio_wire;
mod subjects;
mod tools;
mod wire;
