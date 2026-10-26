//! navree-search integration tests: one binary.

mod commands;
mod config;
mod conformance;
mod derive;
mod http;
mod http_engines;
mod migrations;
#[cfg(feature = "postgres")]
mod postgres;
mod queries;
mod support;
mod surface;
mod sync;
mod vector_stores;
mod vectors;
