//! The routes of a directory settings page: `/api/directory/...`.

use std::sync::Arc;

use axum::extract::{Path, Query};
use http::{Method, StatusCode, Uri};
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, RouteDef, SchemaFn,
    schema_fn,
};
use navree_integrations::Level;
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_orm::Model as _;
use navree_workspace::{InOrganization, InWorkspace};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::options::DirectoryOptions;
use crate::owner::DirectoryOwner;
use crate::service::{ConnectionUpdate, CreatedConnection, NewConnection, synced_user};
use crate::types::{DirectoryConnection, SyncChange, SyncOptions, SyncRun, SyncedGroup, SyncedUser};
use crate::{Directory, DirectoryError, models};

/// The permission every directory route requires.
pub const MANAGE_PERMISSION: &str = "directory.manage";

/// The body of `POST /api/directory/connections/{id}/sync`.
#[derive(Clone, Copy, Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct SyncInput {
    /// Plan only.
    pub dry_run: bool,
    /// Apply past the deactivation limit.
    pub force: bool,
    /// Ignore the delta cursor.
    pub full: bool,
}

/// `page` and `per_page` of the listing routes.
#[derive(Clone, Copy, Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageInput {
    /// The page (default 1).
    pub page: Option<u64>,
    /// Items per page (default 50, at most 200).
    pub per_page: Option<u64>,
}

impl PageInput {
    fn resolve(self) -> (u64, u64) {
        let page = self.page.unwrap_or(1).max(1);
        let per_page = self.per_page.unwrap_or(50).clamp(1, 200);
        (page, per_page)
    }
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
struct DataList<T> {
    data: Vec<T>,
}

/// A page: `{"data": [...], "meta": {current_page, per_page, total, last_page}}`.
#[derive(Clone, Debug, Serialize)]
struct PageOf<T> {
    data: Vec<T>,
    meta: PageMeta,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct PageMeta {
    current_page: u64,
    per_page: u64,
    total: u64,
    last_page: u64,
}

impl PageMeta {
    fn new(page: u64, per_page: u64, total: u64) -> Self {
        Self {
            current_page: page,
            per_page,
            total,
            last_page: total.div_ceil(per_page).max(1),
        }
    }
}

/// The new SCIM token of `POST /api/directory/connections/{id}/token`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ScimTokenOutput {
    /// The bearer token (shown once).
    pub token: String,
    /// The SCIM base URL.
    pub scim_url: String,
}

/// The routes of a directory settings page, mounted under
/// `{http.api_prefix}/directory` (default `/api/directory`) when
/// `directory.routes` is true. Every route requires the
/// `directory.manage` permission; at `level = "workspace"` they act on
/// the request's workspace (the `InWorkspace` guard), at `organization` on
/// its organization, at `app` on the app's own connections.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /directory/sources` | `{data: ["scim", "microsoft", ...]}` |
/// | `GET /directory/connections` | `{data: [DirectoryConnection]}` |
/// | `POST /directory/connections` `{name, source, settings?, secret?, sync_interval_mins?}` | 201 `{connection, scim_token?, scim_url?}` |
/// | `GET /directory/connections/{id}` | the connection |
/// | `PATCH /directory/connections/{id}` `{name?, settings?, secret?, sync_interval_mins?, status?}` | the connection |
/// | `DELETE /directory/connections/{id}` | 204 |
/// | `POST /directory/connections/{id}/token` | 201 `{token, scim_url}` |
/// | `POST /directory/connections/{id}/sync` `{dry_run?, force?, full?}` | 202, the queued run |
/// | `GET /directory/connections/{id}/runs` | `{data: [SyncRun]}` (latest 50) |
/// | `GET /directory/connections/{id}/users?page&per_page` | a page of `SyncedUser` |
/// | `GET /directory/connections/{id}/groups` | `{data: [SyncedGroup]}` |
/// | `GET /directory/runs/{id}` | the run |
/// | `GET /directory/runs/{id}/changes?page&per_page` | a page of `SyncChange` |
/// | `POST /directory/runs/{id}/approve` | 202, the queued run |
pub struct DirectoryController {
    directory: Arc<Directory>,
    enabled: bool,
}

impl Injectable for DirectoryController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Directory>(),
            ProviderKey::of::<DirectoryOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(DirectoryController {
                directory: r.get::<Directory>().await?,
                enabled: r.get::<DirectoryOptions>().await?.routes,
            })
        })
    }
}

fn api(summary: &str, responses: &[u16], request: Option<SchemaFn>, response: Option<SchemaFn>) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Directory".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}

/// `{prefix}/directory/connections/{id}` from the request's path.
fn location(uri: &Uri, id: Uuid) -> String {
    let path = uri.path();
    let prefix = path
        .rsplit_once("/directory/")
        .map_or(path, |(prefix, _)| prefix);
    format!("{prefix}/directory/connections/{id}")
}

/// 202 with a body.
struct Accepted<T>(T);

impl<T: Serialize> axum::response::IntoResponse for Accepted<T> {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::ACCEPTED, Json(self.0)).into_response()
    }
}

impl Controller for DirectoryController {
    fn prefix(&self) -> &'static str {
        "/directory"
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let level = match self.directory.level() {
            Ok(level) => level,
            Err(error) => {
                tracing::warn!(target: "navree::directory", %error, "the directory routes are not mounted");
                return Vec::new();
            }
        };

        let d = self.directory.clone();
        let sources = move || {
            let d = d.clone();
            async move { Ok::<_, AppError>(Json(DataList { data: d.sources()? })) }
        };
        let d = self.directory.clone();
        let index = move |DirectoryOwner(owner): DirectoryOwner| {
            let d = d.clone();
            async move {
                let data = d.connections(owner).await?;
                Ok::<_, AppError>(Json(DataList::<DirectoryConnection> { data }))
            }
        };
        let d = self.directory.clone();
        let store = move |DirectoryOwner(owner): DirectoryOwner,
                          CurrentPrincipal(principal): CurrentPrincipal,
                          uri: Uri,
                          Json(input): Json<NewConnection>| {
            let d = d.clone();
            async move {
                let input = NewConnection {
                    created_by: Some(principal.user_id),
                    ..input
                };
                let created = d.create_connection(owner, input).await?;
                let at = location(&uri, created.connection.id);
                Ok::<_, AppError>(Created(at, created))
            }
        };
        let d = self.directory.clone();
        let show = move |DirectoryOwner(owner): DirectoryOwner, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move { Ok::<_, AppError>(Json(d.connection_for(owner, id).await?)) }
        };
        let d = self.directory.clone();
        let update = move |DirectoryOwner(owner): DirectoryOwner,
                           Path(id): Path<Uuid>,
                           Json(input): Json<ConnectionUpdate>| {
            let d = d.clone();
            async move {
                d.connection_for(owner, id).await?;
                Ok::<_, AppError>(Json(d.update_connection(id, input).await?))
            }
        };
        let d = self.directory.clone();
        let destroy = move |DirectoryOwner(owner): DirectoryOwner, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                d.connection_for(owner, id).await?;
                d.delete_connection(id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let d = self.directory.clone();
        let token = move |DirectoryOwner(owner): DirectoryOwner, uri: Uri, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                d.connection_for(owner, id).await?;
                let token = d.rotate_scim_token(id).await?;
                let output = ScimTokenOutput {
                    token: token.expose().clone(),
                    scim_url: d.scim_url()?,
                };
                Ok::<_, AppError>(Created(location(&uri, id), output))
            }
        };
        let d = self.directory.clone();
        let sync = move |DirectoryOwner(owner): DirectoryOwner,
                         Path(id): Path<Uuid>,
                         body: Option<Json<SyncInput>>| {
            let d = d.clone();
            async move {
                d.connection_for(owner, id).await?;
                let input = body.map(|Json(input)| input).unwrap_or_default();
                let options = SyncOptions {
                    dry_run: input.dry_run,
                    force: input.force,
                    full: input.full,
                    ..SyncOptions::default()
                };
                Ok::<_, AppError>(Accepted(d.sync(id, options).await?))
            }
        };
        let d = self.directory.clone();
        let runs = move |DirectoryOwner(owner): DirectoryOwner, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                d.connection_for(owner, id).await?;
                let data = d.runs(id, 50).await?;
                Ok::<_, AppError>(Json(DataList::<SyncRun> { data }))
            }
        };
        let d = self.directory.clone();
        let users = move |DirectoryOwner(owner): DirectoryOwner,
                          Path(id): Path<Uuid>,
                          Query(page): Query<PageInput>| {
            let d = d.clone();
            async move {
                d.connection_for(owner, id).await?;
                let (page, per_page) = page.resolve();
                let rt = d.rt()?;
                let query = || {
                    models::User::query().where_(models::User::CONNECTION_ID.eq(id))
                };
                let total = query().count(&*rt.db).await.map_err(DirectoryError::from)?;
                let rows = query()
                    .order_by(models::User::USER_NAME)
                    .offset((page - 1) * per_page)
                    .limit(per_page)
                    .get(&*rt.db)
                    .await
                    .map_err(DirectoryError::from)?
                    .into_models();
                let data: Vec<SyncedUser> = rows.iter().map(synced_user).collect();
                Ok::<_, AppError>(Json(PageOf {
                    data,
                    meta: PageMeta::new(page, per_page, total),
                }))
            }
        };
        let d = self.directory.clone();
        let groups = move |DirectoryOwner(owner): DirectoryOwner, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                d.connection_for(owner, id).await?;
                let data = d.groups(id).await?;
                Ok::<_, AppError>(Json(DataList::<SyncedGroup> { data }))
            }
        };
        let d = self.directory.clone();
        let run = move |DirectoryOwner(owner): DirectoryOwner, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                let run = d.run(id).await?;
                d.connection_for(owner, run.connection_id)
                    .await
                    .map_err(|_| DirectoryError::RunNotFound)?;
                Ok::<_, AppError>(Json(run))
            }
        };
        let d = self.directory.clone();
        let changes = move |DirectoryOwner(owner): DirectoryOwner,
                            Path(id): Path<Uuid>,
                            Query(page): Query<PageInput>| {
            let d = d.clone();
            async move {
                let run = d.run(id).await?;
                d.connection_for(owner, run.connection_id)
                    .await
                    .map_err(|_| DirectoryError::RunNotFound)?;
                let (page, per_page) = page.resolve();
                let rt = d.rt()?;
                let total = models::RunChange::query()
                    .where_(models::RunChange::RUN_ID.eq(id))
                    .count(&*rt.db)
                    .await
                    .map_err(DirectoryError::from)?;
                let data = d.changes(id, (page - 1) * per_page, per_page).await?;
                Ok::<_, AppError>(Json(PageOf::<SyncChange> {
                    data,
                    meta: PageMeta::new(page, per_page, total),
                }))
            }
        };
        let d = self.directory.clone();
        let approve = move |DirectoryOwner(owner): DirectoryOwner, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                let run = d.run(id).await?;
                d.connection_for(owner, run.connection_id)
                    .await
                    .map_err(|_| DirectoryError::RunNotFound)?;
                Ok::<_, AppError>(Accepted(d.approve(id).await?))
            }
        };

        let route = move |route: RouteDef| {
            let route = route.permissions(&[MANAGE_PERMISSION]);
            match level {
                Level::Workspace => route.guard(InWorkspace::guard()),
                Level::Organization => route.guard(InOrganization::guard()),
                Level::App => route,
            }
        };
        vec![
            route(RouteDef::on(Method::GET, "/sources", sources))
                .name("directory.sources")
                .api(api("List the directory sources", &[401, 403], None, None)),
            route(RouteDef::on(Method::GET, "/connections", index))
                .name("directory.connections.index")
                .api(api(
                    "List the directory connections",
                    &[401, 403],
                    None,
                    Some(schema_fn::<DirectoryConnection>),
                )),
            route(RouteDef::on(Method::POST, "/connections", store))
                .name("directory.connections.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Create a directory connection",
                        &[400, 401, 403],
                        Some(schema_fn::<NewConnection>),
                        Some(schema_fn::<CreatedConnection>),
                    )
                }),
            route(RouteDef::on(Method::GET, "/connections/{id}", show))
                .name("directory.connections.show")
                .api(api(
                    "Show a directory connection",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<DirectoryConnection>),
                )),
            route(RouteDef::on(Method::PATCH, "/connections/{id}", update))
                .name("directory.connections.update")
                .api(api(
                    "Change a directory connection",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<ConnectionUpdate>),
                    Some(schema_fn::<DirectoryConnection>),
                )),
            route(RouteDef::on(Method::DELETE, "/connections/{id}", destroy))
                .name("directory.connections.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Delete a directory connection", &[401, 403, 404], None, None)
                }),
            route(RouteDef::on(Method::POST, "/connections/{id}/token", token))
                .name("directory.connections.token")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Give a SCIM connection a new bearer token",
                        &[400, 401, 403, 404],
                        None,
                        Some(schema_fn::<ScimTokenOutput>),
                    )
                }),
            route(RouteDef::on(Method::POST, "/connections/{id}/sync", sync))
                .name("directory.connections.sync")
                .api(ApiMeta {
                    success_status: 202,
                    ..api(
                        "Queue a sync of a directory connection",
                        &[400, 401, 403, 404, 409],
                        Some(schema_fn::<SyncInput>),
                        Some(schema_fn::<SyncRun>),
                    )
                }),
            route(RouteDef::on(Method::GET, "/connections/{id}/runs", runs))
                .name("directory.connections.runs")
                .api(api(
                    "List a connection's latest sync runs",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<SyncRun>),
                )),
            route(RouteDef::on(Method::GET, "/connections/{id}/users", users))
                .name("directory.connections.users")
                .api(api(
                    "List a connection's synced users",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<SyncedUser>),
                )),
            route(RouteDef::on(Method::GET, "/connections/{id}/groups", groups))
                .name("directory.connections.groups")
                .api(api(
                    "List a connection's synced groups",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<SyncedGroup>),
                )),
            route(RouteDef::on(Method::GET, "/runs/{id}", run))
                .name("directory.runs.show")
                .api(api(
                    "Show a sync run",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<SyncRun>),
                )),
            route(RouteDef::on(Method::GET, "/runs/{id}/changes", changes))
                .name("directory.runs.changes")
                .api(api(
                    "List a sync run's changes",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<SyncChange>),
                )),
            route(RouteDef::on(Method::POST, "/runs/{id}/approve", approve))
                .name("directory.runs.approve")
                .api(ApiMeta {
                    success_status: 202,
                    ..api(
                        "Approve a held run: queue it again past the deactivation limit",
                        &[401, 403, 404, 409],
                        None,
                        Some(schema_fn::<SyncRun>),
                    )
                }),
        ]
    }
}
