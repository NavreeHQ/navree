//! navree-webhooks end to end: outgoing deliveries, incoming verification,
//! the routes and the commands, on SQLite with the memory queue and a fake
//! HTTP client; the built-in incoming sources with their keys served by the
//! fake client.

mod commands;
mod docs_surface;
mod incoming;
mod orm;
mod outgoing;
mod routes;
mod sources;
mod support;
mod time_series;
