//! The access module's routes: `GET /api/me/abilities` and the role
//! management routes under `/api/access`.

use std::sync::Arc;

use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use http::header::{CACHE_CONTROL, ETAG, IF_NONE_MATCH};
use http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use keel_db::Uow;
use keel_http::{ApiMeta, AppError, Controller, Created, Json, NoContent, RouteDef};
use keel_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::extract::CurrentAccess;
use crate::factory::PrincipalAccess;
use crate::service::AccessService;

/// The permission the management routes require.
pub(crate) const MANAGE_ROLES: &str = "roles.manage";

/// `GET /api/me/abilities`: the user's rules as CASL's JSON rule array,
/// with an ETag.
pub(crate) struct AbilitiesController;

impl Injectable for AbilitiesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AbilitiesController) })
    }
}

impl Controller for AbilitiesController {
    fn prefix(&self) -> &'static str {
        "/me"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/abilities", abilities)
                .authenticated()
                .name("access.abilities")
                .api(api(
                    "The signed-in user's rules (CASL JSON), with an ETag",
                    &[304, 401],
                )),
        ]
    }
}

/// The rules as `createMongoAbility` accepts them. The ETag is
/// `"<user>-<access version>-<rules hash>"`: it changes with any role or
/// permission change and with the rules themselves (a deploy that changes
/// the abilities function, a token with other abilities).
async fn abilities(CurrentAccess(access): CurrentAccess, headers: HeaderMap) -> Response {
    let rules = access.ability.to_casl_json();
    let etag = etag(&access, &rules.to_string());
    let cache = [
        (ETAG, etag.clone()),
        (CACHE_CONTROL, HeaderValue::from_static("private, no-cache")),
    ];
    let fresh = headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| if_none_match(value, etag.to_str().unwrap_or_default()));
    if fresh {
        (StatusCode::NOT_MODIFIED, cache).into_response()
    } else {
        (cache, Json(rules)).into_response()
    }
}

fn etag(access: &PrincipalAccess, rules: &str) -> HeaderValue {
    let user = access.user.map(|u| u.to_string()).unwrap_or_default();
    let value = format!("\"{user}-{}-{:016x}\"", access.version, fnv1a(rules));
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("\"\""))
}

/// Whether an `If-None-Match` header value lists `etag` (weak comparison).
fn if_none_match(header: &str, etag: &str) -> bool {
    header.split(',').map(str::trim).any(|candidate| {
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

/// 64-bit FNV-1a: a stable hash, the same on every node and build.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Role management under `/api/access` (permission `roles.manage`).
pub(crate) struct AccessAdminController {
    service: Arc<AccessService>,
}

impl Injectable for AccessAdminController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<AccessService>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(AccessAdminController {
                service: r.get::<AccessService>().await?,
            })
        })
    }
}

#[derive(Deserialize)]
struct CreateRole {
    name: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct SyncPermissions {
    permissions: Vec<String>,
}

#[derive(Deserialize)]
struct SyncRoles {
    roles: Vec<String>,
}

impl Controller for AccessAdminController {
    fn prefix(&self) -> &'static str {
        "/access"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let service = self.service.clone();
        let list = move || {
            let service = service.clone();
            async move {
                let roles = service.roles().await?;
                Ok::<_, AppError>(Json(json!({ "data": roles })))
            }
        };
        let service = self.service.clone();
        let create = move |uow: Uow, uri: Uri, Json(input): Json<CreateRole>| {
            let service = service.clone();
            async move {
                let role = service
                    .create_role(&uow, &input.name, input.description.as_deref())
                    .await?;
                let location = format!("{}/{}", uri.path().trim_end_matches('/'), role.name);
                Ok::<_, AppError>(Created(location, role))
            }
        };
        let service = self.service.clone();
        let sync_permissions =
            move |uow: Uow, Path(role): Path<String>, Json(input): Json<SyncPermissions>| {
                let service = service.clone();
                async move {
                    let role = service
                        .sync_role_permissions(&uow, &role, &input.permissions)
                        .await?;
                    Ok::<_, AppError>(Json(role))
                }
            };
        let service = self.service.clone();
        let delete = move |uow: Uow, Path(role): Path<String>| {
            let service = service.clone();
            async move {
                service.delete_role(&uow, &role).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let service = self.service.clone();
        let sync_roles = move |uow: Uow, Path(user): Path<Uuid>, Json(input): Json<SyncRoles>| {
            let service = service.clone();
            async move {
                let roles = service.sync_user_roles(&uow, user, &input.roles).await?;
                Ok::<_, AppError>(Json(json!({ "user_id": user, "roles": roles })))
            }
        };
        vec![
            RouteDef::on(Method::GET, "/roles", list)
                .permissions(&[MANAGE_ROLES])
                .name("access.roles.index")
                .api(api("List roles with their permissions", &[401, 403])),
            RouteDef::on(Method::POST, "/roles", create)
                .permissions(&[MANAGE_ROLES])
                .name("access.roles.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api("Create a role", &[400, 401, 403, 409])
                }),
            RouteDef::on(Method::PUT, "/roles/{role}/permissions", sync_permissions)
                .permissions(&[MANAGE_ROLES])
                .name("access.roles.permissions")
                .api(api("Replace a role's permissions", &[400, 401, 403, 404])),
            RouteDef::on(Method::DELETE, "/roles/{role}", delete)
                .permissions(&[MANAGE_ROLES])
                .name("access.roles.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Delete a role", &[401, 403, 404])
                }),
            RouteDef::on(Method::PUT, "/users/{user}/roles", sync_roles)
                .permissions(&[MANAGE_ROLES])
                .name("access.users.roles")
                .api(api("Replace a user's roles", &[400, 401, 403])),
        ]
    }
}

fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Access".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_none_match_lists_and_weak_tags_match() {
        assert!(if_none_match("\"a\"", "\"a\""));
        assert!(if_none_match("W/\"a\"", "\"a\""));
        assert!(if_none_match("\"b\", \"a\"", "\"a\""));
        assert!(if_none_match("*", "\"a\""));
        assert!(!if_none_match("\"b\"", "\"a\""));
    }

    #[test]
    fn fnv1a_is_stable() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
    }
}
