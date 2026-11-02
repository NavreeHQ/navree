//! The numbers beside the menu's resources: `GET {path}/counts`.
//!
//! A page never counts: its menu carries empty slots and the script asks for
//! the numbers once the page is up, so a list page pays for nothing it does
//! not show. Each count is read for the signed-in user and the workspace they
//! work in (it is what their list would show), cached for `helm.metrics_cache_secs`
//! under the user, their access, the workspace and the credential's limit to
//! one, and capped: past
//! [`CAP`] rows it says `1,000+`, so a table of millions costs what one of a
//! thousand does. A resource whose count is not cheap turns it off
//! (`SIDEBAR_COUNT`).

use std::sync::Arc;

use navree_access::READ;
use navree_db::{Backend, Executor};
use navree_http::{AppError, Json};
use serde_json::{Map, Value, json};

use crate::cx::Cx;
use crate::handle::ErasedResource;
use crate::money;
use crate::panel::Panel;

/// The most a count reads: more rows than this show as `1,000+`.
pub(crate) const CAP: u64 = 1000;

/// A count as the menu shows it.
pub(crate) fn text(rows: u64) -> String {
    if rows > CAP {
        format!("{}+", money::format_number(CAP as f64))
    } else {
        money::format_number(rows as f64)
    }
}

/// The cache key of a count: whose it is, what they may do, where, to what their
/// credential holds them (a resource across workspaces keeps to it), and of what.
fn key(cx: &Cx, slug: &str) -> String {
    format!(
        "count|{}|{}|{}|{}|{slug}",
        cx.user_id,
        cx.access.version,
        cx.uow.workspace().unwrap_or_default(),
        cx.pin.unwrap_or_default()
    )
}

/// How many rows of `resource` the user may read. On PostgreSQL a failed statement
/// ends the transaction it is in, so each count has a savepoint of its own: one
/// that fails doesn't take the others with it.
async fn counted(cx: &Cx, resource: &dyn ErasedResource) -> Result<u64, AppError> {
    if Executor::backend(&cx.uow) != Backend::Postgres {
        return resource.count(cx, CAP).await;
    }
    let savepoint = cx.uow.savepoint().await.map_err(AppError::internal)?;
    let rows = resource.count(cx, CAP).await;
    match &rows {
        Ok(_) => savepoint.commit().await.map_err(AppError::internal)?,
        Err(_) => savepoint.rollback().await.map_err(AppError::internal)?,
    }
    rows
}

/// `GET {path}/counts`: `{"counts": {"deals": {"value": 30, "text": "30"}}}`
/// for each resource the user may read whose count is on.
pub(crate) async fn counts(Panel { cx, .. }: Panel) -> Result<Json<Value>, AppError> {
    let mut counts = Map::new();
    for resource in cx.core.registry.resources() {
        let info = resource.info();
        if !info.sidebar_count || !cx.can(READ, info.subject, None) {
            continue;
        }
        let key = key(&cx, &info.slug);
        let cached = cx.core.counts.as_ref().and_then(|cache| cache.get(&key));
        let rows = match cached {
            Some(rows) => *rows,
            // A resource of workspaces has no count outside one; one that fails
            // has none to show, and the menu goes without.
            None => match counted(&cx, resource.as_ref()).await {
                Ok(rows) => {
                    if let Some(cache) = &cx.core.counts {
                        cache.insert(key, Arc::new(rows));
                    }
                    rows
                }
                Err(_) => continue,
            },
        };
        counts.insert(
            info.slug.clone(),
            json!({ "value": rows.min(CAP), "capped": rows > CAP, "text": text(rows) }),
        );
    }
    Ok(Json(json!({ "counts": counts })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_stop_at_the_cap() {
        assert_eq!(text(0), "0");
        assert_eq!(text(212), "212");
        assert_eq!(text(1000), "1,000");
        assert_eq!(text(1001), "1,000+");
        assert_eq!(text(230_000), "1,000+");
    }
}
