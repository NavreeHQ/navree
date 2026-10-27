//! [`ModelEventFake`]: model events recorded for tests instead of heard by
//! listeners.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use navree_kernel::BoxFuture;

use crate::error::Result;
use crate::exec::ExecContext;
use crate::hooks::{Flow, HookKind, ModelEvent, ModelHook};
use crate::model::{Model, PrimaryKey};
use crate::record::Record;

/// Replaces an app's model event listeners in tests: while it is in the
/// app, observers, closure listeners, typed application events and
/// broadcasts do not run; the events they would have heard are recorded
/// instead, when they would have heard them (an "-ed" event inside a unit
/// of work once it commits). Behaviours (touching parents, ...) still run.
///
/// Put it in the app with `override_value`:
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db) -> Result<()> {
/// use std::sync::Arc;
///
/// use navree_orm::events::ModelEventFake;
///
/// let fake = Arc::new(ModelEventFake::new());
/// // TestApp::builder().override_value(fake.clone()) ...
/// Tag::create(&db, Tag { name: "vip".into(), ..Default::default() }).await?;
/// fake.assert_fired::<Tag>(ModelEvent::Created);
/// fake.assert_not_fired::<Tag>(ModelEvent::Deleted);
/// # Ok(()) }
/// ```
#[derive(Default)]
pub struct ModelEventFake {
    fired: Mutex<Vec<FiredEvent>>,
}

/// A model event the fake recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiredEvent {
    /// The model, e.g. `Deal`.
    pub model: &'static str,
    /// The event.
    pub event: ModelEvent,
    /// The record's key, as text.
    pub key: String,
}

impl ModelEventFake {
    /// A fake that has recorded nothing.
    pub fn new() -> Self {
        ModelEventFake::default()
    }

    /// Every event recorded, in order.
    pub fn fired(&self) -> Vec<FiredEvent> {
        self.lock().clone()
    }

    /// The keys of the records of `M` that `event` fired for, in order.
    pub fn keys<M: Model>(&self, event: ModelEvent) -> Vec<String> {
        self.lock()
            .iter()
            .filter(|fired| fired.model == M::meta().name() && fired.event == event)
            .map(|fired| fired.key.clone())
            .collect()
    }

    /// Forgets what was recorded.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Panics unless `event` fired for a record of `M`.
    #[track_caller]
    pub fn assert_fired<M: Model>(&self, event: ModelEvent) {
        if self.keys::<M>(event).is_empty() {
            panic!(
                "expected {} `{event}` to fire, but it did not; fired: {}",
                M::meta().name(),
                self.summary()
            );
        }
    }

    /// Panics unless `event` fired exactly `times` times for records of
    /// `M`.
    #[track_caller]
    pub fn assert_fired_times<M: Model>(&self, event: ModelEvent, times: usize) {
        let fired = self.keys::<M>(event).len();
        if fired != times {
            panic!(
                "expected {} `{event}` to fire {times} time(s), but it fired {fired}; fired: {}",
                M::meta().name(),
                self.summary()
            );
        }
    }

    /// Panics if `event` fired for a record of `M`.
    #[track_caller]
    pub fn assert_not_fired<M: Model>(&self, event: ModelEvent) {
        let keys = self.keys::<M>(event);
        if !keys.is_empty() {
            panic!(
                "expected {} `{event}` not to fire, but it fired for {}",
                M::meta().name(),
                keys.join(", ")
            );
        }
    }

    /// Panics if any event fired.
    #[track_caller]
    pub fn assert_nothing_fired(&self) {
        if !self.lock().is_empty() {
            panic!("expected no model event, but these fired: {}", self.summary());
        }
    }

    fn record<M: Model>(&self, event: ModelEvent, record: &Record<M>) {
        self.lock().push(FiredEvent {
            model: M::meta().name(),
            event,
            key: record.key().to_key_string(),
        });
    }

    fn summary(&self) -> String {
        let fired = self.lock();
        if fired.is_empty() {
            return "(nothing)".to_owned();
        }
        fired
            .iter()
            .map(|fired| format!("{} `{}` ({})", fired.model, fired.event, fired.key))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<FiredEvent>> {
        self.fired.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for ModelEventFake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelEventFake")
            .field("fired", &self.lock().len())
            .finish()
    }
}

/// The listener standing in for an app's listeners while the fake is in
/// the app.
pub(crate) struct FakeListener(pub(crate) Arc<ModelEventFake>);

impl<M: Model> ModelHook<M> for FakeListener {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<Flow>> {
        self.0.record(event, record);
        Box::pin(async { Ok(Flow::Continue) })
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<M>,
    ) -> BoxFuture<'a, Result<()>> {
        self.0.record(event, record);
        Box::pin(async { Ok(()) })
    }

    fn kind(&self) -> HookKind {
        HookKind::Listener
    }
}
