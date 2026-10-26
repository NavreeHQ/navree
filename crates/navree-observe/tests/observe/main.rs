//! navree-observe integration tests: the health and metrics routes served by
//! the full navree-http router, driven in-process with `tower::ServiceExt::oneshot`.

mod support;

mod health;
mod logs;
mod metrics;
