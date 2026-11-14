//! The numbers beside the menu's resources: `GET {path}/counts`.
//!
//! A page never counts: a group of the menu with counted links is fetched again
//! once the page is up, with the numbers beside its links, so a list page pays for
//! nothing it does not show. Each count is read for the signed-in user and the
//! workspace they work in (it is what their list would show), cached for
//! `helm.metrics_cache_secs` under the user, their access, the workspace and the
//! credential's limit to one, and capped: past [`CAP`] rows it says `1,000+`, so a
//! table of millions costs what one of a thousand does. A resource whose count is not
//! cheap turns it off (`SIDEBAR_COUNT`).

use std::collections::HashMap;
use std::sync::Arc;

use http::header::CACHE_CONTROL;
use http::{HeaderMap, HeaderValue};
use navree_access::READ;
use navree_db::{Backend, Executor};
use navree_http::response::{Html, IntoResponse, Response};
use navree_http::{AppError, Json};
use navree_ui::pages::{FROM_HEADER, region_asked};
use serde_json::{Map, json};

use crate::cx::Cx;
use crate::export::refuse_other_sites;
use crate::handle::ErasedResource;
use crate::panel::Panel;
use crate::{money, ui};

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

/// The count of `resource` for `cx`, from the cache or counted now; `None` when it
/// can't be counted (a resource of workspaces outside one, a query that fails), and
/// the menu goes without.
async fn count_of(cx: &Cx, resource: &dyn ErasedResource) -> Option<u64> {
    let key = key(cx, &resource.info().slug);
    if let Some(rows) = cx.core.counts.as_ref().and_then(|cache| cache.get(&key)) {
        return Some(*rows);
    }
    let rows = counted(cx, resource).await.ok()?;
    if let Some(cache) = &cx.core.counts {
        cache.insert(key, Arc::new(rows));
    }
    Some(rows)
}

/// `GET {path}/counts`: for the menu's group that asks (the runtime names it in
/// `X-Navree-Region`), the group drawn with the counts beside its links; for anyone
/// else `{"counts": {"deals": {"value": 30, "text": "30"}}}` for each resource the
/// user may read whose count is on.
pub(crate) async fn counts(Panel { cx, session, .. }: Panel, headers: HeaderMap) -> Response {
    let result = async {
        refuse_other_sites(&headers)?;
        if let Some(region) = region_asked(&headers) {
            let region = region.as_str();
            let base = cx.core.options.base();
            // The page the group stands on, for the link it marks: one of the panel's.
            let at = headers
                .get(FROM_HEADER)
                .and_then(|value| value.to_str().ok())
                .and_then(|at| at.split(['?', '#']).next())
                .filter(|at| *at == base || at.starts_with(&format!("{base}/")))
                .unwrap_or(base)
                .to_owned();
            let mut texts = HashMap::new();
            for slug in ui::counted_in(&cx, region).await {
                let Some(resource) = cx.core.registry.by_slug(&slug) else {
                    continue;
                };
                if let Some(rows) = count_of(&cx, resource.as_ref()).await {
                    texts.insert(slug, text(rows));
                }
            }
            let group = ui::menu_fragment(&cx, session.csrf.as_deref(), region, &at, &texts)
                .await
                .ok_or_else(AppError::not_found)?;
            let mut response = Html(navree_ui::render(&group)).into_response();
            response
                .headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
            return Ok(response);
        }
        let mut counts = Map::new();
        for resource in cx.core.registry.resources() {
            let info = resource.info();
            if !info.sidebar_count || !cx.can(READ, info.subject, None) {
                continue;
            }
            let Some(rows) = count_of(&cx, resource.as_ref()).await else {
                continue;
            };
            counts.insert(
                info.slug.clone(),
                json!({ "value": rows.min(CAP), "capped": rows > CAP, "text": text(rows) }),
            );
        }
        Ok::<_, AppError>(Json(json!({ "counts": counts })).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
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
