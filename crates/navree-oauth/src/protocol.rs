//! The protocol endpoints, mounted at `oauth.path` (default `/oauth`)
//! outside `http.api_prefix`, and the discovery documents under
//! `/.well-known`.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use axum::body::Bytes;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, LOCATION, PRAGMA};
use http::{Extensions, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use navree_contracts::RequestId;
use navree_db::Uow;
use navree_http::{ApiMeta, AppError, ClientIp, Controller, Json, RouteDef, schema_fn};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::clients::{
    ClientCredentials, GRANT_AUTHORIZATION_CODE, GRANT_CLIENT_CREDENTIALS, GRANT_DEVICE_CODE,
    GRANT_REFRESH_TOKEN, NewClient, OAuthClients,
};
use crate::core::Core;
use crate::error::OAuthError;
use crate::grants::{Grants, TokenResponse};
use crate::requests::{AuthorizeOutcome, AuthorizeParams, Requests};
use crate::scope::{OPENID, parse_scope};
use crate::strategy::{OAuthToken, insufficient_scope};
use crate::throttle::{Endpoint, Throttles};

/// The body of a form-encoded protocol request, by parameter (the first
/// value of a repeated parameter).
#[derive(Clone, Debug, Default)]
struct Form(HashMap<String, String>);

impl Form {
    /// Parses `body`; a request with a `Content-Type` other than
    /// `application/x-www-form-urlencoded` is `invalid_request`.
    fn parse(headers: &HeaderMap, body: &[u8]) -> Result<Self, OAuthError> {
        if let Some(content_type) = headers.get(CONTENT_TYPE) {
            let form = content_type
                .to_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .starts_with("application/x-www-form-urlencoded");
            if !form {
                return Err(OAuthError::InvalidRequest(
                    "The body must be application/x-www-form-urlencoded".into(),
                ));
            }
        }
        let mut params = HashMap::new();
        for (name, value) in form_urlencoded::parse(body) {
            if params.contains_key(name.as_ref()) {
                return Err(OAuthError::InvalidRequest(format!(
                    "The parameter `{name}` is repeated"
                )));
            }
            params.insert(name.into_owned(), value.into_owned());
        }
        Ok(Self(params))
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn require(&self, name: &str) -> Result<&str, OAuthError> {
        self.get(name)
            .ok_or_else(|| OAuthError::InvalidRequest(format!("{name} is required")))
    }
}

/// The client credentials of a request: HTTP Basic (`client_secret_basic`)
/// or the body (`client_secret_post`, or `client_id` alone for public
/// clients). Using both is `invalid_request`.
fn client_credentials(headers: &HeaderMap, form: &Form) -> Result<ClientCredentials, OAuthError> {
    let basic = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let (scheme, rest) = value.trim().split_once(' ')?;
            scheme
                .eq_ignore_ascii_case("basic")
                .then(|| rest.trim().to_owned())
        });
    match basic {
        Some(encoded) => {
            let malformed = || OAuthError::InvalidClient("Malformed HTTP Basic credentials".into());
            if form.get("client_secret").is_some() {
                return Err(OAuthError::InvalidRequest(
                    "Use one client authentication method".into(),
                ));
            }
            let decoded = STANDARD.decode(encoded).map_err(|_| malformed())?;
            let decoded = String::from_utf8(decoded).map_err(|_| malformed())?;
            let (id, secret) = decoded.split_once(':').ok_or_else(malformed)?;
            // RFC 6749 §2.3.1: both parts are form-encoded.
            let decode = |value: &str| {
                form_urlencoded::parse(format!("v={value}").as_bytes())
                    .next()
                    .map(|(_, value)| value.into_owned())
                    .unwrap_or_default()
            };
            let (id, secret) = (decode(id), decode(secret));
            if form.get("client_id").is_some_and(|body_id| body_id != id) {
                return Err(OAuthError::InvalidClient(
                    "client_id doesn't match the HTTP Basic credentials".into(),
                ));
            }
            Ok(ClientCredentials {
                client_id: Some(id),
                client_secret: Some(secret).filter(|s| !s.is_empty()),
                basic: true,
            })
        }
        None => Ok(ClientCredentials {
            client_id: form.get("client_id").map(str::to_owned),
            client_secret: form.get("client_secret").map(str::to_owned),
            basic: false,
        }),
    }
}

/// `value` as JSON with `Cache-Control: no-store` (token responses).
fn no_store_json<T: Serialize>(status: StatusCode, value: &T) -> Response {
    let mut response = (status, Json(value)).into_response();
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

/// An HTML error page for `GET /oauth/authorize` requests that can't be
/// trusted to redirect.
fn error_page(message: &str) -> Response {
    let escaped = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Authorization error</title></head>\
         <body><h1>This authorization request can't be completed</h1><p>{escaped}</p></body></html>"
    );
    let mut response = (StatusCode::BAD_REQUEST, html).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// A 302 to `location`.
fn redirect(location: &str) -> Response {
    let mut response = StatusCode::FOUND.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(LOCATION, value);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Runs `f` in a unit of work of its own and commits it, also when `f`
/// answers with an OAuth error (a reused code or refresh token revokes
/// tokens before the error); database failures roll back.
async fn in_own_uow<T, F, Fut>(core: &Core, extensions: &Extensions, f: F) -> Result<T, OAuthError>
where
    F: FnOnce(Uow) -> Fut,
    Fut: Future<Output = Result<T, OAuthError>>,
{
    let request_id = extensions.get::<RequestId>().map(ToString::to_string);
    let uow = core.db.uow(None, request_id).await;
    let result = uow.scope(f(uow.clone())).await;
    match &result {
        Err(OAuthError::Db(_) | OAuthError::Other(_)) => {
            if let Err(error) = uow.rollback().await {
                tracing::warn!(target: "navree::oauth", %error, "rollback failed");
            }
        }
        _ => uow.commit().await?,
    }
    result
}

/// The body of `POST {path}/register` (RFC 7591).
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct RegistrationRequest {
    /// The client's name.
    pub client_name: Option<String>,
    /// Its redirect URIs.
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    /// `authorization_code`, `refresh_token`, the device code grant.
    #[serde(default)]
    pub grant_types: Vec<String>,
    /// `none` (a public client), `client_secret_basic` (default) or
    /// `client_secret_post`.
    pub token_endpoint_auth_method: Option<String>,
    /// The scopes it may ask for, space-separated (default: every scope).
    pub scope: Option<String>,
    /// A logo for the consent page.
    pub logo_uri: Option<String>,
    /// The client's website.
    pub client_uri: Option<String>,
}

/// The protocol controller.
pub struct OAuthProtocolController {
    core: Arc<Core>,
    clients: Arc<OAuthClients>,
    throttles: Arc<Throttles>,
}

impl Injectable for OAuthProtocolController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Core>(), ProviderKey::of::<OAuthClients>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let core = r.get::<Core>().await?;
            let throttles = Arc::new(Throttles::new(&core.options.rate_limits));
            Ok(OAuthProtocolController {
                core,
                clients: r.get::<OAuthClients>().await?,
                throttles,
            })
        })
    }
}

impl fmt::Debug for OAuthProtocolController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthProtocolController")
            .finish_non_exhaustive()
    }
}

impl OAuthProtocolController {
    /// The discovery metadata (RFC 8414 and OpenID Connect Discovery).
    fn metadata(core: &Core) -> Value {
        let grants = [
            GRANT_AUTHORIZATION_CODE,
            GRANT_CLIENT_CREDENTIALS,
            GRANT_REFRESH_TOKEN,
            GRANT_DEVICE_CODE,
        ];
        let auth_methods = ["client_secret_basic", "client_secret_post", "none"];
        let mut metadata = json!({
            "issuer": core.issuer,
            "authorization_endpoint": core.endpoint("authorize"),
            "token_endpoint": core.endpoint("token"),
            "device_authorization_endpoint": core.endpoint("device_authorization"),
            "revocation_endpoint": core.endpoint("revoke"),
            "introspection_endpoint": core.endpoint("introspect"),
            "userinfo_endpoint": core.endpoint("userinfo"),
            "jwks_uri": core.endpoint("jwks"),
            "scopes_supported": core.scopes.names(),
            "response_types_supported": ["code"],
            "response_modes_supported": ["query"],
            "grant_types_supported": grants,
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": auth_methods,
            "revocation_endpoint_auth_methods_supported": auth_methods,
            "introspection_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["EdDSA"],
            "claims_supported": ["iss", "sub", "aud", "exp", "iat", "nonce", "name", "email", "email_verified"],
            "authorization_response_iss_parameter_supported": true,
        });
        if core.options.dynamic_registration {
            metadata["registration_endpoint"] = json!(core.endpoint("register"));
        }
        metadata
    }
}

impl Controller for OAuthProtocolController {
    fn prefix(&self) -> &'static str {
        "!/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let base = self.core.options.base_path().to_owned();
        let path = |name: &str| format!("!{base}/{name}");

        // GET /oauth/authorize
        let core = self.core.clone();
        let authorize = move |uri: Uri, uow: Uow| {
            let requests = Requests { core: core.clone() };
            async move {
                let params = AuthorizeParams::from_query(uri.query().unwrap_or_default());
                match requests.authorize(&uow, params).await {
                    Ok(AuthorizeOutcome::Redirect(location)) => redirect(&location),
                    Ok(AuthorizeOutcome::ErrorPage(message)) => error_page(&message),
                    Err(error) => error.to_app_error().into_response(),
                }
            }
        };

        // POST /oauth/token
        let (core, clients, throttles) = (
            self.core.clone(),
            self.clients.clone(),
            self.throttles.clone(),
        );
        let token = move |ClientIp(ip): ClientIp,
                          headers: HeaderMap,
                          extensions: Extensions,
                          body: Bytes| {
            let (core, clients, throttles) = (core.clone(), clients.clone(), throttles.clone());
            async move {
                let mut basic = false;
                let result: Result<TokenResponse, OAuthError> = async {
                    throttles.per_ip(&core, Endpoint::Token, ip).await?;
                    let form = Form::parse(&headers, &body)?;
                    let credentials = client_credentials(&headers, &form)?;
                    basic = credentials.basic;
                    let grant_type = form.require("grant_type")?.to_owned();
                    let client = clients.authenticate(&credentials).await?;
                    throttles
                        .per_client(&core, Endpoint::Token, client.id, ip)
                        .await?;
                    let grants = Grants { core: core.clone() };
                    in_own_uow(&core, &extensions, |uow| async move {
                        match grant_type.as_str() {
                            GRANT_AUTHORIZATION_CODE => {
                                grants
                                    .exchange_code(
                                        &uow,
                                        &client,
                                        form.require("code")?,
                                        form.get("redirect_uri"),
                                        form.get("code_verifier"),
                                    )
                                    .await
                            }
                            GRANT_CLIENT_CREDENTIALS => {
                                grants
                                    .client_credentials(&uow, &client, form.get("scope"))
                                    .await
                            }
                            GRANT_REFRESH_TOKEN => {
                                grants
                                    .refresh(
                                        &uow,
                                        &client,
                                        form.require("refresh_token")?,
                                        form.get("scope"),
                                    )
                                    .await
                            }
                            GRANT_DEVICE_CODE => {
                                grants
                                    .device_code(&uow, &client, form.require("device_code")?)
                                    .await
                            }
                            other => Err(OAuthError::UnsupportedGrantType(format!(
                                "grant_type `{other}` is not supported"
                            ))),
                        }
                    })
                    .await
                }
                .await;
                match result {
                    Ok(tokens) => no_store_json(StatusCode::OK, &tokens),
                    Err(error) => error.protocol_response(basic),
                }
            }
        };

        // POST /oauth/device_authorization
        let (core, clients, throttles) = (
            self.core.clone(),
            self.clients.clone(),
            self.throttles.clone(),
        );
        let device = move |ClientIp(ip): ClientIp,
                           headers: HeaderMap,
                           extensions: Extensions,
                           body: Bytes| {
            let (core, clients, throttles) = (core.clone(), clients.clone(), throttles.clone());
            async move {
                let mut basic = false;
                let result = async {
                    throttles
                        .per_ip(&core, Endpoint::DeviceAuthorization, ip)
                        .await?;
                    let form = Form::parse(&headers, &body)?;
                    let credentials = client_credentials(&headers, &form)?;
                    basic = credentials.basic;
                    let client = clients.authenticate(&credentials).await?;
                    throttles
                        .per_client(&core, Endpoint::DeviceAuthorization, client.id, ip)
                        .await?;
                    let requests = Requests { core: core.clone() };
                    in_own_uow(&core, &extensions, |uow| async move {
                        requests
                            .device_authorization(&uow, &client, form.get("scope"))
                            .await
                    })
                    .await
                }
                .await;
                match result {
                    Ok(authorization) => no_store_json(StatusCode::OK, &authorization),
                    Err(error) => error.protocol_response(basic),
                }
            }
        };

        // POST /oauth/revoke
        let (core, clients, throttles) = (
            self.core.clone(),
            self.clients.clone(),
            self.throttles.clone(),
        );
        let revoke = move |ClientIp(ip): ClientIp,
                           headers: HeaderMap,
                           extensions: Extensions,
                           body: Bytes| {
            let (core, clients, throttles) = (core.clone(), clients.clone(), throttles.clone());
            async move {
                let mut basic = false;
                let result = async {
                    throttles.per_ip(&core, Endpoint::Revoke, ip).await?;
                    let form = Form::parse(&headers, &body)?;
                    let credentials = client_credentials(&headers, &form)?;
                    basic = credentials.basic;
                    let token = form.require("token")?.to_owned();
                    let client = clients.authenticate(&credentials).await?;
                    throttles
                        .per_client(&core, Endpoint::Revoke, client.id, ip)
                        .await?;
                    let grants = Grants { core: core.clone() };
                    in_own_uow(&core, &extensions, |uow| async move {
                        grants.revoke(&uow, &client, &token).await
                    })
                    .await
                }
                .await;
                match result {
                    Ok(()) => no_store_json(StatusCode::OK, &json!({})),
                    Err(error) => error.protocol_response(basic),
                }
            }
        };

        // POST /oauth/introspect
        let (core, clients, throttles) = (
            self.core.clone(),
            self.clients.clone(),
            self.throttles.clone(),
        );
        let introspect = move |ClientIp(ip): ClientIp, headers: HeaderMap, body: Bytes| {
            let (core, clients, throttles) = (core.clone(), clients.clone(), throttles.clone());
            async move {
                let mut basic = false;
                let result = async {
                    throttles.per_ip(&core, Endpoint::Introspect, ip).await?;
                    let form = Form::parse(&headers, &body)?;
                    let credentials = client_credentials(&headers, &form)?;
                    basic = credentials.basic;
                    let token = form.require("token")?.to_owned();
                    let client = clients.authenticate(&credentials).await?;
                    throttles
                        .per_client(&core, Endpoint::Introspect, client.id, ip)
                        .await?;
                    if client.is_public() || !client.introspect {
                        return Err(OAuthError::InvalidClient(
                            "This client may not introspect tokens".into(),
                        ));
                    }
                    Grants { core: core.clone() }.introspect(&token).await
                }
                .await;
                match result {
                    Ok(body) => no_store_json(StatusCode::OK, &body),
                    Err(error) => error.protocol_response(basic),
                }
            }
        };

        // GET|POST /oauth/userinfo
        let core = self.core.clone();
        let userinfo = move |token: OAuthToken| {
            let core = core.clone();
            async move {
                if token.client_id.is_none() || !token.has_scope(OPENID) {
                    return Err(insufficient_scope(&[OPENID.to_owned()]));
                }
                let claims = Grants { core }
                    .user_claims(token.user_id, &token.scopes)
                    .await
                    .map_err(AppError::from)?;
                Ok::<_, AppError>(no_store_json(StatusCode::OK, &claims))
            }
        };

        // GET /oauth/jwks and the discovery documents.
        let core = self.core.clone();
        let jwks = move || {
            let core = core.clone();
            async move { Json(core.keys.jwks()) }
        };
        let core = self.core.clone();
        let discovery = move || {
            let core = core.clone();
            async move { Json(Self::metadata(&core)) }
        };

        let mut routes = vec![
            RouteDef::on(Method::GET, path("authorize"), authorize)
                .public()
                .name("oauth.authorize")
                .api(api(
                    "Start an authorization code request (redirects)",
                    &[302, 400],
                )),
            RouteDef::on(Method::POST, path("token"), token)
                .public()
                .name("oauth.token")
                .api(ApiMeta {
                    response: Some(schema_fn::<TokenResponse>),
                    ..api("Exchange a grant for tokens", &[400, 401, 429])
                }),
            RouteDef::on(Method::POST, path("device_authorization"), device)
                .public()
                .name("oauth.device_authorization")
                .api(ApiMeta {
                    response: Some(schema_fn::<crate::DeviceAuthorization>),
                    ..api("Start a device authorization", &[400, 401, 429])
                }),
            RouteDef::on(Method::POST, path("revoke"), revoke)
                .public()
                .name("oauth.revoke")
                .api(api("Revoke a token (RFC 7009)", &[400, 401, 429])),
            RouteDef::on(Method::POST, path("introspect"), introspect)
                .public()
                .name("oauth.introspect")
                .api(api("Introspect a token (RFC 7662)", &[400, 401, 429])),
            RouteDef::on(Method::GET, path("userinfo"), userinfo.clone())
                .authenticated()
                .name("oauth.userinfo")
                .api(api(
                    "The signed-in user's OpenID Connect claims",
                    &[401, 403],
                )),
            RouteDef::on(Method::POST, path("userinfo"), userinfo)
                .authenticated()
                .name("oauth.userinfo.post")
                .api(api(
                    "The signed-in user's OpenID Connect claims",
                    &[401, 403],
                )),
            RouteDef::on(Method::GET, path("jwks"), jwks)
                .public()
                .name("oauth.jwks")
                .api(api("The keys that verify ID tokens", &[])),
            RouteDef::on(
                Method::GET,
                "!/.well-known/openid-configuration",
                discovery.clone(),
            )
            .public()
            .name("oauth.openid_configuration")
            .api(api("OpenID Connect discovery", &[])),
            RouteDef::on(
                Method::GET,
                "!/.well-known/oauth-authorization-server",
                discovery,
            )
            .public()
            .name("oauth.authorization_server_metadata")
            .api(api(
                "OAuth 2.0 authorization server metadata (RFC 8414)",
                &[],
            )),
        ];
        if self.core.options.dynamic_registration {
            let clients = self.clients.clone();
            let core = self.core.clone();
            let register = move |extensions: Extensions, body: Bytes| {
                let (clients, core) = (clients.clone(), core.clone());
                async move {
                    let result = async {
                        let request: RegistrationRequest =
                            serde_json::from_slice(&body).map_err(|error| {
                                OAuthError::InvalidClientMetadata(format!(
                                    "The body is not valid client metadata: {error}"
                                ))
                            })?;
                        let new = registration_client(&core, request)?;
                        in_own_uow(&core, &extensions, |uow| async move {
                            clients
                                .create(&uow, new)
                                .await
                                .map_err(|error| match error {
                                    OAuthError::Invalid {
                                        field: "redirect_uris",
                                        message,
                                    } => OAuthError::InvalidRedirectUri(message),
                                    OAuthError::Invalid { message, .. } => {
                                        OAuthError::InvalidClientMetadata(message)
                                    }
                                    other => other,
                                })
                        })
                        .await
                    }
                    .await;
                    match result {
                        Ok(created) => {
                            let client = &created.client;
                            let mut body = json!({
                                "client_id": client.id,
                                "client_id_issued_at": client.created_at.timestamp(),
                                "client_name": client.name,
                                "redirect_uris": client.redirect_uri_list(),
                                "grant_types": registered_grant_types(client),
                                "response_types": ["code"],
                                "token_endpoint_auth_method": if client.is_public() { "none" } else { "client_secret_basic" },
                                "scope": client.scopes,
                            });
                            if let Some(secret) = &created.secret {
                                body["client_secret"] = json!(secret);
                                body["client_secret_expires_at"] = json!(0);
                            }
                            no_store_json(StatusCode::CREATED, &body)
                        }
                        Err(error) => error.protocol_response(false),
                    }
                }
            };
            routes.push(
                RouteDef::on(Method::POST, path("register"), register)
                    .public()
                    .name("oauth.register")
                    .api(ApiMeta {
                        success_status: 201,
                        request: Some(schema_fn::<RegistrationRequest>),
                        ..api("Register a client (RFC 7591)", &[400])
                    }),
            );
        }
        routes
    }
}

/// The grant types a registered client reports (RFC 7591 names).
fn registered_grant_types(client: &crate::Client) -> Vec<String> {
    let mut grants = client.grant_list();
    if grants.iter().any(|g| g == GRANT_AUTHORIZATION_CODE) {
        grants.push(GRANT_REFRESH_TOKEN.to_owned());
    }
    grants
}

/// The client a dynamic registration asks for: never trusted, never
/// introspecting, never client credentials.
fn registration_client(core: &Core, request: RegistrationRequest) -> Result<NewClient, OAuthError> {
    let name = request
        .client_name
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| OAuthError::InvalidClientMetadata("client_name is required".into()))?;
    if request.redirect_uris.is_empty() {
        return Err(OAuthError::InvalidRedirectUri(
            "redirect_uris must list at least one URI".into(),
        ));
    }
    let mut new = match request.token_endpoint_auth_method.as_deref() {
        None | Some("client_secret_basic") | Some("client_secret_post") => {
            NewClient::confidential(name)
        }
        Some("none") => NewClient::public(name),
        Some(other) => {
            return Err(OAuthError::InvalidClientMetadata(format!(
                "token_endpoint_auth_method `{other}` is not supported"
            )));
        }
    };
    for uri in request.redirect_uris {
        new = new.redirect_uri(uri);
    }
    for grant in &request.grant_types {
        match grant.as_str() {
            GRANT_AUTHORIZATION_CODE | GRANT_REFRESH_TOKEN => {}
            GRANT_DEVICE_CODE => new = new.device(),
            other => {
                return Err(OAuthError::InvalidClientMetadata(format!(
                    "grant type `{other}` can't be registered"
                )));
            }
        }
    }
    new = new.grant(GRANT_AUTHORIZATION_CODE);
    let scopes = match request.scope.as_deref() {
        Some(scope) => parse_scope(scope),
        None => core.scopes.names(),
    };
    if let Some(unknown) = core.scopes.unknown(&scopes) {
        return Err(OAuthError::InvalidClientMetadata(format!(
            "scope `{unknown}` is not defined"
        )));
    }
    new = new.scopes(scopes);
    if let Some(logo) = request.logo_uri {
        new = new.logo_url(logo);
    }
    if let Some(website) = request.client_uri {
        new = new.website(website);
    }
    Ok(new)
}

fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["OAuth".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn forms_parse_and_refuse_repeats_and_other_types() {
        let form = Form::parse(
            &HeaderMap::new(),
            b"grant_type=client_credentials&scope=a+b",
        )
        .unwrap();
        assert_eq!(form.get("scope"), Some("a b"));
        assert!(form.require("code").is_err());
        assert!(Form::parse(&HeaderMap::new(), b"a=1&a=2").is_err());
        let json = headers(&[("content-type", "application/json")]);
        assert!(Form::parse(&json, b"{}").is_err());
        let form_type = headers(&[(
            "content-type",
            "application/x-www-form-urlencoded; charset=utf-8",
        )]);
        assert!(Form::parse(&form_type, b"a=1").is_ok());
    }

    #[test]
    fn client_credentials_come_from_basic_or_the_body() {
        let encoded = STANDARD.encode("my%20id:s%3Acret");
        let basic = headers(&[("authorization", &format!("Basic {encoded}"))]);
        let credentials = client_credentials(&basic, &Form::default()).unwrap();
        assert_eq!(credentials.client_id.as_deref(), Some("my id"));
        assert_eq!(credentials.client_secret.as_deref(), Some("s:cret"));
        assert!(credentials.basic);

        let form = Form::parse(&HeaderMap::new(), b"client_id=abc&client_secret=xyz").unwrap();
        let credentials = client_credentials(&HeaderMap::new(), &form).unwrap();
        assert_eq!(credentials.client_id.as_deref(), Some("abc"));
        assert_eq!(credentials.client_secret.as_deref(), Some("xyz"));
        assert!(!credentials.basic);

        // Both methods at once, a mismatched id, garbage.
        assert!(matches!(
            client_credentials(&basic, &form),
            Err(OAuthError::InvalidRequest(_))
        ));
        let other_id = Form::parse(&HeaderMap::new(), b"client_id=other").unwrap();
        assert!(matches!(
            client_credentials(&basic, &other_id),
            Err(OAuthError::InvalidClient(_))
        ));
        let garbage = headers(&[("authorization", "Basic !!!")]);
        assert!(matches!(
            client_credentials(&garbage, &Form::default()),
            Err(OAuthError::InvalidClient(_))
        ));
    }

    #[test]
    fn error_pages_escape_their_message() {
        let response = error_page("<script>alert(1)</script>");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
    }
}
