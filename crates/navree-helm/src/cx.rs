//! [`Cx`]: who is asking, with what, inside which unit of work.

use std::sync::Arc;

use navree_access::{Ability, PrincipalAccess};
use navree_audit::AuditLog;
use navree_contracts::{AuditEntry, AuditKind, AuditSink};
use navree_db::Uow;
use navree_kernel::{AppContext, ModuleId};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::context::ContextInner;
use crate::core::Core;
use crate::workspace::NamedBy;

/// The client of a request, for audit entries.
#[derive(Clone, Debug, Default)]
pub(crate) struct Client {
    pub(crate) request_id: Option<String>,
    pub(crate) ip: Option<String>,
    pub(crate) user_agent: Option<String>,
}

/// One panel request (or queued action): the app, the unit of work, the
/// user and their access.
pub(crate) struct Cx {
    pub(crate) app: AppContext,
    pub(crate) uow: Uow,
    pub(crate) access: Arc<PrincipalAccess>,
    pub(crate) user_id: Uuid,
    pub(crate) core: Arc<Core>,
    pub(crate) client: Client,
    /// The browser's `getTimezoneOffset()`, sent by forms.
    pub(crate) tz_offset: Option<i32>,
    /// How the request named the workspace it entered, if it named one.
    pub(crate) named_by: Option<NamedBy>,
    /// The workspace the request's credential is limited to, if it is.
    pub(crate) pin: Option<Uuid>,
}

impl Cx {
    /// The user's rules.
    pub(crate) fn ability(&self) -> &Ability {
        &self.access.ability
    }

    /// `can(action, subject, record)`.
    pub(crate) fn can(&self, action: &str, subject: &str, record: Option<&Value>) -> bool {
        self.ability().can(action, subject, record)
    }

    /// What a resource's or page's code gets ([`SaveContext`](crate::SaveContext),
    /// [`ActionContext`](crate::ActionContext), [`PageContext`](crate::PageContext)).
    pub(crate) fn inner(&self, module: ModuleId, input: Map<String, Value>) -> ContextInner {
        ContextInner {
            app: self.app.clone(),
            module,
            uow: self.uow.clone(),
            user_id: self.user_id,
            input,
            access: self.access.clone(),
            form_may_name_workspace: false,
        }
    }

    /// `entry` with the user and client.
    fn entry_of_client(&self, entry: AuditEntry) -> AuditEntry {
        let mut entry = entry.actor(self.user_id);
        if let Some(id) = &self.client.request_id {
            entry = entry.request_id(id.clone());
        }
        if let Some(ip) = &self.client.ip {
            entry = entry.ip(ip.clone());
        }
        if let Some(agent) = &self.client.user_agent {
            entry = entry.user_agent(agent.clone());
        }
        entry
    }

    /// Records `entry` (with the user and client) outside the unit of work,
    /// so that it stays whatever becomes of the work: what staff do, which is
    /// written down whether or not it works.
    pub(crate) async fn audit_outside(&self, entry: AuditEntry) {
        record_outside(&self.app, self.entry_of_client(entry)).await;
    }

    /// Records `entry` (with the user and client) inside the unit of work
    /// when the app has navree-audit's `AuditLog` (it commits or rolls back
    /// with the work), else through the app's `dyn AuditSink`.
    pub(crate) async fn audit(&self, entry: AuditEntry) {
        let entry = self.entry_of_client(entry);
        let action = entry.action.clone();
        if let Some(log) = self.app.try_get::<AuditLog>().await {
            if let Err(error) = log.record_in(&self.uow, entry).await {
                tracing::warn!(target: "navree::helm", %error, %action, "could not write an audit entry");
            }
        } else if let Some(sink) = self.app.try_get::<dyn AuditSink>().await
            && let Err(error) = sink.record(entry).await
        {
            tracing::warn!(target: "navree::helm", %error, %action, "could not write an audit entry");
        }
    }
}

/// Records `entry` on the audit log's own connection when the app has
/// navree-audit's `AuditLog`, else through the app's `dyn AuditSink`.
pub(crate) async fn record_outside(app: &AppContext, entry: AuditEntry) {
    let action = entry.action.clone();
    let recorded = if let Some(log) = app.try_get::<AuditLog>().await {
        log.record(entry).await.map_err(|error| error.to_string())
    } else if let Some(sink) = app.try_get::<dyn AuditSink>().await {
        sink.record(entry).await.map_err(|error| error.to_string())
    } else {
        Ok(())
    };
    if let Err(error) = recorded {
        tracing::warn!(target: "navree::helm", %error, %action, "could not write an audit entry");
    }
}

/// The `helm.action` entry of an action run.
pub(crate) fn action_entry(
    resource: &str,
    subject: &str,
    action: &str,
    ids: &[String],
    queued: bool,
) -> AuditEntry {
    let mut entry = AuditEntry::new(AuditKind::Custom, "helm.action").metadata(serde_json::json!({
        "action": action,
        "resource": resource,
        "ids": ids,
        "queued": queued,
    }));
    if let [id] = ids {
        entry = entry.subject(subject, id.clone());
    } else {
        entry.subject_type = Some(subject.to_owned());
    }
    entry
}
