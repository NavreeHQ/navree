//! navree-db integration tests. Every test uses a fresh temporary SQLite file.

mod fixtures;
mod support;

mod connect;
mod http;
mod migrations;
mod module;
mod paginate;
mod repo;
mod schema_migrations;
mod seed;
mod telemetry;
mod text;
mod uow;
