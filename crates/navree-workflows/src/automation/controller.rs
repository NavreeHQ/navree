//! The routes of an automation editor: `/api/automations/...`.

use std::sync::Arc;

use axum::extract::Path;
use http::{Method, Uri};
use navree_db::{PageQuery, Uow};
use navree_http::{
    ApiMeta, AppError, Controller, Created, Json, NoContent, RouteDef, SchemaFn, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_workspace::{InWorkspace, WorkspaceConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::blocks::BlockInfo;
use super::model::{
    Automation, AutomationTemplate, AutomationVersion, Enrollment, EnrollmentPolicy,
};
use super::runner::{SimulateInput, Simulation};
use super::service::{
    AutomationChanges, AutomationDetails, Automations, NewAutomation, Validation,
};
use crate::controller::{DataList, Scope};
use crate::{WorkflowsError, WorkflowsOptions};

/// The permission of the read routes (catalog, templates, automations,
/// versions, enrollments, validate, simulate).
pub const AUTOMATIONS_VIEW_PERMISSION: &str = "automations.view";

/// The permission of the routes that change automations.
pub const AUTOMATIONS_MANAGE_PERMISSION: &str = "automations.manage";

/// What `GET /api/automations/catalog` answers: the blocks an editor offers,
/// each with the JSON Schema of its configuration, and the built-in node
/// kinds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CatalogResponse {
    /// What can start an automation (and end a `wait` node).
    pub triggers: Vec<BlockInfo>,
    /// What can branch.
    pub conditions: Vec<BlockInfo>,
    /// What can be done.
    pub actions: Vec<BlockInfo>,
    /// The node kinds: `action`, `condition`, `delay`, `wait`.
    pub nodes: Vec<String>,
}

/// The body of `POST /api/automations`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateAutomationInput {
    /// Its name.
    pub name: String,
    /// A note for people.
    #[serde(default)]
    pub description: Option<String>,
    /// The draft definition.
    #[schema(value_type = Object)]
    pub definition: Value,
    /// Who may be enrolled again (default `one_at_a_time`).
    #[serde(default)]
    pub enrollment: EnrollmentPolicy,
}

/// The body of `POST /api/automations/templates/{key}`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct FromTemplateInput {
    /// A name (default: the template's).
    pub name: Option<String>,
}

/// The body of `POST /api/automations/validate`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct ValidateInput {
    /// The definition to check.
    #[schema(value_type = Object)]
    pub definition: Value,
}

/// The body of `POST /api/automations/{id}/enroll`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct EnrollInput {
    /// The trigger's event, as JSON.
    #[schema(value_type = Object)]
    pub event: Value,
}

/// The routes of an automation editor, mounted under
/// `{http.api_prefix}/automations` when `workflows.automations` and
/// `workflows.automation_routes` are true and the app has
/// `DatabaseModule`. Reads require `automations.view`, changes
/// `automations.manage`; in an app with `WorkspaceModule` they act on the
/// request's workspace.
pub struct AutomationsController {
    automations: Arc<Automations>,
    enabled: bool,
    workspaces: bool,
}

impl Injectable for AutomationsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Automations>(),
            ProviderKey::of::<WorkflowsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let options = r.get::<WorkflowsOptions>().await?;
            let has_database = app.try_get::<navree_db::Db>().await.is_some();
            Ok(AutomationsController {
                automations: r.get::<Automations>().await?,
                enabled: options.automations && options.automation_routes && has_database,
                workspaces: app.try_get::<WorkspaceConfig>().await.is_some(),
            })
        })
    }
}

/// The automation `id` when it belongs to `scope`, else a 404.
async fn scoped(
    automations: &Automations,
    scope: Option<Uuid>,
    id: Uuid,
) -> Result<Automation, WorkflowsError> {
    let automation = automations.get(id).await?;
    if automation.workspace_id != scope {
        return Err(WorkflowsError::UnknownAutomation(id));
    }
    Ok(automation)
}

/// The enrollment `enrollment` of automation `id`, else a 404.
async fn scoped_enrollment(
    automations: &Automations,
    scope: Option<Uuid>,
    id: Uuid,
    enrollment: Uuid,
) -> Result<Enrollment, WorkflowsError> {
    scoped(automations, scope, id).await?;
    let found = automations.enrollment(enrollment).await?;
    if found.automation_id != id {
        return Err(WorkflowsError::UnknownEnrollment(enrollment));
    }
    Ok(found)
}

fn api(
    summary: &str,
    responses: &[u16],
    request: Option<SchemaFn>,
    response: Option<SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Automations".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}

fn created(
    summary: &str,
    responses: &[u16],
    request: Option<SchemaFn>,
    response: SchemaFn,
) -> ApiMeta {
    ApiMeta {
        success_status: 201,
        ..api(summary, responses, request, Some(response))
    }
}

fn location(uri: &Uri, tail: &str, id: Uuid) -> String {
    let base = uri.path();
    let base = base
        .strip_suffix(tail)
        .unwrap_or(base)
        .trim_end_matches('/');
    format!("{base}/{id}")
}

impl Controller for AutomationsController {
    fn prefix(&self) -> &'static str {
        "/automations"
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let a = self.automations.clone();
        let catalog = move || {
            let a = a.clone();
            async move {
                let catalog = a.catalog();
                Json(CatalogResponse {
                    triggers: catalog.triggers(),
                    conditions: catalog.conditions(),
                    actions: catalog.actions(),
                    nodes: ["action", "condition", "delay", "wait"]
                        .map(str::to_owned)
                        .to_vec(),
                })
            }
        };
        let a = self.automations.clone();
        let templates = move || {
            let a = a.clone();
            async move {
                Json(DataList::<AutomationTemplate> {
                    data: a.templates(),
                })
            }
        };
        let a = self.automations.clone();
        let from_template =
            move |Scope(scope): Scope,
                  uow: Uow,
                  uri: Uri,
                  Path(key): Path<String>,
                  body: Option<Json<FromTemplateInput>>| {
                let a = a.clone();
                async move {
                    let name = body.and_then(|Json(input)| input.name);
                    let automation = a.create_from_template(&uow, &key, scope, name).await?;
                    let prefix = uri.path().split("/templates/").next().unwrap_or_default();
                    let location = format!("{prefix}/{}", automation.id);
                    Ok::<_, AppError>(Created(location, automation))
                }
            };
        let a = self.automations.clone();
        let validate = move |Json(input): Json<ValidateInput>| {
            let a = a.clone();
            async move { Json(a.validate(&input.definition)) }
        };
        let a = self.automations.clone();
        let index = move |Scope(scope): Scope, page: PageQuery| {
            let a = a.clone();
            async move { Ok::<_, AppError>(a.page(scope, page).await?) }
        };
        let a = self.automations.clone();
        let store = move |Scope(scope): Scope,
                          uow: Uow,
                          uri: Uri,
                          Json(input): Json<CreateAutomationInput>| {
            let a = a.clone();
            async move {
                let new = NewAutomation {
                    name: input.name,
                    description: input.description,
                    workspace_id: scope,
                    definition: input.definition,
                    enrollment: input.enrollment,
                };
                let automation = a.create(&uow, new).await?;
                let location = location(&uri, "", automation.id);
                Ok::<_, AppError>(Created(location, automation))
            }
        };
        let a = self.automations.clone();
        let show = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                Ok::<_, AppError>(Json(a.details(id).await?))
            }
        };
        let a = self.automations.clone();
        let update = move |Scope(scope): Scope,
                           uow: Uow,
                           Path(id): Path<Uuid>,
                           Json(changes): Json<AutomationChanges>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                Ok::<_, AppError>(Json(a.update(&uow, id, changes).await?))
            }
        };
        let a = self.automations.clone();
        let destroy = move |Scope(scope): Scope, uow: Uow, Path(id): Path<Uuid>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                a.delete(&uow, id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let a = self.automations.clone();
        let publish = move |Scope(scope): Scope, uow: Uow, Path(id): Path<Uuid>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                let actor = uow.actor();
                Ok::<_, AppError>(Json(a.publish(&uow, id, actor).await?))
            }
        };
        let a = self.automations.clone();
        let pause = move |Scope(scope): Scope, uow: Uow, Path(id): Path<Uuid>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                Ok::<_, AppError>(Json(a.pause(&uow, id).await?))
            }
        };
        let a = self.automations.clone();
        let resume = move |Scope(scope): Scope, uow: Uow, Path(id): Path<Uuid>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                Ok::<_, AppError>(Json(a.resume(&uow, id).await?))
            }
        };
        let a = self.automations.clone();
        let versions = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                let data = a.versions(id).await?;
                Ok::<_, AppError>(Json(DataList::<AutomationVersion> { data }))
            }
        };
        let a = self.automations.clone();
        let simulate =
            move |Scope(scope): Scope, Path(id): Path<Uuid>, Json(input): Json<SimulateInput>| {
                let a = a.clone();
                async move {
                    scoped(&a, scope, id).await?;
                    Ok::<_, AppError>(Json(a.simulate(id, input).await?))
                }
            };
        let a = self.automations.clone();
        let enroll = move |Scope(scope): Scope,
                           uri: Uri,
                           Path(id): Path<Uuid>,
                           Json(input): Json<EnrollInput>| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                let enrollment = a.enroll(id, input.event).await?;
                let base = uri.path().trim_end_matches("/enroll");
                let location = format!("{base}/enrollments/{}", enrollment.id);
                Ok::<_, AppError>(Created(location, enrollment))
            }
        };
        let a = self.automations.clone();
        let enrollments = move |Scope(scope): Scope, Path(id): Path<Uuid>, page: PageQuery| {
            let a = a.clone();
            async move {
                scoped(&a, scope, id).await?;
                Ok::<_, AppError>(a.enrollments(id, page).await?)
            }
        };
        let a = self.automations.clone();
        let show_enrollment =
            move |Scope(scope): Scope, Path((id, enrollment)): Path<(Uuid, Uuid)>| {
                let a = a.clone();
                async move {
                    Ok::<_, AppError>(Json(scoped_enrollment(&a, scope, id, enrollment).await?))
                }
            };
        let a = self.automations.clone();
        let cancel_enrollment =
            move |Scope(scope): Scope, Path((id, enrollment)): Path<(Uuid, Uuid)>| {
                let a = a.clone();
                async move {
                    scoped_enrollment(&a, scope, id, enrollment).await?;
                    Ok::<_, AppError>(Json(a.cancel_enrollment(enrollment).await?))
                }
            };
        let workspaces = self.workspaces;
        let view = move |route: RouteDef| {
            let route = route.permissions(&[AUTOMATIONS_VIEW_PERMISSION]);
            if workspaces {
                route.guard(InWorkspace::guard())
            } else {
                route
            }
        };
        let manage = move |route: RouteDef| {
            let route = route.permissions(&[AUTOMATIONS_MANAGE_PERMISSION]);
            if workspaces {
                route.guard(InWorkspace::guard())
            } else {
                route
            }
        };
        vec![
            view(RouteDef::on(Method::GET, "/catalog", catalog))
                .name("automations.catalog")
                .api(api(
                    "List the triggers, conditions and actions with their JSON Schemas",
                    &[401, 403],
                    None,
                    Some(schema_fn::<CatalogResponse>),
                )),
            view(RouteDef::on(Method::GET, "/templates", templates))
                .name("automations.templates.index")
                .api(api(
                    "List the automation templates",
                    &[401, 403],
                    None,
                    Some(schema_fn::<AutomationTemplate>),
                )),
            manage(RouteDef::on(
                Method::POST,
                "/templates/{key}",
                from_template,
            ))
            .name("automations.templates.use")
            .api(created(
                "Create a draft automation from a template",
                &[400, 401, 403, 404],
                Some(schema_fn::<FromTemplateInput>),
                schema_fn::<Automation>,
            )),
            view(RouteDef::on(Method::POST, "/validate", validate))
                .name("automations.validate")
                .api(api(
                    "Check a definition against the catalog",
                    &[400, 401, 403],
                    Some(schema_fn::<ValidateInput>),
                    Some(schema_fn::<Validation>),
                )),
            view(RouteDef::on(Method::GET, "/", index))
                .name("automations.index")
                .api(api(
                    "List the automations, newest first",
                    &[400, 401, 403],
                    None,
                    Some(schema_fn::<Automation>),
                )),
            manage(RouteDef::on(Method::POST, "/", store))
                .name("automations.store")
                .api(created(
                    "Create a draft automation",
                    &[400, 401, 403],
                    Some(schema_fn::<CreateAutomationInput>),
                    schema_fn::<Automation>,
                )),
            view(RouteDef::on(Method::GET, "/{id}", show))
                .name("automations.show")
                .api(api(
                    "Show an automation with its draft's problems",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<AutomationDetails>),
                )),
            manage(RouteDef::on(Method::PATCH, "/{id}", update))
                .name("automations.update")
                .api(api(
                    "Change an automation's draft, name or enrollment policy",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<AutomationChanges>),
                    Some(schema_fn::<Automation>),
                )),
            manage(RouteDef::on(Method::DELETE, "/{id}", destroy))
                .name("automations.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api(
                        "Delete an automation and cancel its enrollments",
                        &[401, 403, 404],
                        None,
                        None,
                    )
                }),
            manage(RouteDef::on(Method::POST, "/{id}/publish", publish))
                .name("automations.publish")
                .api(api(
                    "Publish the draft as the next version and activate the automation",
                    &[400, 401, 403, 404],
                    None,
                    Some(schema_fn::<Automation>),
                )),
            manage(RouteDef::on(Method::POST, "/{id}/pause", pause))
                .name("automations.pause")
                .api(api(
                    "Stop enrolling",
                    &[401, 403, 404, 409],
                    None,
                    Some(schema_fn::<Automation>),
                )),
            manage(RouteDef::on(Method::POST, "/{id}/resume", resume))
                .name("automations.resume")
                .api(api(
                    "Enroll again",
                    &[401, 403, 404, 409],
                    None,
                    Some(schema_fn::<Automation>),
                )),
            view(RouteDef::on(Method::GET, "/{id}/versions", versions))
                .name("automations.versions")
                .api(api(
                    "List the published versions, newest first",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<AutomationVersion>),
                )),
            view(RouteDef::on(Method::POST, "/{id}/simulate", simulate))
                .name("automations.simulate")
                .api(api(
                    "Simulate an enrollment without storing or running anything",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<SimulateInput>),
                    Some(schema_fn::<Simulation>),
                )),
            manage(RouteDef::on(Method::POST, "/{id}/enroll", enroll))
                .name("automations.enroll")
                .api(created(
                    "Enroll an event in the published version now",
                    &[400, 401, 403, 404, 409],
                    Some(schema_fn::<EnrollInput>),
                    schema_fn::<Enrollment>,
                )),
            view(RouteDef::on(Method::GET, "/{id}/enrollments", enrollments))
                .name("automations.enrollments.index")
                .api(api(
                    "List the enrollments, newest first",
                    &[400, 401, 403, 404],
                    None,
                    Some(schema_fn::<Enrollment>),
                )),
            view(RouteDef::on(
                Method::GET,
                "/{id}/enrollments/{enrollment}",
                show_enrollment,
            ))
            .name("automations.enrollments.show")
            .api(api(
                "Show an enrollment",
                &[401, 403, 404],
                None,
                Some(schema_fn::<Enrollment>),
            )),
            manage(RouteDef::on(
                Method::POST,
                "/{id}/enrollments/{enrollment}/cancel",
                cancel_enrollment,
            ))
            .name("automations.enrollments.cancel")
            .api(api(
                "Cancel an enrollment under way",
                &[401, 403, 404, 409],
                None,
                Some(schema_fn::<Enrollment>),
            )),
        ]
    }
}
