//! The workspace routes: `/api/workspaces...` and
//! `POST /api/invitations/accept`.

use std::sync::Arc;

use axum::extract::Path;
use chrono::{DateTime, Utc};
use http::{Method, Uri};
use navree_db::Uow;
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::current::{CurrentWorkspace, InWorkspace};
use crate::directory::UserSummary;
use crate::entity::{Invitation, Member, Workspace};
use crate::invitations::AcceptedInvitation;
use crate::members::MemberInfo;
use crate::workspaces::NewWorkspace;
use crate::{Invitations, Members, WorkspaceConfig, WorkspaceRole, Workspaces};

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
    /// The caller's role in the workspace.
    #[schema(value_type = String, example = "owner")]
    pub role: WorkspaceRole,
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
            role,
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
struct DataList<T> {
    data: Vec<T>,
}

/// The body of `POST /workspaces`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateWorkspaceInput {
    /// The display name.
    pub name: String,
    /// The slug; derived from the name when absent.
    pub slug: Option<String>,
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
/// | `GET /workspaces` | | the caller's workspaces, `{data: [..]}` |
/// | `POST /workspaces` `{name, slug?}` | | 201, the workspace (caller is owner) |
/// | `GET /workspaces/{workspace}` | (member) | the workspace with the caller's role |
/// | `PATCH /workspaces/{workspace}` `{name}` | `workspace.update` | the workspace |
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
/// also takes the `owner` role (a 403 otherwise). `POST
/// /invitations/accept` is [`InvitationController`].
pub struct WorkspaceController {
    workspaces: Arc<Workspaces>,
    members: Arc<Members>,
    invitations: Arc<Invitations>,
    enabled: bool,
}

impl Injectable for WorkspaceController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Workspaces>(),
            ProviderKey::of::<Members>(),
            ProviderKey::of::<Invitations>(),
            ProviderKey::of::<WorkspaceConfig>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(WorkspaceController {
                workspaces: r.get::<Workspaces>().await?,
                members: r.get::<Members>().await?,
                invitations: r.get::<Invitations>().await?,
                enabled: r.get::<WorkspaceConfig>().await?.routes,
            })
        })
    }
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
        return Err(
            AppError::forbidden().with_detail("Only an owner can grant or change the owner role.")
        );
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
        let workspaces = self.workspaces.clone();
        let index = move |CurrentPrincipal(me): CurrentPrincipal| {
            let workspaces = workspaces.clone();
            async move {
                let data = workspaces
                    .for_user(me.user_id)
                    .await?
                    .into_iter()
                    .map(|membership| {
                        WorkspaceResource::new(&membership.workspace, membership.role)
                    })
                    .collect();
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
                let created = workspaces
                    .create(
                        &uow,
                        me.user_id,
                        NewWorkspace {
                            name: input.name,
                            slug: input.slug,
                        },
                    )
                    .await?;
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
        vec![
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
                .guard(guard())
                .name("workspaces.update")
                .api(api(
                    "Rename a workspace",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<RenameWorkspaceInput>),
                    Some(schema_fn::<WorkspaceResource>),
                )),
            RouteDef::on(Method::DELETE, "/{workspace}", destroy)
                .permissions(&["workspace.delete"])
                .guard(guard())
                .name("workspaces.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Delete a workspace", &[401, 403, 404], None, None)
                }),
            RouteDef::on(Method::POST, "/{workspace}/transfer", transfer)
                .permissions(&["workspace.transfer"])
                .guard(guard())
                .name("workspaces.transfer")
                .api(api(
                    "Transfer a workspace to another member",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<TransferWorkspaceInput>),
                    Some(schema_fn::<WorkspaceResource>),
                )),
            RouteDef::on(Method::POST, "/{workspace}/leave", leave)
                .authenticated()
                .guard(guard())
                .name("workspaces.leave")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Leave a workspace", &[401, 403, 404, 409], None, None)
                }),
            RouteDef::on(Method::GET, "/{workspace}/members", list_members)
                .permissions(&["workspace.members.read"])
                .guard(guard())
                .name("workspaces.members.index")
                .api(api(
                    "List a workspace's members",
                    &[401, 403, 404],
                    None,
                    None,
                )),
            RouteDef::on(Method::POST, "/{workspace}/members", add_member)
                .permissions(&["workspace.members.create"])
                .guard(guard())
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
                .guard(guard())
                .name("workspaces.members.update")
                .api(api(
                    "Change a member's role",
                    &[400, 401, 403, 404, 409],
                    Some(schema_fn::<ChangeRoleInput>),
                    Some(schema_fn::<MemberResource>),
                )),
            RouteDef::on(Method::DELETE, "/{workspace}/members/{user}", remove_member)
                .permissions(&["workspace.members.delete"])
                .guard(guard())
                .name("workspaces.members.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Remove a member", &[401, 403, 404, 409], None, None)
                }),
            RouteDef::on(Method::GET, "/{workspace}/invitations", list_invitations)
                .permissions(&["workspace.invitations.read"])
                .guard(guard())
                .name("workspaces.invitations.index")
                .api(api(
                    "List pending invitations",
                    &[401, 403, 404],
                    None,
                    None,
                )),
            RouteDef::on(Method::POST, "/{workspace}/invitations", invite)
                .permissions(&["workspace.invitations.create"])
                .guard(guard())
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
            .guard(guard())
            .name("workspaces.invitations.destroy")
            .api(ApiMeta {
                success_status: 204,
                ..api("Revoke an invitation", &[401, 403, 404], None, None)
            }),
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
