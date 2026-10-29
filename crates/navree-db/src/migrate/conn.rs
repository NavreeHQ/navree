//! What migrations run on: navree-db's pool ([`Conn`]) and transactions
//! ([`Tx`]).

use navree_kernel::BoxFuture;
use navree_schema::{Dialect, SchemaError, SchemaRow, SchemaValue};

use crate::engine::driver::{Executed, RowStream};
use crate::schema::{dialect, execute_on, query_on};
use crate::{
    Backend, Db, DbError, DbOptions, Executor, Pool, Row, SqliteOptions, Statement, Transaction,
    TxOptions,
};

/// A transaction a migration runs in.
pub(super) type Tx = Transaction;

/// The connection migrations run on.
#[derive(Clone)]
pub(super) struct Conn {
    pool: Pool,
    /// Whether the pool was opened for the migrations alone (and is closed
    /// afterwards).
    dedicated: bool,
}

impl Conn {
    /// The connections of `db`.
    pub(super) fn of(db: &Db) -> Conn {
        Conn {
            pool: db.pool().clone(),
            dedicated: false,
        }
    }

    /// A one-connection pool on the SQLite database at `url` with foreign
    /// keys off, for the migrations alone: rebuilding a table drops it, and
    /// with foreign keys on that would run the ON DELETE actions of the
    /// tables that reference it. Each migration's rows are checked
    /// (`PRAGMA foreign_key_check`) before it commits instead.
    ///
    /// The app's `database.sqlite` settings (journal mode, lock wait,
    /// pragmas) apply to it as to the app's own connections; only the
    /// foreign keys differ.
    pub(super) async fn sqlite(url: &str, sqlite: &SqliteOptions) -> Result<Conn, DbError> {
        let options = DbOptions {
            url: url.to_owned(),
            max_connections: 1,
            min_connections: 1,
            sqlite: SqliteOptions {
                foreign_keys: false,
                ..sqlite.clone()
            },
            ..DbOptions::default()
        };
        Ok(Conn {
            pool: Pool::connect(&options).await?,
            dedicated: true,
        })
    }

    /// Whether the connection is the migrations' own (SQLite, foreign keys
    /// off).
    pub(super) fn is_dedicated(&self) -> bool {
        self.dedicated
    }

    /// The database family.
    pub(super) fn dialect(&self) -> Dialect {
        dialect(Executor::backend(self))
    }

    /// Begins a transaction.
    ///
    /// A migration reads before it writes (does the table exist? which
    /// foreign keys does it have?), and the migrations table is written in
    /// the same transaction. On SQLite a plain `BEGIN` that upgrades to a
    /// write after a read fails with `database is locked` at once, without
    /// waiting for `busy_timeout`, when another connection (a worker, a
    /// scheduler tick) holds the write lock at that moment. So the
    /// transaction takes the write lock when it begins (`BEGIN IMMEDIATE`)
    /// and queues for it like any other writer.
    pub(super) async fn begin(&self) -> Result<Tx, DbError> {
        self.pool.begin_with(TxOptions::new().immediate()).await
    }

    /// Closes a dedicated pool.
    pub(super) async fn close(self) {
        if self.dedicated {
            self.pool.close().await;
        }
    }

    /// SQLite's `migrate:fresh`, on one connection taken out of the pool
    /// with `PRAGMA foreign_keys = OFF`.
    ///
    /// With foreign keys on, `DROP TABLE` first runs an implicit `DELETE`,
    /// and its `ON DELETE CASCADE` actions fail once a table they touch
    /// references a table already dropped ("no such table: main.roles" for
    /// navree-access's `role_permissions`), whatever `defer_foreign_keys`
    /// says. The pragma has no effect inside a transaction, so it is set
    /// before `BEGIN`, and the connection is closed afterwards (also on
    /// error) rather than going back to the pool with foreign keys off.
    pub(super) async fn drop_all_sqlite_tables(&self) -> Result<(), DbError> {
        #[cfg(feature = "sqlite")]
        if let crate::engine::driver::DriverPool::Sqlite(pool) = self.pool.driver() {
            return drop_all_sqlite_tables(pool).await;
        }
        Err(DbError::unsupported(
            "migrate:fresh on SQLite needs navree-db's SQLite driver (the `sqlite` feature)",
        ))
    }
}

#[cfg(feature = "sqlite")]
async fn drop_all_sqlite_tables(pool: &sqlx::SqlitePool) -> Result<(), DbError> {
    use sqlx::{AssertSqlSafe, Connection as _};

    let error = |error: sqlx::Error| DbError::from_sqlx(error, Some(Backend::Sqlite));
    let mut dedicated = pool.acquire().await.map_err(error)?;
    dedicated.close_on_drop();
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *dedicated)
        .await
        .map_err(error)?;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&mut *dedicated)
    .await
    .map_err(error)?;
    // `IMMEDIATE`: the write lock is taken (waiting up to `busy_timeout`)
    // before the first `DROP`, not upgraded to after reading.
    let mut tx = dedicated
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(error)?;
    for table in &tables {
        sqlx::query(AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {}",
            super::quote(table)
        )))
        .execute(&mut *tx)
        .await
        .map_err(error)?;
    }
    tx.commit().await.map_err(error)?;
    dedicated.close().await.map_err(error)
}

/// Statements on the pool.
impl Executor for Conn {
    fn backend(&self) -> Backend {
        self.pool.backend()
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        self.pool.execute(statement)
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        self.pool.fetch_all(statement)
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        self.pool.fetch_optional(statement)
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        self.pool.fetch(statement)
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        self.pool.execute_script(sql)
    }
}

impl navree_schema::SchemaConnection for Conn {
    fn backend(&self) -> Dialect {
        dialect(Executor::backend(self))
    }

    fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>> {
        Box::pin(execute_on(self, sql))
    }

    fn query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [SchemaValue],
    ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>> {
        Box::pin(query_on(self, sql, params))
    }
}
