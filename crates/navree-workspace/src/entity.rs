//! The workspace module's tables as SeaORM entities.
//!
//! - `workspaces(id, name, slug UNIQUE, owner_id, timezone, country,
//!   created_at, updated_at)`
//! - `workspace_members(id, workspace_id → workspaces ON DELETE CASCADE,
//!   user_id, role, created_at, updated_at)`, one row per (workspace, user)
//! - `workspace_invitations(id, workspace_id → workspaces ON DELETE
//!   CASCADE, email, role, token_hash UNIQUE, invited_by, expires_at,
//!   accepted_at, accepted_by, created_at)`
//!
//! User ids are not foreign keys, so the module works with any user store
//! (navree-auth's `users` table or the app's own). Invitation tokens are
//! stored as SHA-256 digests; the plain token exists only in the mail.
//! Change the tables through the [`Workspaces`](crate::Workspaces),
//! [`Members`](crate::Members) and [`Invitations`](crate::Invitations)
//! services, which keep the invariants, emit the events and record the
//! changes; read them freely.

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
        /// The workspace's IANA time zone (`Asia/Kolkata`), stored
        /// normalized; `None` to follow the country or the app's default.
        pub timezone: Option<String>,
        /// The workspace's ISO 3166-1 alpha-2 country (`IN`), upper-case.
        pub country: Option<String>,
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

/// A workspace (a row of `workspaces`).
pub type Workspace = workspace::Model;
/// A membership (a row of `workspace_members`).
pub type Member = member::Model;
/// An invitation (a row of `workspace_invitations`).
pub type Invitation = invitation::Model;

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
