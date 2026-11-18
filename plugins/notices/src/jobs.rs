//! Background job and CLI command that remove expired notices.

use chrono::{TimeDelta, Utc};
use navree::contracts::{Clock, Job, JobContext};
use navree::db::Db;
use navree::kernel::{AppContext, BoxError, CommandSpec};
use navree::orm::Model as _;
use serde::{Deserialize, Serialize};

use crate::model::notice;
use crate::options;

/// Queued and scheduled job that removes notices past `keep_days` or `expires_at`.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ExpireNotices;

impl Job for ExpireNotices {
    const NAME: &'static str = "notices.expire";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        expire_notices(&ctx.app).await?;
        Ok(())
    }
}

/// Deletes notices whose `expires_at` has passed or whose `created_at` is older
/// than `notices.keep_days`, returning how many rows were removed.
pub async fn expire_notices(app: &AppContext) -> Result<u64, BoxError> {
    let settings = options::load(app).await?;
    let now = match app.try_get::<dyn Clock>().await {
        Some(clock) => clock.now(),
        None => Utc::now(),
    };
    let cutoff = now - TimeDelta::days(i64::from(settings.keep_days));
    let db = app.get::<Db>().await?;
    let uow = db.uow(None, None).await;
    let removed = notice::Model::query()
        .where_group(|g| {
            g.where_(notice::Model::EXPIRES_AT.lte(now))
                .or_where(notice::Model::CREATED_AT.lt(cutoff))
        })
        .delete(&uow)
        .await?;
    uow.commit().await?;
    Ok(removed)
}

/// Builds the `notices:expire` CLI command, which deletes expired rows and
/// prints `Removed N notices.`
pub fn expire_command() -> CommandSpec {
    CommandSpec::new(
        "notices:expire",
        "Remove notices older than notices.keep_days",
        |ctx, _args| async move {
            let removed = expire_notices(&ctx).await?;
            println!("Removed {removed} notices.");
            Ok(())
        },
    )
}
