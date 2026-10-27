//! # navree-directory
//!
//! `DirectoryModule`: your customers' people, kept in step with their
//! directory.
//!
//! - **A SCIM 2.0 server** (RFC 7643, RFC 7644) at `/scim/v2`: `Users`,
//!   `Groups`, `ServiceProviderConfig`, `ResourceTypes` and `Schemas`,
//!   with filters, PATCH, paging and attribute projection. Each SCIM
//!   connection has its own bearer token, kept as a SHA-256 hash, so
//!   Microsoft Entra ID, Okta, OneLogin or JumpCloud provision users into
//!   the right workspace.
//! - **Pull connectors**: Microsoft Graph with delta queries, Google
//!   Workspace, Okta (their clients come from navree-integrations'
//!   connections, scope set `directory`), and LDAP or Active Directory
//!   through ldap3 with rustls. Register your own with
//!   [`ModuleBuilderDirectoryExt::directory_source`].
//! - **A normalized directory**: [`DirectoryUser`] and [`DirectoryGroup`],
//!   kept per connection in navree-orm models ([`models`]).
//! - **Your model**: `impl DirectorySink for RecipientSink` gets every
//!   change inside its unit of work ([`DirectorySink`]).
//! - **Runs** stage (pull), diff and apply, with dry runs, a deactivation
//!   limit that holds a run for approval, progress on navree-queue
//!   operations, recorded changes, and the events
//!   [`DirectoryUserCreated`], [`DirectoryUserUpdated`],
//!   [`DirectoryUserDeactivated`] and [`DirectorySyncFinished`].
//! - **Commands** `directory:sync`, `directory:runs`,
//!   `directory:connections`, `directory:token` and `directory:prune`.
//! - **Tests**: [`Directory::fake`] gives a [`DirectoryFake`] that every
//!   pull reads.
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_crypto::CryptoModule;
//! use navree_db::DatabaseModule;
//! use navree_directory::DirectoryModule;
//! use navree_http::HttpModule;
//! use navree_kernel::Navree;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(CryptoModule::from_config())
//!     .module(HttpModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(DirectoryModule::from_config())
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod events;
pub mod models;
pub mod scim;

mod apply;
mod commands;
mod controller;
mod diff;
mod error;
mod fake;
mod migration;
mod module;
mod options;
mod owner;
mod pulled;
mod runs;
mod runtime;
mod service;
mod sink;
mod source;
mod types;
mod user;

pub use controller::{DirectoryController, MANAGE_PERMISSION, PageInput, ScimTokenOutput, SyncInput};
pub use error::DirectoryError;
pub use events::{
    DirectorySyncFinished, DirectoryUserCreated, DirectoryUserDeactivated, DirectoryUserUpdated,
};
pub use fake::{DirectoryFake, FakePull};
pub use migration::DIRECTORY_MIGRATIONS;
pub use module::{DirectoryModule, DirectoryScopes, ModuleBuilderDirectoryExt};
pub use navree_integrations::{Level, Owner};
pub use options::{DirectoryOptions, ScimOptions, SyncSettings};
pub use owner::DirectoryOwner;
pub use pulled::{MemberChange, Pulled};
pub use runs::SyncDirectory;
pub use scim::controller::ScimController;
pub use service::{
    ConnectionUpdate, CreatedConnection, Directory, NewConnection, SCIM_TOKEN_PREFIX,
};
pub use sink::{DirectorySink, GroupChange, SinkContext, UserChange};
pub use source::{DIRECTORY_SCOPE_SET, DirectorySource, PullRequest};
pub use types::{
    BUILT_IN_SOURCES, ChangeAction, ChangeStatus, ChangeSubject, ConnectionStatus,
    DirectoryConnection, GOOGLE, LDAP, MICROSOFT, OKTA, RunStatus, RunSummary, RunTrigger, SCIM,
    SyncChange, SyncOptions, SyncRun, SyncedGroup, SyncedUser,
};
pub use user::{ContactValue, DirectoryGroup, DirectoryUser};
