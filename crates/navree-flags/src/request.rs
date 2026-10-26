//! Flags in requests: the [`CurrentFlags`] extractor, the [`RequireFlag`]
//! guard and [`requires_flag`] (what `#[requires_flag(F)]` expands to).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::FromRequestParts;
use http::StatusCode;
use http::request::Parts;
use navree_contracts::AuthState;
use navree_http::{AppError, Guard, RouteDef};
use navree_kernel::{AppContext, BoxFuture};
use navree_workspace::{CurrentWorkspace, WorkspaceConfig};

use crate::{Flag, FlagDefinition, FlagScope, Flags, FlagsError};

/// The OpenAPI extension naming the flag a route requires.
pub const FLAG_EXTENSION: &str = "x-navree-flag";

/// The flags of the current request: resolved for the signed-in user and,
/// when the app has workspaces and the request names one the user belongs
/// to, the request's [`CurrentWorkspace`]. Results are memoized for the
/// request (shared with `#[requires_flag]`).
///
/// ```ignore
/// #[get("/board")]
/// #[can(Read, Deal)]
/// async fn board(&self, uow: Uow, flags: CurrentFlags) -> Result<Json<DealBoard>> {
///     let board = if flags.active::<NewPipeline>().await? {
///         self.deals.board_v2(&uow).await?
///     } else {
///         self.deals.board(&uow).await?
///     };
///     Ok(Json(board))
/// }
/// ```
///
/// A guest's flags are resolved for no user and no workspace (see
/// `flags.missing_scope`). The extractor answers 500 when `FlagsModule` is
/// not in the app.
#[derive(Clone)]
pub struct CurrentFlags {
    flags: Arc<Flags>,
    scope: FlagScope,
    memo: Arc<Mutex<HashMap<&'static str, bool>>>,
}

impl CurrentFlags {
    /// Whether `F` is on for the request.
    pub async fn active<F: Flag>(&self) -> Result<bool, FlagsError> {
        self.check(&FlagDefinition::of::<F>()).await
    }

    /// Every registered flag's value for the request, by name.
    pub async fn all(&self) -> Result<BTreeMap<String, bool>, FlagsError> {
        let values = self.flags.all(self.scope).await?;
        Ok(values)
    }

    /// Who the request's flags are resolved for.
    pub fn scope(&self) -> &FlagScope {
        &self.scope
    }

    /// The `Flags` service.
    pub fn flags(&self) -> &Arc<Flags> {
        &self.flags
    }

    pub(crate) async fn check(&self, definition: &FlagDefinition) -> Result<bool, FlagsError> {
        let memoized = self
            .memo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(definition.name)
            .copied();
        if let Some(value) = memoized {
            return Ok(value);
        }
        let value = self.flags.check(definition, &self.scope).await?;
        self.memo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(definition.name, value);
        Ok(value)
    }

    /// The request's flags, resolved once per request.
    pub(crate) async fn of_request(parts: &mut Parts) -> Result<CurrentFlags, AppError> {
        if let Some(current) = parts.extensions.get::<CurrentFlags>() {
            return Ok(current.clone());
        }
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("CurrentFlags needs the navree request pipeline"))?;
        let flags = ctx
            .get::<Flags>()
            .await
            .map_err(|_| AppError::internal("CurrentFlags needs FlagsModule in the app"))?;
        let user = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id);
        let mut scope = FlagScope::everyone();
        if let Some(user) = user {
            scope = scope.with_user(user);
            if let Some(workspace) = request_workspace(parts, &ctx).await {
                scope = scope.with_workspace(workspace.id());
            }
        }
        let current = CurrentFlags {
            flags,
            scope,
            memo: Arc::default(),
        };
        parts.extensions.insert(current.clone());
        Ok(current)
    }
}

/// The request's workspace when the app has workspaces and the request
/// names one the signed-in user belongs to; `None` otherwise (no
/// workspace named, an unknown one, or not a member).
async fn request_workspace(parts: &mut Parts, ctx: &AppContext) -> Option<CurrentWorkspace> {
    if let Some(current) = parts.extensions.get::<CurrentWorkspace>() {
        return Some(current.clone());
    }
    ctx.try_get::<WorkspaceConfig>().await?;
    CurrentWorkspace::from_request_parts(parts, &()).await.ok()
}

impl fmt::Debug for CurrentFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentFlags")
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentFlags {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        CurrentFlags::of_request(parts).await
    }
}

/// A route guard: the route exists only while flag `F` is on for the
/// request (its user and workspace, as [`CurrentFlags`] resolves them).
/// Off, it answers 404 Not Found (or the status it was built with) before
/// the route's authorization, extractors and handler run.
///
/// `#[requires_flag(F)]` adds it; by hand, use [`requires_flag`] (which
/// also documents the flag in OpenAPI) or the guard itself:
///
/// ```ignore
/// RouteDef::on(Method::GET, "/forecast", forecast)
///     .can("read", "Deal", None)
///     .guard(RequireFlag::guard::<NewPipeline>());
/// ```
#[derive(Clone)]
pub struct RequireFlag {
    flag: FlagDefinition,
    status: StatusCode,
}

impl RequireFlag {
    /// The guard for `F`, answering 404 while it is off.
    pub fn guard<F: Flag>() -> Arc<dyn Guard> {
        Self::with_status::<F>(StatusCode::NOT_FOUND)
    }

    /// The guard for `F`, answering `status` while it is off.
    pub fn with_status<F: Flag>(status: StatusCode) -> Arc<dyn Guard> {
        Arc::new(RequireFlag {
            flag: FlagDefinition::of::<F>(),
            status,
        })
    }

    /// The flag it requires.
    pub fn flag(&self) -> &FlagDefinition {
        &self.flag
    }
}

impl fmt::Debug for RequireFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequireFlag")
            .field("flag", &self.flag.name)
            .field("status", &self.status.as_u16())
            .finish()
    }
}

impl Guard for RequireFlag {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let flags = CurrentFlags::of_request(parts).await?;
            match flags.check(&self.flag).await {
                Ok(true) => Ok(()),
                Ok(false) => Err(AppError::from_status(self.status)),
                Err(error) => Err(error
                    .to_app_error()
                    .unwrap_or_else(|| AppError::internal(error))),
            }
        })
    }
}

/// `route`, existing only while flag `F` is on: adds the [`RequireFlag`]
/// guard (answering `status`, 404 or 403, while the flag is off) and the
/// OpenAPI extension `x-navree-flag: <name>`. What `#[requires_flag(F)]`
/// and `#[requires_flag(F, status = 403)]` expand to.
///
/// # Panics
///
/// When `status` is not a 4xx status code.
pub fn requires_flag<F: Flag>(route: RouteDef, status: u16) -> RouteDef {
    let status = StatusCode::from_u16(status)
        .ok()
        .filter(StatusCode::is_client_error)
        .unwrap_or_else(|| panic!("#[requires_flag]: `{status}` is not a 4xx status code"));
    route
        .guard(RequireFlag::with_status::<F>(status))
        .extension(FLAG_EXTENSION, F::NAME)
}
