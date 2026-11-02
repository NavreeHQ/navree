//! [`Db`]: the shared connection pool.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use navree_config::ConfigSection;
use navree_kernel::{AppContext, BoxFuture, WeakAppContext};
use navree_telemetry::Telemetry;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::engine::report::Reporter;
use crate::options::{driver_url, redact_url, scheme, url_password};
use crate::queries::Watchers;
use crate::{
    Backend, ChangeObserver, DbError, DbOptions, ErrorKind, Executed, Executor, ExecutorExt,
    LoggedQuery, Pool, Row, RowStream, Statement, Transaction, TxOptions, Uow,
};

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
/// `Db` is an [`Executor`]: statements on it run in the task's unit of
/// work when it has one on this database, else on the writer's pool (see
/// below).
///
/// **Read replicas.** With `read_urls`, reads outside a transaction
/// ([`read_executor`](Db::read_executor), [`read_pool`](Db::read_pool)) go
/// to the replicas in turn; writes and transactions use the writer. A task
/// reads from the writer inside [`Db::using_writer`], and (with `sticky`,
/// the default) once its unit of work has written, so a request sees its
/// own writes while replicas catch up.
#[derive(Clone)]
pub struct Db {
    inner: Arc<DbInner>,
}

struct DbInner {
    /// The writer's pool. Shared by every handle on the same database
    /// ([`Db::with_app`] included), so [`Db::same_as`] can tell whether a
    /// unit of work uses it.
    pool: Pool,
    /// The read replicas' pools.
    replica_pools: Arc<Vec<Pool>>,
    /// The replica the next read goes to.
    next_replica: Arc<AtomicUsize>,
    /// Reads follow a unit of work's writes to the writer.
    sticky: bool,
    /// The connection's name: `default`, or a named connection's.
    name: Arc<str>,
    /// The statement listeners and hooks, shared with the pools' reporters.
    watchers: Arc<Watchers>,
    /// The direct connection (`direct_url`), if configured.
    direct: Option<Arc<Direct>>,
    /// The app whose `dyn ChangeObserver` multi-bindings see every commit.
    app: Option<WeakAppContext>,
    observers: OnceCell<Vec<Arc<dyn ChangeObserver>>>,
    /// Whether the app registers any `dyn ChangeObserver`, decided at boot:
    /// without one, writes defer preparing their change events.
    observed: bool,
    /// `database.transaction_settings`.
    transaction_settings: bool,
}

/// The pools a [`Db`] shares with the handles made from it
/// ([`Db::with_app`], [`Db::with_transaction_settings`]).
struct Pools {
    pool: Pool,
    replica_pools: Arc<Vec<Pool>>,
    sticky: bool,
    watchers: Arc<Watchers>,
    direct: Option<Arc<Direct>>,
}

/// The settings a [`Db`] passes on to the handles made from it.
#[derive(Clone, Copy)]
struct Settings {
    observed: bool,
    transaction_settings: bool,
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

/// The connections of a pool: how many are open, how many idle, the most
/// the pool opens, and how many statements wait for one
/// ([`Db::pool_stats`]). `idle: 0` alone is a busy pool; `waiting` above zero
/// is one that is full.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoolStats {
    /// Connections open (idle or in use).
    pub size: u32,
    /// Open connections nobody uses.
    pub idle: usize,
    /// The most connections the pool opens (`max_connections`).
    pub max: u32,
    /// Statements and transactions waiting for a connection now.
    pub waiting: usize,
}

impl Db {
    /// Opens a pool for `options` (creating the directory of a SQLite file).
    ///
    /// The URL's scheme picks the driver: `sqlite:` (the `sqlite` feature),
    /// `postgres://` (the `postgres` feature), `mysql://` or `mariadb://`
    /// (the `mysql` feature). A URL whose driver is not compiled in fails
    /// with a message naming the feature to turn on. `options` are checked
    /// first, as the `database` configuration section is (an invalid pool
    /// size or SQL mode fails before anything connects), so options built
    /// in code get the same errors as configured ones.
    ///
    /// ```no_run
    /// # async fn demo() -> Result<(), navree_db::DbError> {
    /// use navree_db::{Backend, Db, DbOptions};
    ///
    /// let db = Db::connect(&DbOptions {
    ///     url: "mariadb://crm:secret@127.0.0.1:3306/crm".into(),
    ///     ..DbOptions::default()
    /// })
    /// .await?;
    /// assert_eq!(db.backend(), Backend::MySql);
    /// # Ok(()) }
    /// ```
    pub async fn connect(options: &DbOptions) -> Result<Db, DbError> {
        Self::connect_with_telemetry(options, Telemetry::disabled()).await
    }

    /// Opens a pool like [`connect`](Db::connect) that reports every
    /// statement to `telemetry` as a `QueryExecuted` event (the SQL with its
    /// placeholders, never the bound values; the connection is the URL
    /// without its password). `DatabaseModule` connects this way with the
    /// app's registry.
    pub async fn connect_with_telemetry(
        options: &DbOptions,
        telemetry: Telemetry,
    ) -> Result<Db, DbError> {
        Self::connect_named(DEFAULT_CONNECTION, options, telemetry).await
    }

    /// Opens the pool of connection `name` (a named connection of
    /// [`DbConnections`](crate::DbConnections)), and the pools of its read
    /// replicas.
    pub async fn connect_named(
        name: &str,
        options: &DbOptions,
        telemetry: Telemetry,
    ) -> Result<Db, DbError> {
        options.validate().map_err(|problems| {
            DbError::new(
                ErrorKind::Connect,
                format!("invalid database options: {problems}"),
            )
        })?;
        let watchers = Arc::new(Watchers::new(Arc::from(name)));
        // Boxed: the drivers' connect futures are deep, and every future
        // that connects (workspace databases, migration commands) nests
        // this one; a boxed future keeps their types shallow.
        let connect: BoxFuture<'_, Result<Db, DbError>> =
            Box::pin(Self::connect_watched(options, telemetry, watchers));
        connect.await
    }

    /// Opens the pools of `options` (writer, replicas), reporting to
    /// `watchers`.
    async fn connect_watched(
        options: &DbOptions,
        telemetry: Telemetry,
        watchers: Arc<Watchers>,
    ) -> Result<Db, DbError> {
        let pool = open_pool(&options.url, options, telemetry.clone(), &watchers).await?;
        let mut replica_pools = Vec::with_capacity(options.read_urls.len());
        for url in &options.read_urls {
            replica_pools.push(open_pool(url, options, telemetry.clone(), &watchers).await?);
        }
        let direct = options.direct().map(|options| {
            Arc::new(Direct {
                options,
                telemetry,
                db: OnceCell::new(),
            })
        });
        Ok(Self::build(
            Pools {
                pool,
                replica_pools: Arc::new(replica_pools),
                sticky: options.sticky,
                watchers,
                direct,
            },
            None,
            Settings {
                observed: false,
                transaction_settings: options.transaction_settings,
            },
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
    /// # async fn demo(db: navree_db::Db) -> Result<(), navree_db::DbError> {
    /// use navree_db::Executor;
    ///
    /// let direct = db.direct().await?;
    /// direct.pool().execute_script("LISTEN deals").await?;
    /// # Ok(()) }
    /// ```
    pub async fn direct(&self) -> Result<Db, DbError> {
        let Some(direct) = &self.inner.direct else {
            return Ok(self.clone());
        };
        let db = direct
            .db
            .get_or_try_init(|| -> BoxFuture<'_, Result<Db, DbError>> {
                // Boxed: the driver's connect future is deep, and every
                // caller's future (migration commands) would nest it.
                Box::pin(async {
                    let db = Self::connect_watched(
                        &direct.options,
                        direct.telemetry.clone(),
                        self.inner.watchers.clone(),
                    )
                    .await?;
                    Ok(match &self.inner.app {
                        Some(app) => db.with_weak_app(app.clone(), self.inner.observed),
                        None => db,
                    })
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

    /// The same pool, reporting commits to the `dyn ChangeObserver`
    /// multi-bindings of `app` (and knowing, from its module graph, whether
    /// it has any).
    pub(crate) fn with_app(&self, app: &AppContext) -> Db {
        self.with_weak_app(app.downgrade(), app.multi_count::<dyn ChangeObserver>() > 0)
    }

    /// The same pool, reporting commits to the `dyn ChangeObserver`
    /// multi-bindings of `app` (`observed`: whether it has any).
    fn with_weak_app(&self, app: WeakAppContext, observed: bool) -> Db {
        Self::build(
            self.pools(),
            Some(app),
            Settings {
                observed,
                ..self.settings()
            },
        )
    }

    /// The same pool, its units of work setting the actor and request id
    /// in SQL or not (`database.transaction_settings`, see
    /// [`DbOptions::transaction_settings`]).
    pub fn with_transaction_settings(&self, enabled: bool) -> Db {
        Self::build(
            self.pools(),
            self.inner.app.clone(),
            Settings {
                transaction_settings: enabled,
                ..self.settings()
            },
        )
    }

    fn settings(&self) -> Settings {
        Settings {
            observed: self.inner.observed,
            transaction_settings: self.inner.transaction_settings,
        }
    }

    /// Whether units of work set the actor and request id in SQL
    /// (`database.transaction_settings`).
    pub fn transaction_settings(&self) -> bool {
        self.inner.transaction_settings
    }

    /// Whether the app registers a `dyn ChangeObserver` (decided at boot).
    pub(crate) fn observed(&self) -> bool {
        self.inner.observed
    }

    /// The pools this handle shares with its siblings.
    fn pools(&self) -> Pools {
        Pools {
            pool: self.inner.pool.clone(),
            replica_pools: self.inner.replica_pools.clone(),
            sticky: self.inner.sticky,
            watchers: self.inner.watchers.clone(),
            direct: self.inner.direct.clone(),
        }
    }

    fn build(pools: Pools, app: Option<WeakAppContext>, settings: Settings) -> Db {
        let Pools {
            pool,
            replica_pools,
            sticky,
            watchers,
            direct,
        } = pools;
        Self {
            inner: Arc::new(DbInner {
                pool,
                replica_pools,
                next_replica: Arc::new(AtomicUsize::new(0)),
                sticky,
                name: watchers.name().clone(),
                watchers,
                direct,
                app,
                observers: OnceCell::new(),
                observed: settings.observed,
                transaction_settings: settings.transaction_settings,
            }),
        }
    }

    /// Calls `listener` after every statement this connection runs (its
    /// writer and replicas; named connections have their own listeners),
    /// with the SQL, the bound values, the time it took and whether it
    /// failed. Listeners run inside the statement's task, so keep them
    /// short (spawn slow work).
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
    /// # async fn demo(db: navree_db::Db) -> Result<(), navree_db::DbError> {
    /// let analytics = db.connection("analytics").await?;
    /// assert_eq!(analytics.name(), "analytics");
    /// # Ok(()) }
    /// ```
    pub async fn connection(&self, name: &str) -> Result<Db, DbError> {
        if name == self.name() {
            return Ok(self.clone());
        }
        let Some(app) = self.app() else {
            return Err(DbError::new(
                ErrorKind::Other,
                format!(
                    "no database connection named `{name}`: this connection (`{}`) belongs to no app that configures others",
                    self.name()
                ),
            ));
        };
        let connections = app.get::<crate::DbConnections>().await.map_err(|error| {
            DbError::new(
                ErrorKind::Other,
                format!("database connection `{name}`: {error}"),
            )
        })?;
        Ok(Db::clone(&*connections.get(name).await?))
    }

    /// How many read replicas the connection has.
    pub fn replicas(&self) -> usize {
        self.inner.replica_pools.len()
    }

    /// Runs `future` with every read of the task going to the writer, even
    /// where replicas exist: for reads that must see a write that just
    /// happened outside the task's unit of work.
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) {
    /// use navree_db::Db;
    ///
    /// let pool = Db::using_writer(async { db.read_pool().clone() }).await;
    /// assert!(pool.same_pool(db.pool()));
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

    /// Closes the pools (the writer's, the replicas' and the direct
    /// connection's). Statements fail from then on; a pool replaces a
    /// connection it lost by itself, so there is no reconnecting by hand.
    pub async fn close(&self) -> Result<(), DbError> {
        self.inner.pool.close().await;
        for replica in self.inner.replica_pools.iter() {
            replica.close().await;
        }
        if let Some(direct) = self
            .inner
            .direct
            .as_ref()
            .and_then(|direct| direct.db.get())
        {
            Box::pin(direct.close()).await?;
        }
        Ok(())
    }

    /// The writer pool's connections: open, idle and the most it opens.
    pub fn pool_stats(&self) -> PoolStats {
        self.inner.pool.stats()
    }

    /// The number of connections the database server has open (from every
    /// client): `pg_stat_activity` on PostgreSQL, `Threads_connected` on
    /// MySQL; `None` on SQLite, which has no server. For `db:monitor`.
    pub async fn open_connections(&self) -> Result<Option<u64>, DbError> {
        let sql = match self.backend() {
            Backend::Postgres => "SELECT COUNT(*) FROM pg_stat_activity",
            Backend::MySql => {
                "SELECT CAST(VARIABLE_VALUE AS UNSIGNED) FROM performance_schema.global_status WHERE VARIABLE_NAME = 'Threads_connected'"
            }
            Backend::Sqlite => return Ok(None),
        };
        let row = self.pool().fetch_optional(&Statement::text(sql)).await?;
        let count = match row {
            Some(row) => row
                .get::<i64>(0)
                .map(|count| count.max(0).cast_unsigned())
                .or_else(|_| row.get::<u64>(0))?,
            None => 0,
        };
        Ok(Some(count))
    }

    /// The server's version: `17.2` (PostgreSQL), `8.4.3` or
    /// `11.4.2-MariaDB` (MySQL, MariaDB), `3.47.0` (SQLite).
    pub async fn server_version(&self) -> Result<String, DbError> {
        let sql = match self.backend() {
            Backend::Postgres => "SELECT current_setting('server_version')",
            Backend::MySql => "SELECT VERSION()",
            Backend::Sqlite => "SELECT sqlite_version()",
        };
        self.pool().fetch_scalar(&Statement::text(sql)).await
    }

    /// The name of the database the connection uses (`main` on SQLite).
    pub async fn database_name(&self) -> Result<String, DbError> {
        let sql = match self.backend() {
            Backend::Postgres => "SELECT current_database()",
            Backend::MySql => "SELECT DATABASE()",
            Backend::Sqlite => return Ok("main".to_owned()),
        };
        self.pool().fetch_scalar(&Statement::text(sql)).await
    }

    /// Whether the server is MariaDB (which speaks MySQL's protocol).
    pub async fn is_mariadb(&self) -> Result<bool, DbError> {
        if self.backend() != Backend::MySql {
            return Ok(false);
        }
        Ok(self
            .server_version()
            .await?
            .to_ascii_lowercase()
            .contains("mariadb"))
    }

    /// Whether the database has the TimescaleDB extension installed.
    pub async fn has_timescaledb(&self) -> Result<bool, DbError> {
        if self.backend() != Backend::Postgres {
            return Ok(false);
        }
        let count: i64 = self
            .pool()
            .fetch_scalar(&Statement::text(
                "SELECT COUNT(*) FROM pg_extension WHERE extname = 'timescaledb'",
            ))
            .await?;
        Ok(count > 0)
    }

    /// The database backend.
    pub fn backend(&self) -> Backend {
        self.inner.pool.backend()
    }

    /// The app this pool belongs to, while it runs: set for the `Db` that
    /// `DatabaseModule` provides, `None` for one made with
    /// [`connect`](Db::connect) and once the app is dropped. Code that
    /// receives only a `Db` (or a [`Uow`], through [`Uow::db`]) uses it to
    /// reach app services such as the `dyn Clock`.
    pub fn app(&self) -> Option<AppContext> {
        self.inner.app.as_ref().and_then(WeakAppContext::upgrade)
    }

    /// A new, lazy unit of work for `actor` (jobs, commands, tests; HTTP
    /// requests get theirs from the `DatabaseModule` layer).
    pub async fn uow(&self, actor: Option<Uuid>, request_id: Option<String>) -> Uow {
        Uow::new(self.clone(), actor, request_id)
    }

    /// A new, lazy unit of work for `actor` done by `impersonator` while
    /// impersonating them ([`Uow::impersonator`]). The `DatabaseModule`
    /// layer uses it for requests carrying a
    /// `navree_contracts::Impersonator`.
    pub async fn impersonated_uow(
        &self,
        actor: Uuid,
        impersonator: Uuid,
        request_id: Option<String>,
    ) -> Uow {
        Uow::impersonated(self.clone(), Some(actor), Some(impersonator), request_id)
    }

    /// Runs `f` in a new unit of work (no actor): commits when it returns
    /// `Ok` (running change observers and after-commit hooks), rolls back
    /// when it returns `Err`. While `f` runs, the unit of work is
    /// [`Uow::current`].
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) -> Result<(), navree_db::DbError> {
    /// use navree_db::{Executor, Statement};
    ///
    /// let rows = db
    ///     .transaction(|uow| async move {
    ///         let done = uow.execute(&Statement::text("DELETE FROM sessions")).await?;
    ///         Ok(done.rows_affected)
    ///     })
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn transaction<F, Fut, T>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, DbError>>,
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
    pub(crate) async fn observers(&self) -> Result<&[Arc<dyn ChangeObserver>], DbError> {
        let observers = self
            .inner
            .observers
            .get_or_try_init(|| async {
                let Some(app) = self.inner.app.as_ref() else {
                    return Ok(Vec::new());
                };
                let Some(ctx) = app.upgrade() else {
                    return Err(DbError::new(
                        ErrorKind::Other,
                        "the app owning this database has been dropped",
                    ));
                };
                ctx.get_all::<dyn ChangeObserver>().await.map_err(|e| {
                    DbError::new(
                        ErrorKind::Other,
                        format!("cannot resolve change observers: {e}"),
                    )
                })
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
/// `options` (the writer's and each replica's), its statements reported to
/// the connection's watchers and telemetry.
async fn open_pool(
    url: &str,
    options: &DbOptions,
    telemetry: Telemetry,
    watchers: &Arc<Watchers>,
) -> Result<Pool, DbError> {
    let reporter = Reporter::new(watchers.clone(), telemetry, url, options);
    Pool::open(url, options, reporter).await
}

/// What SQLx says when a MySQL account's password must go over a
/// connection without TLS. SQLx's RSA password exchange stays off: the
/// crypto policy keeps the `rsa` crate out of the tree.
const MYSQL_RSA_DISABLED: &str = "RSA auth backend disabled";

/// The message of a failed connection to `url`: the database named without
/// its password (the driver's own message may echo the URL), with the
/// feature to turn on when the URL's driver is not compiled in, or how to
/// sign in when a MySQL account needs TLS.
pub(crate) fn connect_message(url: &str, message: &str) -> String {
    let redacted = redact_url(url);
    let mut message = message
        .replace(url, &redacted)
        .replace(driver_url(url).as_ref(), &redacted);
    if let Some(password) = url_password(url) {
        message = message.replace(password, "***");
    }
    let hint = match scheme(url).as_deref() {
        Some("postgres" | "postgresql") if !cfg!(feature = "postgres") => {
            " (enable the `postgres` feature of navree-db)"
        }
        Some("mysql" | "mariadb") if !cfg!(feature = "mysql") => {
            " (enable the `mysql` feature of navree-db)"
        }
        Some("sqlite") if !cfg!(feature = "sqlite") => {
            " (enable the `sqlite` feature of navree-db)"
        }
        // The server has no cached hash of the password yet, so the
        // password itself must travel, which takes TLS.
        Some("mysql" | "mariadb") if message.contains(MYSQL_RSA_DISABLED) => {
            " (this caching_sha2_password account needs TLS: use `ssl-mode=PREFERRED`, \
             the default, or `REQUIRED` against a server that offers TLS)"
        }
        _ => "",
    };
    format!("cannot connect to {redacted}: {message}{hint}")
}

impl Db {
    /// The writer's pool: navree-db's own connections.
    pub fn pool(&self) -> &Pool {
        &self.inner.pool
    }

    /// The pool a read outside a transaction uses: the next read replica
    /// in turn, or the writer when there is none or the task reads from the
    /// writer ([`Db::using_writer`]).
    pub fn read_pool(&self) -> &Pool {
        let replicas = &self.inner.replica_pools;
        if replicas.is_empty() || Self::reads_from_writer() {
            return self.pool();
        }
        let at = self.inner.next_replica.fetch_add(1, Ordering::Relaxed) % replicas.len();
        &replicas[at]
    }

    /// The pool the running task reads from outside a transaction, as
    /// [`read_executor`](Db::read_executor) picks it: the writer when the
    /// task reads from the writer ([`Db::using_writer`]) or, with `sticky`,
    /// once its unit of work on this pool wrote
    /// ([`Uow::has_modified_records`]); else [`read_pool`](Db::read_pool).
    pub fn task_read_pool(&self) -> &Pool {
        if self.inner.sticky
            && self
                .current_uow()
                .as_ref()
                .is_some_and(Uow::has_modified_records)
        {
            return self.pool();
        }
        self.read_pool()
    }

    /// Begins a transaction on a connection of its own (not the task's
    /// unit of work: see [`Db::transaction`] for one). The connection's
    /// before-transaction hooks run first.
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) -> Result<(), navree_db::DbError> {
    /// use navree_db::{Executor, Statement};
    ///
    /// let tx = db.begin().await?;
    /// tx.execute(&Statement::text("DELETE FROM sessions")).await?;
    /// tx.commit().await?;
    /// # Ok(()) }
    /// ```
    pub async fn begin(&self) -> Result<Transaction, DbError> {
        self.pool().begin().await
    }

    /// Begins a transaction for work that reads and then writes (outside a
    /// unit of work): `BEGIN IMMEDIATE` on SQLite, so it queues for the
    /// write lock instead of failing with `database is locked` when another
    /// connection holds it between the read and the write; a plain `BEGIN`
    /// elsewhere.
    pub async fn begin_for_write(&self) -> Result<Transaction, DbError> {
        self.begin_with(TxOptions::new().immediate()).await
    }

    /// Begins a transaction with `options` (isolation level, read only,
    /// SQLite's `IMMEDIATE`).
    pub async fn begin_with(&self, options: TxOptions) -> Result<Transaction, DbError> {
        self.pool().begin_with(options).await
    }

    /// Checks the writer with a round trip to the server.
    pub async fn ping(&self) -> Result<(), DbError> {
        self.pool().ping().await
    }

    /// Whether `self` and `other` are handles to the same database (the
    /// same pools): a unit of work of one can run the other's statements.
    pub fn same_as(&self, other: &Db) -> bool {
        self.inner.pool.same_pool(&other.inner.pool)
    }

    /// Where a read goes: the task's unit of work on this database when
    /// its transaction is open (it sees its own writes, and on SQLite a
    /// second connection would wait for the lock it holds); else the writer
    /// when the task reads from it ([`Db::using_writer`]) or, with
    /// `sticky`, its unit of work has written; else the next read replica
    /// in turn ([`Db::read_pool`]).
    ///
    /// Use it for reads in services that take no `&Uow`. Drop it before
    /// the unit of work commits (use it as a temporary).
    ///
    /// ```no_run
    /// # async fn demo(db: navree_db::Db) -> Result<(), navree_db::DbError> {
    /// use navree_db::{ExecutorExt, Statement};
    ///
    /// let open: i64 = db
    ///     .read_executor()
    ///     .await
    ///     .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM deals"))
    ///     .await?;
    /// # let _ = open;
    /// # Ok(()) }
    /// ```
    pub async fn read_executor(&self) -> ReadExecutor<'_> {
        let current = self.current_uow();
        if let Some(uow) = &current
            && !uow.is_finished().await
            && uow.has_open_transaction().await
        {
            return ReadExecutor(Route::Uow(uow.clone()));
        }
        let writer = self.inner.sticky && current.as_ref().is_some_and(Uow::has_modified_records);
        ReadExecutor(Route::Pool(if writer {
            self.pool()
        } else {
            self.read_pool()
        }))
    }

    /// The task's current unit of work, when it works on this pool.
    fn current_uow(&self) -> Option<Uow> {
        Uow::current().filter(|uow| uow.db().same_as(self))
    }

    /// Where a statement of [`Executor`] runs: the task's current unit of
    /// work on this pool when its transaction is open (or, for a `write`,
    /// when it is not finished: the write begins it), else the writer's
    /// pool.
    async fn route(&self, write: bool) -> Route<'_> {
        if let Some(uow) = self.current_uow()
            && !uow.is_finished().await
            && (write || uow.has_open_transaction().await)
        {
            return Route::Uow(uow);
        }
        Route::Pool(self.pool())
    }
}

/// Where [`Db`] runs a statement.
enum Route<'a> {
    Uow(Uow),
    Pool(&'a Pool),
}

impl Route<'_> {
    fn executor(&self) -> &dyn Executor {
        match self {
            Route::Uow(uow) => uow,
            Route::Pool(pool) => *pool,
        }
    }
}

/// The [`Executor`] of [`Db::read_executor`]: the open unit of work, the
/// writer or a read replica, chosen when it was made.
pub struct ReadExecutor<'a>(Route<'a>);

impl ReadExecutor<'_> {
    /// Whether reads go through the current unit of work's transaction.
    pub fn in_transaction(&self) -> bool {
        matches!(self.0, Route::Uow(_))
    }
}

impl fmt::Debug for ReadExecutor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match &self.0 {
            Route::Uow(_) => "ReadExecutor(unit of work)",
            Route::Pool(_) => "ReadExecutor(pool)",
        })
    }
}

impl Executor for ReadExecutor<'_> {
    fn backend(&self) -> Backend {
        self.0.executor().backend()
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        self.0.executor().execute(statement)
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        self.0.executor().fetch_all(statement)
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        self.0.executor().fetch_optional(statement)
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        self.0.executor().fetch(statement)
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        self.0.executor().execute_script(sql)
    }
}

/// Statements on a `Db` run in the task's unit of work on this pool when
/// its transaction is open, as [`Db::read_executor`] reads; `execute` and
/// `execute_script` (writes) join the task's unfinished unit of work even
/// before its transaction began (and begin it), as the ORM's writes do, so
/// a request's writes stay in its transaction and audit trail. Otherwise
/// they run on the writer's pool. Reads that may go to a replica take
/// [`Db::read_pool`].
impl Executor for Db {
    fn backend(&self) -> Backend {
        Db::backend(self)
    }

    fn execute<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move { self.route(true).await.executor().execute(statement).await })
    }

    fn fetch_all<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Vec<Row>, DbError>> {
        Box::pin(async move {
            self.route(false)
                .await
                .executor()
                .fetch_all(statement)
                .await
        })
    }

    fn fetch_optional<'a>(
        &'a self,
        statement: &'a Statement,
    ) -> BoxFuture<'a, Result<Option<Row>, DbError>> {
        Box::pin(async move {
            self.route(false)
                .await
                .executor()
                .fetch_optional(statement)
                .await
        })
    }

    fn fetch<'a>(&'a self, statement: &'a Statement) -> BoxFuture<'a, Result<RowStream, DbError>> {
        Box::pin(async move { self.route(false).await.executor().fetch(statement).await })
    }

    fn execute_script<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<Executed, DbError>> {
        Box::pin(async move { self.route(true).await.executor().execute_script(sql).await })
    }
}
