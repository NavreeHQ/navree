//! Models that belong to workspaces (`#[model(workspace)]`) in the panel.
//!
//! The panel works in one workspace at a time. Which one is decided before
//! a screen runs: the workspace the request's unit of work has entered
//! (the app enters it for requests that are inside a workspace), else the
//! one the request names with the workspace header (`workspace.header` of
//! `WorkspaceModule`, `X-Workspace` without it) or the panel's workspace
//! cookie (set by the form in the sidebar).
//!
//! A write takes more than being let in: a role that can't write, and a read-only
//! demo, take none ([`WorkspaceWrites`](navree_access::WorkspaceWrites)), and
//! whoever holds what app-wide doesn't change that.
//!
//! A workspace a request names is entered only for a user who may work in it:
//! a member, through `WorkspaceModule` (see
//! [`WorkspaceGate`](navree_access::WorkspaceGate)), or whoever the app lets in
//! with [`helm_staff_workspaces`](crate::ModuleBuilderHelmExt::helm_staff_workspaces).
//! An app with neither has no way to check, and the panel refuses rather than
//! trust the request. A header that names a workspace the user may not enter
//! gets a 403; a cookie that does counts for nothing, so that a user who was
//! removed still reaches the sidebar. The panel enters it the way a workspace
//! request does: the unit of work remembers it, which scopes the ORM's queries
//! and fills the records it creates, and on PostgreSQL the transaction's
//! `navree.workspace` setting is set, which is what row-level security
//! policies read, and under schema isolation the `search_path`.
//!
//! A credential limited to a workspace ([`PinnedWorkspace`]) works in that one:
//! it names it when the request doesn't, a header naming another is a 403 before
//! staff is asked, and a cookie naming another counts for nothing.
//!
//! Without a workspace, a resource of a workspace-scoped model answers with
//! a 400 that asks for one: it never falls back to every workspace.
//! [`Workspaces::Every`] is the explicit way for a resource to work across
//! workspaces; its default writes run inside the workspace of the record they
//! change.

use std::future::Future;
use std::sync::Arc;

use http::HeaderMap;
use navree_access::{
    PinnedWorkspace, PrincipalAccess, WorkspaceEntry, WorkspaceGate, WorkspaceWrites,
};
use navree_db::{Backend, Executor as _, Statement, Uow};
use navree_http::AppError;
use navree_kernel::{AppContext, BoxFuture};
use uuid::Uuid;

use crate::core::Core;
use crate::session::cookie;

/// The cookie that holds the workspace the panel works in, set by the form
/// in the sidebar (`POST {path}/workspace`).
pub const WORKSPACE_COOKIE: &str = "navree_helm_workspace";

/// The header an API client names its workspace with, in an app without
/// `WorkspaceModule` (whose `workspace.header` it is otherwise).
pub(crate) const WORKSPACE_HEADER: &str = "x-workspace";

/// Where a resource of a workspace-scoped model (`#[model(workspace)]`)
/// works. Models without a workspace column ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Workspaces {
    /// In one workspace: the one the request works in (see
    /// [`WORKSPACE_COOKIE`]). Without one, every screen of the resource
    /// answers with a 400 that asks for a workspace. This is the default,
    /// and the only scope that holds under PostgreSQL row-level security
    /// with a role that does not bypass it.
    #[default]
    Current,
    /// Across every workspace: the queries leave out the workspace scope,
    /// so the database alone decides what they see (a role that bypasses
    /// row-level security sees every workspace, one that does not sees
    /// none), and on the same terms what a create may make: a form names the
    /// `workspace_id` of the record, which must exist. A user who may work in
    /// it makes the record inside it; anyone else makes it only where the
    /// database would show it to them, so never with a role row-level security
    /// binds.
    ///
    /// The default update and delete run inside the workspace of their record.
    /// An action's handler, or a resource's own `create`, `update` or `delete`,
    /// runs as the request does, in no workspace unless one was named, and
    /// scopes its writes itself (`navree_orm::workspace::scope`).
    Every,
}

/// How a request named a workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NamedBy {
    Header,
    Cookie,
    /// The credential is limited to it.
    Pin,
}

impl NamedBy {
    fn as_str(self) -> &'static str {
        match self {
            NamedBy::Header => "header",
            NamedBy::Cookie => "cookie",
            NamedBy::Pin => "credential",
        }
    }
}

/// The workspace the request names: header `header` (when requests may name
/// one with a header), else the panel's workspace cookie. Anything but a UUID
/// names none.
pub(crate) fn requested(headers: &HeaderMap, header: Option<&str>) -> Option<(Uuid, NamedBy)> {
    let from_header = header
        .and_then(|name| headers.get(name))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match from_header {
        Some(value) => Uuid::parse_str(value)
            .ok()
            .map(|workspace| (workspace, NamedBy::Header)),
        None => cookie(headers, WORKSPACE_COOKIE)
            .and_then(|value| Uuid::parse_str(value.trim()).ok())
            .map(|workspace| (workspace, NamedBy::Cookie)),
    }
}

/// The workspace a request works in under a credential limited to `pin`: the
/// pin, whatever the cookie holds, and an error for a header that names another,
/// before anyone is asked whether the user may work there.
pub(crate) fn within_pin(
    pin: Uuid,
    named: Option<(Uuid, NamedBy)>,
) -> Result<(Uuid, NamedBy), AppError> {
    match named {
        Some((other, NamedBy::Header)) if other != pin => Err(PinnedWorkspace::mismatch()),
        _ => Ok((pin, NamedBy::Pin)),
    }
}

/// What [`helm_staff_workspaces`](crate::ModuleBuilderHelmExt::helm_staff_workspaces)
/// is asked about: a user who is not a member of a workspace a request names.
#[derive(Clone)]
#[non_exhaustive]
pub struct StaffEntry {
    /// The app.
    pub app: AppContext,
    /// The signed-in user.
    pub user_id: Uuid,
    /// The workspace.
    pub workspace: Uuid,
    /// Their app-wide access.
    pub access: Arc<PrincipalAccess>,
}

/// The check of [`helm_staff_workspaces`](crate::ModuleBuilderHelmExt::helm_staff_workspaces).
pub(crate) type StaffCheck =
    Arc<dyn Fn(StaffEntry) -> BoxFuture<'static, Result<bool, AppError>> + Send + Sync>;

/// A contribution of `helm_staff_workspaces`.
pub(crate) struct StaffPolicy(pub(crate) StaffCheck);

impl StaffPolicy {
    pub(crate) fn new<F, Fut>(may_enter: F) -> Self
    where
        F: Fn(StaffEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static,
    {
        StaffPolicy(Arc::new(move |entry| Box::pin(may_enter(entry))))
    }
}

/// How a user gets into a workspace they name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admitted {
    /// As a member.
    Member,
    /// By `helm_staff_workspaces`: entered by the caller, and audited.
    Staff,
}

/// Why a user does not get in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// They may not.
    NotAllowed,
    /// Staff may, and there is no such workspace.
    Missing,
    /// The app has no way of checking.
    Unchecked,
    /// Each workspace has a database of its own, out of the panel's reach.
    OwnDatabases,
}

impl Refused {
    /// The answer to a request that named the workspace itself.
    pub(crate) fn error(self) -> AppError {
        match self {
            Refused::NotAllowed => AppError::forbidden()
                .with_code("helm_workspace_refused")
                .with_detail("You may not work in this workspace."),
            Refused::Missing => AppError::not_found().with_detail("There is no such workspace."),
            Refused::Unchecked => AppError::forbidden()
                .with_code("helm_workspace_unchecked")
                .with_detail(
                    "This panel has no way to check who may work in a workspace: the app needs \
                     WorkspaceModule, or to say who may with helm_staff_workspaces.",
                ),
            Refused::OwnDatabases => AppError::bad_request(
                "The panel cannot work inside a workspace under workspace.isolation = \
                 \"database\": it works on the app's database, and each workspace's records \
                 are in a database of its own.",
            ),
        }
    }
}

/// What `helm_staff_workspaces` says of `user_id` and `workspace`, `None`
/// without one.
async fn staff_says(
    app: &AppContext,
    core: &Core,
    user_id: Uuid,
    access: &Arc<PrincipalAccess>,
    workspace: Uuid,
) -> Result<Option<bool>, AppError> {
    let Some(check) = &core.staff else {
        return Ok(None);
    };
    check(StaffEntry {
        app: app.clone(),
        user_id,
        workspace,
        access: access.clone(),
    })
    .await
    .map(Some)
}

/// Whether `user_id` may work in `workspace`, what the gate said of it being
/// `entry` (`None` without a gate).
async fn decide(
    app: &AppContext,
    core: &Core,
    user_id: Uuid,
    access: &Arc<PrincipalAccess>,
    workspace: Uuid,
    entry: Option<WorkspaceEntry>,
) -> Result<Result<Admitted, Refused>, AppError> {
    if entry == Some(WorkspaceEntry::Allowed) {
        return Ok(Ok(Admitted::Member));
    }
    let staff = staff_says(app, core, user_id, access, workspace).await?;
    Ok(match (entry, staff) {
        (None, None) => Err(Refused::Unchecked),
        (_, None | Some(false)) => Err(Refused::NotAllowed),
        (Some(WorkspaceEntry::NoSuchWorkspace), Some(true)) => Err(Refused::Missing),
        (_, Some(true)) => Ok(Admitted::Staff),
    })
}

/// Whether `user_id` may work in `workspace`, outside a request's own entry:
/// the sidebar's form, a form that names a workspace, a queued action. With
/// `writes`, an error if the workspace takes no writes from them.
pub(crate) async fn admission(
    app: &AppContext,
    core: &Core,
    user_id: Uuid,
    access: &Arc<PrincipalAccess>,
    workspace: Uuid,
    writes: bool,
) -> Result<Result<Admitted, Refused>, AppError> {
    let gate = app.try_get::<dyn WorkspaceGate>().await;
    let entry = match &gate {
        Some(gate) if gate.own_databases() => return Ok(Err(Refused::OwnDatabases)),
        Some(gate) => Some(gate.check(user_id, workspace, writes).await?),
        None => None,
    };
    let decided = decide(app, core, user_id, access, workspace, entry).await?;
    if decided == Ok(Admitted::Staff) && writes {
        staff_writes(gate.as_deref(), user_id, workspace).await?;
    }
    Ok(decided)
}

/// An error if `workspace` is a demo that takes no writes, which staff are held
/// to as much as anyone.
async fn staff_writes(
    gate: Option<&dyn WorkspaceGate>,
    user_id: Uuid,
    workspace: Uuid,
) -> Result<(), AppError> {
    if let Some(gate) = gate
        && gate.writes(user_id, workspace).await?.read_only_demo
    {
        return Err(AppError::demo_read_only());
    }
    Ok(())
}

/// For a request that names `workspace`: checks the user and enters it.
pub(crate) async fn admit_request(
    parts: &mut http::request::Parts,
    app: &AppContext,
    core: &Core,
    uow: &Uow,
    user_id: Uuid,
    access: &Arc<PrincipalAccess>,
    workspace: Uuid,
) -> Result<Result<Admitted, Refused>, AppError> {
    let gate = app.try_get::<dyn WorkspaceGate>().await;
    let entry = match &gate {
        Some(gate) if gate.own_databases() => return Ok(Err(Refused::OwnDatabases)),
        Some(gate) => Some(gate.enter_request(parts, workspace).await?),
        None => None,
    };
    let decided = decide(app, core, user_id, access, workspace, entry).await?;
    if decided == Ok(Admitted::Staff) {
        if !matches!(
            parts.method,
            http::Method::GET | http::Method::HEAD | http::Method::OPTIONS | http::Method::TRACE
        ) {
            staff_writes(gate.as_deref(), user_id, workspace).await?;
        }
        enter_in(app, uow, Some(workspace)).await?;
    }
    Ok(decided)
}

/// Why a workspace takes no write from a user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RefusedWrite {
    /// It is a read-only demo.
    Demo,
    /// Their role can't write.
    Role,
}

impl RefusedWrite {
    /// What `error` says, if it says only that.
    pub(crate) fn of(error: &AppError) -> Option<RefusedWrite> {
        match error.code.as_str() {
            "demo_read_only" => Some(RefusedWrite::Demo),
            "workspace_read_only" => Some(RefusedWrite::Role),
            _ => None,
        }
    }

    /// The answer to a write.
    pub(crate) fn error(self) -> AppError {
        match self {
            RefusedWrite::Demo => AppError::demo_read_only(),
            RefusedWrite::Role => WorkspaceWrites::read_only_role_error(),
        }
    }
}

/// The header requests name their workspace with here, if they may.
pub(crate) async fn header_of(app: &AppContext) -> Option<String> {
    match app.try_get::<dyn WorkspaceGate>().await {
        Some(gate) => gate.header().map(str::to_owned),
        None => Some(WORKSPACE_HEADER.to_owned()),
    }
}

/// The audit entry of a request or job working in `workspace` as staff.
pub(crate) fn staff_entry(workspace: Uuid, how: &str) -> navree_contracts::AuditEntry {
    navree_contracts::AuditEntry::new(navree_contracts::AuditKind::Custom, "helm.workspace.staff")
        .subject("Workspace", workspace.to_string())
        .metadata(serde_json::json!({ "workspace": workspace, "how": how }))
}

/// How a request named its workspace, as an audit entry says it.
pub(crate) fn named_by_name(named_by: Option<NamedBy>) -> &'static str {
    named_by.map_or("app", NamedBy::as_str)
}

/// Whether row-level security binds the connection's role on `table`: on
/// PostgreSQL, whether the database would hide another workspace's rows from it.
pub(crate) async fn bound_by_row_level_security(uow: &Uow, table: &str) -> Result<bool, AppError> {
    if uow.backend() != Backend::Postgres {
        return Ok(false);
    }
    let row = uow
        .fetch_optional(&Statement::new(
            Backend::Postgres,
            "SELECT row_security_active($1) AS active",
            [table.into()],
        ))
        .await
        .map_err(AppError::internal)?;
    match row {
        Some(row) => row.get::<bool>("active").map_err(AppError::internal),
        None => Ok(true),
    }
}

/// Makes `workspace` the one `uow` works in, as `WorkspaceModule` does when
/// the app has it.
pub(crate) async fn enter_in(
    app: &AppContext,
    uow: &Uow,
    workspace: Option<Uuid>,
) -> Result<(), AppError> {
    match app.try_get::<dyn WorkspaceGate>().await {
        Some(gate) => gate.switch(uow, workspace).await,
        None => enter(uow, workspace).await,
    }
}

/// Makes `workspace` the one `uow` works in: the unit of work remembers it
/// and, on PostgreSQL and MySQL, the transaction's `navree.workspace`
/// setting holds it (empty for none).
async fn enter(uow: &Uow, workspace: Option<Uuid>) -> Result<(), AppError> {
    uow.set_workspace(workspace);
    if uow.backend() == Backend::Sqlite {
        return Ok(());
    }
    let value = workspace.map(|id| id.to_string()).unwrap_or_default();
    uow.set_local("navree.workspace", &value)
        .await
        .map_err(AppError::internal)?;
    Ok(())
}

/// Runs `work` inside `workspace` (the ORM's current workspace and the
/// transaction's setting), then puts back the workspace `uow` had.
pub(crate) async fn within<T>(
    app: &AppContext,
    uow: &Uow,
    workspace: Uuid,
    work: impl Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let previous = uow.workspace();
    if previous != Some(workspace) {
        enter_in(app, uow, Some(workspace)).await?;
    }
    let result = navree_orm::workspace::scope(workspace, work).await;
    if previous != Some(workspace) {
        let restored = enter_in(app, uow, previous).await;
        return result.and_then(|value| restored.map(|()| value));
    }
    result
}

/// Runs `work` in no workspace, then puts back the one `uow` had.
pub(crate) async fn outside<T>(
    app: &AppContext,
    uow: &Uow,
    work: impl Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let previous = uow.workspace();
    if previous.is_some() {
        enter_in(app, uow, None).await?;
    }
    let result = work.await;
    if previous.is_some() {
        let restored = enter_in(app, uow, previous).await;
        return result.and_then(|value| restored.map(|()| value));
    }
    result
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use http::header::COOKIE;

    use super::*;

    #[test]
    fn the_header_comes_before_the_cookie_and_only_a_uuid_counts() {
        let acme = Uuid::now_v7();
        let globex = Uuid::now_v7();
        let mut headers = HeaderMap::new();
        let requested = |headers: &HeaderMap| {
            requested(headers, Some(WORKSPACE_HEADER)).map(|(workspace, _)| workspace)
        };
        assert_eq!(requested(&headers), None);
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{WORKSPACE_COOKIE}={acme}")).unwrap(),
        );
        assert_eq!(requested(&headers), Some(acme));
        headers.insert(
            WORKSPACE_HEADER,
            HeaderValue::from_str(&format!(" {globex} ")).unwrap(),
        );
        assert_eq!(requested(&headers), Some(globex));
        // A header that is not a UUID does not fall back to the cookie.
        headers.insert(WORKSPACE_HEADER, HeaderValue::from_static("acme"));
        assert_eq!(requested(&headers), None);
        headers.remove(WORKSPACE_HEADER);
        headers.insert(
            COOKIE,
            HeaderValue::from_static("navree_helm_workspace=nope"),
        );
        assert_eq!(requested(&headers), None);
    }
}
