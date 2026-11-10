//! navree-integrations integration tests: the service, the routes, the
//! extractor and the commands booted on a temporary SQLite database by default, with
//! the fake providers and with the real providers against wiremock servers
//! answering recorded fixtures; on PostgreSQL or MySQL when
//! `NAVREE_TEST_DATABASE` names them (see `servers`).

mod api_key;
mod audit;
mod clients;
mod connect;
mod health;
mod lifecycle;
mod mailboxes;
mod notify;
mod pubsub;
mod real;
mod routes;
mod scopes;
mod servers;
mod service_account;
mod support;
mod tokens;
