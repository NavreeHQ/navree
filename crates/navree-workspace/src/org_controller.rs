//! The organization routes: `/api/organizations...`.

use std::sync::Arc;

use axum::extract::Path;
use chrono::{DateTime, Utc};
use http::{Method, Uri};
use navree_db::Uow;
use navree_http::{
    ApiMeta, AppError, Controller, Created, CurrentPrincipal, Json, NoContent, RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::access_requests::{AccessRequestInfo, ApprovedRequest};
use crate::controller::{DataList, WorkspaceResource};
use crate::directory::UserSummary;
use crate::entity::{AccessRequest, OrgMember, Organization};
use crate::org_current::{CurrentOrganization, InOrganization};
use crate::org_members::OrgMemberInfo;
use crate::organizations::{NewOrganization, OrganizationChanges};
use crate::{
    AccessRequestStatus, AccessRequests, Approval, NewAccessRequest, OrgMembers, OrgRole,
    Organizations, Selector, WorkspaceConfig, WorkspaceError, WorkspaceSelector, Workspaces,
};

/// The path parameter naming the organization (a slug or an id).
const ORGANIZATION_PARAM: &str = "organization";

/// An organization as the API returns it, with the caller's role and the
/// workspaces they reach.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct OrganizationResource {
    /// Always `Organization` (for CASL subject detection).
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "Organization")]
    pub kind: String,
    /// The id.
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The unique slug.
    pub slug: String,
    /// The owner.
    pub owner_id: Uuid,
    /// The ISO 3166-1 alpha-2 country code.
    pub country: Option<String>,
    /// The IANA time zone.
    pub timezone: Option<String>,
    /// The caller's organization role.
    #[schema(value_type = String, example = "admin")]
    pub role: OrgRole,
    /// The workspaces the caller reaches: `"all"` or a list of ids.
    #[schema(value_type = Object)]
    pub workspaces: WorkspaceSelector,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl OrganizationResource {
    /// The resource of `organization` for a caller with `role` reaching
    /// `workspaces`.
    pub fn new(organization: &Organization, role: OrgRole, workspaces: WorkspaceSelector) -> Self {
        Self {
            kind: "Organization".to_owned(),
            id: organization.id,
            name: organization.name.clone(),
            slug: organization.slug.clone(),
            owner_id: organization.owner_id,
            country: organization.country.clone(),
            timezone: organization.timezone.clone(),
            role,
            workspaces,
            created_at: organization.created_at,
            updated_at: organization.updated_at,
        }
    }
}

/// An organization member as the API returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct OrgMemberResource {
    /// Always `OrganizationMember`.
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "OrganizationMember")]
    pub kind: String,
    /// The membership id.
    pub id: Uuid,
    /// The organization.
    pub organization_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// The organization role.
    #[schema(value_type = String, example = "member")]
    pub role: OrgRole,
    /// The workspaces they reach: `"all"` or a list of ids.
    #[schema(value_type = Object)]
    pub workspaces: WorkspaceSelector,
    /// The user's name and email, when the user directory knows them.
    pub user: Option<UserSummary>,
    /// When the user joined.
    pub created_at: DateTime<Utc>,
    /// When the membership last changed.
    pub updated_at: DateTime<Utc>,
}

impl OrgMemberResource {
    /// The resource of `member` reaching `workspaces` (with `user` when
    /// known).
    pub fn new(
        member: &OrgMember,
        workspaces: WorkspaceSelector,
        user: Option<UserSummary>,
    ) -> Self {
        Self {
            kind: "OrganizationMember".to_owned(),
            id: member.id,
            organization_id: member.organization_id,
            user_id: member.user_id,
            role: member.role(),
            workspaces,
            user,
            created_at: member.created_at,
            updated_at: member.updated_at,
        }
    }
}

impl From<OrgMemberInfo> for OrgMemberResource {
    fn from(info: OrgMemberInfo) -> Self {
        OrgMemberResource::new(&info.member, info.workspaces, info.user)
    }
}

/// An access request as the API returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AccessRequestResource {
    /// Always `AccessRequest`.
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "AccessRequest")]
    pub kind: String,
    /// The id.
    pub id: Uuid,
    /// The organization.
    pub organization_id: Uuid,
    /// The workspace asked for, if one.
    pub workspace_id: Option<Uuid>,
    /// Who asks.
    pub user_id: Uuid,
    /// Their message.
    pub message: Option<String>,
    /// `pending`, `approved` or `declined`.
    #[schema(value_type = String, example = "pending")]
    pub status: AccessRequestStatus,
    /// The organization role given on approval.
    pub role: Option<String>,
    /// Who decided.
    pub decided_by: Option<Uuid>,
    /// When it was decided.
    pub decided_at: Option<DateTime<Utc>>,
    /// Why it was declined.
    pub reason: Option<String>,
    /// The requesting user, when the user directory knows them.
    pub user: Option<UserSummary>,
    /// When it was made.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl AccessRequestResource {
    /// The resource of `request` (with `user` when known).
    pub fn new(request: &AccessRequest, user: Option<UserSummary>) -> Self {
        Self {
            kind: "AccessRequest".to_owned(),
            id: request.id,
            organization_id: request.organization_id,
            workspace_id: request.workspace_id,
            user_id: request.user_id,
            message: request.message.clone(),
            status: request.status(),
            role: request.role.clone(),
            decided_by: request.decided_by,
            decided_at: request.decided_at,
            reason: request.reason.clone(),
            user,
            created_at: request.created_at,
            updated_at: request.updated_at,
        }
    }
}

impl From<AccessRequestInfo> for AccessRequestResource {
    fn from(info: AccessRequestInfo) -> Self {
        AccessRequestResource::new(&info.request, info.user)
    }
}

/// What approving an access request returns.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ApprovedAccessRequestResource {
    /// The request, now `approved`.
    pub request: AccessRequestResource,
    /// The new membership.
    pub member: OrgMemberResource,
}

/// The body of `POST /organizations`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CreateOrganizationInput {
    /// The display name.
    pub name: String,
    /// The slug; derived from the name when absent.
    pub slug: Option<String>,
    /// An ISO 3166-1 alpha-2 country code.
    pub country: Option<String>,
    /// An IANA time zone name.
    pub timezone: Option<String>,
}

/// The body of `PATCH /organizations/{organization}`: absent fields stay,
/// `null` clears `country` or `timezone`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct UpdateOrganizationInput {
    /// The new name.
    pub name: Option<String>,
    /// The new country code, or `null`.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub country: Option<Option<String>>,
    /// The new time zone, or `null`.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub timezone: Option<Option<String>>,
}

/// A field that is present (`Some`), with `null` as `Some(None)`.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

/// The body of `POST /organizations/{organization}/transfer`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct TransferOrganizationInput {
    /// The member who becomes the owner.
    pub user_id: Uuid,
}

/// The body of `POST /organizations/{organization}/members`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct AddOrgMemberInput {
    /// The user to add.
    pub user_id: Uuid,
    /// Their organization role (default `member`; never `owner`).
    #[schema(value_type = Option<String>)]
    pub role: Option<OrgRole>,
    /// The workspaces they reach: `"all"` (the default) or a list of ids.
    #[schema(value_type = Option<Object>)]
    pub workspaces: Option<WorkspaceSelector>,
}

/// The body of `PATCH /organizations/{organization}/members/{user}`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct UpdateOrgMemberInput {
    /// The new role.
    #[schema(value_type = Option<String>)]
    pub role: Option<OrgRole>,
    /// The workspaces they reach now.
    #[schema(value_type = Option<Object>)]
    pub workspaces: Option<WorkspaceSelector>,
}

/// The body of `POST /organizations/{organization}/workspaces`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct AttachWorkspaceInput {
    /// The workspace to attach: a slug or an id.
    pub workspace: String,
}

/// The body of `POST /organizations/{organization}/access-requests`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct RequestAccessInput {
    /// A workspace of the organization to ask for.
    pub workspace_id: Option<Uuid>,
    /// A message to the admins.
    pub message: Option<String>,
}

/// The optional body of `POST .../access-requests/{request}/approve`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct ApproveAccessRequestInput {
    /// The organization role (default `member`).
    #[schema(value_type = Option<String>)]
    pub role: Option<OrgRole>,
    /// The workspaces reached (default: the one asked for, else `"all"`).
    #[schema(value_type = Option<Object>)]
    pub workspaces: Option<WorkspaceSelector>,
}

/// The optional body of `POST .../access-requests/{request}/decline`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct DeclineAccessRequestInput {
    /// Why it is declined.
    pub reason: Option<String>,
}

/// The organization routes, mounted under `{http.api_prefix}/organizations`
/// when `workspace.routes` and `workspace.organizations.enabled` are on.
/// `{organization}` is a slug or an id; every route needs a signed-in user,
/// and the `{organization}` routes (except asking for access) a member of
/// it ([`InOrganization::from_path`]), with these permissions in it:
///
/// | Route | Permission | Answer |
/// |---|---|---|
/// | `GET /organizations` | | the caller's organizations, `{data: [..]}` |
/// | `POST /organizations` `{name, slug?, country?, timezone?}` | | 201, the organization (caller is owner) |
/// | `GET /organizations/{organization}` | (member) | the organization with the caller's role |
/// | `PATCH /organizations/{organization}` `{name?, country?, timezone?}` | `organization.update` | the organization |
/// | `DELETE /organizations/{organization}` | `organization.delete` | 204; workspaces are detached |
/// | `POST /organizations/{organization}/transfer` `{user_id}` | `organization.transfer` | the organization |
/// | `POST /organizations/{organization}/leave` | (member) | 204; 409 for the owner |
/// | `GET /organizations/{organization}/members` | `organization.members.read` | `{data: [..]}` |
/// | `POST /organizations/{organization}/members` `{user_id, role?, workspaces?}` | `organization.members.create` | 201, the member |
/// | `PATCH /organizations/{organization}/members/{user}` `{role?, workspaces?}` | `organization.members.update` | the member |
/// | `DELETE /organizations/{organization}/members/{user}` | `organization.members.delete` | 204 |
/// | `GET /organizations/{organization}/workspaces` | `organization.read` | the workspaces the caller reaches, `{data: [..]}` |
/// | `POST /organizations/{organization}/workspaces` `{workspace}` | `organization.attach` (and owning the workspace) | the workspace |
/// | `DELETE /organizations/{organization}/workspaces/{workspace}` | `organization.detach` | 204 |
/// | `GET /organizations/{organization}/access-requests` | `organization.access_requests.read` | pending, `{data: [..]}` |
/// | `POST /organizations/{organization}/access-requests` `{workspace_id?, message?}` | (signed in) | 201, the request |
/// | `POST .../access-requests/{request}/approve` `{role?, workspaces?}` | `organization.access_requests.update` | `{request, member}` |
/// | `POST .../access-requests/{request}/decline` `{reason?}` | `organization.access_requests.update` | the request |
///
/// A caller limited to some workspaces grants only workspaces they reach,
/// and changes only members whose workspaces they reach (a 403
/// otherwise).
pub struct OrganizationController {
    organizations: Arc<Organizations>,
    members: Arc<OrgMembers>,
    requests: Arc<AccessRequests>,
    workspaces: Arc<Workspaces>,
    enabled: bool,
}

impl Injectable for OrganizationController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Organizations>(),
            ProviderKey::of::<OrgMembers>(),
            ProviderKey::of::<AccessRequests>(),
            ProviderKey::of::<Workspaces>(),
            ProviderKey::of::<WorkspaceConfig>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let config = r.get::<WorkspaceConfig>().await?;
            Ok(OrganizationController {
                organizations: r.get::<Organizations>().await?,
                members: r.get::<OrgMembers>().await?,
                requests: r.get::<AccessRequests>().await?,
                workspaces: r.get::<Workspaces>().await?,
                enabled: config.routes && config.organizations.enabled,
            })
        })
    }
}

/// A 403 unless the caller reaches every workspace `selector` names.
fn check_reach(org: &CurrentOrganization, selector: &WorkspaceSelector) -> Result<(), AppError> {
    let covered = match (org.workspaces(), selector) {
        (Selector::All, _) => true,
        (Selector::Only(_), Selector::All) => false,
        (Selector::Only(mine), Selector::Only(theirs)) => theirs.iter().all(|id| mine.contains(id)),
    };
    if covered {
        Ok(())
    } else {
        Err(AppError::forbidden()
            .with_code("workspace_grant_forbidden")
            .with_detail("You can only grant workspaces you reach."))
    }
}

/// A 403 when the caller is limited and `user` reaches workspaces the
/// caller doesn't.
async fn check_member_reach(
    members: &OrgMembers,
    org: &CurrentOrganization,
    user: Uuid,
) -> Result<(), AppError> {
    if org.workspaces().is_all() {
        return Ok(());
    }
    match members.selector(org.id(), user).await? {
        Some(selector) => check_reach(org, &selector),
        None => Ok(()),
    }
}

impl Controller for OrganizationController {
    fn prefix(&self) -> &'static str {
        "/organizations"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.enabled {
            return Vec::new();
        }
        let organizations = self.organizations.clone();
        let index = move |CurrentPrincipal(me): CurrentPrincipal| {
            let organizations = organizations.clone();
            async move {
                let data = organizations
                    .for_user(me.user_id)
                    .await?
                    .into_iter()
                    .map(|m| OrganizationResource::new(&m.organization, m.role, m.workspaces))
                    .collect();
                Ok::<_, AppError>(Json(DataList::<OrganizationResource> { data }))
            }
        };
        let organizations = self.organizations.clone();
        let store = move |CurrentPrincipal(me): CurrentPrincipal,
                          uow: Uow,
                          uri: Uri,
                          Json(input): Json<CreateOrganizationInput>| {
            let organizations = organizations.clone();
            async move {
                let created = organizations
                    .create(
                        &uow,
                        me.user_id,
                        NewOrganization {
                            name: input.name,
                            slug: input.slug,
                            country: input.country,
                            timezone: input.timezone,
                        },
                    )
                    .await?;
                let location = format!("{}/{}", uri.path().trim_end_matches('/'), created.slug);
                Ok::<_, AppError>(Created(
                    location,
                    OrganizationResource::new(&created, OrgRole::Owner, Selector::All),
                ))
            }
        };
        let show = |org: CurrentOrganization| async move {
            Json(OrganizationResource::new(
                org.organization(),
                org.role().clone(),
                org.workspaces().clone(),
            ))
        };
        let organizations = self.organizations.clone();
        let update = move |org: CurrentOrganization,
                           uow: Uow,
                           Json(input): Json<UpdateOrganizationInput>| {
            let organizations = organizations.clone();
            async move {
                let updated = organizations
                    .update(
                        &uow,
                        org.id(),
                        OrganizationChanges {
                            name: input.name,
                            country: input.country,
                            timezone: input.timezone,
                        },
                    )
                    .await?;
                Ok::<_, AppError>(Json(OrganizationResource::new(
                    &updated,
                    org.role().clone(),
                    org.workspaces().clone(),
                )))
            }
        };
        let organizations = self.organizations.clone();
        let destroy = move |org: CurrentOrganization, uow: Uow| {
            let organizations = organizations.clone();
            async move {
                organizations.delete(&uow, org.id()).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let organizations = self.organizations.clone();
        let transfer = move |org: CurrentOrganization,
                             uow: Uow,
                             Json(input): Json<TransferOrganizationInput>| {
            let organizations = organizations.clone();
            async move {
                let updated = organizations
                    .transfer_ownership(&uow, org.id(), input.user_id)
                    .await?;
                let role = if updated.owner_id == org.member().user_id {
                    OrgRole::Owner
                } else {
                    OrgRole::Admin
                };
                Ok::<_, AppError>(Json(OrganizationResource::new(
                    &updated,
                    role,
                    org.workspaces().clone(),
                )))
            }
        };
        let members = self.members.clone();
        let leave = move |org: CurrentOrganization, uow: Uow| {
            let members = members.clone();
            async move {
                members.remove(&uow, org.id(), org.member().user_id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let members = self.members.clone();
        let list_members = move |org: CurrentOrganization| {
            let members = members.clone();
            async move {
                let data = members
                    .list(org.id())
                    .await?
                    .into_iter()
                    .map(OrgMemberResource::from)
                    .collect();
                Ok::<_, AppError>(Json(DataList::<OrgMemberResource> { data }))
            }
        };
        let members = self.members.clone();
        let add_member = move |org: CurrentOrganization,
                               uow: Uow,
                               uri: Uri,
                               Json(input): Json<AddOrgMemberInput>| {
            let members = members.clone();
            async move {
                let role = input.role.unwrap_or(OrgRole::Member);
                let workspaces = input.workspaces.unwrap_or_default();
                check_reach(&org, &workspaces)?;
                let added = members
                    .add(&uow, org.id(), input.user_id, role, workspaces.clone())
                    .await?;
                let location = format!("{}/{}", uri.path().trim_end_matches('/'), added.user_id);
                Ok::<_, AppError>(Created(
                    location,
                    OrgMemberResource::new(&added, workspaces, None),
                ))
            }
        };
        let members = self.members.clone();
        let update_member =
            move |org: CurrentOrganization,
                  uow: Uow,
                  Path((_, user)): Path<(String, Uuid)>,
                  Json(input): Json<UpdateOrgMemberInput>| {
                let members = members.clone();
                async move {
                    check_member_reach(&members, &org, user).await?;
                    if let Some(workspaces) = &input.workspaces {
                        check_reach(&org, workspaces)?;
                    }
                    let changed = members
                        .update(&uow, org.id(), user, input.role, input.workspaces)
                        .await?;
                    let workspaces = members.selector(org.id(), user).await?.unwrap_or_default();
                    Ok::<_, AppError>(Json(OrgMemberResource::new(&changed, workspaces, None)))
                }
            };
        let members = self.members.clone();
        let remove_member =
            move |org: CurrentOrganization, uow: Uow, Path((_, user)): Path<(String, Uuid)>| {
                let members = members.clone();
                async move {
                    check_member_reach(&members, &org, user).await?;
                    members.remove(&uow, org.id(), user).await?;
                    Ok::<_, AppError>(NoContent)
                }
            };
        let workspaces = self.workspaces.clone();
        let organizations = self.organizations.clone();
        let list_workspaces = move |org: CurrentOrganization| {
            let (workspaces, organizations) = (workspaces.clone(), organizations.clone());
            async move {
                let mut data = Vec::new();
                for workspace in organizations.workspaces(org.id()).await? {
                    if !org.reaches(workspace.id) {
                        continue;
                    }
                    let role = match workspaces
                        .role_of(workspace.id, org.member().user_id)
                        .await?
                    {
                        Some((role, _)) => role,
                        None => continue,
                    };
                    data.push(WorkspaceResource::new(&workspace, role));
                }
                Ok::<_, AppError>(Json(DataList::<WorkspaceResource> { data }))
            }
        };
        let workspaces = self.workspaces.clone();
        let organizations = self.organizations.clone();
        let attach =
            move |org: CurrentOrganization, uow: Uow, Json(input): Json<AttachWorkspaceInput>| {
                let (workspaces, organizations) = (workspaces.clone(), organizations.clone());
                async move {
                    let workspace = workspaces.resolve(input.workspace.trim()).await?;
                    if workspace.owner_id != org.member().user_id {
                        return Err(AppError::forbidden()
                            .with_code("not_the_workspace_owner")
                            .with_detail(
                                "Only the workspace's owner can attach it to an organization.",
                            ));
                    }
                    let attached = organizations.attach(&uow, org.id(), workspace.id).await?;
                    Ok::<_, AppError>(Json(WorkspaceResource::new(
                        &attached,
                        crate::WorkspaceRole::Owner,
                    )))
                }
            };
        let workspaces = self.workspaces.clone();
        let organizations = self.organizations.clone();
        let detach = move |org: CurrentOrganization,
                           uow: Uow,
                           Path((_, workspace)): Path<(String, String)>| {
            let (workspaces, organizations) = (workspaces.clone(), organizations.clone());
            async move {
                let workspace = match workspaces.resolve(&workspace).await {
                    Ok(workspace) => workspace,
                    Err(WorkspaceError::WorkspaceNotFound(_)) => {
                        return Err(WorkspaceError::NotAttached.into());
                    }
                    Err(error) => return Err(error.into()),
                };
                organizations.detach(&uow, org.id(), workspace.id).await?;
                Ok::<_, AppError>(NoContent)
            }
        };
        let requests = self.requests.clone();
        let list_requests = move |org: CurrentOrganization| {
            let requests = requests.clone();
            async move {
                let data = requests
                    .pending(org.id())
                    .await?
                    .into_iter()
                    .map(AccessRequestResource::from)
                    .collect();
                Ok::<_, AppError>(Json(DataList::<AccessRequestResource> { data }))
            }
        };
        let requests = self.requests.clone();
        let organizations = self.organizations.clone();
        let request_access =
            move |CurrentPrincipal(me): CurrentPrincipal,
                  uow: Uow,
                  uri: Uri,
                  Path(organization): Path<String>,
                  input: Option<Json<RequestAccessInput>>| {
                let (requests, organizations) = (requests.clone(), organizations.clone());
                async move {
                    let organization = organizations.resolve(&organization).await?;
                    let input = input.map(|Json(input)| input).unwrap_or_default();
                    let created = requests
                        .request(
                            &uow,
                            organization.id,
                            me.user_id,
                            NewAccessRequest {
                                workspace_id: input.workspace_id,
                                message: input.message,
                            },
                        )
                        .await?;
                    let location = format!("{}/{}", uri.path().trim_end_matches('/'), created.id);
                    Ok::<_, AppError>(Created(
                        location,
                        AccessRequestResource::new(&created, None),
                    ))
                }
            };
        let requests = self.requests.clone();
        let members = self.members.clone();
        let approve = move |org: CurrentOrganization,
                            uow: Uow,
                            Path((_, request)): Path<(String, Uuid)>,
                            input: Option<Json<ApproveAccessRequestInput>>| {
            let (requests, members) = (requests.clone(), members.clone());
            async move {
                let input = input.map(|Json(input)| input).unwrap_or_default();
                let found = requests
                    .find(org.id(), request)
                    .await?
                    .ok_or(WorkspaceError::AccessRequestNotFound)?;
                let workspaces = match (&input.workspaces, found.workspace_id) {
                    (Some(workspaces), _) => workspaces.clone(),
                    (None, Some(workspace)) => Selector::only([workspace]),
                    (None, None) => Selector::All,
                };
                check_reach(&org, &workspaces)?;
                let ApprovedRequest { request, member } = requests
                    .approve(
                        &uow,
                        org.id(),
                        request,
                        Some(org.member().user_id),
                        Approval {
                            role: input.role.unwrap_or(OrgRole::Member),
                            workspaces: input.workspaces,
                        },
                    )
                    .await?;
                let reach = members
                    .selector(org.id(), member.user_id)
                    .await?
                    .unwrap_or(workspaces);
                Ok::<_, AppError>(Json(ApprovedAccessRequestResource {
                    request: AccessRequestResource::new(&request, None),
                    member: OrgMemberResource::new(&member, reach, None),
                }))
            }
        };
        let requests = self.requests.clone();
        let decline = move |org: CurrentOrganization,
                            uow: Uow,
                            Path((_, request)): Path<(String, Uuid)>,
                            input: Option<Json<DeclineAccessRequestInput>>| {
            let requests = requests.clone();
            async move {
                let input = input.map(|Json(input)| input).unwrap_or_default();
                let declined = requests
                    .decline(
                        &uow,
                        org.id(),
                        request,
                        Some(org.member().user_id),
                        input.reason,
                    )
                    .await?;
                Ok::<_, AppError>(Json(AccessRequestResource::new(&declined, None)))
            }
        };
        let guard = || InOrganization::from_path(ORGANIZATION_PARAM);
        vec![
            RouteDef::on(Method::GET, "/", index)
                .authenticated()
                .name("organizations.index")
                .api(api("List the caller's organizations", &[401], None, None)),
            RouteDef::on(Method::POST, "/", store)
                .authenticated()
                .name("organizations.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Create an organization owned by the caller",
                        &[400, 401],
                        Some(schema_fn::<CreateOrganizationInput>),
                        Some(schema_fn::<OrganizationResource>),
                    )
                }),
            RouteDef::on(Method::GET, "/{organization}", show)
                .authenticated()
                .guard(guard())
                .name("organizations.show")
                .api(api(
                    "Show an organization with the caller's role",
                    &[401, 403, 404],
                    None,
                    Some(schema_fn::<OrganizationResource>),
                )),
            RouteDef::on(Method::PATCH, "/{organization}", update)
                .permissions(&["organization.update"])
                .guard(guard())
                .name("organizations.update")
                .api(api(
                    "Update an organization",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<UpdateOrganizationInput>),
                    Some(schema_fn::<OrganizationResource>),
                )),
            RouteDef::on(Method::DELETE, "/{organization}", destroy)
                .permissions(&["organization.delete"])
                .guard(guard())
                .name("organizations.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Delete an organization", &[401, 403, 404], None, None)
                }),
            RouteDef::on(Method::POST, "/{organization}/transfer", transfer)
                .permissions(&["organization.transfer"])
                .guard(guard())
                .name("organizations.transfer")
                .api(api(
                    "Transfer an organization to another member",
                    &[400, 401, 403, 404],
                    Some(schema_fn::<TransferOrganizationInput>),
                    Some(schema_fn::<OrganizationResource>),
                )),
            RouteDef::on(Method::POST, "/{organization}/leave", leave)
                .authenticated()
                .guard(guard())
                .name("organizations.leave")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Leave an organization", &[401, 403, 404, 409], None, None)
                }),
            RouteDef::on(Method::GET, "/{organization}/members", list_members)
                .permissions(&["organization.members.read"])
                .guard(guard())
                .name("organizations.members.index")
                .api(api(
                    "List an organization's members",
                    &[401, 403, 404],
                    None,
                    None,
                )),
            RouteDef::on(Method::POST, "/{organization}/members", add_member)
                .permissions(&["organization.members.create"])
                .guard(guard())
                .name("organizations.members.store")
                .api(ApiMeta {
                    success_status: 201,
                    ..api(
                        "Add a user to an organization",
                        &[400, 401, 403, 404, 409],
                        Some(schema_fn::<AddOrgMemberInput>),
                        Some(schema_fn::<OrgMemberResource>),
                    )
                }),
            RouteDef::on(
                Method::PATCH,
                "/{organization}/members/{user}",
                update_member,
            )
            .permissions(&["organization.members.update"])
            .guard(guard())
            .name("organizations.members.update")
            .api(api(
                "Change a member's role or workspaces",
                &[400, 401, 403, 404, 409],
                Some(schema_fn::<UpdateOrgMemberInput>),
                Some(schema_fn::<OrgMemberResource>),
            )),
            RouteDef::on(
                Method::DELETE,
                "/{organization}/members/{user}",
                remove_member,
            )
            .permissions(&["organization.members.delete"])
            .guard(guard())
            .name("organizations.members.destroy")
            .api(ApiMeta {
                success_status: 204,
                ..api("Remove a member", &[401, 403, 404, 409], None, None)
            }),
            RouteDef::on(Method::GET, "/{organization}/workspaces", list_workspaces)
                .permissions(&["organization.read"])
                .guard(guard())
                .name("organizations.workspaces.index")
                .api(api(
                    "List the organization's workspaces the caller reaches",
                    &[401, 403, 404],
                    None,
                    None,
                )),
            RouteDef::on(Method::POST, "/{organization}/workspaces", attach)
                .permissions(&["organization.attach"])
                .guard(guard())
                .name("organizations.workspaces.attach")
                .api(api(
                    "Attach a workspace the caller owns",
                    &[400, 401, 403, 404, 409],
                    Some(schema_fn::<AttachWorkspaceInput>),
                    Some(schema_fn::<WorkspaceResource>),
                )),
            RouteDef::on(
                Method::DELETE,
                "/{organization}/workspaces/{workspace}",
                detach,
            )
            .permissions(&["organization.detach"])
            .guard(guard())
            .name("organizations.workspaces.detach")
            .api(ApiMeta {
                success_status: 204,
                ..api("Detach a workspace", &[401, 403, 404], None, None)
            }),
            RouteDef::on(
                Method::GET,
                "/{organization}/access-requests",
                list_requests,
            )
            .permissions(&["organization.access_requests.read"])
            .guard(guard())
            .name("organizations.access_requests.index")
            .api(api(
                "List pending access requests",
                &[401, 403, 404],
                None,
                None,
            )),
            RouteDef::on(
                Method::POST,
                "/{organization}/access-requests",
                request_access,
            )
            .authenticated()
            .throttle(10, std::time::Duration::from_secs(60))
            .name("organizations.access_requests.store")
            .api(ApiMeta {
                success_status: 201,
                ..api(
                    "Ask to join an organization",
                    &[400, 401, 404, 409, 429],
                    Some(schema_fn::<RequestAccessInput>),
                    Some(schema_fn::<AccessRequestResource>),
                )
            }),
            RouteDef::on(
                Method::POST,
                "/{organization}/access-requests/{request}/approve",
                approve,
            )
            .permissions(&["organization.access_requests.update"])
            .guard(guard())
            .name("organizations.access_requests.approve")
            .api(api(
                "Approve an access request",
                &[400, 401, 403, 404, 409],
                Some(schema_fn::<ApproveAccessRequestInput>),
                Some(schema_fn::<ApprovedAccessRequestResource>),
            )),
            RouteDef::on(
                Method::POST,
                "/{organization}/access-requests/{request}/decline",
                decline,
            )
            .permissions(&["organization.access_requests.update"])
            .guard(guard())
            .name("organizations.access_requests.decline")
            .api(api(
                "Decline an access request",
                &[400, 401, 403, 404, 409],
                Some(schema_fn::<DeclineAccessRequestInput>),
                Some(schema_fn::<AccessRequestResource>),
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
        tags: vec!["Organizations".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}
