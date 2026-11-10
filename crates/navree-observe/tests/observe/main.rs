//! navree-observe integration tests: the health and metrics routes served by
//! the full navree-http router, driven in-process with `tower::ServiceExt::oneshot`.

mod support;

#[cfg(feature = "otel")]
mod collector;
mod health;
mod metrics;
mod options;
#[cfg(feature = "otel")]
mod otlp;
mod spans;
