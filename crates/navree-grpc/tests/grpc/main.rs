//! navree-grpc integration tests: services registered in modules and
//! called through the whole pipeline, in-process with `TestChannel` and
//! the app's router, and over real sockets.

#[path = "../support/mod.rs"]
mod support;

mod channels;
mod metrics;
mod pipeline;
mod protocols;
mod serving;
mod streaming;
mod workspace;
