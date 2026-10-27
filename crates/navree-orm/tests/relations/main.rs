//! navree-orm relationships on SQLite (a fresh temporary database per
//! test, logging the statements it runs), and the SQL relations generate
//! per dialect.

mod fixtures;
mod models;
mod support;

mod definitions;
mod eager;
