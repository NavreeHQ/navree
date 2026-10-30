//! The routes: `GET /api/settings` and `GET`, `PATCH` and `DELETE
//! /api/settings/{key}`.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, Query};
use http::Method;
use http::request::Parts;
use navree_access::AbilityFactory;
use navree_db::Uow;
use navree_http::extract::FromRequestParts as _;
use navree_http::{ApiMeta, AppError, Controller, Json, RouteDef, schema_fn};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_workspace::{CurrentOrganization, CurrentWorkspace};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use crate::registry::RegisteredSettings;
use crate::request::{app_context, request_user, request_workspace};
use crate::{ResolvedSettings, Scope, ScopeKind, Settings, SettingsError, SettingsOptions};

/// The permission a global write needs when the settings name none.
pub const SETTINGS_MANAGE_PERMISSION: &str = "settings.manage";
/// The permission an organization write needs when the settings name none.
pub const ORGANIZATION_WRITE_PERMISSION: &str = "organization.update";
/// The permission a workspace write needs when the settings name none.
pub const WORKSPACE_WRITE_PERMISSION: &str = "workspace.update";

/// The OpenAPI tag of the routes.
const TAG: &str = "Settings";

/// A registered settings type, as `GET /api/settings` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SettingsInfo {
    /// The key (`portal`).
    pub key: String,
    /// What the settings are for.
    pub description: String,
    /// The levels they may be stored at, from the most general.
    pub scopes: Vec<ScopeKind>,
    /// Their fields.
    pub fields: Vec<String>,
}

impl SettingsInfo {
    /// The info of `entry`.
    pub fn of(entry: &RegisteredSettings) -> Self {
        SettingsInfo {
            key: entry.definition.key.to_owned(),
            description: entry.definition.description.to_owned(),
            scopes: entry.definition.levels(),
            fields: entry.fields.iter().cloned().collect(),
        }
    }
}

/// `{"data": [...]}`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SettingsListResource {
    /// Every registered settings type, by key.
    pub data: Vec<SettingsInfo>,
}

/// The query of `/api/settings/{key}`: the level to read or write.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ScopeQuery {
    /// `global`, `organization` (the request's `X-Organization`),
    /// `workspace` (the request's `X-Workspace`) or `user` (the signed-in
    /// user); default: the most specific level the settings allow.
    pub scope: Option<String>,
}

/// The body of `PATCH /api/settings/{key}`: the fields to set.
#[derive(ToSchema)]
#[allow(dead_code)]
struct SettingsPatch(BTreeMap<String, Value>);

/// `{api_prefix}/settings...` (unless `settings.routes` is false), for
/// signed-in users:
///
/// | Route | Answer |
/// |---|---|
/// | `GET /settings` | every registered settings type: `{data: [{key, description, scopes, fields}]}` |
/// | `GET /settings/{key}?scope=` | `{data, sources, overridden}` |
/// | `PATCH /settings/{key}?scope=` `{field: value, ..}` | the result, `{data, sources, overridden}` |
/// | `DELETE /settings/{key}?scope=` | every field at the level inherits again; the result |
/// | `DELETE /settings/{key}/{field}?scope=` | the field inherits again; the result |
///
/// `scope` picks the level: `global`, `organization` (the request's
/// organization, `X-Organization`), `workspace` (the request's workspace,
/// `X-Workspace`) or `user` (the signed-in user, over the request's
/// workspace when it names one). Without it, the most specific level the
/// settings allow.
///
/// Reads need what the scope needs (a member of the workspace or
/// organization) plus the settings' read permission, if they name one.
/// Writes need the settings' permission, or by default `settings.manage`
/// (global, checked with navree-access), `organization.update` (in the
/// organization) or `workspace.update` (in the workspace); users change
/// their own values. An unknown key is a 404.
pub struct SettingsController {
    settings: Arc<Settings>,
    options: Arc<SettingsOptions>,
}

impl Injectable for SettingsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Settings>(),
            ProviderKey::of::<SettingsOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SettingsController {
                settings: r.get::<Settings>().await?,
                options: r.get::<SettingsOptions>().await?,
            })
        })
    }
}

fn api(summary: &str, responses: Vec<u16>) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec![TAG.to_owned()],
        responses,
        response: Some(schema_fn::<ResolvedSettings>),
        ..ApiMeta::default()
    }
}

impl Controller for SettingsController {
    fn prefix(&self) -> &'static str {
        "/settings"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.routes {
            return Vec::new();
        }
        let (index, show, update, reset, inherit) = (
            self.clone(),
            self.clone(),
            self.clone(),
            self.clone(),
            self.clone(),
        );
        vec![
            RouteDef::on(Method::GET, "/", move || {
                let this = index.clone();
                async move { this.index() }
            })
            .authenticated()
            .name("settings.index")
            .api(ApiMeta {
                summary: Some("List the registered settings".to_owned()),
                tags: vec![TAG.to_owned()],
                response: Some(schema_fn::<SettingsListResource>),
                ..ApiMeta::default()
            }),
            RouteDef::on(
                Method::GET,
                "/{key}",
                move |parts: Parts, Path(key): Path<String>, Query(query): Query<ScopeQuery>| {
                    let this = show.clone();
                    async move { this.show(parts, &key, query).await }
                },
            )
            .authenticated()
            .name("settings.show")
            .api(api(
                "Resolve settings for a scope, with the source of every value",
                vec![400, 403, 404],
            )),
            RouteDef::on(
                Method::PATCH,
                "/{key}",
                move |parts: Parts,
                      uow: Uow,
                      Path(key): Path<String>,
                      Query(query): Query<ScopeQuery>,
                      Json(patch): Json<Map<String, Value>>| {
                    let this = update.clone();
                    async move { this.update(parts, uow, &key, query, patch).await }
                },
            )
            .authenticated()
            .name("settings.update")
            .api(ApiMeta {
                request: Some(schema_fn::<SettingsPatch>),
                ..api("Set settings at a scope", vec![400, 403, 404])
            }),
            RouteDef::on(
                Method::DELETE,
                "/{key}",
                move |parts: Parts,
                      uow: Uow,
                      Path(key): Path<String>,
                      Query(query): Query<ScopeQuery>| {
                    let this = reset.clone();
                    async move { this.inherit(parts, uow, &key, None, query).await }
                },
            )
            .authenticated()
            .name("settings.reset")
            .api(api(
                "Make every field at a scope inherit again",
                vec![400, 403, 404],
            )),
            RouteDef::on(
                Method::DELETE,
                "/{key}/{field}",
                move |parts: Parts,
                      uow: Uow,
                      Path((key, field)): Path<(String, String)>,
                      Query(query): Query<ScopeQuery>| {
                    let this = inherit.clone();
                    async move { this.inherit(parts, uow, &key, Some(field), query).await }
                },
            )
            .authenticated()
            .name("settings.inherit")
            .api(api(
                "Make one field at a scope inherit again",
                vec![400, 403, 404],
            )),
        ]
    }
}

/// What a route does at a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Intent {
    Read,
    Write,
}

fn app_error(error: SettingsError) -> AppError {
    error
        .to_app_error()
        .unwrap_or_else(|| AppError::internal(error))
}

impl SettingsController {
    fn index(&self) -> Result<Json<SettingsListResource>, AppError> {
        let data = self
            .settings
            .registry()
            .all()
            .iter()
            .map(|entry| SettingsInfo::of(entry))
            .collect();
        Ok(Json(SettingsListResource { data }))
    }

    async fn show(
        &self,
        mut parts: Parts,
        key: &str,
        query: ScopeQuery,
    ) -> Result<Json<ResolvedSettings>, AppError> {
        let entry = self.settings.registry().by_key(key).map_err(app_error)?;
        let scope = route_scope(&mut parts, &entry, query, Intent::Read).await?;
        let resolved = self
            .settings
            .resolve_key(key, scope)
            .await
            .map_err(app_error)?;
        Ok(Json(resolved))
    }

    async fn update(
        &self,
        mut parts: Parts,
        uow: Uow,
        key: &str,
        query: ScopeQuery,
        patch: Map<String, Value>,
    ) -> Result<Json<ResolvedSettings>, AppError> {
        let entry = self.settings.registry().by_key(key).map_err(app_error)?;
        let scope = route_scope(&mut parts, &entry, query, Intent::Write).await?;
        let resolved = self
            .settings
            .update_key(&uow, key, scope, patch)
            .await
            .map_err(app_error)?;
        Ok(Json(resolved))
    }

    async fn inherit(
        &self,
        mut parts: Parts,
        uow: Uow,
        key: &str,
        field: Option<String>,
        query: ScopeQuery,
    ) -> Result<Json<ResolvedSettings>, AppError> {
        let entry = self.settings.registry().by_key(key).map_err(app_error)?;
        let scope = route_scope(&mut parts, &entry, query, Intent::Write).await?;
        let resolved = match field {
            Some(field) => self.settings.inherit_key(&uow, key, scope, &[field]).await,
            None => self.settings.reset_key(&uow, key, scope).await,
        }
        .map_err(app_error)?;
        Ok(Json(resolved))
    }
}

/// The level `query` names for `entry`, or the most specific it allows.
fn level_of(entry: &RegisteredSettings, query: &ScopeQuery) -> Result<ScopeKind, AppError> {
    let level = match query.scope.as_deref() {
        Some(raw) => raw.parse::<ScopeKind>().map_err(|message| {
            AppError::validation(BTreeMap::from([("scope".to_owned(), vec![message])]))
        })?,
        None => entry
            .definition
            .levels()
            .last()
            .copied()
            .unwrap_or(ScopeKind::Global),
    };
    if !entry.definition.allows(level) {
        return Err(app_error(SettingsError::ScopeNotAllowed {
            key: entry.definition.key.to_owned(),
            scope: level,
            allowed: entry.definition.levels_list(),
        }));
    }
    Ok(level)
}

/// The permission `intent` needs at `level` for `entry`, if any.
fn permission(
    entry: &RegisteredSettings,
    level: ScopeKind,
    intent: Intent,
) -> Option<&'static str> {
    match (intent, level) {
        (_, ScopeKind::User) => None,
        (Intent::Read, _) => entry.definition.read_permission,
        (Intent::Write, ScopeKind::Global) => Some(
            entry
                .definition
                .permission
                .unwrap_or(SETTINGS_MANAGE_PERMISSION),
        ),
        (Intent::Write, ScopeKind::Organization) => Some(
            entry
                .definition
                .permission
                .unwrap_or(ORGANIZATION_WRITE_PERMISSION),
        ),
        (Intent::Write, ScopeKind::Workspace) => Some(
            entry
                .definition
                .permission
                .unwrap_or(WORKSPACE_WRITE_PERMISSION),
        ),
    }
}

/// Whether the signed-in user holds `permission` app-wide (navree-access;
/// refused without `AccessModule`).
async fn authorize_globally(
    parts: &Parts,
    ctx: &AppContext,
    permission: &str,
) -> Result<(), AppError> {
    let Some(factory) = ctx.try_get::<AbilityFactory>().await else {
        return Err(AppError::forbidden());
    };
    let principal = parts
        .extensions
        .get::<navree_contracts::AuthState>()
        .and_then(|state| state.0.clone())
        .ok_or_else(AppError::unauthenticated)?;
    let access = factory
        .access_for(Some(&principal))
        .await
        .map_err(AppError::internal)?;
    let (action, subject) = factory.permission_target(permission);
    if access.ability.has_unconditional(action, subject) {
        Ok(())
    } else {
        Err(AppError::forbidden())
    }
}

/// The scope a route reads or writes, checked for the signed-in user.
async fn route_scope(
    parts: &mut Parts,
    entry: &RegisteredSettings,
    query: ScopeQuery,
    intent: Intent,
) -> Result<Scope, AppError> {
    let ctx = app_context(parts)?;
    let user = request_user(parts).ok_or_else(AppError::unauthenticated)?;
    let level = level_of(entry, &query)?;
    let needs = permission(entry, level, intent);
    match level {
        ScopeKind::Global => {
            if let Some(permission) = needs {
                authorize_globally(parts, &ctx, permission).await?;
            }
            Ok(Scope::global())
        }
        ScopeKind::Organization => {
            let organization = CurrentOrganization::from_request_parts(parts).await?;
            if let Some(permission) = needs {
                organization.authorize(permission)?;
            }
            Ok(Scope::from(&organization))
        }
        ScopeKind::Workspace => {
            let workspace = CurrentWorkspace::from_request_parts(parts).await?;
            if let Some(permission) = needs {
                workspace.authorize(permission)?;
            }
            Ok(Scope::from(&workspace))
        }
        ScopeKind::User => {
            let mut scope = Scope::user(user);
            if let Some(workspace) = request_workspace(parts, &ctx).await? {
                scope = scope
                    .in_workspace(workspace.id())
                    .in_organization(workspace.workspace().organization_id);
            }
            Ok(scope)
        }
    }
}
