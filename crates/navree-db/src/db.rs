//! [`Db`]: the shared connection pool.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_config::ConfigSection;
use navree_kernel::{AppContext, WeakAppContext};
use navree_telemetry::{EventKind, QueryExecuted, Telemetry, TelemetryEvent};
use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr, Statement,
};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::options::{driver_url, redact_url, scheme, url_password};
use crate::queries::Watchers;
use crate::{ChangeObserver, DbOptions, LoggedQuery, Reader, SqliteOptions, Uow};

/// The database: a cheap, cloneable handle to the connection pool (and to
/// the pools of its read replicas).
///
/// Provided (and exported) by `DatabaseModule`; inject `Arc<Db>` or take a
/// [`Uow`](crate::Uow) per request. SQLite connections use WAL,
/// `busy_timeout = 5000` and `foreign_keys = ON` unless `database.sqlite`
/// says otherwise (see [`SqliteOptions`](crate::SqliteOptions)). MySQL and
/// MariaDB connections (`mysql://` or `mariadb://`, the `mysql` feature)
/// run in the UTC time zone with `utf8mb4`, over TLS when the server offers
/// it, in strict SQL mode unless `database.mysql` says otherwise, and at
/// its `isolation_level` when set (see
/// [`MySqlOptions`](crate::MySqlOptions)). PostgreSQL connections take the
/// session settings of `database.postgres`
/// ([`PostgresOptions`](crate::PostgresOptions)).
///
/// **Read replicas.** With `read_urls`, reads outside a transaction
/// ([`reader`](Db::reader)) go to the replicas in turn; writes and
/// transactions use the writer. A task reads from the writer inside
/// [`Db::using_writer`], and (with `sticky`, the default) once its unit of
/// work has written, so a request sees its own writes while replicas catch
/// up.
#[derive(Clone)]
pub struct Db {
    inner: Arc<DbInner>,
}

struct DbInner {
    /// Shared by every handle on the same pool ([`Db::with_app`] included),
    /// so [`Db::reader`] can tell whether a unit of work uses this pool.
    conn: Arc<DatabaseConnection>,
    /// The pools of the read replicas.
    replicas: Arc<Vec<DatabaseConnection>>,
    /// The replica the next read goes to.
    next_replica: Arc<AtomicUsize>,
    /// Reads follow a unit of work's writes to the writer.
    sticky: bool,
    /// The connection's name: `default`, or a named connection's.
    name: Arc<str>,
    /// The statement listeners and hooks, shared with the pools' timing
    /// callbacks.
    watchers: Arc<Watchers>,
    /// The direct connection (`direct_url`), if configured.
    direct: Option<Arc<Direct>>,
    /// The app whose `dyn ChangeObserver` multi-bindings see every commit.
    app: Option<WeakAppContext>,
    observers: OnceCell<Vec<Arc<dyn ChangeObserver>>>,
}

/// The direct connection of `direct_url`, opened on first use.
struct Direct {
    options: DbOptions,
    telemetry: Telemetry,
    db: OnceCell<Db>,
}

tokio::task_local! {
    /// Set while [`Db::using_writer`] runs a future.
    static USING_WRITER: bool;
}

/// The name of the main connection (the `database` section itself, beside
/// the named ones of `[database.connections.<name>]`).
pub const DEFAULT_CONNECTION: &str = "default";

/// The connections of a pool: how many are open, how many idle, and the
/// most the pool opens ([`Db::pool_stats`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoolStats {
    /// Connections open (idle or in use).
    pub size: u32,
    /// Open connections nobody uses.
    pub idle: usize,
    /// The most connections the pool opens (`max_connections`).
    pub max: u32,
}

impl Db {
    /// Opens a pool for `options` (creating the directory of a SQLite file).
    ///
    /// The URL's scheme picks the driver: `sqlite:`, `postgres://` (the
    /// `postgres` feature), `mysql://` or `mariadb://` (the `mysql`
    /// feature). A URL whose driver is not compiled in fails with a message
    /// naming the feature to turn on. `options` are checked first, as the
    /// `database` configuration section is (an invalid pool size or SQL
    /// mode fails before anything connects), so options built in code get
    /// the same errors as configured ones.
    ///
    /// ```no_run
    /// # async fn demo() -> Result<(), sea_orm::DbErr> {
    /// use navree_db::{Db, DbOptions};
    ///
    /// let db = Db::connect(&DbOptions {
    ///     url: "mariadb://crm:secret@127.0.0.1:3306/crm".into(),
    ///     ..DbOptions::default()
    /// })
    /// .await?;
    /// assert_eq!(db.backend(), sea_orm::DbBackend::MySql);
    /// # Ok(()) }
    /// ```
    pub async fn connect(options: &DbOptions) -> Result<Db, DbErr> {
        Self::connect_with_telemetry(options, Telemetry::disabled()).await
    }

    /// Opens a pool like [`connect`](Db::connect) that reports every
    /// statement SeaORM times to `telemetry` as a `QueryExecuted` event (the
    /// SQL with its placeholders, never the bound values; the connection is
    /// the URL without its password). `DatabaseModule` connects this way
    /// with the app's registry.
    pub async fn connect_with_telemetry(
        options: &DbOptions,
        telemetry: Telemetry,
    ) -> Result<Db, DbErr> {
        Self::connect_named(DEFAULT_CONNECTION, options, telemetry).await
    }

    /// Opens the pool of connection `name` (a named connection of
    /// [`DbConnections`](crate::DbConnections)), and the pools of its read
    /// replicas.
    pub async fn connect_named(
        name: &str,
        options: &DbOptions,
        telemetry: Telemetry,
    ) -> Result<Db, DbErr> {
        options
            .validate()
            .map_err(|problems| DbErr::Custom(format!("invalid database options: {problems}")))?;
        let watchers = Arc::new(Watchers::new(Arc::from(name)));
        Self::connect_watched(options, telemetry, watchers).await
    }

    /// Opens the pools of `options` (writer, replicas), reporting to
    /// `watchers`.
    async fn connect_watched(
        options: &DbOptions,
        telemetry: Telemetry,
        watchers: Arc<Watchers>,
    ) -> Result<Db, DbErr> {
        let conn = open_pool(&options.url, options, telemetry.clone(), &watchers).await?;
        let mut replicas = Vec::with_capacity(options.read_urls.len());
        for url in &options.read_urls {
            replicas.push(open_pool(url, options, telemetry.clone(), &watchers).await?);
        }
        let direct = options.direct().map(|options| {
            Arc::new(Direct {
                options,
                telemetry,
                db: OnceCell::new(),
            })
        });
        Ok(Self::build(
            Arc::new(conn),
            Arc::new(replicas),
            options.sticky,
            watchers,
            direct,
            None,
        ))
    }

    /// The direct connection to this database (`direct_url`), which
    /// bypasses a transaction-mode pooler in front of `url`: for
    /// session-level work such as advisory locks, `LISTEN` and migrations
    /// (which run on it). Opened on first use and shared from then on; its
    /// statements reach this connection's listeners. Without `direct_url`
    /// it is this connection.
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) -> Result<(), sea_orm::DbErr> {
    /// use sea_orm::ConnectionTrait;
    ///
    /// let direct = db.direct().await?;
    /// direct.conn().execute_unprepared("LISTEN deals").await?;
    /// # Ok(()) }
    /// ```
    pub async fn direct(&self) -> Result<Db, DbErr> {
        let Some(direct) = &self.inner.direct else {
            return Ok(self.clone());
        };
        let db = direct
            .db
            .get_or_try_init(|| async {
                let db = Self::connect_watched(
                    &direct.options,
                    direct.telemetry.clone(),
                    self.inner.watchers.clone(),
                )
                .await?;
                Ok::<_, DbErr>(match &self.inner.app {
                    Some(app) => db.with_app(app.clone()),
                    None => db,
                })
            })
            .await?;
        Ok(db.clone())
    }

    /// Whether this connection has a direct connection of its own
    /// (`direct_url`).
    pub fn has_direct(&self) -> bool {
        self.inner.direct.is_some()
    }

    /// Wraps an existing SeaORM connection (no replicas, no change
    /// observers). Its statements are not watched: query listeners, query
    /// logs and unit-of-work statistics see only pools opened by
    /// [`connect`](Db::connect) and its siblings.
    pub fn from_connection(conn: DatabaseConnection) -> Db {
        Self::build(
            Arc::new(conn),
            Arc::new(Vec::new()),
            true,
            Arc::new(Watchers::new(Arc::from(DEFAULT_CONNECTION))),
            None,
            None,
        )
    }

    /// The same pool, reporting commits to the `dyn ChangeObserver`
    /// multi-bindings of `app`.
    pub(crate) fn with_app(&self, app: WeakAppContext) -> Db {
        Self::build(
            self.inner.conn.clone(),
            self.inner.replicas.clone(),
            self.inner.sticky,
            self.inner.watchers.clone(),
            self.inner.direct.clone(),
            Some(app),
        )
    }

    fn build(
        conn: Arc<DatabaseConnection>,
        replicas: Arc<Vec<DatabaseConnection>>,
        sticky: bool,
        watchers: Arc<Watchers>,
        direct: Option<Arc<Direct>>,
        app: Option<WeakAppContext>,
    ) -> Db {
        Self {
            inner: Arc::new(DbInner {
                conn,
                replicas,
                next_replica: Arc::new(AtomicUsize::new(0)),
                sticky,
                name: watchers.name().clone(),
                watchers,
                direct,
                app,
                observers: OnceCell::new(),
            }),
        }
    }

    /// Calls `listener` after every statement this connection runs (its
    /// writer and replicas; named connections have their own listeners),
    /// with the SQL, the bound values, the time it took and whether it
    /// failed. Listeners run inside the statement's task, so keep them
    /// short (spawn slow work). SeaORM does not time
    /// `execute_unprepared`, so those statements are not seen.
    ///
    /// ```no_run
    /// # fn demo(db: navree_db::Db) {
    /// db.on_query(|query| {
    ///     if query.duration > std::time::Duration::from_millis(200) {
    ///         tracing::warn!(sql = %query.sql, "slow statement");
    ///     }
    /// });
    /// # }
    /// ```
    pub fn on_query(&self, listener: impl Fn(&LoggedQuery) + Send + Sync + 'static) {
        self.inner.watchers.add_listener(Arc::new(listener));
    }

    /// Calls `hook` with the SQL of every statement the ORM sends through
    /// this connection (its `Db` and its units of work), right before the
    /// statement runs; and before the statements of code that announces
    /// them with [`run_before_executing`](Db::run_before_executing).
    pub fn before_executing(&self, hook: impl Fn(&str) + Send + Sync + 'static) {
        self.inner.watchers.add_before_executing(Arc::new(hook));
    }

    /// Runs the [`before_executing`](Db::before_executing) hooks for `sql`
    /// (for code that sends statements itself, as the ORM does).
    pub fn run_before_executing(&self, sql: &str) {
        self.inner.watchers.before_executing(sql);
    }

    /// Calls `hook` right before a transaction of this connection begins:
    /// a unit of work's (requests, [`Db::transaction`]) and the ORM's own.
    pub fn before_starting_transaction(&self, hook: impl Fn() + Send + Sync + 'static) {
        self.inner.watchers.add_before_transaction(Arc::new(hook));
    }

    /// Runs the [`before_starting_transaction`](Db::before_starting_transaction)
    /// hooks (for code that begins transactions itself, as the ORM does).
    pub fn run_before_starting_transaction(&self) {
        self.inner.watchers.before_transaction();
    }

    /// Calls `handler` once per unit of work whose statements on this
    /// connection take longer than `threshold` in total, with the unit of
    /// work and the total so far: the statements run while it is
    /// [current](crate::Uow::current) (a request's, [`Db::transaction`]).
    /// [`Uow::allow_duration_handlers_again`](crate::Uow::allow_duration_handlers_again)
    /// lets it run again.
    ///
    /// ```no_run
    /// # fn demo(db: navree_db::Db) {
    /// db.when_querying_for_longer_than(std::time::Duration::from_secs(2), |uow, total| {
    ///     tracing::warn!(request_id = uow.request_id(), ?total, "slow database work");
    /// });
    /// # }
    /// ```
    pub fn when_querying_for_longer_than(
        &self,
        threshold: Duration,
        handler: impl Fn(&Uow, Duration) + Send + Sync + 'static,
    ) {
        self.inner
            .watchers
            .add_duration_handler(threshold, Arc::new(handler));
    }

    /// Whether `watchers` watch this connection's statements.
    pub(crate) fn is_watched_by(&self, watchers: &Arc<Watchers>) -> bool {
        Arc::ptr_eq(&self.inner.watchers, watchers)
    }

    /// The SeaORM connection (pool) of the writer.
    pub fn conn(&self) -> &DatabaseConnection {
        &self.inner.conn
    }

    /// The connection's name: `default`, or the name of a named connection
    /// (`[database.connections.<name>]`).
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Whether this is the default connection.
    pub fn is_default(&self) -> bool {
        self.name() == DEFAULT_CONNECTION
    }

    /// Connection `name` of the app this connection belongs to: this one
    /// when `name` is its own, else that connection from the app's
    /// [`DbConnections`](crate::DbConnections) (opened on first use).
    ///
    /// Fails for a name the app does not configure, and for any other name
    /// when this `Db` belongs to no app (one made with
    /// [`connect`](Db::connect)): connect that database yourself.
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) -> Result<(), sea_orm::DbErr> {
    /// let analytics = db.connection("analytics").await?;
    /// assert_eq!(analytics.name(), "analytics");
    /// # Ok(()) }
    /// ```
    pub async fn connection(&self, name: &str) -> Result<Db, DbErr> {
        if name == self.name() {
            return Ok(self.clone());
        }
        let Some(app) = self.app() else {
            return Err(DbErr::Custom(format!(
                "no database connection named `{name}`: this connection (`{}`) belongs to no app that configures others",
                self.name()
            )));
        };
        let connections = app
            .get::<crate::DbConnections>()
            .await
            .map_err(|error| DbErr::Custom(format!("database connection `{name}`: {error}")))?;
        Ok(Db::clone(&*connections.get(name).await?))
    }

    /// How many read replicas the connection has.
    pub fn replicas(&self) -> usize {
        self.inner.replicas.len()
    }

    /// The pool a read outside a transaction uses: the next read replica
    /// in turn, or the writer when there is none or the task reads from the
    /// writer ([`Db::using_writer`]).
    pub fn read_conn(&self) -> &DatabaseConnection {
        let replicas = &self.inner.replicas;
        if replicas.is_empty() || Self::reads_from_writer() {
            return &self.inner.conn;
        }
        let at = self.inner.next_replica.fetch_add(1, Ordering::Relaxed) % replicas.len();
        &replicas[at]
    }

    /// Runs `future` with every read of the task going to the writer, even
    /// where replicas exist: for reads that must see a write that just
    /// happened outside the task's unit of work.
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) {
    /// use navree_db::Db;
    ///
    /// let fresh = Db::using_writer(async { db.reader().await.in_transaction() }).await;
    /// # let _ = fresh;
    /// # }
    /// ```
    pub async fn using_writer<F: Future>(future: F) -> F::Output {
        USING_WRITER.scope(true, future).await
    }

    /// Whether the running task reads from the writer
    /// ([`Db::using_writer`]).
    pub fn reads_from_writer() -> bool {
        USING_WRITER.try_with(|writer| *writer).unwrap_or(false)
    }

    /// Closes the pools (the writer's and the replicas'). Statements fail
    /// from then on; a pool replaces a connection it lost by itself, so
    /// there is no reconnecting by hand.
    pub async fn close(&self) -> Result<(), DbErr> {
        self.inner.conn.close_by_ref().await?;
        for replica in self.inner.replicas.iter() {
            replica.close_by_ref().await?;
        }
        if let Some(direct) = self.inner.direct.as_ref().and_then(|direct| direct.db.get()) {
            Box::pin(direct.close()).await?;
        }
        Ok(())
    }

    /// The writer pool's connections: open, idle and the most it opens.
    pub fn pool_stats(&self) -> PoolStats {
        pool_stats(&self.inner.conn)
    }

    /// The number of connections the database server has open (from every
    /// client): `pg_stat_activity` on PostgreSQL, `Threads_connected` on
    /// MySQL; `None` on SQLite, which has no server. For `db:monitor`.
    pub async fn open_connections(&self) -> Result<Option<u64>, DbErr> {
        let sql = match self.backend() {
            DbBackend::Postgres => "SELECT COUNT(*) FROM pg_stat_activity",
            DbBackend::MySql => {
                "SELECT CAST(VARIABLE_VALUE AS UNSIGNED) FROM performance_schema.global_status WHERE VARIABLE_NAME = 'Threads_connected'"
            }
            _ => return Ok(None),
        };
        let row = self.conn().query_one_raw(Statement::from_string(self.backend(), sql)).await?;
        let count = match row {
            Some(row) => row
                .try_get_by_index::<i64>(0)
                .map(|count| count.max(0).cast_unsigned())
                .or_else(|_| row.try_get_by_index::<u64>(0))?,
            None => 0,
        };
        Ok(Some(count))
    }

    /// The server's version: `17.2` (PostgreSQL), `8.4.3` or
    /// `11.4.2-MariaDB` (MySQL, MariaDB), `3.47.0` (SQLite).
    pub async fn server_version(&self) -> Result<String, DbErr> {
        let sql = match self.backend() {
            DbBackend::Postgres => "SELECT current_setting('server_version')",
            DbBackend::MySql => "SELECT VERSION()",
            _ => "SELECT sqlite_version()",
        };
        self.text(sql).await
    }

    /// The name of the database the connection uses (`main` on SQLite).
    pub async fn database_name(&self) -> Result<String, DbErr> {
        let sql = match self.backend() {
            DbBackend::Postgres => "SELECT current_database()",
            DbBackend::MySql => "SELECT DATABASE()",
            _ => return Ok("main".to_owned()),
        };
        self.text(sql).await
    }

    /// Whether the server is MariaDB (which speaks MySQL's protocol).
    pub async fn is_mariadb(&self) -> Result<bool, DbErr> {
        if self.backend() != DbBackend::MySql {
            return Ok(false);
        }
        Ok(self.server_version().await?.to_ascii_lowercase().contains("mariadb"))
    }

    /// Whether the database has the TimescaleDB extension installed.
    pub async fn has_timescaledb(&self) -> Result<bool, DbErr> {
        if self.backend() != DbBackend::Postgres {
            return Ok(false);
        }
        let row = self
            .conn()
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT COUNT(*) FROM pg_extension WHERE extname = 'timescaledb'",
            ))
            .await?;
        Ok(row
            .map(|row| row.try_get_by_index::<i64>(0))
            .transpose()?
            .unwrap_or(0)
            > 0)
    }

    /// The text in the first column of the first row of `sql`.
    async fn text(&self, sql: &str) -> Result<String, DbErr> {
        let row = self
            .conn()
            .query_one_raw(Statement::from_string(self.backend(), sql))
            .await?
            .ok_or_else(|| DbErr::RecordNotFound(format!("`{sql}` returned no row")))?;
        Ok(row.try_get_by_index::<String>(0)?)
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
    ///
    /// With read replicas, a read outside a transaction goes to a replica
    /// ([`read_conn`](Db::read_conn)), unless the task reads from the
    /// writer ([`Db::using_writer`]) or, with `sticky`, its unit of work
    /// has written ([`Uow::has_modified_records`]).
    pub async fn reader(&self) -> Reader<'_> {
        let current = Uow::current().filter(|uow| Arc::ptr_eq(&uow.db().inner.conn, &self.inner.conn));
        if let Some(uow) = &current
            && let Some(tx) = uow.open_tx().await
        {
            return Reader::Transaction(tx);
        }
        if self.inner.sticky && current.as_ref().is_some_and(Uow::has_modified_records) {
            return Reader::Pool(&self.inner.conn);
        }
        Reader::Pool(self.read_conn())
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
            .field("name", &self.name())
            .field("backend", &self.backend())
            .field("replicas", &self.replicas())
            .finish_non_exhaustive()
    }
}

/// Opens one pool at `url` with the pool, session and logging settings of
/// `options` (the writer's and each replica's).
async fn open_pool(
    url: &str,
    options: &DbOptions,
    telemetry: Telemetry,
    watchers: &Arc<Watchers>,
) -> Result<DatabaseConnection, DbErr> {
    let mut connect = ConnectOptions::new(driver_url(url).into_owned());
    connect
        .max_connections(options.max_connections)
        .min_connections(options.min_connections)
        .connect_timeout(Duration::from_secs(options.connect_timeout_secs))
        .acquire_timeout(Duration::from_secs(options.connect_timeout_secs.max(1) * 4))
        .sqlx_logging(false);
    if let Some(idle) = options.idle_timeout_secs {
        connect.idle_timeout(Duration::from_secs(idle));
    }
    if let Some(lifetime) = options.max_lifetime_secs {
        connect.max_lifetime(Duration::from_secs(lifetime));
    }
    if is_sqlite_url(url) {
        create_sqlite_parent_dir(url).await?;
        configure_sqlite(&mut connect, &options.sqlite);
    }
    #[cfg(feature = "mysql")]
    if crate::options::is_mysql_url(url) {
        crate::mysql::configure(&mut connect, &options.mysql);
    }
    #[cfg(feature = "postgres")]
    if matches!(scheme(url).as_deref(), Some("postgres" | "postgresql")) {
        configure_postgres(&mut connect, &options.postgres)?;
    }
    let reported = DbOptions {
        url: url.to_owned(),
        ..DbOptions::default()
    };
    let mut conn = Database::connect(connect)
        .await
        .map_err(|error| connect_error(&reported, &error))?;
    install_query_log(
        &mut conn,
        options.log_queries,
        options.slow_query_ms,
        QueryTelemetry {
            telemetry,
            connection: redact_url(url),
        },
        watchers.clone(),
    );
    Ok(conn)
}

/// The SQLite settings of every connection: journal mode, lock wait,
/// foreign keys, `synchronous` and further pragmas.
fn configure_sqlite(connect: &mut ConnectOptions, sqlite: &SqliteOptions) {
    let sqlite = sqlite.clone();
    connect.map_sqlx_sqlite_opts(move |o| {
        let journal = sqlite
            .journal_mode
            .parse::<SqliteJournalMode>()
            .unwrap_or(SqliteJournalMode::Wal);
        let mut o = o
            .journal_mode(journal)
            .busy_timeout(Duration::from_millis(sqlite.busy_timeout_ms))
            .foreign_keys(sqlite.foreign_keys);
        if let Some(synchronous) = sqlite
            .synchronous
            .as_deref()
            .and_then(|mode| mode.parse::<SqliteSynchronous>().ok())
        {
            o = o.synchronous(synchronous);
        }
        for (name, value) in &sqlite.pragmas {
            o = o.pragma(name.clone(), value.clone());
        }
        o
    });
}

/// The PostgreSQL session settings of every connection, and TLS.
#[cfg(feature = "postgres")]
fn configure_postgres(
    connect: &mut ConnectOptions,
    postgres: &crate::PostgresOptions,
) -> Result<(), DbErr> {
    use sea_orm::sqlx::postgres::PgSslMode;

    let ssl = postgres
        .ssl_mode
        .as_deref()
        .map(|mode| {
            mode.parse::<PgSslMode>()
                .map_err(|error| DbErr::Custom(format!("postgres.ssl_mode: {error}")))
        })
        .transpose()?;
    let settings = postgres.settings();
    let application = postgres.application_name.clone();
    connect.map_sqlx_postgres_opts(move |o| {
        let mut o = o.options(settings.iter().map(|(name, value)| (*name, value.as_str())));
        if let Some(application) = &application {
            o = o.application_name(application);
        }
        if let Some(ssl) = ssl {
            o = o.ssl_mode(ssl);
        }
        o
    });
    Ok(())
}

/// The connections of `conn`'s pool.
fn pool_stats(conn: &DatabaseConnection) -> PoolStats {
    if conn.is_mock_connection() {
        return PoolStats::default();
    }
    macro_rules! of {
        ($pool:expr) => {{
            let pool = $pool;
            PoolStats {
                size: pool.size(),
                idle: pool.num_idle(),
                max: pool.options().get_max_connections(),
            }
        }};
    }
    match conn.get_database_backend() {
        DbBackend::Sqlite => of!(conn.get_sqlite_connection_pool()),
        #[cfg(feature = "postgres")]
        DbBackend::Postgres => of!(conn.get_postgres_connection_pool()),
        #[cfg(feature = "mysql")]
        DbBackend::MySql => of!(conn.get_mysql_connection_pool()),
        _ => PoolStats::default(),
    }
}

/// Where statements are reported, and the connection's name in the events.
struct QueryTelemetry {
    telemetry: Telemetry,
    connection: String,
}

/// Logs statements through `tracing` (target `navree_db::query`): every
/// statement at debug level when `log_queries`, statements slower than
/// `slow_query_ms` at warn level. Only the SQL text is logged, never the
/// bound values. SeaORM does not time `execute_unprepared` statements, so
/// those are not logged. Also reports every statement to the telemetry
/// sinks that want `QueryExecuted` events, and to `watchers` (query logs,
/// listeners, unit-of-work statistics).
fn install_query_log(
    conn: &mut DatabaseConnection,
    log_queries: bool,
    slow_query_ms: u64,
    report: QueryTelemetry,
    watchers: Arc<Watchers>,
) {
    let slow = (slow_query_ms > 0).then(|| Duration::from_millis(slow_query_ms));
    conn.set_metric_callback(move |info| {
        watchers.observe(info);
        let sql = info.statement.sql.as_str();
        report.telemetry.emit_with(EventKind::QueryExecuted, || {
            TelemetryEvent::QueryExecuted(QueryExecuted {
                sql: sql.to_owned(),
                duration: info.elapsed,
                rows: None,
                connection: report.connection.clone(),
                failed: info.failed,
            })
        });
        let elapsed_ms = info.elapsed.as_secs_f64() * 1000.0;
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

/// What SQLx says when a MySQL account's password must go over a
/// connection without TLS. SQLx's RSA password exchange stays off: the
/// crypto policy keeps the `rsa` crate out of the tree.
const MYSQL_RSA_DISABLED: &str = "RSA auth backend disabled";

/// A connection error that names the database without its password (the
/// driver's own message may echo the URL), with the feature to turn on
/// when the URL's driver is not compiled in, or how to sign in when a MySQL
/// account needs TLS.
fn connect_error(options: &DbOptions, error: &DbErr) -> DbErr {
    let redacted = options.redacted_url();
    let mut message = error
        .to_string()
        .replace(&options.url, &redacted)
        .replace(driver_url(&options.url).as_ref(), &redacted);
    if let Some(password) = url_password(&options.url) {
        message = message.replace(password, "***");
    }
    let hint = match scheme(&options.url).as_deref() {
        Some("postgres" | "postgresql") if !cfg!(feature = "postgres") => {
            " (enable the `postgres` feature of navree-db)"
        }
        Some("mysql" | "mariadb") if !cfg!(feature = "mysql") => {
            " (enable the `mysql` feature of navree-db)"
        }
        // The server has no cached hash of the password yet, so the
        // password itself must travel, which takes TLS.
        Some("mysql" | "mariadb") if message.contains(MYSQL_RSA_DISABLED) => {
            " (this caching_sha2_password account needs TLS: use `ssl-mode=PREFERRED`, \
             the default, or `REQUIRED` against a server that offers TLS)"
        }
        _ => "",
    };
    DbErr::Custom(format!("cannot connect to {redacted}: {message}{hint}"))
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
