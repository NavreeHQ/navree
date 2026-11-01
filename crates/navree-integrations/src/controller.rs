//! The routes of an integrations settings page: `/api/integrations/...`.

use std::collections::BTreeMap;
use std::sync::Arc;

use http::{Method, Uri};
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, Path, Query,
    RouteDef, SchemaFn, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_workspace::{InOrganization, InWorkspace};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::extract::IntegrationOwner;
use crate::provider::DEFAULT_SCOPE_SET;
use crate::{
    CompletedConnection, ConnectCallback, ConnectOptions, ConnectRedirect, ConnectionInfo, Health,
    Integrations, IntegrationsOptions, Level, NewApiKey, NewServiceAccount, ProviderInfo,
    ServiceAccountKey,
};

/// The permission every integrations route requires.
pub const MANAGE_PERMISSION: &str = "integrations.manage";

/// The query of `GET /api/integrations/{provider}/connect`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConnectQuery {
    /// The scope set to connect (default `default`).
    pub scope_set: Option<String>,
    /// A relative path to come back to.
    pub return_to: Option<String>,
}

/// The body of `POST /api/integrations/{provider}/service-account`.
#[derive(Clone, Deserialize, ToSchema)]
pub struct ServiceAccountInput {
    /// The key file: its JSON object, or the file's text.
    #[schema(value_type = Object)]
    pub key: Value,
    /// The user to act as (domain-wide delegation).
    pub subject: Option<String>,
    /// The scope set (default `default`).
    pub scope_set: Option<String>,
}

/// The body of `POST /api/integrations/{provider}/api-key`.
#[derive(Clone, Deserialize, ToSchema)]
pub struct ApiKeyInput {
    /// The key.
    pub key: String,
    /// This connection's API root, when it differs per customer.
    pub api_url: Option<String>,
    /// Extra fields kept with the key.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// The scope set (default `default`).
    pub scope_set: Option<String>,
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
struct DataList<T> {
    data: Vec<T>,
}

/// The routes of an integrations settings page, mounted under
/// `{http.api_prefix}/integrations` (default `/api/integrations`) when
/// `integrations.routes` is true. Every route requires the
/// `integrations.manage` permission; at `level = "workspace"` they act on
/// the request's workspace (the `InWorkspace` guard), at `organization` on
/// its organization (`InOrganization`), at `app` on the app's own
/// connections.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /integrations/providers` | `{data: [ProviderInfo]}` |
/// | `GET /integrations/connections` | `{data: [ConnectionInfo]}` (a workspace's own, then its organization's) |
/// | `GET /integrations/connections/{id}` | the connection |
/// | `POST /integrations/connections/{id}/check` | the [`Health`] after a check |
/// | `DELETE /integrations/connections/{id}` | 204 (the owner's own connections) |
/// | `GET /integrations/{provider}/connect?scope_set&return_to` | `{url, state}` |
/// | `GET /integrations/{provider}/callback?state&code&tenant&admin_consent&error` | 201, the connection and `return_to` |
/// | `POST /integrations/{provider}/service-account` `{key, subject?, scope_set?}` | 201, the connection |
/// | `POST /integrations/{provider}/api-key` `{key, api_url?, fields?, scope_set?}` | 201, the connection |
pub struct IntegrationsController {
    integrations: Arc<Integrations>,
    enabled: bool,
}

impl Injectable for IntegrationsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Integrations>(),
            ProviderKey::of::<IntegrationsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(IntegrationsController {
                integrations: r.get::<Integrations>().await?,
                enabled: r.get::<IntegrationsOptions>().await?.routes,
            })
        })
    }
}

fn api(
    summary: &str,
    responses: &[u16],
    request: Option<SchemaFn>,
    response: Option<SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Integrations".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}

fn created(uri: &Uri, connection: &ConnectionInfo) -> String {
    let path = uri.path();
    let prefix = path
        .rsplit_once("/integrations/")
        .map_or(path, |(prefix, _)| prefix);
    format!("{prefix}/integrations/connections/{}", connection.id)
}

impl Controller for IntegrationsController {
    fn prefix(&self) -> &'static str {
        "/integrations"
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let level = match self.integrations.level() {
            Ok(level) => level,
            Err(error) => {
                tracing::warn!(target: "navree::integrations", %error, "the integrations routes are not mounted");
                return Vec::new();
            }
        };

        let integrations = self.integrations.clone();
        let providers = move || {
            let integrations = integrations.clone();
            async move {
                let data = integrations.providers()?;
                Ok::<_, AppError>(Json(DataList::<ProviderInfo> { data }))
            }
        };
        let integrations = self.integrations.clone();
        let index = move |IntegrationOwner(owner): IntegrationOwner| {
            let integrations = integrations.clone();
            async move {
                let data = integrations.connections(owner).await?;
                Ok::<_, AppError>(Json(DataList::<ConnectionInfo> { data }))
            }
        };
        let integrations = self.integrations.clone();
        let show = move |IntegrationOwner(owner): IntegrationOwner, Path(id): Path<Uuid>| {
            let integrations = integrations.clone();
            async move { Ok::<_, AppError>(Json(integrations.find_for(owner, id).await?)) }
        };
        let integrations = self.integrations.clone();
        let check = move |IntegrationOwner(owner): IntegrationOwner, Path(id): Path<Uuid>| {
            let integrations = integrations.clone();
            async move {
                integrations.find_for(owner, id).await?;
                Ok::<_, AppError>(Json(integrations.check(id).await?))
            }
        };
        let integrations = self.integrations.clone();
        let destroy = move |IntegrationOwner(owner): IntegrationOwner, Path(id): Path<Uuid>| {
            let integrations = integrations.clone();
            async move {
                integrations.disconnect_for(owner, id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let integrations = self.integrations.clone();
        let connect = move |IntegrationOwner(owner): IntegrationOwner,
                            CurrentPrincipal(principal): CurrentPrincipal,
                            Path(provider): Path<String>,
                            Query(query): Query<ConnectQuery>| {
            let integrations = integrations.clone();
            async move {
                let options = ConnectOptions {
                    scope_set: query.scope_set,
                    return_to: query.return_to,
                    user_id: Some(principal.user_id),
                    ..ConnectOptions::default()
                };
                let redirect = integrations.connect_url(owner, &provider, options).await?;
                Ok::<_, AppError>(Json(redirect))
            }
        };
        let integrations = self.integrations.clone();
        let callback = move |IntegrationOwner(owner): IntegrationOwner,
                             CurrentPrincipal(principal): CurrentPrincipal,
                             Path(provider): Path<String>,
                             uri: Uri,
                             Query(callback): Query<ConnectCallback>| {
            let integrations = integrations.clone();
            async move {
                let completed = integrations
                    .complete(owner, &provider, &callback, Some(principal.user_id))
                    .await?;
                let location = created(&uri, &completed.connection);
                Ok::<_, AppError>(Created(location, completed))
            }
        };
        let integrations = self.integrations.clone();
        let service_account =
            move |IntegrationOwner(owner): IntegrationOwner,
                  CurrentPrincipal(principal): CurrentPrincipal,
                  Path(provider): Path<String>,
                  uri: Uri,
                  Json(input): Json<ServiceAccountInput>| {
                let integrations = integrations.clone();
                async move {
                    let key = match &input.key {
                        Value::String(text) => ServiceAccountKey::from_json(text)?,
                        other => ServiceAccountKey::from_value(other)?,
                    };
                    let new = NewServiceAccount {
                        scope_set: input
                            .scope_set
                            .unwrap_or_else(|| DEFAULT_SCOPE_SET.to_owned()),
                        key,
                        subject: input.subject,
                        connected_by: Some(principal.user_id),
                    };
                    let connection = integrations
                        .connect_service_account(owner, &provider, new)
                        .await?;
                    Ok::<_, AppError>(Created(created(&uri, &connection), connection))
                }
            };
        let integrations = self.integrations.clone();
        let api_key = move |IntegrationOwner(owner): IntegrationOwner,
                            CurrentPrincipal(principal): CurrentPrincipal,
                            Path(provider): Path<String>,
                            uri: Uri,
                            Json(input): Json<ApiKeyInput>| {
            let integrations = integrations.clone();
            async move {
                let new = NewApiKey {
                    scope_set: input
                        .scope_set
                        .unwrap_or_else(|| DEFAULT_SCOPE_SET.to_owned()),
                    key: input.key,
                    api_url: input.api_url,
                    fields: input.fields,
                    connected_by: Some(principal.user_id),
                };
                let connection = integrations.connect_api_key(owner, &provider, new).await?;
                Ok::<_, AppError>(Created(created(&uri, &connection), connection))
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
            route(RouteDef::on(Method::GET, "/providers", providers))
                .name("integrations.providers")
                .api(api(
                    "List the configured integration providers",
                    &[401, 403],
                    None,
                    Some(schema_fn::<ProviderInfo>),
                )),
            route(RouteDef::on(Method::GET, "/connections", index))
                .name("integrations.connections.index")
                .api(api(
                    "List the connections",
                    &[401, 403],
                    None,
                    Some(schema_fn::<ConnectionInfo>),
                )),
            route(RouteDef::on(Method::GET, "/connections/{id}", show))
                .name("integrations.connections.show")
                .api(api(
                    "Show a connection",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<ConnectionInfo>),
                )),
            route(RouteDef::on(Method::POST, "/connections/{id}/check", check))
                .name("integrations.connections.check")
                .api(api(
                    "Check a connection now",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<Health>),
                )),
            route(RouteDef::on(Method::DELETE, "/connections/{id}", destroy))
                .name("integrations.connections.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Disconnect", &[401, 403, 404], None, None)
                }),
            route(RouteDef::on(Method::GET, "/{provider}/connect", connect))
                .name("integrations.connect")
                .api(api(
                    "Start a connect flow: the provider's consent URL and the sealed state",
                    &[400, 401, 403, 404],
                    None,
                    Some(schema_fn::<ConnectRedirect>),
                )),
            route(RouteDef::on(Method::GET, "/{provider}/callback", callback))
                .name("integrations.callback")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Finish a connect flow with the provider's callback parameters",
                        &[400, 401, 403, 404, 502],
                        None,
                        Some(schema_fn::<CompletedConnection>),
                    )
                }),
            route(RouteDef::on(
                Method::POST,
                "/{provider}/service-account",
                service_account,
            ))
            .name("integrations.service_account")
            .api(ApiMeta {
                success_status: 201,
                ..api(
                    "Connect with a service-account key",
                    &[400, 401, 403, 404, 502],
                    Some(schema_fn::<ServiceAccountInput>),
                    Some(schema_fn::<ConnectionInfo>),
                )
            }),
            route(RouteDef::on(Method::POST, "/{provider}/api-key", api_key))
                .name("integrations.api_key")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Connect with an API key",
                        &[400, 401, 403, 404, 502],
                        Some(schema_fn::<ApiKeyInput>),
                        Some(schema_fn::<ConnectionInfo>),
                    )
                }),
        ]
    }
}
