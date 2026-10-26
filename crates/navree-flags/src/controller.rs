//! The routes: `GET /api/flags` for the front end and `/api/admin/flags`
//! for admin screens.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use http::header::{CACHE_CONTROL, HeaderValue};
use http::{Method, StatusCode};
use navree_http::{ApiMeta, AppError, Controller, Json, NoContent, RouteDef, schema_fn};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::request::CurrentFlags;
use crate::service::Change;
use crate::{
    FlagDefinition, FlagOverride, FlagRegistry, FlagState, Flags, FlagsError, FlagsOptions,
    Rollout, ScopeType,
};

/// The permission the admin routes require.
pub const FLAGS_MANAGE_PERMISSION: &str = "flags.manage";

/// The OpenAPI tag of the routes.
const TAG: &str = "Flags";

/// A flag and its stored state, as the admin routes return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct FlagResource {
    /// The flag's name.
    pub name: String,
    /// What it is for.
    pub description: String,
    /// The value when nothing else decides.
    pub default: bool,
    /// Whether `GET /api/flags` serves it.
    pub expose: bool,
    /// On or off for everyone; `null` when unset.
    pub everyone: Option<bool>,
    /// The rollout, if any.
    pub rollout: Option<Rollout>,
    /// Whether the kill switch is on.
    pub killed: bool,
    /// How many users and workspaces have an override.
    pub overrides: u64,
    /// When the state last changed.
    pub updated_at: Option<DateTime<Utc>>,
    /// Who changed it last.
    pub updated_by: Option<Uuid>,
}

impl FlagResource {
    /// The resource of `definition` with `state` and `overrides` overrides.
    pub fn new(definition: &FlagDefinition, state: &FlagState, overrides: u64) -> Self {
        FlagResource {
            name: definition.name.to_owned(),
            description: definition.description.to_owned(),
            default: definition.default,
            expose: definition.expose,
            everyone: state.everyone,
            rollout: state.rollout.clone(),
            killed: state.killed,
            overrides,
            updated_at: state.updated_at,
            updated_by: state.updated_by,
        }
    }
}

/// One flag with its overrides (`GET /api/admin/flags/{name}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct FlagDetailsResource {
    /// The flag and its state.
    pub flag: FlagResource,
    /// Its overrides, users first.
    pub overrides: Vec<FlagOverride>,
}

/// `{"data": [...]}`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct FlagListResource {
    /// Every registered flag, by name.
    pub data: Vec<FlagResource>,
}

/// Deserializes a present field (`null` included) as `Some`, so an absent
/// field stays `None`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// The body of `PUT /api/admin/flags/{name}`: the fields present change.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateFlagInput {
    /// On (`true`) or off (`false`) for everyone, or `null` to unset.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<bool>)]
    pub everyone: Option<Option<bool>>,
    /// A rollout (`{"percent": 25, "of": "workspaces", "plans": ["pro"]}`),
    /// or `null` to stop it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<Rollout>)]
    pub rollout: Option<Option<Rollout>>,
    /// The kill switch.
    pub killed: Option<bool>,
}

/// The body of `PUT /api/admin/flags/{name}/overrides`: one of `user_id`
/// and `workspace_id`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OverrideInput {
    /// The user.
    pub user_id: Option<Uuid>,
    /// The workspace.
    pub workspace_id: Option<Uuid>,
    /// On or off.
    pub active: bool,
}

/// The query of `DELETE /api/admin/flags/{name}/overrides`: one of
/// `user_id` and `workspace_id`.
#[derive(Clone, Debug, Deserialize)]
pub struct OverrideTarget {
    /// The user.
    pub user_id: Option<Uuid>,
    /// The workspace.
    pub workspace_id: Option<Uuid>,
}

impl OverrideTarget {
    fn target(&self) -> Result<(ScopeType, Uuid), AppError> {
        match (self.user_id, self.workspace_id) {
            (Some(user), None) => Ok((ScopeType::User, user)),
            (None, Some(workspace)) => Ok((ScopeType::Workspace, workspace)),
            _ => Err(AppError::validation(BTreeMap::from([(
                "user_id".to_owned(),
                vec!["exactly one of user_id and workspace_id must be given".to_owned()],
            )]))),
        }
    }
}

/// `GET {api_prefix}/flags`: the exposed flags, resolved for the request's
/// user and workspace (unless `flags.routes` is false).
pub struct FlagsController {
    registry: Arc<FlagRegistry>,
    options: Arc<FlagsOptions>,
}

impl Injectable for FlagsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<FlagRegistry>(),
            ProviderKey::of::<FlagsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(FlagsController {
                registry: r.get::<FlagRegistry>().await?,
                options: r.get::<FlagsOptions>().await?,
            })
        })
    }
}

/// The schema of `GET /api/flags`: flag name → value.
#[derive(ToSchema)]
#[allow(dead_code)]
struct ExposedFlags(BTreeMap<String, bool>);

impl Controller for FlagsController {
    fn prefix(&self) -> &'static str {
        "/flags"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.routes {
            return Vec::new();
        }
        let this = self.clone();
        vec![
            RouteDef::on(Method::GET, "/", move |flags: CurrentFlags| {
                let this = this.clone();
                async move { this.index(flags).await }
            })
            .authenticated()
            .name("flags.index")
            .api(ApiMeta {
                summary: Some("The flags the front end may read, for the caller".to_owned()),
                tags: vec![TAG.to_owned()],
                response: Some(schema_fn::<ExposedFlags>),
                ..ApiMeta::default()
            }),
        ]
    }
}

impl FlagsController {
    async fn index(&self, flags: CurrentFlags) -> Result<Response, AppError> {
        let mut values = BTreeMap::new();
        for definition in self.registry.iter().filter(|definition| definition.expose) {
            values.insert(definition.name.to_owned(), flags.check(definition).await?);
        }
        let mut response = Json(values).into_response();
        let cache_control = format!("private, max-age={}", self.options.cache_ttl_secs);
        if let Ok(value) = HeaderValue::from_str(&cache_control) {
            response.headers_mut().insert(CACHE_CONTROL, value);
        }
        Ok(response)
    }
}

/// `{api_prefix}/admin/flags...`, with the `flags.manage` permission
/// (unless `flags.admin_routes` is false):
///
/// | Route | Answer |
/// |---|---|
/// | `GET /admin/flags` | every registered flag and its state, `{data: [..]}` |
/// | `GET /admin/flags/{name}` | the flag and its overrides |
/// | `PUT /admin/flags/{name}` `{everyone?, rollout?, killed?}` | the flag and its overrides |
/// | `PUT /admin/flags/{name}/overrides` `{user_id \| workspace_id, active}` | the flag and its overrides |
/// | `DELETE /admin/flags/{name}/overrides?user_id=..` (or `workspace_id`) | 204 |
///
/// An unknown flag is a 404.
pub struct FlagsAdminController {
    flags: Arc<Flags>,
    registry: Arc<FlagRegistry>,
    options: Arc<FlagsOptions>,
}

impl Injectable for FlagsAdminController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Flags>(),
            ProviderKey::of::<FlagRegistry>(),
            ProviderKey::of::<FlagsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(FlagsAdminController {
                flags: r.get::<Flags>().await?,
                registry: r.get::<FlagRegistry>().await?,
                options: r.get::<FlagsOptions>().await?,
            })
        })
    }
}

fn admin_api(summary: &str, responses: Vec<u16>) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec![TAG.to_owned()],
        responses,
        ..ApiMeta::default()
    }
}

impl Controller for FlagsAdminController {
    fn prefix(&self) -> &'static str {
        "/admin/flags"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.admin_routes {
            return Vec::new();
        }
        let (index, show, update, put_override, delete_override) = (
            self.clone(),
            self.clone(),
            self.clone(),
            self.clone(),
            self.clone(),
        );
        vec![
            RouteDef::on(Method::GET, "/", move || {
                let this = index.clone();
                async move { this.index().await }
            })
            .permissions(&[FLAGS_MANAGE_PERMISSION])
            .name("flags.admin.index")
            .api(ApiMeta {
                response: Some(schema_fn::<FlagListResource>),
                ..admin_api("List every flag and its state", vec![])
            }),
            RouteDef::on(Method::GET, "/{name}", move |Path(name): Path<String>| {
                let this = show.clone();
                async move { this.show(&name).await }
            })
            .permissions(&[FLAGS_MANAGE_PERMISSION])
            .name("flags.admin.show")
            .api(ApiMeta {
                response: Some(schema_fn::<FlagDetailsResource>),
                ..admin_api("Show a flag and its overrides", vec![404])
            }),
            RouteDef::on(
                Method::PUT,
                "/{name}",
                move |Path(name): Path<String>, Json(input): Json<UpdateFlagInput>| {
                    let this = update.clone();
                    async move { this.update(&name, input).await }
                },
            )
            .permissions(&[FLAGS_MANAGE_PERMISSION])
            .name("flags.admin.update")
            .api(ApiMeta {
                request: Some(schema_fn::<UpdateFlagInput>),
                response: Some(schema_fn::<FlagDetailsResource>),
                ..admin_api(
                    "Set a flag's global state, rollout or kill switch",
                    vec![400, 404],
                )
            }),
            RouteDef::on(
                Method::PUT,
                "/{name}/overrides",
                move |Path(name): Path<String>, Json(input): Json<OverrideInput>| {
                    let this = put_override.clone();
                    async move { this.put_override(&name, input).await }
                },
            )
            .permissions(&[FLAGS_MANAGE_PERMISSION])
            .name("flags.admin.overrides.update")
            .api(ApiMeta {
                request: Some(schema_fn::<OverrideInput>),
                response: Some(schema_fn::<FlagDetailsResource>),
                ..admin_api(
                    "Turn a flag on or off for a user or workspace",
                    vec![400, 404],
                )
            }),
            RouteDef::on(
                Method::DELETE,
                "/{name}/overrides",
                move |Path(name): Path<String>, Query(target): Query<OverrideTarget>| {
                    let this = delete_override.clone();
                    async move { this.delete_override(&name, target).await }
                },
            )
            .permissions(&[FLAGS_MANAGE_PERMISSION])
            .name("flags.admin.overrides.delete")
            .api(ApiMeta {
                success_status: StatusCode::NO_CONTENT.as_u16(),
                ..admin_api("Remove a user's or workspace's override", vec![400, 404])
            }),
        ]
    }
}

impl FlagsAdminController {
    async fn index(&self) -> Result<Json<FlagListResource>, AppError> {
        let states = self.flags.stored_states().await?;
        let counts = self.flags.override_counts().await?;
        let data = self
            .registry
            .iter()
            .map(|definition| {
                let state = states
                    .get(definition.name)
                    .cloned()
                    .unwrap_or_else(|| FlagState::unset(definition.name));
                let overrides = counts.get(definition.name).copied().unwrap_or(0);
                FlagResource::new(definition, &state, overrides)
            })
            .collect();
        Ok(Json(FlagListResource { data }))
    }

    async fn details(
        &self,
        definition: &FlagDefinition,
    ) -> Result<FlagDetailsResource, FlagsError> {
        let state = self.flags.state_of(definition).await?;
        let overrides = self.flags.overrides(definition).await?;
        let count = u64::try_from(overrides.len()).unwrap_or(u64::MAX);
        Ok(FlagDetailsResource {
            flag: FlagResource::new(definition, &state, count),
            overrides,
        })
    }

    async fn show(&self, name: &str) -> Result<Json<FlagDetailsResource>, AppError> {
        let definition = self.flags.definition(name)?;
        Ok(Json(self.details(&definition).await?))
    }

    async fn update(
        &self,
        name: &str,
        input: UpdateFlagInput,
    ) -> Result<Json<FlagDetailsResource>, AppError> {
        let definition = self.flags.definition(name)?;
        if let Some(everyone) = input.everyone {
            self.flags
                .change(&definition, Change::Everyone(everyone))
                .await?;
        }
        if let Some(rollout) = input.rollout {
            self.flags.set_rollout(&definition, rollout).await?;
        }
        if let Some(killed) = input.killed {
            self.flags
                .change(&definition, Change::Killed(killed))
                .await?;
        }
        Ok(Json(self.details(&definition).await?))
    }

    async fn put_override(
        &self,
        name: &str,
        input: OverrideInput,
    ) -> Result<Json<FlagDetailsResource>, AppError> {
        let definition = self.flags.definition(name)?;
        let (scope_type, id) = OverrideTarget {
            user_id: input.user_id,
            workspace_id: input.workspace_id,
        }
        .target()?;
        self.flags
            .change(
                &definition,
                Change::Override(scope_type, id, Some(input.active)),
            )
            .await?;
        Ok(Json(self.details(&definition).await?))
    }

    async fn delete_override(
        &self,
        name: &str,
        target: OverrideTarget,
    ) -> Result<NoContent, AppError> {
        let definition = self.flags.definition(name)?;
        let (scope_type, id) = target.target()?;
        self.flags
            .change(&definition, Change::Override(scope_type, id, None))
            .await?;
        Ok(NoContent)
    }
}
