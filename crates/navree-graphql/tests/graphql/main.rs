//! navree-graphql integration tests: resolvers registered by modules,
//! executed through `GraphqlSchema` and the HTTP endpoint of a `TestApp`,
//! subscriptions over a real WebSocket, and the `graphql:schema` command.

mod support;

mod access;
mod command;
mod config;
mod endpoint;
mod errors;
mod identity;
mod loaders;
mod schema;
mod subscriptions;
mod units_of_work;
mod validation;
