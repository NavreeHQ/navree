//! Scheduled tasks for the demo, run by `schedule:work`:
//!
//! - `demo.heartbeat` every minute: checks the database and finishes at once.
//! - `demo.rollup` every two minutes: counts the tasks of every workspace,
//!   and fails about one run in three because it cannot get its lock.
//! - `demo.prune-exports` every five minutes: takes about a second and a
//!   half.

use std::time::Duration;

use navree::db::Db;
use navree::prelude::*;
use navree::schedule::Schedule;

use super::random::one_in;
use crate::modules::tasks::Task;

/// Declares the tasks.
pub fn schedule(s: &mut Schedule) {
    s.task("demo.heartbeat", |ctx: AppContext| async move {
        let db = ctx.get::<Db>().await?;
        db.ping().await?;
        tracing::info!(target: "evidence::schedule", "heartbeat: database answered");
        Ok(())
    })
    .every_minute();

    s.task("demo.rollup", |ctx: AppContext| async move {
        if one_in(3) {
            return Err("rollup lock timed out after 30 s".into());
        }
        let db = ctx.get::<Db>().await?;
        let tasks = Task::query().without_workspace_scope().count(&*db).await?;
        tracing::info!(target: "evidence::schedule", tasks, "rollup: counted the tasks");
        Ok(())
    })
    .cron("*/2 * * * *")
    .without_overlapping();

    s.task("demo.prune-exports", |_ctx: AppContext| async move {
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        tracing::info!(target: "evidence::schedule", "prune-exports: nothing older than a week");
        Ok(())
    })
    .cron("*/5 * * * *")
    .without_overlapping();
}
