//! navree-schedule integration tests. Every test uses a fresh temporary SQLite
//! file and a `FixedClock`.

mod support;

mod cli;
mod commands;
mod failures;
mod jobs;
mod module;
mod overlap;
mod prune;
mod single_node;
mod telemetry;
mod tick;
mod zones;
