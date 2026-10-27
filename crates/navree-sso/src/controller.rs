//! The routes under `{api_prefix}{sso.routes_prefix}` (default `/api/sso`).

use std::sync::Arc;

use axum::Form;
use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use http::header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE, LOCATION};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree_auth::ClientMeta;
use navree_db::Uow;
use navree_http::{ApiMeta, AppError, Controller, Json, NoContent, RouteDef, schema_fn};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    ConnectionUpdate, NewConnection, OidcCallback, Sso, SsoConnection, SsoDiscovery, SsoDomain,
    SsoError, SsoOptions, SsoOwner, SsoRedirect, SsoSignIn,
};

/// The permission the management routes need.
pub const MANAGE_PERMISSION: &str = "sso.manage";

/// `POST /sso/discover`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct DiscoverInput {
    /// The email address typed on the sign-in page.
    pub email: String,
}

/// `POST /sso/exchange`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ExchangeInput {
    /// The one-time code from the `complete_url` redirect.
    pub code: String,
}

/// `POST /sso/connections/{id}/domains`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddDomainInput {
    /// The domain (`acme.com`).
    pub domain: String,
}

/// The form an identity provider posts to the assertion consumer service.
#[derive(Debug, Deserialize)]
struct AcsForm {
    #[serde(rename = "SAMLResponse")]
    saml_response: String,
    #[serde(rename = "RelayState", default)]
    relay_state: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct LoginQuery {
    return_to: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OwnerQuery {
    owner_type: Option<String>,
    owner_id: Option<Uuid>,
}

/// The single sign-on routes.
pub(crate) struct SsoController {
    sso: Arc<Sso>,
    options: Arc<SsoOptions>,
}

impl Injectable for SsoController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Sso>(), ProviderKey::of::<SsoOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SsoController {
                sso: r.get::<Sso>().await?,
                options: r.get::<SsoOptions>().await?,
            })
        })
    }
}

fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Single sign-on".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

fn sso_error(error: SsoError) -> AppError {
    error.into_app_error()
}

fn redirect(status: StatusCode, url: &str) -> Response {
    match HeaderValue::from_str(url) {
        Ok(location) => (
            status,
            [(LOCATION, location), (CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        )
            .into_response(),
        Err(_) => AppError::internal("an invalid redirect URL").into_response(),
    }
}

/// The answer of the assertion consumer service and the callback: a
/// redirect to the front end's `complete_url` (with a one-time code or an
/// error), or the login JSON (or the error) when none is configured.
async fn finish(sso: &Sso, result: Result<SsoSignIn, SsoError>, meta: &ClientMeta) -> Response {
    let has_complete_url = sso.options().complete_url.is_some();
    let outcome = match result {
        Ok(sign_in) if has_complete_url => sso
            .issue_code(&sign_in)
            .await
            .map(|code| redirect(StatusCode::SEE_OTHER, &sso.complete_url(&[("code", &code)]).unwrap_or_default())),
        Ok(sign_in) => sso
            .tokens_for(&sign_in, meta)
            .await
            .map(|body| Json(body).into_response()),
        Err(error) => Err(error),
    };
    match outcome {
        Ok(response) => response,
        Err(error) if has_complete_url => {
            let app_error = error.into_app_error();
            if app_error.status.is_server_error() {
                tracing::error!(target: "navree::sso", error = %app_error, "single sign-on failed");
            }
            let message = app_error
                .detail
                .clone()
                .unwrap_or_else(|| app_error.title.clone());
            let url = sso
                .complete_url(&[("error", &app_error.code), ("message", &message)])
                .unwrap_or_default();
            redirect(StatusCode::SEE_OTHER, &url)
        }
        Err(error) => sso_error(error).into_response(),
    }
}

fn owner_filter(query: &OwnerQuery) -> Result<Option<SsoOwner>, AppError> {
    match query.owner_type.as_deref() {
        None => Ok(None),
        Some(kind) => SsoOwner::from_parts(kind, query.owner_id)
            .map(Some)
            .ok_or_else(|| {
                sso_error(SsoError::invalid(
                    "owner_type",
                    "owner_type must be app, organization or workspace (with owner_id)",
                ))
            }),
    }
}

impl Controller for SsoController {
    fn prefix(&self) -> &'static str {
        ""
    }

    #[allow(clippy::too_many_lines)]
    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.routes {
            return Vec::new();
        }
        let prefix = self.options.routes_prefix.trim_end_matches('/').to_owned();
        let path = |p: &str| format!("{prefix}{p}");
        let mut routes = Vec::new();

        // -- sign-in ---------------------------------------------------------

        let sso = self.sso.clone();
        let discover = move |Json(input): Json<DiscoverInput>| {
            let sso = sso.clone();
            async move {
                let discovery = sso.discover(&input.email).await.map_err(sso_error)?;
                Ok::<_, AppError>(Json(discovery))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/discover"), discover)
                .public()
                .name("sso.discover")
                .api(ApiMeta {
                    description: Some(
                        "Whether the email's domain signs in with single sign-on, where, and \
                         whether a password is still allowed."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<DiscoverInput>),
                    response: Some(schema_fn::<SsoDiscovery>),
                    ..api("Find the single sign-on connection of an email", &[400])
                }),
        );

        let sso = self.sso.clone();
        let login = move |Path(connection): Path<String>,
                          Query(query): Query<LoginQuery>,
                          headers: HeaderMap| {
            let sso = sso.clone();
            async move {
                let redirect_to = sso
                    .login(&connection, query.return_to)
                    .await
                    .map_err(sso_error)?;
                let wants_json = headers
                    .get(ACCEPT)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|accept| accept.contains("application/json"));
                if wants_json {
                    return Ok::<_, AppError>(Json(redirect_to).into_response());
                }
                Ok(redirect(StatusCode::FOUND, &redirect_to.url))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/{connection}/login"), login)
                .public()
                .name("sso.login")
                .api(ApiMeta {
                    description: Some(
                        "Redirects (302) to the identity provider; with `Accept: \
                         application/json`, answers `{url}` instead. `?return_to=/path` (a \
                         relative path) comes back with the sign-in."
                            .to_owned(),
                    ),
                    response: Some(schema_fn::<SsoRedirect>),
                    ..api("Start a single sign-on", &[400, 403, 404, 502])
                }),
        );

        let sso = self.sso.clone();
        let acs = move |Path(connection): Path<String>, meta: ClientMeta, Form(form): Form<AcsForm>| {
            let sso = sso.clone();
            async move {
                let result = sso
                    .complete_saml(
                        &connection,
                        &form.saml_response,
                        form.relay_state.as_deref(),
                        &meta,
                    )
                    .await;
                finish(&sso, result, &meta).await
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/saml/{connection}/acs"), acs)
                .public()
                .name("sso.saml.acs")
                .api(ApiMeta {
                    description: Some(
                        "The identity provider posts `SAMLResponse` and `RelayState` (form). \
                         Redirects (303) to `sso.complete_url` with `?code=` or `?error=`; \
                         without one, answers like `POST /auth/login`, plus `return_to`."
                            .to_owned(),
                    ),
                    response: Some(schema_fn::<navree_auth::LoginResponse>),
                    ..api("SAML assertion consumer service", &[400, 403, 404, 409, 502])
                }),
        );

        let sso = self.sso.clone();
        let metadata = move |Path(connection): Path<String>| {
            let sso = sso.clone();
            async move {
                let xml = sso.saml_metadata(&connection).await.map_err(sso_error)?;
                Ok::<_, AppError>(
                    (
                        [(CONTENT_TYPE, HeaderValue::from_static("application/samlmetadata+xml"))],
                        xml,
                    )
                        .into_response(),
                )
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/saml/{connection}/metadata"), metadata)
                .public()
                .name("sso.saml.metadata")
                .api(api("The service provider metadata of a SAML connection", &[404])),
        );

        let sso = self.sso.clone();
        let callback = move |Path(connection): Path<String>,
                             meta: ClientMeta,
                             Query(callback): Query<OidcCallback>| {
            let sso = sso.clone();
            async move {
                let result = sso.complete_oidc(&connection, &callback, &meta).await;
                finish(&sso, result, &meta).await
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/oidc/{connection}/callback"), callback)
                .public()
                .name("sso.oidc.callback")
                .api(ApiMeta {
                    description: Some(
                        "The provider redirects here with `code` and `state`. Answers like \
                         the assertion consumer service."
                            .to_owned(),
                    ),
                    response: Some(schema_fn::<navree_auth::LoginResponse>),
                    ..api("OpenID Connect callback", &[400, 403, 404, 409, 502])
                }),
        );

        let sso = self.sso.clone();
        let exchange = move |meta: ClientMeta, Json(input): Json<ExchangeInput>| {
            let sso = sso.clone();
            async move {
                let (user, tokens, return_to) =
                    sso.exchange(&input.code, &meta).await.map_err(sso_error)?;
                let body: Value = sso
                    .login_body(&user, &tokens, return_to)
                    .await
                    .map_err(sso_error)?;
                Ok::<_, AppError>(Json(body))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/exchange"), exchange)
                .public()
                .name("sso.exchange")
                .api(ApiMeta {
                    description: Some(
                        "Exchanges the one-time code of a finished sign-in (once, within \
                         `sso.code_ttl_secs`) for the login answer: the user and tokens, plus \
                         `return_to`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<ExchangeInput>),
                    response: Some(schema_fn::<navree_auth::LoginResponse>),
                    ..api("Finish a single sign-on", &[400])
                }),
        );

        // -- management ------------------------------------------------------

        let manage = [MANAGE_PERMISSION];

        let sso = self.sso.clone();
        let list = move |Query(query): Query<OwnerQuery>| {
            let sso = sso.clone();
            async move {
                let owner = owner_filter(&query)?;
                let connections = sso.connections(owner).await.map_err(sso_error)?;
                Ok::<_, AppError>(Json(connections))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/connections"), list)
                .permissions(&manage)
                .name("sso.connections.index")
                .api(ApiMeta {
                    description: Some("`?owner_type=workspace&owner_id=...` filters.".to_owned()),
                    response: Some(schema_fn::<Vec<SsoConnection>>),
                    ..api("List connections", &[400, 401, 403])
                }),
        );

        let sso = self.sso.clone();
        let create = move |uow: Uow, Json(input): Json<NewConnection>| {
            let sso = sso.clone();
            async move {
                let connection = sso
                    .create_connection(&uow, input)
                    .await
                    .map_err(sso_error)?;
                Ok::<_, AppError>((StatusCode::CREATED, Json(connection)).into_response())
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/connections"), create)
                .permissions(&manage)
                .name("sso.connections.store")
                .api(ApiMeta {
                    success_status: 201,
                    request: Some(schema_fn::<NewConnection>),
                    response: Some(schema_fn::<SsoConnection>),
                    ..api("Create a connection", &[400, 401, 403])
                }),
        );

        let sso = self.sso.clone();
        let show = move |Path(id): Path<String>| {
            let sso = sso.clone();
            async move {
                let connection = sso.connection(&id).await.map_err(sso_error)?;
                Ok::<_, AppError>(Json(connection))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/connections/{id}"), show)
                .permissions(&manage)
                .name("sso.connections.show")
                .api(ApiMeta {
                    response: Some(schema_fn::<SsoConnection>),
                    ..api("Show a connection", &[401, 403, 404])
                }),
        );

        let sso = self.sso.clone();
        let update = move |Path(id): Path<String>, uow: Uow, Json(input): Json<ConnectionUpdate>| {
            let sso = sso.clone();
            async move {
                let connection = sso
                    .update_connection(&uow, &id, input)
                    .await
                    .map_err(sso_error)?;
                Ok::<_, AppError>(Json(connection))
            }
        };
        routes.push(
            RouteDef::on(Method::PATCH, path("/connections/{id}"), update)
                .permissions(&manage)
                .name("sso.connections.update")
                .api(ApiMeta {
                    request: Some(schema_fn::<ConnectionUpdate>),
                    response: Some(schema_fn::<SsoConnection>),
                    ..api("Change a connection", &[400, 401, 403, 404])
                }),
        );

        let sso = self.sso.clone();
        let destroy = move |Path(id): Path<String>, uow: Uow| {
            let sso = sso.clone();
            async move {
                sso.delete_connection(&uow, &id).await.map_err(sso_error)?;
                Ok::<_, AppError>(NoContent)
            }
        };
        routes.push(
            RouteDef::on(Method::DELETE, path("/connections/{id}"), destroy)
                .permissions(&manage)
                .name("sso.connections.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Delete a connection", &[401, 403, 404])
                }),
        );

        let sso = self.sso.clone();
        let domains = move |Path(id): Path<String>| {
            let sso = sso.clone();
            async move {
                let domains = sso.domains(&id).await.map_err(sso_error)?;
                Ok::<_, AppError>(Json(domains))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/connections/{id}/domains"), domains)
                .permissions(&manage)
                .name("sso.domains.index")
                .api(ApiMeta {
                    response: Some(schema_fn::<Vec<SsoDomain>>),
                    ..api("List a connection's domains", &[401, 403, 404])
                }),
        );

        let sso = self.sso.clone();
        let add_domain = move |Path(id): Path<String>, uow: Uow, Json(input): Json<AddDomainInput>| {
            let sso = sso.clone();
            async move {
                let domain = sso
                    .add_domain(&uow, &id, &input.domain)
                    .await
                    .map_err(sso_error)?;
                Ok::<_, AppError>((StatusCode::CREATED, Json(domain)).into_response())
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/connections/{id}/domains"), add_domain)
                .permissions(&manage)
                .name("sso.domains.store")
                .api(ApiMeta {
                    success_status: 201,
                    description: Some(
                        "Answers the TXT record to create (`txt_name`, `txt_value`); verify \
                         it with `POST /sso/domains/{id}/verify`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<AddDomainInput>),
                    response: Some(schema_fn::<SsoDomain>),
                    ..api("Add an email domain", &[400, 401, 403, 404])
                }),
        );

        let sso = self.sso.clone();
        let verify = move |Path(id): Path<Uuid>, uow: Uow| {
            let sso = sso.clone();
            async move {
                let domain = sso.verify_domain(&uow, id).await.map_err(sso_error)?;
                Ok::<_, AppError>(Json(domain))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/domains/{id}/verify"), verify)
                .permissions(&manage)
                .name("sso.domains.verify")
                .api(ApiMeta {
                    response: Some(schema_fn::<SsoDomain>),
                    ..api("Verify a domain's TXT record", &[400, 401, 403, 404, 409, 502])
                }),
        );

        let sso = self.sso.clone();
        let remove = move |Path(id): Path<Uuid>, uow: Uow| {
            let sso = sso.clone();
            async move {
                sso.remove_domain(&uow, id).await.map_err(sso_error)?;
                Ok::<_, AppError>(NoContent)
            }
        };
        routes.push(
            RouteDef::on(Method::DELETE, path("/domains/{id}"), remove)
                .permissions(&manage)
                .name("sso.domains.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Remove a domain", &[401, 403, 404])
                }),
        );

        routes
    }
}

/// Responses carry the error mapping.
impl IntoResponse for SsoError {
    fn into_response(self) -> Response {
        self.into_app_error().into_response()
    }
}
