//! Broadcasting model changes on real-time channels.
//!
//! A model marked `#[model(broadcasts)]` (with `impl Broadcasts for ..`)
//! announces its records' changes to the app's
//! [`Broadcaster`](navree_contracts::Broadcaster) (navree-realtime's): on
//! `created`, `updated`, `deleted`, and for soft-deleting models `trashed`
//! and `restored`. The broadcast goes out once the unit of work commits
//! (at once outside one), never for a write that was rolled back:
//!
//! | | Default | Override |
//! |---|---|---|
//! | channels | the record's private channel, `private-deal.{key}` | [`Broadcasts::broadcast_on`] (none: no broadcast) |
//! | event name | `deal.created`, `deal.updated`, ... | [`Broadcasts::broadcast_as`] |
//! | data | `{"deal": <the record>}` | [`Broadcasts::broadcast_with`] |
//! | delivery | right after the commit | [`Broadcasts::broadcast_queue`]: a job on that queue |
//!
//! [`without_broadcasting`] (and [`without_events`](crate::without_events),
//! the quiet operations) silences it.
//!
//! ```no_run
//! use chrono::{DateTime, Utc};
//! use navree_orm::broadcast::Broadcasts;
//! use navree_orm::prelude::*;
//!
//! #[derive(Model, Clone, Debug, Default, serde::Serialize)]
//! #[model(timestamps, broadcasts)]
//! struct Deal {
//!     id: i64,
//!     title: String,
//!     created_at: DateTime<Utc>,
//!     updated_at: DateTime<Utc>,
//! }
//!
//! impl Broadcasts for Deal {
//!     /// Every board shows every deal.
//!     fn broadcast_on(_deal: &Record<Self>, _event: ModelEvent) -> Vec<String> {
//!         vec!["private-crm.deals".to_owned()]
//!     }
//! }
//! ```

use std::fmt;
use std::future::Future;
use std::marker::PhantomData;

use navree_contracts::{Broadcaster, Dispatcher, Job, JobContext, JobEnvelope};
use navree_kernel::{BoxError, BoxFuture};
use serde::{Deserialize, Serialize};

use crate::error::{OrmError, Result};
use crate::exec::ExecContext;
use crate::hooks::{HookKind, ModelEvent, ModelHook};
use crate::model::{Model, PrimaryKey};
use crate::record::Record;

/// How a model's changes are broadcast. Every method has a default; mark
/// the model `#[model(broadcasts)]` and implement the ones to change.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is marked `broadcasts` but does not implement `Broadcasts`",
    note = "add `impl navree_orm::broadcast::Broadcasts for {Self} {{}}` (every method has a default)"
)]
pub trait Broadcasts: Model {
    /// The channels `event` of `record` goes to (full names, with their
    /// `private-` or `presence-` prefix); none skips the broadcast. By
    /// default the record's private channel, `private-deal.{key}`.
    fn broadcast_on(record: &Record<Self>, event: ModelEvent) -> Vec<String> {
        let _ = event;
        vec![channel_of(record)]
    }

    /// The event name: `deal.created` by default.
    fn broadcast_as(record: &Record<Self>, event: ModelEvent) -> String {
        let _ = record;
        format!("{}.{}", snake_name(Self::meta().name()), event.name())
    }

    /// The data: `{"deal": <the record>}` by default (the record as it
    /// serializes, with its extras and loaded relations).
    fn broadcast_with(record: &Record<Self>, event: ModelEvent) -> Result<serde_json::Value> {
        let _ = event;
        let value = serde_json::to_value(record).map_err(|error| OrmError::Hook {
            model: Self::meta().name().into(),
            message: format!("cannot serialize the record to broadcast it: {error}"),
        })?;
        let mut data = serde_json::Map::new();
        data.insert(snake_name(Self::meta().name()), value);
        Ok(serde_json::Value::Object(data))
    }

    /// The queue that broadcasts the event (as job `orm.broadcast`, when
    /// the app has a job dispatcher); `None`, the default, broadcasts right
    /// after the commit.
    fn broadcast_queue(record: &Record<Self>, event: ModelEvent) -> Option<String> {
        let _ = (record, event);
        None
    }
}

/// The private channel of `record`: `private-{model}.{key}` (`private-deal.42`,
/// `private-log_entry.7`).
pub fn channel_of<M: Model>(record: &Record<M>) -> String {
    format!(
        "private-{}.{}",
        snake_name(M::meta().name()),
        record.key().to_key_string()
    )
}

/// `LogEntry` → `log_entry`.
fn snake_name(name: &str) -> String {
    heck::ToSnakeCase::to_snake_case(name)
}

tokio::task_local! {
    /// Set while [`without_broadcasting`] runs a future.
    static SILENCED: ();
}

/// Runs `future` without broadcasting any model change (writes and their
/// other listeners go on as usual). The silence belongs to the task.
pub async fn without_broadcasting<F: Future>(future: F) -> F::Output {
    SILENCED.scope((), future).await
}

/// Whether the current task runs inside [`without_broadcasting`].
pub fn broadcasting_is_silenced() -> bool {
    SILENCED.try_with(|()| ()).is_ok()
}

/// The hook `#[model(broadcasts)]` adds: broadcasts `M`'s changes. A
/// [`HookKind::Listener`] (after the commit, silenced by quiet
/// operations).
pub struct BroadcastHook<M>(PhantomData<fn() -> M>);

impl<M> BroadcastHook<M> {
    /// The hook.
    pub const fn new() -> Self {
        BroadcastHook(PhantomData)
    }
}

impl<M> Default for BroadcastHook<M> {
    fn default() -> Self {
        BroadcastHook::new()
    }
}

impl<M> fmt::Debug for BroadcastHook<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BroadcastHook")
    }
}

impl<M: Broadcasts> ModelHook<M> for BroadcastHook<M> {
    fn after<'a>(
        &'a self,
        event: ModelEvent,
        ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let broadcast = matches!(
                event,
                ModelEvent::Created
                    | ModelEvent::Updated
                    | ModelEvent::Deleted
                    | ModelEvent::Trashed
                    | ModelEvent::Restored
            );
            if !broadcast || broadcasting_is_silenced() {
                return Ok(());
            }
            let channels = M::broadcast_on(record, event);
            if channels.is_empty() {
                return Ok(());
            }
            let message = BroadcastModelEvent {
                channels,
                event: M::broadcast_as(record, event),
                data: M::broadcast_with(record, event)?,
            };
            message
                .deliver(ctx, M::broadcast_queue(record, event))
                .await
                .map_err(|error| OrmError::Hook {
                    model: M::meta().name().into(),
                    message: format!("cannot broadcast `{}`: {error}", event.name()),
                })
        })
    }

    fn kind(&self) -> HookKind {
        HookKind::Listener
    }
}

/// A model change to broadcast; also the job (`orm.broadcast`) that
/// broadcasts it from a queue worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BroadcastModelEvent {
    /// The channels.
    pub channels: Vec<String>,
    /// The event name.
    pub event: String,
    /// The data.
    pub data: serde_json::Value,
}

impl BroadcastModelEvent {
    /// Hands the broadcast to `queue` (when there is one and the app has a
    /// job dispatcher), else to the app's broadcaster.
    async fn deliver(
        self,
        ctx: &ExecContext,
        queue: Option<String>,
    ) -> std::result::Result<(), BoxError> {
        let Some(app) = ctx.app() else {
            return Ok(());
        };
        if let Some(queue) = queue
            && let Some(dispatcher) = app.try_get::<dyn Dispatcher>().await
        {
            let mut envelope = JobEnvelope::of(&self)?;
            envelope.options.queue = queue;
            dispatcher.dispatch_envelope(envelope).await?;
            return Ok(());
        }
        match app.try_get::<dyn Broadcaster>().await {
            Some(broadcaster) => {
                broadcaster
                    .broadcast(self.channels, self.event, self.data)
                    .await
            }
            None => {
                tracing::debug!(target: "navree_orm::broadcast", event = %self.event, "no broadcaster in the app: the model change is not broadcast");
                Ok(())
            }
        }
    }
}

impl Job for BroadcastModelEvent {
    const NAME: &'static str = "orm.broadcast";

    async fn handle(self, ctx: JobContext) -> std::result::Result<(), BoxError> {
        let broadcaster = ctx.app.get::<dyn Broadcaster>().await?;
        broadcaster
            .broadcast(self.channels, self.event, self.data)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_become_snake_case() {
        assert_eq!(snake_name("Deal"), "deal");
        assert_eq!(snake_name("LogEntry"), "log_entry");
        assert_eq!(snake_name("APIKey"), "api_key");
    }

    #[tokio::test]
    async fn silence_lasts_as_long_as_the_future() {
        assert!(!broadcasting_is_silenced());
        without_broadcasting(async { assert!(broadcasting_is_silenced()) }).await;
        assert!(!broadcasting_is_silenced());
    }
}
