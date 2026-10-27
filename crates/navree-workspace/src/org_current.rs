//! [`CurrentOrganization`], the [`InOrganization`] guard,
//! [`CurrentPartition`] and [`PartitionScope`]: resolving a request's
//! organization or partition and checking the signed-in user's access.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use http::request::Parts;
use navree_access::{Ability, PrincipalAccess};
use navree_contracts::AuthState;
use navree_db::Uow;
use navree_http::{AppError, Guard};
use navree_kernel::{AppContext, BoxFuture};
use sea_orm::{IntoActiveModel, Select};
use serde::Serialize;
use uuid::Uuid;

use crate::access::{AccessScope, permission_target, scoped_access, subjects_of};
use crate::current::{PinnedWorkspace, app_error, current_workspace, path_param};
use crate::entity::{OrgMember, Organization, Partition};
use crate::isolation::WorkspaceScoped;
use crate::resolve::Lookup;
use crate::scoped::{OrgRepo, PartitionRepo, PartitionScoped};
use crate::workspaces::GrantedBy;
use crate::{
    Isolation, OrgRole, OrgRoleTable, Organizations, PartitionSelector, Partitions, ResolverKind,
    Selector, WorkspaceConfig, WorkspaceError, WorkspaceRole, WorkspaceSelector,
};

/// The organization of the current request, resolved, loaded and checked:
/// the signed-in user is a member of it.
///
/// As an extractor it resolves the organization with the configured
/// resolvers (`workspace.organizations.resolvers`: header `X-Organization`,
/// path `/o/{slug}`, subdomain, claim) unless a guard already did; it
/// answers 401 for guests, 400 when the request names no organization, 404
/// for an unknown one and 403 for non-members, and for tokens pinned to a
/// workspace. It derefs to the [`Organization`].
///
/// ```ignore
/// async fn report(org: CurrentOrganization, uow: Uow) -> Result<Json<Vec<deal::Model>>> {
///     org.authorize("reports.read")?;
///     let deals = org.scope(deal::Entity::find()).all(&*uow.tx().await?).await?;
///     Ok(Json(deals))
/// }
/// ```
///
/// On PostgreSQL it sets `navree.organization` for the request's
/// transaction, which [`RlsPolicy::organizations`](crate::RlsPolicy::organizations)
/// policies read. Route-level access checks see the member's organization
/// role behind the [`InOrganization`] guard.
#[derive(Clone)]
pub struct CurrentOrganization(Arc<ResolvedOrg>);

struct ResolvedOrg {
    organization: Organization,
    member: OrgMember,
    role: OrgRole,
    workspaces: WorkspaceSelector,
    workspace_ids: Vec<Uuid>,
    access: Arc<PrincipalAccess>,
    subjects: Arc<BTreeMap<String, String>>,
    resolver: Option<ResolverKind>,
}

impl CurrentOrganization {
    /// The organization id.
    pub fn id(&self) -> Uuid {
        self.0.organization.id
    }

    /// The organization.
    pub fn organization(&self) -> &Organization {
        &self.0.organization
    }

    /// The scope to send this organization's mail with
    /// (`mailer.for_scope(&organization.mail_scope())`).
    pub fn mail_scope(&self) -> navree_mail::MailScope {
        navree_mail::MailScope::organization(self.id())
    }

    /// The signed-in user's membership.
    pub fn member(&self) -> &OrgMember {
        &self.0.member
    }

    /// The signed-in user's organization role.
    pub fn role(&self) -> &OrgRole {
        &self.0.role
    }

    /// The organization's workspaces the user reaches: `All` or `Only` some.
    pub fn workspaces(&self) -> &WorkspaceSelector {
        &self.0.workspaces
    }

    /// The ids of the attached workspaces the user reaches now, by name:
    /// what [`OrgRepo`] filters by.
    pub fn workspace_ids(&self) -> &[Uuid] {
        &self.0.workspace_ids
    }

    /// Whether the user reaches workspace `id` through the organization.
    pub fn reaches(&self, id: Uuid) -> bool {
        self.0.workspace_ids.contains(&id)
    }

    /// The role's permissions in the organization (built-in plus
    /// `workspace.organizations.roles`), sorted.
    pub fn permissions(&self) -> &[String] {
        &self.0.access.permissions
    }

    /// The user's abilities in the organization.
    pub fn ability(&self) -> &Arc<Ability> {
        &self.0.access.ability
    }

    /// The same, as navree-access's `PrincipalAccess`.
    pub fn access(&self) -> &Arc<PrincipalAccess> {
        &self.0.access
    }

    /// Whether the user may do `action` on `subject` in the organization
    /// (a type-level check).
    pub fn can(&self, action: &str, subject: &str) -> bool {
        self.0.access.ability.can(action, subject, None)
    }

    /// Whether the user holds `permission` in the organization.
    pub fn has_permission(&self, permission: &str) -> bool {
        let (action, subject) = permission_target(permission, &self.0.subjects);
        self.0.access.ability.has_unconditional(action, subject)
    }

    /// `Ok` when the user holds `permission` in the organization, else a
    /// 403.
    pub fn authorize(&self, permission: &str) -> Result<(), AppError> {
        if self.has_permission(permission) {
            Ok(())
        } else {
            Err(AppError::forbidden())
        }
    }

    /// `select` restricted to the rows of the workspaces the user reaches
    /// ([`OrgRepo::scope`] with [`workspace_ids`](CurrentOrganization::workspace_ids)).
    pub fn scope<E>(&self, select: Select<E>) -> Select<E>
    where
        E: WorkspaceScoped,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync,
    {
        OrgRepo::<E>::scope(&self.0.workspace_ids, select)
    }

    /// The resolver that named the organization (`None` when a route's path
    /// parameter did, through [`InOrganization::from_path`]).
    pub fn resolver(&self) -> Option<ResolverKind> {
        self.0.resolver
    }
}

impl Deref for CurrentOrganization {
    type Target = Organization;

    fn deref(&self) -> &Organization {
        &self.0.organization
    }
}

impl fmt::Debug for CurrentOrganization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentOrganization")
            .field("organization", &self.0.organization.id)
            .field("slug", &self.0.organization.slug)
            .field("user", &self.0.member.user_id)
            .field("role", &self.0.role)
            .finish_non_exhaustive()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentOrganization {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        resolve_organization(parts, &OrgSource::Configured).await
    }
}

/// Where a request's organization identifier comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum OrgSource {
    Configured,
    PathParam(&'static str),
}

/// The app's context, the signed-in principal and the module's config.
async fn request_context(
    parts: &Parts,
    what: &str,
) -> Result<
    (
        AppContext,
        navree_contracts::Principal,
        Arc<WorkspaceConfig>,
    ),
    AppError,
> {
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal(format!("{what} needs the navree request pipeline")))?;
    let principal = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.clone())
        .ok_or_else(AppError::unauthenticated)?;
    let config = ctx
        .get::<WorkspaceConfig>()
        .await
        .map_err(|_| AppError::internal(format!("{what} needs WorkspaceModule in the app")))?;
    Ok((ctx, principal, config))
}

/// The organization a request names when the signed-in user is a member of
/// it: the one [`CurrentOrganization`] already resolved, else a read-only
/// lookup with the configured resolvers (no access scope, no unit of work
/// entered, nothing kept); `None` otherwise, never an error. The
/// organization zone source reads it.
pub(crate) async fn try_organization(parts: &Parts) -> Option<Organization> {
    if let Some(current) = parts.extensions.get::<CurrentOrganization>() {
        return Some(current.organization().clone());
    }
    let (ctx, principal, config) = request_context(parts, "CurrentZone").await.ok()?;
    if !config.organizations.enabled || parts.extensions.get::<PinnedWorkspace>().is_some() {
        return None;
    }
    let (_, identifier) = Lookup::organization(&config.organizations).find(parts)?;
    let organizations = ctx.get::<Organizations>().await.ok()?;
    let organization = organizations.resolve(&identifier).await.ok()?;
    organizations
        .membership(organization.id, principal.user_id)
        .await
        .ok()??;
    Some(organization)
}

/// Resolves, loads and checks the request's organization once.
async fn resolve_organization(
    parts: &mut Parts,
    source: &OrgSource,
) -> Result<CurrentOrganization, AppError> {
    if let Some(current) = parts.extensions.get::<CurrentOrganization>() {
        return Ok(current.clone());
    }
    let (ctx, principal, config) = request_context(parts, "CurrentOrganization").await?;
    if !config.organizations.enabled {
        return Err(AppError::internal(
            "CurrentOrganization needs workspace.organizations.enabled = true",
        ));
    }
    if parts.extensions.get::<PinnedWorkspace>().is_some() {
        return Err(AppError::forbidden()
            .with_code("token_limited_to_workspace")
            .with_detail("This token is limited to a workspace."));
    }
    let (resolver, identifier) = match source {
        OrgSource::Configured => Lookup::organization(&config.organizations)
            .find(parts)
            .map(|(kind, identifier)| (Some(kind), identifier)),
        OrgSource::PathParam(name) => path_param(parts, name).await?.map(|value| (None, value)),
    }
    .ok_or(WorkspaceError::NoOrganization)
    .map_err(app_error)?;
    let organizations = ctx
        .get::<Organizations>()
        .await
        .map_err(AppError::internal)?;
    let organization = organizations
        .resolve(&identifier)
        .await
        .map_err(app_error)?;
    let member = organizations
        .membership(organization.id, principal.user_id)
        .await
        .map_err(app_error)?
        .ok_or(WorkspaceError::NotOrganizationMember)
        .map_err(app_error)?;
    let role = member.role();
    let permissions = OrgRoleTable::from_config(&config)
        .permissions(&role)
        .to_vec();
    let access = scoped_access(
        &ctx,
        &principal,
        AccessScope {
            workspace: None,
            organization: Some(organization.id),
        },
        role.as_str(),
        permissions,
    )
    .await
    .map_err(app_error)?;
    let (workspaces, reached) = organizations.reach(&member).await.map_err(app_error)?;
    if let Some(uow) = parts.extensions.get::<Uow>() {
        organizations
            .enter(uow, organization.id)
            .await
            .map_err(app_error)?;
    }
    let current = CurrentOrganization(Arc::new(ResolvedOrg {
        organization,
        member,
        role,
        workspaces,
        workspace_ids: reached.iter().map(|w| w.id).collect(),
        access: access.clone(),
        subjects: subjects_of(&ctx).await,
        resolver,
    }));
    parts.extensions.insert(current.clone());
    parts.extensions.insert(access);
    Ok(current)
}

/// The route guard that resolves the organization before the route's
/// access checks run, so `#[roles]`, `#[permissions]` and `#[can]` see the
/// member's organization role and its permissions
/// (`organization.members.update`, `reports.read`). It answers like
/// [`CurrentOrganization`]: 400, 404 or 403.
///
/// ```ignore
/// #[controller("/reports", version = "1")]
/// #[guard(InOrganization::guard())]
/// impl ReportsController {
///     #[get("/")]
///     #[permissions("reports.read")]
///     async fn index(&self, org: CurrentOrganization) -> .. { .. }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InOrganization {
    source: OrgSource,
}

impl InOrganization {
    /// The guard, resolving the organization with the configured
    /// resolvers.
    pub fn guard() -> Arc<dyn Guard> {
        Arc::new(InOrganization {
            source: OrgSource::Configured,
        })
    }

    /// The guard, taking the organization (a slug or an id) from the
    /// route's path parameter `param`.
    pub fn from_path(param: &'static str) -> Arc<dyn Guard> {
        Arc::new(InOrganization {
            source: OrgSource::PathParam(param),
        })
    }
}

impl Guard for InOrganization {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            resolve_organization(parts, &self.source).await?;
            Ok(())
        })
    }
}

/// The partition the current request names, resolved and checked: it
/// belongs to the request's workspace (404 otherwise) and the signed-in
/// member sees it (403 otherwise). 400 when the request names none.
///
/// The partition comes from `workspace.partitions.resolvers`: the
/// `X-Partition` header (a slug or an id), a path segment after `/p`, or a
/// claim. Resolving it resolves the workspace first, as
/// [`CurrentWorkspace`](crate::CurrentWorkspace) does. On PostgreSQL it
/// sets `navree.partition` for the request's transaction. It derefs to the
/// [`Partition`].
#[derive(Clone, Debug)]
pub struct CurrentPartition {
    partition: Arc<Partition>,
    resolver: ResolverKind,
}

impl CurrentPartition {
    /// The partition id.
    pub fn id(&self) -> Uuid {
        self.partition.id
    }

    /// The partition.
    pub fn partition(&self) -> &Partition {
        &self.partition
    }

    /// The workspace the partition belongs to.
    pub fn workspace_id(&self) -> Uuid {
        self.partition.workspace_id
    }

    /// The resolver that named the partition.
    pub fn resolver(&self) -> ResolverKind {
        self.resolver
    }
}

impl Deref for CurrentPartition {
    type Target = Partition;

    fn deref(&self) -> &Partition {
        &self.partition
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentPartition {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        resolve_partitions(parts)
            .await?
            .current
            .ok_or_else(|| app_error(WorkspaceError::NoPartition))
    }
}

/// The partitions the current request may touch in its workspace: the one
/// it names, if it names one (checked as [`CurrentPartition`] checks it),
/// else every partition the signed-in member sees.
///
/// ```ignore
/// async fn index(scope: PartitionScope, uow: Uow) -> Result<Json<Vec<contact::Model>>> {
///     let contacts = scope.scope(contact::Entity::find()).all(&*uow.tx().await?).await?;
///     Ok(Json(contacts))
/// }
/// ```
///
/// The workspace owner and members who enter through an organization see
/// every partition; other members see the partitions granted to them
/// ([`Partitions::grant`]).
#[derive(Clone, Debug)]
pub struct PartitionScope {
    workspace: Uuid,
    allowed: PartitionSelector,
    selector: PartitionSelector,
    current: Option<CurrentPartition>,
}

impl PartitionScope {
    /// The workspace.
    pub fn workspace_id(&self) -> Uuid {
        self.workspace
    }

    /// The partitions this request touches: the named one, else the
    /// member's.
    pub fn selector(&self) -> &PartitionSelector {
        &self.selector
    }

    /// The partitions the member sees in the workspace, whatever the
    /// request names.
    pub fn allowed(&self) -> &PartitionSelector {
        &self.allowed
    }

    /// The partition the request names, if any.
    pub fn current(&self) -> Option<&CurrentPartition> {
        self.current.as_ref()
    }

    /// Whether the request may touch partition `id`.
    pub fn includes(&self, id: Uuid) -> bool {
        self.selector.includes(id)
    }

    /// `select` restricted to the workspace and to [`selector`](PartitionScope::selector)
    /// ([`PartitionRepo::scope`]).
    pub fn scope<E>(&self, select: Select<E>) -> Select<E>
    where
        E: PartitionScoped,
        E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
        E::ActiveModel: Send + Sync,
    {
        PartitionRepo::<E>::scope(self.workspace, &self.selector, select)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for PartitionScope {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        resolve_partitions(parts).await
    }
}

/// Resolves the request's workspace, the member's partitions and the
/// partition the request names, once.
async fn resolve_partitions(parts: &mut Parts) -> Result<PartitionScope, AppError> {
    if let Some(scope) = parts.extensions.get::<PartitionScope>() {
        return Ok(scope.clone());
    }
    let workspace = current_workspace(parts).await?;
    let (ctx, _principal, config) = request_context(parts, "PartitionScope").await?;
    if !config.partitions.enabled {
        return Err(AppError::internal(
            "CurrentPartition and PartitionScope need workspace.partitions.enabled = true",
        ));
    }
    let partitions = ctx.get::<Partitions>().await.map_err(AppError::internal)?;
    let allowed = match workspace.granted_by() {
        GrantedBy::Membership if workspace.role() != &WorkspaceRole::Owner => partitions
            .member_access(workspace.member())
            .await
            .map_err(app_error)?,
        _ => Selector::All,
    };
    let current = match Lookup::partition(&config.partitions).find(parts) {
        Some((resolver, identifier)) => {
            let partition = partitions
                .resolve(workspace.id(), &identifier)
                .await
                .map_err(app_error)?;
            if !allowed.includes(partition.id) {
                return Err(app_error(WorkspaceError::PartitionForbidden));
            }
            if workspace.isolation() != Isolation::Database
                && let Some(uow) = parts.extensions.get::<Uow>()
            {
                partitions
                    .enter(uow, partition.id)
                    .await
                    .map_err(app_error)?;
            }
            Some(CurrentPartition {
                partition: Arc::new(partition),
                resolver,
            })
        }
        None => None,
    };
    let selector = match &current {
        Some(partition) => Selector::only([partition.id()]),
        None => allowed.clone(),
    };
    let scope = PartitionScope {
        workspace: workspace.id(),
        allowed,
        selector,
        current,
    };
    parts.extensions.insert(scope.clone());
    Ok(scope)
}
