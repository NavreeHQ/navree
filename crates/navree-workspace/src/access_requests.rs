//! [`AccessRequests`]: users ask to join an organization; its admins
//! approve or decline.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use navree_db::Uow;
use navree_orm::{Model as _, Record};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::Core;
use crate::directory::UserSummary;
use crate::entity::{AccessRequest, OrgMember};
use crate::events::{AccessRequestApproved, AccessRequestDeclined, AccessRequested};
use crate::grants::attached;
use crate::org_members::OrgMembers;
use crate::store;
use crate::{OrgRole, Selector, WorkspaceError, WorkspaceSelector};

/// The longest access request message or decline reason, in characters.
pub const MAX_MESSAGE_LEN: usize = 1000;

/// Where an access request stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessRequestStatus {
    /// Waiting for an admin.
    Pending,
    /// Approved: the user is a member.
    Approved,
    /// Declined.
    Declined,
}

impl AccessRequestStatus {
    /// `pending`, `approved` or `declined`.
    pub fn as_str(self) -> &'static str {
        match self {
            AccessRequestStatus::Pending => "pending",
            AccessRequestStatus::Approved => "approved",
            AccessRequestStatus::Declined => "declined",
        }
    }

    /// The status stored as `status` (anything unknown reads as
    /// `declined`, so it can't be acted on).
    pub fn parse(status: &str) -> AccessRequestStatus {
        match status {
            "pending" => AccessRequestStatus::Pending,
            "approved" => AccessRequestStatus::Approved,
            _ => AccessRequestStatus::Declined,
        }
    }
}

impl fmt::Display for AccessRequestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The input of [`AccessRequests::request`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewAccessRequest {
    /// A workspace of the organization to ask for (else the organization
    /// as a whole).
    pub workspace_id: Option<Uuid>,
    /// A message to the organization's admins (up to 1000 characters).
    pub message: Option<String>,
}

/// What an approval gives ([`AccessRequests::approve`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    /// The organization role (default `member`; never `owner`).
    pub role: OrgRole,
    /// The workspaces reached; by default the workspace the request named,
    /// or all of them.
    pub workspaces: Option<WorkspaceSelector>,
}

impl Default for Approval {
    fn default() -> Self {
        Self {
            role: OrgRole::Member,
            workspaces: None,
        }
    }
}

/// An access request with the requesting user's name and email, when the
/// [`UserDirectory`](crate::UserDirectory) knows them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AccessRequestInfo {
    /// The request.
    #[serde(flatten)]
    pub request: AccessRequest,
    /// The requesting user, if the directory knows them.
    pub user: Option<UserSummary>,
}

/// What approving gives: the decided request and the new membership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedRequest {
    /// The request, now `approved`.
    pub request: AccessRequest,
    /// The new organization membership.
    pub member: OrgMember,
}

/// Users ask to join an organization (or one of its workspaces); its
/// admins approve, which makes them members, or decline. Provided and
/// exported by the workspace module; its methods answer
/// [`WorkspaceError::FeatureDisabled`] unless `workspace.organizations.enabled`
/// is on.
///
/// A user has at most one pending request per organization. The module
/// emits [`AccessRequested`] once a request commits; listen to it to tell
/// the admins.
pub struct AccessRequests {
    core: Arc<Core>,
    members: Arc<OrgMembers>,
}

impl AccessRequests {
    pub(crate) fn new(core: Arc<Core>, members: Arc<OrgMembers>) -> Self {
        Self { core, members }
    }

    /// `user` asks to join organization `org`. A 409 when they are a member
    /// or have a pending request already; a 400 on `workspace_id` for a
    /// workspace outside the organization, or on `message` when it is too
    /// long.
    pub async fn request(
        &self,
        uow: &Uow,
        org: Uuid,
        user: Uuid,
        input: NewAccessRequest,
    ) -> Result<AccessRequest, WorkspaceError> {
        self.core.require_organizations()?;
        let message = check_text("message", input.message.as_deref())?;
        self.core.organization_in(uow, org).await?;
        if let Some(workspace) = input.workspace_id
            && !attached(uow, org).await?.iter().any(|w| w.id == workspace)
        {
            return Err(WorkspaceError::invalid(
                "workspace_id",
                "workspace_id must be a workspace of the organization",
            ));
        }
        if self.core.org_member_in(uow, org, user).await?.is_some() {
            return Err(WorkspaceError::AlreadyOrganizationMember);
        }
        let pending = AccessRequest::query()
            .where_(AccessRequest::ORGANIZATION_ID.eq(org))
            .where_(AccessRequest::USER_ID.eq(user))
            .where_(AccessRequest::STATUS.eq(AccessRequestStatus::Pending.as_str()))
            .exists(uow)
            .await?;
        if pending {
            return Err(WorkspaceError::AccessRequestPending);
        }
        let now = self.core.now();
        let created = store::insert(
            uow,
            AccessRequest {
                id: Uuid::now_v7(),
                organization_id: org,
                workspace_id: input.workspace_id,
                user_id: user,
                message,
                status: AccessRequestStatus::Pending.as_str().to_owned(),
                role: None,
                decided_by: None,
                decided_at: None,
                reason: None,
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        self.core
            .emit_after_commit(
                uow,
                AccessRequested {
                    organization_id: org,
                    request_id: created.id,
                    user_id: user,
                    workspace_id: created.workspace_id,
                },
            )
            .await;
        Ok(created)
    }

    /// The pending requests of organization `org`, oldest first, with their
    /// users.
    pub async fn pending(&self, org: Uuid) -> Result<Vec<AccessRequestInfo>, WorkspaceError> {
        self.core.require_organizations()?;
        let requests = AccessRequest::query()
            .where_(AccessRequest::ORGANIZATION_ID.eq(org))
            .where_(AccessRequest::STATUS.eq(AccessRequestStatus::Pending.as_str()))
            .order_by(AccessRequest::CREATED_AT)
            .order_by(AccessRequest::ID)
            .get(&*self.core.db)
            .await?
            .into_models();
        let ids: Vec<Uuid> = requests.iter().map(|r| r.user_id).collect();
        let users: HashMap<Uuid, UserSummary> = self
            .core
            .directory
            .users(&ids)
            .await
            .map_err(WorkspaceError::Other)?
            .into_iter()
            .map(|user| (user.id, user))
            .collect();
        Ok(requests
            .into_iter()
            .map(|request| AccessRequestInfo {
                user: users.get(&request.user_id).cloned(),
                request,
            })
            .collect())
    }

    /// Access request `id` of organization `org`.
    pub async fn find(&self, org: Uuid, id: Uuid) -> Result<Option<AccessRequest>, WorkspaceError> {
        self.core.require_organizations()?;
        Ok(AccessRequest::query()
            .where_key(id)
            .where_(AccessRequest::ORGANIZATION_ID.eq(org))
            .first(&*self.core.db)
            .await?
            .map(Record::into_inner))
    }

    /// Every request `user` made, newest first.
    pub async fn for_user(&self, user: Uuid) -> Result<Vec<AccessRequest>, WorkspaceError> {
        self.core.require_organizations()?;
        Ok(AccessRequest::query()
            .where_(AccessRequest::USER_ID.eq(user))
            .order_by_desc(AccessRequest::CREATED_AT)
            .order_by_desc(AccessRequest::ID)
            .get(&*self.core.db)
            .await?
            .into_models())
    }

    /// Approves pending request `id` of organization `org`: the user joins
    /// with `approval`'s role and workspaces. A 404 for an unknown request,
    /// 409 once it is decided or when the user is a member already.
    pub async fn approve(
        &self,
        uow: &Uow,
        org: Uuid,
        id: Uuid,
        decided_by: Option<Uuid>,
        approval: Approval,
    ) -> Result<ApprovedRequest, WorkspaceError> {
        self.core.require_organizations()?;
        let found = self.pending_in(uow, org, id).await?;
        let workspaces = match (approval.workspaces, found.workspace_id) {
            (Some(workspaces), _) => workspaces,
            (None, Some(workspace)) => {
                if !attached(uow, org).await?.iter().any(|w| w.id == workspace) {
                    return Err(WorkspaceError::invalid(
                        "workspaces",
                        "the requested workspace left the organization: name the workspaces to grant",
                    ));
                }
                Selector::only([workspace])
            }
            (None, None) => Selector::All,
        };
        let user = found.user_id;
        let member = self
            .members
            .add(uow, org, user, approval.role.clone(), workspaces)
            .await?;
        let now = self.core.now();
        let request = store::update(uow, found, |row| {
            row.status = AccessRequestStatus::Approved.as_str().to_owned();
            row.role = Some(approval.role.as_str().to_owned());
            row.decided_by = decided_by;
            row.decided_at = Some(now);
            row.updated_at = now;
        })
        .await?;
        self.core
            .emit_after_commit(
                uow,
                AccessRequestApproved {
                    organization_id: org,
                    request_id: id,
                    user_id: user,
                    role: approval.role.as_str().to_owned(),
                    decided_by,
                },
            )
            .await;
        Ok(ApprovedRequest { request, member })
    }

    /// Declines pending request `id` of organization `org`, with an
    /// optional `reason` (up to 1000 characters). A 404 for an unknown
    /// request, 409 once it is decided.
    pub async fn decline(
        &self,
        uow: &Uow,
        org: Uuid,
        id: Uuid,
        decided_by: Option<Uuid>,
        reason: Option<String>,
    ) -> Result<AccessRequest, WorkspaceError> {
        self.core.require_organizations()?;
        let reason = check_text("reason", reason.as_deref())?;
        let found = self.pending_in(uow, org, id).await?;
        let user = found.user_id;
        let now = self.core.now();
        let request = store::update(uow, found, |row| {
            row.status = AccessRequestStatus::Declined.as_str().to_owned();
            row.reason = reason;
            row.decided_by = decided_by;
            row.decided_at = Some(now);
            row.updated_at = now;
        })
        .await?;
        self.core
            .emit_after_commit(
                uow,
                AccessRequestDeclined {
                    organization_id: org,
                    request_id: id,
                    user_id: user,
                    decided_by,
                },
            )
            .await;
        Ok(request)
    }

    /// The pending request `id` of `org`, read in the unit of work.
    async fn pending_in(
        &self,
        uow: &Uow,
        org: Uuid,
        id: Uuid,
    ) -> Result<AccessRequest, WorkspaceError> {
        let found = AccessRequest::query()
            .where_key(id)
            .where_(AccessRequest::ORGANIZATION_ID.eq(org))
            .first(uow)
            .await?
            .ok_or(WorkspaceError::AccessRequestNotFound)?
            .into_inner();
        if found.is_pending() {
            Ok(found)
        } else {
            Err(WorkspaceError::AccessRequestDecided)
        }
    }
}

impl fmt::Debug for AccessRequests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessRequests").finish_non_exhaustive()
    }
}

/// A trimmed message (`None` when empty), at most [`MAX_MESSAGE_LEN`]
/// characters.
fn check_text(field: &'static str, text: Option<&str>) -> Result<Option<String>, WorkspaceError> {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > MAX_MESSAGE_LEN {
        return Err(WorkspaceError::invalid(
            field,
            format!("{field} must be shorter than or equal to {MAX_MESSAGE_LEN} characters"),
        ));
    }
    Ok(Some(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_parse_and_print() {
        for status in [
            AccessRequestStatus::Pending,
            AccessRequestStatus::Approved,
            AccessRequestStatus::Declined,
        ] {
            assert_eq!(AccessRequestStatus::parse(status.as_str()), status);
        }
        assert_eq!(
            AccessRequestStatus::parse("weird"),
            AccessRequestStatus::Declined
        );
        assert_eq!(AccessRequestStatus::Pending.to_string(), "pending");
        assert_eq!(
            serde_json::to_value(AccessRequestStatus::Approved).unwrap(),
            serde_json::json!("approved")
        );
    }

    #[test]
    fn messages_are_trimmed_and_limited() {
        assert_eq!(
            check_text("message", Some("  hi ")).unwrap().as_deref(),
            Some("hi")
        );
        assert_eq!(check_text("message", Some("   ")).unwrap(), None);
        assert!(check_text("message", Some(&"x".repeat(1001))).is_err());
    }
}
