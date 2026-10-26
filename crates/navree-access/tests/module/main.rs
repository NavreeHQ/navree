//! Integration tests of the AccessModule half of navree-access: tables,
//! services, the route authorizer, extractors, endpoints and commands.

mod support;

mod routes;

mod authorized;

mod commands;
mod endpoints;
mod parity;
mod service;
mod wiring;
