//! [`SearchModule`].

use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_db::ModuleBuilderDbExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver};

use crate::commands::commands;
use crate::error::map_search_error;
use crate::index::IndexRegistration;
use crate::migration::migrations;
use crate::sync::{SearchObserver, SyncDocuments};
use crate::{Search, SearchError, SearchOptions, Searchable};

/// Full-text and vector search over the app's models.
///
/// Global; provides and exports [`Search`] and [`SearchOptions`], and
/// contributes:
///
/// - a change observer that queues a `search.sync` job
///   ([`SyncDocuments`]) for every committed write to a searchable model
///   made through the model API (or syncs inline, per `search.sync`);
/// - the `search` migrations (`search_documents`, used by the `postgres`
///   engine);
/// - the commands `search:import`, `search:flush`, `search:settings` and
///   `search:status`;
/// - the mapping of [`SearchError`]s returned from handlers to error
///   responses (400 for invalid filter or sort fields, 503 when the engine
///   is unavailable).
///
/// At boot it pushes every index's settings to the engine. Register
/// models with [`for_feature`](SearchModule::for_feature) in the modules
/// that own them.
///
/// ```no_run
/// use navree_kernel::Navree;
/// use navree_search::{Engine, SearchModule};
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     // ConfigModule, DatabaseModule, QueueModule, ...
///     .module(SearchModule::for_root(Engine::memory()))
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(SearchModule)`) or through
/// [`from_config`](SearchModule::from_config), it reads the `search`
/// configuration section, so the app must include `ConfigModule`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchModule;

impl SearchModule {
    /// The module with an engine, or full [`SearchOptions`] (configure it
    /// once, in the app root).
    pub fn for_root(options: impl Into<SearchOptions>) -> impl Module {
        ConfiguredModule::<SearchModule, _>::for_root(options.into(), register)
    }

    /// The module configured from section `search` of the app
    /// configuration.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<SearchModule, SearchOptions>::from_config(register)
    }

    /// Registers model `M`: its index is added to the commands, its
    /// settings pushed at boot, and its changes synced.
    pub fn for_feature<M: Searchable>() -> impl Module {
        SearchFeature::<M>(PhantomData)
    }
}

impl Module for SearchModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<SearchOptions>) {
    m.global();
    match options {
        Some(options) => {
            let options = Arc::new(options);
            m.factory(Factory::<SearchOptions>::new(move |_| {
                let options = options.clone();
                async move {
                    options
                        .check()
                        .map_err(|message| format!("invalid search options: {message}"))?;
                    Ok(options)
                }
            }));
        }
        None => {
            m.config::<SearchOptions>();
        }
    }
    m.exports::<SearchOptions>()
        .factory(
            Factory::<Search>::new(|r: Resolver| async move {
                let options = r.get::<SearchOptions>().await?;
                let app = r.app();
                let search = Search::build(&app, (*options).clone())
                    .await
                    .map_err(|error| format!("cannot start search: {error}"))?;
                Ok(Arc::new(search))
            })
            .deps(vec![ProviderKey::of::<SearchOptions>()]),
        )
        .exports::<Search>()
        .lifecycle::<Search>()
        .change_observer::<SearchObserver>()
        .job::<SyncDocuments>()
        .migrations(migrations())
        .map_error::<SearchError>(map_search_error);
    for command in commands() {
        m.command(command);
    }
}

/// `SearchModule::for_feature::<M>()`.
struct SearchFeature<M>(PhantomData<fn() -> M>);

impl<M: Searchable> Module for SearchFeature<M> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(IndexRegistration::of::<M>());
    }
}
