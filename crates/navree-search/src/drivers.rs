//! Search engines a module registers under a name:
//! [`ModuleBuilderSearchExt`].

use std::future::Future;
use std::sync::Arc;

use navree_config::__private::register_driver;
use navree_config::{DriverContext, DriverSlot};
use navree_kernel::{BoxError, ModuleBuilder};

use crate::SearchEngine;

/// The selector `search.engine`, its built-in engines (`database` is
/// another name of `postgres`) and the method that registers more.
pub(crate) const SLOT: DriverSlot = DriverSlot::new(
    "search.engine",
    &[
        "postgres",
        "database",
        "meilisearch",
        "typesense",
        "tantivy",
        "memory",
    ],
    "m.search_engine",
    "search engine",
);

/// What the boot builds before `Search`: it holds that every registration
/// of a search engine was checked. A provider of its own, so an app that
/// replaces `Search` still has its registrations checked.
pub(crate) struct CheckedEngines;

/// Registers search engines on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::{Module, ModuleBuilder, Navree};
/// use navree_search::{MemoryEngine, ModuleBuilderSearchExt, SearchModule};
///
/// struct KoshModule;
///
/// impl Module for KoshModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // Selected with `search.engine = "kosh"`; its settings are `[search.kosh]`.
///         m.search_engine("kosh", |_ctx| async move { Ok(MemoryEngine::new()) });
///     }
/// }
///
/// fn main() -> Result<(), navree_kernel::BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(SearchModule::from_config())
///         .module(KoshModule)
///         .run()
/// }
/// ```
pub trait ModuleBuilderSearchExt {
    /// Registers a search engine under `name` (a lower-case letter
    /// followed by up to 63 lower-case letters, digits or `_`):
    /// `search.engine = "{name}"` selects it, and `factory` builds it at
    /// boot from its [`DriverContext`], whose settings are `search.{name}`.
    ///
    /// A name that is not such a word, a built-in name (`postgres`,
    /// `database`, `meilisearch`, `typesense`, `tantivy`, `memory`), a key
    /// of the `search` section (`prefix`) and a name two modules register
    /// fail the boot, naming `m.search_engine` and the module, also when
    /// another engine is selected; `config:check` says the same. A factory
    /// that fails fails the boot, naming the engine and the module. A
    /// factory's error is shown in the boot error as written, so it must not
    /// contain a secret from the driver's settings.
    fn search_engine<E, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        E: SearchEngine,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<E, BoxError>> + Send + 'static;
}

impl ModuleBuilderSearchExt for ModuleBuilder {
    fn search_engine<E, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        E: SearchEngine,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<E, BoxError>> + Send + 'static,
    {
        register_driver::<dyn SearchEngine, _, _>(self, &SLOT, Some("search"), name, move |ctx| {
            let built = factory(ctx);
            async move {
                built
                    .await
                    .map(|engine| Arc::new(engine) as Arc<dyn SearchEngine>)
            }
        })
    }
}
