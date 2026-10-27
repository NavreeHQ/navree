//! [`CurrentWorkspace`], the [`InWorkspace`] guard and [`WorkspaceUow`]:
//! resolving the workspace of a request and checking the membership.

use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path};
use http::request::Parts;
use navree_access::{Ability, PrincipalAccess};
use navree_contracts::{AuthState, RequestId};
use navree_db::Uow;
use navree_http::{AppError, Guard, RouteMeta};
use navree_kernel::{AppContext, BoxFuture};
use navree_telemetry::TelemetryContext;
use sea_orm::{
    ColumnTrait, ColumnType, DbErr, IntoActiveModel, Iterable, PrimaryKeyToColumn, QueryFilter,
    Select, Value,
};
use serde::Serialize;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::access::{permission_target, subjects_of, workspace_access};
use crate::demo::DemoIntent;
use crate::entity::{Member, Workspace, member};
use crate::isolation::{WorkspaceRepo, WorkspaceScoped};
use crate::resolve;
use crate::tenant::WorkspaceDatabases;
use crate::{
    DemoMode, Isolation, ResolverKind, RoleTable, WorkspaceConfig, WorkspaceError, WorkspaceRole,
    Workspaces,
};

/// The workspace of the current request, resolved, loaded and checked:
/// the signed-in user is a member of it.
///
/// As an extractor it resolves the workspace with the configured resolvers
/// (`workspace.resolvers`: header, path, subdomain, claim) unless a guard
/// already did; it answers 401 for guests, 400 when the request names no
/// workspace, 404 for an unknown one and 403 when the user is not a member
/// (in Navree's JSON error shape). It derefs to the [`Workspace`].
///
/// ```ignore
/// async fn index(ws: CurrentWorkspace, uow: Uow) -> Result<Json<Vec<deal::Model>>> {
///     ws.authorize("deals.read")?;
///     let deals = WorkspaceRepo::<deal::Entity>::select(ws.id()).all(&*uow.tx().await?).await?;
///     Ok(Json(deals))
/// }
/// ```
///
/// Route-level access checks (`#[roles]`, `#[permissions]`, `#[can]`) run
/// before extractors; to evaluate them against the member's workspace role,
/// add the [`InWorkspace`] guard to the route or controller.
#[derive(Clone)]
pub struct CurrentWorkspace(Arc<Resolved>);

struct Resolved {
    workspace: Workspace,
    member: Member,
    role: WorkspaceRole,
    access: Arc<PrincipalAccess>,
    subjects: Arc<std::collections::BTreeMap<String, String>>,
    isolation: Isolation,
    resolver: Option<ResolverKind>,
    demo: Option<DemoMode>,
    guest: bool,
}

impl CurrentWorkspace {
    /// The workspace id.
    pub fn id(&self) -> Uuid {
        self.0.workspace.id
    }

    /// The workspace.
    pub fn workspace(&self) -> &Workspace {
        &self.0.workspace
    }

    /// The signed-in user's membership.
    pub fn member(&self) -> &Member {
        &self.0.member
    }

    /// The signed-in user's role in the workspace.
    pub fn role(&self) -> &WorkspaceRole {
        &self.0.role
    }

    /// The role's permissions (built-in plus `workspace.roles`), sorted.
    pub fn permissions(&self) -> &[String] {
        &self.0.access.permissions
    }

    /// The user's abilities inside this workspace (navree-access rules from
    /// the app's abilities function and the role's permissions).
    pub fn ability(&self) -> &Arc<Ability> {
        &self.0.access.ability
    }

    /// The same, as navree-access's `PrincipalAccess` (what the
    /// [`InWorkspace`] guard hands to the route authorizer).
    pub fn access(&self) -> &Arc<PrincipalAccess> {
        &self.0.access
    }

    /// Whether the user may do `action` on `subject` in this workspace
    /// (a type-level check: conditions are not evaluated).
    pub fn can(&self, action: &str, subject: &str) -> bool {
        self.0.access.ability.can(action, subject, None)
    }

    /// Whether the user holds `permission` (`<prefix>.<action>`, as route
    /// `permissions` declarations check it) in this workspace.
    pub fn has_permission(&self, permission: &str) -> bool {
        let (action, subject) = permission_target(permission, &self.0.subjects);
        self.0.access.ability.has_unconditional(action, subject)
    }

    /// `Ok` when the user holds `permission` in this workspace, else a 403.
    pub fn authorize(&self, permission: &str) -> Result<(), AppError> {
        if self.has_permission(permission) {
            Ok(())
        } else {
            Err(AppError::forbidden())
        }
    }

    /// `select` restricted to this workspace's rows (row isolation; see
    /// [`WorkspaceRepo::scope`]).
    pub fn scope<E>(&self, select: Select<E>) -> Select<E>
    where
        E: WorkspaceScoped,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync,
    {
        WorkspaceRepo::<E>::scope(self.id(), select)
    }

    /// The configured isolation strategy.
    pub fn isolation(&self) -> Isolation {
        self.0.isolation
    }

    /// The resolver that named the workspace (`None` when a route's path
    /// parameter did, through [`InWorkspace::from_path`]).
    pub fn resolver(&self) -> Option<ResolverKind> {
        self.0.resolver
    }

    /// Whether the workspace is a demo workspace.
    pub fn is_demo(&self) -> bool {
        self.0.demo.is_some()
    }

    /// `workspace.demo.mode` when the workspace is a demo workspace
    /// (`None` for a live one).
    pub fn demo_mode(&self) -> Option<DemoMode> {
        self.0.demo
    }

    /// Whether the user entered as a guest (a demo workspace with
    /// `workspace.demo.guest_role`): not a member, with the guest role.
    /// [`member`](Self::member) then describes the guest membership; it has
    /// the nil id and is stored nowhere.
    pub fn is_guest(&self) -> bool {
        self.0.guest
    }
}

impl Deref for CurrentWorkspace {
    type Target = Workspace;

    fn deref(&self) -> &Workspace {
        &self.0.workspace
    }
}

impl fmt::Debug for CurrentWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentWorkspace")
            .field("workspace", &self.0.workspace.id)
            .field("slug", &self.0.workspace.slug)
            .field("user", &self.0.member.user_id)
            .field("role", &self.0.role)
            .finish_non_exhaustive()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentWorkspace {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        resolve_request(parts, &Source::Configured).await
    }
}

/// A request extension that limits the request to one workspace: set by an
/// authentication strategy whose credential belongs to a workspace (an
/// OAuth client created for one workspace). When the request names no
/// workspace, [`CurrentWorkspace`] and [`InWorkspace`] use this one; when
/// it names another, they answer 403.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinnedWorkspace(pub Uuid);

/// Where a request's workspace identifier comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    /// The configured resolvers.
    Configured,
    /// A path parameter of the route.
    PathParam(&'static str),
}

/// Resolves, loads and checks the request's workspace once (later calls
/// reuse it), applies the isolation settings to the request's unit of work,
/// and hands the workspace-scoped access to navree-access's extractors and
/// route authorizer.
async fn resolve_request(parts: &mut Parts, source: &Source) -> Result<CurrentWorkspace, AppError> {
    if let Some(current) = parts.extensions.get::<CurrentWorkspace>() {
        return Ok(current.clone());
    }
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal("CurrentWorkspace needs the navree request pipeline"))?;
    let principal = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.clone())
        .ok_or_else(AppError::unauthenticated)?;
    let config = ctx
        .get::<WorkspaceConfig>()
        .await
        .map_err(|_| AppError::internal("CurrentWorkspace needs WorkspaceModule in the app"))?;
    let pinned = parts.extensions.get::<PinnedWorkspace>().map(|pin| pin.0);
    let (resolver, identifier) = match source {
        Source::Configured => {
            resolve::identifier(&config, parts).map(|(kind, identifier)| (Some(kind), identifier))
        }
        Source::PathParam(name) => path_param(parts, name).await?.map(|value| (None, value)),
    }
    // A pinned credential names its workspace when the request doesn't.
    .or_else(|| pinned.map(|id| (None, id.to_string())))
    .ok_or(WorkspaceError::NoWorkspace)
    .map_err(app_error)?;
    let workspaces = ctx.get::<Workspaces>().await.map_err(AppError::internal)?;
    let workspace = workspaces.resolve(&identifier).await.map_err(app_error)?;
    if pinned.is_some_and(|pin| pin != workspace.id) {
        return Err(
            AppError::forbidden().with_detail("This token is limited to another workspace.")
        );
    }
    let (member, guest) = match workspaces
        .membership(workspace.id, principal.user_id)
        .await
        .map_err(app_error)?
    {
        Some(member) => (member, false),
        None => guest_member(&config, &workspace, principal.user_id)
            .map(|member| (member, true))
            .ok_or(WorkspaceError::NotMember)
            .map_err(app_error)?,
    };
    let role = member.role();
    let demo = workspace.is_demo().then_some(config.demo.mode);
    if let Some(mode) = demo {
        check_demo_write(parts, &config, mode, &role)?;
    }
    let roles = RoleTable::from_config(&config);
    let access = workspace_access(&ctx, &principal, workspace.id, &role, &roles)
        .await
        .map_err(app_error)?;
    if config.isolation != Isolation::Database
        && let Some(uow) = parts.extensions.get::<Uow>()
    {
        workspaces
            .enter(uow, workspace.id)
            .await
            .map_err(app_error)?;
    }
    let current = CurrentWorkspace(Arc::new(Resolved {
        workspace,
        member,
        role,
        access: access.clone(),
        subjects: subjects_of(&ctx).await,
        isolation: config.isolation,
        resolver,
        demo,
        guest,
    }));
    parts.extensions.insert(current.clone());
    // The request's telemetry (`RequestHandled.workspace_id`, and the
    // context of every event the request emits from now on).
    if let Some(telemetry) = parts.extensions.get::<TelemetryContext>() {
        telemetry.set_workspace_id(current.id());
    }
    // navree-access's route authorizer and its `Abilities`, `CurrentAccess`
    // and `Accessible` extractors read this before computing global access.
    parts.extensions.insert(access);
    Ok(current)
}

/// The error response of a [`WorkspaceError`].
fn app_error(error: WorkspaceError) -> AppError {
    error.to_app_error()
}

/// The guest membership of `user` in `workspace`: a demo workspace with
/// `workspace.demo.guest_role` set. Stored nowhere (the nil id).
fn guest_member(config: &WorkspaceConfig, workspace: &Workspace, user: Uuid) -> Option<Member> {
    let role = config
        .demo
        .guest_role
        .as_ref()
        .filter(|_| workspace.is_demo())?;
    Some(member::Model {
        id: Uuid::nil(),
        workspace_id: workspace.id,
        user_id: user,
        role: role.clone(),
        created_at: workspace.created_at,
        updated_at: workspace.created_at,
    })
}

/// For a request to a demo workspace: marks the response for the
/// `X-Navree-Demo` header, and refuses a write to a read-only demo (403,
/// code `demo_read_only`) unless the role edits demos or the route is
/// allowed.
fn check_demo_write(
    parts: &Parts,
    config: &WorkspaceConfig,
    mode: DemoMode,
    role: &WorkspaceRole,
) -> Result<(), AppError> {
    if let Some(cell) = parts.extensions.get::<TenantCell>() {
        cell.mark_demo(mode);
    }
    if !writes(&parts.method, parts.extensions.get::<DemoIntent>().copied()) {
        return Ok(());
    }
    let meta = parts.extensions.get::<Arc<RouteMeta>>();
    let name = meta.and_then(|meta| meta.name.as_deref());
    let path = meta
        .map(|meta| meta.full_path.as_str())
        .filter(|path| !path.is_empty());
    if config
        .demo
        .allows_write(&parts.method, name, path, role.as_str())
    {
        Ok(())
    } else {
        Err(app_error(WorkspaceError::DemoReadOnly))
    }
}

/// Whether a request writes: as its [`DemoIntent`] says, else unless its
/// method is `GET`, `HEAD`, `OPTIONS` or `TRACE`.
fn writes(method: &http::Method, intent: Option<DemoIntent>) -> bool {
    match intent {
        Some(DemoIntent::Read) => false,
        Some(DemoIntent::Write) => true,
        None => !matches!(
            *method,
            http::Method::GET | http::Method::HEAD | http::Method::OPTIONS | http::Method::TRACE
        ),
    }
}

/// The value of path parameter `name`, if the route has it.
async fn path_param(parts: &mut Parts, name: &str) -> Result<Option<String>, AppError> {
    let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, &())
        .await
        .map_err(|_| AppError::not_found())?;
    Ok(params.get(name).cloned())
}

/// The route guard that resolves the workspace before the route's access
/// checks run, so `#[roles]`, `#[permissions]` and `#[can]` (and
/// navree-access's `Abilities`, `CurrentAccess` and `Accessible`
/// extractors) see the member's role in that workspace. The guard answers
/// like [`CurrentWorkspace`]: 400, 404 or 403.
///
/// For a `can(.., from = "id")` route on an entity registered with
/// [`ModuleBuilderWorkspaceExt::workspace_scoped`](crate::ModuleBuilderWorkspaceExt::workspace_scoped),
/// it also checks that the record belongs to the workspace (a 404
/// otherwise), on every database.
///
/// ```ignore
/// #[controller("/deals", version = "1")]
/// #[guard(InWorkspace::guard())]
/// impl DealsController {
///     #[patch("/{id}")]
///     #[can(Update, Deal, from = "id")]
///     async fn update(&self, ws: CurrentWorkspace, deal: Authorized<deal::Entity, Update>) -> .. { .. }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InWorkspace {
    source: Source,
    /// Refuses guests of a demo workspace (`workspace.demo.guest_role`).
    members_only: bool,
}

impl InWorkspace {
    /// The guard, resolving the workspace with the configured resolvers.
    pub fn guard() -> Arc<dyn Guard> {
        Arc::new(InWorkspace {
            source: Source::Configured,
            members_only: false,
        })
    }

    /// The guard, taking the workspace (a slug or an id) from the route's
    /// path parameter `param` (`/workspaces/{workspace}/members` with
    /// `"workspace"`).
    pub fn from_path(param: &'static str) -> Arc<dyn Guard> {
        Arc::new(InWorkspace {
            source: Source::PathParam(param),
            members_only: false,
        })
    }

    /// [`from_path`](Self::from_path) that also refuses a demo workspace's
    /// guests (403): the module's routes about members, invitations and
    /// the workspace's settings.
    pub(crate) fn members_from_path(param: &'static str) -> Arc<dyn Guard> {
        Arc::new(InWorkspace {
            source: Source::PathParam(param),
            members_only: true,
        })
    }
}

impl Guard for InWorkspace {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let current = resolve_request(parts, &self.source).await?;
            if self.members_only && current.is_guest() {
                return Err(AppError::forbidden()
                    .with_detail("Only members of this workspace can do this."));
            }
            check_scoped_record(parts, &current).await
        })
    }
}

/// An entity registered as workspace-scoped: how to check that a record
/// belongs to a workspace.
pub(crate) struct ScopedSubject {
    pub(crate) subject: &'static str,
    pub(crate) belongs: fn(Uow, Uuid, String) -> BoxFuture<'static, Result<bool, DbErr>>,
}

impl ScopedSubject {
    pub(crate) fn of<E>() -> Self
    where
        E: WorkspaceScoped + navree_access::Subject,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync,
    {
        ScopedSubject {
            subject: E::SUBJECT,
            belongs: |uow, workspace, id| Box::pin(belongs::<E>(uow, workspace, id)),
        }
    }
}

/// Whether the record of `E` with primary key `id` belongs to `workspace`
/// (read in `uow`'s transaction).
async fn belongs<E>(uow: Uow, workspace: Uuid, id: String) -> Result<bool, DbErr>
where
    E: WorkspaceScoped,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    let Some(pk) = E::PrimaryKey::iter().next() else {
        return Ok(false);
    };
    let column = pk.into_column();
    let Some(value) = key_value(column.def().get_column_type(), &id) else {
        return Ok(false);
    };
    let select = WorkspaceRepo::<E>::scope(workspace, E::find().filter(column.eq(value)));
    Ok(select.one(&*uow.tx().await?).await?.is_some())
}

/// A path parameter as a value of the key column's type (`None` when it
/// cannot be one).
fn key_value(column_type: &ColumnType, id: &str) -> Option<Value> {
    match column_type {
        ColumnType::Uuid => Uuid::from_str(id).ok().map(Value::from),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::Integer
        | ColumnType::BigInteger => id.parse::<i64>().ok().map(Value::from),
        ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned
        | ColumnType::Unsigned
        | ColumnType::BigUnsigned => id.parse::<u64>().ok().map(Value::from),
        _ => Some(Value::from(id.to_owned())),
    }
}

/// For a `can(.., from = "<param>")` route on a workspace-scoped subject:
/// a 404 unless the record belongs to the current workspace.
async fn check_scoped_record(
    parts: &mut Parts,
    current: &CurrentWorkspace,
) -> Result<(), AppError> {
    let Some(meta) = parts.extensions.get::<Arc<RouteMeta>>().cloned() else {
        return Ok(());
    };
    let Some(can) = &meta.access.can else {
        return Ok(());
    };
    let Some(param) = &can.from else {
        return Ok(());
    };
    let Some(ctx) = parts.extensions.get::<AppContext>().cloned() else {
        return Ok(());
    };
    let Some(scoped) = ctx
        .contributions::<ScopedSubject>()
        .into_iter()
        .find(|c| c.value.subject == can.subject)
    else {
        return Ok(());
    };
    let Some(id) = path_param(parts, param).await? else {
        return Ok(());
    };
    let uow = parts.extensions.get::<Uow>().cloned().ok_or_else(|| {
        AppError::internal("no unit of work in the request: add DatabaseModule to the app")
    })?;
    let uow = match current.isolation() {
        Isolation::Database => tenant_uow(parts, current).await?,
        _ => uow,
    };
    if (scoped.value.belongs)(uow, current.id(), id)
        .await
        .map_err(AppError::internal)?
    {
        Ok(())
    } else {
        Err(AppError::not_found())
    }
}

/// The unit of work for the current workspace's data:
///
/// - row and schema isolation: the request's own unit of work (navree-db's
///   `Uow`), already scoped to the workspace (on PostgreSQL its transaction
///   carries `navree.workspace` and, for schemas, the `search_path`);
/// - database isolation: a unit of work on the workspace's database,
///   opened on first use and committed with the request (when the status
///   is below 400; rolled back otherwise).
///
/// It derefs to [`Uow`], so `Repo`, `WorkspaceRepo` and `paginate` take it
/// as they take the request's.
#[derive(Clone, Debug)]
pub struct WorkspaceUow(Uow);

impl WorkspaceUow {
    /// The unit of work.
    pub fn into_inner(self) -> Uow {
        self.0
    }
}

impl Deref for WorkspaceUow {
    type Target = Uow;

    fn deref(&self) -> &Uow {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for WorkspaceUow {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let current = resolve_request(parts, &Source::Configured).await?;
        match current.isolation() {
            Isolation::Database => Ok(WorkspaceUow(tenant_uow(parts, &current).await?)),
            _ => parts
                .extensions
                .get::<Uow>()
                .cloned()
                .map(WorkspaceUow)
                .ok_or_else(|| {
                    AppError::internal(
                        "no unit of work in the request: add DatabaseModule to the app",
                    )
                }),
        }
    }
}

/// The per-request state the module's layer reads once the response is
/// ready: the workspace database's unit of work (database isolation) and
/// whether the request's workspace is a demo (the `X-Navree-Demo` header).
#[derive(Clone, Default)]
pub(crate) struct TenantCell(pub(crate) Arc<TenantState>);

#[derive(Default)]
pub(crate) struct TenantState {
    pub(crate) uow: OnceCell<Uow>,
    pub(crate) demo: std::sync::OnceLock<DemoMode>,
}

impl TenantCell {
    /// Marks the request as one to a demo workspace in `mode`.
    pub(crate) fn mark_demo(&self, mode: DemoMode) {
        let _ = self.0.demo.set(mode);
    }
}

async fn tenant_uow(parts: &Parts, current: &CurrentWorkspace) -> Result<Uow, AppError> {
    let cell = parts
        .extensions
        .get::<TenantCell>()
        .cloned()
        .ok_or_else(|| {
            AppError::internal("the workspace unit of work needs WorkspaceModule's request layer")
        })?;
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal("WorkspaceUow needs the navree request pipeline"))?;
    let actor = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
        .map(|principal| principal.user_id);
    let request_id = parts
        .extensions
        .get::<RequestId>()
        .map(|id| id.as_str().to_owned());
    let workspace = current.id();
    let uow = cell
        .0
        .uow
        .get_or_try_init(|| async move {
            let databases = ctx
                .get::<WorkspaceDatabases>()
                .await
                .map_err(AppError::internal)?;
            let db = databases.db(workspace).await.map_err(app_error)?;
            Ok::<_, AppError>(db.uow(actor, request_id).await)
        })
        .await?;
    Ok(uow.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_write_by_method_unless_they_declare_their_intent() {
        use http::Method;
        for method in [Method::GET, Method::HEAD, Method::OPTIONS, Method::TRACE] {
            assert!(!writes(&method, None), "{method}");
            assert!(writes(&method, Some(DemoIntent::Write)), "{method}");
        }
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(writes(&method, None), "{method}");
            assert!(!writes(&method, Some(DemoIntent::Read)), "{method}");
        }
    }

    #[test]
    fn keys_take_the_column_type() {
        let id = Uuid::now_v7();
        assert_eq!(
            key_value(&ColumnType::Uuid, &id.to_string()),
            Some(Value::from(id))
        );
        assert_eq!(key_value(&ColumnType::Uuid, "nope"), None);
        assert_eq!(
            key_value(&ColumnType::BigInteger, "42"),
            Some(Value::from(42_i64))
        );
        assert_eq!(key_value(&ColumnType::Integer, "x"), None);
        assert_eq!(
            key_value(&ColumnType::Text, "abc"),
            Some(Value::from("abc".to_owned()))
        );
    }
}
