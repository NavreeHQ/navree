//! navree-db integration tests. Every test uses a fresh temporary SQLite file.

mod fixtures;
mod support;

mod change_events;
mod connect;
mod connections;
mod http;
mod migrations;
mod module;
mod mysql_sql;
mod paginate;
mod queries;
mod repo;
mod schema_migrations;
mod seed;
mod telemetry;
mod text;
mod time_series;
mod transaction_settings;
mod uow;
