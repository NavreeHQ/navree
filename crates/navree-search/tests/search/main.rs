//! navree-search integration tests: one binary.

mod access_parity;
mod ai;
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
mod servers;
mod support;
mod surface;
mod sync;
mod telemetry;
mod vector_stores;
mod vectors;
