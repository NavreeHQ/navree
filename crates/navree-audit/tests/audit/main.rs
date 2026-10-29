//! navree-audit integration tests. Every test uses a fresh database of its
//! own, on SQLite or the server `NAVREE_TEST_DATABASE` names (see `servers`).

mod servers;
mod support;

mod data;
mod http;
mod impersonation;
mod module;
mod orm;
mod prune;
mod sink;
mod skip;
mod time_series;
