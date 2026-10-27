//! Integration tests of the AccessModule half of navree-access: tables,
//! services, the route authorizer, extractors, endpoints and commands.

mod support;

mod routes;

mod authorized;

mod commands;
mod endpoints;
mod field_checks;
mod outside;
mod parity;
mod permission_check;
mod service;
mod wiring;
