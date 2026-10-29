//! navree-notify integration tests. Every test uses a fresh database of its
//! own (a temporary SQLite file, or a database on the server
//! `NAVREE_TEST_DATABASE` names: see `database.rs`); the HTTP drivers run
//! against a local mock server.

mod support;

mod attribute;
mod channels;
mod conformance;
mod database;
mod drivers;
mod external;
mod held;
mod http;
mod module;
mod opt_outs;
mod orm;
mod people;
mod preferences;
mod push;
mod queue;
mod recipient;
mod rendering;
mod servers;
mod telemetry;
mod uow;
