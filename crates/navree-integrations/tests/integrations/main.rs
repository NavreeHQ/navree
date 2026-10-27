//! navree-integrations integration tests: the service, the routes, the
//! extractor and the commands booted on a temporary SQLite database, with
//! the fake providers and with the real providers against wiremock servers
//! answering recorded fixtures.

mod api_key;
mod clients;
mod connect;
mod health;
mod real;
mod routes;
mod scopes;
mod service_account;
mod support;
mod tokens;
