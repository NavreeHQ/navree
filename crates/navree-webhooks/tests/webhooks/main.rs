//! navree-webhooks end to end: outgoing deliveries, incoming verification,
//! the routes and the commands, on SQLite with the memory queue and a fake
//! HTTP client.

mod commands;
mod docs_surface;
mod incoming;
mod outgoing;
mod routes;
mod support;
mod time_series;
