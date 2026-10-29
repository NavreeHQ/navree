//! [`DatabaseWorkflowStore`]: runs in `workflow_runs` and `workflow_steps`.

use chrono::{DateTime, Utc};
use navree_db::{Db, Executor, Row, Statement, Transaction, TxOptions, Uow, Value as SqlValue};
use navree_kernel::BoxFuture;
use serde_json::Value;
use uuid::Uuid;

use super::{DueQuery, Lease, Release, StoreResult, WorkflowStore};
use crate::migration::{RUNS_TABLE, STEPS_TABLE};
use crate::{RunQuery, RunStatus, StepStatus, WorkflowRun, WorkflowStep, WorkflowsError};

/// The columns of `workflow_runs` read back, in [`run_row`]'s order.
const RUN_COLUMNS: &str = "id, workflow, version, status, input, output, error, workspace_id, \
     dedup_key, wake_at, created_at, updated_at, finished_at";

/// The columns of `workflow_steps` read back, in [`step_row`]'s order.
const STEP_COLUMNS: &str = "run_id, step_key, position, kind, status, output, error, attempts, \
     wake_at, event, filter, created_at, updated_at";

/// The statuses of a finished run, for SQL.
const FINISHED: &str = "('completed', 'failed', 'cancelled')";

/// Runs and steps in the app database (`workflows.store = "database"`, the
/// default): tables `workflow_runs` and `workflow_steps` from the
/// `workflows` migration set. Times are Unix milliseconds.
#[derive(Clone, Debug)]
pub struct DatabaseWorkflowStore {
    db: Db,
}

impl DatabaseWorkflowStore {
    /// The store on `db`.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// What statements outside a unit of work run on: the writer's pool,
    /// never the task's unit of work (a lease or a claim must not roll back
    /// with a request), or the connection itself when it has no pool.
    fn executor(&self) -> &dyn Executor {
        self.db.pool()
    }

    /// Runs `stmt` in `uow`'s transaction, or on its own.
    async fn execute_in(&self, uow: Option<&Uow>, stmt: Statement) -> StoreResult<u64> {
        let executed = match uow {
            Some(uow) => uow.execute(&stmt).await?,
            None => self.executor().execute(&stmt).await?,
        };
        Ok(executed.rows_affected)
    }

    async fn one_run(&self, sql: &str, values: Vec<SqlValue>) -> StoreResult<Option<WorkflowRun>> {
        let row = self
            .executor()
            .fetch_optional(&statement(sql, values))
            .await?;
        row.map(|row| run_row(&row)).transpose()
    }
}

/// `sql` with `?` placeholders (numbered on PostgreSQL when it runs) and
/// its values.
pub(crate) fn statement(sql: &str, values: Vec<SqlValue>) -> Statement {
    Statement::portable(sql, values)
}

/// Unix milliseconds.
pub(crate) fn millis(at: DateTime<Utc>) -> i64 {
    at.timestamp_millis()
}

/// The time at Unix milliseconds `ms`.
pub(crate) fn from_millis(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

fn opt_millis(at: Option<DateTime<Utc>>) -> SqlValue {
    at.map(millis).into()
}

fn text(value: &Value) -> String {
    value.to_string()
}

fn opt_text(value: Option<&Value>) -> SqlValue {
    value.map(text).into()
}

fn opt_uuid(id: Option<Uuid>) -> SqlValue {
    id.map(|id| id.to_string()).into()
}

fn json(column: &str, text: &str) -> StoreResult<Value> {
    serde_json::from_str(text).map_err(|error| {
        WorkflowsError::payload(&format!("stored `{column}` is not valid JSON"), error)
    })
}

fn opt_json(row: &Row, column: &str) -> StoreResult<Option<Value>> {
    row.get::<Option<String>>(column)?
        .map(|text| json(column, &text))
        .transpose()
}

fn uuid(text: &str) -> StoreResult<Uuid> {
    Uuid::parse_str(text)
        .map_err(|_| WorkflowsError::Other(format!("stored id `{text}` is not a UUID")))
}

fn opt_uuid_column(row: &Row, column: &str) -> StoreResult<Option<Uuid>> {
    row.get::<Option<String>>(column)?
        .map(|text| uuid(&text))
        .transpose()
}

fn number(row: &Row, column: &str) -> StoreResult<u32> {
    let value: i64 = row.get(column)?;
    Ok(u32::try_from(value).unwrap_or(u32::MAX))
}

fn opt_time(row: &Row, column: &str) -> StoreResult<Option<DateTime<Utc>>> {
    Ok(row.get::<Option<i64>>(column)?.map(from_millis))
}

fn time(row: &Row, column: &str) -> StoreResult<DateTime<Utc>> {
    Ok(from_millis(row.get(column)?))
}

pub(crate) fn run_row(row: &Row) -> StoreResult<WorkflowRun> {
    let status: String = row.get("status")?;
    Ok(WorkflowRun {
        id: uuid(&row.get::<String>("id")?)?,
        workflow: row.get("workflow")?,
        version: number(row, "version")?,
        status: status.parse().map_err(WorkflowsError::Other)?,
        input: json("input", &row.get::<String>("input")?)?,
        output: opt_json(row, "output")?,
        error: row.get("error")?,
        workspace_id: opt_uuid_column(row, "workspace_id")?,
        key: row.get("dedup_key")?,
        wake_at: opt_time(row, "wake_at")?,
        created_at: time(row, "created_at")?,
        updated_at: time(row, "updated_at")?,
        finished_at: opt_time(row, "finished_at")?,
    })
}

fn step_row(row: &Row) -> StoreResult<WorkflowStep> {
    let kind: String = row.get("kind")?;
    let status: String = row.get("status")?;
    Ok(WorkflowStep {
        run_id: uuid(&row.get::<String>("run_id")?)?,
        key: row.get("step_key")?,
        position: number(row, "position")?,
        kind: kind.parse().map_err(WorkflowsError::Other)?,
        status: status.parse().map_err(WorkflowsError::Other)?,
        output: opt_json(row, "output")?,
        error: row.get("error")?,
        attempts: number(row, "attempts")?,
        wake_at: opt_time(row, "wake_at")?,
        event: row.get("event")?,
        filter: opt_json(row, "filter")?,
        created_at: time(row, "created_at")?,
        updated_at: time(row, "updated_at")?,
    })
}

/// The `WHERE` clause and values of `query`.
fn run_filter(query: &RunQuery) -> (String, Vec<SqlValue>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(workflow) = &query.workflow {
        clauses.push("workflow = ?".to_owned());
        values.push(workflow.clone().into());
    }
    if !query.statuses.is_empty() {
        let marks = vec!["?"; query.statuses.len()].join(", ");
        clauses.push(format!("status IN ({marks})"));
        values.extend(query.statuses.iter().map(|s| s.as_str().into()));
    }
    if let Some(workspace) = query.workspace_id {
        clauses.push("workspace_id = ?".to_owned());
        values.push(workspace.to_string().into());
    }
    if !query.ids.is_empty() {
        let marks = vec!["?"; query.ids.len()].join(", ");
        clauses.push(format!("id IN ({marks})"));
        values.extend(query.ids.iter().map(|id| id.to_string().into()));
    }
    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (clause, values)
}

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Commits `tx` when `result` is a success, rolls it back otherwise.
async fn finish<T>(tx: Transaction, result: StoreResult<T>) -> StoreResult<T> {
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(error) => {
            if let Err(rollback) = tx.rollback().await {
                tracing::warn!(target: "navree::workflows", error = %rollback, "rollback failed");
            }
            Err(error)
        }
    }
}

/// Renews the lease and writes `step` (a completed step is never rewritten);
/// `false` when the lease is no longer held.
async fn save_step_in(tx: &Transaction, lease: &Lease, step: &WorkflowStep) -> StoreResult<bool> {
    let held = tx
        .execute(&statement(
            &format!(
                "UPDATE {RUNS_TABLE} SET lease_until = ?, updated_at = ? \
                 WHERE id = ? AND status = 'running' AND lease_token = ?"
            ),
            vec![
                millis(lease.until).into(),
                millis(lease.now).into(),
                lease.run_id.to_string().into(),
                lease.token.to_string().into(),
            ],
        ))
        .await?
        .rows_affected;
    if held == 0 {
        return Ok(false);
    }
    let existing = tx
        .fetch_optional(&statement(
            &format!("SELECT status FROM {STEPS_TABLE} WHERE run_id = ? AND step_key = ?"),
            vec![step.run_id.to_string().into(), step.key.clone().into()],
        ))
        .await?;
    let output = opt_text(step.output.as_ref());
    let filter = opt_text(step.filter.as_ref());
    match existing {
        Some(row) => {
            let status: String = row.get("status")?;
            if status != StepStatus::Completed.as_str() {
                tx.execute(&statement(
                    &format!(
                        "UPDATE {STEPS_TABLE} SET kind = ?, status = ?, \
                         output = ?, error = ?, attempts = ?, wake_at = ?, \
                         event = ?, filter = ?, updated_at = ? \
                         WHERE run_id = ? AND step_key = ?"
                    ),
                    vec![
                        step.kind.as_str().into(),
                        step.status.as_str().into(),
                        output,
                        step.error.clone().into(),
                        i64::from(step.attempts).into(),
                        opt_millis(step.wake_at),
                        step.event.clone().into(),
                        filter,
                        millis(step.updated_at).into(),
                        step.run_id.to_string().into(),
                        step.key.clone().into(),
                    ],
                ))
                .await?;
            }
        }
        None => {
            tx.execute(&statement(
                &format!(
                    "INSERT INTO {STEPS_TABLE} ({STEP_COLUMNS}) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                ),
                vec![
                    step.run_id.to_string().into(),
                    step.key.clone().into(),
                    i64::from(step.position).into(),
                    step.kind.as_str().into(),
                    step.status.as_str().into(),
                    output,
                    step.error.clone().into(),
                    i64::from(step.attempts).into(),
                    opt_millis(step.wake_at),
                    step.event.clone().into(),
                    filter,
                    millis(step.created_at).into(),
                    millis(step.updated_at).into(),
                ],
            ))
            .await?;
        }
    }
    Ok(true)
}

/// Ends the lease with `sets` (see [`WorkflowStore::release`]); the run's
/// new status, or `None` when the lease is no longer held.
async fn release_in(
    tx: &Transaction,
    sets: &str,
    values: Vec<SqlValue>,
    run_id: &str,
) -> StoreResult<Option<String>> {
    let updated = tx
        .execute(&statement(
            &format!(
                "UPDATE {RUNS_TABLE} SET {sets}, lease_token = NULL, \
                 lease_until = NULL, updated_at = ? \
                 WHERE id = ? AND status = 'running' AND lease_token = ?"
            ),
            values,
        ))
        .await?
        .rows_affected;
    if updated == 0 {
        return Ok(None);
    }
    // The wake flag is cleared in a second statement: the CASEs above read
    // it.
    tx.execute(&statement(
        &format!("UPDATE {RUNS_TABLE} SET wake_requested = ? WHERE id = ?"),
        vec![false.into(), run_id.into()],
    ))
    .await?;
    let row = tx
        .fetch_optional(&statement(
            &format!("SELECT status FROM {RUNS_TABLE} WHERE id = ?"),
            vec![run_id.into()],
        ))
        .await?;
    Ok(match row {
        Some(row) => Some(row.get::<String>("status")?),
        None => None,
    })
}

impl WorkflowStore for DatabaseWorkflowStore {
    fn insert_run<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        run: &'a WorkflowRun,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let stmt = statement(
                &format!(
                    "INSERT INTO {RUNS_TABLE} (id, workflow, version, status, input, output, \
                     error, workspace_id, dedup_key, wake_at, wake_requested, created_at, \
                     updated_at, finished_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                ),
                vec![
                    run.id.to_string().into(),
                    run.workflow.clone().into(),
                    i64::from(run.version).into(),
                    run.status.as_str().into(),
                    text(&run.input).into(),
                    opt_text(run.output.as_ref()),
                    run.error.clone().into(),
                    opt_uuid(run.workspace_id),
                    run.key.clone().into(),
                    opt_millis(run.wake_at),
                    false.into(),
                    millis(run.created_at).into(),
                    millis(run.updated_at).into(),
                    opt_millis(run.finished_at),
                ],
            );
            self.execute_in(uow, stmt).await?;
            Ok(())
        })
    }

    fn run(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<WorkflowRun>>> {
        Box::pin(async move {
            self.one_run(
                &format!("SELECT {RUN_COLUMNS} FROM {RUNS_TABLE} WHERE id = ?"),
                vec![id.to_string().into()],
            )
            .await
        })
    }

    fn unfinished_with_key<'a>(
        &'a self,
        workflow: &'a str,
        key: &'a str,
    ) -> BoxFuture<'a, StoreResult<Option<WorkflowRun>>> {
        Box::pin(async move {
            self.one_run(
                &format!(
                    "SELECT {RUN_COLUMNS} FROM {RUNS_TABLE} WHERE workflow = ? AND dedup_key = ? \
                     AND status NOT IN {FINISHED} ORDER BY created_at DESC LIMIT 1"
                ),
                vec![workflow.into(), key.into()],
            )
            .await
        })
    }

    fn runs<'a>(&'a self, query: &'a RunQuery) -> BoxFuture<'a, StoreResult<Vec<WorkflowRun>>> {
        Box::pin(async move {
            let (clause, mut values) = run_filter(query);
            values.push(to_i64(query.effective_limit()).into());
            values.push(to_i64(query.offset).into());
            let rows = self
                .executor()
                .fetch_all(&statement(
                    &format!(
                        "SELECT {RUN_COLUMNS} FROM {RUNS_TABLE}{clause} \
                         ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
                    ),
                    values,
                ))
                .await?;
            rows.iter().map(run_row).collect()
        })
    }

    fn count_runs<'a>(&'a self, query: &'a RunQuery) -> BoxFuture<'a, StoreResult<u64>> {
        Box::pin(async move {
            let (clause, values) = run_filter(query);
            let row = self
                .executor()
                .fetch_optional(&statement(
                    &format!("SELECT COUNT(*) AS total FROM {RUNS_TABLE}{clause}"),
                    values,
                ))
                .await?;
            let total: i64 = match row {
                Some(row) => row.get("total")?,
                None => 0,
            };
            Ok(u64::try_from(total).unwrap_or_default())
        })
    }

    fn steps(&self, run_id: Uuid) -> BoxFuture<'_, StoreResult<Vec<WorkflowStep>>> {
        Box::pin(async move {
            let rows = self
                .executor()
                .fetch_all(&statement(
                    &format!(
                        "SELECT {STEP_COLUMNS} FROM {STEPS_TABLE} WHERE run_id = ? \
                         ORDER BY position, step_key"
                    ),
                    vec![run_id.to_string().into()],
                ))
                .await?;
            rows.iter().map(step_row).collect()
        })
    }

    fn claim(&self, lease: Lease) -> BoxFuture<'_, StoreResult<Option<WorkflowRun>>> {
        Box::pin(async move {
            let now = millis(lease.now);
            let claimed = self
                .executor()
                .execute(&statement(
                    &format!(
                        "UPDATE {RUNS_TABLE} SET lease_token = ?, lease_until = ?, \
                         wake_requested = ?, updated_at = ?, status = 'running' \
                         WHERE id = ? AND (status = 'pending' \
                         OR (status IN ('sleeping', 'waiting') AND wake_at IS NOT NULL \
                             AND wake_at <= ?) \
                         OR (status = 'running' AND (lease_until IS NULL OR lease_until < ?)))"
                    ),
                    vec![
                        lease.token.to_string().into(),
                        millis(lease.until).into(),
                        false.into(),
                        now.into(),
                        lease.run_id.to_string().into(),
                        now.into(),
                        now.into(),
                    ],
                ))
                .await?
                .rows_affected;
            if claimed == 0 {
                return Ok(None);
            }
            self.one_run(
                &format!(
                    "SELECT {RUN_COLUMNS} FROM {RUNS_TABLE} WHERE id = ? AND lease_token = ? \
                     AND status = 'running'"
                ),
                vec![
                    lease.run_id.to_string().into(),
                    lease.token.to_string().into(),
                ],
            )
            .await
        })
    }

    fn save_step<'a>(
        &'a self,
        lease: &'a Lease,
        step: &'a WorkflowStep,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move {
            let tx = self.db.begin_with(TxOptions::new().immediate()).await?;
            let saved = save_step_in(&tx, lease, step).await;
            finish(tx, saved).await
        })
    }

    fn release<'a>(
        &'a self,
        lease: &'a Lease,
        outcome: &'a Release,
    ) -> BoxFuture<'a, StoreResult<Option<RunStatus>>> {
        Box::pin(async move {
            let now = millis(lease.now);
            // MySQL evaluates the assignments left to right: `status` comes
            // last, so the CASEs see the old row.
            let (sets, mut values): (String, Vec<SqlValue>) = match outcome {
                Release::Completed(output) => (
                    "output = ?, error = NULL, finished_at = ?, wake_at = NULL, \
                     status = 'completed'"
                        .to_owned(),
                    vec![text(output).into(), now.into()],
                ),
                Release::Failed(error) => (
                    "error = ?, finished_at = ?, wake_at = NULL, status = 'failed'".to_owned(),
                    vec![error.clone().into(), now.into()],
                ),
                Release::Pending => ("wake_at = NULL, status = 'pending'".to_owned(), Vec::new()),
                Release::Sleeping(at) => (
                    "wake_at = CASE WHEN wake_requested THEN NULL ELSE ? END, \
                     status = CASE WHEN wake_requested THEN 'pending' ELSE 'sleeping' END"
                        .to_owned(),
                    vec![millis(*at).into()],
                ),
                Release::Waiting(at) => (
                    "wake_at = CASE WHEN wake_requested THEN NULL ELSE ? END, \
                     status = CASE WHEN wake_requested THEN 'pending' ELSE 'waiting' END"
                        .to_owned(),
                    vec![opt_millis(*at)],
                ),
            };
            values.extend([
                now.into(),
                lease.run_id.to_string().into(),
                lease.token.to_string().into(),
            ]);
            let run_id = lease.run_id.to_string();
            let tx = self.db.begin_with(TxOptions::new().immediate()).await?;
            let status = release_in(&tx, &sets, values, &run_id).await;
            let status = finish(tx, status).await?;
            status
                .map(|status| status.parse().map_err(WorkflowsError::Other))
                .transpose()
        })
    }

    fn due(&self, query: DueQuery) -> BoxFuture<'_, StoreResult<Vec<Uuid>>> {
        Box::pin(async move {
            let rows = self
                .executor()
                .fetch_all(&statement(
                    &format!(
                        "SELECT id, status, CASE \
                           WHEN status = 'running' THEN lease_until \
                           WHEN status = 'pending' THEN updated_at \
                           ELSE wake_at END AS due_at \
                         FROM {RUNS_TABLE} \
                         WHERE (status IN ('sleeping', 'waiting') AND wake_at IS NOT NULL \
                                AND wake_at <= ?) \
                            OR (status = 'running' AND lease_until IS NOT NULL \
                                AND lease_until < ?) \
                            OR (status = 'pending' AND updated_at <= ?) \
                         ORDER BY due_at, id LIMIT ?"
                    ),
                    vec![
                        millis(query.wake_before).into(),
                        millis(query.now).into(),
                        millis(query.stale_before).into(),
                        to_i64(query.limit).into(),
                    ],
                ))
                .await?;
            let mut ids = Vec::with_capacity(rows.len());
            let mut stale = Vec::new();
            for row in &rows {
                let id = uuid(&row.get::<String>("id")?)?;
                if row.get::<String>("status")? == RunStatus::Pending.as_str() {
                    stale.push(id);
                }
                ids.push(id);
            }
            for id in stale {
                self.executor()
                    .execute(&statement(
                        &format!(
                            "UPDATE {RUNS_TABLE} SET updated_at = ? \
                             WHERE id = ? AND status = 'pending'"
                        ),
                        vec![millis(query.now).into(), id.to_string().into()],
                    ))
                    .await?;
            }
            Ok(ids)
        })
    }

    fn open_waits<'a>(&'a self, event: &'a str) -> BoxFuture<'a, StoreResult<Vec<WorkflowStep>>> {
        Box::pin(async move {
            let columns = STEP_COLUMNS
                .split(", ")
                .map(|column| format!("s.{column}"))
                .collect::<Vec<_>>()
                .join(", ");
            let rows = self
                .executor()
                .fetch_all(&statement(
                    &format!(
                        "SELECT {columns} FROM {STEPS_TABLE} s \
                         JOIN {RUNS_TABLE} r ON r.id = s.run_id \
                         WHERE s.event = ? AND s.kind = 'wait' AND s.status = 'waiting' \
                         AND r.status NOT IN {FINISHED} \
                         ORDER BY s.created_at, s.run_id"
                    ),
                    vec![event.into()],
                ))
                .await?;
            rows.iter().map(step_row).collect()
        })
    }

    fn resolve_wait<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        run_id: Uuid,
        key: &'a str,
        output: &'a Value,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move {
            let now = millis(now);
            let resolved = self
                .execute_in(
                    uow,
                    statement(
                        &format!(
                            "UPDATE {STEPS_TABLE} SET output = ?, updated_at = ?, \
                             status = 'completed' \
                             WHERE run_id = ? AND step_key = ? AND status = 'waiting' \
                             AND run_id IN (SELECT id FROM {RUNS_TABLE} \
                                            WHERE status NOT IN {FINISHED})"
                        ),
                        vec![
                            text(output).into(),
                            now.into(),
                            run_id.to_string().into(),
                            key.into(),
                        ],
                    ),
                )
                .await?;
            if resolved == 0 {
                return Ok(false);
            }
            // `status` last: MySQL evaluates the assignments left to right.
            self.execute_in(
                uow,
                statement(
                    &format!(
                        "UPDATE {RUNS_TABLE} SET \
                         wake_requested = CASE WHEN status = 'running' THEN ? \
                                               ELSE wake_requested END, \
                         wake_at = CASE WHEN status IN ('sleeping', 'waiting') THEN NULL \
                                        ELSE wake_at END, \
                         updated_at = ?, \
                         status = CASE WHEN status IN ('sleeping', 'waiting') THEN 'pending' \
                                       ELSE status END \
                         WHERE id = ? AND status NOT IN {FINISHED}"
                    ),
                    vec![true.into(), now.into(), run_id.to_string().into()],
                ),
            )
            .await?;
            Ok(true)
        })
    }

    fn cancel<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move {
            let now = millis(now);
            let cancelled = self
                .execute_in(
                    uow,
                    statement(
                        &format!(
                            "UPDATE {RUNS_TABLE} SET finished_at = ?, updated_at = ?, \
                             wake_at = NULL, lease_token = NULL, lease_until = NULL, \
                             status = 'cancelled' WHERE id = ? AND status NOT IN {FINISHED}"
                        ),
                        vec![now.into(), now.into(), id.to_string().into()],
                    ),
                )
                .await?;
            Ok(cancelled == 1)
        })
    }

    fn retry<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move {
            let retried = self
                .execute_in(
                    uow,
                    statement(
                        &format!(
                            "UPDATE {RUNS_TABLE} SET error = NULL, finished_at = NULL, \
                             updated_at = ?, status = 'pending' \
                             WHERE id = ? AND status = 'failed'"
                        ),
                        vec![millis(now).into(), id.to_string().into()],
                    ),
                )
                .await?;
            if retried == 0 {
                return Ok(false);
            }
            self.execute_in(
                uow,
                statement(
                    &format!("DELETE FROM {STEPS_TABLE} WHERE run_id = ? AND status = 'failed'"),
                    vec![id.to_string().into()],
                ),
            )
            .await?;
            Ok(true)
        })
    }

    fn prune(&self, before: DateTime<Utc>) -> BoxFuture<'_, StoreResult<u64>> {
        Box::pin(async move {
            let before = millis(before);
            self.executor()
                .execute(&statement(
                    &format!(
                        "DELETE FROM {STEPS_TABLE} WHERE run_id IN (SELECT id FROM {RUNS_TABLE} \
                         WHERE finished_at IS NOT NULL AND finished_at < ?)"
                    ),
                    vec![before.into()],
                ))
                .await?;
            let pruned = self
                .executor()
                .execute(&statement(
                    &format!(
                        "DELETE FROM {RUNS_TABLE} WHERE finished_at IS NOT NULL \
                         AND finished_at < ?"
                    ),
                    vec![before.into()],
                ))
                .await?
                .rows_affected;
            Ok(pruned)
        })
    }
}
