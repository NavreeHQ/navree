//! navree-mongo integration tests. The command tests (`commands`) and the
//! module wiring (`module`) run without a server; the others run against
//! `NAVREE_TEST_MONGODB_URL` and pass with a notice when it is not set (see
//! `support`).

mod models;
mod support;

mod bulk;
mod commands;
mod crud;
mod migrations;
mod module;
mod queries;
mod schema;
mod uow;
