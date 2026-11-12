//! # navree-navigator
//!
//! Navigator is the MCP server for your coding agent: `navree mcp` (the
//! CLI) starts it over stdio in your app's directory, and the agent asks the
//! app itself instead of guessing from the source.
//!
//! | Tool | Answers | From |
//! |---|---|---|
//! | `app_info` | Navree and Rust versions, facade features, modules, database driver, config files | `Cargo.lock`, `Cargo.toml`, the binary |
//! | `list_routes` | Every route: method, path, name, access, module, handler, and whether it is a page (`.page()`); only the pages with `pages_only` | `<app> routes --json` |
//! | `module_graph`, `list_providers`, `list_commands` | Modules, imports, exports, providers, commands | `<app> introspect --json` |
//! | `describe_schema` | Tables, columns, indexes, foreign keys, migrations run and pending | the database, `<app> migrate:status --json` |
//! | `query_database` | One read-only SQL statement, at most 100 rows | the database ([`database`]) |
//! | `read_logs`, `last_error` | Recent log lines; the last error with its request | `log.file` ([`logs`]) |
//! | `read_config` | The merged configuration, secrets masked; with `defaults`, every default too | `<app> config:show --json`, else navree-config ([`Project::config`]) |
//! | `search_docs`, `read_doc` | The docs of the app's Navree version | [`docs`] |
//! | `list_components` | The Navree UI components: id, group, summary, Rust types, docs slug | the catalog built into Navigator ([`ui::Catalog`]) |
//! | `render_page` | What one page draws, as an outline: title, headings, links, buttons, forms with their fields, components used | `<app> ui:render <path> --json` ([`ui::outline`]) |
//! | `lighthouse_list_issues`, `lighthouse_get_issue`, `lighthouse_slow_routes`, `lighthouse_slow_queries`, `lighthouse_failed_jobs` | Lighthouse's issues and figures, when the app includes Lighthouse | Lighthouse's tables ([`lighthouse`]) |
//!
//! [`Navigator`] answers the tools; [`server`] puts them on MCP (a small
//! server of its own, on stdio);
//! [`app::AppProbe`] builds and asks the app binary through a
//! [`app::Runner`] ([`app::FakeRunner`] in tests).
//!
//! The crate also embeds the agent skills of the repository's `skills/`
//! directory: [`skills::install`] is `navree skills:install`.
//!
//! The component catalog (`docs/ui-data/catalog.json`, cut down)
//! is embedded the same way, so `list_components` works offline and names
//! the Navree version it describes.
//!
//! Everything is read-only and local: Navigator writes nothing to the app,
//! reads its database read-only (as far as the database role allows: a
//! PostgreSQL superuser or a role with server-file, server-program or signal
//! privileges is refused for queries unless `--allow-privileged-database-role`
//! is given), connects only to databases on this machine unless told
//! otherwise, and reaches the network only to download the docs of the
//! app's Navree version once. `render_page` has the app draw a page with a
//! `GET` as a guest, in the app's own process; no server needs to run.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod app;
pub mod database;
pub mod docs;
pub mod lighthouse;
pub mod logs;
pub mod project;
pub mod server;
pub mod skills;
pub mod tools;
pub mod ui;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

pub use project::Project;
pub use server::{NavigatorServer, ServeError, run_stdio, serve};
pub use tools::{Navigator, NavigatorOptions, ToolDef, ToolError, ToolOutput, tools};
