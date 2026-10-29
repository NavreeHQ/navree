//! What migrations run on: navree-db's pool and transactions ([`Conn`],
//! [`Tx`]). SeaORM migrations, until every crate's are written with the
//! schema builder, run on a SeaORM view of the same connections; a `Db`
//! made from a scripted SeaORM connection (tests) runs everything through
//! that connection.

use navree_kernel::BoxFuture;
use navree_schema::{Dialect, SchemaError, SchemaRow, SchemaValue};
use sea_orm::{DatabaseConnection, DatabaseTransaction};

use crate::compat::SeaExecutor;
use crate::engine::driver::{Executed, RowStream};
use crate::schema::{dialect, execute_on, query_on};
use crate::{
    Backend, Db, DbError, DbOptions, Executor, Pool, Row, SqliteOptions, Statement, Transaction,
    TxOptions,
};

/// The connection migrations run on.
#[derive(Clone)]
pub(super) struct Conn {
    /// navree-db's pool; `None` for a `Db` made from a scripted SeaORM
    /// connection.
    pool: Option<Pool>,
    /// The SeaORM connection on the same connections, for SeaORM
    /// migrations.
    sea: DatabaseConnection,
    /// Whether the pool was opened for the migrations alone (and is closed
    /// afterwards).
    dedicated: bool,
}

/// A transaction a migration runs in.
pub(super) enum Tx {
    /// navree-db's own.
    Native(Transaction),
    /// SeaORM's, for a SeaORM migration (or on a scripted connection).
    Sea(DatabaseTransaction),
}

impl Conn {
    /// The connections of `db`.
    pub(super) fn of(db: &Db) -> Conn {
        Conn {
            pool: db.pool().cloned(),
            sea: db.conn().clone(),
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
        let pool = Pool::connect(&options).await?;
        let sea = crate::db::sea_view(pool.driver());
        Ok(Conn {
            pool: Some(pool),
            sea,
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

    /// The SeaORM connection on the same connections.
    pub(super) fn sea(&self) -> &DatabaseConnection {
        &self.sea
    }

    /// Begins a transaction: navree-db's own (SeaORM's on a scripted
    /// connection).
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
        match &self.pool {
            Some(pool) => Ok(Tx::Native(
                pool.begin_with(TxOptions::new().immediate()).await?,
            )),
            None => self.begin_sea().await,
        }
    }

    /// Begins a SeaORM transaction on the same connections, for a SeaORM
    /// migration (`IMMEDIATE` on SQLite, as [`begin`](Conn::begin)).
    pub(super) async fn begin_sea(&self) -> Result<Tx, DbError> {
        Ok(Tx::Sea(crate::db::begin_for_write(&self.sea).await?))
    }

    /// Closes a dedicated pool.
    pub(super) async fn close(self) {
        if self.dedicated
            && let Some(pool) = &self.pool
        {
            pool.close().await;
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
        if let Some(pool) = &self.pool
            && let crate::engine::driver::DriverPool::Sqlite(pool) = pool.driver()
        {
            return drop_all_sqlite_tables(pool).await;
        }
        Err(DbError::unsupported(
            "migrate:fresh on SQLite needs navree-db's own SQLite pool",
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

impl Tx {
    /// The SeaORM transaction, for a SeaORM migration.
    pub(super) fn sea(&self) -> Option<&DatabaseTransaction> {
        match self {
            Tx::Native(_) => None,
            Tx::Sea(tx) => Some(tx),
        }
    }

    /// Commits.
    pub(super) async fn commit(self) -> Result<(), DbError> {
        match self {
            Tx::Native(tx) => tx.commit().await,
            Tx::Sea(tx) => Ok(tx.commit().await?),
        }
    }

    /// Rolls back.
    pub(super) async fn rollback(self) -> Result<(), DbError> {
        match self {
            Tx::Native(tx) => tx.rollback().await,
            Tx::Sea(tx) => Ok(tx.rollback().await?),
        }
    }
}

/// Statements on the pool, or through the scripted connection.
impl Executor for Conn {
    fn backend(&self) -> Backend {
        match &self.pool {
            Some(pool) => pool.backend(),
            None => SeaExecutor::new(&self.sea).backend(),
        }
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        match &self.pool {
            Some(pool) => pool.execute(statement),
            None => Box::pin(async move { SeaExecutor::new(&self.sea).execute(statement).await }),
        }
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        match &self.pool {
            Some(pool) => pool.fetch_all(statement),
            None => Box::pin(async move { SeaExecutor::new(&self.sea).fetch_all(statement).await }),
        }
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        match &self.pool {
            Some(pool) => pool.fetch_optional(statement),
            None => {
                Box::pin(async move { SeaExecutor::new(&self.sea).fetch_optional(statement).await })
            }
        }
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        match &self.pool {
            Some(pool) => pool.fetch(statement),
            None => Box::pin(async move { SeaExecutor::new(&self.sea).fetch(statement).await }),
        }
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        match &self.pool {
            Some(pool) => pool.execute_script(sql),
            None => Box::pin(async move { SeaExecutor::new(&self.sea).execute_script(sql).await }),
        }
    }
}

/// Statements in the transaction.
impl Executor for Tx {
    fn backend(&self) -> Backend {
        match self {
            Tx::Native(tx) => tx.backend(),
            Tx::Sea(tx) => SeaExecutor::new(tx).backend(),
        }
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        match self {
            Tx::Native(tx) => tx.execute(statement),
            Tx::Sea(tx) => Box::pin(async move { SeaExecutor::new(tx).execute(statement).await }),
        }
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        match self {
            Tx::Native(tx) => tx.fetch_all(statement),
            Tx::Sea(tx) => Box::pin(async move { SeaExecutor::new(tx).fetch_all(statement).await }),
        }
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        match self {
            Tx::Native(tx) => tx.fetch_optional(statement),
            Tx::Sea(tx) => {
                Box::pin(async move { SeaExecutor::new(tx).fetch_optional(statement).await })
            }
        }
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        match self {
            Tx::Native(tx) => tx.fetch(statement),
            Tx::Sea(tx) => Box::pin(async move { SeaExecutor::new(tx).fetch(statement).await }),
        }
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        match self {
            Tx::Native(tx) => tx.execute_script(sql),
            Tx::Sea(tx) => Box::pin(async move { SeaExecutor::new(tx).execute_script(sql).await }),
        }
    }
}

macro_rules! schema_connection {
    ($($ty:ty),*) => {
        $(
            impl navree_schema::SchemaConnection for $ty {
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
        )*
    };
}

schema_connection!(Conn, Tx);
