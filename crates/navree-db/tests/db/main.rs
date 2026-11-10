//! navree-db integration tests. Every test uses a fresh temporary SQLite file.

mod fixtures;
mod support;

mod change_events;
mod connect;
mod connections;
mod http;
mod migrations;
mod module;
mod paginate;
mod queries;
mod schema_migrations;
mod seed;
mod sqlite_lock;
mod telemetry;
mod text;
mod time_series;
mod uow;
