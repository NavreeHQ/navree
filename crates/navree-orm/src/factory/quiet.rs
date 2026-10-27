//! Quiet operations: while a future runs [`scope`]d, the app's model hooks
//! (observers, listeners registered as `dyn ModelHook<M>`) do not run. The
//! model's own hooks ([`Model::hooks`](crate::Model::hooks): generated
//! behaviour such as workspace scoping) still run, and writes still record
//! their change events for the audit log.
//!
//! Factories create records quietly with `create_quietly`; seeders that ask
//! for it run quietly.

use std::future::Future;

tokio::task_local! {
    /// Set while a quiet operation runs.
    static QUIET: ();
}

/// Runs `future` quietly.
pub(crate) async fn scope<F: Future>(future: F) -> F::Output {
    QUIET.scope((), future).await
}

/// Whether the running task is inside a quiet operation (or a seeder that
/// runs without model events, see navree-db's
/// `Seeder::without_model_events`).
pub(crate) fn is_active() -> bool {
    QUIET.try_with(|_| ()).is_ok() || navree_db::seeding_without_model_events()
}
