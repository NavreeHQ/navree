//! The routes your front end calls, under `{http.api_prefix}/oauth`: the
//! consent and device pages' decisions, personal access tokens and the
//! user's authorized apps.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use http::{Method, Uri};
use navree_db::Uow;
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, Path, RouteDef,
    schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::core::Core;
use crate::pats::{PersonalAccessTokenResource, PersonalAccessTokens};
use crate::requests::{
    AuthorizationRequestView, AuthorizationView, Decision, DeviceRequestView, Requests,
};
use crate::strategy::OAuthToken;

/// The body of `POST /api/oauth/personal-access-tokens`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreatePersonalAccessTokenInput {
    /// A name to recognize it by.
    pub name: String,
    /// Its scopes (the app's scopes from `oauth.scopes`).
    pub scopes: Vec<String>,
    /// Days until it expires; at most `oauth.personal_access_tokens.max_days`
    /// (the default when absent).
    pub expires_in_days: Option<u32>,
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DataList<T> {
    /// The items.
    pub data: Vec<T>,
}

/// OAuth tokens and personal access tokens may not approve other apps,
/// mint tokens or manage grants: these routes need a first-party sign-in.
fn first_party(token: &Option<OAuthToken>) -> Result<(), AppError> {
    if token.is_some() {
        return Err(AppError::forbidden().with_detail(
            "OAuth and personal access tokens can't manage OAuth access; sign in to the app.",
        ));
    }
    Ok(())
}

/// The `/api/oauth` routes.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /oauth/requests/{id}` | the authorization request for the consent page |
/// | `POST /oauth/requests/{id}/approve` | `{redirect_to}` with a code |
/// | `POST /oauth/requests/{id}/deny` | `{redirect_to}` with `error=access_denied` |
/// | `GET /oauth/device/{user_code}` | the device authorization for the device page |
/// | `POST /oauth/device/{user_code}/approve` | 204 |
/// | `POST /oauth/device/{user_code}/deny` | 204 |
/// | `GET /oauth/personal-access-tokens` | the user's tokens, `{data: [..]}` |
/// | `POST /oauth/personal-access-tokens` | 201, the token with its secret |
/// | `DELETE /oauth/personal-access-tokens/{id}` | 204 |
/// | `GET /oauth/authorizations` | the apps the user approved, `{data: [..]}` |
/// | `DELETE /oauth/authorizations/{client_id}` | 204; revokes the app's tokens |
///
/// Every route needs a signed-in user, and a first-party one: a request
/// made with an OAuth token or a personal access token gets a 403.
pub struct OAuthApiController {
    core: Arc<Core>,
    pats: Arc<PersonalAccessTokens>,
}

impl Injectable for OAuthApiController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Core>(),
            ProviderKey::of::<PersonalAccessTokens>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(OAuthApiController {
                core: r.get::<Core>().await?,
                pats: r.get::<PersonalAccessTokens>().await?,
            })
        })
    }
}

impl fmt::Debug for OAuthApiController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthApiController").finish_non_exhaustive()
    }
}

impl Controller for OAuthApiController {
    fn prefix(&self) -> &'static str {
        "/oauth"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let core = self.core.clone();
        let show = move |CurrentPrincipal(me): CurrentPrincipal,
                         token: Option<OAuthToken>,
                         Path(id): Path<Uuid>| {
            let r = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                Ok::<_, AppError>(Json(r.show(me.user_id, id).await?))
            }
        };
        let core = self.core.clone();
        let approve = move |CurrentPrincipal(me): CurrentPrincipal,
                            token: Option<OAuthToken>,
                            uow: Uow,
                            Path(id): Path<Uuid>| {
            let r = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                Ok::<_, AppError>(Json(r.approve(&uow, me.user_id, id).await?))
            }
        };
        let core = self.core.clone();
        let deny = move |CurrentPrincipal(me): CurrentPrincipal,
                         token: Option<OAuthToken>,
                         uow: Uow,
                         Path(id): Path<Uuid>| {
            let r = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                Ok::<_, AppError>(Json(r.deny(&uow, me.user_id, id).await?))
            }
        };
        let core = self.core.clone();
        let show_device = move |CurrentPrincipal(me): CurrentPrincipal,
                                token: Option<OAuthToken>,
                                Path(code): Path<String>| {
            let r = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                Ok::<_, AppError>(Json(r.show_device(me.user_id, &code).await?))
            }
        };
        let core = self.core.clone();
        let approve_device = move |CurrentPrincipal(me): CurrentPrincipal,
                                   token: Option<OAuthToken>,
                                   uow: Uow,
                                   Path(code): Path<String>| {
            let r = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                r.decide_device(&uow, me.user_id, &code, true).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let core = self.core.clone();
        let deny_device = move |CurrentPrincipal(me): CurrentPrincipal,
                                token: Option<OAuthToken>,
                                uow: Uow,
                                Path(code): Path<String>| {
            let r = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                r.decide_device(&uow, me.user_id, &code, false).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let core = self.core.clone();
        let authorizations = move |CurrentPrincipal(me): CurrentPrincipal,
                                   token: Option<OAuthToken>| {
            let r = Requests { core: core.clone() };
            async move {
                first_party(&token)?;
                let data: Vec<AuthorizationView> = r.authorizations(me.user_id).await?;
                Ok::<_, AppError>(Json(DataList { data }))
            }
        };
        let core = self.core.clone();
        let revoke_authorization =
            move |CurrentPrincipal(me): CurrentPrincipal,
                  token: Option<OAuthToken>,
                  uow: Uow,
                  Path(client_id): Path<Uuid>| {
                let r = Requests { core: core.clone() };
                async move {
                    first_party(&token)?;
                    r.revoke_authorization(&uow, me.user_id, client_id).await?;
                    Ok::<_, AppError>(NoContent)
                }
            };

        let throttle = Duration::from_secs(60);
        let mut routes = vec![
            RouteDef::on(Method::GET, "/requests/{id}", show)
                .authenticated()
                .name("oauth.requests.show")
                .api(api(
                    "Read an authorization request for the consent page",
                    &[401, 403, 404, 410],
                    None,
                    Some(schema_fn::<AuthorizationRequestView>),
                )),
            RouteDef::on(Method::POST, "/requests/{id}/approve", approve)
                .authenticated()
                .name("oauth.requests.approve")
                .api(api(
                    "Approve an authorization request",
                    &[401, 403, 404, 410],
                    None,
                    Some(schema_fn::<Decision>),
                )),
            RouteDef::on(Method::POST, "/requests/{id}/deny", deny)
                .authenticated()
                .name("oauth.requests.deny")
                .api(api(
                    "Deny an authorization request",
                    &[401, 403, 404, 410],
                    None,
                    Some(schema_fn::<Decision>),
                )),
            RouteDef::on(Method::GET, "/device/{user_code}", show_device)
                .authenticated()
                .throttle(20, throttle)
                .name("oauth.device.show")
                .api(api(
                    "Read a device authorization for the device page",
                    &[401, 403, 404, 429],
                    None,
                    Some(schema_fn::<DeviceRequestView>),
                )),
            RouteDef::on(Method::POST, "/device/{user_code}/approve", approve_device)
                .authenticated()
                .throttle(20, throttle)
                .name("oauth.device.approve")
                .api(ApiMeta {
                    success_status: 204,
                    ..api(
                        "Approve a device authorization",
                        &[401, 403, 404, 429],
                        None,
                        None,
                    )
                }),
            RouteDef::on(Method::POST, "/device/{user_code}/deny", deny_device)
                .authenticated()
                .throttle(20, throttle)
                .name("oauth.device.deny")
                .api(ApiMeta {
                    success_status: 204,
                    ..api(
                        "Deny a device authorization",
                        &[401, 403, 404, 429],
                        None,
                        None,
                    )
                }),
            RouteDef::on(Method::GET, "/authorizations", authorizations)
                .authenticated()
                .name("oauth.authorizations.index")
                .api(api(
                    "List the apps the user authorized",
                    &[401, 403],
                    None,
                    None,
                )),
            RouteDef::on(
                Method::DELETE,
                "/authorizations/{client_id}",
                revoke_authorization,
            )
            .authenticated()
            .name("oauth.authorizations.destroy")
            .api(ApiMeta {
                success_status: 204,
                ..api(
                    "Revoke an app's access and its tokens",
                    &[401, 403, 404],
                    None,
                    None,
                )
            }),
        ];

        if self.core.options.personal_access_tokens.enabled {
            let pats = self.pats.clone();
            let index = move |CurrentPrincipal(me): CurrentPrincipal, token: Option<OAuthToken>| {
                let pats = pats.clone();
                async move {
                    first_party(&token)?;
                    let data: Vec<PersonalAccessTokenResource> = pats
                        .list(me.user_id)
                        .await?
                        .iter()
                        .map(PersonalAccessTokenResource::from)
                        .collect();
                    Ok::<_, AppError>(Json(DataList { data }))
                }
            };
            let pats = self.pats.clone();
            let store =
                move |CurrentPrincipal(me): CurrentPrincipal,
                      token: Option<OAuthToken>,
                      uow: Uow,
                      uri: Uri,
                      Json(input): Json<CreatePersonalAccessTokenInput>| {
                    let pats = pats.clone();
                    async move {
                        first_party(&token)?;
                        let lifetime = input
                            .expires_in_days
                            .map(|days| Duration::from_secs(u64::from(days) * 86_400));
                        let created = pats
                            .create(&uow, me.user_id, &input.name, input.scopes, lifetime)
                            .await?;
                        let location =
                            format!("{}/{}", uri.path().trim_end_matches('/'), created.record.id);
                        Ok::<_, AppError>(Created(
                            location,
                            PersonalAccessTokenResource::from(&created),
                        ))
                    }
                };
            let pats = self.pats.clone();
            let destroy = move |CurrentPrincipal(me): CurrentPrincipal,
                                token: Option<OAuthToken>,
                                uow: Uow,
                                Path(id): Path<Uuid>| {
                let pats = pats.clone();
                async move {
                    first_party(&token)?;
                    pats.revoke(&uow, me.user_id, id).await?;
                    Ok::<_, AppError>(NoContent)
                }
            };
            routes.extend([
                RouteDef::on(Method::GET, "/personal-access-tokens", index)
                    .authenticated()
                    .name("oauth.personal_access_tokens.index")
                    .api(api(
                        "List the user's personal access tokens",
                        &[401, 403],
                        None,
                        None,
                    )),
                RouteDef::on(Method::POST, "/personal-access-tokens", store)
                    .authenticated()
                    .throttle(10, throttle)
                    .name("oauth.personal_access_tokens.store")
                    .api(ApiMeta {
                        success_status: 201,
                        ..api(
                            "Create a personal access token",
                            &[400, 401, 403, 429],
                            Some(schema_fn::<CreatePersonalAccessTokenInput>),
                            Some(schema_fn::<PersonalAccessTokenResource>),
                        )
                    }),
                RouteDef::on(Method::DELETE, "/personal-access-tokens/{id}", destroy)
                    .authenticated()
                    .name("oauth.personal_access_tokens.destroy")
                    .api(ApiMeta {
                        success_status: 204,
                        ..api(
                            "Revoke a personal access token",
                            &[401, 403, 404],
                            None,
                            None,
                        )
                    }),
            ]);
        }
        routes
    }
}

fn api(
    summary: &str,
    responses: &[u16],
    request: Option<navree_http::SchemaFn>,
    response: Option<navree_http::SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["OAuth".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}
