//! What the framework runs by itself, outside any request, job or scheduled
//! task: the outbox's poll and the transaction statements around it. Without
//! this, an app that nobody is using fills its log with four queries a second.

use navree_telemetry::EventContext;

/// Whether `context` belongs to no request, job or scheduled task: the work of a
/// background loop.
pub(crate) fn is_unscoped(context: &EventContext) -> bool {
    context.request_id.is_none() && context.job_id.is_none() && context.trace.is_none()
}

/// Whether `sql` is a statement only the framework runs when nothing is using
/// the app: transaction control, the statement that makes the actor readable in
/// SQL, and anything that reads or writes a table of the framework's own
/// (`micro_outbox`, `micro_inbox`, `navree_migrations`).
pub(crate) fn is_framework_statement(sql: &str) -> bool {
    let first = sql
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|word| !word.is_empty())
        .unwrap_or_default();
    if [
        "begin",
        "commit",
        "rollback",
        "savepoint",
        "release",
        "start",
        "end",
    ]
    .iter()
    .any(|keyword| first.eq_ignore_ascii_case(keyword))
    {
        return true;
    }
    let lower = sql.to_ascii_lowercase();
    if lower.contains("navree.actor")
        || lower.contains("navree.request_id")
        || lower.contains("@navree_")
    {
        return true;
    }
    lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word.starts_with("micro_") || word.starts_with("navree_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_control_and_framework_tables_are_the_frameworks() {
        for sql in [
            "BEGIN",
            "begin immediate",
            "  COMMIT",
            "ROLLBACK TO SAVEPOINT _sqlx_savepoint_1",
            "START TRANSACTION READ ONLY",
            "SELECT set_config('navree.actor', $1, true)",
            "SET @navree_actor = ?, @navree_request_id = ?",
            "SELECT id FROM micro_outbox WHERE seq < $1",
            "DELETE FROM \"micro_inbox\" WHERE at < ?",
            "SELECT version FROM navree_migrations",
        ] {
            assert!(is_framework_statement(sql), "{sql}");
        }
    }

    #[test]
    fn the_apps_own_statements_are_not() {
        for sql in [
            "SELECT * FROM deals",
            "INSERT INTO begin_dates (id) VALUES (?)",
            "UPDATE deals SET title = 'begin'",
            "SELECT * FROM microsoft_accounts",
            "SELECT * FROM navreeists",
        ] {
            assert!(!is_framework_statement(sql), "{sql}");
        }
    }

    #[test]
    fn only_work_outside_a_unit_is_unscoped() {
        assert!(is_unscoped(&EventContext::default()));
        let request = EventContext {
            request_id: Some("r".into()),
            ..EventContext::default()
        };
        assert!(!is_unscoped(&request));
    }
}
