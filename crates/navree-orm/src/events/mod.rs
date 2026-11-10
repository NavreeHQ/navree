//! Model events: how they reach listeners, and silencing them.
//!
//! Every persistence flow announces its steps as [`ModelEvent`]s to the
//! model's [`ModelHook`]s. On top of that seam:
//!
//! - **Observers** ([`Observer`]): one async method per event, declared on
//!   the model (`#[model(observers(DealObserver))]`) or registered by a
//!   module (`m.observe::<Deal>(DealObserver)`).
//! - **Closure listeners**: `m.on_model::<Deal, _>(ModelEvent::Creating, f)`.
//! - **Typed application events** through navree-events:
//!   `#[model(dispatches(created = DealCreated))]` (see [`Dispatch`]).
//! - **Custom events** the application fires itself:
//!   [`Record::fire`](crate::Record::fire).
//!
//! Inside a unit of work, `-ing` events run in its transaction (a listener
//! can still cancel or change the write), and listeners hear the write
//! events (`created`, `updated`, `saved`, `deleted`, `trashed`,
//! `force_deleted`, `restored`) once it commits, never for a write that
//! was rolled back; outside one they hear them at once. See
//! [`HookKind`](crate::hooks::HookKind).
//!
//! [`without_events`] runs code with every listener silenced (behaviours
//! such as touching parents still run, and the unit of work still records
//! the change for the audit trail); the `_quietly` methods
//! ([`Record::save_quietly`](crate::Record::save_quietly), ...) use it.
//!
//! Mass writes ([`Builder::update`](crate::Builder::update),
//! [`delete`](crate::Builder::delete), `increment`, `touch`, `restore`,
//! `force_delete`) run one statement over every matching row without
//! reading them, so they fire no model events: load the records and save
//! or delete them one by one when listeners must hear about each.

use std::any::Any;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;
use navree_db::Uow;

use crate::exec::ExecContext;
use crate::hooks::{ModelEvent, ModelHook};
use crate::model::{Model, PrimaryKey};
use crate::record::Record;

mod dispatch;
mod fake;
mod listener;
mod observer;
mod record;

pub use dispatch::Dispatch;
pub(crate) use fake::FakeListener;
pub use fake::{FiredEvent, ModelEventFake};
pub(crate) use listener::ClosureListener;
pub use observer::{Observer, ObserverHook};

tokio::task_local! {
    /// Set while [`without_events`] runs a future.
    static SILENCED: ();
}

/// Runs `future` with every model event listener silenced: observers,
/// closure listeners, typed application events and broadcasts hear
/// nothing, for every model. Behaviours (hooks of
/// [`HookKind::Behaviour`](crate::hooks::HookKind::Behaviour)) still run,
/// and writes inside a unit of work still record their change events (the
/// audit trail).
///
/// The silence belongs to the task running `future`: concurrent requests
/// are unaffected, and tasks it spawns are not silenced.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(uow: Uow, mut deal: Record<Deal>) -> Result<()> {
/// use navree_orm::without_events;
///
/// // A data fix that must not notify anyone.
/// without_events(async {
///     deal.stage = Stage::Lost;
///     deal.save(&uow).await
/// })
/// .await?;
/// # Ok(()) }
/// ```
pub async fn without_events<F: Future>(future: F) -> F::Output {
    SILENCED.scope((), future).await
}

/// Whether the current task runs inside [`without_events`].
pub fn events_are_silenced() -> bool {
    SILENCED.try_with(|()| ()).is_ok()
}

/// Runs `future` (a hook) with the operation's unit of work as the task's
/// current one, so what the hook dispatches (jobs, events) and writes
/// through a `Db` joins it.
pub(crate) async fn in_unit_of_work<F: Future>(ctx: &ExecContext, future: F) -> F::Output {
    match ctx.uow() {
        Some(uow) if !Uow::current().is_some_and(|current| current.same_as(uow)) => {
            uow.scope(future).await
        }
        _ => future.await,
    }
}

/// Hands `event` to `hooks` once `uow` commits, on `snapshot` (the record
/// as it was when the event happened). A listener that fails or panics is
/// logged and reported to the app's telemetry; the others still run, and
/// the commit stands.
pub(crate) fn after_commit<M: Model>(
    uow: &Uow,
    hooks: Vec<Arc<dyn ModelHook<M>>>,
    event: ModelEvent,
    ctx: ExecContext,
    snapshot: Record<M>,
) {
    uow.after_commit(move || {
        Box::pin(async move {
            let mut snapshot = snapshot;
            for hook in hooks {
                let outcome = AssertUnwindSafe(hook.after(event, &ctx, &mut snapshot))
                    .catch_unwind()
                    .await;
                let failure = match outcome {
                    Ok(Ok(())) => continue,
                    Ok(Err(error)) => ListenerFailed::error::<M>(event, &snapshot, error),
                    Err(panic) => ListenerFailed::panic::<M>(event, &snapshot, &*panic),
                };
                failure.report(&ctx).await;
            }
        })
    });
}

/// A listener that failed after its unit of work committed.
#[derive(Debug, thiserror::Error)]
#[error("a {model} listener of `{event}` (record {key}) failed after the commit: {message}")]
pub(crate) struct ListenerFailed {
    model: &'static str,
    event: &'static str,
    key: String,
    message: String,
    #[source]
    source: Option<crate::error::OrmError>,
}

impl ListenerFailed {
    fn error<M: Model>(
        event: ModelEvent,
        record: &Record<M>,
        error: crate::error::OrmError,
    ) -> Self {
        ListenerFailed {
            model: M::meta().name(),
            event: event.name(),
            key: record.key().to_key_string(),
            message: error.to_string(),
            source: Some(error),
        }
    }

    fn panic<M: Model>(event: ModelEvent, record: &Record<M>, panic: &(dyn Any + Send)) -> Self {
        ListenerFailed {
            model: M::meta().name(),
            event: event.name(),
            key: record.key().to_key_string(),
            message: format!("it panicked: {}", panic_message(panic)),
            source: None,
        }
    }

    /// Logs the failure (target `navree_orm::events`) and reports it to
    /// the app's telemetry sinks.
    pub(crate) async fn report(&self, ctx: &ExecContext) {
        tracing::error!(
            target: "navree_orm::events",
            model = self.model,
            event = self.event,
            key = %self.key,
            error = %self.message,
            "a model listener failed after the commit"
        );
        if let Some(app) = ctx.app() {
            navree_telemetry::Telemetry::from_app(app)
                .await
                .report_exception(self);
        }
    }
}

/// The message a panic carries, when it is text.
fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "a non-text panic payload".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn silence_lasts_as_long_as_the_future() {
        assert!(!events_are_silenced());
        without_events(async {
            assert!(events_are_silenced());
            without_events(async { assert!(events_are_silenced()) }).await;
            assert!(events_are_silenced());
        })
        .await;
        assert!(!events_are_silenced());
    }

    #[test]
    fn panic_messages_are_read_from_text_payloads() {
        assert_eq!(panic_message(&"boom"), "boom");
        assert_eq!(panic_message(&String::from("bang")), "bang");
        assert_eq!(panic_message(&7_u8), "a non-text panic payload");
    }
}
