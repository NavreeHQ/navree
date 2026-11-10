//! [`Cx`]: who is asking, with what, inside which unit of work.

use std::collections::HashMap;
use std::sync::Arc;

use navree_access::{
    Ability, CREATE, DELETE, PrincipalAccess, UPDATE, WorkspaceGate, WorkspaceWrites,
};
use navree_audit::AuditLog;
use navree_contracts::{AuditEntry, AuditKind, AuditSink};
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::{AppContext, ModuleId};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::context::ContextInner;
use crate::core::Core;
use crate::handle::ResourceInfo;
use crate::workspace::{NamedBy, RefusedWrite};

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
    /// The zone the panel's pages show date-times in and read the ones typed into its
    /// forms in: the request's `CurrentZone` when the app has `TimeModule`, else UTC.
    pub(crate) zone: navree_time::Zone,
    /// How the request named the workspace it entered, if it named one.
    pub(crate) named_by: Option<NamedBy>,
    /// The workspace the request's credential is limited to, if it is.
    pub(crate) pin: Option<Uuid>,
    /// What the workspace the request works in takes from the user, when the
    /// request may write and the app has a gate.
    pub(crate) writes: Option<WorkspaceWrites>,
    /// Why a write can't go to the workspace the panel's cookie names, which
    /// the request isn't in for that reason.
    pub(crate) refused_write: Option<RefusedWrite>,
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

    /// Whether the panel offers `action` (`create`, `update`, `delete` or an
    /// action's ability) on the records of `info`: what the resource and the
    /// workspace the request works in take from the user. It decides what the
    /// pages show; the writes themselves are checked again when they run.
    pub(crate) fn offers(&self, info: &ResourceInfo, action: &str) -> bool {
        if info.read_only && matches!(action, CREATE | UPDATE | DELETE) {
            return false;
        }
        takes_writes(
            self.writes.as_ref(),
            self.named_by,
            info.workspaces.is_some(),
            info.permission_prefix,
            action,
        )
    }

    /// [`offers`](Self::offers) for a record of `owner`'s workspace, which a
    /// resource across workspaces shows beside others: `memo` keeps what each
    /// workspace said.
    pub(crate) async fn offers_for(
        &self,
        info: &ResourceInfo,
        action: &str,
        owner: Option<Uuid>,
        memo: &mut HashMap<Uuid, Option<WorkspaceWrites>>,
    ) -> bool {
        let Some(owner) = owner
            .filter(|owner| Some(*owner) != self.uow.workspace())
            .filter(|_| info.workspaces == Some(crate::Workspaces::Every))
        else {
            return self.offers(info, action);
        };
        if info.read_only && matches!(action, CREATE | UPDATE | DELETE) {
            return false;
        }
        let writes = match memo.get(&owner) {
            Some(writes) => writes.clone(),
            None => {
                let writes = match self.app.try_get::<dyn WorkspaceGate>().await {
                    Some(gate) => gate.writes(self.user_id, owner).await.ok(),
                    None => None,
                };
                memo.insert(owner, writes.clone());
                writes
            }
        };
        takes_writes(writes.as_ref(), None, true, info.permission_prefix, action)
    }

    /// What `workspace` takes from the user: the workspace the request works in
    /// when it is that one or `None`, else the gate says.
    async fn writes_to(
        &self,
        workspace: Option<Uuid>,
    ) -> Result<Option<WorkspaceWrites>, AppError> {
        match workspace.filter(|workspace| Some(*workspace) != self.uow.workspace()) {
            None => Ok(self.writes.clone()),
            Some(other) => match self.app.try_get::<dyn WorkspaceGate>().await {
                Some(gate) => gate.writes(self.user_id, other).await.map(Some),
                None => Ok(None),
            },
        }
    }

    /// An error if the user's role in `workspace` (see
    /// [`writes_to`](Self::writes_to)) doesn't write `action` on the model with
    /// the permission prefix `prefix`.
    pub(crate) async fn role_writes(
        &self,
        workspace: Option<Uuid>,
        prefix: &str,
        action: &str,
    ) -> Result<(), AppError> {
        match self.writes_to(workspace).await? {
            Some(writes) if !writes.role_writes(prefix, action) => {
                Err(WorkspaceWrites::read_only_role_error())
            }
            _ => Ok(()),
        }
    }

    /// [`role_writes`](Self::role_writes), for a record in `workspace`, which a
    /// read-only demo refuses as well when it isn't the workspace the request
    /// entered, and so was asked about already.
    pub(crate) async fn record_writes(
        &self,
        workspace: Option<Uuid>,
        prefix: &str,
        action: &str,
    ) -> Result<(), AppError> {
        if let Some(other) = workspace.filter(|workspace| Some(*workspace) != self.uow.workspace())
            && let Some(gate) = self.app.try_get::<dyn WorkspaceGate>().await
            && gate.writes(self.user_id, other).await?.read_only_demo
        {
            return Err(AppError::demo_read_only());
        }
        self.role_writes(workspace, prefix, action).await
    }

    /// `entry` with the user and client.
    pub(crate) fn entry_of_client(&self, entry: AuditEntry) -> AuditEntry {
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

/// Whether the workspace a request works in (`writes`, as its gate said of
/// the user) takes `action` on a model with the permission prefix `prefix`.
///
/// A model of workspaces takes what the user's role writes there, and nothing
/// in a read-only demo. Another model is refused only what the app's own
/// workspace refuses outright: the workspace the panel was told of (a cookie)
/// is entered for the models that belong to it.
pub(crate) fn takes_writes(
    writes: Option<&WorkspaceWrites>,
    named_by: Option<NamedBy>,
    workspace_model: bool,
    prefix: &str,
    action: &str,
) -> bool {
    let Some(writes) = writes else {
        return true;
    };
    if workspace_model {
        !writes.read_only_demo && writes.role_writes(prefix, action)
    } else {
        !(named_by.is_none() && writes.refusal().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writes(read_only_demo: bool, read_only_role: bool, permissions: &[&str]) -> WorkspaceWrites {
        WorkspaceWrites {
            read_only_demo,
            read_only_role,
            role_permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    #[test]
    fn a_workspace_takes_what_its_role_writes() {
        let member = writes(false, false, &[]);
        let viewer = writes(false, true, &[]);
        let demo = writes(true, false, &[]);
        for (who, expected) in [(&member, true), (&viewer, false), (&demo, false)] {
            assert_eq!(
                takes_writes(Some(who), Some(NamedBy::Cookie), true, "notes", "update"),
                expected
            );
        }
        // No workspace, no gate: the user's rules are all there is.
        assert!(takes_writes(None, None, true, "notes", "update"));
        // A viewer who the app lets update notes does.
        let allowed = writes(false, true, &["notes.update"]);
        assert!(takes_writes(Some(&allowed), None, true, "notes", "update"));
        assert!(!takes_writes(Some(&allowed), None, true, "notes", "delete"));
    }

    #[test]
    fn a_model_outside_workspaces_is_refused_only_by_the_workspace_the_app_entered() {
        let viewer = writes(false, true, &[]);
        assert!(!takes_writes(Some(&viewer), None, false, "users", "update"));
        assert!(takes_writes(
            Some(&viewer),
            Some(NamedBy::Cookie),
            false,
            "users",
            "update"
        ));
    }
}
