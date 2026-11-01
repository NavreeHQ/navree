//! The workspace routes: `/api/workspaces...` and
//! `POST /api/invitations/accept`.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::{Method, Uri};
use navree_db::Uow;
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, Path, RouteDef,
    schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::current::{CurrentWorkspace, InWorkspace};
use crate::directory::UserSummary;
use crate::entity::{Invitation, Member, Partition, Workspace, WorkspaceKind};
use crate::invitations::AcceptedInvitation;
use crate::members::MemberInfo;
use crate::org_current::PartitionScope;
use crate::workspaces::{NewWorkspace, check_time_zone};
use crate::{
    DemoWorkspaces, Invitations, Members, NewPartition, PartitionSelector, Partitions, Selector,
    WorkspaceConfig, WorkspaceRole, Workspaces,
};

/// The path parameter naming the workspace (a slug or an id).
const WORKSPACE_PARAM: &str = "workspace";

/// A workspace as the API returns it, with the caller's role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct WorkspaceResource {
    /// Always `Workspace` (for CASL subject detection).
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "Workspace")]
    pub kind: String,
    /// The id.
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The unique slug.
    pub slug: String,
    /// The owner.
    pub owner_id: Uuid,
    /// The organization the workspace is attached to, if any.
    pub organization_id: Option<Uuid>,
    /// `live` or `demo` (a demo workspace, for a front end's Live/Demo
    /// switch).
    #[serde(rename = "kind")]
    #[schema(rename = "kind")]
    pub workspace_kind: WorkspaceKind,
    /// The caller's role in the workspace.
    #[schema(value_type = String, example = "owner")]
    pub role: WorkspaceRole,
    /// The IANA time zone (`Asia/Kolkata`), if set.
    pub timezone: Option<String>,
    /// The ISO 3166-1 alpha-2 country (`IN`), if set.
    pub country: Option<String>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceResource {
    /// The resource of `workspace` for a caller with `role`.
    pub fn new(workspace: &Workspace, role: WorkspaceRole) -> Self {
        Self {
            kind: "Workspace".to_owned(),
            id: workspace.id,
            name: workspace.name.clone(),
            slug: workspace.slug.clone(),
            owner_id: workspace.owner_id,
            organization_id: workspace.organization_id,
            workspace_kind: workspace.kind(),
            role,
            timezone: workspace.timezone.clone(),
            country: workspace.country.clone(),
            created_at: workspace.created_at,
            updated_at: workspace.updated_at,
        }
    }
}

/// A member as the API returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct MemberResource {
    /// Always `WorkspaceMember`.
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "WorkspaceMember")]
    pub kind: String,
    /// The membership id.
    pub id: Uuid,
    /// The workspace.
    pub workspace_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// The role.
    #[schema(value_type = String, example = "member")]
    pub role: WorkspaceRole,
    /// The user's name and email, when the user directory knows them.
    pub user: Option<UserSummary>,
    /// When the user joined.
    pub created_at: DateTime<Utc>,
    /// When the membership last changed.
    pub updated_at: DateTime<Utc>,
}

impl MemberResource {
    /// The resource of `member` (with `user` when known).
    pub fn new(member: &Member, user: Option<UserSummary>) -> Self {
        Self {
            kind: "WorkspaceMember".to_owned(),
            id: member.id,
            workspace_id: member.workspace_id,
            user_id: member.user_id,
            role: member.role(),
            user,
            created_at: member.created_at,
            updated_at: member.updated_at,
        }
    }
}

impl From<MemberInfo> for MemberResource {
    fn from(info: MemberInfo) -> Self {
        MemberResource::new(&info.member, info.user)
    }
}

/// An invitation as the API returns it (never with its token).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct InvitationResource {
    /// Always `WorkspaceInvitation`.
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "WorkspaceInvitation")]
    pub kind: String,
    /// The id.
    pub id: Uuid,
    /// The workspace.
    pub workspace_id: Uuid,
    /// The invited address.
    pub email: String,
    /// The role offered.
    #[schema(value_type = String, example = "member")]
    pub role: WorkspaceRole,
    /// Who sent it.
    pub invited_by: Option<Uuid>,
    /// When it expires.
    pub expires_at: DateTime<Utc>,
    /// When it was sent.
    pub created_at: DateTime<Utc>,
}

impl From<&Invitation> for InvitationResource {
    fn from(invitation: &Invitation) -> Self {
        Self {
            kind: "WorkspaceInvitation".to_owned(),
            id: invitation.id,
            workspace_id: invitation.workspace_id,
            email: invitation.email.clone(),
            role: invitation.role(),
            invited_by: invitation.invited_by,
            expires_at: invitation.expires_at,
            created_at: invitation.created_at,
        }
    }
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DataList<T> {
    pub(crate) data: Vec<T>,
}

/// A partition as the API returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PartitionResource {
    /// Always `WorkspacePartition`.
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "WorkspacePartition")]
    pub kind: String,
    /// The id.
    pub id: Uuid,
    /// The workspace.
    pub workspace_id: Uuid,
    /// The display name.
    pub name: String,
    /// The slug, unique in the workspace.
    pub slug: String,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<&Partition> for PartitionResource {
    fn from(partition: &Partition) -> Self {
        Self {
            kind: "WorkspacePartition".to_owned(),
            id: partition.id,
            workspace_id: partition.workspace_id,
            name: partition.name.clone(),
            slug: partition.slug.clone(),
            created_at: partition.created_at,
            updated_at: partition.updated_at,
        }
    }
}

/// The partitions a member sees, as the API returns them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PartitionAccessResource {
    /// The member.
    pub user_id: Uuid,
    /// `"all"` or a list of partition ids.
    #[schema(value_type = Object)]
    pub partitions: PartitionSelector,
}

/// The body of `POST /workspaces/{workspace}/partitions`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreatePartitionInput {
    /// The display name.
    pub name: String,
    /// The slug; derived from the name when absent.
    pub slug: Option<String>,
}

/// The body of `PATCH /workspaces/{workspace}/partitions/{partition}`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct RenamePartitionInput {
    /// The new name.
    pub name: String,
}

/// The body of `PUT /workspaces/{workspace}/members/{user}/partitions`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct PartitionAccessInput {
    /// `"all"` or a list of partition ids.
    #[schema(value_type = Object)]
    pub partitions: PartitionSelector,
}

/// A 403 unless the caller sees every partition `selector` names.
fn check_partition_reach(
    scope: &PartitionScope,
    selector: &PartitionSelector,
) -> Result<(), AppError> {
    let covered = match (scope.allowed(), selector) {
        (Selector::All, _) => true,
        (Selector::Only(_), Selector::All) => false,
        (Selector::Only(mine), Selector::Only(theirs)) => theirs.iter().all(|id| mine.contains(id)),
    };
    if covered {
        Ok(())
    } else {
        Err(AppError::forbidden()
            .with_code("partition_grant_forbidden")
            .with_detail("You can only grant partitions you see."))
    }
}

/// A 403 unless the caller sees every partition.
fn check_partition_manager(scope: &PartitionScope) -> Result<(), AppError> {
    if scope.allowed().is_all() {
        Ok(())
    } else {
        Err(AppError::forbidden()
            .with_code("partition_manager_required")
            .with_detail("Only members who see every partition can manage partitions."))
    }
}

/// The body of `POST /workspaces`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateWorkspaceInput {
    /// The display name.
    pub name: String,
    /// The slug; derived from the name when absent.
    pub slug: Option<String>,
    /// The IANA time zone (`Asia/Kolkata`); legacy names are stored under
    /// their current names.
    #[serde(default)]
    pub timezone: Option<String>,
    /// The ISO 3166-1 alpha-2 country (`IN`).
    #[serde(default)]
    pub country: Option<String>,
}

/// The body of `PUT /workspaces/{workspace}/time-zone`: both values are
/// replaced (`null` or absent clears one).
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct WorkspaceTimeZoneInput {
    /// The IANA time zone, or `null`.
    #[serde(default)]
    pub timezone: Option<String>,
    /// The ISO 3166-1 alpha-2 country, or `null`.
    #[serde(default)]
    pub country: Option<String>,
}

/// The body of `PATCH /workspaces/{workspace}`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct RenameWorkspaceInput {
    /// The new name.
    pub name: String,
}

/// The body of `POST /workspaces/{workspace}/transfer`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct TransferWorkspaceInput {
    /// The member who becomes the owner.
    pub user_id: Uuid,
}

/// The body of `POST /workspaces/{workspace}/members`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct AddMemberInput {
    /// The user to add.
    pub user_id: Uuid,
    /// Their role (default `member`).
    #[schema(value_type = Option<String>)]
    pub role: Option<WorkspaceRole>,
}

/// The body of `PATCH /workspaces/{workspace}/members/{user}`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct ChangeRoleInput {
    /// The new role.
    #[schema(value_type = String)]
    pub role: WorkspaceRole,
}

/// The body of `POST /workspaces/{workspace}/invitations`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct InviteInput {
    /// The address to invite.
    pub email: String,
    /// The role offered (default `member`; never `owner`).
    #[schema(value_type = Option<String>)]
    pub role: Option<WorkspaceRole>,
}

/// The body of `POST /invitations/accept`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct AcceptInvitationInput {
    /// The token from the invitation mail.
    pub token: String,
}

/// What `POST /invitations/accept` returns.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AcceptedInvitationResource {
    /// The workspace joined.
    pub workspace: WorkspaceResource,
    /// The new membership.
    pub member: MemberResource,
}

/// The workspace routes, mounted under `{http.api_prefix}/workspaces`
/// unless `workspace.routes` is false. `{workspace}` is a slug or an id;
/// every route needs a signed-in user, and the `{workspace}` routes a
/// member of that workspace ([`InWorkspace::from_path`]), with these
/// permissions in it:
///
/// | Route | Permission | Answer |
/// |---|---|---|
/// | `GET /workspaces` | | the caller's workspaces, `{data: [..]}` (with the demo workspaces when `workspace.demo.guest_role` is set) |
/// | `POST /workspaces` `{name, slug?, timezone?, country?}` | | 201, the workspace (caller is owner) |
/// | `GET /workspaces/{workspace}` | (member) | the workspace with the caller's role |
/// | `PATCH /workspaces/{workspace}` `{name}` | `workspace.update` | the workspace |
/// | `PUT /workspaces/{workspace}/time-zone` `{timezone, country}` | `workspace.update` | the workspace |
/// | `DELETE /workspaces/{workspace}` | `workspace.delete` | 204 |
/// | `POST /workspaces/{workspace}/transfer` `{user_id}` | `workspace.transfer` | the workspace |
/// | `POST /workspaces/{workspace}/leave` | (member) | 204; 409 for the owner |
/// | `GET /workspaces/{workspace}/members` | `workspace.members.read` | `{data: [..]}` |
/// | `POST /workspaces/{workspace}/members` `{user_id, role?}` | `workspace.members.create` | 201, the member |
/// | `PATCH /workspaces/{workspace}/members/{user}` `{role}` | `workspace.members.update` | the member |
/// | `DELETE /workspaces/{workspace}/members/{user}` | `workspace.members.delete` | 204 |
/// | `GET /workspaces/{workspace}/invitations` | `workspace.invitations.read` | pending, `{data: [..]}` |
/// | `POST /workspaces/{workspace}/invitations` `{email, role?}` | `workspace.invitations.create` | 201, the invitation |
/// | `DELETE /workspaces/{workspace}/invitations/{invitation}` | `workspace.invitations.delete` | 204 |
///
/// Granting the `owner` role, or changing or removing a member who has it,
/// also takes the `owner` role (a 403 otherwise). In a read-only demo
/// workspace the routes that write answer 403 (`demo_read_only`); a demo's
/// guests (`workspace.demo.guest_role`) get `GET /workspaces/{workspace}`
/// and a 403 from the others. `POST /invitations/accept` is
/// [`InvitationController`].
///
/// With `workspace.partitions.enabled`, the partition routes join them:
///
/// | Route | Permission | Answer |
/// |---|---|---|
/// | `GET /workspaces/{workspace}/partitions` | `workspace.partitions.read` | the partitions the caller sees, `{data: [..]}` |
/// | `POST /workspaces/{workspace}/partitions` `{name, slug?}` | `workspace.partitions.create` | 201, the partition |
/// | `PATCH /workspaces/{workspace}/partitions/{partition}` `{name}` | `workspace.partitions.update` | the partition |
/// | `DELETE /workspaces/{workspace}/partitions/{partition}` | `workspace.partitions.delete` | 204 |
/// | `GET /workspaces/{workspace}/members/{user}/partitions` | `workspace.members.read` | `{user_id, partitions}` |
/// | `PUT /workspaces/{workspace}/members/{user}/partitions` `{partitions}` | `workspace.members.update` | `{user_id, partitions}` |
///
/// Creating, renaming and deleting partitions also takes seeing every
/// partition, and a member limited to some partitions grants only those
/// (a 403 otherwise).
pub struct WorkspaceController {
    workspaces: Arc<Workspaces>,
    members: Arc<Members>,
    invitations: Arc<Invitations>,
    partitions: Arc<Partitions>,
    demos: Arc<DemoWorkspaces>,
    guest_role: Option<WorkspaceRole>,
    enabled: bool,
    partitions_enabled: bool,
}

impl Injectable for WorkspaceController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Workspaces>(),
            ProviderKey::of::<Members>(),
            ProviderKey::of::<Invitations>(),
            ProviderKey::of::<Partitions>(),
            ProviderKey::of::<DemoWorkspaces>(),
            ProviderKey::of::<WorkspaceConfig>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let config = r.get::<WorkspaceConfig>().await?;
            Ok(WorkspaceController {
                workspaces: r.get::<Workspaces>().await?,
                members: r.get::<Members>().await?,
                invitations: r.get::<Invitations>().await?,
                partitions: r.get::<Partitions>().await?,
                demos: r.get::<DemoWorkspaces>().await?,
                guest_role: config.demo.guest_role.as_deref().map(WorkspaceRole::parse),
                enabled: config.routes,
                partitions_enabled: config.routes && config.partitions.enabled,
            })
        })
    }
}

/// The caller's workspaces with their role in each, by name: their
/// memberships, plus the demo workspaces they enter as guests when
/// `workspace.demo.guest_role` is set.
async fn visible_workspaces(
    workspaces: &Workspaces,
    demos: &DemoWorkspaces,
    guest_role: Option<&WorkspaceRole>,
    user: Uuid,
) -> Result<Vec<WorkspaceResource>, AppError> {
    let mut visible: Vec<WorkspaceResource> = workspaces
        .for_user(user)
        .await?
        .into_iter()
        .map(|membership| WorkspaceResource::new(&membership.workspace, membership.role))
        .collect();
    if let Some(role) = guest_role {
        let guest_of: Vec<WorkspaceResource> = demos
            .all()
            .await?
            .iter()
            .filter(|demo| !visible.iter().any(|joined| joined.id == demo.id))
            .map(|demo| WorkspaceResource::new(demo, role.clone()))
            .collect();
        if !guest_of.is_empty() {
            visible.extend(guest_of);
            visible.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        }
    }
    Ok(visible)
}

/// Granting `owner`, or touching a member who is an owner, takes the
/// `owner` role.
fn check_owner_change(
    ws: &CurrentWorkspace,
    current: Option<&WorkspaceRole>,
    new: Option<&WorkspaceRole>,
) -> Result<(), AppError> {
    let touches_owner =
        current == Some(&WorkspaceRole::Owner) || new == Some(&WorkspaceRole::Owner);
    if touches_owner && ws.role() != &WorkspaceRole::Owner {
        return Err(AppError::forbidden()
            .with_code("owner_role_required")
            .with_detail("Only an owner can grant or change the owner role."));
    }
    Ok(())
}

impl Controller for WorkspaceController {
    fn prefix(&self) -> &'static str {
        "/workspaces"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let (workspaces, demos) = (self.workspaces.clone(), self.demos.clone());
        let guest_role = self.guest_role.clone();
        let index = move |CurrentPrincipal(me): CurrentPrincipal| {
            let (workspaces, demos) = (workspaces.clone(), demos.clone());
            let guest_role = guest_role.clone();
            async move {
                let data = visible_workspaces(&workspaces, &demos, guest_role.as_ref(), me.user_id)
                    .await?;
                Ok::<_, AppError>(Json(DataList::<WorkspaceResource> { data }))
            }
        };
        let workspaces = self.workspaces.clone();
        let store = move |CurrentPrincipal(me): CurrentPrincipal,
                          uow: Uow,
                          uri: Uri,
                          Json(input): Json<CreateWorkspaceInput>| {
            let workspaces = workspaces.clone();
            async move {
                let (timezone, country) =
                    check_time_zone(input.timezone.as_deref(), input.country.as_deref())?;
                let mut created = workspaces
                    .create(
                        &uow,
                        me.user_id,
                        NewWorkspace {
                            name: input.name,
                            slug: input.slug,
                        },
                    )
                    .await?;
                if timezone.is_some() || country.is_some() {
                    created = workspaces
                        .set_time_zone(&uow, created.id, timezone.as_deref(), country.as_deref())
                        .await?;
                }
                let location = format!("{}/{}", uri.path().trim_end_matches('/'), created.slug);
                Ok::<_, AppError>(Created(
                    location,
                    WorkspaceResource::new(&created, WorkspaceRole::Owner),
                ))
            }
        };
        let show = |ws: CurrentWorkspace| async move {
            Json(WorkspaceResource::new(ws.workspace(), ws.role().clone()))
        };
        let workspaces = self.workspaces.clone();
        let rename =
            move |ws: CurrentWorkspace, uow: Uow, Json(input): Json<RenameWorkspaceInput>| {
                let workspaces = workspaces.clone();
                async move {
                    let renamed = workspaces.rename(&uow, ws.id(), &input.name).await?;
                    Ok::<_, AppError>(Json(WorkspaceResource::new(&renamed, ws.role().clone())))
                }
            };
        let workspaces = self.workspaces.clone();
        let set_time_zone =
            move |ws: CurrentWorkspace, uow: Uow, Json(input): Json<WorkspaceTimeZoneInput>| {
                let workspaces = workspaces.clone();
                async move {
                    let updated = workspaces
                        .set_time_zone(
                            &uow,
                            ws.id(),
                            input.timezone.as_deref(),
                            input.country.as_deref(),
                        )
                        .await?;
                    Ok::<_, AppError>(Json(WorkspaceResource::new(&updated, ws.role().clone())))
                }
            };
        let workspaces = self.workspaces.clone();
        let destroy = move |ws: CurrentWorkspace, uow: Uow| {
            let workspaces = workspaces.clone();
            async move {
                workspaces.delete(&uow, ws.id()).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let workspaces = self.workspaces.clone();
        let transfer =
            move |ws: CurrentWorkspace, uow: Uow, Json(input): Json<TransferWorkspaceInput>| {
                let workspaces = workspaces.clone();
                async move {
                    let updated = workspaces
                        .transfer_ownership(&uow, ws.id(), input.user_id)
                        .await?;
                    let role = if updated.owner_id == ws.member().user_id {
                        WorkspaceRole::Owner
                    } else {
                        WorkspaceRole::Admin
                    };
                    Ok::<_, AppError>(Json(WorkspaceResource::new(&updated, role)))
                }
            };
        let members = self.members.clone();
        let leave = move |ws: CurrentWorkspace, uow: Uow| {
            let members = members.clone();
            async move {
                members.remove(&uow, ws.id(), ws.member().user_id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let members = self.members.clone();
        let list_members = move |ws: CurrentWorkspace| {
            let members = members.clone();
            async move {
                let data = members
                    .list(ws.id())
                    .await?
                    .into_iter()
                    .map(MemberResource::from)
                    .collect();
                Ok::<_, AppError>(Json(DataList::<MemberResource> { data }))
            }
        };
        let members = self.members.clone();
        let add_member =
            move |ws: CurrentWorkspace, uow: Uow, uri: Uri, Json(input): Json<AddMemberInput>| {
                let members = members.clone();
                async move {
                    let role = input.role.unwrap_or(WorkspaceRole::Member);
                    check_owner_change(&ws, None, Some(&role))?;
                    let added = members.add(&uow, ws.id(), input.user_id, role).await?;
                    let location =
                        format!("{}/{}", uri.path().trim_end_matches('/'), added.user_id);
                    Ok::<_, AppError>(Created(location, MemberResource::new(&added, None)))
                }
            };
        let members = self.members.clone();
        let change_role = move |ws: CurrentWorkspace,
                                uow: Uow,
                                Path((_, user)): Path<(String, Uuid)>,
                                Json(input): Json<ChangeRoleInput>| {
            let members = members.clone();
            async move {
                let current = members.find(ws.id(), user).await?.map(|m| m.role());
                check_owner_change(&ws, current.as_ref(), Some(&input.role))?;
                let changed = members.change_role(&uow, ws.id(), user, input.role).await?;
                Ok::<_, AppError>(Json(MemberResource::new(&changed, None)))
            }
        };
        let members = self.members.clone();
        let remove_member =
            move |ws: CurrentWorkspace, uow: Uow, Path((_, user)): Path<(String, Uuid)>| {
                let members = members.clone();
                async move {
                    let current = members.find(ws.id(), user).await?.map(|m| m.role());
                    check_owner_change(&ws, current.as_ref(), None)?;
                    members.remove(&uow, ws.id(), user).await?;
                    Ok::<_, AppError>(NoContent)
                }
            };
        let invitations = self.invitations.clone();
        let list_invitations = move |ws: CurrentWorkspace| {
            let invitations = invitations.clone();
            async move {
                let data = invitations
                    .pending(ws.id())
                    .await?
                    .iter()
                    .map(InvitationResource::from)
                    .collect();
                Ok::<_, AppError>(Json(DataList::<InvitationResource> { data }))
            }
        };
        let invitations = self.invitations.clone();
        let invite =
            move |ws: CurrentWorkspace, uow: Uow, uri: Uri, Json(input): Json<InviteInput>| {
                let invitations = invitations.clone();
                async move {
                    let role = input.role.unwrap_or(WorkspaceRole::Member);
                    let sent = invitations
                        .invite(&uow, ws.id(), &input.email, role, Some(ws.member().user_id))
                        .await?;
                    let location = format!(
                        "{}/{}",
                        uri.path().trim_end_matches('/'),
                        sent.invitation.id
                    );
                    Ok::<_, AppError>(Created(
                        location,
                        InvitationResource::from(&sent.invitation),
                    ))
                }
            };
        let invitations = self.invitations.clone();
        let revoke =
            move |ws: CurrentWorkspace, uow: Uow, Path((_, invitation)): Path<(String, Uuid)>| {
                let invitations = invitations.clone();
                async move {
                    invitations.revoke(&uow, ws.id(), invitation).await?;
                    Ok::<_, AppError>(NoContent)
                }
            };
        let guard = || InWorkspace::from_path(WORKSPACE_PARAM);
        // Everything but `show` is for members: a demo's guests see the workspace only.
        let members_only = || InWorkspace::members_from_path(WORKSPACE_PARAM);
        let mut routes = vec![
            RouteDef::on(Method::GET, "/", index)
                .authenticated()
                .name("workspaces.index")
                .api(api("List the caller's workspaces", &[401], None, None)),
            RouteDef::on(Method::POST, "/", store)
                .authenticated()
                .name("workspaces.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Create a workspace owned by the caller",
                        &[400, 401],
                        Some(schema_fn::<CreateWorkspaceInput>),
                        Some(schema_fn::<WorkspaceResource>),
                    )
                }),
            RouteDef::on(Method::GET, "/{workspace}", show)
                .authenticated()
                .guard(guard())
                .name("workspaces.show")
                .api(api(
                    "Show a workspace with the caller's role",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<WorkspaceResource>),
                )),
            RouteDef::on(Method::PATCH, "/{workspace}", rename)
                .permissions(&["workspace.update"])
                .guard(members_only())
                .name("workspaces.update")
                .api(api(
                    "Rename a workspace",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<RenameWorkspaceInput>),
                    Some(schema_fn::<WorkspaceResource>),
                )),
            RouteDef::on(Method::PUT, "/{workspace}/time-zone", set_time_zone)
                .permissions(&["workspace.update"])
                .guard(members_only())
                .name("workspaces.time_zone")
                .api(api(
                    "Set a workspace's time zone and country",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<WorkspaceTimeZoneInput>),
                    Some(schema_fn::<WorkspaceResource>),
                )),
            RouteDef::on(Method::DELETE, "/{workspace}", destroy)
                .permissions(&["workspace.delete"])
                .guard(members_only())
                .name("workspaces.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Delete a workspace", &[401, 403, 404], None, None)
                }),
            RouteDef::on(Method::POST, "/{workspace}/transfer", transfer)
                .permissions(&["workspace.transfer"])
                .guard(members_only())
                .name("workspaces.transfer")
                .api(api(
                    "Transfer a workspace to another member",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<TransferWorkspaceInput>),
                    Some(schema_fn::<WorkspaceResource>),
                )),
            RouteDef::on(Method::POST, "/{workspace}/leave", leave)
                .authenticated()
                .guard(members_only())
                .name("workspaces.leave")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Leave a workspace", &[401, 403, 404, 409], None, None)
                }),
            RouteDef::on(Method::GET, "/{workspace}/members", list_members)
                .permissions(&["workspace.members.read"])
                .guard(members_only())
                .name("workspaces.members.index")
                .api(api(
                    "List a workspace's members",
                    &[401, 403, 404],
                    None,
                    None,
                )),
            RouteDef::on(Method::POST, "/{workspace}/members", add_member)
                .permissions(&["workspace.members.create"])
                .guard(members_only())
                .name("workspaces.members.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Add a user to a workspace",
                        &[400, 401, 403, 404, 409],
                        Some(schema_fn::<AddMemberInput>),
                        Some(schema_fn::<MemberResource>),
                    )
                }),
            RouteDef::on(Method::PATCH, "/{workspace}/members/{user}", change_role)
                .permissions(&["workspace.members.update"])
                .guard(members_only())
                .name("workspaces.members.update")
                .api(api(
                    "Change a member's role",
                    &[400, 401, 403, 404, 409],
                    Some(schema_fn::<ChangeRoleInput>),
                    Some(schema_fn::<MemberResource>),
                )),
            RouteDef::on(Method::DELETE, "/{workspace}/members/{user}", remove_member)
                .permissions(&["workspace.members.delete"])
                .guard(members_only())
                .name("workspaces.members.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Remove a member", &[401, 403, 404, 409], None, None)
                }),
            RouteDef::on(Method::GET, "/{workspace}/invitations", list_invitations)
                .permissions(&["workspace.invitations.read"])
                .guard(members_only())
                .name("workspaces.invitations.index")
                .api(api(
                    "List pending invitations",
                    &[401, 403, 404],
                    None,
                    None,
                )),
            RouteDef::on(Method::POST, "/{workspace}/invitations", invite)
                .permissions(&["workspace.invitations.create"])
                .guard(members_only())
                .name("workspaces.invitations.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Invite someone by email",
                        &[400, 401, 403, 404, 409],
                        Some(schema_fn::<InviteInput>),
                        Some(schema_fn::<InvitationResource>),
                    )
                }),
            RouteDef::on(
                Method::DELETE,
                "/{workspace}/invitations/{invitation}",
                revoke,
            )
            .permissions(&["workspace.invitations.delete"])
            .guard(members_only())
            .name("workspaces.invitations.destroy")
            .api(ApiMeta {
                success_status: 204,
                ..api("Revoke an invitation", &[401, 403, 404], None, None)
            }),
        ];
        if self.partitions_enabled {
            routes.extend(self.partition_routes());
        }
        routes
    }
}

impl WorkspaceController {
    /// `/workspaces/{workspace}/partitions...` and the members' partition
    /// access.
    fn partition_routes(self: Arc<Self>) -> Vec<RouteDef> {
        let partitions = self.partitions.clone();
        let index = move |ws: CurrentWorkspace, scope: PartitionScope| {
            let partitions = partitions.clone();
            async move {
                let data = partitions
                    .visible(ws.id(), scope.allowed())
                    .await?
                    .iter()
                    .map(PartitionResource::from)
                    .collect();
                Ok::<_, AppError>(Json(DataList::<PartitionResource> { data }))
            }
        };
        let partitions = self.partitions.clone();
        let store = move |ws: CurrentWorkspace,
                          scope: PartitionScope,
                          uow: Uow,
                          uri: Uri,
                          Json(input): Json<CreatePartitionInput>| {
            let partitions = partitions.clone();
            async move {
                check_partition_manager(&scope)?;
                let created = partitions
                    .create(
                        &uow,
                        ws.id(),
                        NewPartition {
                            name: input.name,
                            slug: input.slug,
                        },
                    )
                    .await?;
                let location = format!("{}/{}", uri.path().trim_end_matches('/'), created.slug);
                Ok::<_, AppError>(Created(location, PartitionResource::from(&created)))
            }
        };
        let partitions = self.partitions.clone();
        let rename = move |ws: CurrentWorkspace,
                           scope: PartitionScope,
                           uow: Uow,
                           Path((_, partition)): Path<(String, String)>,
                           Json(input): Json<RenamePartitionInput>| {
            let partitions = partitions.clone();
            async move {
                check_partition_manager(&scope)?;
                let found = partitions.resolve(ws.id(), &partition).await?;
                let renamed = partitions
                    .rename(&uow, ws.id(), found.id, &input.name)
                    .await?;
                Ok::<_, AppError>(Json(PartitionResource::from(&renamed)))
            }
        };
        let partitions = self.partitions.clone();
        let destroy = move |ws: CurrentWorkspace,
                            scope: PartitionScope,
                            uow: Uow,
                            Path((_, partition)): Path<(String, String)>| {
            let partitions = partitions.clone();
            async move {
                check_partition_manager(&scope)?;
                let found = partitions.resolve(ws.id(), &partition).await?;
                partitions.delete(&uow, ws.id(), found.id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let partitions = self.partitions.clone();
        let show_access = move |ws: CurrentWorkspace, Path((_, user)): Path<(String, Uuid)>| {
            let partitions = partitions.clone();
            async move {
                let access = partitions.access(ws.id(), user).await?;
                Ok::<_, AppError>(Json(PartitionAccessResource {
                    user_id: user,
                    partitions: access,
                }))
            }
        };
        let partitions = self.partitions.clone();
        let grant = move |ws: CurrentWorkspace,
                          scope: PartitionScope,
                          uow: Uow,
                          Path((_, user)): Path<(String, Uuid)>,
                          Json(input): Json<PartitionAccessInput>| {
            let partitions = partitions.clone();
            async move {
                check_partition_reach(&scope, &input.partitions)?;
                let current = partitions.access(ws.id(), user).await?;
                check_partition_reach(&scope, &current)?;
                partitions
                    .grant(&uow, ws.id(), user, input.partitions.clone())
                    .await?;
                Ok::<_, AppError>(Json(PartitionAccessResource {
                    user_id: user,
                    partitions: input.partitions,
                }))
            }
        };
        // A demo's guests see the workspace only, not its partitions.
        let guard = || InWorkspace::members_from_path(WORKSPACE_PARAM);
        vec![
            RouteDef::on(Method::GET, "/{workspace}/partitions", index)
                .permissions(&["workspace.partitions.read"])
                .guard(guard())
                .name("workspaces.partitions.index")
                .api(api(
                    "List the partitions the caller sees",
                    &[401, 403, 404],
                    None,
                    None,
                )),
            RouteDef::on(Method::POST, "/{workspace}/partitions", store)
                .permissions(&["workspace.partitions.create"])
                .guard(guard())
                .name("workspaces.partitions.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Create a partition",
                        &[400, 401, 403, 404],
                        Some(schema_fn::<CreatePartitionInput>),
                        Some(schema_fn::<PartitionResource>),
                    )
                }),
            RouteDef::on(Method::PATCH, "/{workspace}/partitions/{partition}", rename)
                .permissions(&["workspace.partitions.update"])
                .guard(guard())
                .name("workspaces.partitions.update")
                .api(api(
                    "Rename a partition",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<RenamePartitionInput>),
                    Some(schema_fn::<PartitionResource>),
                )),
            RouteDef::on(
                Method::DELETE,
                "/{workspace}/partitions/{partition}",
                destroy,
            )
            .permissions(&["workspace.partitions.delete"])
            .guard(guard())
            .name("workspaces.partitions.destroy")
            .api(ApiMeta {
                success_status: 204,
                ..api("Delete a partition", &[401, 403, 404], None, None)
            }),
            RouteDef::on(
                Method::GET,
                "/{workspace}/members/{user}/partitions",
                show_access,
            )
            .permissions(&["workspace.members.read"])
            .guard(guard())
            .name("workspaces.members.partitions.show")
            .api(api(
                "Show the partitions a member sees",
                &[401, 403, 404],
                None,
                Some(schema_fn::<PartitionAccessResource>),
            )),
            RouteDef::on(Method::PUT, "/{workspace}/members/{user}/partitions", grant)
                .permissions(&["workspace.members.update"])
                .guard(guard())
                .name("workspaces.members.partitions.update")
                .api(api(
                    "Set the partitions a member sees",
                    &[400, 401, 403, 404, 409],
                    Some(schema_fn::<PartitionAccessInput>),
                    Some(schema_fn::<PartitionAccessResource>),
                )),
        ]
    }
}

/// `POST {http.api_prefix}/invitations/accept` `{token}`: the signed-in
/// user accepts an invitation and joins its workspace. 404 for an unknown,
/// revoked or used token, 410 once it has expired, 403 when it was sent to
/// another address, 409 for a member.
pub struct InvitationController {
    invitations: Arc<Invitations>,
    enabled: bool,
}

impl Injectable for InvitationController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Invitations>(),
            ProviderKey::of::<WorkspaceConfig>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(InvitationController {
                invitations: r.get::<Invitations>().await?,
                enabled: r.get::<WorkspaceConfig>().await?.routes,
            })
        })
    }
}

impl Controller for InvitationController {
    fn prefix(&self) -> &'static str {
        "/invitations"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let invitations = self.invitations.clone();
        let accept = move |CurrentPrincipal(me): CurrentPrincipal,
                           uow: Uow,
                           Json(input): Json<AcceptInvitationInput>| {
            let invitations = invitations.clone();
            async move {
                let AcceptedInvitation { workspace, member } =
                    invitations.accept(&uow, &input.token, me.user_id).await?;
                Ok::<_, AppError>(Json(AcceptedInvitationResource {
                    workspace: WorkspaceResource::new(&workspace, member.role()),
                    member: MemberResource::new(&member, None),
                }))
            }
        };
        vec![
            RouteDef::on(Method::POST, "/accept", accept)
                .authenticated()
                .throttle(10, std::time::Duration::from_secs(60))
                .name("invitations.accept")
                .api(api(
                    "Accept an invitation",
                    &[400, 401, 403, 404, 409, 410, 429],
                    Some(schema_fn::<AcceptInvitationInput>),
                    Some(schema_fn::<AcceptedInvitationResource>),
                )),
        ]
    }
}

fn api(
    summary: &str,
    responses: &[u16],
    request: Option<navree_http::SchemaFn>,
    response: Option<navree_http::SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Workspaces".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}
