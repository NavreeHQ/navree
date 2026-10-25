//! The audit trail contract: [`AuditSink`] and [`AuditEntry`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{BoxError, BoxFuture};

/// Writes audit entries (implemented by navree-audit). Modules resolve it as
/// `Arc<dyn AuditSink>`, usually optionally.
pub trait AuditSink: Send + Sync + 'static {
    /// Records one entry.
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>>;
}

/// The category of an audit entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    /// A data change (created, updated, deleted).
    Data,
    /// An authentication event (login, logout, password reset).
    Auth,
    /// An authorization change (roles, permissions).
    Access,
    /// A security event (failed login, lockout, CSRF failure).
    Security,
    /// Anything else.
    Custom,
}

/// One audit trail entry. Build it with [`AuditEntry::new`] and the setters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    /// The category.
    pub kind: AuditKind,
    /// What happened, e.g. `deal.updated` or `auth.login`.
    pub action: String,
    /// The acting user, if any.
    pub actor_id: Option<Uuid>,
    /// The affected record type, e.g. `deal`.
    pub subject_type: Option<String>,
    /// The affected record id.
    pub subject_id: Option<String>,
    /// Values before the change.
    pub old_values: Option<Value>,
    /// Values after the change.
    pub new_values: Option<Value>,
    /// The request id of the unit of work.
    pub request_id: Option<String>,
    /// The client IP address.
    pub ip: Option<String>,
    /// The client user agent.
    pub user_agent: Option<String>,
    /// Extra data (an object; `{}` by default).
    pub metadata: Value,
}

impl AuditEntry {
    /// An entry of `kind` for `action`, with every optional field empty.
    pub fn new(kind: AuditKind, action: impl Into<String>) -> Self {
        Self {
            kind,
            action: action.into(),
            actor_id: None,
            subject_type: None,
            subject_id: None,
            old_values: None,
            new_values: None,
            request_id: None,
            ip: None,
            user_agent: None,
            metadata: Value::Object(Default::default()),
        }
    }

    /// Sets the acting user.
    pub fn actor(mut self, actor_id: Uuid) -> Self {
        self.actor_id = Some(actor_id);
        self
    }

    /// Sets the affected record.
    pub fn subject(
        mut self,
        subject_type: impl Into<String>,
        subject_id: impl Into<String>,
    ) -> Self {
        self.subject_type = Some(subject_type.into());
        self.subject_id = Some(subject_id.into());
        self
    }

    /// Sets the values before the change.
    pub fn old_values(mut self, values: Value) -> Self {
        self.old_values = Some(values);
        self
    }

    /// Sets the values after the change.
    pub fn new_values(mut self, values: Value) -> Self {
        self.new_values = Some(values);
        self
    }

    /// Sets the request id.
    pub fn request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Sets the client IP address.
    pub fn ip(mut self, ip: impl Into<String>) -> Self {
        self.ip = Some(ip.into());
        self
    }

    /// Sets the client user agent.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Sets the extra data.
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}
