//! The database driver: tables `jobs` and `failed_jobs`.
//!
//! Jobs are reserved, released and completed with hand-written statements
//! (a reservation is one atomic `UPDATE … RETURNING` that skips locked
//! rows); failed jobs are [`FailedJob`] models, written and read through
//! navree-orm.

use navree_db::{Db, Uow};
use navree_orm::{Model as _, Record};
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, QueryResult, Statement, Value,
};

use crate::migration::JOBS_TABLE;
use crate::record::{
    FailedJob, Membership, NewJob, PendingJob, PendingQuery, PendingState, QueueStatus,
    ReservedJob, RetryTarget, UNLIMITED_RESERVATION_SECS, from_millis,
};
use crate::{QueueError, STALE_RESERVATION_GRACE_SECS};

/// Jobs in the app database.
#[derive(Clone, Debug)]
pub(crate) struct DatabaseStore {
    pub(super) db: Db,
}

impl DatabaseStore {
    pub(crate) fn new(db: Db) -> Self {
        Self { db }
    }

    fn stmt(&self, sql: &str, values: Vec<Value>) -> Statement {
        statement(self.db.backend(), sql, values)
    }

    /// Stores `job`; returns its id, or the id of the pending unique job
    /// that takes its place.
    pub(crate) async fn insert(&self, job: &NewJob) -> Result<String, QueueError> {
        insert_on(self.db.conn(), self.db.backend(), job).await
    }

    /// [`insert`](DatabaseStore::insert) in the transaction of `uow`.
    pub(crate) async fn insert_in(&self, uow: &Uow, job: &NewJob) -> Result<String, QueueError> {
        let tx = uow.tx().await?;
        insert_on(&*tx, uow.db().backend(), job).await
    }

    /// [`insert`](DatabaseStore::insert) through `tx`, the transaction a
    /// change observer is given while its unit of work commits.
    pub(crate) async fn insert_on_tx(
        &self,
        tx: &DatabaseTransaction,
        job: &NewJob,
    ) -> Result<String, QueueError> {
        insert_on(tx, self.db.backend(), job).await
    }

    /// Atomically reserves the next job of `queue` for `worker`: an
    /// unreserved job whose time has come, or one whose reservation is
    /// stale (see [`stale_after_ms`](crate::record::stale_after_ms)). One
    /// `UPDATE … WHERE id = (SELECT … LIMIT 1) RETURNING`; PostgreSQL adds
    /// `FOR UPDATE SKIP LOCKED` to the subquery.
    pub(crate) async fn reserve(
        &self,
        queue: &str,
        worker: &str,
        now: i64,
    ) -> Result<Option<ReservedJob>, QueueError> {
        let row = self
            .db
            .conn()
            .query_one_raw(reserve_statement(self.db.backend(), queue, worker, now))
            .await?;
        row.map(|row| reserved_job(&row)).transpose()
    }

    pub(crate) async fn complete(&self, id: &str) -> Result<(), QueueError> {
        self.db
            .conn()
            .execute_raw(self.stmt(
                &format!("DELETE FROM {JOBS_TABLE} WHERE id = ?"),
                vec![id.into()],
            ))
            .await?;
        Ok(())
    }

    /// Unreserves job `id`, available again at `available_at`. Returns
    /// false when `worker` no longer holds the reservation.
    pub(crate) async fn release(
        &self,
        id: &str,
        worker: &str,
        available_at: i64,
    ) -> Result<bool, QueueError> {
        let result = self
            .db
            .conn()
            .execute_raw(self.stmt(
                &format!(
                    "UPDATE {JOBS_TABLE} SET reserved_at = NULL, reserved_by = NULL, available_at = ? \
                     WHERE id = ? AND reserved_by = ?"
                ),
                vec![available_at.into(), id.into(), worker.into()],
            ))
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Moves a reserved job to `failed_jobs` (in one transaction). Returns
    /// false when `worker` no longer holds the reservation.
    pub(crate) async fn fail(
        &self,
        job: &ReservedJob,
        worker: &str,
        error: &str,
        now: i64,
    ) -> Result<bool, QueueError> {
        let backend = self.db.backend();
        let failed = FailedJob::of(job, error, now);
        let moved = self
            .db
            .transaction(|uow| async move {
                let tx = uow.tx().await?;
                let deleted = tx
                    .execute_raw(statement(
                        backend,
                        &format!("DELETE FROM {JOBS_TABLE} WHERE id = ? AND reserved_by = ?"),
                        vec![job.id.as_str().into(), worker.into()],
                    ))
                    .await?
                    .rows_affected();
                if deleted == 1 {
                    FailedJob::insert(&*tx, [failed]).await?;
                }
                Ok(deleted == 1)
            })
            .await?;
        Ok(moved)
    }

    /// Every failed job, newest first.
    pub(crate) async fn failed(&self) -> Result<Vec<FailedJob>, QueueError> {
        Ok(FailedJob::query()
            .order_by_desc(FailedJob::FAILED_AT)
            .order_by_desc(FailedJob::ID)
            .get(self.db.conn())
            .await?
            .into_models())
    }

    /// Moves the targeted failed jobs back to `jobs`, oldest first (one
    /// transaction); returns how many.
    pub(crate) async fn retry(&self, target: &RetryTarget, now: i64) -> Result<u64, QueueError> {
        let backend = self.db.backend();
        let mut targeted = FailedJob::query()
            .order_by(FailedJob::FAILED_AT)
            .order_by(FailedJob::ID);
        if let RetryTarget::Id(id) = target {
            targeted = targeted.where_key(id.clone());
        }

        let uow = self.db.uow(None, None).await;
        let result = async {
            let tx = uow.tx().await?;
            let failed = targeted.get(&*tx).await?.into_models();
            for job in &failed {
                tx.execute_raw(insert_job(backend, &job.requeued(now))?)
                    .await?;
                FailedJob::query()
                    .where_key(job.id.clone())
                    .delete(&*tx)
                    .await?;
            }
            Ok::<_, QueueError>(failed.len() as u64)
        }
        .await;
        match result {
            Ok(retried) => {
                uow.commit().await?;
                Ok(retried)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::job", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    /// Deletes every failed job; returns how many.
    pub(crate) async fn flush_failed(&self) -> Result<u64, QueueError> {
        Ok(FailedJob::query().delete(self.db.conn()).await?)
    }

    pub(crate) async fn pending_count(&self, queue: &str) -> Result<u64, QueueError> {
        let row = self
            .db
            .conn()
            .query_one_raw(self.stmt(
                &format!("SELECT COUNT(*) AS n FROM {JOBS_TABLE} WHERE queue = ?"),
                vec![queue.into()],
            ))
            .await?;
        let n: i64 = match row {
            Some(row) => row.try_get("", "n")?,
            None => 0,
        };
        Ok(u64::try_from(n).unwrap_or_default())
    }

    /// Waiting, delayed and running counts of `queue` at `now`, and the
    /// oldest waiting job's availability: one aggregate query.
    pub(crate) async fn status(&self, queue: &str, now: i64) -> Result<QueueStatus, QueueError> {
        let row = self
            .db
            .conn()
            .query_one_raw(self.stmt(
                &format!(
                    "SELECT \
                     COALESCE(SUM(CASE WHEN reserved_at IS NULL AND available_at <= ? THEN 1 ELSE 0 END), 0) AS waiting, \
                     COALESCE(SUM(CASE WHEN reserved_at IS NULL AND available_at > ? THEN 1 ELSE 0 END), 0) AS delayed, \
                     COALESCE(SUM(CASE WHEN reserved_at IS NOT NULL THEN 1 ELSE 0 END), 0) AS running, \
                     MIN(CASE WHEN reserved_at IS NULL AND available_at <= ? THEN available_at END) AS oldest \
                     FROM {JOBS_TABLE} WHERE queue = ?"
                ),
                vec![now.into(), now.into(), now.into(), queue.into()],
            ))
            .await?;
        let mut status = QueueStatus {
            queue: queue.to_owned(),
            waiting: 0,
            delayed: 0,
            running: 0,
            oldest_available_at: None,
            at: from_millis(now),
        };
        if let Some(row) = row {
            let number = |column: &str| -> Result<u64, DbErr> {
                let n: i64 = row.try_get("", column)?;
                Ok(u64::try_from(n).unwrap_or_default())
            };
            status.waiting = number("waiting")?;
            status.delayed = number("delayed")?;
            status.running = number("running")?;
            let oldest: Option<i64> = row.try_get("", "oldest")?;
            status.oldest_available_at = oldest.map(from_millis);
        }
        Ok(status)
    }

    /// Unfinished jobs matching `query`, oldest first.
    pub(crate) async fn pending_jobs(
        &self,
        query: &PendingQuery,
        now: i64,
    ) -> Result<Vec<PendingJob>, QueueError> {
        let mut sql = format!("SELECT {PENDING_COLUMNS} FROM {JOBS_TABLE} WHERE 1 = 1");
        let mut values: Vec<Value> = Vec::new();
        if let Some(queue) = &query.queue {
            sql.push_str(" AND queue = ?");
            values.push(queue.as_str().into());
        }
        if let Some(needle) = query.needle() {
            let pattern = format!("%{}%", escape_like(&needle));
            sql.push_str(
                " AND (LOWER(job_type) LIKE ? ESCAPE '\\' OR LOWER(queue) LIKE ? ESCAPE '\\')",
            );
            values.push(pattern.clone().into());
            values.push(pattern.into());
        }
        sql.push_str(" ORDER BY available_at, id LIMIT ?");
        values.push(i64::try_from(query.limit()).unwrap_or(i64::MAX).into());
        let rows = self
            .db
            .conn()
            .query_all_raw(self.stmt(&sql, values))
            .await?;
        rows.iter().map(|row| pending_job(row, now)).collect()
    }

    pub(crate) async fn pending_job(
        &self,
        id: &str,
        now: i64,
    ) -> Result<Option<PendingJob>, QueueError> {
        let row = self
            .db
            .conn()
            .query_one_raw(self.stmt(
                &format!("SELECT {PENDING_COLUMNS} FROM {JOBS_TABLE} WHERE id = ?"),
                vec![id.into()],
            ))
            .await?;
        row.map(|row| pending_job(&row, now)).transpose()
    }

    /// Failed job `id`, if there is one.
    pub(crate) async fn failed_job(&self, id: &str) -> Result<Option<FailedJob>, QueueError> {
        Ok(FailedJob::find(self.db.conn(), id.to_owned())
            .await?
            .map(Record::into_inner))
    }

    /// Deletes failed job `id`; false when there is none.
    pub(crate) async fn forget(&self, id: &str) -> Result<bool, QueueError> {
        let deleted = FailedJob::query()
            .where_key(id.to_owned())
            .delete(self.db.conn())
            .await?;
        Ok(deleted == 1)
    }
}

/// The columns [`pending_job`] reads.
const PENDING_COLUMNS: &str =
    "id, queue, job_type, payload, options, attempts, available_at, reserved_at, created_at";

/// `text` with the `LIKE` wildcards (and the escape character) escaped
/// with `\`.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

fn pending_job(row: &QueryResult, now: i64) -> Result<PendingJob, QueueError> {
    let available_at: i64 = row.try_get("", "available_at")?;
    let reserved_at: Option<i64> = row.try_get("", "reserved_at")?;
    let state = if reserved_at.is_some() {
        PendingState::Running
    } else if available_at <= now {
        PendingState::Waiting
    } else {
        PendingState::Delayed
    };
    Ok(PendingJob {
        id: row.try_get("", "id")?,
        queue: row.try_get("", "queue")?,
        job_type: row.try_get("", "job_type")?,
        payload: json(&row.try_get::<String>("", "payload")?)?,
        options: json(&row.try_get::<String>("", "options")?)?,
        attempts: count(row, "attempts")?,
        state,
        available_at: from_millis(available_at),
        reserved_at: reserved_at.map(from_millis),
        created_at: from_millis(row.try_get("", "created_at")?),
    })
}

/// A statement for `backend`: `?` placeholders become `$1, $2, …` on
/// PostgreSQL. (No SQL here contains a literal `?`.)
pub(crate) fn statement(backend: DbBackend, sql: &str, values: Vec<Value>) -> Statement {
    let sql = if backend == DbBackend::Postgres {
        let mut numbered = String::with_capacity(sql.len() + 8);
        let mut n = 0;
        for c in sql.chars() {
            if c == '?' {
                n += 1;
                numbered.push('$');
                numbered.push_str(&n.to_string());
            } else {
                numbered.push(c);
            }
        }
        numbered
    } else {
        sql.to_owned()
    };
    Statement::from_sql_and_values(backend, sql, values)
}

/// The reservation statement (see [`DatabaseStore::reserve`]).
fn reserve_statement(backend: DbBackend, queue: &str, worker: &str, now: i64) -> Statement {
    let lock = if backend == DbBackend::Postgres {
        " FOR UPDATE SKIP LOCKED"
    } else {
        ""
    };
    let sql = format!(
        "UPDATE {JOBS_TABLE} SET reserved_at = ?, reserved_by = ?, attempts = attempts + 1 \
         WHERE id = (SELECT id FROM {JOBS_TABLE} \
             WHERE queue = ? AND ( \
                 (reserved_at IS NULL AND available_at <= ?) \
                 OR reserved_at + (CASE WHEN timeout_secs = 0 THEN {UNLIMITED_RESERVATION_SECS} \
                     ELSE timeout_secs END + {STALE_RESERVATION_GRACE_SECS}) * 1000 <= ?) \
             ORDER BY available_at, id LIMIT 1{lock}) \
         RETURNING id, queue, job_type, payload, options, attempts, trace_parent, batch_id, \
             batch_callback, operation_id"
    );
    statement(
        backend,
        &sql,
        vec![
            now.into(),
            worker.into(),
            queue.into(),
            now.into(),
            now.into(),
        ],
    )
}

/// How often a unique insert retries after losing a race with an
/// identical dispatch.
const UNIQUE_RACE_RETRIES: usize = 3;

/// Inserts `job` through `conn`. For a unique job, a pending job with the
/// same key wins while it blocks duplicates; an older one loses its key.
/// The insert itself is `ON CONFLICT (unique_key) DO NOTHING`, so a race
/// with an identical dispatch never aborts the caller's transaction.
async fn insert_on<C: ConnectionTrait>(
    conn: &C,
    backend: DbBackend,
    job: &NewJob,
) -> Result<String, QueueError> {
    let Some(key) = &job.unique_key else {
        conn.execute_raw(insert_job(backend, job)?).await?;
        return Ok(job.id.clone());
    };
    for _ in 0..UNIQUE_RACE_RETRIES {
        let pending = conn
            .query_one_raw(statement(
                backend,
                &format!("SELECT id, created_at FROM {JOBS_TABLE} WHERE unique_key = ?"),
                vec![key.as_str().into()],
            ))
            .await?;
        if let Some(pending) = pending {
            let id: String = pending.try_get("", "id")?;
            if job.blocked_by(pending.try_get("", "created_at")?) {
                return Ok(id);
            }
            conn.execute_raw(statement(
                backend,
                &format!(
                    "UPDATE {JOBS_TABLE} SET unique_key = NULL WHERE id = ? AND unique_key = ?"
                ),
                vec![id.into(), key.as_str().into()],
            ))
            .await?;
        }
        let inserted = conn
            .execute_raw(insert_job(backend, job)?)
            .await?
            .rows_affected();
        if inserted == 1 {
            return Ok(job.id.clone());
        }
    }
    Err(DbErr::Custom(format!(
        "cannot queue unique job `{}`: identical dispatches keep racing",
        job.job_type
    ))
    .into())
}

/// `INSERT INTO jobs …` for `job` (`ON CONFLICT (unique_key) DO NOTHING`).
pub(super) fn insert_job(backend: DbBackend, job: &NewJob) -> Result<Statement, QueueError> {
    let options = serde_json::to_string(&job.options).map_err(QueueError::Payload)?;
    Ok(statement(
        backend,
        &format!(
            "INSERT INTO {JOBS_TABLE} \
             (id, queue, job_type, payload, options, attempts, max_attempts, timeout_secs, \
              available_at, unique_key, created_at, trace_parent, batch_id, batch_callback, \
              operation_id) \
             VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (unique_key) DO NOTHING"
        ),
        vec![
            job.id.as_str().into(),
            job.queue.as_str().into(),
            job.job_type.as_str().into(),
            job.payload.to_string().into(),
            options.into(),
            i64::from(job.options.max_attempts).into(),
            i64::try_from(job.options.timeout_secs)
                .unwrap_or(i64::MAX)
                .into(),
            job.available_at.into(),
            job.unique_key.clone().into(),
            job.created_at.into(),
            job.trace_parent.clone().into(),
            job.membership.batch_id.clone().into(),
            job.membership.callback.into(),
            job.membership.operation_id.clone().into(),
        ],
    ))
}

pub(super) fn json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, QueueError> {
    serde_json::from_str(text).map_err(QueueError::Payload)
}

fn count(row: &QueryResult, column: &str) -> Result<u32, DbErr> {
    let n: i64 = row.try_get("", column)?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

fn reserved_job(row: &QueryResult) -> Result<ReservedJob, QueueError> {
    Ok(ReservedJob {
        id: row.try_get("", "id")?,
        queue: row.try_get("", "queue")?,
        job_type: row.try_get("", "job_type")?,
        payload: json(&row.try_get::<String>("", "payload")?)?,
        options: json(&row.try_get::<String>("", "options")?)?,
        attempts: count(row, "attempts")?,
        trace_parent: row.try_get("", "trace_parent")?,
        membership: Membership {
            batch_id: row.try_get("", "batch_id")?,
            callback: row.try_get("", "batch_callback")?,
            operation_id: row.try_get("", "operation_id")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_placeholders_are_numbered() {
        let stmt = statement(
            DbBackend::Postgres,
            "UPDATE jobs SET a = ? WHERE b = ? AND c = ?",
            vec![1_i64.into(), 2_i64.into(), 3_i64.into()],
        );
        assert_eq!(stmt.sql, "UPDATE jobs SET a = $1 WHERE b = $2 AND c = $3");
        let stmt = statement(DbBackend::Sqlite, "SELECT ?", vec![1_i64.into()]);
        assert_eq!(stmt.sql, "SELECT ?");
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("a%b_c\\d"), "a\\%b\\_c\\\\d");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn postgres_reservation_skips_locked_rows() {
        let stmt = reserve_statement(DbBackend::Postgres, "default", "w1", 1000);
        assert!(
            stmt.sql.contains("LIMIT 1 FOR UPDATE SKIP LOCKED)"),
            "{}",
            stmt.sql
        );
        assert!(stmt.sql.contains("$5"), "{}", stmt.sql);
        assert!(!stmt.sql.contains('?'), "{}", stmt.sql);
        assert_eq!(stmt.values.map(|v| v.0.len()), Some(5));

        let stmt = reserve_statement(DbBackend::Sqlite, "default", "w1", 1000);
        assert!(!stmt.sql.contains("FOR UPDATE"), "{}", stmt.sql);
        assert!(stmt.sql.contains("RETURNING"), "{}", stmt.sql);
    }
}
