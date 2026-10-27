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
//! - **Routes.** `/api/workspaces...` and `POST /api/invitations/accept`
//!   ([`WorkspaceController`], [`InvitationController`]), unless
//!   `workspace.routes` is false.
//! - **Demo workspaces** ([`DemoWorkspaces`]). Workspaces of kind
//!   [`WorkspaceKind::Demo`], filled from fixtures by each module's
//!   [`DemoSeeder`] (`m.demo_seeder::<CrmDemo>()`). With
//!   `workspace.demo.mode = "read_only"` writes to them are refused (403,
//!   code `demo_read_only`); with `"reset"` they are cleared and seeded
//!   again every night (`workspace:demo-reset`). Responses about a demo
//!   carry `X-Navree-Demo: read_only|reset` ([`DEMO_HEADER`]).
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

mod access;
mod commands;
mod config;
mod controller;
mod core;
mod current;
mod demo;
mod directory;
mod error;
mod fixtures;
mod invitations;
mod isolation;
mod members;
mod migration;
mod module;
mod resolve;
mod role;
mod tenant;
mod workspaces;

pub use config::{
    DemoMode, DemoOptions, InvitationOptions, Isolation, ResolverKind, RoleOptions, WorkspaceConfig,
};
pub use controller::{
    AcceptInvitationInput, AcceptedInvitationResource, AddMemberInput, ChangeRoleInput,
    CreateWorkspaceInput, InvitationController, InvitationResource, InviteInput, MemberResource,
    RenameWorkspaceInput, TransferWorkspaceInput, WorkspaceController, WorkspaceResource,
};
pub use current::{CurrentWorkspace, InWorkspace, PinnedWorkspace, WorkspaceUow};
pub use demo::{DEMO_HEADER, DemoReset, DemoSeed, DemoSeeder, DemoWorkspaces};
pub use directory::{TableUserDirectory, UserDirectory, UserSummary};
pub use entity::{
    INVITATION_SUBJECT, Invitation, MEMBER_SUBJECT, Member, WORKSPACE_SUBJECT, Workspace,
    WorkspaceKind,
};
pub use error::{DEMO_READ_ONLY, WorkspaceError};
pub use invitations::{AcceptedInvitation, Invitations, SentInvitation, TOKEN_LEN};
pub use isolation::{RlsPolicy, WorkspaceRepo, WorkspaceScoped, rls_policy_sql};
pub use members::{MemberInfo, Members};
pub use migration::WORKSPACE_MIGRATIONS;
pub use module::{ModuleBuilderWorkspaceExt, WorkspaceModule};
pub use role::{ADMIN, MEMBER, OWNER, RoleTable, VIEWER, WorkspaceRole};
pub use tenant::WorkspaceDatabases;
pub use workspaces::{MAX_NAME_LEN, MAX_SLUG_LEN, Membership, NewWorkspace, Workspaces, slugify};
