//! navree-mcp integration tests: `#[mcp_server]` servers in a `TestApp`,
//! spoken to through the full HTTP pipeline with `McpTestClient`, and over
//! stdio with the official SDK's client.

mod support;

mod boot;
mod http;
mod notifications;
mod observability;
mod prompts;
mod resources;
mod stdio;
mod tools;
