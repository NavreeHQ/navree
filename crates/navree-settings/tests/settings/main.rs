//! navree-settings integration tests: settings as types, the inheritance
//! matrix and the source of every value, writes in units of work (validated,
//! audited, announced, cached by tag), storage, the routes, the request
//! extractor, the commands, the module and the fake.

mod support;

mod commands;
mod derive;
mod fake;
mod http;
mod module;
mod resolution;
mod servers;
mod store;
mod writes;
