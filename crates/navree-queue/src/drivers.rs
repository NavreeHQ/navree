//! Queue stores a module registers under a name: [`ModuleBuilderQueueExt`].

use std::future::Future;
use std::sync::Arc;

use navree_config::__private::register_driver;
use navree_config::{DriverContext, DriverSlot};
use navree_kernel::{BoxError, ModuleBuilder};

use crate::QueueStore;

/// The selector `queue.driver`, its built-in drivers and the method that
/// registers more.
pub(crate) const SLOT: DriverSlot = DriverSlot::new(
    "queue.driver",
    &["database", "memory", "redis"],
    "m.queue_store",
    "queue store",
);

/// What the boot builds first: it holds that every registration of a queue
/// store was checked. A provider of its own, so an app that replaces `Queue`
/// (with a fake, in a test) still has its registrations checked.
pub(crate) struct CheckedStores;

/// Registers queue stores on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```
/// use navree_kernel::ModuleBuilder;
/// use navree_queue::{ModuleBuilderQueueExt, QueueStore};
/// use serde::Deserialize;
///
/// /// The settings of the store: `[queue.amritsar_queue]`.
/// #[derive(Default, Deserialize)]
/// #[serde(default)]
/// struct AmritsarSettings {
///     depot: String,
/// }
///
/// /// What a module's `register` calls: `open` builds your `QueueStore`
/// /// from the settings the app configured. Selected with
/// /// `queue.driver = "amritsar_queue"`.
/// fn register_store<S: QueueStore>(m: &mut ModuleBuilder, open: fn(AmritsarSettings) -> S) {
///     m.queue_store("amritsar_queue", move |ctx| {
///         let store = ctx.settings().map(open);
///         async move { store }
///     });
/// }
/// ```
pub trait ModuleBuilderQueueExt {
    /// Registers a queue store under `name` (a lower-case letter followed by
    /// up to 63 lower-case letters, digits or `_`): `queue.driver = "{name}"`
    /// selects it, and `factory` builds it at boot from its
    /// [`DriverContext`], whose settings are `queue.{name}`.
    ///
    /// A name that is not such a word, a built-in name (`database`, `memory`,
    /// `redis`), a key of the `queue` section (`workers`) and a name two
    /// modules register fail the boot, naming `m.queue_store` and the
    /// module, also when another store is selected; `config:check` says the
    /// same. A factory that fails fails the boot, naming the store and the
    /// module.
    fn queue_store<S, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        S: QueueStore,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S, BoxError>> + Send + 'static;
}

impl ModuleBuilderQueueExt for ModuleBuilder {
    fn queue_store<S, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        S: QueueStore,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S, BoxError>> + Send + 'static,
    {
        register_driver::<dyn QueueStore, _, _>(self, &SLOT, Some("queue"), name, move |ctx| {
            let built = factory(ctx);
            async move {
                built
                    .await
                    .map(|store| Arc::new(store) as Arc<dyn QueueStore>)
            }
        })
    }
}
