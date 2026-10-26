//! The workspace module's events. They are emitted through navree-events'
//! `EventBus` once the unit of work that made the change commits (never
//! for a rolled-back change), when `EventsModule` is in the app.

use chrono::{DateTime, Utc};
use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
