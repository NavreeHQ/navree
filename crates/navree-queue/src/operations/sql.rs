//! The SQL of table `operations`. Times are Unix milliseconds; ids are
//! stored as text. Every change of a pending or running operation bumps
//! `version` and `updated_at`; a finished operation never changes.

use navree_contracts::{JobEnvelope, JobProgress};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde_json::Value;
use uuid::Uuid;

use super::{Operation, OperationProgress, OperationRecord, OperationStatus};
use crate::QueueError;
use crate::migration::OPERATIONS_TABLE;
use crate::record::from_millis;
use crate::store::statement;

/// The columns read back, in [`record`]'s order.
const COLUMNS: &str = "id, kind, status, owner_id, workspace_id, progress_done, progress_total, \
     progress_note, result, error, batch_id, job, idempotent, redispatches, version, created_at, \
     started_at, heartbeat_at, finished_at, updated_at";

/// Pending or running.
const ACTIVE: &str = "status IN ('pending', 'running')";

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// A new `pending` operation.
pub(super) fn new_record(id: Uuid, operation: &Operation, now: i64) -> OperationRecord {
    OperationRecord {
        id,
        kind: operation.kind.clone(),
        status: OperationStatus::Pending,
        progress: progress_of(0, operation.total, operation.note.clone()),
        result: None,
        error: None,
        owner_id: operation.owner_id,
        workspace_id: operation.workspace_id,
        batch_id: None,
        created_at: from_millis(now),
        started_at: None,
        heartbeat_at: None,
        finished_at: None,
        updated_at: from_millis(now),
        version: 1,
        job: None,
        idempotent: operation.idempotent,
        redispatches: 0,
    }
}

fn progress_of(done: u64, total: u64, note: String) -> OperationProgress {
    let percent = JobProgress {
        done,
        total,
        note: String::new(),
    }
    .percent();
    OperationProgress {
        done,
        total,
        percent,
        note,
    }
}

pub(super) fn insert(backend: DbBackend, record: &OperationRecord) -> Result<Statement, QueueError> {
    let now = record.created_at.timestamp_millis();
    Ok(statement(
        backend,
        &format!(
            "INSERT INTO {OPERATIONS_TABLE} (id, kind, status, owner_id, workspace_id, \
             progress_done, progress_total, progress_note, idempotent, redispatches, version, \
             created_at, updated_at) VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?, 0, 1, ?, ?)"
        ),
        vec![
            record.id.to_string().into(),
            record.kind.as_str().into(),
            record.status.as_str().into(),
            record.owner_id.map(|id| id.to_string()).into(),
            record.workspace_id.map(|id| id.to_string()).into(),
            to_i64(record.progress.total).into(),
            record.progress.note.as_str().into(),
            record.idempotent.into(),
            now.into(),
            now.into(),
        ],
    ))
}

/// Keeps the job of an idempotent operation for the watchdog.
pub(super) fn set_job(
    backend: DbBackend,
    id: Uuid,
    job: &JobEnvelope,
) -> Result<Statement, QueueError> {
    let job = serde_json::to_string(job).map_err(QueueError::Payload)?;
    Ok(statement(
        backend,
        &format!("UPDATE {OPERATIONS_TABLE} SET job = ? WHERE id = ?"),
        vec![job.into(), id.to_string().into()],
    ))
}

/// Ties the operation to its batch; the total becomes the batch's jobs.
pub(super) fn set_batch(
    backend: DbBackend,
    id: Uuid,
    batch_id: &str,
    total: u64,
    now: i64,
) -> Statement {
    statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET batch_id = ?, progress_total = ?, updated_at = ?, \
             version = version + 1 WHERE id = ?"
        ),
        vec![
            batch_id.into(),
            to_i64(total).into(),
            now.into(),
            id.to_string().into(),
        ],
    )
}

/// `running` with progress, heartbeat renewed.
pub(super) fn progress(
    backend: DbBackend,
    id: Uuid,
    done: u64,
    total: u64,
    note: &str,
    now: i64,
) -> Statement {
    statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET status = 'running', \
             started_at = COALESCE(started_at, ?), progress_done = ?, progress_total = ?, \
             progress_note = ?, heartbeat_at = ?, updated_at = ?, version = version + 1 \
             WHERE id = ? AND {ACTIVE}"
        ),
        vec![
            now.into(),
            to_i64(done).into(),
            to_i64(total).into(),
            note.into(),
            now.into(),
            now.into(),
            id.to_string().into(),
        ],
    )
}

/// `running` with new counts (a batch's finished jobs).
pub(super) fn counts(backend: DbBackend, id: Uuid, done: u64, total: u64, now: i64) -> Statement {
    statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET status = 'running', \
             started_at = COALESCE(started_at, ?), progress_done = ?, progress_total = ?, \
             heartbeat_at = ?, updated_at = ?, version = version + 1 \
             WHERE id = ? AND {ACTIVE}"
        ),
        vec![
            now.into(),
            to_i64(done).into(),
            to_i64(total).into(),
            now.into(),
            now.into(),
            id.to_string().into(),
        ],
    )
}

/// `running` with a new note, counts unchanged.
pub(super) fn note(backend: DbBackend, id: Uuid, note: &str, now: i64) -> Statement {
    statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET status = 'running', \
             started_at = COALESCE(started_at, ?), progress_note = ?, heartbeat_at = ?, \
             updated_at = ?, version = version + 1 WHERE id = ? AND {ACTIVE}"
        ),
        vec![
            now.into(),
            note.into(),
            now.into(),
            now.into(),
            id.to_string().into(),
        ],
    )
}

/// `running` (its job started), heartbeat renewed.
pub(super) fn running(backend: DbBackend, id: Uuid, now: i64) -> Statement {
    statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET status = 'running', \
             started_at = COALESCE(started_at, ?), heartbeat_at = ?, updated_at = ?, \
             version = version + 1 WHERE id = ? AND status = 'pending'"
        ),
        vec![now.into(), now.into(), now.into(), id.to_string().into()],
    )
}

/// The heartbeat alone (no version bump: nothing a person sees changed).
pub(super) fn heartbeat(backend: DbBackend, id: Uuid, now: i64) -> Statement {
    statement(
        backend,
        &format!("UPDATE {OPERATIONS_TABLE} SET heartbeat_at = ? WHERE id = ? AND {ACTIVE}"),
        vec![now.into(), id.to_string().into()],
    )
}

/// An operation with no sign of life since before `before` (the watchdog's
/// cutoff).
const STALE: &str = "COALESCE(heartbeat_at, started_at, created_at) < ?";

/// Restarted by the watchdog, if it is still stale (its heartbeat is
/// renewed, so a second sweep of the same operation changes nothing).
pub(super) fn redispatched(backend: DbBackend, id: Uuid, before: i64, now: i64) -> Statement {
    statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET redispatches = redispatches + 1, heartbeat_at = ?, \
             progress_note = 'Restarted after it stopped responding', updated_at = ?, \
             version = version + 1 WHERE id = ? AND {ACTIVE} AND {STALE}"
        ),
        vec![
            now.into(),
            now.into(),
            id.to_string().into(),
            before.into(),
        ],
    )
}

/// Failed by the watchdog with `error`, if it is still stale (a heartbeat
/// that arrived during the sweep keeps it alive).
pub(super) fn stalled(backend: DbBackend, id: Uuid, error: &str, before: i64, now: i64) -> Statement {
    statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET status = 'failed', error = ?, finished_at = ?, \
             updated_at = ?, version = version + 1 WHERE id = ? AND {ACTIVE} AND {STALE}"
        ),
        vec![
            error.into(),
            now.into(),
            now.into(),
            id.to_string().into(),
            before.into(),
        ],
    )
}

/// Finishes a pending or running operation as `status`. A success fills
/// the progress up to its total and keeps a result already set when
/// `result` is `None`.
pub(super) fn finish(
    backend: DbBackend,
    id: Uuid,
    status: OperationStatus,
    result: Option<&Value>,
    error: Option<&str>,
    now: i64,
) -> Result<Statement, QueueError> {
    let fill = if status == OperationStatus::Succeeded {
        "progress_done = CASE WHEN progress_total > progress_done THEN progress_total \
         ELSE progress_done END, "
    } else {
        ""
    };
    let result = result
        .map(serde_json::to_string)
        .transpose()
        .map_err(QueueError::Payload)?;
    Ok(statement(
        backend,
        &format!(
            "UPDATE {OPERATIONS_TABLE} SET status = ?, {fill}result = COALESCE(?, result), \
             error = ?, finished_at = ?, updated_at = ?, version = version + 1 \
             WHERE id = ? AND {ACTIVE}"
        ),
        vec![
            status.as_str().into(),
            result.into(),
            error.map(str::to_owned).into(),
            now.into(),
            now.into(),
            id.to_string().into(),
        ],
    ))
}

pub(super) fn prune(backend: DbBackend, before: i64) -> Statement {
    statement(
        backend,
        &format!(
            "DELETE FROM {OPERATIONS_TABLE} WHERE finished_at IS NOT NULL AND finished_at < ?"
        ),
        vec![before.into()],
    )
}

pub(super) async fn find<C: ConnectionTrait>(
    conn: &C,
    backend: DbBackend,
    id: Uuid,
) -> Result<Option<OperationRecord>, QueueError> {
    let row = conn
        .query_one_raw(statement(
            backend,
            &format!("SELECT {COLUMNS} FROM {OPERATIONS_TABLE} WHERE id = ?"),
            vec![id.to_string().into()],
        ))
        .await?;
    row.map(|row| record(&row)).transpose()
}

pub(super) async fn status<C: ConnectionTrait>(
    conn: &C,
    backend: DbBackend,
    id: Uuid,
) -> Result<Option<OperationStatus>, QueueError> {
    let row = conn
        .query_one_raw(statement(
            backend,
            &format!("SELECT status FROM {OPERATIONS_TABLE} WHERE id = ?"),
            vec![id.to_string().into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(OperationStatus::parse(&row.try_get::<String>("", "status")?)),
        None => Ok(None),
    }
}

pub(super) async fn for_owner<C: ConnectionTrait>(
    conn: &C,
    backend: DbBackend,
    owner: Uuid,
    limit: usize,
) -> Result<Vec<OperationRecord>, QueueError> {
    let rows = conn
        .query_all_raw(statement(
            backend,
            &format!(
                "SELECT {COLUMNS} FROM {OPERATIONS_TABLE} WHERE owner_id = ? \
                 ORDER BY created_at DESC, id DESC LIMIT ?"
            ),
            vec![
                owner.to_string().into(),
                i64::try_from(limit).unwrap_or(i64::MAX).into(),
            ],
        ))
        .await?;
    rows.iter().map(record).collect()
}

/// Pending or running operations without a sign of life since `before`.
pub(super) async fn stale<C: ConnectionTrait>(
    conn: &C,
    backend: DbBackend,
    before: i64,
) -> Result<Vec<OperationRecord>, QueueError> {
    let rows = conn
        .query_all_raw(statement(
            backend,
            &format!(
                "SELECT {COLUMNS} FROM {OPERATIONS_TABLE} WHERE {ACTIVE} AND {STALE} \
                 ORDER BY created_at, id"
            ),
            vec![before.into()],
        ))
        .await?;
    rows.iter().map(record).collect()
}

fn uuid_column(row: &QueryResult, column: &str) -> Result<Option<Uuid>, QueueError> {
    let text: Option<String> = row.try_get("", column)?;
    Ok(text.and_then(|text| Uuid::parse_str(&text).ok()))
}

fn record(row: &QueryResult) -> Result<OperationRecord, QueueError> {
    let id: String = row.try_get("", "id")?;
    let status: String = row.try_get("", "status")?;
    let done: i64 = row.try_get("", "progress_done")?;
    let total: i64 = row.try_get("", "progress_total")?;
    let result: Option<String> = row.try_get("", "result")?;
    let job: Option<String> = row.try_get("", "job")?;
    let redispatches: i64 = row.try_get("", "redispatches")?;
    Ok(OperationRecord {
        id: Uuid::parse_str(&id).unwrap_or_default(),
        kind: row.try_get("", "kind")?,
        status: OperationStatus::parse(&status).unwrap_or(OperationStatus::Failed),
        progress: progress_of(
            u64::try_from(done).unwrap_or_default(),
            u64::try_from(total).unwrap_or_default(),
            row.try_get("", "progress_note")?,
        ),
        result: result
            .map(|text| serde_json::from_str(&text))
            .transpose()
            .map_err(QueueError::Payload)?,
        error: row.try_get("", "error")?,
        owner_id: uuid_column(row, "owner_id")?,
        workspace_id: uuid_column(row, "workspace_id")?,
        batch_id: row.try_get("", "batch_id")?,
        created_at: from_millis(row.try_get("", "created_at")?),
        started_at: row.try_get::<Option<i64>>("", "started_at")?.map(from_millis),
        heartbeat_at: row
            .try_get::<Option<i64>>("", "heartbeat_at")?
            .map(from_millis),
        finished_at: row
            .try_get::<Option<i64>>("", "finished_at")?
            .map(from_millis),
        updated_at: from_millis(row.try_get("", "updated_at")?),
        version: row.try_get("", "version")?,
        job: job
            .map(|text| serde_json::from_str(&text))
            .transpose()
            .map_err(QueueError::Payload)?,
        idempotent: row.try_get("", "idempotent")?,
        redispatches: u32::try_from(redispatches).unwrap_or(u32::MAX),
    })
}
