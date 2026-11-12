//! The Fleet tables: masters, shared state, finished jobs and metric
//! snapshots.

use std::collections::BTreeMap;

use chrono::{DateTime, TimeZone, Utc};
use navree_db::{Backend, Db, Executor, Row, Statement, Value, prune_time_series};
use navree_queue::AttemptOutcome;
use uuid::Uuid;

use crate::migration::{JOBS_TABLE, MASTERS_TABLE, METRICS_TABLE, STATE_TABLE};
use crate::record::{CompletedJob, MasterReport, MetricKind, MetricSnapshot};
use crate::{FleetError, FleetStatus};

/// Rows per multi-row `INSERT`.
const INSERT_CHUNK: usize = 200;

/// Fleet's tables in the app database.
#[derive(Clone, Debug)]
pub(crate) struct FleetStore {
    db: Db,
}

impl FleetStore {
    pub(crate) fn new(db: Db) -> Self {
        Self { db }
    }

    /// What the store's statements run on: the writer's pool (Fleet's
    /// bookkeeping never joins a unit of work), or the connection itself
    /// when it has no pool.
    fn executor(&self) -> &dyn Executor {
        self.db.pool()
    }

    /// The database server the store talks to.
    fn backend(&self) -> Backend {
        Executor::backend(&self.db)
    }

    /// Quotes a column name that is a keyword on some database (`key` on
    /// MySQL).
    fn quoted(&self, name: &str) -> String {
        self.backend().quote_identifier(name)
    }

    async fn execute(&self, sql: &str, values: Vec<Value>) -> Result<u64, FleetError> {
        Ok(self
            .executor()
            .execute(&Statement::portable(sql, values))
            .await?
            .rows_affected)
    }

    async fn query(&self, sql: &str, values: Vec<Value>) -> Result<Vec<Row>, FleetError> {
        Ok(self
            .executor()
            .fetch_all(&Statement::portable(sql, values))
            .await?)
    }

    // ------------------------------------------------------------- masters

    /// Inserts or replaces a master's row (its heartbeat).
    pub(crate) async fn save_master(&self, master: &MasterReport) -> Result<(), FleetError> {
        let supervisors = serde_json::to_string(&master.supervisors)?;
        let on_conflict = if self.backend() == Backend::MySql {
            "ON DUPLICATE KEY UPDATE status = VALUES(status), \
             supervisors = VALUES(supervisors), heartbeat_at = VALUES(heartbeat_at)"
        } else {
            "ON CONFLICT (id) DO UPDATE SET status = excluded.status, \
             supervisors = excluded.supervisors, heartbeat_at = excluded.heartbeat_at"
        };
        // `{on_conflict}` is one of two fixed clauses; every value is bound.
        // nosemgrep: navree-sql-built-from-strings
        self.execute(
            &format!(
                "INSERT INTO {MASTERS_TABLE} (id, name, pid, status, supervisors, started_at, heartbeat_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?) {on_conflict}"
            ),
            vec![
                master.id.as_str().into(),
                master.name.as_str().into(),
                i64::from(master.pid).into(),
                master.status.as_str().into(),
                supervisors.into(),
                master.started_at.timestamp_millis().into(),
                master.heartbeat_at.timestamp_millis().into(),
            ],
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_master(&self, id: &str) -> Result<(), FleetError> {
        self.execute(
            &format!("DELETE FROM {MASTERS_TABLE} WHERE id = ?"),
            vec![id.into()],
        )
        .await?;
        Ok(())
    }

    /// Masters whose last heartbeat is at or after `since`, oldest first.
    pub(crate) async fn masters(&self, since: i64) -> Result<Vec<MasterReport>, FleetError> {
        let rows = self
            .query(
                &format!(
                    "SELECT id, name, pid, status, supervisors, started_at, heartbeat_at \
                     FROM {MASTERS_TABLE} WHERE heartbeat_at >= ? ORDER BY started_at, id"
                ),
                vec![since.into()],
            )
            .await?;
        rows.iter().map(master_row).collect()
    }

    /// Deletes masters that have not reported since `before`.
    pub(crate) async fn delete_masters_before(&self, before: i64) -> Result<u64, FleetError> {
        self.execute(
            &format!("DELETE FROM {MASTERS_TABLE} WHERE heartbeat_at < ?"),
            vec![before.into()],
        )
        .await
    }

    // --------------------------------------------------------------- state

    pub(crate) async fn set_state(
        &self,
        key: &str,
        value: &str,
        now: i64,
    ) -> Result<(), FleetError> {
        let k = self.quoted("key");
        let on_conflict = if self.backend() == Backend::MySql {
            "ON DUPLICATE KEY UPDATE value = VALUES(value), updated_at = VALUES(updated_at)"
        } else {
            "ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
        };
        // `{k}` is the quoted `key` column and `{on_conflict}` one of two fixed clauses; values are bound.
        // nosemgrep: navree-sql-built-from-strings
        self.execute(
            &format!(
                "INSERT INTO {STATE_TABLE} ({k}, value, updated_at) VALUES (?, ?, ?) {on_conflict}"
            ),
            vec![key.into(), value.into(), now.into()],
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_state(&self, key: &str) -> Result<(), FleetError> {
        let k = self.quoted("key");
        // `{k}` is the quoted `key` column; the key is bound.
        // nosemgrep: navree-sql-built-from-strings
        self.execute(
            &format!("DELETE FROM {STATE_TABLE} WHERE {k} = ?"),
            vec![key.into()],
        )
        .await?;
        Ok(())
    }

    /// Deletes every key starting with `prefix`.
    pub(crate) async fn delete_state_prefix(&self, prefix: &str) -> Result<(), FleetError> {
        let rows = self.state_prefix(prefix).await?;
        for (key, _) in rows {
            self.delete_state(&key).await?;
        }
        Ok(())
    }

    /// Every key starting with `prefix`, with its value.
    pub(crate) async fn state_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, String)>, FleetError> {
        let k = self.quoted("key");
        let rows = self
            .query(
                &format!("SELECT {k}, value FROM {STATE_TABLE} ORDER BY {k}"),
                vec![],
            )
            .await?;
        let mut found = Vec::new();
        for row in rows {
            let key: String = row.get("key")?;
            if key.starts_with(prefix) {
                found.push((key, row.get("value")?));
            }
        }
        Ok(found)
    }

    pub(crate) async fn state(&self, key: &str) -> Result<Option<String>, FleetError> {
        let k = self.quoted("key");
        let rows = self
            .query(
                &format!("SELECT value FROM {STATE_TABLE} WHERE {k} = ?"),
                vec![key.into()],
            )
            .await?;
        rows.first()
            .map(|row| row.get("value").map_err(FleetError::from))
            .transpose()
    }

    /// Claims `key` for `now` when its stored time is at least `cooldown_ms`
    /// old (or there is none): true for exactly one caller per cooldown,
    /// across processes.
    pub(crate) async fn claim(
        &self,
        key: &str,
        now: i64,
        cooldown_ms: i64,
    ) -> Result<bool, FleetError> {
        let k = self.quoted("key");
        // `{k}` is the quoted `key` column; every value is bound.
        // nosemgrep: navree-sql-built-from-strings
        let updated = self
            .execute(
                &format!(
                    "UPDATE {STATE_TABLE} SET value = ?, updated_at = ? WHERE {k} = ? AND updated_at <= ?"
                ),
                vec![
                    now.to_string().into(),
                    now.into(),
                    key.into(),
                    now.saturating_sub(cooldown_ms).into(),
                ],
            )
            .await?;
        if updated > 0 {
            return Ok(true);
        }
        // `key` is the table's only unique key, so ignoring a duplicate
        // ignores exactly the race with another claimer.
        let insert = if self.backend() == Backend::MySql {
            format!("INSERT IGNORE INTO {STATE_TABLE} ({k}, value, updated_at) VALUES (?, ?, ?)")
        } else {
            format!(
                "INSERT INTO {STATE_TABLE} ({k}, value, updated_at) VALUES (?, ?, ?) \
                 ON CONFLICT (key) DO NOTHING"
            )
        };
        // `{k}` is the quoted `key` column; every value is bound.
        // nosemgrep: navree-sql-built-from-strings
        let inserted = self
            .execute(
                &insert,
                vec![key.into(), now.to_string().into(), now.into()],
            )
            .await?;
        Ok(inserted > 0)
    }

    // ---------------------------------------------------------------- jobs

    /// Stores finished attempts reported to master `master`.
    pub(crate) async fn insert_jobs(
        &self,
        master: Option<&str>,
        jobs: &[CompletedJob],
    ) -> Result<(), FleetError> {
        for chunk in jobs.chunks(INSERT_CHUNK) {
            let mut sql = format!(
                "INSERT INTO {JOBS_TABLE} (id, job_id, queue, job_type, attempt, outcome, runtime_ms, finished_at, master) VALUES "
            );
            let mut values: Vec<Value> = Vec::with_capacity(chunk.len() * 9);
            for (i, job) in chunk.iter().enumerate() {
                if i > 0 {
                    sql.push_str(", ");
                }
                sql.push_str("(?, ?, ?, ?, ?, ?, ?, ?, ?)");
                values.extend([
                    Uuid::now_v7().to_string().into(),
                    job.job_id.as_str().into(),
                    job.queue.as_str().into(),
                    job.job_type.as_str().into(),
                    i64::from(job.attempt).into(),
                    job.outcome.as_str().into(),
                    i64::try_from(job.runtime_ms).unwrap_or(i64::MAX).into(),
                    job.finished_at.timestamp_millis().into(),
                    master.map(str::to_owned).into(),
                ]);
            }
            self.execute(&sql, values).await?;
        }
        Ok(())
    }

    /// Attempts finished at or after `since`, newest first, whose type or
    /// queue contains `search` (case-insensitive), at most `limit`.
    pub(crate) async fn jobs(
        &self,
        since: i64,
        search: Option<&str>,
        limit: u64,
    ) -> Result<Vec<CompletedJob>, FleetError> {
        let rows = self
            .query(
                &format!(
                    "SELECT job_id, queue, job_type, attempt, outcome, runtime_ms, finished_at \
                     FROM {JOBS_TABLE} WHERE finished_at >= ? ORDER BY finished_at DESC, id DESC"
                ),
                vec![since.into()],
            )
            .await?;
        let needle = needle(search);
        let mut jobs = Vec::new();
        for row in &rows {
            let job = job_row(row)?;
            let matches = needle.as_deref().is_none_or(|needle| {
                job.queue.to_lowercase().contains(needle)
                    || job.job_type.to_lowercase().contains(needle)
            });
            if matches {
                jobs.push(job);
                if jobs.len() as u64 >= limit {
                    break;
                }
            }
        }
        Ok(jobs)
    }

    /// Per queue, attempts finished at or after `since` and their average
    /// runtime: `queue -> (count, avg_ms)`.
    pub(crate) async fn queue_runtimes(
        &self,
        since: i64,
    ) -> Result<BTreeMap<String, (u64, u64)>, FleetError> {
        let rows = self
            .query(
                &format!(
                    "SELECT queue, COUNT(*) AS n, COALESCE(SUM(runtime_ms), 0) AS total \
                     FROM {JOBS_TABLE} WHERE finished_at >= ? GROUP BY queue"
                ),
                vec![since.into()],
            )
            .await?;
        let mut found = BTreeMap::new();
        for row in rows {
            let queue: String = row.get("queue")?;
            let n = to_u64(row.get::<i64>("n")?);
            let total = to_u64(row.get::<i64>("total")?);
            found.insert(queue, (n, total.checked_div(n).unwrap_or(0)));
        }
        Ok(found)
    }

    /// Attempts finished at or after `since`.
    pub(crate) async fn count_jobs(&self, since: i64) -> Result<u64, FleetError> {
        let rows = self
            .query(
                &format!("SELECT COUNT(*) AS n FROM {JOBS_TABLE} WHERE finished_at >= ?"),
                vec![since.into()],
            )
            .await?;
        Ok(rows
            .first()
            .map(|row| row.get::<i64>("n"))
            .transpose()?
            .map_or(0, to_u64))
    }

    /// Per job type, the runtimes of attempts finished at or after `since`.
    pub(crate) async fn job_runtimes(
        &self,
        since: i64,
    ) -> Result<BTreeMap<String, Vec<u64>>, FleetError> {
        let rows = self
            .query(
                &format!("SELECT job_type, runtime_ms FROM {JOBS_TABLE} WHERE finished_at >= ?"),
                vec![since.into()],
            )
            .await?;
        let mut found: BTreeMap<String, Vec<u64>> = BTreeMap::new();
        for row in rows {
            let job_type: String = row.get("job_type")?;
            let runtime = to_u64(row.get::<i64>("runtime_ms")?);
            found.entry(job_type).or_default().push(runtime);
        }
        Ok(found)
    }

    /// Deletes the jobs that finished before `before` (Unix milliseconds):
    /// whole chunks on TimescaleDB.
    pub(crate) async fn delete_jobs_before(&self, before: i64) -> Result<u64, FleetError> {
        Ok(prune_time_series(self.executor(), JOBS_TABLE, "finished_at", before).await?)
    }

    // ------------------------------------------------------------- metrics

    pub(crate) async fn insert_metrics(
        &self,
        master: Option<&str>,
        snapshots: &[MetricSnapshot],
    ) -> Result<(), FleetError> {
        let at = self.quoted("at");
        for chunk in snapshots.chunks(INSERT_CHUNK) {
            let mut sql = format!(
                "INSERT INTO {METRICS_TABLE} (id, {at}, kind, name, processed, failed, runtime_avg_ms, runtime_p95_ms, pending, wait_ms, master) VALUES "
            );
            let mut values: Vec<Value> = Vec::with_capacity(chunk.len() * 11);
            for (i, s) in chunk.iter().enumerate() {
                if i > 0 {
                    sql.push_str(", ");
                }
                sql.push_str("(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)");
                values.extend([
                    Uuid::now_v7().to_string().into(),
                    s.at.timestamp_millis().into(),
                    s.kind.as_str().into(),
                    s.name.as_str().into(),
                    to_i64(s.processed).into(),
                    to_i64(s.failed).into(),
                    to_i64(s.runtime_avg_ms).into(),
                    to_i64(s.runtime_p95_ms).into(),
                    s.pending.map(to_i64).into(),
                    s.wait_ms.map(to_i64).into(),
                    master.map(str::to_owned).into(),
                ]);
            }
            // `{at}` is the quoted `at` column, then `?` groups; every value is bound.
            // nosemgrep: navree-sql-built-from-strings
            self.execute(&sql, values).await?;
        }
        Ok(())
    }

    /// Snapshots taken at or after `since`, oldest first.
    pub(crate) async fn metrics(&self, since: i64) -> Result<Vec<MetricSnapshot>, FleetError> {
        let at = self.quoted("at");
        let rows = self
            .query(
                &format!(
                    "SELECT {at}, kind, name, processed, failed, runtime_avg_ms, runtime_p95_ms, pending, wait_ms \
                     FROM {METRICS_TABLE} WHERE {at} >= ? ORDER BY {at}, id"
                ),
                vec![since.into()],
            )
            .await?;
        let mut found = Vec::new();
        for row in rows {
            let kind: String = row.get("kind")?;
            let Some(kind) = MetricKind::parse(&kind) else {
                continue;
            };
            found.push(MetricSnapshot {
                at: from_millis(row.get("at")?),
                kind,
                name: row.get("name")?,
                processed: to_u64(row.get("processed")?),
                failed: to_u64(row.get("failed")?),
                runtime_avg_ms: to_u64(row.get("runtime_avg_ms")?),
                runtime_p95_ms: to_u64(row.get("runtime_p95_ms")?),
                pending: row.get::<Option<i64>>("pending")?.map(to_u64),
                wait_ms: row.get::<Option<i64>>("wait_ms")?.map(to_u64),
            });
        }
        Ok(found)
    }

    /// Deletes the snapshots taken before `before` (Unix milliseconds):
    /// whole chunks on TimescaleDB.
    pub(crate) async fn delete_metrics_before(&self, before: i64) -> Result<u64, FleetError> {
        Ok(prune_time_series(self.executor(), METRICS_TABLE, "at", before).await?)
    }
}

fn master_row(row: &Row) -> Result<MasterReport, FleetError> {
    let status: String = row.get("status")?;
    let supervisors: String = row.get("supervisors")?;
    Ok(MasterReport {
        id: row.get("id")?,
        name: row.get("name")?,
        pid: u32::try_from(row.get::<i64>("pid")?).unwrap_or_default(),
        status: if status == "paused" {
            FleetStatus::Paused
        } else {
            FleetStatus::Running
        },
        supervisors: serde_json::from_str(&supervisors)?,
        started_at: from_millis(row.get("started_at")?),
        heartbeat_at: from_millis(row.get("heartbeat_at")?),
    })
}

fn job_row(row: &Row) -> Result<CompletedJob, FleetError> {
    let outcome: String = row.get("outcome")?;
    Ok(CompletedJob {
        job_id: row.get("job_id")?,
        queue: row.get("queue")?,
        job_type: row.get("job_type")?,
        attempt: u32::try_from(row.get::<i64>("attempt")?).unwrap_or_default(),
        outcome: parse_outcome(&outcome)?,
        runtime_ms: to_u64(row.get("runtime_ms")?),
        finished_at: from_millis(row.get("finished_at")?),
    })
}

pub(crate) fn parse_outcome(text: &str) -> Result<AttemptOutcome, FleetError> {
    match text {
        "succeeded" => Ok(AttemptOutcome::Succeeded),
        "released" => Ok(AttemptOutcome::Released),
        "failed" => Ok(AttemptOutcome::Failed),
        "cancelled" => Ok(AttemptOutcome::Cancelled),
        other => Err(FleetError::Corrupt(format!(
            "unknown job outcome `{other}`"
        ))),
    }
}

/// The lowercased, trimmed search text, if any.
pub(crate) fn needle(search: Option<&str>) -> Option<String> {
    search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
}

/// Unix milliseconds as a UTC time.
pub(crate) fn from_millis(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn to_u64(n: i64) -> u64 {
    u64::try_from(n).unwrap_or_default()
}

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_numbered_on_postgres() {
        let s = Statement::portable("a = ? AND b = ?", [1.into(), 2.into()]);
        let native = s.for_backend(Backend::Postgres).unwrap();
        assert_eq!(native.sql(), "a = $1 AND b = $2");
        let native = s.for_backend(Backend::Sqlite).unwrap();
        assert_eq!(native.sql(), "a = ? AND b = ?");
    }

    #[test]
    fn outcomes_and_needles_parse() {
        assert_eq!(parse_outcome("released").unwrap(), AttemptOutcome::Released);
        assert_eq!(
            parse_outcome("cancelled").unwrap(),
            AttemptOutcome::Cancelled
        );
        assert!(parse_outcome("odd").is_err());
        assert_eq!(needle(Some("  Mail ")).as_deref(), Some("mail"));
        assert_eq!(needle(Some("  ")), None);
    }
}
