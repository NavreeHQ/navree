//! The dashboard's JSON API under `{path}/api`, behind the `Fleet` abilities
//! `view` and `operate`. The pages are drawn with Navree UI (`crate::ui`),
//! served next to it.

use std::sync::Arc;

use http::{Method, StatusCode};
use navree_access::Subject;
use navree_http::{AppError, Controller, Json, Path, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_queue::{FailedJob, PendingJob};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::record::{CompletedJob, MasterReport, MetricSeries, MetricsPeriod};
use crate::{Fleet, FleetError, Overview};

/// The access subject of the dashboard: `Fleet`, permission prefix `fleet`
/// (`fleet.view`, `fleet.operate`).
#[derive(Clone, Copy, Debug, Default)]
pub struct FleetSubject;

impl Subject for FleetSubject {
    const SUBJECT: &'static str = "Fleet";
    const PERMISSION_PREFIX: &'static str = "fleet";
}

/// The ability that opens the dashboard and reads its API.
pub const VIEW: &str = "view";
/// The ability that retries and forgets failed jobs and pauses and
/// continues supervisors.
pub const OPERATE: &str = "operate";

/// The dashboard's pages and API.
pub(crate) struct FleetController {
    fleet: Arc<Fleet>,
}

impl Injectable for FleetController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Fleet>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let fleet = r.get::<Fleet>().await?;
            Ok(Self { fleet })
        })
    }
}

/// `?search=`.
#[derive(Debug, Default, Deserialize)]
struct Search {
    search: Option<String>,
}

/// `?period=hour|day|week`.
#[derive(Debug, Default, Deserialize)]
struct Period {
    #[serde(default)]
    period: MetricsPeriod,
}

/// The body of pause and continue: one supervisor, or all.
#[derive(Debug, Default, Deserialize)]
struct SupervisorBody {
    supervisor: Option<String>,
}

/// A failed job as the API shows it (payload redacted).
#[derive(Debug, Serialize)]
struct FailedJobView {
    #[serde(flatten)]
    job: FailedJob,
}

/// `GET {path}/api/jobs`.
#[derive(Debug, Serialize)]
struct JobsView {
    pending: Vec<PendingJob>,
    completed: Vec<CompletedJob>,
}

/// `GET {path}/api/masters`.
#[derive(Debug, Serialize)]
struct MastersView {
    data: Vec<MasterReport>,
}

/// `GET {path}/api/failed`.
#[derive(Debug, Serialize)]
struct FailedView {
    data: Vec<FailedJobView>,
}

/// `GET {path}/api/metrics`.
#[derive(Debug, Serialize)]
struct MetricsView {
    period: MetricsPeriod,
    queues: Vec<MetricSeries>,
    jobs: Vec<MetricSeries>,
}

/// A service error as the response it is: a missing supervisor or job is a 404.
pub(crate) fn api_error(error: FleetError) -> AppError {
    match error {
        FleetError::UnknownSupervisor(_) | FleetError::FailedJobNotFound { .. } => {
            AppError::not_found().with_detail(error.to_string())
        }
        other => AppError::internal(other),
    }
}

impl Controller for FleetController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.fleet.options().dashboard {
            return Vec::new();
        }
        let base = self.fleet.options().base_path().to_owned();
        let api = format!("!{base}/api");
        let fleet = &self.fleet;
        let mut routes = Vec::new();

        let f = fleet.clone();
        routes.push(
            view(RouteDef::on(
                Method::GET,
                format!("{api}/overview"),
                move || {
                    let fleet = f.clone();
                    async move {
                        fleet
                            .overview()
                            .await
                            .map(Json::<Overview>)
                            .map_err(api_error)
                    }
                },
            ))
            .name("fleet.overview"),
        );

        let f = fleet.clone();
        routes.push(
            view(RouteDef::on(
                Method::GET,
                format!("{api}/masters"),
                move || {
                    let fleet = f.clone();
                    async move {
                        let data = fleet.masters().await.map_err(api_error)?;
                        Ok::<_, AppError>(Json(MastersView { data }))
                    }
                },
            ))
            .name("fleet.masters"),
        );

        let f = fleet.clone();
        routes.push(
            view(RouteDef::on(
                Method::GET,
                format!("{api}/jobs"),
                move |Query(q): Query<Search>| {
                    let fleet = f.clone();
                    async move {
                        let search = q.search.as_deref();
                        let pending = fleet
                            .pending_jobs(search)
                            .await
                            .map_err(api_error)?
                            .into_iter()
                            .map(|mut job| {
                                job.payload = fleet.redact(&job.payload);
                                job
                            })
                            .collect();
                        let completed = fleet.completed_jobs(search).await.map_err(api_error)?;
                        Ok::<_, AppError>(Json(JobsView { pending, completed }))
                    }
                },
            ))
            .name("fleet.jobs"),
        );

        let f = fleet.clone();
        routes.push(
            view(RouteDef::on(
                Method::GET,
                format!("{api}/jobs/{{id}}"),
                move |Path(id): Path<String>| {
                    let fleet = f.clone();
                    async move {
                        let mut job = fleet
                            .pending_job(&id)
                            .await
                            .map_err(api_error)?
                            .ok_or_else(AppError::not_found)?;
                        job.payload = fleet.redact(&job.payload);
                        Ok::<_, AppError>(Json(job))
                    }
                },
            ))
            .name("fleet.jobs.show"),
        );

        let f = fleet.clone();
        routes.push(
            view(RouteDef::on(
                Method::GET,
                format!("{api}/failed"),
                move |Query(q): Query<Search>| {
                    let fleet = f.clone();
                    async move {
                        let data = fleet
                            .failed_jobs(q.search.as_deref())
                            .await
                            .map_err(api_error)?
                            .into_iter()
                            .map(|job| redacted(&fleet, job))
                            .collect();
                        Ok::<_, AppError>(Json(FailedView { data }))
                    }
                },
            ))
            .name("fleet.failed"),
        );

        let f = fleet.clone();
        routes.push(
            view(RouteDef::on(
                Method::GET,
                format!("{api}/failed/{{id}}"),
                move |Path(id): Path<String>| {
                    let fleet = f.clone();
                    async move {
                        let job = fleet
                            .failed_job(&id)
                            .await
                            .map_err(api_error)?
                            .ok_or_else(AppError::not_found)?;
                        Ok::<_, AppError>(Json(redacted(&fleet, job)))
                    }
                },
            ))
            .name("fleet.failed.show"),
        );

        let f = fleet.clone();
        routes.push(
            operate(RouteDef::on(
                Method::POST,
                format!("{api}/failed/{{id}}/retry"),
                move |Path(id): Path<String>| {
                    let fleet = f.clone();
                    async move {
                        fleet.retry(&id).await.map_err(api_error)?;
                        Ok::<_, AppError>(Json(json!({ "retried": 1 })))
                    }
                },
            ))
            .name("fleet.failed.retry"),
        );

        let f = fleet.clone();
        routes.push(
            operate(RouteDef::on(
                Method::POST,
                format!("{api}/failed/retry"),
                move |Query(q): Query<Search>| {
                    let fleet = f.clone();
                    async move {
                        let retried = fleet
                            .retry_all(q.search.as_deref())
                            .await
                            .map_err(api_error)?;
                        Ok::<_, AppError>(Json(json!({ "retried": retried })))
                    }
                },
            ))
            .name("fleet.failed.retry_all"),
        );

        let f = fleet.clone();
        routes.push(
            operate(RouteDef::on(
                Method::DELETE,
                format!("{api}/failed/{{id}}"),
                move |Path(id): Path<String>| {
                    let fleet = f.clone();
                    async move {
                        fleet.forget(&id).await.map_err(api_error)?;
                        Ok::<_, AppError>(StatusCode::NO_CONTENT)
                    }
                },
            ))
            .name("fleet.failed.forget"),
        );

        for (action, pause) in [("pause", true), ("continue", false)] {
            let f = fleet.clone();
            routes.push(
                operate(RouteDef::on(
                    Method::POST,
                    format!("{api}/{action}"),
                    move |body: Option<Json<SupervisorBody>>| {
                        let fleet = f.clone();
                        async move {
                            let name = body.and_then(|Json(b)| b.supervisor);
                            let result = if pause {
                                fleet.pause(name.as_deref()).await
                            } else {
                                fleet.resume(name.as_deref()).await
                            };
                            result.map_err(api_error)?;
                            let paused: Vec<String> = fleet
                                .paused()
                                .await
                                .map_err(api_error)?
                                .into_iter()
                                .collect();
                            Ok::<_, AppError>(Json(json!({ "paused": paused })))
                        }
                    },
                ))
                .name(&format!("fleet.{action}")),
            );
        }

        let f = fleet.clone();
        routes.push(
            view(RouteDef::on(
                Method::GET,
                format!("{api}/metrics"),
                move |Query(p): Query<Period>| {
                    let fleet = f.clone();
                    async move {
                        let series = fleet.metrics(p.period).await.map_err(api_error)?;
                        let (queues, jobs) = series
                            .into_iter()
                            .partition(|s| s.kind == crate::record::MetricKind::Queue);
                        Ok::<_, AppError>(Json(MetricsView {
                            period: p.period,
                            queues,
                            jobs,
                        }))
                    }
                },
            ))
            .name("fleet.metrics"),
        );

        routes.extend(crate::ui::routes(fleet, &base));
        routes
    }
}

/// An API route that needs the `view` ability. Its errors are always JSON,
/// even for a client that asks for HTML.
fn view(route: RouteDef) -> RouteDef {
    route.can(VIEW, FleetSubject::SUBJECT, None).json_only()
}

/// An API route that needs the `operate` ability.
fn operate(route: RouteDef) -> RouteDef {
    route.can(OPERATE, FleetSubject::SUBJECT, None).json_only()
}

fn redacted(fleet: &Fleet, mut job: FailedJob) -> FailedJobView {
    job.payload = fleet.redact(&job.payload);
    FailedJobView { job }
}
