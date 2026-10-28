//! Watching statements: [`LoggedQuery`], [`QueryLog`], the listeners and
//! hooks of a [`Db`] ([`Db::on_query`], [`Db::before_executing`],
//! [`Db::before_starting_transaction`],
//! [`Db::when_querying_for_longer_than`]) and the statistics of a unit of
//! work ([`QueryStats`]).

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use sea_orm::metric::Info;
use sea_orm::{DbBackend, Statement};

use crate::Uow;

/// A statement a connection ran: its SQL (with placeholders), the bound
/// values, how long it took, whether it failed, and the connection's name.
/// [`Db::on_query`](crate::Db::on_query) listeners and [`QueryLog`] see
/// these.
#[derive(Clone, Debug)]
pub struct LoggedQuery {
    /// The statement with its placeholders.
    pub sql: String,
    /// The bound values, in placeholder order.
    pub values: Vec<sea_orm::Value>,
    /// How long it took.
    pub duration: Duration,
    /// Whether it failed.
    pub failed: bool,
    /// The connection's name: `default`, or a named connection's.
    pub connection: Arc<str>,
    /// The database it ran on.
    pub backend: DbBackend,
}

impl LoggedQuery {
    /// The statement with its values written in, for reading (logs, test
    /// output); run statements with their values bound instead.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use navree_db::LoggedQuery;
    /// use sea_orm::DbBackend;
    ///
    /// let query = LoggedQuery {
    ///     sql: "SELECT * FROM deals WHERE title = $1".into(),
    ///     values: vec!["Renewal".into()],
    ///     duration: Duration::from_millis(2),
    ///     failed: false,
    ///     connection: "default".into(),
    ///     backend: DbBackend::Postgres,
    /// };
    /// assert_eq!(query.raw_sql(), "SELECT * FROM deals WHERE title = 'Renewal'");
    /// ```
    pub fn raw_sql(&self) -> String {
        Statement::from_sql_and_values(self.backend, self.sql.clone(), self.values.clone())
            .to_string()
    }
}

tokio::task_local! {
    /// The innermost running [`QueryLog::capture`].
    static CAPTURE: Arc<Capture>;
}

/// The statements of one [`QueryLog::capture`], and the capture around it.
struct Capture {
    queries: Mutex<Vec<LoggedQuery>>,
    parent: Option<Arc<Capture>>,
}

/// Captures the statements a piece of code runs.
///
/// ```no_run
/// # async fn demo(db: navree_db::Db) {
/// use navree_db::QueryLog;
/// use sea_orm::ConnectionTrait;
///
/// let (_, queries) = QueryLog::capture(async {
///     db.reader().await.query_all_raw(sea_orm::Statement::from_string(db.backend(), "SELECT 1")).await
/// })
/// .await;
/// for query in &queries {
///     println!("{} ({:?} on {})", query.raw_sql(), query.duration, query.connection);
/// }
/// # }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct QueryLog;

impl QueryLog {
    /// Runs `future` and returns its output with the statements it ran, in
    /// order, on any connection opened by [`Db`](crate::Db). Statements
    /// count while the future's task runs them (tasks it spawns are not
    /// followed); SeaORM does not time `execute_unprepared`, so those are
    /// left out. Captures nest: an outer capture also gets the statements
    /// of an inner one.
    pub async fn capture<F: Future>(future: F) -> (F::Output, Vec<LoggedQuery>) {
        let capture = Arc::new(Capture {
            queries: Mutex::new(Vec::new()),
            parent: CAPTURE.try_with(Arc::clone).ok(),
        });
        let output = CAPTURE.scope(capture.clone(), future).await;
        let queries = std::mem::take(&mut *lock(&capture.queries));
        (output, queries)
    }

    /// Whether the running task captures statements.
    pub fn is_capturing() -> bool {
        CAPTURE.try_with(|_| ()).is_ok()
    }
}

/// A listener of [`Db::on_query`](crate::Db::on_query).
type QueryListener = Arc<dyn Fn(&LoggedQuery) + Send + Sync>;

/// A hook of [`Db::before_executing`](crate::Db::before_executing).
type StatementHook = Arc<dyn Fn(&str) + Send + Sync>;

/// A hook of [`Db::before_starting_transaction`](crate::Db::before_starting_transaction).
type TransactionHook = Arc<dyn Fn() + Send + Sync>;

/// What [`Db::when_querying_for_longer_than`](crate::Db::when_querying_for_longer_than)
/// calls: the unit of work and its total query time.
type SlowWorkHandler = Arc<dyn Fn(&Uow, Duration) + Send + Sync>;

/// A handler of [`Db::when_querying_for_longer_than`](crate::Db::when_querying_for_longer_than).
#[derive(Clone)]
struct DurationHandler {
    id: usize,
    threshold: Duration,
    handler: SlowWorkHandler,
}

/// The listeners and hooks of one connection (its writer and replicas),
/// shared by every handle on it.
pub(crate) struct Watchers {
    name: Arc<str>,
    listeners: RwLock<Vec<QueryListener>>,
    before_executing: RwLock<Vec<StatementHook>>,
    before_transaction: RwLock<Vec<TransactionHook>>,
    duration_handlers: RwLock<Vec<DurationHandler>>,
}

impl Watchers {
    pub(crate) fn new(name: Arc<str>) -> Self {
        Watchers {
            name,
            listeners: RwLock::new(Vec::new()),
            before_executing: RwLock::new(Vec::new()),
            before_transaction: RwLock::new(Vec::new()),
            duration_handlers: RwLock::new(Vec::new()),
        }
    }

    /// The connection's name.
    pub(crate) fn name(&self) -> &Arc<str> {
        &self.name
    }

    pub(crate) fn add_listener(&self, listener: QueryListener) {
        write(&self.listeners).push(listener);
    }

    pub(crate) fn add_before_executing(&self, hook: StatementHook) {
        write(&self.before_executing).push(hook);
    }

    pub(crate) fn add_before_transaction(&self, hook: TransactionHook) {
        write(&self.before_transaction).push(hook);
    }

    pub(crate) fn add_duration_handler(&self, threshold: Duration, handler: SlowWorkHandler) {
        let mut handlers = write(&self.duration_handlers);
        let id = handlers.len();
        handlers.push(DurationHandler {
            id,
            threshold,
            handler,
        });
    }

    /// Runs the before-executing hooks for `sql`.
    pub(crate) fn before_executing(&self, sql: &str) {
        let hooks = read(&self.before_executing).clone();
        for hook in hooks {
            hook(sql);
        }
    }

    /// Runs the before-transaction hooks.
    pub(crate) fn before_transaction(&self) {
        let hooks = read(&self.before_transaction).clone();
        for hook in hooks {
            hook();
        }
    }

    /// Sees a statement SeaORM timed (see [`observe_statement`](Self::observe_statement)).
    pub(crate) fn observe(self: &Arc<Self>, info: &Info<'_>) {
        self.observe_statement(
            &info.statement.sql,
            || {
                info.statement
                    .values
                    .as_ref()
                    .map(|values| values.0.clone())
                    .unwrap_or_default()
            },
            info.statement.db_backend,
            info.elapsed,
            info.failed,
        );
    }

    /// Sees a statement that ran on this connection: the task's captures,
    /// the listeners (`values` is called only when one of them is there),
    /// and the statistics and duration handlers of the task's unit of work
    /// when it runs on this connection.
    pub(crate) fn observe_statement(
        self: &Arc<Self>,
        sql: &str,
        values: impl FnOnce() -> Vec<sea_orm::Value>,
        backend: DbBackend,
        elapsed: Duration,
        failed: bool,
    ) {
        let capture = CAPTURE.try_with(Arc::clone).ok();
        let listeners = read(&self.listeners).clone();
        if capture.is_some() || !listeners.is_empty() {
            let logged = LoggedQuery {
                sql: sql.to_owned(),
                values: values(),
                duration: elapsed,
                failed,
                connection: self.name.clone(),
                backend,
            };
            let mut next = capture;
            while let Some(capture) = next {
                lock(&capture.queries).push(logged.clone());
                next = capture.parent.clone();
            }
            for listener in listeners {
                listener(&logged);
            }
        }
        if let Some(uow) = Uow::current().filter(|uow| uow.db().is_watched_by(self)) {
            let total = uow.count_query(sql, elapsed);
            let handlers = read(&self.duration_handlers).clone();
            for handler in handlers {
                if total > handler.threshold && uow.claim_duration_handler(handler.id) {
                    (handler.handler)(&uow, total);
                }
            }
        }
    }
}

impl fmt::Debug for Watchers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watchers")
            .field("connection", &self.name)
            .field("listeners", &read(&self.listeners).len())
            .finish_non_exhaustive()
    }
}

/// How many distinct statements a unit of work counts one by one.
const DISTINCT_STATEMENTS: usize = 256;

/// The statements of a unit of work: how many, how long in total, and the
/// ones that ran more than once. See [`Uow::query_stats`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryStats {
    /// Statements run.
    pub statements: u64,
    /// Their total time (see [`Uow::reset_total_query_duration`]).
    pub total: Duration,
    /// The statements that ran more than once, most frequent first: the
    /// telltale of a query in a loop (an N+1). Counted for the first 256
    /// distinct statements.
    pub repeated: Vec<RepeatedQuery>,
}

/// A statement a unit of work ran more than once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepeatedQuery {
    /// The statement with its placeholders.
    pub sql: String,
    /// How many times it ran.
    pub count: u64,
}

/// What a unit of work counts of its statements.
#[derive(Default)]
pub(crate) struct QueryCounter {
    statements: u64,
    total: Duration,
    by_sql: HashMap<String, u64>,
    /// The duration handlers that ran for this unit of work.
    handled: Vec<usize>,
}

impl QueryCounter {
    /// Counts a statement; returns the total time so far.
    pub(crate) fn count(&mut self, sql: &str, elapsed: Duration) -> Duration {
        self.statements += 1;
        self.total += elapsed;
        if let Some(count) = self.by_sql.get_mut(sql) {
            *count += 1;
        } else if self.by_sql.len() < DISTINCT_STATEMENTS {
            self.by_sql.insert(sql.to_owned(), 1);
        }
        self.total
    }

    pub(crate) fn total(&self) -> Duration {
        self.total
    }

    pub(crate) fn reset_total(&mut self) {
        self.total = Duration::ZERO;
    }

    /// Whether duration handler `id` may run (it then counts as run).
    pub(crate) fn claim(&mut self, id: usize) -> bool {
        if self.handled.contains(&id) {
            return false;
        }
        self.handled.push(id);
        true
    }

    pub(crate) fn allow_handlers_again(&mut self) {
        self.handled.clear();
    }

    pub(crate) fn stats(&self) -> QueryStats {
        let mut repeated: Vec<RepeatedQuery> = self
            .by_sql
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(sql, count)| RepeatedQuery {
                sql: sql.clone(),
                count: *count,
            })
            .collect();
        repeated.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.sql.cmp(&b.sql)));
        QueryStats {
            statements: self.statements,
            total: self.total,
            repeated,
        }
    }
}

/// How often one statement may run in a unit of work before the summary
/// logged when it finishes warns about it.
pub(crate) const REPEATED_WARNING: u64 = 10;

/// Logs the statistics of a finished unit of work (target
/// `navree_db::uow`): a debug summary, and a warning per statement that ran
/// [`REPEATED_WARNING`] times or more.
pub(crate) fn log_stats(stats: &QueryStats, request_id: Option<&str>) {
    if stats.statements == 0 {
        return;
    }
    let total_ms = stats.total.as_secs_f64() * 1000.0;
    tracing::debug!(
        target: "navree_db::uow",
        statements = stats.statements,
        total_ms,
        repeated = stats.repeated.len(),
        request_id,
        "unit of work queries"
    );
    for repeated in stats
        .repeated
        .iter()
        .filter(|repeated| repeated.count >= REPEATED_WARNING)
    {
        tracing::warn!(
            target: "navree_db::uow",
            sql = %repeated.sql,
            count = repeated.count,
            request_id,
            "the same statement ran {} times in one unit of work (a query in a loop?)",
            repeated.count
        );
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_track_totals_repeats_and_handlers() {
        let mut counter = QueryCounter::default();
        counter.count("SELECT 1", Duration::from_millis(2));
        counter.count("SELECT 2", Duration::from_millis(3));
        let total = counter.count("SELECT 1", Duration::from_millis(5));
        assert_eq!(total, Duration::from_millis(10));
        let stats = counter.stats();
        assert_eq!(stats.statements, 3);
        assert_eq!(
            stats.repeated,
            [RepeatedQuery {
                sql: "SELECT 1".into(),
                count: 2
            }]
        );
        assert!(counter.claim(0));
        assert!(!counter.claim(0), "once");
        counter.allow_handlers_again();
        assert!(counter.claim(0));
        counter.reset_total();
        assert_eq!(counter.total(), Duration::ZERO);
        assert_eq!(counter.stats().statements, 3, "the count stays");
    }

    #[test]
    fn distinct_statements_are_bounded() {
        let mut counter = QueryCounter::default();
        for n in 0..(DISTINCT_STATEMENTS + 10) {
            counter.count(&format!("SELECT {n}"), Duration::ZERO);
        }
        assert_eq!(counter.by_sql.len(), DISTINCT_STATEMENTS);
        assert_eq!(
            counter.stats().statements,
            (DISTINCT_STATEMENTS + 10) as u64
        );
    }
}
