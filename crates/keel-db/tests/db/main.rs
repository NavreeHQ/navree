//! keel-db integration tests. Every test uses a fresh temporary SQLite file.

mod fixtures;
mod support;

mod connect;
mod http;
mod migrations;
mod module;
mod paginate;
mod repo;
mod seed;
mod text;
mod uow;
