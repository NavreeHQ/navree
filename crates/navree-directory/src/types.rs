//! The values the service hands out: [`DirectoryConnection`],
//! [`SyncRun`], [`SyncChange`], [`SyncedUser`], [`SyncedGroup`] and their
//! enums.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_integrations::{Owner, OwnerType};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::user::{DirectoryGroup, DirectoryUser};

/// The source of a SCIM connection: the identity provider pushes changes.
pub const SCIM: &str = "scim";
/// Microsoft Entra ID through Microsoft Graph (delta queries).
pub const MICROSOFT: &str = "microsoft";
/// Google Workspace through the Admin SDK Directory API.
pub const GOOGLE: &str = "google";
/// Okta through its Users and Groups APIs.
pub const OKTA: &str = "okta";
/// LDAP and Active Directory.
pub const LDAP: &str = "ldap";

/// The sources Navree ships, in the order a settings page lists them.
pub const BUILT_IN_SOURCES: [&str; 5] = [SCIM, MICROSOFT, GOOGLE, OKTA, LDAP];

/// A directory connection: where an owner's people come from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DirectoryConnection {
    /// The connection's id.
    pub id: Uuid,
    /// Who it belongs to (`app`, `workspace` or `organization`).
    pub owner_type: OwnerType,
    /// The workspace or organization (`None` for the app).
    pub owner_id: Option<Uuid>,
    /// Its name, for people (`Agra Entra ID`).
    pub name: String,
    /// The source: `scim`, `microsoft`, `google`, `okta`, `ldap` or one
    /// your app registered.
    pub source: String,
    /// The source's settings (never a secret).
    #[schema(value_type = Object)]
    pub settings: Value,
    /// Whether a secret (the LDAP bind password) is stored.
    pub has_secret: bool,
    /// `active` or `paused` (paused: no syncs, SCIM requests refused).
    pub status: ConnectionStatus,
    /// The first characters of the SCIM token, to recognize it
    /// (`nvscim_3fQx`).
    pub scim_token_hint: Option<String>,
    /// Minutes between scheduled syncs (0: only on demand).
    pub sync_interval_mins: u32,
    /// The last run.
    pub last_run_id: Option<Uuid>,
    /// When a sync last finished applying.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// When the scheduler queues the next sync.
    pub next_sync_at: Option<DateTime<Utc>>,
    /// The user who created it.
    pub created_by: Option<Uuid>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl DirectoryConnection {
    /// Who it belongs to.
    pub fn owner(&self) -> Owner {
        match (self.owner_type, self.owner_id) {
            (OwnerType::Workspace, Some(id)) => Owner::Workspace(id),
            (OwnerType::Organization, Some(id)) => Owner::Organization(id),
            _ => Owner::App,
        }
    }

    /// Whether the identity provider pushes changes over SCIM.
    pub fn is_scim(&self) -> bool {
        self.source == SCIM
    }

    /// A setting as a string.
    pub fn setting(&self, key: &str) -> Option<&str> {
        self.settings.get(key).and_then(Value::as_str)
    }
}

/// Whether a connection works. Named `DirectoryConnectionStatus` in the
/// OpenAPI document: navree-integrations has a `ConnectionStatus` of its
/// own, and an app with both modules would otherwise fail to boot with a
/// schema conflict.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DirectoryConnectionStatus)]
pub enum ConnectionStatus {
    /// Syncs run and SCIM requests are served.
    #[default]
    Active,
    /// Nothing runs; SCIM requests get 403.
    Paused,
}

impl ConnectionStatus {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ConnectionStatus::Active => "active",
            ConnectionStatus::Paused => "paused",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "paused" => ConnectionStatus::Paused,
            _ => ConnectionStatus::Active,
        }
    }
}

/// Where a run stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DirectoryRunStatus)]
pub enum RunStatus {
    /// Waiting for a worker.
    Queued,
    /// Pulling, diffing or applying.
    Running,
    /// Done: every change applied (or, for a dry run, planned).
    Succeeded,
    /// Done, but some changes failed (see the run's changes); the delta
    /// cursor was kept, so the next run tries them again.
    Partial,
    /// Stopped before applying: it would deactivate more users than
    /// `directory.sync.max_deactivation_percent` allows. Approve it to run
    /// again without the limit.
    Held,
    /// The source could not be read, or applying failed.
    Failed,
    /// Its operation was cancelled.
    Cancelled,
}

impl RunStatus {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            RunStatus::Queued => "queued",
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::Partial => "partial",
            RunStatus::Held => "held",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "queued" => RunStatus::Queued,
            "running" => RunStatus::Running,
            "succeeded" => RunStatus::Succeeded,
            "partial" => RunStatus::Partial,
            "held" => RunStatus::Held,
            "cancelled" => RunStatus::Cancelled,
            _ => RunStatus::Failed,
        }
    }

    /// Whether the run is over.
    pub fn is_finished(self) -> bool {
        !matches!(self, RunStatus::Queued | RunStatus::Running)
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What started a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunTrigger {
    /// Someone asked (the API or [`Directory::sync`](crate::Directory::sync)).
    #[default]
    Manual,
    /// The scheduler (`directory:sync --due`).
    Schedule,
    /// The `directory:sync` command.
    Command,
}

impl RunTrigger {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            RunTrigger::Manual => "manual",
            RunTrigger::Schedule => "schedule",
            RunTrigger::Command => "command",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "schedule" => RunTrigger::Schedule,
            "command" => RunTrigger::Command,
            _ => RunTrigger::Manual,
        }
    }
}

/// How to run a sync.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct SyncOptions {
    /// Pull and diff, record the planned changes, apply nothing.
    pub dry_run: bool,
    /// Apply even when the run would deactivate more users than the limit.
    pub force: bool,
    /// Ignore the stored delta cursor and pull everything (a full sync
    /// also deactivates users missing from the directory).
    pub full: bool,
    /// What started it.
    pub trigger: RunTrigger,
}

impl SyncOptions {
    /// A dry run.
    pub fn dry_run() -> Self {
        Self {
            dry_run: true,
            ..Self::default()
        }
    }
}

/// The counts of a run: what the source gave, and what changed (planned,
/// for a dry run or a held run).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct RunSummary {
    /// Users the source gave.
    pub users_seen: u64,
    /// Groups the source gave.
    pub groups_seen: u64,
    /// Users seen for the first time.
    pub users_created: u64,
    /// Users whose attributes changed.
    pub users_updated: u64,
    /// Disabled or removed users who came back.
    pub users_reactivated: u64,
    /// Users disabled, removed or missing from a full pull.
    pub users_deactivated: u64,
    /// Users without a change.
    pub users_unchanged: u64,
    /// Groups seen for the first time.
    pub groups_created: u64,
    /// Groups whose attributes or members changed.
    pub groups_updated: u64,
    /// Groups removed from the directory.
    pub groups_deleted: u64,
    /// Groups without a change.
    pub groups_unchanged: u64,
    /// Members added to groups.
    pub memberships_added: u64,
    /// Members removed from groups.
    pub memberships_removed: u64,
    /// Records the source gave without an id or a name (left out).
    pub skipped: u64,
    /// Group members who are not users of this pull (left out).
    pub unknown_members: u64,
    /// Changes that failed to apply.
    pub failed: u64,
}

impl RunSummary {
    /// Every change: created, updated, reactivated and deactivated users
    /// and created, updated and deleted groups.
    pub fn changes(&self) -> u64 {
        self.users_created
            + self.users_updated
            + self.users_reactivated
            + self.users_deactivated
            + self.groups_created
            + self.groups_updated
            + self.groups_deleted
    }
}

/// A sync run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SyncRun {
    /// The run's id.
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// Where it stands.
    pub status: RunStatus,
    /// What started it.
    pub trigger: RunTrigger,
    /// Whether it only planned.
    pub dry_run: bool,
    /// Whether it ignored the deactivation limit.
    pub force: bool,
    /// Whether the source gave everything (`true`) or the changes since
    /// the last run (`false`); `None` until it pulled.
    pub complete: Option<bool>,
    /// The counts.
    pub summary: RunSummary,
    /// Changes applied so far.
    pub done: u64,
    /// Changes to apply.
    pub total: u64,
    /// What it is doing, for people.
    pub note: Option<String>,
    /// Why it failed or was held.
    pub error: Option<String>,
    /// The navree-queue operation that follows it, when the app has
    /// operations.
    pub operation_id: Option<Uuid>,
    /// The user who started it.
    pub created_by: Option<Uuid>,
    /// When it was queued.
    pub created_at: DateTime<Utc>,
    /// When a worker started it.
    pub started_at: Option<DateTime<Utc>>,
    /// When it finished.
    pub finished_at: Option<DateTime<Utc>>,
}

/// What a change does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeAction {
    /// A user or group seen for the first time.
    Create,
    /// Changed attributes (or a group's members).
    Update,
    /// A disabled or removed user who came back.
    Reactivate,
    /// A user disabled at the source, removed, or missing from a full pull.
    Deactivate,
    /// A group removed from the directory.
    Delete,
}

impl ChangeAction {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeAction::Create => "create",
            ChangeAction::Update => "update",
            ChangeAction::Reactivate => "reactivate",
            ChangeAction::Deactivate => "deactivate",
            ChangeAction::Delete => "delete",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "create" => ChangeAction::Create,
            "reactivate" => ChangeAction::Reactivate,
            "deactivate" => ChangeAction::Deactivate,
            "delete" => ChangeAction::Delete,
            _ => ChangeAction::Update,
        }
    }
}

/// What a change is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeSubject {
    /// A user.
    User,
    /// A group.
    Group,
}

impl ChangeSubject {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeSubject::User => "user",
            ChangeSubject::Group => "group",
        }
    }
}

/// Whether a recorded change was applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    /// Planned by a dry run or a held run.
    Planned,
    /// Applied.
    Applied,
    /// Applying failed (see `error`).
    Failed,
}

impl ChangeStatus {
    /// The stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeStatus::Planned => "planned",
            ChangeStatus::Applied => "applied",
            ChangeStatus::Failed => "failed",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "applied" => ChangeStatus::Applied,
            "failed" => ChangeStatus::Failed,
            _ => ChangeStatus::Planned,
        }
    }
}

/// One change of a run, as recorded (`directory.sync.max_recorded_changes`
/// per run; the run's counts are always complete).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SyncChange {
    /// The change's id.
    pub id: Uuid,
    /// The run.
    pub run_id: Uuid,
    /// A user or a group.
    pub subject: ChangeSubject,
    /// What it does.
    pub action: ChangeAction,
    /// The user's or group's id in the directory.
    pub external_id: String,
    /// The user's sign-in name or the group's name.
    pub label: String,
    /// The fields that change (`title`, `members`).
    pub fields: Vec<String>,
    /// Members added (groups).
    pub members_added: u64,
    /// Members removed (groups).
    pub members_removed: u64,
    /// Planned, applied or failed.
    pub status: ChangeStatus,
    /// Why it failed.
    pub error: Option<String>,
}

/// A user as the directory holds it for a connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SyncedUser {
    /// The id Navree gave the user (the SCIM id).
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// The user.
    pub user: DirectoryUser,
    /// The id of your record, as your sink returned it.
    pub local_id: Option<String>,
    /// When it was first seen.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// When it was deactivated.
    pub deactivated_at: Option<DateTime<Utc>>,
}

/// A group as the directory holds it for a connection (members as user
/// `external_id`s).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SyncedGroup {
    /// The id Navree gave the group (the SCIM id).
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// The group.
    pub group: DirectoryGroup,
    /// The id of your record, as your sink returned it.
    pub local_id: Option<String>,
    /// When it was first seen.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_names_round_trip() {
        for status in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::Succeeded,
            RunStatus::Partial,
            RunStatus::Held,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            assert_eq!(RunStatus::parse(status.as_str()), status);
        }
        for action in [
            ChangeAction::Create,
            ChangeAction::Update,
            ChangeAction::Reactivate,
            ChangeAction::Deactivate,
            ChangeAction::Delete,
        ] {
            assert_eq!(ChangeAction::parse(action.as_str()), action);
        }
        for trigger in [
            RunTrigger::Manual,
            RunTrigger::Schedule,
            RunTrigger::Command,
        ] {
            assert_eq!(RunTrigger::parse(trigger.as_str()), trigger);
        }
        for status in [
            ChangeStatus::Planned,
            ChangeStatus::Applied,
            ChangeStatus::Failed,
        ] {
            assert_eq!(ChangeStatus::parse(status.as_str()), status);
        }
        assert_eq!(ConnectionStatus::parse("paused"), ConnectionStatus::Paused);
        assert_eq!(ConnectionStatus::parse("x"), ConnectionStatus::Active);
        assert!(RunStatus::Held.is_finished());
        assert!(!RunStatus::Running.is_finished());
        assert_eq!(RunStatus::Partial.to_string(), "partial");
    }

    #[test]
    fn a_summary_counts_its_changes() {
        let summary = RunSummary {
            users_created: 3,
            users_updated: 1,
            users_deactivated: 2,
            groups_created: 1,
            users_unchanged: 40,
            ..RunSummary::default()
        };
        assert_eq!(summary.changes(), 7);
        assert!(SyncOptions::dry_run().dry_run);
    }
}
