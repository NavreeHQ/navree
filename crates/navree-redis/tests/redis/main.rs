//! navree-redis integration tests. `module` runs without a server; the
//! others run against `NAVREE_TEST_REDIS_URL` and pass with a notice when
//! it is not set (see `support`).

mod backplane;
mod cache;
mod limiter;
mod module;
mod queue;
mod session;
mod support;
