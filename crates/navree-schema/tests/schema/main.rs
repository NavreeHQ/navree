//! navree-schema integration tests: SQL per dialect (snapshots), SQLite end
//! to end, and real servers when `NAVREE_TEST_POSTGRES_URL` /
//! `NAVREE_TEST_MYSQL_URL` are set.

mod support;

mod alter;
mod columns;
mod create;
mod errors;
mod sqlite;
mod timescale;
