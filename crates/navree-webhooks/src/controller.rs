//! The routes of a webhooks settings page: `/api/webhooks/...`.

use std::collections::BTreeMap;
use std::sync::Arc;

use http::request::Parts;
use http::{Method, Uri};
use navree_db::{PageQuery, Uow};
use navree_http::extract::FromRequestParts;
use navree_http::{
    ApiMeta, AppError, Controller, Created, Json, NoContent, Path, RouteDef, SchemaFn, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_workspace::{CurrentWorkspace, InWorkspace, WorkspaceConfig};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Attempt, CreatedEndpoint, Delivery, Endpoint, EndpointChanges, EventType, NewEndpoint,
    Webhooks, WebhooksOptions,
};

/// The permission every webhooks route requires.
pub const MANAGE_PERMISSION: &str = "webhooks.manage";

/// The body of `POST /api/webhooks/endpoints`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateEndpointInput {
    /// The URL deliveries are posted to (`https`).
    pub url: String,
    /// The event types it receives (`*` for every type).
    pub events: Vec<String>,
    /// A note for people.
    pub description: Option<String>,
    /// Extra headers sent with every delivery.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

/// The body of `PATCH /api/webhooks/endpoints/{id}`: every field is
/// optional.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct UpdateEndpointInput {
    /// A new URL.
    pub url: Option<String>,
    /// New event types.
    pub events: Option<Vec<String>>,
    /// A new description (`""` removes it).
    pub description: Option<String>,
    /// New extra headers (replacing the old ones).
    pub headers: Option<BTreeMap<String, String>>,
    /// Turns delivery on or off.
    pub enabled: Option<bool>,
}

/// What `POST /api/webhooks/endpoints/{id}/rotate-secret` returns.
#[derive(Clone, Serialize, ToSchema)]
pub struct RotatedSecret {
    /// The new secret; the old one keeps signing for 24 hours.
    pub secret: String,
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
struct DataList<T> {
    data: Vec<T>,
}

/// The workspace a request acts on: the request's workspace in an app with
/// `WorkspaceModule` (resolved by the [`InWorkspace`] guard), else `None`
/// (app-wide endpoints).
struct Scope(Option<Uuid>);

impl FromRequestParts for Scope {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Scope(
            parts
                .extensions
                .get::<CurrentWorkspace>()
                .map(CurrentWorkspace::id),
        ))
    }
}

/// The routes of a webhooks settings page, mounted under
/// `{http.api_prefix}/webhooks` (default `/api/webhooks`) when
/// `webhooks.routes` is true and the app has `DatabaseModule`. Every route
/// requires the `webhooks.manage` permission; in an app with
/// `WorkspaceModule` they act on the request's workspace (resolved by the
/// `InWorkspace` guard: `X-Workspace` header, path, subdomain or claim)
/// and the permission is checked in it.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /webhooks/event-types` | `{data: [{name, description}]}` |
/// | `GET /webhooks/endpoints` | `{data: [Endpoint]}` |
/// | `POST /webhooks/endpoints` `{url, events, description?, headers?}` | 201, the endpoint with its `secret` |
/// | `GET /webhooks/endpoints/{id}` | the endpoint |
/// | `PATCH /webhooks/endpoints/{id}` `{url?, events?, description?, headers?, enabled?}` | the endpoint |
/// | `DELETE /webhooks/endpoints/{id}` | 204 |
/// | `POST /webhooks/endpoints/{id}/rotate-secret` | `{secret}` |
/// | `POST /webhooks/endpoints/{id}/test` | the attempt |
/// | `GET /webhooks/endpoints/{id}/deliveries` | a page of deliveries, newest first |
/// | `POST /webhooks/deliveries/{id}/retry` | the delivery, queued again (409 unless it failed) |
pub struct WebhooksController {
    webhooks: Arc<Webhooks>,
    enabled: bool,
    workspaces: bool,
}

impl Injectable for WebhooksController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Webhooks>(),
            ProviderKey::of::<WebhooksOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let has_database = app.try_get::<navree_db::Db>().await.is_some();
            Ok(WebhooksController {
                webhooks: r.get::<Webhooks>().await?,
                enabled: r.get::<WebhooksOptions>().await?.routes && has_database,
                workspaces: app.try_get::<WorkspaceConfig>().await.is_some(),
            })
        })
    }
}

/// The endpoint `id` when it belongs to `scope`, else a 404.
async fn scoped_endpoint(
    webhooks: &Webhooks,
    scope: Option<Uuid>,
    id: Uuid,
) -> Result<Endpoint, AppError> {
    let endpoint = webhooks.endpoint(id).await?;
    if endpoint.workspace_id != scope {
        return Err(AppError::not_found());
    }
    Ok(endpoint)
}

fn api(
    summary: &str,
    responses: &[u16],
    request: Option<SchemaFn>,
    response: Option<SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Webhooks".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}

impl Controller for WebhooksController {
    fn prefix(&self) -> &'static str {
        "/webhooks"
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let webhooks = self.webhooks.clone();
        let event_types = move || {
            let webhooks = webhooks.clone();
            async move {
                Json(DataList::<EventType> {
                    data: webhooks.event_types(),
                })
            }
        };
        let webhooks = self.webhooks.clone();
        let index = move |Scope(scope): Scope| {
            let webhooks = webhooks.clone();
            async move {
                let data = webhooks.endpoints(scope).await?;
                Ok::<_, AppError>(Json(DataList::<Endpoint> { data }))
            }
        };
        let webhooks = self.webhooks.clone();
        let store = move |Scope(scope): Scope,
                          uow: Uow,
                          uri: Uri,
                          Json(input): Json<CreateEndpointInput>| {
            let webhooks = webhooks.clone();
            async move {
                let mut new = NewEndpoint::new(input.url).events(input.events);
                new.workspace_id = scope;
                new.description = input.description;
                new.headers = input.headers;
                let created = webhooks.create_endpoint(&uow, new).await?;
                let location = format!(
                    "{}/{}",
                    uri.path().trim_end_matches('/'),
                    created.endpoint.id
                );
                Ok::<_, AppError>(Created(location, created))
            }
        };
        let webhooks = self.webhooks.clone();
        let show = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let webhooks = webhooks.clone();
            async move { Ok::<_, AppError>(Json(scoped_endpoint(&webhooks, scope, id).await?)) }
        };
        let webhooks = self.webhooks.clone();
        let update = move |Scope(scope): Scope,
                           uow: Uow,
                           Path(id): Path<Uuid>,
                           Json(input): Json<UpdateEndpointInput>| {
            let webhooks = webhooks.clone();
            async move {
                scoped_endpoint(&webhooks, scope, id).await?;
                let changes = EndpointChanges {
                    url: input.url,
                    events: input.events,
                    description: input.description,
                    headers: input.headers,
                };
                let mut endpoint = webhooks.update_endpoint(&uow, id, changes).await?;
                match input.enabled {
                    Some(true) if !endpoint.enabled => {
                        endpoint = webhooks.enable(&uow, id).await?;
                    }
                    Some(false) if endpoint.enabled => {
                        endpoint = webhooks.disable(&uow, id).await?;
                    }
                    _ => {}
                }
                Ok::<_, AppError>(Json(endpoint))
            }
        };
        let webhooks = self.webhooks.clone();
        let destroy = move |Scope(scope): Scope, uow: Uow, Path(id): Path<Uuid>| {
            let webhooks = webhooks.clone();
            async move {
                scoped_endpoint(&webhooks, scope, id).await?;
                webhooks.delete_endpoint(&uow, id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let webhooks = self.webhooks.clone();
        let rotate = move |Scope(scope): Scope, uow: Uow, Path(id): Path<Uuid>| {
            let webhooks = webhooks.clone();
            async move {
                scoped_endpoint(&webhooks, scope, id).await?;
                let secret = webhooks.rotate_secret(&uow, id).await?;
                Ok::<_, AppError>(Json(RotatedSecret { secret }))
            }
        };
        let webhooks = self.webhooks.clone();
        let test = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let webhooks = webhooks.clone();
            async move {
                scoped_endpoint(&webhooks, scope, id).await?;
                Ok::<_, AppError>(Json(webhooks.send_test(id).await?))
            }
        };
        let webhooks = self.webhooks.clone();
        let deliveries = move |Scope(scope): Scope, Path(id): Path<Uuid>, page: PageQuery| {
            let webhooks = webhooks.clone();
            async move {
                scoped_endpoint(&webhooks, scope, id).await?;
                Ok::<_, AppError>(webhooks.deliveries(id, page).await?)
            }
        };
        let webhooks = self.webhooks.clone();
        let retry = move |Scope(scope): Scope, Path(id): Path<Uuid>| {
            let webhooks = webhooks.clone();
            async move {
                let delivery = webhooks.delivery(id).await?;
                if delivery.workspace_id != scope {
                    return Err(AppError::not_found());
                }
                Ok::<_, AppError>(Json(webhooks.retry(id).await?))
            }
        };
        let workspaces = self.workspaces;
        let route = move |route: RouteDef| {
            let route = route.permissions(&[MANAGE_PERMISSION]);
            if workspaces {
                route.guard(InWorkspace::guard())
            } else {
                route
            }
        };
        vec![
            route(RouteDef::on(Method::GET, "/event-types", event_types))
                .name("webhooks.event_types")
                .api(api(
                    "List the webhook event types",
                    &[401, 403],
                    None,
                    Some(schema_fn::<EventType>),
                )),
            route(RouteDef::on(Method::GET, "/endpoints", index))
                .name("webhooks.endpoints.index")
                .api(api(
                    "List the webhook endpoints",
                    &[401, 403],
                    None,
                    Some(schema_fn::<Endpoint>),
                )),
            route(RouteDef::on(Method::POST, "/endpoints", store))
                .name("webhooks.endpoints.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Create a webhook endpoint (the response holds its secret)",
                        &[400, 401, 403],
                        Some(schema_fn::<CreateEndpointInput>),
                        Some(schema_fn::<CreatedEndpoint>),
                    )
                }),
            route(RouteDef::on(Method::GET, "/endpoints/{id}", show))
                .name("webhooks.endpoints.show")
                .api(api(
                    "Show a webhook endpoint",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<Endpoint>),
                )),
            route(RouteDef::on(Method::PATCH, "/endpoints/{id}", update))
                .name("webhooks.endpoints.update")
                .api(api(
                    "Change a webhook endpoint",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<UpdateEndpointInput>),
                    Some(schema_fn::<Endpoint>),
                )),
            route(RouteDef::on(Method::DELETE, "/endpoints/{id}", destroy))
                .name("webhooks.endpoints.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Delete a webhook endpoint", &[401, 403, 404], None, None)
                }),
            route(RouteDef::on(
                Method::POST,
                "/endpoints/{id}/rotate-secret",
                rotate,
            ))
            .name("webhooks.endpoints.rotate_secret")
            .api(api(
                "Rotate a webhook endpoint's secret",
                &[401, 403, 404],
                None,
                Some(schema_fn::<RotatedSecret>),
            )),
            route(RouteDef::on(Method::POST, "/endpoints/{id}/test", test))
                .name("webhooks.endpoints.test")
                .api(api(
                    "Send a webhook.test message to an endpoint",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<Attempt>),
                )),
            route(RouteDef::on(
                Method::GET,
                "/endpoints/{id}/deliveries",
                deliveries,
            ))
            .name("webhooks.endpoints.deliveries")
            .api(api(
                "List an endpoint's deliveries, newest first",
                &[401, 403, 404],
                None,
                Some(schema_fn::<Delivery>),
            )),
            route(RouteDef::on(Method::POST, "/deliveries/{id}/retry", retry))
                .name("webhooks.deliveries.retry")
                .api(api(
                    "Queue a failed delivery again",
                    &[401, 403, 404, 409],
                    None,
                    Some(schema_fn::<Delivery>),
                )),
        ]
    }
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_scope_are_native() {
        async fn handler(_: Scope) {}
        let _ = Endpoint::new(handler);
    }
}
