//! navree-integrations integration tests: the service, the routes, the
//! extractor and the commands booted on a temporary SQLite database, with
//! the fake providers and with the real providers against wiremock servers
//! answering recorded fixtures; PostgreSQL and MySQL when their test
//! servers are configured.

mod api_key;
mod audit;
mod clients;
mod connect;
mod health;
mod notify;
mod real;
mod routes;
mod scopes;
#[cfg(any(feature = "postgres", feature = "mysql"))]
mod servers;
mod service_account;
mod support;
mod tokens;
