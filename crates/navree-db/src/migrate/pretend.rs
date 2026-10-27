//! `--pretend`: the connection SeaORM migrations run on while the migrator
//! only pretends. It keeps every statement and runs nothing; reads fail,
//! since there is nothing to read. (Schema migrations read the real
//! database through the context's reader instead; see
//! [`MigrationContext::pretending`].)

use navree_schema::MigrationContext;
use sea_orm::{DatabaseConnection, DbBackend, MockDatabase, MockExecResult};

/// How many statements one pretended SeaORM migration may run.
pub(super) const STATEMENT_LIMIT: usize = 4096;

/// A connection for `backend` that keeps what runs on it.
pub(super) fn recording_connection(backend: DbBackend) -> DatabaseConnection {
    MockDatabase::new(backend)
        .append_exec_results((0..STATEMENT_LIMIT).map(|_| MockExecResult {
            last_insert_id: 0,
            rows_affected: 0,
        }))
        .into_connection()
}

/// Moves the statements kept by `connection` into `context`, in order.
pub(super) fn record_into(connection: DatabaseConnection, context: &MigrationContext) {
    for transaction in connection.into_transaction_log() {
        for statement in transaction.statements() {
            context.record(statement.to_string());
        }
    }
}
