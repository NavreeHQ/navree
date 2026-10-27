//! # navree-workspace
//!
//! `WorkspaceModule`: multi-tenancy at the workspace level. A workspace (a
//! team, an organization, a tenant) has members with roles, invites people
//! by email, and keeps its data apart from every other workspace's.
//!
//! - **Workspaces, members, invitations.** [`Workspaces`] creates, renames,
//!   lists, transfers and deletes workspaces; [`Members`] adds members,
//!   changes their roles and removes them (the owner stays until ownership
//!   is transferred); [`Invitations`] invites by email (the token is mailed
//!   and stored as a SHA-256 digest), accepts by token, revokes and
//!   expires. Every change goes through `navree_db::Repo` (audited) and
//!   emits an [`events`] event once its unit of work commits.
//! - **Roles.** [`WorkspaceRole`]: `owner`, `admin`, `member`, `viewer` and
//!   roles the app defines under `workspace.roles`, each with permission
//!   strings that navree-access evaluates.
//! - **Resolution.** [`CurrentWorkspace`] (an extractor) and [`InWorkspace`]
//!   (a route guard) find the request's workspace from a header
//!   (`X-Workspace`), a subdomain, a path prefix (`/w/acme/...`) or a JWT
//!   claim, in the configured order, and check that the signed-in user is a
//!   member: 400 when none is named, 404 for an unknown workspace, 403 for a
//!   non-member. Behind the guard, route `roles`, `permissions` and `can`
//!   checks use the member's role in that workspace.
//! - **Isolation** ([`Isolation`]). `Row`: a `workspace_id` column, scoped
//!   by [`WorkspaceRepo`] and, on PostgreSQL, enforced by row-level security
//!   ([`RlsPolicy`]) through `navree.workspace`, set for every workspace
//!   request's transaction. `Schema`: one PostgreSQL schema per workspace,
//!   selected with `search_path`. `Database`: one database per workspace
//!   ([`WorkspaceDatabases`]), used through [`WorkspaceUow`].
//! - **Organizations** (`workspace.organizations.enabled`). An optional
//!   level above workspaces: [`Organizations`] creates them and attaches
//!   workspaces; [`OrgMembers`] gives members an [`OrgRole`] that maps to a
//!   workspace role in all or some of the organization's workspaces
//!   ([`WorkspaceSelector`]), so [`CurrentWorkspace`] admits a member or an
//!   organization grant; [`AccessRequests`] lets users ask to join.
//!   [`CurrentOrganization`] and [`InOrganization`] resolve the request's
//!   organization (`X-Organization`, `/o/acme/...`, a subdomain or a claim)
//!   and [`OrgRepo`] reads across the workspaces the member reaches (on
//!   PostgreSQL, through `navree.organization`).
//! - **Partitions** (`workspace.partitions.enabled`). An optional level
//!   inside a workspace: [`Partitions`] creates named slices of its data
//!   (a `partition_id` column, [`PartitionScoped`]) and limits members to
//!   all or some of them; [`CurrentPartition`] (`X-Partition`) and
//!   [`PartitionScope`] keep a request to the partitions it may touch, with
//!   [`PartitionRepo`] and, on PostgreSQL, `navree.partition`.
//! - **Routes.** `/api/workspaces...` and `POST /api/invitations/accept`
//!   ([`WorkspaceController`], [`InvitationController`]), plus
//!   `/api/organizations...` ([`OrganizationController`]) and
//!   `/api/workspaces/{workspace}/partitions` when those are on, unless
//!   `workspace.routes` is false.
//! - **Testing** (`testing`, feature `testing`). `TestApp::isolation_suite::<M>()`
//!   with `testing::IsolationSuiteExt::factory` sends every route of module
//!   `M` from one workspace against another's records: a 404, not listed,
//!   never created there.
//! - **Conformity.** `navree check` runs `workspace.entities` (every entity
//!   classified with [`ModuleBuilderWorkspaceExt::workspace_entity`] or
//!   [`global_entity`](ModuleBuilderWorkspaceExt::global_entity)),
//!   `workspace.repo` (workspace entities reached only through
//!   [`WorkspaceRepo`]) and `workspace.tables` (an indexed workspace
//!   column, row-level security on PostgreSQL).
//!
//! ```no_run
//! use navree_access::{AccessModule, AccessOptions};
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_events::EventsModule;
//! use navree_http::HttpModule;
//! use navree_kernel::Navree;
//! use navree_workspace::WorkspaceModule;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(HttpModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(EventsModule::for_root())
//!     .module(AccessModule::for_root(AccessOptions::default()))
//!     // AuthModule (navree-auth) authenticates; MailModule mails invitations.
//!     .module(WorkspaceModule::from_config()) // section `workspace`
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod entity;
pub mod events;
#[cfg(feature = "testing")]
pub mod testing;

mod access;
mod access_requests;
mod commands;
mod config;
mod conformity;
mod controller;
mod core;
mod current;
mod directory;
mod error;
mod grants;
mod invitations;
mod isolation;
mod members;
mod migration;
mod module;
mod org_controller;
mod org_current;
mod org_members;
mod org_role;
mod organizations;
mod partitions;
mod resolve;
mod role;
mod scoped;
mod tenant;
mod workspaces;

pub use access_requests::{
    AccessRequestInfo, AccessRequestStatus, AccessRequests, Approval, ApprovedRequest,
    MAX_MESSAGE_LEN, NewAccessRequest,
};
pub use config::{
    InvitationOptions, Isolation, OrgRoleOptions, OrganizationOptions, PartitionOptions,
    ResolverKind, RoleOptions, WorkspaceConfig,
};
pub use controller::{
    AcceptInvitationInput, AcceptedInvitationResource, AddMemberInput, ChangeRoleInput,
    CreatePartitionInput, CreateWorkspaceInput, InvitationController, InvitationResource,
    InviteInput, MemberResource, PartitionAccessInput, PartitionAccessResource, PartitionResource,
    RenamePartitionInput, RenameWorkspaceInput, TransferWorkspaceInput, WorkspaceController,
    WorkspaceResource,
};
pub use current::{CurrentWorkspace, InWorkspace, PinnedWorkspace, WorkspaceUow};
pub use directory::{TableUserDirectory, UserDirectory, UserSummary};
pub use entity::{
    ACCESS_REQUEST_SUBJECT, AccessRequest, INVITATION_SUBJECT, Invitation, MEMBER_SUBJECT, Member,
    ORG_MEMBER_SUBJECT, ORGANIZATION_SUBJECT, OrgMember, Organization, PARTITION_SUBJECT,
    Partition, WORKSPACE_SUBJECT, Workspace,
};
pub use error::WorkspaceError;
pub use invitations::{AcceptedInvitation, Invitations, SentInvitation, TOKEN_LEN};
pub use isolation::{RlsPolicy, WorkspaceRepo, WorkspaceScoped, rls_policy_sql};
pub use members::{MemberInfo, Members};
pub use migration::WORKSPACE_MIGRATIONS;
pub use module::{ModuleBuilderWorkspaceExt, WorkspaceModule};
pub use org_controller::{
    AccessRequestResource, AddOrgMemberInput, ApproveAccessRequestInput,
    ApprovedAccessRequestResource, AttachWorkspaceInput, CreateOrganizationInput,
    DeclineAccessRequestInput, OrgMemberResource, OrganizationController, OrganizationResource,
    RequestAccessInput, TransferOrganizationInput, UpdateOrgMemberInput, UpdateOrganizationInput,
};
pub use org_current::{CurrentOrganization, CurrentPartition, InOrganization, PartitionScope};
pub use org_members::{OrgMemberInfo, OrgMembers};
pub use org_role::{OrgRole, OrgRoleTable, PartitionSelector, Selector, WorkspaceSelector};
pub use organizations::{NewOrganization, OrgMembership, OrganizationChanges, Organizations};
pub use partitions::{NewPartition, Partitions};
pub use role::{ADMIN, MEMBER, OWNER, RoleTable, VIEWER, WorkspaceRole};
pub use scoped::{OrgRepo, PartitionRepo, PartitionScoped};
pub use tenant::WorkspaceDatabases;
pub use workspaces::{
    GrantedBy, MAX_NAME_LEN, MAX_SLUG_LEN, Membership, NewWorkspace, Workspaces, slugify,
};
