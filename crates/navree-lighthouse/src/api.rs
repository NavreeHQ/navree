//! The JSON API under `{lighthouse.path}/api`: the dashboard's data, for
//! the dashboard's own script, scripts with an access token and agents.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::Method;
use navree_http::{ApiMeta, AppError, Controller, Json, Path, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use uuid::Uuid;

use navree_access::Subject as _;

use crate::model::Issue;
use crate::module::{LighthouseSubject, RESOLVE, VIEW};
use crate::options::{Period, StoreKind};
use crate::service::Lighthouse;
use crate::store::{
    AlertRecord, HostStats, IgnoreFor, IssueDetail, IssueFilter, IssueSummary, JobStats,
    LighthouseStore, ListPage, Occurrence, Overview, QueryStats, RouteDetail, RouteStats, Sample,
};

/// The OpenAPI tag of the routes.
const TAG: &str = "Lighthouse";

/// `?period=1h|24h|7d` (24 hours by default).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PeriodQuery {
    /// `1h`, `24h` or `7d`.
    pub period: Option<String>,
}

impl PeriodQuery {
    /// The period; a 400 for an unknown one.
    pub fn period(&self) -> Result<Period, AppError> {
        match self
            .period
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            None => Ok(Period::default()),
            Some(text) => Period::parse(text).ok_or_else(|| {
                AppError::bad_request(format!("unknown period {text:?} (expected 1h, 24h or 7d)"))
            }),
        }
    }
}

/// The issue filters of the dashboard's Issues page: the API's
/// [`IssueFilter`], showing open issues when no status is given.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(transparent)]
pub(crate) struct IssueFilterQuery(pub(crate) IssueFilter);

impl IssueFilterQuery {
    /// The filter to apply.
    pub(crate) fn filter(&self) -> IssueFilter {
        let mut filter = self.0.clone();
        if filter.status.is_none() {
            filter.status = Some("open".to_owned());
        }
        filter
    }

    /// The filters as a query string without the page (for page links).
    pub(crate) fn encoded_without_page(&self) -> String {
        let filter = self.filter();
        let mut query = form_urlencoded::Serializer::new(String::new());
        let pairs = [
            ("status", filter.status.as_deref()),
            ("kind", filter.kind.as_deref()),
            ("level", filter.level.as_deref()),
            ("release", filter.release.as_deref()),
            ("environment", filter.environment.as_deref()),
            ("route", filter.route.as_deref()),
            ("job", filter.job.as_deref()),
            ("tag", filter.tag.as_deref()),
            ("user", filter.user.as_deref()),
            ("workspace", filter.workspace.as_deref()),
            ("search", filter.search.as_deref()),
        ];
        for (key, value) in pairs {
            if let Some(value) = value {
                query.append_pair(key, value);
            }
        }
        let sort = match filter.sort {
            crate::store::IssueSort::LastSeen => "last_seen",
            crate::store::IssueSort::FirstSeen => "first_seen",
            crate::store::IssueSort::Count => "count",
        };
        query.append_pair("sort", sort);
        query.finish()
    }
}

/// `?endpoint=GET /api/v1/deals/{id}&period=..`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RouteQuery {
    /// The method and route template.
    pub endpoint: String,
    /// `1h`, `24h` or `7d`.
    pub period: Option<String>,
}

/// `?page=..&per_page=..`.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct PageQuery {
    /// The page, from 1.
    pub page: Option<u64>,
    /// Items per page (at most 100; 25 by default).
    pub per_page: Option<u64>,
}

/// The body of `POST {path}/api/issues/{id}/resolve`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResolveInput {
    /// Resolve in the next release: only an occurrence from another release
    /// reopens it.
    pub in_next_release: bool,
    /// A comment kept on the issue.
    pub comment: Option<String>,
}

/// The body of `POST {path}/api/issues/{id}/ignore`: forever, `until` a
/// time, or for `occurrences` more occurrences.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IgnoreInput {
    /// Ignored until then.
    pub until: Option<DateTime<Utc>>,
    /// Ignored for this many more occurrences.
    pub occurrences: Option<i64>,
    /// A comment kept on the issue.
    pub comment: Option<String>,
}

impl IgnoreInput {
    /// How long.
    pub fn ignore_for(&self) -> Result<IgnoreFor, AppError> {
        match (self.until, self.occurrences) {
            (Some(_), Some(_)) => Err(AppError::bad_request(
                "give at most one of until and occurrences",
            )),
            (Some(until), None) => Ok(IgnoreFor::Until(until)),
            (None, Some(n)) if n > 0 => Ok(IgnoreFor::Occurrences(n)),
            (None, Some(_)) => Err(AppError::bad_request("occurrences must be at least 1")),
            (None, None) => Ok(IgnoreFor::Forever),
        }
    }
}

/// The JSON API, mounted when `lighthouse.dashboard` is on (each route
/// checks `view` or `resolve` on the subject `Lighthouse`):
///
/// | Route | Answer |
/// |---|---|
/// | `GET {path}/api/overview?period=` | [`Overview`] |
/// | `GET {path}/api/issues?status=&kind=&level=&release=&environment=&route=&job=&tag=&user=&workspace=&search=&sort=&page=&per_page=` | a page of [`IssueSummary`] |
/// | `GET {path}/api/issues/{id}` | [`IssueDetail`] |
/// | `GET {path}/api/issues/{id}/occurrences?page=&per_page=` | a page of [`Occurrence`] |
/// | `POST {path}/api/issues/{id}/resolve` `{in_next_release?, comment?}` | the [`Issue`] (`resolve`) |
/// | `POST {path}/api/issues/{id}/ignore` `{until? \| occurrences?, comment?}` | the [`Issue`] (`resolve`) |
/// | `POST {path}/api/issues/{id}/reopen` | the [`Issue`] (`resolve`) |
/// | `GET {path}/api/routes?period=` | every [`RouteStats`], slowest first |
/// | `GET {path}/api/routes/detail?endpoint=&period=` | [`RouteDetail`] |
/// | `GET {path}/api/queries?period=` | [`QueryStats`], by total time |
/// | `GET {path}/api/jobs?period=` | [`JobStats`] |
/// | `GET {path}/api/outgoing?period=` | [`HostStats`] |
/// | `GET {path}/api/traces/{id}` | a [`Sample`] with its spans |
/// | `GET {path}/api/alerts` | the latest [`AlertRecord`]s |
pub struct LighthouseApiController {
    lighthouse: Arc<Lighthouse>,
}

impl Injectable for LighthouseApiController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Lighthouse>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(LighthouseApiController {
                lighthouse: r.get::<Lighthouse>().await?,
            })
        })
    }
}

fn api(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec![TAG.to_owned()],
        ..ApiMeta::default()
    }
}

/// The store, or a 503 when Lighthouse keeps nothing.
fn store_of(lighthouse: &Lighthouse) -> Result<&LighthouseStore, AppError> {
    lighthouse
        .store()
        .ok_or_else(|| AppError::service_unavailable("lighthouse.store is off"))
}

macro_rules! route {
    ($this:expr, $method:expr, $path:expr, $ability:expr, $name:expr, $summary:expr, |$l:ident $(, $arg:ident : $ty:ty)*| $body:expr) => {{
        let this = $this.clone();
        RouteDef::on($method, $path, move |$($arg: $ty),*| {
            let this = this.clone();
            async move {
                let $l: &Lighthouse = &this.lighthouse;
                $body
            }
        })
        .can($ability, LighthouseSubject::SUBJECT, None)
        .name($name)
        .api(api($summary))
    }};
}

impl Controller for LighthouseApiController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let options = self.lighthouse.options();
        if !options.dashboard
            || options.store == StoreKind::Off
            || self.lighthouse.store().is_none()
        {
            return Vec::new();
        }
        let base = format!("!{}/api", options.mount_path());
        let path = |rest: &str| format!("{base}{rest}");
        vec![
            route!(
                self,
                Method::GET,
                path("/overview"),
                VIEW,
                "lighthouse.api.overview",
                "The overview's figures, timeline and live feed",
                |l, q: Query<PeriodQuery>| {
                    let mut overview: Overview = store_of(l)?.overview(q.period()?).await?;
                    overview.dropped = l.dropped();
                    Ok::<_, AppError>(Json(overview))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/issues"),
                VIEW,
                "lighthouse.api.issues",
                "List issues",
                |l, q: Query<IssueFilter>| {
                    let page: ListPage<IssueSummary> = store_of(l)?.issues(&q.0).await?;
                    Ok::<_, AppError>(Json(page))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/issues/{id}"),
                VIEW,
                "lighthouse.api.issues.show",
                "One issue with its latest occurrence, timeline and alerts",
                |l, id: Path<Uuid>| {
                    let detail: IssueDetail = store_of(l)?.issue_detail(id.0).await?;
                    Ok::<_, AppError>(Json(detail))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/issues/{id}/occurrences"),
                VIEW,
                "lighthouse.api.issues.occurrences",
                "An issue's stored occurrences, newest first",
                |l, id: Path<Uuid>, q: Query<PageQuery>| {
                    let page: ListPage<Occurrence> = store_of(l)?
                        .occurrences(id.0, q.page.unwrap_or(1), q.per_page.unwrap_or(25))
                        .await?;
                    Ok::<_, AppError>(Json(page))
                }
            ),
            route!(
                self,
                Method::POST,
                path("/issues/{id}/resolve"),
                RESOLVE,
                "lighthouse.api.issues.resolve",
                "Resolve an issue, now or in the next release",
                |l, id: Path<Uuid>, input: Json<ResolveInput>| {
                    let input = input.0;
                    let issue: Issue = store_of(l)?
                        .resolve(id.0, input.in_next_release, clean(input.comment))
                        .await?;
                    Ok::<_, AppError>(Json(issue))
                }
            ),
            route!(
                self,
                Method::POST,
                path("/issues/{id}/ignore"),
                RESOLVE,
                "lighthouse.api.issues.ignore",
                "Ignore an issue: forever, until a time or for a number of occurrences",
                |l, id: Path<Uuid>, input: Json<IgnoreInput>| {
                    let ignore = input.ignore_for()?;
                    let issue: Issue = store_of(l)?
                        .ignore(id.0, ignore, clean(input.0.comment))
                        .await?;
                    Ok::<_, AppError>(Json(issue))
                }
            ),
            route!(
                self,
                Method::POST,
                path("/issues/{id}/reopen"),
                RESOLVE,
                "lighthouse.api.issues.reopen",
                "Reopen an issue",
                |l, id: Path<Uuid>| {
                    let issue: Issue = store_of(l)?.reopen(id.0).await?;
                    Ok::<_, AppError>(Json(issue))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/routes"),
                VIEW,
                "lighthouse.api.routes",
                "Every route with throughput, error rate and latency percentiles",
                |l, q: Query<PeriodQuery>| {
                    let routes: Vec<RouteStats> = store_of(l)?.routes(q.period()?).await?;
                    Ok::<_, AppError>(Json(routes))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/routes/detail"),
                VIEW,
                "lighthouse.api.routes.show",
                "One route: its figures, timeline and slowest traces",
                |l, q: Query<RouteQuery>| {
                    let period = PeriodQuery {
                        period: q.period.clone(),
                    }
                    .period()?;
                    let route: RouteDetail = store_of(l)?.route(&q.endpoint, period).await?;
                    Ok::<_, AppError>(Json(route))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/queries"),
                VIEW,
                "lighthouse.api.queries",
                "Normalized statements by total time",
                |l, q: Query<PeriodQuery>| {
                    let queries: Vec<QueryStats> = store_of(l)?.queries(q.period()?).await?;
                    Ok::<_, AppError>(Json(queries))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/jobs"),
                VIEW,
                "lighthouse.api.jobs",
                "Job types with throughput, runtime and failures",
                |l, q: Query<PeriodQuery>| {
                    let jobs: Vec<JobStats> = store_of(l)?.jobs(q.period()?).await?;
                    Ok::<_, AppError>(Json(jobs))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/outgoing"),
                VIEW,
                "lighthouse.api.outgoing",
                "Hosts called through the HTTP client",
                |l, q: Query<PeriodQuery>| {
                    let hosts: Vec<HostStats> = store_of(l)?.outgoing(q.period()?).await?;
                    Ok::<_, AppError>(Json(hosts))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/traces/{id}"),
                VIEW,
                "lighthouse.api.traces.show",
                "A stored trace with its spans",
                |l, id: Path<Uuid>| {
                    let sample: Sample = store_of(l)?.sample(id.0).await?;
                    Ok::<_, AppError>(Json(sample))
                }
            ),
            route!(
                self,
                Method::GET,
                path("/alerts"),
                VIEW,
                "lighthouse.api.alerts",
                "The latest alerts and whether they were delivered",
                |l| {
                    let alerts: Vec<AlertRecord> = store_of(l)?
                        .alerts_for(None)
                        .await
                        .map_err(AppError::internal)?;
                    Ok::<_, AppError>(Json(alerts))
                }
            ),
        ]
    }
}

/// A comment, trimmed; `None` when blank.
fn clean(comment: Option<String>) -> Option<String> {
    comment
        .map(|comment| comment.trim().to_owned())
        .filter(|comment| !comment.is_empty())
}
