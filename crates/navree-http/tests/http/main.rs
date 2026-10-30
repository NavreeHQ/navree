//! navree-http integration tests: the full router built by `build_router`,
//! driven in-process with `tower::ServiceExt::oneshot`.

mod support;

mod access;
mod errors;
mod logs;
mod matching;
mod pipeline;
mod routes;
mod server;
mod telemetry;
