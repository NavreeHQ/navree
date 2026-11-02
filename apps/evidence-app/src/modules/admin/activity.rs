//! The audit trail in the panel: a read-only list of who changed what,
//! with filters and counts.
//!
//! [`AuditEntry`] is a view of the audit module's `audit_logs` table as a
//! model the panel can list. Entries are written by the audit module only,
//! so the resource is read-only: the panel shows no New, Edit or Delete for
//! one and refuses them.

use chrono::{DateTime, Utc};
use navree::helm::{Field, Filter, Metric, Resource};
use navree::orm::prelude::*;
use navree::prelude::*;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::accounts::UserAdmin;

/// One row of the audit trail. The permission prefix is `audit`, so
/// `audit.read` lets a role read the trail here and in the API.
#[derive(Model, Clone, Debug, PartialEq, Serialize)]
#[model(
    table = "audit_logs",
    subject = "AuditEntry",
    permission_prefix = "audit"
)]
pub struct AuditEntry {
    /// A UUID v7.
    pub id: Uuid,
    /// `data`, `auth`, `access`, `security` or `custom`.
    pub kind: String,
    /// What happened: `task.updated`, `auth.login`, `access.denied`, ...
    pub action: String,
    /// The acting user, if any.
    pub actor_id: Option<Uuid>,
    /// The user impersonating the actor, if any.
    pub impersonator_id: Option<Uuid>,
    /// The affected record type, such as `Task`.
    pub subject_type: Option<String>,
    /// The affected record id.
    pub subject_id: Option<String>,
    /// Values before the change.
    pub old_values: Option<serde_json::Value>,
    /// Values after the change.
    pub new_values: Option<serde_json::Value>,
    /// The request the entry was written in.
    pub request_id: Option<String>,
    /// The client address.
    pub ip: Option<String>,
    /// The client's user agent.
    pub user_agent: Option<String>,
    /// Extra data.
    pub metadata: serde_json::Value,
    /// When the entry was recorded.
    pub created_at: DateTime<Utc>,
}

/// The kinds of entry.
const KINDS: [(&str, &str); 5] = [
    ("data", "Data change"),
    ("auth", "Sign-in"),
    ("access", "Access"),
    ("security", "Security"),
    ("custom", "Custom"),
];

/// The record types most entries are about.
const SUBJECTS: [(&str, &str); 8] = [
    ("Task", "Task"),
    ("Project", "Project"),
    ("Report", "Report"),
    ("User", "User"),
    ("Workspace", "Workspace"),
    ("role", "Role"),
    ("Flag", "Feature flag"),
    ("Settings", "Settings"),
];

/// The audit trail, newest first.
#[injectable]
pub struct ActivityAdmin;

impl Resource for ActivityAdmin {
    type Model = AuditEntry;

    const LABEL: &'static str = "Audit log";
    const SINGULAR: &'static str = "Audit entry";
    const GROUP: &'static str = "System";
    const ICON: &'static str = "shield-check";
    const TITLE: &'static str = "action";
    const SEARCH: &'static [&'static str] = &["action", "subject_type", "subject_id"];
    const SLUG: &'static str = "audit";
    const PER_PAGE: u64 = 30;
    const ORDER: i32 = 1;
    const READ_ONLY: bool = true;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::datetime("created_at")
                .label("When")
                .readonly()
                .sortable(),
            Field::select("kind")
                .options(KINDS)
                .badge()
                .readonly()
                .sortable(),
            Field::text("action").readonly().sortable(),
            Field::computed("changes", changes),
            Field::belongs_to::<UserAdmin>("actor_id")
                .label("Actor")
                .readonly(),
            Field::text("subject_type")
                .label("Subject")
                .readonly()
                .sortable(),
            Field::text("subject_id")
                .label("Subject id")
                .readonly()
                .hide_on_index(),
            Field::text("request_id")
                .label("Request")
                .readonly()
                .hide_on_index(),
            Field::text("ip")
                .label("Address")
                .readonly()
                .hide_on_index(),
            Field::text("user_agent")
                .label("Client")
                .readonly()
                .hide_on_index(),
        ]
    }

    fn filters(&self) -> Vec<Filter<AuditEntry>> {
        vec![
            Filter::select("kind").options(KINDS),
            Filter::select("subject_type")
                .label("Subject")
                .options(SUBJECTS),
            Filter::belongs_to::<UserAdmin>("actor_id").label("Actor"),
            Filter::date_range("created_at").label("When"),
            Filter::custom("action_contains", "Action contains", |query, text| {
                query.where_(AuditEntry::ACTION.like(format!("%{text}%")))
            }),
        ]
    }

    fn metrics(&self) -> Vec<Metric<AuditEntry>> {
        vec![
            Metric::value("Entries")
                .count()
                .over(AuditEntry::CREATED_AT)
                .on_dashboard(),
            Metric::trend("Entries per day")
                .count()
                .by_day(AuditEntry::CREATED_AT, 30)
                .on_dashboard(),
            Metric::partition("Entries by kind")
                .count()
                .group_by(AuditEntry::KIND)
                .on_dashboard(),
        ]
    }
}

/// What an entry changed, as `field: before -> after` pairs.
fn changes(entry: &AuditEntry) -> String {
    let Some(new) = entry.new_values.as_ref().and_then(Value::as_object) else {
        return String::new();
    };
    let old = entry.old_values.as_ref().and_then(Value::as_object);
    new.iter()
        .filter(|(field, _)| {
            !matches!(
                field.as_str(),
                "id" | "created_at" | "updated_at" | "workspace_id"
            )
        })
        .take(3)
        .map(|(field, value)| match old.and_then(|old| old.get(field)) {
            Some(before) => format!("{field}: {} -> {}", shown(before), shown(value)),
            None => format!("{field}: {}", shown(value)),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A value as short text.
fn shown(value: &Value) -> String {
    match value {
        Value::String(text) if text.chars().count() > 32 => {
            format!("{}...", text.chars().take(32).collect::<String>())
        }
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
