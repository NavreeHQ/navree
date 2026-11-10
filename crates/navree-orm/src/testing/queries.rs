//! Counting the statements a piece of code runs.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

tokio::task_local! {
    /// The counter of the innermost counting scope.
    static STATEMENTS: Arc<AtomicUsize>;
}

/// Counts one statement in the current counting scope (called for every
/// statement the ORM runs, SQL or document).
pub(crate) fn note_statement() {
    let _ = STATEMENTS.try_with(|counter| counter.fetch_add(1, Ordering::Relaxed));
}

/// Runs `future` and returns its output with the number of statements the
/// ORM ran for it (on this task; a nested count is not added to the outer
/// one).
///
/// ```no_run
/// # use navree_orm::__doc_factories::*;
/// # async fn demo(db: Db) -> Result<()> {
/// let (deals, queries) = navree_orm::testing::count_queries(Deal::query().get(&db)).await;
/// assert_eq!(queries, 1);
/// # Ok(()) }
/// ```
pub async fn count_queries<F: Future>(future: F) -> (F::Output, usize) {
    let counter = Arc::new(AtomicUsize::new(0));
    let output = STATEMENTS.scope(counter.clone(), future).await;
    (output, counter.load(Ordering::Relaxed))
}

/// Runs `future` and asserts that the ORM ran exactly `expected` statements
/// for it; returns the future's output.
///
/// # Panics
///
/// When it ran another number.
pub async fn expect_query_count<F: Future>(expected: usize, future: F) -> F::Output {
    let (output, count) = count_queries(future).await;
    assert!(
        count == expected,
        "expect_query_count: {count} statement(s) ran, expected {expected}"
    );
    output
}
