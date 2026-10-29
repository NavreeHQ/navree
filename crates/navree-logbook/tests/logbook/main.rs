//! Integration tests for navree-logbook: the stores (one conformance suite
//! for the memory and the SQLite store), the filter language, every
//! watcher, the module in a `TestApp` (capture, the UI and its API, the
//! loopback guard, replay, pruning, `logbook:clear`), and the docs.

mod servers;
mod support;

mod app;
mod docs_examples;
mod docs_surface;
mod filter;
mod replay;
mod store;
mod ui;
mod watchers;
