//! The workspace module's tables as SeaORM entities.
//!
//! - `workspaces(id, name, slug UNIQUE, owner_id, created_at, updated_at)`
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

use navree_access::Subject;

/// `workspaces`: the tenants.
pub mod workspace {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A workspace.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "workspaces")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The display name.
        pub name: String,
        /// The unique, URL-safe handle (`acme`), used by the resolvers.
        #[sea_orm(unique)]
        pub slug: String,
        /// The owner: always a member with the `owner` role.
        pub owner_id: Uuid,
        /// The organization the workspace is attached to, if any.
        pub organization_id: Option<Uuid>,
        /// When the workspace was created.
        pub created_at: DateTimeUtc,
        /// When the workspace last changed.
        pub updated_at: DateTimeUtc,
    }

    /// A workspace has many members and invitations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `workspace_members.workspace_id`.
        #[sea_orm(has_many = "super::member::Entity")]
        Members,
        /// `workspace_invitations.workspace_id`.
        #[sea_orm(has_many = "super::invitation::Entity")]
        Invitations,
    }

    impl Related<super::member::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Members.def()
        }
    }

    impl Related<super::invitation::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Invitations.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `workspace_members`: who belongs to a workspace, with which role.
pub mod member {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    use crate::WorkspaceRole;

    /// A membership.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "workspace_members")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub created_at: DateTimeUtc,
        /// When the membership last changed.
        pub updated_at: DateTimeUtc,
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

    /// A membership belongs to a workspace.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `workspaces.id`.
        #[sea_orm(
            belongs_to = "super::workspace::Entity",
            from = "Column::WorkspaceId",
            to = "super::workspace::Column::Id",
            on_delete = "Cascade"
        )]
        Workspace,
    }

    impl Related<super::workspace::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Workspace.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `workspace_invitations`: pending and accepted invitations.
pub mod invitation {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    use crate::WorkspaceRole;

    /// An invitation. The token digest is never serialized.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "workspace_invitations")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The workspace.
        pub workspace_id: Uuid,
        /// The invited address, trimmed and lower-cased.
        pub email: String,
        /// The role the invitee gets.
        pub role: String,
        /// SHA-256 of the token (lower-case hex). Never serialized.
        #[serde(skip_serializing)]
        #[sea_orm(unique)]
        pub token_hash: String,
        /// Who sent it, if a user did.
        pub invited_by: Option<Uuid>,
        /// When it stops being accepted.
        pub expires_at: DateTimeUtc,
        /// When it was accepted, if it was.
        pub accepted_at: Option<DateTimeUtc>,
        /// The user who accepted it.
        pub accepted_by: Option<Uuid>,
        /// When it was sent.
        pub created_at: DateTimeUtc,
    }

    impl Model {
        /// The role.
        pub fn role(&self) -> WorkspaceRole {
            WorkspaceRole::parse(&self.role)
        }

        /// Whether it is still open: not accepted and not expired at `now`.
        pub fn is_pending(&self, now: DateTimeUtc) -> bool {
            self.accepted_at.is_none() && self.expires_at > now
        }
    }

    /// An invitation belongs to a workspace.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `workspaces.id`.
        #[sea_orm(
            belongs_to = "super::workspace::Entity",
            from = "Column::WorkspaceId",
            to = "super::workspace::Column::Id",
            on_delete = "Cascade"
        )]
        Workspace,
    }

    impl Related<super::workspace::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Workspace.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `organizations`: the level above workspaces.
pub mod organization {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// An organization.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "organizations")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The display name.
        pub name: String,
        /// The unique, URL-safe handle (`acme-group`), used by the
        /// resolvers.
        #[sea_orm(unique)]
        pub slug: String,
        /// The owner: always a member with the `owner` role.
        pub owner_id: Uuid,
        /// An ISO 3166-1 alpha-2 country code (`GB`), if set.
        pub country: Option<String>,
        /// An IANA time zone name (`Europe/London`), if set.
        pub timezone: Option<String>,
        /// When the organization was created.
        pub created_at: DateTimeUtc,
        /// When the organization last changed.
        pub updated_at: DateTimeUtc,
    }

    /// An organization has many members and access requests.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `organization_members.organization_id`.
        #[sea_orm(has_many = "super::organization_member::Entity")]
        Members,
        /// `access_requests.organization_id`.
        #[sea_orm(has_many = "super::access_request::Entity")]
        AccessRequests,
    }

    impl Related<super::organization_member::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Members.def()
        }
    }

    impl Related<super::access_request::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::AccessRequests.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `organization_members`: who belongs to an organization, with which role
/// and in which of its workspaces.
pub mod organization_member {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    use crate::OrgRole;

    /// An organization membership.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "organization_members")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub created_at: DateTimeUtc,
        /// When the membership last changed.
        pub updated_at: DateTimeUtc,
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

    /// A membership belongs to an organization.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `organizations.id`.
        #[sea_orm(
            belongs_to = "super::organization::Entity",
            from = "Column::OrganizationId",
            to = "super::organization::Column::Id",
            on_delete = "Cascade"
        )]
        Organization,
    }

    impl Related<super::organization::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Organization.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `organization_grants`: the workspaces an organization member limited to
/// `only` some of them reaches.
pub mod organization_grant {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A grant of one workspace to one organization member.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "organization_grants")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The organization.
        pub organization_id: Uuid,
        /// The organization membership (`organization_members.id`).
        pub member_id: Uuid,
        /// The workspace the member reaches.
        pub workspace_id: Uuid,
        /// When the grant was made.
        pub created_at: DateTimeUtc,
    }

    /// A grant belongs to a membership.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `organization_members.id`.
        #[sea_orm(
            belongs_to = "super::organization_member::Entity",
            from = "Column::MemberId",
            to = "super::organization_member::Column::Id",
            on_delete = "Cascade"
        )]
        Member,
    }

    impl Related<super::organization_member::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Member.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `access_requests`: users asking to join an organization.
pub mod access_request {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    use crate::AccessRequestStatus;

    /// An access request.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "access_requests")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub decided_at: Option<DateTimeUtc>,
        /// Why it was declined, if a reason was given.
        pub reason: Option<String>,
        /// When it was made.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
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

    /// An access request belongs to an organization.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `organizations.id`.
        #[sea_orm(
            belongs_to = "super::organization::Entity",
            from = "Column::OrganizationId",
            to = "super::organization::Column::Id",
            on_delete = "Cascade"
        )]
        Organization,
    }

    impl Related<super::organization::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Organization.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `workspace_partitions`: named slices of a workspace's data.
pub mod partition {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A partition.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "workspace_partitions")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The workspace.
        pub workspace_id: Uuid,
        /// The display name (`acme.co.uk`).
        pub name: String,
        /// The handle, unique in the workspace (`acme-co-uk`).
        pub slug: String,
        /// When the partition was created.
        pub created_at: DateTimeUtc,
        /// When the partition last changed.
        pub updated_at: DateTimeUtc,
    }

    /// A partition belongs to a workspace.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `workspaces.id`.
        #[sea_orm(
            belongs_to = "super::workspace::Entity",
            from = "Column::WorkspaceId",
            to = "super::workspace::Column::Id",
            on_delete = "Cascade"
        )]
        Workspace,
    }

    impl Related<super::workspace::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Workspace.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `partition_grants`: the partitions a workspace member limited to `only`
/// some of them sees.
pub mod partition_grant {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A grant of one partition to one workspace member.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "partition_grants")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The workspace.
        pub workspace_id: Uuid,
        /// The workspace membership (`workspace_members.id`).
        pub member_id: Uuid,
        /// The partition the member sees.
        pub partition_id: Uuid,
        /// When the grant was made.
        pub created_at: DateTimeUtc,
    }

    /// A grant belongs to a membership and a partition.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `workspace_members.id`.
        #[sea_orm(
            belongs_to = "super::member::Entity",
            from = "Column::MemberId",
            to = "super::member::Column::Id",
            on_delete = "Cascade"
        )]
        Member,
        /// `workspace_partitions.id`.
        #[sea_orm(
            belongs_to = "super::partition::Entity",
            from = "Column::PartitionId",
            to = "super::partition::Column::Id",
            on_delete = "Cascade"
        )]
        Partition,
    }

    impl Related<super::partition::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Partition.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
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

impl Subject for workspace::Entity {
    const SUBJECT: &'static str = WORKSPACE_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "workspace";
}

impl Subject for member::Entity {
    const SUBJECT: &'static str = MEMBER_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "workspace.members";
}

impl Subject for invitation::Entity {
    const SUBJECT: &'static str = INVITATION_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "workspace.invitations";
}

impl Subject for organization::Entity {
    const SUBJECT: &'static str = ORGANIZATION_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "organization";
}

impl Subject for organization_member::Entity {
    const SUBJECT: &'static str = ORG_MEMBER_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "organization.members";
}

impl Subject for access_request::Entity {
    const SUBJECT: &'static str = ACCESS_REQUEST_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "organization.access_requests";
}

impl Subject for partition::Entity {
    const SUBJECT: &'static str = PARTITION_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "workspace.partitions";
}
