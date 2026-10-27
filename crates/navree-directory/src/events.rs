//! The directory module's events, emitted after their change commits when
//! the app has `EventsModule`. They carry the normalized user, so a
//! listener (a welcome mail, an audit trail, a search index) needs no
//! query.

use navree_events::Event;
use navree_integrations::Owner;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{RunStatus, RunSummary};
use crate::user::DirectoryUser;

/// A user appeared in a directory: the first pull that saw them, or a
/// SCIM `POST /Users`. The user may arrive inactive (`user.active`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectoryUserCreated {
    /// The connection.
    pub connection_id: Uuid,
    /// Who the connection belongs to.
    pub owner: Owner,
    /// The id Navree gave the user (the SCIM id).
    pub directory_user_id: Uuid,
    /// Your record's id, as your sink returned it.
    pub local_id: Option<String>,
    /// The user.
    pub user: DirectoryUser,
    /// The run that saw them (`None` for SCIM).
    pub run_id: Option<Uuid>,
}

impl Event for DirectoryUserCreated {
    const NAME: &'static str = "directory.user_created";
}

/// A user's attributes changed, or a disabled or removed user came back
/// (`reactivated`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectoryUserUpdated {
    /// The connection.
    pub connection_id: Uuid,
    /// Who the connection belongs to.
    pub owner: Owner,
    /// The id Navree gave the user.
    pub directory_user_id: Uuid,
    /// Your record's id.
    pub local_id: Option<String>,
    /// The user after the change.
    pub user: DirectoryUser,
    /// The fields that changed (`title`, `emails`, `active`).
    pub changed: Vec<String>,
    /// Whether the user was inactive before.
    pub reactivated: bool,
    /// The run (`None` for SCIM).
    pub run_id: Option<Uuid>,
}

impl Event for DirectoryUserUpdated {
    const NAME: &'static str = "directory.user_updated";
}

/// A user was disabled in the directory, removed from it, missing from a
/// full pull, set inactive or deleted over SCIM.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectoryUserDeactivated {
    /// The connection.
    pub connection_id: Uuid,
    /// Who the connection belongs to.
    pub owner: Owner,
    /// The id Navree gave the user.
    pub directory_user_id: Uuid,
    /// Your record's id.
    pub local_id: Option<String>,
    /// The user, inactive.
    pub user: DirectoryUser,
    /// Whether the SCIM client deleted the user (their directory row is
    /// gone).
    pub deleted: bool,
    /// The run (`None` for SCIM).
    pub run_id: Option<Uuid>,
}

impl Event for DirectoryUserDeactivated {
    const NAME: &'static str = "directory.user_deactivated";
}

/// A run finished: applied, planned (a dry run), held, failed or
/// cancelled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectorySyncFinished {
    /// The run.
    pub run_id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// Who the connection belongs to.
    pub owner: Owner,
    /// How it ended.
    pub status: RunStatus,
    /// Whether it only planned.
    pub dry_run: bool,
    /// The counts.
    pub summary: RunSummary,
    /// Why it failed or was held.
    pub error: Option<String>,
}

impl Event for DirectorySyncFinished {
    const NAME: &'static str = "directory.sync_finished";
}
