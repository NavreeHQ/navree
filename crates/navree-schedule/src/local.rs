//! [`LocalRun`]: what a scheduled run is for, and on which clock.

use chrono::{DateTime, NaiveDateTime, Utc};
use navree_contracts::ZoneLevel;
use navree_time::Zone;
use uuid::Uuid;

tokio::task_local! {
    static LOCAL_RUN: LocalRun;
}

/// The run of a scheduled task, as its body sees it through
/// [`LocalRun::current`]: the owner it runs for (with
/// [`per_workspace_local`](crate::TaskBuilder::per_workspace_local)), the
/// zone its frequency was read on, and the tick on both clocks.
///
/// ```ignore
/// s.task("digest", |ctx| async move {
///     let run = LocalRun::current().expect("inside a scheduled run");
///     send_digest(&ctx, run.owner.unwrap(), run.zone).await
/// })
/// .daily_at("09:00")
/// .per_workspace_local();
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalRun {
    /// The workspace (or user, organization) the run is for; `None` for
    /// tasks that do not run per owner.
    pub owner: Option<Uuid>,
    /// The owner's level; `None` for tasks that do not run per owner.
    pub level: Option<ZoneLevel>,
    /// The zone the frequency was read on (`UTC` for plain tasks).
    pub zone: Zone,
    /// The tick, UTC.
    pub tick: DateTime<Utc>,
    /// The tick on the zone's clock.
    pub local: NaiveDateTime,
}

impl LocalRun {
    /// The run of the scheduled task being executed, or `None` outside one
    /// (it follows the task, not spawned work).
    pub fn current() -> Option<LocalRun> {
        LOCAL_RUN.try_with(Clone::clone).ok()
    }

    /// Runs `future` as this run.
    pub(crate) async fn scope<F: Future>(self, future: F) -> F::Output {
        LOCAL_RUN.scope(self, future).await
    }
}
