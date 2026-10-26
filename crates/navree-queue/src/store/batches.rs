//! The database driver's batches: table `job_batches`, and the batch and
//! operation columns of `jobs`.

use navree_db::Uow;
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};

use super::PrepareCallback;
use super::database::{DatabaseStore, insert_job, json, statement};
use crate::QueueError;
use crate::batch::{BatchRow, BatchStep, Finish, Finished};
use crate::migration::{FAILED_JOBS_TABLE, JOB_BATCHES_TABLE, JOBS_TABLE};
use crate::record::{NewJob, ReservedJob};

/// The columns of `job_batches`, in [`batch_row`]'s order.
const BATCH_COLUMNS: &str = "id, name, total_jobs, pending_jobs, failed_jobs, failed_job_ids, \
     callbacks, callbacks_pending, callbacks_failed, allow_failures, operation_id, created_at, \
     cancelled_at, finished_at";

impl DatabaseStore {
    /// Stores a batch with its jobs in one transaction.
    pub(crate) async fn insert_batch(
        &self,
        row: &BatchRow,
        jobs: &[NewJob],
    ) -> Result<(), QueueError> {
        let uow = self.db.uow(None, None).await;
        let result = insert_batch_on(&uow, row, jobs).await;
        finish_uow(uow, result).await
    }

    /// [`insert_batch`](DatabaseStore::insert_batch) in the transaction of
    /// `uow`.
    pub(crate) async fn insert_batch_in(
        &self,
        uow: &Uow,
        row: &BatchRow,
        jobs: &[NewJob],
    ) -> Result<(), QueueError> {
        insert_batch_on(uow, row, jobs).await
    }

    /// Finishes a reserved job of a batch in one transaction: deletes it
    /// (moving it to `failed_jobs` when it failed), updates the batch row
    /// (locked on PostgreSQL; SQLite serialises writers) and inserts the
    /// callbacks that became due. `None` when `worker` no longer holds the
    /// reservation.
    pub(crate) async fn finish_batch_job(
        &self,
        job: &ReservedJob,
        worker: &str,
        finish: Finish,
        error: &str,
        now: i64,
        prepare: &PrepareCallback<'_>,
    ) -> Result<Finished, QueueError> {
        let uow = self.db.uow(None, None).await;
        let result = async {
            let backend = uow.db().backend();
            let tx = uow.tx().await?;
            let deleted = tx
                .execute_raw(statement(
                    backend,
                    &format!("DELETE FROM {JOBS_TABLE} WHERE id = ? AND reserved_by = ?"),
                    vec![job.id.as_str().into(), worker.into()],
                ))
                .await?
                .rows_affected();
            if deleted != 1 {
                return Ok(Finished::Lost);
            }
            if finish == Finish::Failed {
                tx.execute_raw(insert_failed(backend, job, error, now)?)
                    .await?;
            }
            let Some(batch_id) = &job.membership.batch_id else {
                return Ok(Finished::Done(None));
            };
            let lock = if backend == DbBackend::Postgres {
                " FOR UPDATE"
            } else {
                ""
            };
            let Some(found) = tx
                .query_one_raw(statement(
                    backend,
                    &format!("SELECT {BATCH_COLUMNS} FROM {JOB_BATCHES_TABLE} WHERE id = ?{lock}"),
                    vec![batch_id.as_str().into()],
                ))
                .await?
            else {
                return Ok(Finished::Done(None));
            };
            let mut row = batch_row(&found)?;
            let was_settled = row.settled();
            let due = if job.membership.callback {
                row.callback_finished(finish == Finish::Failed);
                Vec::new()
            } else {
                row.job_finished(&job.id, finish, now)
            };
            for envelope in due {
                tx.execute_raw(insert_job(backend, &prepare(envelope, &row)?)?)
                    .await?;
            }
            tx.execute_raw(update_batch(backend, &row)?).await?;
            let settled_now = !was_settled && row.settled();
            Ok(Finished::Done(Some(BatchStep {
                batch: row,
                settled_now,
            })))
        }
        .await;
        finish_uow(uow, result).await
    }

    pub(crate) async fn find_batch(&self, id: &str) -> Result<Option<BatchRow>, QueueError> {
        let row = self
            .db
            .conn()
            .query_one_raw(statement(
                self.db.backend(),
                &format!("SELECT {BATCH_COLUMNS} FROM {JOB_BATCHES_TABLE} WHERE id = ?"),
                vec![id.into()],
            ))
            .await?;
        row.map(|row| batch_row(&row)).transpose()
    }

    /// Newest first, at most `limit`.
    pub(crate) async fn batches(&self, limit: usize) -> Result<Vec<BatchRow>, QueueError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = self
            .db
            .conn()
            .query_all_raw(statement(
                self.db.backend(),
                &format!(
                    "SELECT {BATCH_COLUMNS} FROM {JOB_BATCHES_TABLE} \
                     ORDER BY created_at DESC, id DESC LIMIT ?"
                ),
                vec![limit.into()],
            ))
            .await?;
        rows.iter().map(batch_row).collect()
    }

    /// Cancels batch `id` at `now` unless it finished or was cancelled; the
    /// batch afterwards.
    pub(crate) async fn cancel_batch(
        &self,
        id: &str,
        now: i64,
    ) -> Result<Option<BatchRow>, QueueError> {
        self.db
            .conn()
            .execute_raw(statement(
                self.db.backend(),
                &format!(
                    "UPDATE {JOB_BATCHES_TABLE} SET cancelled_at = ? \
                     WHERE id = ? AND cancelled_at IS NULL AND finished_at IS NULL"
                ),
                vec![now.into(), id.into()],
            ))
            .await?;
        self.find_batch(id).await
    }

    /// Deletes the batches that finished, with every callback, before
    /// `before`; returns how many.
    pub(crate) async fn prune_batches(&self, before: i64) -> Result<u64, QueueError> {
        let result = self
            .db
            .conn()
            .execute_raw(statement(
                self.db.backend(),
                &format!(
                    "DELETE FROM {JOB_BATCHES_TABLE} \
                     WHERE finished_at IS NOT NULL AND finished_at < ? AND callbacks_pending = 0"
                ),
                vec![before.into()],
            ))
            .await?;
        Ok(result.rows_affected())
    }

    /// How many stored jobs do the work of operation `operation_id`.
    pub(crate) async fn count_for_operation(&self, operation_id: &str) -> Result<u64, QueueError> {
        let row = self
            .db
            .conn()
            .query_one_raw(statement(
                self.db.backend(),
                &format!("SELECT COUNT(*) AS n FROM {JOBS_TABLE} WHERE operation_id = ?"),
                vec![operation_id.into()],
            ))
            .await?;
        let n: i64 = match row {
            Some(row) => row.try_get("", "n")?,
            None => 0,
        };
        Ok(u64::try_from(n).unwrap_or_default())
    }
}

async fn insert_batch_on(uow: &Uow, row: &BatchRow, jobs: &[NewJob]) -> Result<(), QueueError> {
    let backend = uow.db().backend();
    let tx = uow.tx().await?;
    tx.execute_raw(insert_batch(backend, row)?).await?;
    for job in jobs {
        tx.execute_raw(insert_job(backend, job)?).await?;
    }
    Ok(())
}

/// Commits `uow` when `result` is a success, rolls it back otherwise.
async fn finish_uow<T>(uow: Uow, result: Result<T, QueueError>) -> Result<T, QueueError> {
    match result {
        Ok(value) => {
            uow.commit().await?;
            Ok(value)
        }
        Err(error) => {
            if let Err(rollback) = uow.rollback().await {
                tracing::warn!(target: "navree::job", error = %rollback, "rollback failed");
            }
            Err(error)
        }
    }
}

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn to_u64(row: &QueryResult, column: &str) -> Result<u64, QueueError> {
    let n: i64 = row.try_get("", column)?;
    Ok(u64::try_from(n).unwrap_or_default())
}

fn insert_batch(backend: DbBackend, row: &BatchRow) -> Result<Statement, QueueError> {
    Ok(statement(
        backend,
        &format!(
            "INSERT INTO {JOB_BATCHES_TABLE} ({BATCH_COLUMNS}) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ),
        vec![
            row.id.as_str().into(),
            row.name.as_str().into(),
            to_i64(row.total_jobs).into(),
            to_i64(row.pending_jobs).into(),
            to_i64(row.failed_jobs).into(),
            serde_json::to_string(&row.failed_job_ids)
                .map_err(QueueError::Payload)?
                .into(),
            serde_json::to_string(&row.callbacks)
                .map_err(QueueError::Payload)?
                .into(),
            to_i64(row.callbacks_pending).into(),
            to_i64(row.callbacks_failed).into(),
            row.allow_failures.into(),
            row.operation_id.clone().into(),
            row.created_at.into(),
            row.cancelled_at.into(),
            row.finished_at.into(),
        ],
    ))
}

fn update_batch(backend: DbBackend, row: &BatchRow) -> Result<Statement, QueueError> {
    Ok(statement(
        backend,
        &format!(
            "UPDATE {JOB_BATCHES_TABLE} SET pending_jobs = ?, failed_jobs = ?, \
             failed_job_ids = ?, callbacks_pending = ?, callbacks_failed = ?, \
             cancelled_at = ?, finished_at = ? WHERE id = ?"
        ),
        vec![
            to_i64(row.pending_jobs).into(),
            to_i64(row.failed_jobs).into(),
            serde_json::to_string(&row.failed_job_ids)
                .map_err(QueueError::Payload)?
                .into(),
            to_i64(row.callbacks_pending).into(),
            to_i64(row.callbacks_failed).into(),
            row.cancelled_at.into(),
            row.finished_at.into(),
            row.id.as_str().into(),
        ],
    ))
}

/// `INSERT INTO failed_jobs …` for `job`.
fn insert_failed(
    backend: DbBackend,
    job: &ReservedJob,
    error: &str,
    now: i64,
) -> Result<Statement, QueueError> {
    let options = serde_json::to_string(&job.options).map_err(QueueError::Payload)?;
    Ok(statement(
        backend,
        &format!(
            "INSERT INTO {FAILED_JOBS_TABLE} \
             (id, queue, job_type, payload, options, error, attempts, failed_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        ),
        vec![
            job.id.as_str().into(),
            job.queue.as_str().into(),
            job.job_type.as_str().into(),
            job.payload.to_string().into(),
            options.into(),
            error.into(),
            i64::from(job.attempts).into(),
            now.into(),
        ],
    ))
}

fn batch_row(row: &QueryResult) -> Result<BatchRow, QueueError> {
    Ok(BatchRow {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        total_jobs: to_u64(row, "total_jobs")?,
        pending_jobs: to_u64(row, "pending_jobs")?,
        failed_jobs: to_u64(row, "failed_jobs")?,
        failed_job_ids: json(&row.try_get::<String>("", "failed_job_ids")?)?,
        callbacks: json(&row.try_get::<String>("", "callbacks")?)?,
        callbacks_pending: to_u64(row, "callbacks_pending")?,
        callbacks_failed: to_u64(row, "callbacks_failed")?,
        allow_failures: row.try_get("", "allow_failures")?,
        operation_id: row.try_get("", "operation_id")?,
        created_at: row.try_get("", "created_at")?,
        cancelled_at: row.try_get("", "cancelled_at")?,
        finished_at: row.try_get("", "finished_at")?,
    })
}
