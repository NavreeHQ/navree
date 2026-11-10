//! The routes: a workspace's domain settings (`/api/domains/...`), the ask
//! endpoint of an on-demand TLS proxy, and ACME challenge answers.

use std::sync::Arc;

use http::{Method, StatusCode, Uri, header};
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, IntoResponse, Json, NoContent, Path,
    Query, RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_workspace::{CurrentWorkspace, InWorkspace};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::options::{DomainsOptions, TlsMode};
use crate::types::{CustomDomain, NewDomain};
use crate::{Domains, DomainsError};

/// The permission every `/api/domains` route requires.
pub const MANAGE_PERMISSION: &str = "domains.manage";

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
struct DataList<T> {
    data: Vec<T>,
}

fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Domains".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

/// `{prefix}/domains/{id}` from the request's path.
fn location(uri: &Uri, id: Uuid) -> String {
    let path = uri.path().trim_end_matches('/');
    format!("{path}/{id}")
}

/// The routes of a workspace's domain settings page, mounted under
/// `{http.api_prefix}/domains` (default `/api/domains`) when
/// `domains.routes` is true. Every route acts on the request's workspace
/// (the `InWorkspace` guard) and requires the `domains.manage`
/// permission.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /domains` | `{data: [CustomDomain]}` |
/// | `POST /domains` `{host, purpose?}` | 201 `CustomDomain` with its `dns_records` |
/// | `GET /domains/{id}` | `CustomDomain` |
/// | `POST /domains/{id}/verify` | `CustomDomain` after a DNS check |
/// | `POST /domains/{id}/primary` | `CustomDomain`, now the primary domain |
/// | `DELETE /domains/{id}` | 204 |
pub struct DomainsController {
    domains: Arc<Domains>,
    enabled: bool,
}

impl Injectable for DomainsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Domains>(),
            ProviderKey::of::<DomainsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(DomainsController {
                domains: r.get::<Domains>().await?,
                enabled: r.get::<DomainsOptions>().await?.routes,
            })
        })
    }
}

impl Controller for DomainsController {
    fn prefix(&self) -> &'static str {
        "/domains"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let d = self.domains.clone();
        let index = move |workspace: CurrentWorkspace| {
            let d = d.clone();
            async move {
                let data = d.list(workspace.id()).await?;
                Ok::<_, AppError>(Json(DataList::<CustomDomain> { data }))
            }
        };
        let d = self.domains.clone();
        let store = move |workspace: CurrentWorkspace,
                          CurrentPrincipal(principal): CurrentPrincipal,
                          uri: Uri,
                          Json(input): Json<NewDomain>| {
            let d = d.clone();
            async move {
                let input = NewDomain {
                    created_by: Some(principal.user_id),
                    ..input
                };
                let domain = d.add(workspace.id(), input).await?;
                Ok::<_, AppError>(Created(location(&uri, domain.id), domain))
            }
        };
        let d = self.domains.clone();
        let show = move |workspace: CurrentWorkspace, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move { Ok::<_, AppError>(Json(d.get_for(workspace.id(), id).await?)) }
        };
        let d = self.domains.clone();
        let verify = move |workspace: CurrentWorkspace, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                d.get_for(workspace.id(), id).await?;
                Ok::<_, AppError>(Json(d.verify(id).await?))
            }
        };
        let d = self.domains.clone();
        let primary = move |workspace: CurrentWorkspace, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                d.get_for(workspace.id(), id).await?;
                Ok::<_, AppError>(Json(d.make_primary(id).await?))
            }
        };
        let d = self.domains.clone();
        let destroy = move |workspace: CurrentWorkspace, Path(id): Path<Uuid>| {
            let d = d.clone();
            async move {
                d.get_for(workspace.id(), id).await?;
                d.remove(id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let route = |route: RouteDef| {
            route
                .permissions(&[MANAGE_PERMISSION])
                .guard(InWorkspace::guard())
        };
        vec![
            route(RouteDef::on(Method::GET, "", index))
                .name("domains.index")
                .api(ApiMeta {
                    response: Some(schema_fn::<CustomDomain>),
                    ..api("List the workspace's domains", &[400, 401, 403, 404])
                }),
            route(RouteDef::on(Method::POST, "", store))
                .name("domains.store")
                .api(ApiMeta {
                    success_status: 201,
                    request: Some(schema_fn::<NewDomain>),
                    response: Some(schema_fn::<CustomDomain>),
                    ..api("Add a domain", &[400, 401, 403, 404, 409, 502])
                }),
            route(RouteDef::on(Method::GET, "/{id}", show))
                .name("domains.show")
                .api(ApiMeta {
                    response: Some(schema_fn::<CustomDomain>),
                    ..api("Show a domain", &[401, 403, 404])
                }),
            route(RouteDef::on(Method::POST, "/{id}/verify", verify))
                .name("domains.verify")
                .api(ApiMeta {
                    response: Some(schema_fn::<CustomDomain>),
                    ..api("Check a domain's DNS now", &[401, 403, 404, 502])
                }),
            route(RouteDef::on(Method::POST, "/{id}/primary", primary))
                .name("domains.primary")
                .api(ApiMeta {
                    response: Some(schema_fn::<CustomDomain>),
                    ..api(
                        "Make a domain the workspace's primary domain",
                        &[401, 403, 404, 409],
                    )
                }),
            route(RouteDef::on(Method::DELETE, "/{id}", destroy))
                .name("domains.destroy")
                .api(api("Remove a domain", &[401, 403, 404])),
        ]
    }
}

/// The query of the ask endpoint: `?domain=crm.acme-corp.com`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AskQuery {
    /// The domain the proxy is about to get a certificate for.
    pub domain: Option<String>,
}

/// The public routes, outside the API prefix:
///
/// - with `domains.tls = "on_demand"`, `GET {domains.ask_path}?domain=`
///   (default `/_domains/allow`): 200 for a verified custom domain, 404
///   otherwise (400 without `domain`). An on-demand TLS proxy (Caddy's
///   `on_demand_tls ask`) calls it before it gets a certificate;
/// - with `domains.tls = "acme"`,
///   `GET /.well-known/acme-challenge/{token}`: the answer to an open
///   HTTP-01 challenge, as `text/plain`.
pub struct DomainsPublicController {
    domains: Arc<Domains>,
    options: Arc<DomainsOptions>,
}

impl Injectable for DomainsPublicController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Domains>(),
            ProviderKey::of::<DomainsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(DomainsPublicController {
                domains: r.get::<Domains>().await?,
                options: r.get::<DomainsOptions>().await?,
            })
        })
    }
}

impl Controller for DomainsPublicController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        match self.options.tls {
            TlsMode::OnDemand => {
                let d = self.domains.clone();
                let ask = move |Query(query): Query<AskQuery>| {
                    let d = d.clone();
                    async move {
                        let host = query
                            .domain
                            .filter(|host| !host.trim().is_empty())
                            .ok_or_else(|| {
                                AppError::bad_request("Give the domain as `?domain=`")
                                    .with_code("invalid_domain")
                            })?;
                        match d.resolve(&host).await? {
                            Some(domain) => Ok::<_, AppError>(
                                Json(serde_json::json!({ "domain": domain.host })).into_response(),
                            ),
                            None => Err(DomainsError::DomainNotFound.into_app_error()),
                        }
                    }
                };
                vec![
                    RouteDef::on(Method::GET, format!("!{}", self.options.ask_path), ask)
                        .public()
                        .name("domains.ask")
                        .api(api(
                            "Whether a TLS proxy may get a certificate for a domain",
                            &[400, 404],
                        )),
                ]
            }
            TlsMode::Acme => {
                let d = self.domains.clone();
                let answer = move |Path(token): Path<String>| {
                    let d = d.clone();
                    async move {
                        match d.challenge_answer(&token).await? {
                            Some(answer) => Ok::<_, AppError>(
                                (
                                    StatusCode::OK,
                                    [(header::CONTENT_TYPE, "text/plain")],
                                    answer,
                                )
                                    .into_response(),
                            ),
                            None => Err(AppError::not_found()),
                        }
                    }
                };
                vec![
                    RouteDef::on(Method::GET, "!/.well-known/acme-challenge/{token}", answer)
                        .public()
                        .name("domains.acme_challenge")
                        .api(api("The answer to an ACME HTTP-01 challenge", &[404])),
                ]
            }
            TlsMode::Off => Vec::new(),
        }
    }
}
