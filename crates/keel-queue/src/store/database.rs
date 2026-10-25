//! The database driver: tables `jobs` and `failed_jobs`.

use keel_db::{Db, Uow};
use sea_orm::{ConnectionTrait, DbBackend, DbErr, QueryResult, Statement, Value};

use crate::migration::{FAILED_JOBS_TABLE, JOBS_TABLE};
use crate::record::{
    FailedJob, NewJob, ReservedJob, RetryTarget, UNLIMITED_RESERVATION_SECS, from_millis,
};
use crate::{QueueError, STALE_RESERVATION_GRACE_SECS};

/// Jobs in the app database.
#[derive(Clone, Debug)]
pub(crate) struct DatabaseStore {
    db: Db,
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
        let options = serde_json::to_string(&job.options).map_err(QueueError::Payload)?;
        let payload = job.payload.to_string();
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
                    tx.execute_raw(statement(
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
                            payload.into(),
                            options.into(),
                            error.into(),
                            i64::from(job.attempts).into(),
                            now.into(),
                        ],
                    ))
                    .await?;
                }
                Ok(deleted == 1)
            })
            .await?;
        Ok(moved)
    }

    pub(crate) async fn failed(&self) -> Result<Vec<FailedJob>, QueueError> {
        let rows = self
            .db
            .conn()
            .query_all_raw(self.stmt(
                &format!(
                    "SELECT id, queue, job_type, payload, options, error, attempts, failed_at \
                     FROM {FAILED_JOBS_TABLE} ORDER BY failed_at DESC, id DESC"
                ),
                Vec::new(),
            ))
            .await?;
        rows.iter().map(failed_job).collect()
    }

    /// Moves the targeted failed jobs back to `jobs` (one transaction);
    /// returns how many.
    pub(crate) async fn retry(&self, target: &RetryTarget, now: i64) -> Result<u64, QueueError> {
        let backend = self.db.backend();
        let mut select = format!(
            "SELECT id, queue, job_type, payload, options, error, attempts, failed_at \
             FROM {FAILED_JOBS_TABLE}"
        );
        let mut values = Vec::new();
        if let RetryTarget::Id(id) = target {
            select.push_str(" WHERE id = ?");
            values.push(id.as_str().into());
        }
        select.push_str(" ORDER BY failed_at, id");

        let uow = self.db.uow(None, None).await;
        let result = async {
            let tx = uow.tx().await?;
            let rows = tx
                .query_all_raw(statement(backend, &select, values))
                .await?;
            let failed = rows.iter().map(failed_job).collect::<Result<Vec<_>, _>>()?;
            for job in &failed {
                tx.execute_raw(insert_job(backend, &job.requeued(now))?)
                    .await?;
                tx.execute_raw(statement(
                    backend,
                    &format!("DELETE FROM {FAILED_JOBS_TABLE} WHERE id = ?"),
                    vec![job.id.as_str().into()],
                ))
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
                    tracing::warn!(target: "keel::job", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    pub(crate) async fn flush_failed(&self) -> Result<u64, QueueError> {
        let result = self
            .db
            .conn()
            .execute_raw(self.stmt(&format!("DELETE FROM {FAILED_JOBS_TABLE}"), Vec::new()))
            .await?;
        Ok(result.rows_affected())
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
}

/// A statement for `backend`: `?` placeholders become `$1, $2, …` on
/// PostgreSQL. (No SQL here contains a literal `?`.)
fn statement(backend: DbBackend, sql: &str, values: Vec<Value>) -> Statement {
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
         RETURNING id, queue, job_type, payload, options, attempts"
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
fn insert_job(backend: DbBackend, job: &NewJob) -> Result<Statement, QueueError> {
    let options = serde_json::to_string(&job.options).map_err(QueueError::Payload)?;
    Ok(statement(
        backend,
        &format!(
            "INSERT INTO {JOBS_TABLE} \
             (id, queue, job_type, payload, options, attempts, max_attempts, timeout_secs, \
              available_at, unique_key, created_at) \
             VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?) \
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
        ],
    ))
}

fn json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, QueueError> {
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
    })
}

fn failed_job(row: &QueryResult) -> Result<FailedJob, QueueError> {
    Ok(FailedJob {
        id: row.try_get("", "id")?,
        queue: row.try_get("", "queue")?,
        job_type: row.try_get("", "job_type")?,
        payload: json(&row.try_get::<String>("", "payload")?)?,
        options: json(&row.try_get::<String>("", "options")?)?,
        error: row.try_get("", "error")?,
        attempts: count(row, "attempts")?,
        failed_at: from_millis(row.try_get("", "failed_at")?),
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
