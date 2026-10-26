//! navree-social integration tests: the routes and the `Social` service
//! booted with navree-auth on a temporary SQLite database, with the fake
//! provider and with each built-in provider against a wiremock server.

mod flow;
mod linking;
mod providers;
mod support;
mod tokens;
