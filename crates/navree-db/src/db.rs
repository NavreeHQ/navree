//! [`Db`]: the shared connection pool.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use navree_kernel::{AppContext, WeakAppContext};
use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbBackend, DbErr};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::options::url_password;
use crate::{ChangeObserver, DbOptions, Reader, Uow};

/// How long SQLite waits for a lock before failing with `SQLITE_BUSY`.
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

/// The database: a cheap, cloneable handle to the connection pool.
///
/// Provided (and exported) by `DatabaseModule`; inject `Arc<Db>` or take a
/// [`Uow`](crate::Uow) per request. SQLite connections use WAL,
/// `busy_timeout = 5000` and `foreign_keys = ON`.
#[derive(Clone)]
pub struct Db {
    inner: Arc<DbInner>,
}

struct DbInner {
    /// Shared by every handle on the same pool ([`Db::with_app`] included),
    /// so [`Db::reader`] can tell whether a unit of work uses this pool.
    conn: Arc<DatabaseConnection>,
    /// The app whose `dyn ChangeObserver` multi-bindings see every commit.
    app: Option<WeakAppContext>,
    observers: OnceCell<Vec<Arc<dyn ChangeObserver>>>,
}

impl Db {
    /// Opens a pool for `options` (creating the directory of a SQLite file).
    pub async fn connect(options: &DbOptions) -> Result<Db, DbErr> {
        let mut connect = ConnectOptions::new(options.url.clone());
        connect
            .max_connections(options.max_connections)
            .min_connections(options.min_connections)
            .connect_timeout(Duration::from_secs(options.connect_timeout_secs))
            .acquire_timeout(Duration::from_secs(options.connect_timeout_secs.max(1) * 4))
            .sqlx_logging(false);
        if is_sqlite_url(&options.url) {
            create_sqlite_parent_dir(&options.url).await?;
            connect.map_sqlx_sqlite_opts(|o| {
                o.journal_mode(SqliteJournalMode::Wal)
                    .busy_timeout(SQLITE_BUSY_TIMEOUT)
                    .foreign_keys(true)
            });
        }
        let mut conn = Database::connect(connect)
            .await
            .map_err(|error| connect_error(options, &error))?;
        install_query_log(&mut conn, options.log_queries, options.slow_query_ms);
        Ok(Self::from_connection(conn))
    }

    /// Wraps an existing SeaORM connection (no change observers).
    pub fn from_connection(conn: DatabaseConnection) -> Db {
        Self::build(Arc::new(conn), None)
    }

    /// The same pool, reporting commits to the `dyn ChangeObserver`
    /// multi-bindings of `app`.
    pub(crate) fn with_app(&self, app: WeakAppContext) -> Db {
        Self::build(self.inner.conn.clone(), Some(app))
    }

    fn build(conn: Arc<DatabaseConnection>, app: Option<WeakAppContext>) -> Db {
        Self {
            inner: Arc::new(DbInner {
                conn,
                app,
                observers: OnceCell::new(),
            }),
        }
    }

    /// The SeaORM connection (pool).
    pub fn conn(&self) -> &DatabaseConnection {
        &self.inner.conn
    }

    /// The database backend.
    pub fn backend(&self) -> DbBackend {
        self.inner.conn.get_database_backend()
    }

    /// The app this pool belongs to, while it runs: set for the `Db` that
    /// `DatabaseModule` provides, `None` for one made with
    /// [`connect`](Db::connect) or [`from_connection`](Db::from_connection)
    /// and once the app is dropped. Code that receives only a `Db` (or a
    /// [`Uow`], through [`Uow::db`]) uses it to reach app services such as
    /// the `dyn Clock`.
    pub fn app(&self) -> Option<AppContext> {
        self.inner.app.as_ref().and_then(WeakAppContext::upgrade)
    }

    /// The connection to read with: the open transaction of the task's
    /// unit of work on this pool ([`Uow::current`], e.g. the request's once
    /// a record check or a write began it), else the pool.
    ///
    /// Use it for reads in services that take no `&Uow`. On SQLite an open
    /// unit of work holds the write lock on one pool connection; waiting
    /// for a second connection while holding it deadlocks the pool as soon
    /// as about `max_connections` requests run together (the others hold
    /// connections while they wait for the lock, and fail with "database
    /// is locked"). Reading through the transaction also sees its own
    /// uncommitted writes. Drop the reader before the unit of work commits
    /// (use it as a temporary: `.one(&db.reader().await)`).
    pub async fn reader(&self) -> Reader<'_> {
        if let Some(uow) = Uow::current()
            && Arc::ptr_eq(&uow.db().inner.conn, &self.inner.conn)
            && let Some(tx) = uow.open_tx().await
        {
            return Reader::Transaction(tx);
        }
        Reader::Pool(&self.inner.conn)
    }

    /// A new, lazy unit of work for `actor` (jobs, commands, tests; HTTP
    /// requests get theirs from the `DatabaseModule` layer).
    pub async fn uow(&self, actor: Option<Uuid>, request_id: Option<String>) -> Uow {
        Uow::new(self.clone(), actor, request_id)
    }

    /// Runs `f` in a new unit of work (no actor): commits when it returns
    /// `Ok` (running change observers and after-commit hooks), rolls back
    /// when it returns `Err`. While `f` runs, the unit of work is
    /// [`Uow::current`].
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) -> Result<(), sea_orm::DbErr> {
    /// use sea_orm::ConnectionTrait;
    ///
    /// let rows = db
    ///     .transaction(|uow| async move {
    ///         let tx = uow.tx().await?;
    ///         let result = tx.execute_unprepared("DELETE FROM sessions").await?;
    ///         Ok(result.rows_affected())
    ///     })
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn transaction<F, Fut, T>(&self, f: F) -> Result<T, DbErr>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, DbErr>>,
    {
        let uow = self.uow(None, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree_db::uow", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    /// The app's change observers (resolved once).
    pub(crate) async fn observers(&self) -> Result<&[Arc<dyn ChangeObserver>], DbErr> {
        let observers = self
            .inner
            .observers
            .get_or_try_init(|| async {
                let Some(app) = self.inner.app.as_ref() else {
                    return Ok(Vec::new());
                };
                let Some(ctx) = app.upgrade() else {
                    return Err(DbErr::Custom(
                        "the app owning this database has been dropped".to_owned(),
                    ));
                };
                ctx.get_all::<dyn ChangeObserver>()
                    .await
                    .map_err(|e| DbErr::Custom(format!("cannot resolve change observers: {e}")))
            })
            .await?;
        Ok(observers)
    }
}

impl fmt::Debug for Db {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Db")
            .field("backend", &self.backend())
            .finish_non_exhaustive()
    }
}

/// Logs statements through `tracing` (target `navree_db::query`): every
/// statement at debug level when `log_queries`, statements slower than
/// `slow_query_ms` at warn level. Only the SQL text is logged, never the
/// bound values. SeaORM does not time `execute_unprepared` statements, so
/// those are not logged.
fn install_query_log(conn: &mut DatabaseConnection, log_queries: bool, slow_query_ms: u64) {
    if !log_queries && slow_query_ms == 0 {
        return;
    }
    let slow = (slow_query_ms > 0).then(|| Duration::from_millis(slow_query_ms));
    conn.set_metric_callback(move |info| {
        let elapsed_ms = info.elapsed.as_secs_f64() * 1000.0;
        let sql = info.statement.sql.as_str();
        if slow.is_some_and(|slow| info.elapsed >= slow) {
            tracing::warn!(
                target: "navree_db::query",
                sql,
                elapsed_ms,
                failed = info.failed,
                "slow query"
            );
        } else if log_queries {
            tracing::debug!(
                target: "navree_db::query",
                sql,
                elapsed_ms,
                failed = info.failed,
                "query"
            );
        }
    });
}

/// A connection error that names the database without its password (the
/// driver's own message may echo the URL).
fn connect_error(options: &DbOptions, error: &DbErr) -> DbErr {
    let mut message = error
        .to_string()
        .replace(&options.url, &options.redacted_url());
    if let Some(password) = url_password(&options.url) {
        message = message.replace(password, "***");
    }
    let hint = if options.url.starts_with("postgres") && !cfg!(feature = "postgres") {
        " (enable the `postgres` feature of navree-db)"
    } else {
        ""
    };
    DbErr::Custom(format!(
        "cannot connect to {}: {message}{hint}",
        options.redacted_url()
    ))
}

fn is_sqlite_url(url: &str) -> bool {
    url.starts_with("sqlite:")
}

/// Creates the directory of a SQLite database file, so
/// `sqlite://storage/app.db?mode=rwc` works on a fresh checkout.
async fn create_sqlite_parent_dir(url: &str) -> Result<(), DbErr> {
    let Ok(options) = url.parse::<SqliteConnectOptions>() else {
        return Ok(());
    };
    let file = options.get_filename();
    if file.as_os_str().is_empty() || file.as_os_str() == ":memory:" || url.contains("mode=memory")
    {
        return Ok(());
    }
    match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                DbErr::Custom(format!(
                    "cannot create the database directory {}: {error}",
                    parent.display()
                ))
            })
        }
        _ => Ok(()),
    }
}
