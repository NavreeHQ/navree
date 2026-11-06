//! The module's tables as navree-orm models.
//!
//! Read them freely (`models::User::query().where_(models::User::CONNECTION_ID.eq(id))`);
//! change them through [`Directory`](crate::Directory), which keeps the
//! directory, your [sink](crate::DirectorySink) and the events in step.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_orm::Model;
use serde::Serialize;
use uuid::Uuid;

/// A row of `directory_connections`: one directory an owner syncs from.
///
/// `secret` is a navree-crypto ciphertext and `scim_token_hash` a SHA-256;
/// both, and the delta `cursor`, are left out of serialized records and
/// `Debug`.
#[derive(Model, Clone, Default, PartialEq, Eq, Serialize)]
#[model(table = "directory_connections")]
pub struct Connection {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// `app`, `workspace` or `organization`.
    pub owner_type: String,
    /// The workspace or organization (the nil UUID for the app).
    pub owner_id: Uuid,
    /// The connection's name.
    pub name: String,
    /// `scim`, `microsoft`, `google`, `okta`, `ldap` or a registered one.
    pub source: String,
    /// The source's settings as JSON text.
    pub settings: String,
    /// The encrypted secret (the LDAP bind password). Never serialized.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub secret: Option<String>,
    /// The SHA-256 (hex) of the SCIM token. Never serialized.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub scim_token_hash: Option<String>,
    /// The token's first characters.
    pub scim_token_hint: Option<String>,
    /// Where the next pull continues (a delta link). Never serialized.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub cursor: Option<String>,
    /// `active` or `paused`.
    pub status: String,
    /// Minutes between scheduled syncs (0: on demand only).
    pub sync_interval_mins: i32,
    /// The last run.
    pub last_run_id: Option<Uuid>,
    /// When a run last finished applying.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// When the scheduler queues the next run.
    pub next_sync_at: Option<DateTime<Utc>>,
    /// Who created it.
    pub created_by: Option<Uuid>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("owner_type", &self.owner_type)
            .field("owner_id", &self.owner_id)
            .field("name", &self.name)
            .field("source", &self.source)
            .field("status", &self.status)
            .field("has_secret", &self.secret.is_some())
            .field("has_scim_token", &self.scim_token_hash.is_some())
            .finish_non_exhaustive()
    }
}

/// A row of `directory_users`: a user of one connection.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "directory_users")]
pub struct User {
    /// The id (the SCIM id).
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// The user's id in the directory (unique per connection).
    pub external_id: String,
    /// The sign-in name.
    pub user_name: String,
    /// The primary email.
    pub email: Option<String>,
    /// The name to show.
    pub display_name: Option<String>,
    /// Whether the user is active.
    pub active: bool,
    /// The [`DirectoryUser`](crate::DirectoryUser) as JSON text.
    pub data: String,
    /// The SCIM resource as the client wrote it (SCIM connections).
    pub resource: Option<String>,
    /// The SHA-256 of `data`.
    pub fingerprint: String,
    /// Your record's id, as your sink returned it.
    pub local_id: Option<String>,
    /// When it was first seen.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// When it was deactivated.
    pub deactivated_at: Option<DateTime<Utc>>,
}

/// A row of `directory_groups`: a group of one connection.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "directory_groups")]
pub struct Group {
    /// The id (the SCIM id).
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// The group's id in the directory (unique per connection).
    pub external_id: String,
    /// The group's name.
    pub display_name: String,
    /// The [`DirectoryGroup`](crate::DirectoryGroup) without members, as
    /// JSON text.
    pub data: String,
    /// The SCIM resource as the client wrote it, without members (SCIM
    /// connections).
    pub resource: Option<String>,
    /// The SHA-256 of `data`.
    pub fingerprint: String,
    /// Your record's id, as your sink returned it.
    pub local_id: Option<String>,
    /// When it was first seen.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A row of `directory_memberships`: a user in a group.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "directory_memberships")]
pub struct Membership {
    /// The id.
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// The group (`directory_groups.id`).
    pub group_id: Uuid,
    /// The user (`directory_users.id`).
    pub user_id: Uuid,
}

/// A row of `directory_runs`: one sync run.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "directory_runs")]
pub struct Run {
    /// The id.
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// `queued`, `running`, `succeeded`, `partial`, `held`, `failed` or
    /// `cancelled`.
    pub status: String,
    /// `manual`, `schedule` or `command`.
    pub triggered_by: String,
    /// Whether it only plans.
    pub dry_run: bool,
    /// Whether it ignores the deactivation limit.
    pub force: bool,
    /// Whether it ignores the delta cursor.
    pub full_sync: bool,
    /// Whether the source gave everything (`None` until it pulled).
    pub complete: Option<bool>,
    /// The [`RunSummary`](crate::RunSummary) as JSON text.
    pub summary: String,
    /// Changes applied.
    pub done: i64,
    /// Changes to apply.
    pub total: i64,
    /// What it is doing.
    pub note: Option<String>,
    /// Why it failed or was held.
    pub error: Option<String>,
    /// The navree-queue operation following it.
    pub operation_id: Option<Uuid>,
    /// Who started it.
    pub created_by: Option<Uuid>,
    /// When it was queued.
    pub created_at: DateTime<Utc>,
    /// When it started.
    pub started_at: Option<DateTime<Utc>>,
    /// When it finished.
    pub finished_at: Option<DateTime<Utc>>,
}

/// A row of `directory_run_changes`: one recorded change of a run.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "directory_run_changes")]
pub struct RunChange {
    /// The id.
    pub id: Uuid,
    /// The run.
    pub run_id: Uuid,
    /// `user` or `group`.
    pub subject: String,
    /// `create`, `update`, `reactivate`, `deactivate` or `delete`.
    pub action: String,
    /// The user's or group's id in the directory.
    pub external_id: String,
    /// The sign-in name or the group's name.
    pub label: String,
    /// The changed fields as a JSON array.
    pub fields: String,
    /// Members added (groups).
    pub members_added: i32,
    /// Members removed (groups).
    pub members_removed: i32,
    /// `planned`, `applied` or `failed`.
    pub status: String,
    /// Why it failed.
    pub error: Option<String>,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_stay_out_of_debug_and_json() {
        let row = Connection {
            secret: Some("nvc1.ciphertext".into()),
            scim_token_hash: Some("ab12".into()),
            cursor: Some("https://graph.microsoft.com/v1.0/users/delta?$deltatoken=x".into()),
            name: "Agra".into(),
            ..Connection::default()
        };
        let debug = format!("{row:?}");
        assert!(!debug.contains("ciphertext"), "{debug}");
        assert!(!debug.contains("deltatoken"), "{debug}");
        assert!(debug.contains("has_secret: true"));
        let json = serde_json::to_value(&row).unwrap();
        assert!(json.get("secret").is_none());
        assert!(json.get("scim_token_hash").is_none());
        assert!(json.get("cursor").is_none());
        assert_eq!(Connection::meta().table(), "directory_connections");
        for hidden in ["secret", "scim_token_hash", "cursor"] {
            assert!(Connection::meta().hidden_names().contains(&hidden));
        }
        assert_eq!(User::meta().table(), "directory_users");
        assert_eq!(Group::meta().table(), "directory_groups");
        assert_eq!(Membership::meta().table(), "directory_memberships");
        assert_eq!(Run::meta().table(), "directory_runs");
        assert_eq!(RunChange::meta().table(), "directory_run_changes");
    }
}
