//! Settings in requests: the [`Setting<T>`] extractor.

use std::fmt;
use std::ops::Deref;

use axum::extract::FromRequestParts;
use http::request::Parts;
use navree_contracts::AuthState;
use navree_http::AppError;
use navree_kernel::AppContext;
use navree_workspace::{CurrentWorkspace, WorkspaceConfig};
use uuid::Uuid;

use crate::{Resolved, Scope, ScopeKind, Settings, SettingsGroup, Source};

/// Settings `T` resolved for the current request: the signed-in user (when
/// `T` has the user level) in the request's workspace (when `T` has the
/// workspace or organization level, the app has workspaces and the request
/// names one), on top of that workspace's organization and the global
/// values. Derefs to `T`.
///
/// ```ignore
/// #[get("/portal")]
/// #[authenticated]
/// async fn portal(&self, portal: Setting<PortalSettings>) -> Json<PortalPage> {
///     Json(PortalPage {
///         leaderboard: portal.show_leaderboard.then(|| self.leaderboard()),
///         welcome: portal.welcome_text.clone(),
///     })
/// }
/// ```
///
/// A request that names no workspace resolves without one (the user's and
/// the global values). One that names a workspace the user may not enter
/// is refused as `CurrentWorkspace` refuses it (404, 403). A guest's
/// settings are the global values. The extractor reads for your handler's
/// logic and checks no permission; it answers 500 when `SettingsModule` is
/// not in the app.
pub struct Setting<T: SettingsGroup>(Resolved<T>);

impl<T: SettingsGroup> Setting<T> {
    /// The resolved settings, with their sources.
    pub fn resolved(&self) -> &Resolved<T> {
        &self.0
    }

    /// Where `field` came from.
    pub fn source(&self, field: &str) -> Option<Source> {
        self.0.source(field)
    }

    /// Who the settings were resolved for.
    pub fn scope(&self) -> &Scope {
        &self.0.scope
    }

    /// The resolved settings, with their sources.
    pub fn into_resolved(self) -> Resolved<T> {
        self.0
    }

    /// The settings.
    pub fn into_inner(self) -> T {
        self.0.value
    }
}

impl<T: SettingsGroup> Deref for Setting<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0.value
    }
}

impl<T: SettingsGroup> fmt::Debug for Setting<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Setting")
            .field("key", &T::KEY)
            .field("scope", &self.0.scope)
            .field("sources", &self.0.sources)
            .finish_non_exhaustive()
    }
}

impl<T: SettingsGroup, S: Send + Sync> FromRequestParts<S> for Setting<T> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = app_context(parts)?;
        let settings = ctx
            .get::<Settings>()
            .await
            .map_err(|_| AppError::internal("Setting<T> needs SettingsModule in the app"))?;
        let scope = request_scope(parts, &ctx, T::SCOPES).await?;
        let resolved = settings.resolve::<T>(scope).await.map_err(|error| {
            error
                .to_app_error()
                .unwrap_or_else(|| AppError::internal(error))
        })?;
        Ok(Setting(resolved))
    }
}

/// The request's `AppContext`.
pub(crate) fn app_context(parts: &Parts) -> Result<AppContext, AppError> {
    parts
        .extensions
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal("navree-settings needs the navree request pipeline"))
}

/// The signed-in user, if any.
pub(crate) fn request_user(parts: &Parts) -> Option<Uuid> {
    parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
        .map(|principal| principal.user_id)
}

/// The request's workspace when the app has workspaces and the request
/// names one; `Ok(None)` when it names none; the refusal of
/// `CurrentWorkspace` for an unknown workspace or a non-member.
pub(crate) async fn request_workspace(
    parts: &mut Parts,
    ctx: &AppContext,
) -> Result<Option<CurrentWorkspace>, AppError> {
    if let Some(current) = parts.extensions.get::<CurrentWorkspace>() {
        return Ok(Some(current.clone()));
    }
    if ctx.try_get::<WorkspaceConfig>().await.is_none() {
        return Ok(None);
    }
    match CurrentWorkspace::from_request_parts(parts, &()).await {
        Ok(current) => Ok(Some(current)),
        Err(error) if error.code == "workspace_required" => Ok(None),
        Err(error) => Err(error),
    }
}

/// The scope a request resolves settings with `levels` for: its user (with
/// the user level), its workspace and that workspace's organization (with
/// either level), else the global level.
pub(crate) async fn request_scope(
    parts: &mut Parts,
    ctx: &AppContext,
    levels: &[ScopeKind],
) -> Result<Scope, AppError> {
    let Some(user) = request_user(parts) else {
        return Ok(Scope::global());
    };
    let mut scope = if levels.contains(&ScopeKind::User) {
        Scope::user(user)
    } else {
        Scope::global()
    };
    let wants_workspace =
        levels.contains(&ScopeKind::Workspace) || levels.contains(&ScopeKind::Organization);
    if wants_workspace && let Some(current) = request_workspace(parts, ctx).await? {
        scope = scope
            .in_workspace(current.id())
            .in_organization(current.workspace().organization_id);
    }
    Ok(scope)
}
