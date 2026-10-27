//! navree-orm relationships on SQLite (a fresh temporary database per
//! test, logging the statements it runs), and the SQL relations generate
//! per dialect.

mod fixtures;
mod models;
mod support;

mod aggregates;
mod definitions;
mod eager;
mod events;
mod existence;
mod lazy;
mod morphs;
mod one_of_many;
mod pivots;
mod querying;
mod scopes;
mod sql;
mod through;
mod touches;
mod writes;
