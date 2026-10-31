//! The SCIM routes: `{directory.scim.path}/Users`, `/Groups`,
//! `/ServiceProviderConfig`, `/ResourceTypes` and `/Schemas`.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use navree_http::{ApiMeta, Controller, IntoResponse, Path, Query, Response, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde_json::Value;

use super::filter::Filter;
use super::resources::{
    Projection, list_response, resource_types, schemas, service_provider_config,
};
use super::server::{ListQuery, Server, scim_error};
use super::{ScimError, scim_body};
use crate::Directory;
use crate::options::DirectoryOptions;
use crate::types::ConnectionStatus;

/// The SCIM 2.0 server, mounted at `directory.scim.path` (default
/// `/scim/v2`, outside the API prefix) when `directory.scim.enabled`.
///
/// Every request carries a SCIM connection's bearer token
/// (`Authorization: Bearer nvscim_...`), which picks the connection; the
/// discovery endpoints answer without one. Answers are
/// `application/scim+json`, errors in the SCIM error shape.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /ServiceProviderConfig` | what the server supports |
/// | `GET /ResourceTypes`, `GET /ResourceTypes/{id}` | `User` and `Group` |
/// | `GET /Schemas`, `GET /Schemas/{id}` | the User, Group and enterprise user schemas |
/// | `GET /Users?filter&startIndex&count&attributes&excludedAttributes` | a list response |
/// | `POST /Users` | 201, the user |
/// | `GET`, `PUT`, `PATCH /Users/{id}` | the user |
/// | `DELETE /Users/{id}` | 204 |
/// | `GET /Groups...`, `POST /Groups`, `GET`, `PUT`, `PATCH`, `DELETE /Groups/{id}` | the same for groups |
pub struct ScimController {
    directory: Arc<Directory>,
    options: Arc<DirectoryOptions>,
}

impl Injectable for ScimController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Directory>(),
            ProviderKey::of::<DirectoryOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(ScimController {
                directory: r.get::<Directory>().await?,
                options: r.get::<DirectoryOptions>().await?,
            })
        })
    }
}

type Shared = Arc<(Arc<Directory>, Arc<DirectoryOptions>)>;

/// The server of the connection the bearer token names.
async fn server(shared: &Shared, headers: &HeaderMap) -> Result<Server, ScimError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (scheme, token) = value.trim().split_once(' ')?;
            scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
        })
        .ok_or_else(ScimError::unauthorized)?;
    let directory = &shared.0;
    let row = directory
        .connection_by_token(token)
        .await
        .map_err(scim_error)?
        .ok_or_else(ScimError::unauthorized)?;
    if ConnectionStatus::parse(&row.status) == ConnectionStatus::Paused {
        return Err(ScimError::forbidden("The directory connection is paused"));
    }
    let rt = directory.rt().map_err(scim_error)?.clone();
    let connection = rt.info(&row);
    Ok(Server::new(rt, connection))
}

fn json_body(body: &Bytes) -> Result<Value, ScimError> {
    serde_json::from_slice(body)
        .map_err(|error| ScimError::invalid_syntax(format!("the body is not valid JSON: {error}")))
}

/// A query parameter, its name matched without regard to case.
fn param<'a>(params: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn projection(params: &HashMap<String, String>) -> Projection {
    Projection::new(
        param(params, "attributes"),
        param(params, "excludedAttributes"),
    )
}

fn list_query(
    params: &HashMap<String, String>,
    max_results: usize,
) -> Result<ListQuery, ScimError> {
    let filter = param(params, "filter")
        .filter(|filter| !filter.trim().is_empty())
        .map(Filter::parse)
        .transpose()?;
    let number = |name: &str| -> Result<Option<i64>, ScimError> {
        param(params, name)
            .map(|value| {
                value
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| ScimError::invalid_value(format!("{name} must be an integer")))
            })
            .transpose()
    };
    // startIndex below 1 is 1, a negative count 0 (RFC 7644 §3.4.2.4).
    let start_index = usize::try_from(number("startIndex")?.unwrap_or(1).max(1)).unwrap_or(1);
    let count = number("count")?
        .map_or(max_results, |count| {
            usize::try_from(count.max(0)).unwrap_or(0)
        })
        .min(max_results);
    Ok(ListQuery {
        filter,
        start_index,
        count,
        projection: projection(params),
    })
}

fn ok(value: &Value) -> Response {
    scim_body(value).into_response()
}

fn created(value: &Value) -> Response {
    let mut response = (StatusCode::CREATED, scim_body(value)).into_response();
    if let Some(location) = value.pointer("/meta/location").and_then(Value::as_str)
        && let Ok(location) = HeaderValue::from_str(location)
    {
        response.headers_mut().insert(header::LOCATION, location);
    }
    response
}

fn respond(result: Result<Response, ScimError>) -> Response {
    result.unwrap_or_else(IntoResponse::into_response)
}

fn api(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["SCIM".to_owned()],
        ..ApiMeta::default()
    }
}

impl Controller for ScimController {
    fn prefix(&self) -> &'static str {
        ""
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.scim.enabled {
            return Vec::new();
        }
        let base = format!("!{}", self.options.scim.path);
        let shared: Shared = Arc::new((self.directory.clone(), self.options.clone()));
        let max_results = self.options.scim.max_results;
        let path = |rest: &str| format!("{base}{rest}");

        let s = shared.clone();
        let config = move || {
            let s = s.clone();
            async move {
                let base = s.0.scim_url().unwrap_or_default();
                ok(&service_provider_config(&base, s.1.scim.max_results))
            }
        };
        let s = shared.clone();
        let types = move || {
            let s = s.clone();
            async move {
                let base = s.0.scim_url().unwrap_or_default();
                let types = resource_types(&base);
                ok(&list_response(types.len(), 1, types))
            }
        };
        let s = shared.clone();
        let one_type = move |Path(id): Path<String>| {
            let s = s.clone();
            async move {
                let base = s.0.scim_url().unwrap_or_default();
                respond(
                    resource_types(&base)
                        .into_iter()
                        .find(|t| t["id"].as_str() == Some(id.as_str()))
                        .map(|t| ok(&t))
                        .ok_or_else(|| {
                            ScimError::not_found(format!("Resource type {id} not found"))
                        }),
                )
            }
        };
        let s = shared.clone();
        let all_schemas = move || {
            let s = s.clone();
            async move {
                let base = s.0.scim_url().unwrap_or_default();
                let schemas = schemas(&base);
                ok(&list_response(schemas.len(), 1, schemas))
            }
        };
        let s = shared.clone();
        let one_schema = move |Path(id): Path<String>| {
            let s = s.clone();
            async move {
                let base = s.0.scim_url().unwrap_or_default();
                respond(
                    schemas(&base)
                        .into_iter()
                        .find(|schema| schema["id"].as_str() == Some(id.as_str()))
                        .map(|schema| ok(&schema))
                        .ok_or_else(|| ScimError::not_found(format!("Schema {id} not found"))),
                )
            }
        };

        // -- users --------------------------------------------------------------
        let s = shared.clone();
        let list_users =
            move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                let s = s.clone();
                async move {
                    respond(
                        async {
                            let server = server(&s, &headers).await?;
                            let query = list_query(&params, max_results)?;
                            Ok(ok(&server.list_users(&query).await?))
                        }
                        .await,
                    )
                }
            };
        let s = shared.clone();
        let create_user = move |headers: HeaderMap, body: Bytes| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        let (_, user) = server.create_user(&json_body(&body)?).await?;
                        Ok(created(&user))
                    }
                    .await,
                )
            }
        };
        let s = shared.clone();
        let show_user =
            move |headers: HeaderMap,
                  Path(id): Path<String>,
                  Query(params): Query<HashMap<String, String>>| {
                let s = s.clone();
                async move {
                    respond(
                        async {
                            let server = server(&s, &headers).await?;
                            Ok(ok(&server.get_user(&id, &projection(&params)).await?))
                        }
                        .await,
                    )
                }
            };
        let s = shared.clone();
        let replace_user = move |headers: HeaderMap, Path(id): Path<String>, body: Bytes| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        Ok(ok(&server.replace_user(&id, &json_body(&body)?).await?))
                    }
                    .await,
                )
            }
        };
        let s = shared.clone();
        let patch_user = move |headers: HeaderMap, Path(id): Path<String>, body: Bytes| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        Ok(ok(&server.patch_user(&id, &json_body(&body)?).await?))
                    }
                    .await,
                )
            }
        };
        let s = shared.clone();
        let delete_user = move |headers: HeaderMap, Path(id): Path<String>| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        server.delete_user(&id).await?;
                        Ok(StatusCode::NO_CONTENT.into_response())
                    }
                    .await,
                )
            }
        };

        // -- groups -------------------------------------------------------------
        let s = shared.clone();
        let list_groups =
            move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                let s = s.clone();
                async move {
                    respond(
                        async {
                            let server = server(&s, &headers).await?;
                            let query = list_query(&params, max_results)?;
                            Ok(ok(&server.list_groups(&query).await?))
                        }
                        .await,
                    )
                }
            };
        let s = shared.clone();
        let create_group = move |headers: HeaderMap, body: Bytes| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        let (_, group) = server.create_group(&json_body(&body)?).await?;
                        Ok(created(&group))
                    }
                    .await,
                )
            }
        };
        let s = shared.clone();
        let show_group =
            move |headers: HeaderMap,
                  Path(id): Path<String>,
                  Query(params): Query<HashMap<String, String>>| {
                let s = s.clone();
                async move {
                    respond(
                        async {
                            let server = server(&s, &headers).await?;
                            Ok(ok(&server.get_group(&id, &projection(&params)).await?))
                        }
                        .await,
                    )
                }
            };
        let s = shared.clone();
        let replace_group = move |headers: HeaderMap, Path(id): Path<String>, body: Bytes| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        Ok(ok(&server.replace_group(&id, &json_body(&body)?).await?))
                    }
                    .await,
                )
            }
        };
        let s = shared.clone();
        let patch_group = move |headers: HeaderMap,
                                Path(id): Path<String>,
                                Query(params): Query<HashMap<String, String>>,
                                body: Bytes| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        let body = json_body(&body)?;
                        Ok(ok(&server
                            .patch_group(&id, &body, &projection(&params))
                            .await?))
                    }
                    .await,
                )
            }
        };
        let s = shared;
        let delete_group = move |headers: HeaderMap, Path(id): Path<String>| {
            let s = s.clone();
            async move {
                respond(
                    async {
                        let server = server(&s, &headers).await?;
                        server.delete_group(&id).await?;
                        Ok(StatusCode::NO_CONTENT.into_response())
                    }
                    .await,
                )
            }
        };

        vec![
            RouteDef::on(Method::GET, path("/ServiceProviderConfig"), config)
                .public()
                .name("directory.scim.config")
                .api(api("SCIM service provider configuration")),
            RouteDef::on(Method::GET, path("/ResourceTypes"), types)
                .public()
                .name("directory.scim.resource_types")
                .api(api("SCIM resource types")),
            RouteDef::on(Method::GET, path("/ResourceTypes/{id}"), one_type)
                .public()
                .name("directory.scim.resource_type")
                .api(api("A SCIM resource type")),
            RouteDef::on(Method::GET, path("/Schemas"), all_schemas)
                .public()
                .name("directory.scim.schemas")
                .api(api("SCIM schemas")),
            RouteDef::on(Method::GET, path("/Schemas/{id}"), one_schema)
                .public()
                .name("directory.scim.schema")
                .api(api("A SCIM schema")),
            RouteDef::on(Method::GET, path("/Users"), list_users)
                .public()
                .name("directory.scim.users.index")
                .api(api("List or filter SCIM users")),
            RouteDef::on(Method::POST, path("/Users"), create_user)
                .public()
                .name("directory.scim.users.store")
                .api(api("Create a SCIM user")),
            RouteDef::on(Method::GET, path("/Users/{id}"), show_user)
                .public()
                .name("directory.scim.users.show")
                .api(api("Show a SCIM user")),
            RouteDef::on(Method::PUT, path("/Users/{id}"), replace_user)
                .public()
                .name("directory.scim.users.replace")
                .api(api("Replace a SCIM user")),
            RouteDef::on(Method::PATCH, path("/Users/{id}"), patch_user)
                .public()
                .name("directory.scim.users.patch")
                .api(api("Patch a SCIM user")),
            RouteDef::on(Method::DELETE, path("/Users/{id}"), delete_user)
                .public()
                .name("directory.scim.users.destroy")
                .api(api("Delete a SCIM user")),
            RouteDef::on(Method::GET, path("/Groups"), list_groups)
                .public()
                .name("directory.scim.groups.index")
                .api(api("List or filter SCIM groups")),
            RouteDef::on(Method::POST, path("/Groups"), create_group)
                .public()
                .name("directory.scim.groups.store")
                .api(api("Create a SCIM group")),
            RouteDef::on(Method::GET, path("/Groups/{id}"), show_group)
                .public()
                .name("directory.scim.groups.show")
                .api(api("Show a SCIM group")),
            RouteDef::on(Method::PUT, path("/Groups/{id}"), replace_group)
                .public()
                .name("directory.scim.groups.replace")
                .api(api("Replace a SCIM group")),
            RouteDef::on(Method::PATCH, path("/Groups/{id}"), patch_group)
                .public()
                .name("directory.scim.groups.patch")
                .api(api("Patch a SCIM group")),
            RouteDef::on(Method::DELETE, path("/Groups/{id}"), delete_group)
                .public()
                .name("directory.scim.groups.destroy")
                .api(api("Delete a SCIM group")),
        ]
    }
}
