//! The routes of a workflow runs page: `/api/workflows/...`.

use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, Query};
use http::Method;
use http::request::Parts;
use navree_db::PageQuery;
use navree_http::{ApiMeta, AppError, Controller, Json, RouteDef, SchemaFn, schema_fn};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_workspace::{CurrentWorkspace, InWorkspace, WorkspaceConfig};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{RunDetails, RunQuery, RunStatus, WorkflowRun, Workflows, WorkflowsError, WorkflowsOptions};

/// The permission of the read routes (`GET /api/workflows...`).
pub const VIEW_PERMISSION: &str = "workflows.view";

/// The permission of the routes that change runs (cancel, retry).
pub const MANAGE_PERMISSION: &str = "workflows.manage";

/// A registered workflow (what `GET /api/workflows` lists).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WorkflowInfo {
    /// Its name.
    pub name: String,
    /// The version new runs start on.
    pub version: u32,
    /// The oldest version whose runs it resumes.
    pub min_version: u32,
    /// Its description.
    pub description: String,
}

/// The query of `GET /api/workflows/runs`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct RunsFilter {
    /// Only runs of this workflow.
    pub workflow: Option<String>,
    /// Only runs with these statuses (comma-separated: `failed,waiting`).
    pub status: Option<String>,
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DataList<T> {
    pub(crate) data: Vec<T>,
}

/// The workspace a request acts on: the request's workspace in an app with
/// `WorkspaceModule` (resolved by the [`InWorkspace`] guard), else `None`.
pub(crate) struct Scope(pub(crate) Option<Uuid>);

impl<S: Send + Sync> FromRequestParts<S> for Scope {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Scope(
            parts
                .extensions
                .get::<CurrentWorkspace>()
                .map(CurrentWorkspace::id),
        ))
    }
}

/// The routes of a workflow runs page, mounted under
/// `{http.api_prefix}/workflows` (default `/api/workflows`) when
/// `workflows.routes` is true. Reads require `workflows.view`, cancel and
/// retry `workflows.manage`; in an app with `WorkspaceModule` they act on
/// the request's workspace (its runs only) and the permission is checked
/// in it.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /workflows` | `{data: [{name, version, min_version, description}]}` |
/// | `GET /workflows/runs?workflow=&status=&page=&per_page=` | a page of runs, newest first |
/// | `GET /workflows/runs/{id}` | the run with its `steps` |
/// | `POST /workflows/runs/{id}/cancel` | the run, cancelled (409 when it finished) |
/// | `POST /workflows/runs/{id}/retry` | the run, pending again (409 unless it failed) |
pub struct WorkflowsController {
    workflows: Arc<Workflows>,
    enabled: bool,
    workspaces: bool,
}

impl Injectable for WorkflowsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Workflows>(),
            ProviderKey::of::<WorkflowsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            Ok(WorkflowsController {
                workflows: r.get::<Workflows>().await?,
                enabled: r.get::<WorkflowsOptions>().await?.routes,
                workspaces: app.try_get::<WorkspaceConfig>().await.is_some(),
            })
        })
    }
}

/// The run `id` when it belongs to `scope` (any run without workspaces),
/// else a 404.
pub(crate) async fn scoped_run(
    workflows: &Workflows,
    scope: Option<Uuid>,
    workspaces: bool,
    id: Uuid,
) -> Result<WorkflowRun, WorkflowsError> {
    let run = workflows.run(id).await?;
    if workspaces && run.workspace_id != scope {
        return Err(WorkflowsError::UnknownRun(id));
    }
    Ok(run)
}

fn statuses(text: Option<&str>) -> Result<Vec<RunStatus>, AppError> {
    let Some(text) = text else {
        return Ok(Vec::new());
    };
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<RunStatus>().map_err(|_| {
                AppError::validation(
                    [(
                        "status".to_owned(),
                        vec![
                            "status must be one of the following values: pending, running, \
                             sleeping, waiting, completed, failed, cancelled"
                                .to_owned(),
                        ],
                    )]
                    .into_iter()
                    .collect(),
                )
            })
        })
        .collect()
}

fn api(
    summary: &str,
    responses: &[u16],
    request: Option<SchemaFn>,
    response: Option<SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Workflows".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}

impl Controller for WorkflowsController {
    fn prefix(&self) -> &'static str {
        "/workflows"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let workspaces = self.workspaces;
        let workflows = self.workflows.clone();
        let index = move || {
            let workflows = workflows.clone();
            async move {
                let data = workflows
                    .definitions()
                    .into_iter()
                    .map(|d| WorkflowInfo {
                        name: d.name.to_owned(),
                        version: d.version,
                        min_version: d.min_version,
                        description: d.description.to_owned(),
                    })
                    .collect();
                Json(DataList::<WorkflowInfo> { data })
            }
        };
        let workflows = self.workflows.clone();
        let runs = move |Scope(scope): Scope, page: PageQuery, Query(filter): Query<RunsFilter>| {
            let workflows = workflows.clone();
            async move {
                let query = RunQuery {
                    workflow: filter.workflow.filter(|w| !w.is_empty()),
                    statuses: statuses(filter.status.as_deref())?,
                    workspace_id: if workspaces { scope } else { None },
                    ..RunQuery::default()
                };
                Ok::<_, AppError>(workflows.page(&query, page).await?)
            }
        };
        let workflows = self.workflows.clone();
        let show = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let workflows = workflows.clone();
            async move {
                scoped_run(&workflows, scope, workspaces, id).await?;
                Ok::<_, AppError>(Json(workflows.details(id).await?))
            }
        };
        let workflows = self.workflows.clone();
        let cancel = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let workflows = workflows.clone();
            async move {
                scoped_run(&workflows, scope, workspaces, id).await?;
                Ok::<_, AppError>(Json(workflows.cancel(id).await?))
            }
        };
        let workflows = self.workflows.clone();
        let retry = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let workflows = workflows.clone();
            async move {
                scoped_run(&workflows, scope, workspaces, id).await?;
                Ok::<_, AppError>(Json(workflows.retry(id).await?))
            }
        };
        let route = move |route: RouteDef, permission: &'static str| {
            let route = route.permissions(&[permission]);
            if workspaces {
                route.guard(InWorkspace::guard())
            } else {
                route
            }
        };
        vec![
            route(RouteDef::on(Method::GET, "/", index), VIEW_PERMISSION)
                .name("workflows.index")
                .api(api(
                    "List the registered workflows",
                    &[401, 403],
                    None,
                    Some(schema_fn::<WorkflowInfo>),
                )),
            route(RouteDef::on(Method::GET, "/runs", runs), VIEW_PERMISSION)
                .name("workflows.runs.index")
                .api(api(
                    "List workflow runs, newest first",
                    &[400, 401, 403],
                    None,
                    Some(schema_fn::<WorkflowRun>),
                )),
            route(RouteDef::on(Method::GET, "/runs/{id}", show), VIEW_PERMISSION)
                .name("workflows.runs.show")
                .api(api(
                    "Show a workflow run with its steps",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<RunDetails>),
                )),
            route(
                RouteDef::on(Method::POST, "/runs/{id}/cancel", cancel),
                MANAGE_PERMISSION,
            )
            .name("workflows.runs.cancel")
            .api(api(
                "Cancel a workflow run",
                &[401, 403, 404, 409],
                None,
                Some(schema_fn::<WorkflowRun>),
            )),
            route(
                RouteDef::on(Method::POST, "/runs/{id}/retry", retry),
                MANAGE_PERMISSION,
            )
            .name("workflows.runs.retry")
            .api(api(
                "Run a failed workflow run again from its failed step",
                &[401, 403, 404, 409],
                None,
                Some(schema_fn::<WorkflowRun>),
            )),
        ]
    }
}
