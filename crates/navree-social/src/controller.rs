//! The routes under `{api_prefix}{auth.routes_prefix}/social` (default
//! `/api/auth/social`).

use std::sync::Arc;

use http::request::Parts;
use http::{Method, StatusCode};
use navree_auth::{AuthOptions, ClientMeta, LoginOutcome};
use navree_contracts::{AuthState, Principal};
use navree_db::Uow;
use navree_http::extract::FromRequestParts;
use navree_http::{
    ApiMeta, AppError, Controller, CurrentPrincipal, IntoResponse, Json, NoContent, Path, Query,
    Response, RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Intent, LinkedAccount, RedirectOptions, Social, SocialCallback, SocialError, SocialOptions,
    SocialRedirect,
};

/// `GET .../redirect` query.
#[derive(Debug, Default, Deserialize)]
struct RedirectQuery {
    /// A relative path to come back to.
    return_to: Option<String>,
    /// `sign_in` (default) or `link`.
    intent: Option<Intent>,
}

/// The request's principal, if any (on public routes).
struct MaybePrincipal(Option<Principal>);

impl FromRequestParts for MaybePrincipal {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(MaybePrincipal(
            parts
                .extensions
                .get::<AuthState>()
                .and_then(|state| state.0.clone()),
        ))
    }
}

/// The social routes.
pub(crate) struct SocialController {
    social: Arc<Social>,
    auth: Arc<AuthOptions>,
    options: Arc<SocialOptions>,
}

impl Injectable for SocialController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Social>(),
            ProviderKey::of::<AuthOptions>(),
            ProviderKey::of::<SocialOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SocialController {
                social: r.get::<Social>().await?,
                auth: r.get::<AuthOptions>().await?,
                options: r.get::<SocialOptions>().await?,
            })
        })
    }
}

fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Auth".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

fn social_error(error: SocialError) -> AppError {
    error.into_app_error()
}

impl Controller for SocialController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.routes {
            return Vec::new();
        }
        let prefix = format!("{}/social", self.auth.routes_prefix.trim_end_matches('/'));
        let path = |p: &str| format!("{prefix}{p}");
        let mut routes = Vec::new();

        let social = self.social.clone();
        let redirect = move |Path(provider): Path<String>,
                             MaybePrincipal(principal): MaybePrincipal,
                             Query(query): Query<RedirectQuery>| {
            let social = social.clone();
            async move {
                let intent = query.intent.unwrap_or_default();
                let user_id = match (intent, principal) {
                    (Intent::Link, Some(principal)) => Some(principal.user_id),
                    (Intent::Link, None) => {
                        return Err(AppError::unauthenticated()
                            .with_detail("Sign in to connect a provider"));
                    }
                    (Intent::SignIn, _) => None,
                };
                let redirect = social
                    .redirect_with(
                        &provider,
                        RedirectOptions {
                            return_to: query.return_to,
                            intent,
                            user_id,
                            ..RedirectOptions::default()
                        },
                    )
                    .await
                    .map_err(social_error)?;
                Ok::<_, AppError>(Json(redirect))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/{provider}/redirect"), redirect)
                .public()
                .name("social.redirect")
                .api(ApiMeta {
                    description: Some(
                        "`?return_to=/path` (a relative path, returned by the callback) and \
                         `?intent=link` (signed in; finish with `POST .../link`)."
                            .to_owned(),
                    ),
                    response: Some(schema_fn::<SocialRedirect>),
                    ..api(
                        "The provider's authorization URL and the sealed state",
                        &[400, 401, 404],
                    )
                }),
        );

        let social = self.social.clone();
        let callback = move |Path(provider): Path<String>,
                             meta: ClientMeta,
                             Json(callback): Json<SocialCallback>| {
            let social = social.clone();
            async move {
                let throttle = social
                    .count_callback(meta.ip.as_deref())
                    .await
                    .map_err(social_error)?;
                let (profile, state) = social
                    .complete(&provider, &callback)
                    .await
                    .map_err(social_error)?;
                if state.intent != Intent::SignIn {
                    return Err(social_error(SocialError::WrongIntent));
                }
                let outcome = social
                    .sign_in(&profile, &meta)
                    .await
                    .map_err(social_error)?;
                social.clear_callback_throttle(throttle).await;
                let body = social
                    .present_outcome(outcome, state.return_to)
                    .await
                    .map_err(social_error)?;
                Ok::<_, AppError>(Json(body))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/{provider}/callback"), callback)
                .public()
                .name("social.callback")
                .api(ApiMeta {
                    description: Some(
                        "Answers like `POST /auth/login` (the user and tokens, or a two-factor \
                         challenge), plus `return_to` from the state."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<SocialCallback>),
                    response: Some(schema_fn::<navree_auth::LoginResponse>),
                    ..api(
                        "Sign in with the provider's code and the state",
                        &[400, 403, 404, 409, 429, 502],
                    )
                }),
        );

        let social = self.social.clone();
        let accounts = move |CurrentPrincipal(principal): CurrentPrincipal| {
            let social = social.clone();
            async move {
                let accounts = social
                    .accounts(principal.user_id)
                    .await
                    .map_err(social_error)?;
                Ok::<_, AppError>(Json(accounts))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/accounts"), accounts)
                .authenticated()
                .name("social.accounts")
                .api(ApiMeta {
                    response: Some(schema_fn::<Vec<LinkedAccount>>),
                    ..api("The signed-in user's linked accounts", &[401])
                }),
        );

        let social = self.social.clone();
        let link = move |Path(provider): Path<String>,
                         uow: Uow,
                         CurrentPrincipal(principal): CurrentPrincipal,
                         Json(callback): Json<SocialCallback>| {
            let social = social.clone();
            async move {
                let (profile, state) = social
                    .complete(&provider, &callback)
                    .await
                    .map_err(social_error)?;
                if state.intent != Intent::Link || state.user != Some(principal.user_id) {
                    return Err(social_error(SocialError::WrongIntent));
                }
                let account = social
                    .link(&uow, principal.user_id, &profile)
                    .await
                    .map_err(social_error)?;
                Ok::<_, AppError>((StatusCode::CREATED, Json(account)).into_response())
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/{provider}/link"), link)
                .authenticated()
                .name("social.link")
                .api(ApiMeta {
                    success_status: 201,
                    request: Some(schema_fn::<SocialCallback>),
                    response: Some(schema_fn::<LinkedAccount>),
                    ..api(
                        "Connect a provider to the signed-in user",
                        &[400, 401, 404, 409, 502],
                    )
                }),
        );

        let social = self.social.clone();
        let unlink = move |Path(provider): Path<String>,
                           uow: Uow,
                           CurrentPrincipal(principal): CurrentPrincipal| {
            let social = social.clone();
            async move {
                social
                    .unlink(&uow, principal.user_id, &provider)
                    .await
                    .map_err(social_error)?;
                Ok::<_, AppError>(NoContent)
            }
        };
        routes.push(
            RouteDef::on(Method::DELETE, path("/accounts/{provider}"), unlink)
                .authenticated()
                .name("social.unlink")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Disconnect a provider", &[401, 404, 409])
                }),
        );
        routes
    }
}

impl Social {
    /// The callback's answer, shaped like `POST /auth/login`'s, plus
    /// `return_to`.
    pub(crate) async fn present_outcome(
        &self,
        outcome: LoginOutcome,
        return_to: Option<String>,
    ) -> Result<Value, SocialError> {
        let mut body = match outcome {
            LoginOutcome::Authenticated { user, tokens } => {
                let user = self.auth_service()?.present(&user).await?;
                let mut body = serde_json::to_value(&tokens)?;
                if let Value::Object(fields) = &mut body {
                    fields.insert("user".to_owned(), user);
                }
                body
            }
            LoginOutcome::TwoFactorRequired {
                challenge_token, ..
            } => json!({
                "two_factor_required": true,
                "challenge_token": challenge_token,
                "expires_in": self.challenge_ttl_secs()?,
            }),
        };
        if let (Some(return_to), Value::Object(fields)) = (return_to, &mut body) {
            fields.insert("return_to".to_owned(), Value::String(return_to));
        }
        Ok(body)
    }
}

/// Responses carry the error mapping.
impl IntoResponse for SocialError {
    fn into_response(self) -> Response {
        self.into_app_error().into_response()
    }
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_maybe_principal_are_native() {
        async fn handler(_: MaybePrincipal) {}
        assert!(Endpoint::new(handler).is_native());
    }
}
