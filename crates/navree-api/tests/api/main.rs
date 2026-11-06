//! navree-api integration tests: the OpenAPI document an app builds from its
//! route table, the routes that serve it and the `openapi:export` command.

mod support;

mod config;
mod conformity;
mod document;
mod environment;
mod export;
mod route_docs;
mod routes;
mod schemas;
