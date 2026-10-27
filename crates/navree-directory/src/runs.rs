//! Sync runs: stage (pull), diff, apply, with dry runs, the deactivation
//! limit, progress and the [`SyncDirectory`] job.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::{Job, JobContext, JobOptions};
use navree_db::Uow;
use navree_kernel::BoxError;
use navree_orm::{Model as _, Record};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Directory;
use crate::apply::Applier;
use crate::diff::{
    Current, CurrentGroup, CurrentUser, GroupStep, Plan, UserStep, diff, exceeds_deactivation_limit,
};
use crate::error::DirectoryError;
use crate::events::DirectorySyncFinished;
use crate::models;
use crate::pulled::Pulled;
use crate::runtime::Runtime;
use crate::source::PullRequest;
use crate::types::{
    ChangeStatus, ChangeSubject, ConnectionStatus, DirectoryConnection, RunStatus, RunSummary,
    RunTrigger, SyncChange, SyncRun,
};
use crate::user::{DirectoryGroup, DirectoryUser};

/// The job of a queued run: `directory.sync`, one attempt, up to an hour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDirectory {
    /// The run.
    pub run_id: Uuid,
}

impl Job for SyncDirectory {
    const NAME: &'static str = "directory.sync";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 1,
            timeout_secs: 3600,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let directory = ctx.app.get::<Directory>().await?;
        let rt = directory.rt()?.clone();
        rt.execute(self.run_id, Some(&ctx)).await?;
        Ok(())
    }
}

/// One step of a plan, in apply order.
#[derive(Clone, Debug)]
enum Step {
    User(Box<UserStep>),
    Group(Box<GroupStep>),
}

impl Step {
    fn subject(&self) -> ChangeSubject {
        match self {
            Step::User(_) => ChangeSubject::User,
            Step::Group(_) => ChangeSubject::Group,
        }
    }
}

fn steps(plan: Plan) -> Vec<Step> {
    let user = |step| Step::User(Box::new(step));
    let group = |step| Step::Group(Box::new(step));
    plan.users
        .into_iter()
        .map(user)
        .chain(plan.groups.into_iter().map(group))
        .chain(plan.deleted_groups.into_iter().map(group))
        .chain(plan.deactivations.into_iter().map(user))
        .collect()
}

/// The recorded form of a step.
fn change_row(
    run_id: Uuid,
    step: &Step,
    status: ChangeStatus,
    error: Option<String>,
    now: DateTime<Utc>,
) -> models::RunChange {
    let (action, external_id, label, fields, added, removed) = match step {
        Step::User(step) => (
            step.action,
            step.user.external_id.clone(),
            step.user.user_name.clone(),
            step.changed.clone(),
            0,
            0,
        ),
        Step::Group(step) => (
            step.action,
            step.group.external_id.clone(),
            step.group.display_name.clone(),
            step.changed.clone(),
            step.added.len(),
            step.removed.len(),
        ),
    };
    models::RunChange {
        id: Uuid::now_v7(),
        run_id,
        subject: step.subject().as_str().to_owned(),
        action: action.as_str().to_owned(),
        external_id: truncate(&external_id, 255),
        label: truncate(&label, 255),
        fields: serde_json::to_string(&fields).unwrap_or_else(|_| "[]".to_owned()),
        members_added: i32::try_from(added).unwrap_or(i32::MAX),
        members_removed: i32::try_from(removed).unwrap_or(i32::MAX),
        status: status.as_str().to_owned(),
        error: error.map(|e| truncate(&e, 2000)),
        created_at: now,
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// The public view of a run row.
pub(crate) fn run_view(row: &models::Run) -> SyncRun {
    SyncRun {
        id: row.id,
        connection_id: row.connection_id,
        status: RunStatus::parse(&row.status),
        trigger: RunTrigger::parse(&row.triggered_by),
        dry_run: row.dry_run,
        force: row.force,
        complete: row.complete,
        summary: serde_json::from_str(&row.summary).unwrap_or_default(),
        done: u64::try_from(row.done.max(0)).unwrap_or(0),
        total: u64::try_from(row.total.max(0)).unwrap_or(0),
        note: row.note.clone(),
        error: row.error.clone(),
        operation_id: row.operation_id,
        created_by: row.created_by,
        created_at: row.created_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
    }
}

/// The public view of a change row.
pub(crate) fn change_view(row: &models::RunChange) -> SyncChange {
    SyncChange {
        id: row.id,
        run_id: row.run_id,
        subject: if row.subject == "group" {
            ChangeSubject::Group
        } else {
            ChangeSubject::User
        },
        action: crate::types::ChangeAction::parse(&row.action),
        external_id: row.external_id.clone(),
        label: row.label.clone(),
        fields: serde_json::from_str(&row.fields).unwrap_or_default(),
        members_added: u64::try_from(row.members_added.max(0)).unwrap_or(0),
        members_removed: u64::try_from(row.members_removed.max(0)).unwrap_or(0),
        status: ChangeStatus::parse(&row.status),
        error: row.error.clone(),
    }
}

/// How a run ends.
struct Outcome {
    status: RunStatus,
    summary: RunSummary,
    complete: Option<bool>,
    done: u64,
    total: u64,
    error: Option<String>,
    /// The cursor to keep (only after every change applied).
    cursor: Option<Option<String>>,
}

impl Runtime {
    /// Runs the queued run `run_id` to its end: pull, diff, then hold,
    /// plan or apply. Progress goes to the run and, through `job`, to its
    /// operation. A run that is not queued is left as it is.
    pub(crate) async fn execute(
        self: &Arc<Self>,
        run_id: Uuid,
        job: Option<&JobContext>,
    ) -> Result<SyncRun, DirectoryError> {
        let run = models::Run::query()
            .where_(models::Run::ID.eq(run_id))
            .first(&*self.db)
            .await?
            .ok_or(DirectoryError::RunNotFound)?
            .into_inner();
        if RunStatus::parse(&run.status) != RunStatus::Queued {
            return Ok(run_view(&run));
        }
        let Some(connection) = models::Connection::query()
            .where_(models::Connection::ID.eq(run.connection_id))
            .first(&*self.db)
            .await?
            .map(Record::into_inner)
        else {
            return self
                .finish(&run, None, failed_outcome("the connection was deleted"))
                .await;
        };
        if ConnectionStatus::parse(&connection.status) == ConnectionStatus::Paused {
            return self
                .finish(
                    &run,
                    Some(&connection),
                    failed_outcome("the connection is paused"),
                )
                .await;
        }
        let started = self.now();
        self.update_run(run.id, |record| {
            record.status = RunStatus::Running.as_str().to_owned();
            record.started_at = Some(started);
            record.note = Some("Reading the directory".to_owned());
        })
        .await?;
        if let Some(job) = job {
            let _ = job.progress(0, 0, "Reading the directory").await;
        }

        // -- stage -----------------------------------------------------------
        let info = self.info(&connection);
        let pulled = match self.pull(&connection, &info, run.full_sync).await {
            Ok(pulled) => pulled,
            Err(error) => {
                tracing::warn!(target: "navree::directory", connection = %connection.id, %error, "a directory pull failed");
                return self
                    .finish(&run, Some(&connection), failed_outcome(&error.to_string()))
                    .await;
            }
        };

        // -- diff ------------------------------------------------------------
        let current = self.current(connection.id).await?;
        let plan = diff(&current, &pulled);
        let mut summary = plan.summary;
        let total = plan.len() as u64;
        let complete = Some(pulled.complete);
        let options = &self.options.sync;
        if !run.dry_run
            && !run.force
            && exceeds_deactivation_limit(
                &plan,
                &current,
                options.max_deactivation_percent,
                options.min_deactivations,
            )
        {
            let message = format!(
                "the run would deactivate {} of {} active users (more than {} percent); approve it to apply",
                plan.deactivations.len(),
                current.active_users(),
                options.max_deactivation_percent
            );
            self.record_planned(run.id, &steps(plan)).await?;
            return self
                .finish(
                    &run,
                    Some(&connection),
                    Outcome {
                        status: RunStatus::Held,
                        summary,
                        complete,
                        done: 0,
                        total,
                        error: Some(message),
                        cursor: None,
                    },
                )
                .await;
        }
        if run.dry_run {
            self.record_planned(run.id, &steps(plan)).await?;
            return self
                .finish(
                    &run,
                    Some(&connection),
                    Outcome {
                        status: RunStatus::Succeeded,
                        summary,
                        complete,
                        done: 0,
                        total,
                        error: None,
                        cursor: None,
                    },
                )
                .await;
        }

        // -- apply -----------------------------------------------------------
        self.update_run(run.id, |record| {
            record.total = i64::try_from(total).unwrap_or(i64::MAX);
            record.complete = complete;
            record.note = Some("Applying changes".to_owned());
        })
        .await?;
        let applier = Applier::new(self.clone(), info, Some(run.id)).await?;
        let mut done = 0_u64;
        let mut recorded = 0_usize;
        let mut cancelled = false;
        let all = steps(plan);
        for chunk in all.chunks(options.chunk_size.max(1)) {
            let failures = self
                .apply_chunk(&applier, run.id, chunk, &mut recorded)
                .await?;
            summary.failed += failures;
            done += chunk.len() as u64;
            let note = format!("Applied {done} of {total} changes");
            self.update_run(run.id, |record| {
                record.done = i64::try_from(done).unwrap_or(i64::MAX);
                record.note = Some(note.clone());
            })
            .await?;
            if let Some(job) = job
                && job.progress(done, total, note).await.is_err()
            {
                cancelled = true;
                break;
            }
        }
        let outcome = if cancelled {
            Outcome {
                status: RunStatus::Cancelled,
                summary,
                complete,
                done,
                total,
                error: Some("the run was cancelled".to_owned()),
                cursor: None,
            }
        } else if summary.failed > 0 {
            Outcome {
                status: RunStatus::Partial,
                summary,
                complete,
                done,
                total,
                error: Some(format!("{} changes failed", summary.failed)),
                cursor: None,
            }
        } else {
            Outcome {
                status: RunStatus::Succeeded,
                summary,
                complete,
                done,
                total,
                error: None,
                cursor: Some(pulled.cursor.clone()),
            }
        };
        self.finish(&run, Some(&connection), outcome).await
    }

    async fn pull(
        &self,
        connection: &models::Connection,
        info: &DirectoryConnection,
        full: bool,
    ) -> Result<Pulled, DirectoryError> {
        let source = self.source(&connection.source)?;
        let request = PullRequest {
            connection: info.clone(),
            cursor: if full {
                None
            } else {
                connection.cursor.clone()
            },
            secret: self.secret(connection)?,
            app: self.app()?,
        };
        source.pull(&request).await
    }

    /// What the directory holds for `connection`.
    pub(crate) async fn current(&self, connection: Uuid) -> Result<Current, DirectoryError> {
        let db = &*self.db;
        let users = models::User::query()
            .where_(models::User::CONNECTION_ID.eq(connection))
            .get(db)
            .await?
            .into_models();
        let groups = models::Group::query()
            .where_(models::Group::CONNECTION_ID.eq(connection))
            .get(db)
            .await?
            .into_models();
        let memberships = models::Membership::query()
            .where_(models::Membership::CONNECTION_ID.eq(connection))
            .get(db)
            .await?
            .into_models();
        let external: HashMap<Uuid, String> = users
            .iter()
            .map(|user| (user.id, user.external_id.clone()))
            .collect();
        let mut members: HashMap<Uuid, Vec<String>> = HashMap::new();
        for membership in memberships {
            if let Some(user) = external.get(&membership.user_id) {
                members
                    .entry(membership.group_id)
                    .or_default()
                    .push(user.clone());
            }
        }
        let mut current = Current::default();
        for row in users {
            let user: DirectoryUser = serde_json::from_str(&row.data).unwrap_or_default();
            current.users.insert(
                row.external_id.clone(),
                CurrentUser {
                    id: row.id,
                    user,
                    fingerprint: row.fingerprint,
                    local_id: row.local_id,
                },
            );
        }
        for row in groups {
            let mut group: DirectoryGroup = serde_json::from_str(&row.data).unwrap_or_default();
            let mut list = members.remove(&row.id).unwrap_or_default();
            list.sort();
            group.members = list;
            current.groups.insert(
                row.external_id.clone(),
                CurrentGroup {
                    id: row.id,
                    group,
                    fingerprint: row.fingerprint,
                    local_id: row.local_id,
                },
            );
        }
        Ok(current)
    }

    /// Applies a chunk in one transaction; when it fails, each step on its
    /// own. Returns how many steps failed.
    async fn apply_chunk(
        &self,
        applier: &Applier,
        run_id: Uuid,
        chunk: &[Step],
        recorded: &mut usize,
    ) -> Result<u64, DirectoryError> {
        let max = self.options.sync.max_recorded_changes;
        let uow = self.db.uow(None, None).await;
        let together = uow
            .scope(async {
                for step in chunk {
                    apply_step(applier, &uow, step).await?;
                }
                let now = self.now();
                let rows: Vec<models::RunChange> = chunk
                    .iter()
                    .take(max.saturating_sub(*recorded))
                    .map(|step| change_row(run_id, step, ChangeStatus::Applied, None, now))
                    .collect();
                if !rows.is_empty() {
                    let tx = uow.tx().await?;
                    models::RunChange::query().insert(&*tx, rows).await?;
                }
                Ok::<_, DirectoryError>(())
            })
            .await;
        match together {
            Ok(()) => {
                uow.commit().await?;
                *recorded = (*recorded + chunk.len()).min(max);
                return Ok(0);
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::directory", error = %rollback, "rollback failed");
                }
                tracing::debug!(target: "navree::directory", %error, "a chunk failed; applying its changes one by one");
            }
        }
        let mut failures = 0;
        for step in chunk {
            let uow = self.db.uow(None, None).await;
            let result = uow.scope(apply_step(applier, &uow, step)).await;
            let now = self.now();
            match result {
                Ok(()) => {
                    if *recorded < max {
                        let tx = uow.tx().await?;
                        models::RunChange::query()
                            .insert(
                                &*tx,
                                [change_row(run_id, step, ChangeStatus::Applied, None, now)],
                            )
                            .await?;
                        drop(tx);
                        *recorded += 1;
                    }
                    uow.commit().await?;
                }
                Err(error) => {
                    failures += 1;
                    if let Err(rollback) = uow.rollback().await {
                        tracing::warn!(target: "navree::directory", error = %rollback, "rollback failed");
                    }
                    tracing::warn!(target: "navree::directory", %error, "a directory change failed");
                    if *recorded < max {
                        let row = change_row(
                            run_id,
                            step,
                            ChangeStatus::Failed,
                            Some(error.to_string()),
                            now,
                        );
                        models::RunChange::query().insert(&*self.db, [row]).await?;
                        *recorded += 1;
                    }
                }
            }
        }
        Ok(failures)
    }

    /// Records a plan's steps as planned (up to the limit).
    async fn record_planned(&self, run_id: Uuid, steps: &[Step]) -> Result<(), DirectoryError> {
        let now = self.now();
        let rows: Vec<models::RunChange> = steps
            .iter()
            .take(self.options.sync.max_recorded_changes)
            .map(|step| change_row(run_id, step, ChangeStatus::Planned, None, now))
            .collect();
        for chunk in rows.chunks(500) {
            models::RunChange::query()
                .insert(&*self.db, chunk.to_vec())
                .await?;
        }
        Ok(())
    }

    async fn update_run(
        &self,
        id: Uuid,
        change: impl FnOnce(&mut models::Run),
    ) -> Result<(), DirectoryError> {
        let Some(mut record) = models::Run::query()
            .where_(models::Run::ID.eq(id))
            .first(&*self.db)
            .await?
        else {
            return Err(DirectoryError::RunNotFound);
        };
        change(&mut record);
        record.save(&*self.db).await?;
        Ok(())
    }

    /// Ends a run: its row, the connection's last run (and cursor and next
    /// sync), and the event after the commit.
    async fn finish(
        &self,
        run: &models::Run,
        connection: Option<&models::Connection>,
        outcome: Outcome,
    ) -> Result<SyncRun, DirectoryError> {
        let now = self.now();
        let run_id = run.id;
        let connection_row = connection.cloned();
        let event_owner = connection.map(|c| crate::runtime::owner_of(&c.owner_type, c.owner_id));
        let dry_run = run.dry_run;
        let summary = outcome.summary;
        let status = outcome.status;
        let error = outcome.error.clone();
        let row = self
            .housekeeping(|uow: Uow| async move {
                let tx = uow.tx().await?;
                let mut record = models::Run::query()
                    .where_(models::Run::ID.eq(run_id))
                    .first(&*tx)
                    .await?
                    .ok_or(DirectoryError::RunNotFound)?;
                record.status = outcome.status.as_str().to_owned();
                record.summary = serde_json::to_string(&outcome.summary).unwrap_or_default();
                record.complete = outcome.complete.or(record.complete);
                record.done = i64::try_from(outcome.done).unwrap_or(i64::MAX);
                record.total = i64::try_from(outcome.total).unwrap_or(i64::MAX);
                record.error = outcome.error.clone();
                record.note = None;
                record.finished_at = Some(now);
                record.save(&*tx).await?;
                if let Some(connection) = &connection_row {
                    let mut assignments = vec![
                        models::Connection::LAST_RUN_ID.set(Some(run_id)),
                        models::Connection::UPDATED_AT.set(now),
                    ];
                    if let Some(cursor) = outcome.cursor.clone() {
                        assignments.push(models::Connection::CURSOR.set(cursor));
                        assignments.push(models::Connection::LAST_SYNCED_AT.set(Some(now)));
                    }
                    if connection.sync_interval_mins > 0 && !dry_run {
                        let next =
                            now + TimeDelta::minutes(i64::from(connection.sync_interval_mins));
                        assignments.push(models::Connection::NEXT_SYNC_AT.set(Some(next)));
                    }
                    models::Connection::query()
                        .where_(models::Connection::ID.eq(connection.id))
                        .update(&*tx, assignments)
                        .await?;
                }
                uow.mark_modified();
                drop(tx);
                if let (Some(connection), Some(owner)) = (&connection_row, event_owner) {
                    self.emit_after_commit(
                        &uow,
                        DirectorySyncFinished {
                            run_id,
                            connection_id: connection.id,
                            owner,
                            status,
                            dry_run,
                            summary,
                            error,
                        },
                    )
                    .await;
                }
                Ok(record.into_inner())
            })
            .await?;
        Ok(run_view(&row))
    }
}

async fn apply_step(applier: &Applier, uow: &Uow, step: &Step) -> Result<(), DirectoryError> {
    match step {
        Step::User(step) => applier.user(uow, step, None, None).await.map(|_| ()),
        Step::Group(step) => applier.group(uow, step, None, None).await.map(|_| ()),
    }
}

fn failed_outcome(message: &str) -> Outcome {
    Outcome {
        status: RunStatus::Failed,
        summary: RunSummary::default(),
        complete: None,
        done: 0,
        total: 0,
        error: Some(message.to_owned()),
        cursor: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChangeAction;

    #[test]
    fn steps_apply_users_then_groups_then_deletions_then_deactivations() {
        let user = |id: &str, action| UserStep {
            action,
            user: DirectoryUser::new(id, id),
            previous: None,
            changed: Vec::new(),
        };
        let group = |id: &str, action| GroupStep {
            action,
            group: DirectoryGroup::new(id, id),
            previous: None,
            changed: Vec::new(),
            added: vec!["u1".into()],
            removed: Vec::new(),
        };
        let plan = Plan {
            users: vec![user("u1", ChangeAction::Create)],
            groups: vec![group("g1", ChangeAction::Create)],
            deleted_groups: vec![group("g2", ChangeAction::Delete)],
            deactivations: vec![user("u2", ChangeAction::Deactivate)],
            summary: RunSummary::default(),
        };
        let order: Vec<String> = steps(plan)
            .iter()
            .map(|step| match step {
                Step::User(step) => format!("{}:{}", step.action.as_str(), step.user.external_id),
                Step::Group(step) => format!("{}:{}", step.action.as_str(), step.group.external_id),
            })
            .collect();
        assert_eq!(
            order,
            vec!["create:u1", "create:g1", "delete:g2", "deactivate:u2"]
        );
    }

    #[test]
    fn change_rows_record_labels_fields_and_member_counts() {
        let now = Utc::now();
        let step = Step::Group(Box::new(GroupStep {
            action: ChangeAction::Update,
            group: DirectoryGroup::new("g1", "Engineering"),
            previous: None,
            changed: vec!["members".into()],
            added: vec!["u1".into(), "u2".into()],
            removed: vec!["u3".into()],
        }));
        let row = change_row(Uuid::nil(), &step, ChangeStatus::Applied, None, now);
        assert_eq!(row.subject, "group");
        assert_eq!(row.action, "update");
        assert_eq!(row.label, "Engineering");
        assert_eq!(row.fields, r#"["members"]"#);
        assert_eq!(row.members_added, 2);
        assert_eq!(row.members_removed, 1);
        let view = change_view(&row);
        assert_eq!(view.subject, ChangeSubject::Group);
        assert_eq!(view.fields, vec!["members"]);
        assert_eq!(truncate("héllo", 2), "h");
    }
}
