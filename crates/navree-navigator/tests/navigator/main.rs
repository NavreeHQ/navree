//! Navigator's tools against a fixture app: a SQLite database, a log file,
//! configuration, the embedded docs, and the app binary through a fake
//! process runner; then the MCP protocol as raw JSON-RPC lines.

mod support;

mod app_tools;
mod database;
mod docs;
mod files;
mod lighthouse;
mod postgres;
mod protocol;
mod ui;
