//! navree-schedule integration tests. Every test uses a fresh database (a
//! temporary SQLite file; the tests of the run table also run on PostgreSQL
//! and MySQL, see `databases.rs`) and a `FixedClock`.

mod databases;
mod support;

mod cli;
mod commands;
mod cron_language;
mod failures;
mod jobs;
mod module;
mod overlap;
mod prune;
mod single_node;
mod telemetry;
mod tick;
mod zones;
