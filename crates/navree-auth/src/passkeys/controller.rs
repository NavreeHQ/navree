//! The passkey routes under `{api_prefix}{routes_prefix}/passkeys`, mounted
//! when `auth.passkeys.enabled` is on.

use std::sync::Arc;
use std::time::Duration;

use http::{Method, StatusCode};
use navree_http::{
    ApiMeta, AppError, Controller, IntoResponse, Json, NoContent, Path, RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_validation::Valid;
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{MAX_PASSKEY_NAME, PasskeyResource};
use crate::controller::{OptionalBody, api, login_response};
use crate::{
    AuthError, AuthOptions, AuthService, ClientMeta, CurrentUser, LoginResponse,
    not_while_impersonating,
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
}

impl Injectable for PasskeyController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(PasskeyController {
                service: r.get::<AuthService>().await?,
                options: r.get::<AuthOptions>().await?,
            })
        })
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
        routes
    }
}
