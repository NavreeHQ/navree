//! [`SqliteStore`]: entries in a SQLite file of their own.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use navree_kernel::BoxFuture;
use sqlx::sqlite::{
    SqliteArguments, SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteRow, SqliteSynchronous,
};
use sqlx::{Arguments, AssertSqlSafe, ConnectOptions, Row};

use super::{ListQuery, LogbookStore};
use crate::filter::{Comparison, StatusMatch, Term};
use crate::{Content, Entry, LogbookError};

/// The schema version in `PRAGMA user_version`; a file with another version
/// is emptied and recreated (it only holds development data).
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &[&str] = &[
    "CREATE TABLE logbook_entries (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL UNIQUE,
        batch_id TEXT,
        batch_key TEXT NOT NULL,
        kind TEXT NOT NULL,
        top_level INTEGER NOT NULL,
        at INTEGER NOT NULL,
        duration_ms REAL,
        user_id TEXT,
        workspace_id TEXT,
        trace_id TEXT,
        status INTEGER,
        route TEXT,
        failed INTEGER NOT NULL,
        slow INTEGER NOT NULL,
        search TEXT NOT NULL,
        content TEXT NOT NULL
    )",
    "CREATE INDEX logbook_entries_batch_key ON logbook_entries (batch_key)",
    "CREATE INDEX logbook_entries_batch_id ON logbook_entries (batch_id)",
    "CREATE INDEX logbook_entries_kind ON logbook_entries (kind, seq)",
    "CREATE INDEX logbook_entries_at ON logbook_entries (at)",
    "CREATE TABLE logbook_tags (
        batch_key TEXT NOT NULL,
        tag TEXT NOT NULL,
        PRIMARY KEY (batch_key, tag)
    )",
];

/// The columns every read selects, tags included.
const COLUMNS: &str = "e.seq, e.id, e.batch_id, e.at, e.duration_ms, e.user_id, \
    e.workspace_id, e.trace_id, e.failed, e.slow, e.content, \
    (SELECT group_concat(t.tag, char(10)) FROM logbook_tags t WHERE t.batch_key = e.batch_key) AS tags";

/// Keeps entries in a SQLite file of its own (`logbook.store = "file"`,
/// `logbook.file`), never in the app's database: they survive the restarts
/// `navree dev` makes.
#[derive(Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
    path: PathBuf,
}

impl SqliteStore {
    /// Opens (creating it and its directory when missing) the file at `path`.
    pub async fn open(path: impl AsRef<Path>) -> Result<SqliteStore, LogbookError> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|error| {
                LogbookError::Store(format!("cannot create {}: {error}", dir.display()))
            })?;
        }
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            // Its own statements must never become log lines: Logbook would
            // record them, write them, and log again.
            .disable_statement_logging();
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        let store = SqliteStore { pool, path };
        store.migrate().await?;
        Ok(store)
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Creates the tables, or recreates them for another schema version.
    async fn migrate(&self) -> Result<(), LogbookError> {
        let mut tx = self.pool.begin().await?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&mut *tx)
            .await?;
        if version != SCHEMA_VERSION {
            sqlx::query("DROP TABLE IF EXISTS logbook_entries")
                .execute(&mut *tx)
                .await?;
            sqlx::query("DROP TABLE IF EXISTS logbook_tags")
                .execute(&mut *tx)
                .await?;
            for statement in SCHEMA {
                sqlx::query(*statement).execute(&mut *tx).await?;
            }
            sqlx::query(AssertSqlSafe(format!(
                "PRAGMA user_version = {SCHEMA_VERSION}"
            )))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn insert_all(&self, entries: Vec<Entry>) -> Result<(), LogbookError> {
        let mut tx = self.pool.begin().await?;
        for entry in entries {
            let content = serde_json::to_string(&entry.content)
                .map_err(|error| LogbookError::Store(error.to_string()))?;
            sqlx::query(
                "INSERT INTO logbook_entries (id, batch_id, batch_key, kind, top_level, at, \
                 duration_ms, user_id, workspace_id, trace_id, status, route, failed, slow, \
                 search, content) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&entry.id)
            .bind(&entry.batch_id)
            .bind(entry.batch_key())
            .bind(entry.kind().as_str())
            .bind(entry.is_top_level())
            .bind(micros(entry.at))
            .bind(entry.duration_ms)
            .bind(&entry.user_id)
            .bind(&entry.workspace_id)
            .bind(&entry.trace_id)
            .bind(entry.status().map(i64::from))
            .bind(entry.route())
            .bind(entry.failed)
            .bind(entry.slow)
            .bind(entry.search_text().to_lowercase())
            .bind(content)
            .execute(&mut *tx)
            .await?;
            for tag in &entry.tags {
                sqlx::query("INSERT OR IGNORE INTO logbook_tags (batch_key, tag) VALUES (?, ?)")
                    .bind(entry.batch_key())
                    .bind(tag)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn add_all_tags(&self, key: String, tags: Vec<String>) -> Result<(), LogbookError> {
        let mut tx = self.pool.begin().await?;
        for tag in tags {
            sqlx::query("INSERT OR IGNORE INTO logbook_tags (batch_key, tag) VALUES (?, ?)")
                .bind(&key)
                .bind(tag)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn list_entries(&self, query: &ListQuery) -> Result<Vec<Entry>, LogbookError> {
        let mut sql = format!("SELECT {COLUMNS} FROM logbook_entries e WHERE 1 = 1");
        let mut args = SqliteArguments::default();
        let filter = &query.filter;
        if filter.top_level_only() {
            sql.push_str(" AND e.top_level = 1");
        }
        let kinds = filter.kinds();
        if !kinds.is_empty() {
            sql.push_str(" AND e.kind IN (");
            for (i, kind) in kinds.iter().enumerate() {
                sql.push_str(if i == 0 { "?" } else { ", ?" });
                bind(&mut args, kind.as_str().to_owned())?;
            }
            sql.push(')');
        }
        if let Some(before) = query.before {
            sql.push_str(" AND e.seq < ?");
            bind(&mut args, seq_arg(before))?;
        }
        if let Some(after) = query.after {
            sql.push_str(" AND e.seq > ?");
            bind(&mut args, seq_arg(after))?;
        }
        for term in filter.terms() {
            push_term(&mut sql, &mut args, term)?;
        }
        sql.push_str(" ORDER BY e.seq DESC LIMIT ?");
        bind(&mut args, query.capped_limit() as i64)?;
        // Every value is a bound argument; the text only joins fixed fragments.
        let rows = sqlx::query_with(AssertSqlSafe(sql), args)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(entry_from_row).collect()
    }

    async fn find(&self, id: &str) -> Result<Option<Entry>, LogbookError> {
        let row = sqlx::query(AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM logbook_entries e WHERE e.id = ?"
        )))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(entry_from_row).transpose()
    }

    async fn batch_entries(&self, batch_id: &str) -> Result<Vec<Entry>, LogbookError> {
        let rows = sqlx::query(AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM logbook_entries e WHERE e.batch_id = ? ORDER BY e.seq"
        )))
        .bind(batch_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(entry_from_row).collect()
    }

    async fn count_entries(&self) -> Result<u64, LogbookError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logbook_entries")
            .fetch_one(&self.pool)
            .await?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    async fn prune_entries(
        &self,
        max_entries: usize,
        older_than: DateTime<Utc>,
    ) -> Result<u64, LogbookError> {
        let mut tx = self.pool.begin().await?;
        let doomed_old = "SELECT DISTINCT batch_key FROM logbook_entries WHERE at < ?";
        // A fixed subquery held in a local &str; the value is bound.
        // nosemgrep: navree-sql-built-from-strings
        sqlx::query(AssertSqlSafe(format!(
            "DELETE FROM logbook_tags WHERE batch_key IN ({doomed_old})"
        )))
        .bind(micros(older_than))
        .execute(&mut *tx)
        .await?;
        // A fixed subquery held in a local &str; the value is bound.
        // nosemgrep: navree-sql-built-from-strings
        let mut deleted = sqlx::query(AssertSqlSafe(format!(
            "DELETE FROM logbook_entries WHERE batch_key IN ({doomed_old})"
        )))
        .bind(micros(older_than))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let cutoff: Option<i64> = sqlx::query_scalar(
            "SELECT seq FROM logbook_entries ORDER BY seq DESC LIMIT 1 OFFSET ?",
        )
        .bind(i64::try_from(max_entries).unwrap_or(i64::MAX))
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(cutoff) = cutoff {
            let doomed = "SELECT DISTINCT batch_key FROM logbook_entries WHERE seq <= ?";
            // A fixed subquery held in a local &str; the value is bound.
            // nosemgrep: navree-sql-built-from-strings
            sqlx::query(AssertSqlSafe(format!(
                "DELETE FROM logbook_tags WHERE batch_key IN ({doomed})"
            )))
            .bind(cutoff)
            .execute(&mut *tx)
            .await?;
            // A fixed subquery held in a local &str; the value is bound.
            // nosemgrep: navree-sql-built-from-strings
            deleted += sqlx::query(AssertSqlSafe(format!(
                "DELETE FROM logbook_entries WHERE batch_key IN ({doomed})"
            )))
            .bind(cutoff)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        tx.commit().await?;
        Ok(deleted)
    }

    async fn clear_all(&self) -> Result<u64, LogbookError> {
        let mut tx = self.pool.begin().await?;
        let deleted = sqlx::query("DELETE FROM logbook_entries")
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM logbook_tags")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(deleted)
    }
}

/// Adds one term's condition and arguments.
fn push_term(
    sql: &mut String,
    args: &mut SqliteArguments,
    term: &Term,
) -> Result<(), LogbookError> {
    match term {
        Term::Text(text) => {
            sql.push_str(" AND instr(e.search, ?) > 0");
            bind(args, text.to_lowercase())
        }
        Term::User(prefix) => prefix_on(sql, args, "e.user_id", prefix),
        Term::Batch(prefix) => prefix_on(sql, args, "e.batch_id", prefix),
        Term::Trace(prefix) => prefix_on(sql, args, "e.trace_id", prefix),
        Term::Tag(prefix) => {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM logbook_tags t WHERE t.batch_key = e.batch_key \
                 AND instr(lower(t.tag), ?) = 1)",
            );
            bind(args, prefix.to_ascii_lowercase())
        }
        Term::Status(StatusMatch::Exact(code)) => {
            sql.push_str(" AND e.status = ?");
            bind(args, i64::from(*code))
        }
        Term::Status(StatusMatch::Class(class)) => {
            sql.push_str(" AND e.status >= ? AND e.status < ?");
            bind(args, i64::from(*class) * 100)?;
            bind(args, (i64::from(*class) + 1) * 100)
        }
        Term::Route(route) => {
            sql.push_str(" AND e.route = ?");
            bind(args, route.clone())
        }
        Term::Type(_) => Ok(()),
        Term::Slow => {
            sql.push_str(" AND e.slow = 1");
            Ok(())
        }
        Term::Failed => {
            sql.push_str(" AND e.failed = 1");
            Ok(())
        }
        Term::Duration(comparison, ms) => {
            sql.push_str(match comparison {
                Comparison::Longer => " AND e.duration_ms > ?",
                Comparison::Shorter => " AND e.duration_ms < ?",
            });
            bind(args, *ms)
        }
    }
}

/// `column` starts with `prefix`, ASCII case-insensitively.
fn prefix_on(
    sql: &mut String,
    args: &mut SqliteArguments,
    column: &str,
    prefix: &str,
) -> Result<(), LogbookError> {
    sql.push_str(&format!(" AND instr(lower({column}), ?) = 1"));
    bind(args, prefix.to_ascii_lowercase())
}

fn bind<T>(args: &mut SqliteArguments, value: T) -> Result<(), LogbookError>
where
    T: for<'q> sqlx::Encode<'q, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
{
    args.add(value)
        .map_err(|error| LogbookError::Store(error.to_string()))
}

fn seq_arg(seq: u64) -> i64 {
    i64::try_from(seq).unwrap_or(i64::MAX)
}

/// Microseconds since the Unix epoch.
fn micros(at: DateTime<Utc>) -> i64 {
    at.timestamp_micros()
}

fn entry_from_row(row: &SqliteRow) -> Result<Entry, LogbookError> {
    let content: String = row.try_get("content")?;
    let content: Content = serde_json::from_str(&content)
        .map_err(|error| LogbookError::Store(format!("an entry does not decode: {error}")))?;
    let at: i64 = row.try_get("at")?;
    let tags: Option<String> = row.try_get("tags")?;
    let mut tags: Vec<String> = tags
        .unwrap_or_default()
        .split('\n')
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect();
    tags.sort();
    let seq: i64 = row.try_get("seq")?;
    Ok(Entry {
        id: row.try_get("id")?,
        seq: u64::try_from(seq).unwrap_or(0),
        batch_id: row.try_get("batch_id")?,
        at: Utc.timestamp_micros(at).single().unwrap_or_else(Utc::now),
        duration_ms: row.try_get("duration_ms")?,
        user_id: row.try_get("user_id")?,
        workspace_id: row.try_get("workspace_id")?,
        trace_id: row.try_get("trace_id")?,
        tags,
        failed: row.try_get("failed")?,
        slow: row.try_get("slow")?,
        content,
    })
}

impl LogbookStore for SqliteStore {
    fn insert(&self, entries: Vec<Entry>) -> BoxFuture<'_, Result<(), LogbookError>> {
        Box::pin(self.insert_all(entries))
    }

    fn add_tags(
        &self,
        batch_key: String,
        tags: Vec<String>,
    ) -> BoxFuture<'_, Result<(), LogbookError>> {
        Box::pin(self.add_all_tags(batch_key, tags))
    }

    fn list<'a>(&'a self, query: &'a ListQuery) -> BoxFuture<'a, Result<Vec<Entry>, LogbookError>> {
        Box::pin(self.list_entries(query))
    }

    fn entry<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Entry>, LogbookError>> {
        Box::pin(self.find(id))
    }

    fn batch<'a>(&'a self, batch_id: &'a str) -> BoxFuture<'a, Result<Vec<Entry>, LogbookError>> {
        Box::pin(self.batch_entries(batch_id))
    }

    fn count(&self) -> BoxFuture<'_, Result<u64, LogbookError>> {
        Box::pin(self.count_entries())
    }

    fn prune(
        &self,
        max_entries: usize,
        older_than: DateTime<Utc>,
    ) -> BoxFuture<'_, Result<u64, LogbookError>> {
        Box::pin(self.prune_entries(max_entries, older_than))
    }

    fn clear(&self) -> BoxFuture<'_, Result<u64, LogbookError>> {
        Box::pin(self.clear_all())
    }
}

impl std::fmt::Debug for SqliteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStore")
            .field("path", &self.path)
            .finish()
    }
}
