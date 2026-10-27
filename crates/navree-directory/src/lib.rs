//! # navree-directory
//!
//! `DirectoryModule`: directory sync for business apps.
#![forbid(unsafe_code)]

pub mod scim;

mod diff;
mod pulled;
mod types;
mod user;

pub use pulled::{MemberChange, Pulled};
pub use types::{
    BUILT_IN_SOURCES, ChangeAction, ChangeStatus, ChangeSubject, ConnectionStatus,
    DirectoryConnection, GOOGLE, LDAP, MICROSOFT, OKTA, RunStatus, RunSummary, RunTrigger, SCIM,
    SyncChange, SyncOptions, SyncRun, SyncedGroup, SyncedUser,
};
pub use user::{ContactValue, DirectoryGroup, DirectoryUser};
