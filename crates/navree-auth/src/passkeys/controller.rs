//! The passkey routes under `{api_prefix}{routes_prefix}/passkeys`, mounted
//! when `auth.passkeys.enabled` is on.

use std::sync::Arc;
use std::time::Duration;

use http::{Method, StatusCode};
use navree_contracts::{AuthState, CallTickets, TicketCheck, TicketError, TicketHolder};
use navree_http::{
    ApiMeta, AppError, Controller, Extension, IntoResponse, Json, NoContent, Path, RouteDef,
    schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_validation::Valid;
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::confirm::{ConfirmAction, STEP_UP_ACTION, is_digest};
use super::{MAX_PASSKEY_NAME, PasskeyResource};
use crate::controller::{BrowserSession, OptionalBody, api, login_response};
use crate::{
    AccessClaims, AuthError, AuthOptions, AuthService, ClientMeta, ConfirmProof, ConfirmedToken,
    CurrentUser, LoginResponse, not_while_impersonating,
};

/// Ceremony option requests one client may make per minute.
const OPTIONS_PER_MINUTE: u32 = 30;

/// `POST /auth/passkeys/register`: the credential from
/// `navigator.credentials.create()` (its `toJSON()`).
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct PasskeyRegisterInput {
    /// A name for the passkey ("Work laptop"); `Passkey` when absent.
    #[validate(length(min = 1, max = MAX_PASSKEY_NAME))]
    #[serde(default)]
    pub name: Option<String>,
    /// The `PublicKeyCredential` JSON.
    #[validate(skip)]
    #[schema(value_type = Object)]
    pub credential: Value,
}

/// `POST /auth/passkeys/login/options`.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct PasskeyLoginOptionsInput {
    /// The account's email address, to list its passkeys in
    /// `allowCredentials`; without it the browser offers discoverable
    /// passkeys.
    #[validate(email, length(max = 255))]
    #[serde(default)]
    pub email: Option<String>,
}

/// `POST /auth/passkeys/login`: the assertion from
/// `navigator.credentials.get()` (its `toJSON()`).
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct PasskeyLoginInput {
    /// The `PublicKeyCredential` JSON.
    #[validate(skip)]
    #[schema(value_type = Object)]
    pub credential: Value,
}

/// `POST /auth/confirm/options`: the ticket of a confirmation a page asked
/// for (the `confirm` call's), or nothing for a plain step-up.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[non_exhaustive]
pub struct ConfirmOptionsInput {
    /// The sealed ticket the page issued with the call (its binding is the
    /// digest of the action to confirm). Used up here.
    #[validate(length(min = 1, max = 4096))]
    #[serde(default)]
    pub ticket: Option<String>,
}

/// `POST /auth/confirm/passkey`: a step-up with a passkey, the assertion
/// from `navigator.credentials.get()` (its `toJSON()`) over the options of
/// `POST /auth/confirm/options`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[non_exhaustive]
pub struct ConfirmPasskeyInput {
    /// The `PublicKeyCredential` JSON.
    #[validate(skip)]
    #[schema(value_type = Object)]
    pub credential: Value,
}

/// Ceremony options: `{"publicKey": {...}}`, for
/// `PublicKeyCredential.parseCreationOptionsFromJSON` /
/// `parseRequestOptionsFromJSON`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PasskeyOptionsResponse {
    /// The `publicKey` member of the credential options.
    #[serde(rename = "publicKey")]
    #[schema(value_type = Object)]
    pub public_key: Value,
}

/// `GET /auth/passkeys`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PasskeyList {
    /// The user's passkeys, oldest first.
    pub data: Vec<PasskeyResource>,
}

/// The passkey routes.
pub(crate) struct PasskeyController {
    service: Arc<AuthService>,
    options: Arc<AuthOptions>,
    tickets: Arc<dyn CallTickets>,
}

impl Injectable for PasskeyController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthOptions>(),
            ProviderKey::of::<dyn CallTickets>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(PasskeyController {
                service: r.get::<AuthService>().await?,
                options: r.get::<AuthOptions>().await?,
                tickets: r.get::<dyn CallTickets>().await?,
            })
        })
    }
}

/// The `confirm` call's name in tickets.
pub(crate) const CONFIRM_FEATURE: &str = "confirm";

impl PasskeyController {
    /// `POST /confirm/options` and `POST /confirm/passkey`.
    fn confirm_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();

        let service = self.service.clone();
        let tickets = self.tickets.clone();
        let options =
            move |claims: AccessClaims,
                  state: Extension<AuthState>,
                  OptionalBody(input): OptionalBody<ConfirmOptionsInput>| {
                let service = service.clone();
                let tickets = tickets.clone();
                async move {
                    let digest = match input.ticket {
                        // The action the page asked about, sealed in its ticket
                        // for this session.
                        Some(ticket) => {
                            let check = TicketCheck::new(
                                TicketHolder::of(state.0.0.as_ref()),
                                CONFIRM_FEATURE,
                            );
                            let redeemed = tickets
                                .redeem(&ticket, &check)
                                .await
                                .map_err(|error| AuthError::from(error).into_app_error())?;
                            redeemed
                                .binding
                                .filter(|digest| is_digest(digest))
                                .ok_or_else(|| {
                                    AuthError::from(TicketError::Malformed).into_app_error()
                                })?
                        }
                        None => ConfirmAction::new(STEP_UP_ACTION).digest(),
                    };
                    let public_key = service
                        .confirmation_options_for(claims.sub, claims.sid, digest)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(Json(PasskeyOptionsResponse { public_key }))
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/confirm/options"), options)
                .authenticated()
                .guard(not_while_impersonating())
                .throttle(OPTIONS_PER_MINUTE, Duration::from_secs(60))
                .name("auth.confirm.options")
                .api(ApiMeta {
                    description: Some(
                        "A challenge for confirming with a passkey (user verification \
                         required, two minutes, one use). With the `ticket` of a page's \
                         `confirm` call, the challenge is derived from that action, and the \
                         assertion goes to the page's own route; without one, it is for \
                         `POST /auth/confirm/passkey`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<ConfirmOptionsInput>),
                    response: Some(schema_fn::<PasskeyOptionsResponse>),
                    ..api("Start confirming with a passkey", &[401, 403, 422, 429])
                }),
        );

        let service = self.service.clone();
        let step_up =
            move |claims: AccessClaims,
                  meta: ClientMeta,
                  browser: BrowserSession,
                  Valid(Json(input)): Valid<Json<ConfirmPasskeyInput>>| {
                let service = service.clone();
                async move {
                    let proof = ConfirmProof::Passkey(input.credential);
                    // A browser signed in on its session: the sign-in's time
                    // moves on; it gets no token.
                    #[cfg(feature = "session")]
                    if let Some(session) = browser.0 {
                        service
                            .confirm_session(&session, proof, &meta)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        return Ok::<_, AppError>(NoContent.into_response());
                    }
                    #[cfg(not(feature = "session"))]
                    let _ = browser;
                    let confirmed = service
                        .confirm(&claims, proof, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(Json(confirmed).into_response())
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/confirm/passkey"), step_up)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.confirm.passkey")
                .api(ApiMeta {
                    description: Some(
                        "Confirms with a passkey, as `POST /auth/confirm` does with the \
                         password: a new access token whose `auth_time` is now, or 204 for a \
                         browser signed in on the session cookie."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<ConfirmPasskeyInput>),
                    response: Some(schema_fn::<ConfirmedToken>),
                    ..api("Confirm your identity with a passkey", &[400, 401, 403])
                }),
        );
        routes
    }
}

impl Controller for PasskeyController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.passkeys.enabled {
            return Vec::new();
        }
        let prefix = self.options.routes_prefix.trim_end_matches('/');
        let path = |p: &str| format!("{prefix}/passkeys{p}");
        let mut routes = Vec::new();

        let service = self.service.clone();
        let register_options = move |CurrentUser(user): CurrentUser| {
            let service = service.clone();
            async move {
                let public_key = service
                    .passkey_registration_options(&user)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(PasskeyOptionsResponse { public_key }))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/register/options"), register_options)
                .authenticated()
                .guard(not_while_impersonating())
                .throttle(OPTIONS_PER_MINUTE, Duration::from_secs(60))
                .name("auth.passkeys.register_options")
                .api(ApiMeta {
                    response: Some(schema_fn::<PasskeyOptionsResponse>),
                    ..api("Start registering a passkey", &[401, 403, 429])
                }),
        );

        let service = self.service.clone();
        let register =
            move |CurrentUser(user): CurrentUser,
                  meta: ClientMeta,
                  Valid(Json(input)): Valid<Json<PasskeyRegisterInput>>| {
                let service = service.clone();
                async move {
                    let passkey = service
                        .register_passkey(&user, input.name.as_deref(), &input.credential, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(
                        (StatusCode::CREATED, Json(PasskeyResource::from(&passkey)))
                            .into_response(),
                    )
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/register"), register)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.passkeys.register")
                .api(ApiMeta {
                    success_status: 201,
                    request: Some(schema_fn::<PasskeyRegisterInput>),
                    response: Some(schema_fn::<PasskeyResource>),
                    ..api("Register a passkey", &[400, 401, 403])
                }),
        );

        let service = self.service.clone();
        let list = move |CurrentUser(user): CurrentUser| {
            let service = service.clone();
            async move {
                let data = service
                    .passkeys(user.id)
                    .await
                    .map_err(AuthError::into_app_error)?
                    .iter()
                    .map(PasskeyResource::from)
                    .collect();
                Ok::<_, AppError>(Json(PasskeyList { data }))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path(""), list)
                .authenticated()
                .name("auth.passkeys.index")
                .api(ApiMeta {
                    response: Some(schema_fn::<PasskeyList>),
                    ..api("The signed-in user's passkeys", &[401])
                }),
        );

        let service = self.service.clone();
        let delete =
            move |CurrentUser(user): CurrentUser, meta: ClientMeta, Path(id): Path<Uuid>| {
                let service = service.clone();
                async move {
                    service
                        .delete_passkey(user.id, id, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(NoContent)
                }
            };
        routes.push(
            RouteDef::on(Method::DELETE, path("/{id}"), delete)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.passkeys.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Remove a passkey", &[401, 403, 404])
                }),
        );

        let service = self.service.clone();
        let login_options = move |OptionalBody(input): OptionalBody<PasskeyLoginOptionsInput>| {
            let service = service.clone();
            async move {
                let public_key = service
                    .passkey_login_options(input.email.as_deref())
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(PasskeyOptionsResponse { public_key }))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/login/options"), login_options)
                .public()
                .throttle(OPTIONS_PER_MINUTE, Duration::from_secs(60))
                .name("auth.passkeys.login_options")
                .api(ApiMeta {
                    request: Some(schema_fn::<PasskeyLoginOptionsInput>),
                    response: Some(schema_fn::<PasskeyOptionsResponse>),
                    ..api("Start signing in with a passkey", &[400, 429])
                }),
        );

        let service = self.service.clone();
        let login = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<PasskeyLoginInput>>| {
            let service = service.clone();
            async move {
                let outcome = service
                    .login_with_passkey(&input.credential, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                login_response(&service, outcome).await
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/login"), login)
                .public()
                .name("auth.passkeys.login")
                .api(ApiMeta {
                    request: Some(schema_fn::<PasskeyLoginInput>),
                    response: Some(schema_fn::<LoginResponse>),
                    ..api("Sign in with a passkey", &[400, 401])
                }),
        );
        // Beside `POST /confirm`: `/confirm/options`, `/confirm/passkey`.
        let beside_confirm = |p: &str| format!("{prefix}{p}");
        routes.extend(self.confirm_routes(&beside_confirm));
        routes
    }
}
