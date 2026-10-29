//! [`Reporter`]: where a pool reports the statements it runs.

use std::sync::Arc;
use std::time::Duration;

use navree_telemetry::{EventKind, QueryExecuted, Telemetry, TelemetryEvent};

use crate::DbOptions;
use crate::engine::{Backend, Value};
use crate::queries::Watchers;

/// Where the statements of a connection (its writer, replicas and
/// transactions) are reported: its [`Watchers`] (query logs, listeners,
/// unit-of-work statistics and duration handlers), the telemetry sinks
/// that want `QueryExecuted` events, and `tracing` (target
/// `navree_db::query`: every statement at debug level with `log_queries`,
/// slow ones at warn level). Only SQL is reported, never bound values,
/// except to the query logs and listeners an app installs itself.
#[derive(Clone)]
pub(crate) struct Reporter {
    pub(crate) watchers: Arc<Watchers>,
    telemetry: Telemetry,
    /// The connection in telemetry events: the URL without its password.
    connection: Arc<str>,
    log_queries: bool,
    slow: Option<Duration>,
}

impl Reporter {
    pub(crate) fn new(
        watchers: Arc<Watchers>,
        telemetry: Telemetry,
        url: &str,
        options: &DbOptions,
    ) -> Reporter {
        Reporter {
            watchers,
            telemetry,
            connection: crate::options::redact_url(url).into(),
            log_queries: options.log_queries,
            slow: (options.slow_query_ms > 0).then(|| Duration::from_millis(options.slow_query_ms)),
        }
    }

    /// Runs the connection's before-executing hooks.
    pub(crate) fn before(&self, sql: &str) {
        self.watchers.before_executing(sql);
    }

    /// Runs the connection's before-transaction hooks.
    pub(crate) fn before_transaction(&self) {
        self.watchers.before_transaction();
    }

    /// Reports a statement that ran.
    pub(crate) fn report(
        &self,
        sql: &str,
        values: &[Value],
        backend: Backend,
        elapsed: Duration,
        failed: bool,
        rows: Option<u64>,
    ) {
        self.watchers
            .observe_statement(sql, || values.to_vec(), backend, elapsed, failed);
        self.emit(sql, elapsed, failed, rows);
    }

    /// Sends the telemetry event and the `tracing` log of a statement.
    pub(crate) fn emit(&self, sql: &str, elapsed: Duration, failed: bool, rows: Option<u64>) {
        self.telemetry.emit_with(EventKind::QueryExecuted, || {
            TelemetryEvent::QueryExecuted(QueryExecuted {
                sql: sql.to_owned(),
                duration: elapsed,
                rows,
                connection: self.connection.to_string(),
                failed,
            })
        });
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        if self.slow.is_some_and(|slow| elapsed >= slow) {
            tracing::warn!(target: "navree_db::query", sql, elapsed_ms, failed, "slow query");
        } else if self.log_queries {
            tracing::debug!(target: "navree_db::query", sql, elapsed_ms, failed, "query");
        }
    }
}

impl std::fmt::Debug for Reporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reporter")
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}
