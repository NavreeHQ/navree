//! The workspace module's tables, as navree-orm models.
//!
//! Each table's `Model` is a navree-orm model: rows are read with the model
//! API and its builders (`Workspace::query()`, typed columns such as
//! `Member::USER_ID`). Seven of them are navree-access subjects
//! (`#[model(subject)]`): `Workspace`, `WorkspaceMember`,
//! `WorkspaceInvitation`, `Organization`, `OrganizationMember`,
//! `AccessRequest` and `WorkspacePartition`, registered by the module, so
//! `can(.., from = "id")` routes load them through the ORM in the
//! request's unit of work.
//!
//! - `workspaces(id, name, slug UNIQUE, owner_id, timezone, country, kind,
//!   created_at, updated_at)`, `kind` being `live` or `demo`
//! - `workspace_members(id, workspace_id → workspaces ON DELETE CASCADE,
//!   user_id, role, created_at, updated_at)`, one row per (workspace, user)
//! - `workspace_invitations(id, workspace_id → workspaces ON DELETE
//!   CASCADE, email, role, token_hash UNIQUE, invited_by, expires_at,
//!   accepted_at, accepted_by, created_at)`
//!
//! Organizations (`workspace.organizations.enabled`) and partitions
//! (`workspace.partitions.enabled`) add:
//!
//! - `workspaces.organization_id` (nullable, indexed): the organization a
//!   workspace is attached to;
//! - `organizations(id, name, slug UNIQUE, owner_id, country, timezone,
//!   created_at, updated_at)`;
//! - `organization_members(id, organization_id → organizations ON DELETE
//!   CASCADE, user_id, role, scope, created_at, updated_at)`, one row per
//!   (organization, user); `scope` is `all` or `only`;
//! - `organization_grants(id, organization_id, member_id →
//!   organization_members ON DELETE CASCADE, workspace_id → workspaces ON
//!   DELETE CASCADE, created_at)`: the workspaces of an `only` member;
//! - `access_requests(id, organization_id → organizations ON DELETE
//!   CASCADE, workspace_id, user_id, message, status, role, decided_by,
//!   decided_at, reason, created_at, updated_at)`;
//! - `workspace_members.partition_scope` (`all` or `only`, default `all`);
//! - `workspace_partitions(id, workspace_id → workspaces ON DELETE CASCADE,
//!   name, slug, created_at, updated_at)`, slugs unique per workspace;
//! - `partition_grants(id, workspace_id, member_id → workspace_members ON
//!   DELETE CASCADE, partition_id → workspace_partitions ON DELETE CASCADE,
//!   created_at)`: the partitions of an `only` member.
//!
//! User ids are not foreign keys, so the module works with any user store
//! (navree-auth's `users` table or the app's own). Invitation tokens are
//! stored as SHA-256 digests; the plain token exists only in the mail.
//! Change the tables through the [`Workspaces`](crate::Workspaces),
//! [`Members`](crate::Members) and [`Invitations`](crate::Invitations)
//! services, which keep the invariants, emit the events and record the
//! changes; read them freely. The organization and partition tables exist
//! whether or not the features are on; they stay empty while they are off.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What a workspace holds: a team's own data, or a demo seeded from
/// fixtures by the app's [`DemoSeeder`](crate::DemoSeeder)s.
///
/// Stored in `workspaces.kind` as `live` or `demo`; the API returns it as
/// `kind`, so a front end can offer a Live/Demo switch.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    /// A team's own data (every workspace [`Workspaces::create`](crate::Workspaces::create) makes).
    #[default]
    Live,
    /// A demo workspace ([`DemoWorkspaces::create`](crate::DemoWorkspaces::create)):
    /// read-only or reset every night (`workspace.demo.mode`).
    Demo,
}

impl WorkspaceKind {
    /// `live` or `demo`.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceKind::Live => "live",
            WorkspaceKind::Demo => "demo",
        }
    }

    /// The kind stored as `value`: `demo`, else live.
    pub fn parse(value: &str) -> WorkspaceKind {
        if value == "demo" {
            WorkspaceKind::Demo
        } else {
            WorkspaceKind::Live
        }
    }
}

impl fmt::Display for WorkspaceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `workspaces`: the tenants.
pub mod workspace {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;
    use serde::Serialize;

    use super::WorkspaceKind;

    /// A workspace: the access subject `Workspace` (permissions
    /// `workspace.<action>`).
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "workspaces",
        subject = "Workspace",
        permission_prefix = "workspace"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The display name.
        pub name: String,
        /// The unique, URL-safe handle (`acme`), used by the resolvers.
        pub slug: String,
        /// The owner: always a member with the `owner` role.
        pub owner_id: Uuid,
        /// The organization the workspace is attached to, if any.
        pub organization_id: Option<Uuid>,
        /// The workspace's IANA time zone (`Asia/Kolkata`), stored
        /// normalized; `None` to follow the country or the app's default.
        pub timezone: Option<String>,
        /// The workspace's ISO 3166-1 alpha-2 country (`IN`), upper-case.
        pub country: Option<String>,
        /// `live` (a team's own data) or `demo` (seeded from fixtures; see
        /// [`DemoWorkspaces`](crate::DemoWorkspaces)).
        pub kind: String,
        /// When the workspace was created.
        pub created_at: DateTime<Utc>,
        /// When the workspace last changed.
        pub updated_at: DateTime<Utc>,
    }

    impl Model {
        /// The kind: [`WorkspaceKind::Live`] or [`WorkspaceKind::Demo`].
        pub fn kind(&self) -> WorkspaceKind {
            WorkspaceKind::parse(&self.kind)
        }

        /// Whether this is a demo workspace.
        pub fn is_demo(&self) -> bool {
            self.kind() == WorkspaceKind::Demo
        }
    }

}

/// `workspace_members`: who belongs to a workspace, with which role.
pub mod member {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;
    use serde::Serialize;

    use crate::WorkspaceRole;

    /// A membership: the access subject `WorkspaceMember` (permissions
    /// `workspace.members.<action>`).
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "workspace_members",
        subject = "WorkspaceMember",
        permission_prefix = "workspace.members"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace.
        pub workspace_id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The role name (`owner`, `admin`, `member`, `viewer` or an
        /// app-defined role).
        pub role: String,
        /// Which partitions the member sees: `all`, or `only` the ones
        /// listed in `partition_grants`.
        pub partition_scope: String,
        /// When the user joined.
        pub created_at: DateTime<Utc>,
        /// When the membership last changed.
        pub updated_at: DateTime<Utc>,
    }

    impl Model {
        /// The role.
        pub fn role(&self) -> WorkspaceRole {
            WorkspaceRole::parse(&self.role)
        }

        /// Whether the member sees only the partitions granted to them.
        pub fn partitions_limited(&self) -> bool {
            self.partition_scope == crate::org_role::SCOPE_ONLY
        }
    }

}

/// `workspace_invitations`: pending and accepted invitations.
pub mod invitation {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    use crate::WorkspaceRole;

    /// An invitation: the access subject `WorkspaceInvitation` (permissions
    /// `workspace.invitations.<action>`). The token digest is never
    /// serialized.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "workspace_invitations",
        subject = "WorkspaceInvitation",
        permission_prefix = "workspace.invitations"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace.
        pub workspace_id: Uuid,
        /// The invited address, trimmed and lower-cased.
        pub email: String,
        /// The role the invitee gets.
        pub role: String,
        /// SHA-256 of the token (lower-case hex). Never serialized.
        #[serde(skip_serializing)]
        pub token_hash: String,
        /// Who sent it, if a user did.
        pub invited_by: Option<Uuid>,
        /// When it stops being accepted.
        pub expires_at: DateTime<Utc>,
        /// When it was accepted, if it was.
        pub accepted_at: Option<DateTime<Utc>>,
        /// The user who accepted it.
        pub accepted_by: Option<Uuid>,
        /// When it was sent.
        pub created_at: DateTime<Utc>,
    }

    impl Model {
        /// The role.
        pub fn role(&self) -> WorkspaceRole {
            WorkspaceRole::parse(&self.role)
        }

        /// Whether it is still open: not accepted and not expired at `now`.
        pub fn is_pending(&self, now: DateTime<Utc>) -> bool {
            self.accepted_at.is_none() && self.expires_at > now
        }
    }
}

/// `organizations`: the level above workspaces.
pub mod organization {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;
    use serde::Serialize;

    /// An organization: the access subject `Organization` (permissions
    /// `organization.<action>`).
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "organizations",
        subject = "Organization",
        permission_prefix = "organization"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The display name.
        pub name: String,
        /// The unique, URL-safe handle (`acme-group`), used by the
        /// resolvers.
        pub slug: String,
        /// The owner: always a member with the `owner` role.
        pub owner_id: Uuid,
        /// An ISO 3166-1 alpha-2 country code (`GB`), if set.
        pub country: Option<String>,
        /// An IANA time zone name (`Europe/London`), if set.
        pub timezone: Option<String>,
        /// When the organization was created.
        pub created_at: DateTime<Utc>,
        /// When the organization last changed.
        pub updated_at: DateTime<Utc>,
    }

}

/// `organization_members`: who belongs to an organization, with which role
/// and in which of its workspaces.
pub mod organization_member {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    use crate::OrgRole;

    /// An organization membership: the access subject `OrganizationMember`
    /// (permissions `organization.members.<action>`).
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "organization_members",
        subject = "OrganizationMember",
        permission_prefix = "organization.members"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The organization.
        pub organization_id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The role name (`owner`, `admin`, `member`, `viewer` or an
        /// app-defined organization role).
        pub role: String,
        /// Which workspaces the member reaches: `all` of the organization's,
        /// or `only` the ones listed in `organization_grants`.
        pub scope: String,
        /// When the user joined.
        pub created_at: DateTime<Utc>,
        /// When the membership last changed.
        pub updated_at: DateTime<Utc>,
    }

    impl Model {
        /// The role.
        pub fn role(&self) -> OrgRole {
            OrgRole::parse(&self.role)
        }

        /// Whether the member reaches only the workspaces granted to them.
        pub fn is_limited(&self) -> bool {
            self.scope == crate::org_role::SCOPE_ONLY
        }
    }
}

/// `organization_grants`: the workspaces an organization member limited to
/// `only` some of them reaches.
pub mod organization_grant {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A grant of one workspace to one organization member.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "organization_grants")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The organization.
        pub organization_id: Uuid,
        /// The organization membership (`organization_members.id`).
        pub member_id: Uuid,
        /// The workspace the member reaches.
        pub workspace_id: Uuid,
        /// When the grant was made.
        pub created_at: DateTime<Utc>,
    }
}

/// `access_requests`: users asking to join an organization.
pub mod access_request {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    use crate::AccessRequestStatus;

    /// An access request: the access subject `AccessRequest` (permissions
    /// `organization.access_requests.<action>`).
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "access_requests",
        subject = "AccessRequest",
        permission_prefix = "organization.access_requests"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The organization asked.
        pub organization_id: Uuid,
        /// The workspace of the organization the user asks for, if one.
        pub workspace_id: Option<Uuid>,
        /// Who asks.
        pub user_id: Uuid,
        /// Their message to the organization's admins.
        pub message: Option<String>,
        /// `pending`, `approved` or `declined`.
        pub status: String,
        /// The organization role given on approval.
        pub role: Option<String>,
        /// Who approved or declined it.
        pub decided_by: Option<Uuid>,
        /// When it was approved or declined.
        pub decided_at: Option<DateTime<Utc>>,
        /// Why it was declined, if a reason was given.
        pub reason: Option<String>,
        /// When it was made.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }

    impl Model {
        /// The status.
        pub fn status(&self) -> AccessRequestStatus {
            AccessRequestStatus::parse(&self.status)
        }

        /// Whether it waits for a decision.
        pub fn is_pending(&self) -> bool {
            self.status() == AccessRequestStatus::Pending
        }
    }
}

/// `workspace_partitions`: named slices of a workspace's data.
pub mod partition {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A partition: the access subject `WorkspacePartition` (permissions
    /// `workspace.partitions.<action>`).
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "workspace_partitions",
        subject = "WorkspacePartition",
        permission_prefix = "workspace.partitions"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace.
        pub workspace_id: Uuid,
        /// The display name (`acme.co.uk`).
        pub name: String,
        /// The handle, unique in the workspace (`acme-co-uk`).
        pub slug: String,
        /// When the partition was created.
        pub created_at: DateTime<Utc>,
        /// When the partition last changed.
        pub updated_at: DateTime<Utc>,
    }
}

/// `partition_grants`: the partitions a workspace member limited to `only`
/// some of them sees.
pub mod partition_grant {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A grant of one partition to one workspace member.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "partition_grants")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The workspace.
        pub workspace_id: Uuid,
        /// The workspace membership (`workspace_members.id`).
        pub member_id: Uuid,
        /// The partition the member sees.
        pub partition_id: Uuid,
        /// When the grant was made.
        pub created_at: DateTime<Utc>,
    }
}

/// A workspace (a row of `workspaces`).
pub type Workspace = workspace::Model;

/// The mail scope of a workspace: the workspace and its organization, so
/// `mailer.for_scope(&MailScope::from(&workspace))` uses their transports,
/// suppressions and delivery log.
impl From<&Workspace> for navree_mail::MailScope {
    fn from(workspace: &Workspace) -> Self {
        let scope = navree_mail::MailScope::workspace(workspace.id);
        match workspace.organization_id {
            Some(organization) => scope.with_organization(organization),
            None => scope,
        }
    }
}
/// A membership (a row of `workspace_members`).
pub type Member = member::Model;
/// An invitation (a row of `workspace_invitations`).
pub type Invitation = invitation::Model;
/// An organization (a row of `organizations`).
pub type Organization = organization::Model;
/// An organization membership (a row of `organization_members`).
pub type OrgMember = organization_member::Model;
/// An access request (a row of `access_requests`).
pub type AccessRequest = access_request::Model;
/// A partition (a row of `workspace_partitions`).
pub type Partition = partition::Model;

/// The CASL subject of organizations: `Organization`, permissions
/// `organization.<action>` (`organization.update`, `organization.attach`).
pub const ORGANIZATION_SUBJECT: &str = "Organization";
/// The CASL subject of organization memberships: `OrganizationMember`,
/// permissions `organization.members.<action>`.
pub const ORG_MEMBER_SUBJECT: &str = "OrganizationMember";
/// The CASL subject of access requests: `AccessRequest`, permissions
/// `organization.access_requests.<action>`.
pub const ACCESS_REQUEST_SUBJECT: &str = "AccessRequest";
/// The CASL subject of partitions: `WorkspacePartition`, permissions
/// `workspace.partitions.<action>`.
pub const PARTITION_SUBJECT: &str = "WorkspacePartition";

/// The CASL subject of workspaces: `Workspace`, permissions
/// `workspace.<action>` (`workspace.update`, `workspace.delete`,
/// `workspace.transfer`).
pub const WORKSPACE_SUBJECT: &str = "Workspace";
/// The CASL subject of memberships: `WorkspaceMember`, permissions
/// `workspace.members.<action>`.
pub const MEMBER_SUBJECT: &str = "WorkspaceMember";
/// The CASL subject of invitations: `WorkspaceInvitation`, permissions
/// `workspace.invitations.<action>`.
pub const INVITATION_SUBJECT: &str = "WorkspaceInvitation";

#[cfg(test)]
mod tests {
    use navree_access::SubjectRecords;

    use super::*;

    fn subject<S: SubjectRecords>() -> (&'static str, &'static str) {
        (S::SUBJECT, S::PERMISSION_PREFIX)
    }

    #[test]
    fn the_models_are_the_module_s_subjects() {
        assert_eq!(subject::<Workspace>(), (WORKSPACE_SUBJECT, "workspace"));
        assert_eq!(subject::<Member>(), (MEMBER_SUBJECT, "workspace.members"));
        assert_eq!(
            subject::<Invitation>(),
            (INVITATION_SUBJECT, "workspace.invitations")
        );
        assert_eq!(
            subject::<Organization>(),
            (ORGANIZATION_SUBJECT, "organization")
        );
        assert_eq!(
            subject::<OrgMember>(),
            (ORG_MEMBER_SUBJECT, "organization.members")
        );
        assert_eq!(
            subject::<AccessRequest>(),
            (ACCESS_REQUEST_SUBJECT, "organization.access_requests")
        );
        assert_eq!(
            subject::<Partition>(),
            (PARTITION_SUBJECT, "workspace.partitions")
        );
    }
}
