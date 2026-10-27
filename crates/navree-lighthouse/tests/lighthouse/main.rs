//! Integration tests of navree-lighthouse: recording through a running
//! app, issue statuses, alerts, the dashboard and its API, the commands,
//! the fake and booting the module.

mod support;

mod alerts;
mod commands;
mod dashboard;
mod fake;
#[cfg(feature = "mcp")]
mod mcp;
mod module;
mod recording;
mod statuses;
