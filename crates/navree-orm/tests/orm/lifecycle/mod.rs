//! The lifecycle: model events and observers, quiet operations, soft
//! deletes, runtime global scopes, strict modes, pruning, broadcasting and
//! the ORM module.

mod models;

mod app_events;
mod dispatch;
mod observers;
mod prune;
mod quiet;
mod scopes;
mod soft_deletes;
mod strict;
