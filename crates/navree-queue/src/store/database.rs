//! The database driver: tables `jobs` and `failed_jobs`.
//!
//! Jobs are reserved, released and completed with hand-written statements
//! (a reservation is one atomic `UPDATE … RETURNING` that skips locked
//! rows; MySQL, which has no `UPDATE … RETURNING`, locks a candidate row
//! with `FOR UPDATE SKIP LOCKED` in a short transaction instead); failed
//! jobs are [`FailedJob`] models, written and read through navree-orm.

use navree_db::{Backend, Db, DbError, Executor, ExecutorExt as _, Row, Statement, Uow, Value};
use navree_orm::{Model as _, Record};

use super::batches::finish_uow;
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

    /// What the store's statements run on: the writer's pool, never the
    /// task's unit of work (a worker's bookkeeping commits on its own), or
    /// the connection itself when it has no pool.
    pub(super) fn executor(&self) -> &dyn Executor {
        self.db.pool()
    }

    /// The database server the store talks to.
    pub(super) fn backend(&self) -> Backend {
        Executor::backend(&self.db)
    }

    /// Stores `job`; returns its id, or the id of the pending unique job
    /// that takes its place.
    pub(crate) async fn insert(&self, job: &NewJob) -> Result<String, QueueError> {
        insert_on(self.executor(), job).await
    }

    /// [`insert`](DatabaseStore::insert) in the transaction of `uow`.
    pub(crate) async fn insert_in(&self, uow: &Uow, job: &NewJob) -> Result<String, QueueError> {
        insert_on(uow, job).await
    }

    /// Atomically reserves the next job of `queue` for `worker`: an
    /// unreserved job whose time has come, or one whose reservation is
    /// stale (see [`stale_after_ms`](crate::record::stale_after_ms)). One
    /// `UPDATE … WHERE id = (SELECT … LIMIT 1) RETURNING`; PostgreSQL adds
    /// `FOR UPDATE SKIP LOCKED` to the subquery. MySQL locks the candidate
    /// first (see [`reserve_locking`](DatabaseStore::reserve_locking)).
    pub(crate) async fn reserve(
        &self,
        queue: &str,
        worker: &str,
        now: i64,
    ) -> Result<Option<ReservedJob>, QueueError> {
        if self.backend() == Backend::MySql {
            return self.reserve_locking(queue, worker, now).await;
        }
        let row = self
            .executor()
            .fetch_optional(&reserve_statement(self.backend(), queue, worker, now))
            .await?;
        row.map(|row| reserved_job(&row)).transpose()
    }

    /// [`reserve`](DatabaseStore::reserve) where the database cannot
    /// return the rows of an `UPDATE`: in one transaction, `SELECT … FOR
    /// UPDATE SKIP LOCKED` picks and locks the candidate (workers racing
    /// for the queue skip each other's rows), then it is updated and read
    /// back.
    async fn reserve_locking(
        &self,
        queue: &str,
        worker: &str,
        now: i64,
    ) -> Result<Option<ReservedJob>, QueueError> {
        let tx = self.db.begin().await?;
        let reserved = async {
            let Some(candidate) = tx.fetch_optional(&candidate_statement(queue, now)).await? else {
                return Ok::<_, DbError>(None);
            };
            let id: String = candidate.get("id")?;
            tx.execute(&Statement::portable(
                format!(
                    "UPDATE {JOBS_TABLE} SET reserved_at = ?, reserved_by = ?, \
                     attempts = attempts + 1 WHERE id = ?"
                ),
                [now.into(), worker.into(), id.as_str().into()],
            ))
            .await?;
            let row = tx
                .fetch_one(&Statement::portable(
                    format!("SELECT {RESERVED_COLUMNS} FROM {JOBS_TABLE} WHERE id = ?"),
                    [id.into()],
                ))
                .await?;
            Ok(Some(row))
        }
        .await;
        match reserved {
            Ok(row) => {
                tx.commit().await?;
                row.map(|row| reserved_job(&row)).transpose()
            }
            Err(error) => {
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(target: "navree::job", error = %rollback, "rollback failed");
                }
                Err(error.into())
            }
        }
    }

    pub(crate) async fn complete(&self, id: &str) -> Result<(), QueueError> {
        self.executor()
            .execute(&Statement::portable(
                format!("DELETE FROM {JOBS_TABLE} WHERE id = ?"),
                [id.into()],
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
            .executor()
            .execute(&Statement::portable(
                format!(
                    "UPDATE {JOBS_TABLE} SET reserved_at = NULL, reserved_by = NULL, available_at = ? \
                     WHERE id = ? AND reserved_by = ?"
                ),
                [available_at.into(), id.into(), worker.into()],
            ))
            .await?;
        Ok(result.rows_affected == 1)
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
        let failed = FailedJob::of(job, error, now);
        let uow = self.db.uow(None, None).await;
        let result = async {
            let deleted = uow
                .execute(&Statement::portable(
                    format!("DELETE FROM {JOBS_TABLE} WHERE id = ? AND reserved_by = ?"),
                    [job.id.as_str().into(), worker.into()],
                ))
                .await?
                .rows_affected;
            if deleted == 1 {
                FailedJob::insert(&uow, [failed]).await?;
            }
            Ok::<_, QueueError>(deleted == 1)
        }
        .await;
        finish_uow(uow, result).await
    }

    /// Every failed job, newest first.
    pub(crate) async fn failed(&self) -> Result<Vec<FailedJob>, QueueError> {
        Ok(FailedJob::query()
            .order_by_desc(FailedJob::FAILED_AT)
            .order_by_desc(FailedJob::ID)
            .get(&self.db)
            .await?
            .into_models())
    }

    /// Moves the targeted failed jobs back to `jobs`, oldest first (one
    /// transaction); returns how many.
    pub(crate) async fn retry(&self, target: &RetryTarget, now: i64) -> Result<u64, QueueError> {
        let backend = self.backend();
        let mut targeted = FailedJob::query()
            .order_by(FailedJob::FAILED_AT)
            .order_by(FailedJob::ID);
        if let RetryTarget::Id(id) = target {
            targeted = targeted.where_key(id.clone());
        }

        let uow = self.db.uow(None, None).await;
        let result = async {
            let failed = targeted.get(&uow).await?.into_models();
            for job in &failed {
                uow.execute(&insert_job(backend, &job.requeued(now))?)
                    .await?;
                FailedJob::query()
                    .where_key(job.id.clone())
                    .delete(&uow)
                    .await?;
            }
            Ok::<_, QueueError>(failed.len() as u64)
        }
        .await;
        finish_uow(uow, result).await
    }

    /// Deletes every failed job; returns how many.
    pub(crate) async fn flush_failed(&self) -> Result<u64, QueueError> {
        Ok(FailedJob::query().delete(&self.db).await?)
    }

    pub(crate) async fn pending_count(&self, queue: &str) -> Result<u64, QueueError> {
        let n: i64 = self
            .executor()
            .fetch_scalar(&Statement::portable(
                format!("SELECT COUNT(*) AS n FROM {JOBS_TABLE} WHERE queue = ?"),
                [queue.into()],
            ))
            .await?;
        Ok(u64::try_from(n).unwrap_or_default())
    }

    /// Waiting, delayed and running counts of `queue` at `now`, and the
    /// oldest waiting job's availability: one aggregate query.
    pub(crate) async fn status(&self, queue: &str, now: i64) -> Result<QueueStatus, QueueError> {
        let row = self
            .executor()
            .fetch_optional(&Statement::portable(
                format!(
                    "SELECT \
                     COALESCE(SUM(CASE WHEN reserved_at IS NULL AND available_at <= ? THEN 1 ELSE 0 END), 0) AS waiting_jobs, \
                     COALESCE(SUM(CASE WHEN reserved_at IS NULL AND available_at > ? THEN 1 ELSE 0 END), 0) AS delayed_jobs, \
                     COALESCE(SUM(CASE WHEN reserved_at IS NOT NULL THEN 1 ELSE 0 END), 0) AS running_jobs, \
                     MIN(CASE WHEN reserved_at IS NULL AND available_at <= ? THEN available_at END) AS oldest \
                     FROM {JOBS_TABLE} WHERE queue = ?"
                ),
                [now.into(), now.into(), now.into(), queue.into()],
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
            let number = |column: &str| -> Result<u64, DbError> {
                let n: i64 = row.get(column)?;
                Ok(u64::try_from(n).unwrap_or_default())
            };
            status.waiting = number("waiting_jobs")?;
            status.delayed = number("delayed_jobs")?;
            status.running = number("running_jobs")?;
            let oldest: Option<i64> = row.get("oldest")?;
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
            sql.push_str(&format!(
                " AND (LOWER(job_type) LIKE ? ESCAPE '{LIKE_ESCAPE}' \
                 OR LOWER(queue) LIKE ? ESCAPE '{LIKE_ESCAPE}')"
            ));
            values.push(pattern.clone().into());
            values.push(pattern.into());
        }
        sql.push_str(" ORDER BY available_at, id LIMIT ?");
        values.push(i64::try_from(query.limit()).unwrap_or(i64::MAX).into());
        let rows = self
            .executor()
            .fetch_all(&Statement::portable(sql, values))
            .await?;
        rows.iter().map(|row| pending_job(row, now)).collect()
    }

    pub(crate) async fn pending_job(
        &self,
        id: &str,
        now: i64,
    ) -> Result<Option<PendingJob>, QueueError> {
        let row = self
            .executor()
            .fetch_optional(&Statement::portable(
                format!("SELECT {PENDING_COLUMNS} FROM {JOBS_TABLE} WHERE id = ?"),
                [id.into()],
            ))
            .await?;
        row.map(|row| pending_job(&row, now)).transpose()
    }

    /// Failed job `id`, if there is one.
    pub(crate) async fn failed_job(&self, id: &str) -> Result<Option<FailedJob>, QueueError> {
        Ok(FailedJob::find(&self.db, id.to_owned())
            .await?
            .map(Record::into_inner))
    }

    /// Deletes failed job `id`; false when there is none.
    pub(crate) async fn forget(&self, id: &str) -> Result<bool, QueueError> {
        let deleted = FailedJob::query()
            .where_key(id.to_owned())
            .delete(&self.db)
            .await?;
        Ok(deleted == 1)
    }
}

/// The columns [`pending_job`] reads.
const PENDING_COLUMNS: &str =
    "id, queue, job_type, payload, options, attempts, available_at, reserved_at, created_at";

/// The columns [`reserved_job`] reads.
const RESERVED_COLUMNS: &str = "id, queue, job_type, payload, options, attempts, trace_parent, \
     batch_id, batch_callback, operation_id";

/// The `ESCAPE` character of the `LIKE` patterns (`^` needs no escaping in
/// a string literal on any of the databases).
const LIKE_ESCAPE: char = '^';

/// `text` with the `LIKE` wildcards (and the escape character) escaped
/// with [`LIKE_ESCAPE`].
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, LIKE_ESCAPE | '%' | '_') {
            escaped.push(LIKE_ESCAPE);
        }
        escaped.push(c);
    }
    escaped
}

fn pending_job(row: &Row, now: i64) -> Result<PendingJob, QueueError> {
    let available_at: i64 = row.get("available_at")?;
    let reserved_at: Option<i64> = row.get("reserved_at")?;
    let state = if reserved_at.is_some() {
        PendingState::Running
    } else if available_at <= now {
        PendingState::Waiting
    } else {
        PendingState::Delayed
    };
    Ok(PendingJob {
        id: row.get("id")?,
        queue: row.get("queue")?,
        job_type: row.get("job_type")?,
        payload: json(&row.get::<String>("payload")?)?,
        options: json(&row.get::<String>("options")?)?,
        attempts: count(row, "attempts")?,
        state,
        available_at: from_millis(available_at),
        reserved_at: reserved_at.map(from_millis),
        created_at: from_millis(row.get("created_at")?),
    })
}

/// What makes a job reservable by a worker: unreserved and available (one
/// bound time), or reserved longer ago than its timeout and the grace
/// period (a crashed worker's; one bound time).
fn reservable() -> String {
    format!(
        "(reserved_at IS NULL AND available_at <= ?) \
         OR reserved_at + (CASE WHEN timeout_secs = 0 THEN {UNLIMITED_RESERVATION_SECS} \
             ELSE timeout_secs END + {STALE_RESERVATION_GRACE_SECS}) * 1000 <= ?"
    )
}

/// The reservation statement (see [`DatabaseStore::reserve`]), for a
/// database that returns the rows of an `UPDATE`.
fn reserve_statement(backend: Backend, queue: &str, worker: &str, now: i64) -> Statement {
    let lock = if backend == Backend::Postgres {
        " FOR UPDATE SKIP LOCKED"
    } else {
        ""
    };
    let reservable = reservable();
    let sql = format!(
        "UPDATE {JOBS_TABLE} SET reserved_at = ?, reserved_by = ?, attempts = attempts + 1 \
         WHERE id = (SELECT id FROM {JOBS_TABLE} \
             WHERE queue = ? AND ( {reservable}) \
             ORDER BY available_at, id LIMIT 1{lock}) \
         RETURNING {RESERVED_COLUMNS}"
    );
    Statement::portable(
        sql,
        [
            now.into(),
            worker.into(),
            queue.into(),
            now.into(),
            now.into(),
        ],
    )
}

/// The candidate of a locking reservation: the next reservable job of
/// `queue`, locked, skipping rows other workers hold.
fn candidate_statement(queue: &str, now: i64) -> Statement {
    let reservable = reservable();
    Statement::portable(
        format!(
            "SELECT id FROM {JOBS_TABLE} WHERE queue = ? AND ( {reservable}) \
             ORDER BY available_at, id LIMIT 1 FOR UPDATE SKIP LOCKED"
        ),
        [queue.into(), now.into(), now.into()],
    )
}

/// How often a unique insert retries after losing a race with an
/// identical dispatch.
const UNIQUE_RACE_RETRIES: usize = 3;

/// Inserts `job` through `executor`. For a unique job, a pending job with
/// the same key wins while it blocks duplicates; an older one loses its
/// key. The insert itself is `ON CONFLICT (unique_key) DO NOTHING` (on
/// MySQL, which has no such clause, a duplicate `unique_key` is told from
/// the error), so a race with an identical dispatch never aborts the
/// caller's transaction.
async fn insert_on(executor: &dyn Executor, job: &NewJob) -> Result<String, QueueError> {
    let Some(key) = &job.unique_key else {
        insert_unless_duplicate(executor, job).await?;
        return Ok(job.id.clone());
    };
    for _ in 0..UNIQUE_RACE_RETRIES {
        let pending = executor
            .fetch_optional(&Statement::portable(
                format!("SELECT id, created_at FROM {JOBS_TABLE} WHERE unique_key = ?"),
                [key.as_str().into()],
            ))
            .await?;
        if let Some(pending) = pending {
            let id: String = pending.get("id")?;
            if job.blocked_by(pending.get("created_at")?) {
                return Ok(id);
            }
            executor
                .execute(&Statement::portable(
                    format!(
                        "UPDATE {JOBS_TABLE} SET unique_key = NULL WHERE id = ? AND unique_key = ?"
                    ),
                    [id.into(), key.as_str().into()],
                ))
                .await?;
        }
        if insert_unless_duplicate(executor, job).await? == 1 {
            return Ok(job.id.clone());
        }
    }
    Err(DbError::invalid(format!(
        "cannot queue unique job `{}`: identical dispatches keep racing",
        job.job_type
    ))
    .into())
}

/// Runs the insert of `job`; how many rows it stored (0: another job holds
/// its unique key).
async fn insert_unless_duplicate(executor: &dyn Executor, job: &NewJob) -> Result<u64, QueueError> {
    let backend = executor.backend();
    match executor.execute(&insert_job(backend, job)?).await {
        Ok(executed) => Ok(executed.rows_affected),
        Err(error)
            if backend == Backend::MySql
                && error.is_unique_violation()
                && (error.constraint().is_some_and(is_unique_key)
                    || is_unique_key(error.message())) =>
        {
            Ok(0)
        }
        Err(error) => Err(error.into()),
    }
}

/// Whether `name` (a constraint name or an error message) is about the
/// unique index of `jobs.unique_key`.
fn is_unique_key(name: &str) -> bool {
    name.contains("unique_key")
}

/// `INSERT INTO jobs …` for `job` (`ON CONFLICT (unique_key) DO NOTHING`,
/// except on MySQL, which has no such clause).
pub(super) fn insert_job(backend: Backend, job: &NewJob) -> Result<Statement, QueueError> {
    let options = serde_json::to_string(&job.options).map_err(QueueError::Payload)?;
    let on_conflict = if backend == Backend::MySql {
        ""
    } else {
        " ON CONFLICT (unique_key) DO NOTHING"
    };
    Ok(Statement::portable(
        format!(
            "INSERT INTO {JOBS_TABLE} \
             (id, queue, job_type, payload, options, attempts, max_attempts, timeout_secs, \
              available_at, unique_key, created_at, trace_parent, batch_id, batch_callback, \
              operation_id) \
             VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?, ?, ?){on_conflict}"
        ),
        [
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

fn count(row: &Row, column: &str) -> Result<u32, DbError> {
    let n: i64 = row.get(column)?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

fn reserved_job(row: &Row) -> Result<ReservedJob, QueueError> {
    Ok(ReservedJob {
        id: row.get("id")?,
        queue: row.get("queue")?,
        job_type: row.get("job_type")?,
        payload: json(&row.get::<String>("payload")?)?,
        options: json(&row.get::<String>("options")?)?,
        attempts: count(row, "attempts")?,
        trace_parent: row.get("trace_parent")?,
        membership: Membership {
            batch_id: row.get("batch_id")?,
            callback: row.get("batch_callback")?,
            operation_id: row.get("operation_id")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_placeholders_are_numbered() {
        let stmt = reserve_statement(Backend::Postgres, "default", "w1", 1000);
        let native = stmt.for_backend(Backend::Postgres).expect("renders");
        assert!(native.sql().contains("$1"), "{}", native.sql());
        assert!(native.sql().contains("$5"), "{}", native.sql());
        assert!(!native.sql().contains('?'), "{}", native.sql());
        let native = stmt.for_backend(Backend::Sqlite).expect("renders");
        assert!(native.sql().contains('?'), "{}", native.sql());
        assert!(!native.sql().contains("$1"), "{}", native.sql());
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("a%b_c^d"), "a^%b^_c^^d");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn postgres_reservation_skips_locked_rows() {
        let stmt = reserve_statement(Backend::Postgres, "default", "w1", 1000);
        assert!(
            stmt.sql().contains("LIMIT 1 FOR UPDATE SKIP LOCKED)"),
            "{}",
            stmt.sql()
        );
        assert_eq!(stmt.values().len(), 5);

        let stmt = reserve_statement(Backend::Sqlite, "default", "w1", 1000);
        assert!(!stmt.sql().contains("FOR UPDATE"), "{}", stmt.sql());
        assert!(stmt.sql().contains("RETURNING"), "{}", stmt.sql());
    }

    #[test]
    fn mysql_reservation_locks_its_candidate_and_skips_locked_rows() {
        let stmt = candidate_statement("default", 1000);
        assert!(
            stmt.sql().ends_with("LIMIT 1 FOR UPDATE SKIP LOCKED"),
            "{}",
            stmt.sql()
        );
        assert_eq!(stmt.values().len(), 3);
    }
}
