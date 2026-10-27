//! The workspace module's events. They are emitted through navree-events'
//! `EventBus` once the unit of work that made the change commits (never
//! for a rolled-back change), when `EventsModule` is in the app.

use chrono::{DateTime, Utc};
use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{PartitionSelector, WorkspaceSelector};

/// A workspace was created (its owner joined it as `owner`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceCreated {
    /// The workspace.
    pub workspace_id: Uuid,
    /// Its name.
    pub name: String,
    /// Its slug.
    pub slug: String,
    /// Its owner.
    pub owner_id: Uuid,
}

impl Event for WorkspaceCreated {
    const NAME: &'static str = "workspace.created";
}

/// A workspace was deleted, with its members and invitations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDeleted {
    /// The workspace.
    pub workspace_id: Uuid,
    /// Its slug.
    pub slug: String,
}

impl Event for WorkspaceDeleted {
    const NAME: &'static str = "workspace.deleted";
}

/// A workspace changed owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipTransferred {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The previous owner (now an `admin`).
    pub from: Uuid,
    /// The new owner.
    pub to: Uuid,
}

impl Event for OwnershipTransferred {
    const NAME: &'static str = "workspace.ownership_transferred";
}

/// A user joined a workspace (added directly, by accepting an invitation,
/// or as the creator).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberAdded {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// Their role.
    pub role: String,
}

impl Event for MemberAdded {
    const NAME: &'static str = "workspace.member_added";
}

/// A member's role changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRoleChanged {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// The old role.
    pub from: String,
    /// The new role.
    pub to: String,
}

impl Event for MemberRoleChanged {
    const NAME: &'static str = "workspace.member_role_changed";
}

/// A user left or was removed from a workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRemoved {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// The role they had.
    pub role: String,
}

impl Event for MemberRemoved {
    const NAME: &'static str = "workspace.member_removed";
}

/// An invitation was sent. The token is not part of the event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationSent {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The invitation.
    pub invitation_id: Uuid,
    /// The invited address.
    pub email: String,
    /// The role offered.
    pub role: String,
    /// Who sent it.
    pub invited_by: Option<Uuid>,
    /// When it expires.
    pub expires_at: DateTime<Utc>,
}

impl Event for InvitationSent {
    const NAME: &'static str = "workspace.invitation_sent";
}

/// An invitation was accepted; the user is now a member.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationAccepted {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The invitation.
    pub invitation_id: Uuid,
    /// The user who accepted it.
    pub user_id: Uuid,
    /// The role they got.
    pub role: String,
}

impl Event for InvitationAccepted {
    const NAME: &'static str = "workspace.invitation_accepted";
}

/// An organization was created (its owner joined it as `owner`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationCreated {
    /// The organization.
    pub organization_id: Uuid,
    /// Its name.
    pub name: String,
    /// Its slug.
    pub slug: String,
    /// Its owner.
    pub owner_id: Uuid,
}

impl Event for OrganizationCreated {
    const NAME: &'static str = "organization.created";
}

/// An organization was deleted with its members and access requests; its
/// workspaces were detached and kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationDeleted {
    /// The organization.
    pub organization_id: Uuid,
    /// Its slug.
    pub slug: String,
    /// The workspaces it had, now standalone.
    pub detached: Vec<Uuid>,
}

impl Event for OrganizationDeleted {
    const NAME: &'static str = "organization.deleted";
}

/// An organization changed owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationOwnershipTransferred {
    /// The organization.
    pub organization_id: Uuid,
    /// The previous owner (now an `admin`).
    pub from: Uuid,
    /// The new owner.
    pub to: Uuid,
}

impl Event for OrganizationOwnershipTransferred {
    const NAME: &'static str = "organization.ownership_transferred";
}

/// A workspace was attached to an organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAttached {
    /// The organization.
    pub organization_id: Uuid,
    /// The workspace.
    pub workspace_id: Uuid,
}

impl Event for WorkspaceAttached {
    const NAME: &'static str = "organization.workspace_attached";
}

/// A workspace was detached from an organization (it stays, standalone).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDetached {
    /// The organization.
    pub organization_id: Uuid,
    /// The workspace.
    pub workspace_id: Uuid,
}

impl Event for WorkspaceDetached {
    const NAME: &'static str = "organization.workspace_detached";
}

/// A user joined an organization (added, granted, through an approved
/// access request, or as the creator).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgMemberAdded {
    /// The organization.
    pub organization_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// Their organization role.
    pub role: String,
    /// The workspaces they reach (`"all"` or a list of ids).
    pub workspaces: WorkspaceSelector,
}

impl Event for OrgMemberAdded {
    const NAME: &'static str = "organization.member_added";
}

/// An organization member's role or workspaces changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgMemberChanged {
    /// The organization.
    pub organization_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// The old role.
    pub from: String,
    /// The new role.
    pub to: String,
    /// The workspaces they reach now.
    pub workspaces: WorkspaceSelector,
}

impl Event for OrgMemberChanged {
    const NAME: &'static str = "organization.member_changed";
}

/// A user left or was removed from an organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgMemberRemoved {
    /// The organization.
    pub organization_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// The role they had.
    pub role: String,
}

impl Event for OrgMemberRemoved {
    const NAME: &'static str = "organization.member_removed";
}

/// A user asked to join an organization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessRequested {
    /// The organization.
    pub organization_id: Uuid,
    /// The access request.
    pub request_id: Uuid,
    /// Who asks.
    pub user_id: Uuid,
    /// The workspace they ask for, if one.
    pub workspace_id: Option<Uuid>,
}

impl Event for AccessRequested {
    const NAME: &'static str = "organization.access_requested";
}

/// An access request was approved; the user is now a member.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessRequestApproved {
    /// The organization.
    pub organization_id: Uuid,
    /// The access request.
    pub request_id: Uuid,
    /// The new member.
    pub user_id: Uuid,
    /// The organization role they got.
    pub role: String,
    /// Who approved it.
    pub decided_by: Option<Uuid>,
}

impl Event for AccessRequestApproved {
    const NAME: &'static str = "organization.access_request_approved";
}

/// An access request was declined.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessRequestDeclined {
    /// The organization.
    pub organization_id: Uuid,
    /// The access request.
    pub request_id: Uuid,
    /// Who asked.
    pub user_id: Uuid,
    /// Who declined it.
    pub decided_by: Option<Uuid>,
}

impl Event for AccessRequestDeclined {
    const NAME: &'static str = "organization.access_request_declined";
}

/// A partition was created in a workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionCreated {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The partition.
    pub partition_id: Uuid,
    /// Its name.
    pub name: String,
    /// Its slug.
    pub slug: String,
}

impl Event for PartitionCreated {
    const NAME: &'static str = "workspace.partition_created";
}

/// A partition was deleted, with its grants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionDeleted {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The partition.
    pub partition_id: Uuid,
    /// Its slug.
    pub slug: String,
}

impl Event for PartitionDeleted {
    const NAME: &'static str = "workspace.partition_deleted";
}

/// The partitions a workspace member sees changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionAccessChanged {
    /// The workspace.
    pub workspace_id: Uuid,
    /// The member.
    pub user_id: Uuid,
    /// The partitions they see now (`"all"` or a list of ids).
    pub partitions: PartitionSelector,
}

impl Event for PartitionAccessChanged {
    const NAME: &'static str = "workspace.partition_access_changed";
}

/// A demo workspace was seeded: when it was created
/// ([`DemoWorkspaces::create`](crate::DemoWorkspaces::create), `reset:
/// false`) or reset (`workspace:demo-reset`, `reset: true`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoWorkspaceSeeded {
    /// The workspace.
    pub workspace_id: Uuid,
    /// Its slug.
    pub slug: String,
    /// The demo seeders that ran, in order.
    pub seeders: Vec<String>,
    /// Whether the workspace was cleared first.
    pub reset: bool,
}

impl Event for DemoWorkspaceSeeded {
    const NAME: &'static str = "workspace.demo_seeded";
}
