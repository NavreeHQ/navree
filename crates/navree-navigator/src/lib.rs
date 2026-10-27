//! # navree-navigator
//!
//! Navigator is the MCP server for your coding agent: `navree mcp` (the
//! CLI) starts it over stdio in your app's directory, and the agent asks the
//! app itself instead of guessing from the source.
//!
//! | Tool | Answers | From |
//! |---|---|---|
//! | `app_info` | Navree and Rust versions, facade features, modules, database driver, config files | `Cargo.lock`, `Cargo.toml`, the binary |
//! | `list_routes` | Every route: method, path, name, access, module, handler | `<app> routes --json` |
//! | `module_graph`, `list_providers`, `list_commands` | Modules, imports, exports, providers, commands | `<app> introspect --json` |
//! | `describe_schema` | Tables, columns, indexes, foreign keys, migrations run and pending | the database, `<app> migrate:status --json` |
//! | `query_database` | One read-only SQL statement, at most 100 rows | the database ([`database`]) |
//! | `read_logs`, `last_error` | Recent log lines; the last error with its request | `log.file` ([`logs`]) |
//! | `read_config` | The merged configuration, secrets redacted | navree-config ([`Project::config`]) |
//! | `search_docs`, `read_doc` | The docs of the app's Navree version | [`docs`] |
//!
//! [`Navigator`] answers the tools; [`server`] puts them on MCP (rmcp);
//! [`app::AppProbe`] builds and asks the app binary through a
//! [`app::Runner`] ([`app::FakeRunner`] in tests).
//!
//! The crate also embeds the agent skills of the repository's `skills/`
//! directory: [`skills::install`] is `navree skills:install`.
//!
//! Everything is read-only and local: Navigator writes nothing to the app
//! or its database, connects only to databases on this machine unless told
//! otherwise, and reaches the network only to download the docs of the
//! app's Navree version once.
#![forbid(unsafe_code)]

pub mod app;
pub mod database;
pub mod docs;
pub mod logs;
pub mod project;
pub mod server;
pub mod skills;
pub mod tools;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

pub use project::Project;
pub use server::{NavigatorServer, ServeError, run_stdio, serve};
pub use tools::{Navigator, NavigatorOptions, ToolDef, ToolError, ToolOutput, tools};
