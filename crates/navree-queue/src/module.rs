//! [`QueueModule`].

use std::sync::Arc;

use navree_config::{Config, ConfigSection, ModuleBuilderConfigExt};
use navree_contracts::{Clock, Dispatcher, JobMonitor, JobRegistration};
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};
use navree_telemetry::{Telemetry, TracePolicy};
use navree_realtime::ModuleBuilderRealtimeExt;

use crate::operations::{QueueMonitor, authorize_channel};
use crate::store::{DatabaseStore, MemoryStore, Store};
use crate::{
    Operations, OperationsController, Queue, QueueDriver, QueueOptions, commands, migration,
};

/// The queue module. Global; provides and exports [`Queue`] (also as the
/// app's `dyn Dispatcher`, so mail, events, notifications and schedules
/// queue through it) and [`QueueOptions`]; contributes the `queue`
/// migrations (tables `jobs` and `failed_jobs`); registers the commands
/// `queue:work [--queue a,b] [--once] [--stop-when-empty] [--max-jobs N]`,
/// `queue:failed`, `queue:retry <id|all>` and `queue:flush`.
///
/// Workers run the handler of every
/// [`JobRegistration`](navree_contracts::JobRegistration) contributed by the
/// app's modules (`m.job::<J>()`).
///
/// ```no_run
/// use navree_kernel::Navree;
/// use navree_queue::{QueueDriver, QueueModule, QueueOptions};
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(QueueModule::for_root(QueueOptions {
///         driver: QueueDriver::Memory,
///         ..QueueOptions::default()
///     }))
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(QueueModule)`) or through
/// [`from_config`](QueueModule::from_config), it reads the `queue`
/// configuration section, so the app must include `ConfigModule`. The
/// database driver (the default) needs `DatabaseModule`.
#[derive(Clone, Copy, Debug, Default)]
pub struct QueueModule;

impl QueueModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: QueueOptions) -> impl Module {
        ConfiguredQueueModule {
            options: Some(options),
        }
    }

    /// The module configured from section `queue` of the app configuration.
    pub fn from_config() -> impl Module {
        ConfiguredQueueModule { options: None }
    }
}

impl Module for QueueModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `QueueModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredQueueModule {
    /// `None`: read from configuration.
    options: Option<QueueOptions>,
}

impl Module for ConfiguredQueueModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<QueueModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<QueueOptions>) {
    m.global();
    let mut queue_deps = vec![ProviderKey::of::<QueueOptions>()];
    match options {
        Some(options) => {
            if options.driver == QueueDriver::Database {
                queue_deps.push(ProviderKey::of::<Db>());
            }
            let options = Arc::new(options);
            m.factory(Factory::<QueueOptions>::new(move |_| {
                let options = options.clone();
                async move {
                    options
                        .validate()
                        .map_err(|message| format!("invalid queue options: {message}"))?;
                    Ok(options)
                }
            }));
        }
        None => {
            m.config::<QueueOptions>();
        }
    }
    m.exports::<QueueOptions>()
        .factory(
            Factory::<Queue>::new(|r: Resolver| async move {
                let options = r.get::<QueueOptions>().await?;
                let store = match options.driver {
                    QueueDriver::Database => {
                        Store::Database(DatabaseStore::new((*r.get::<Db>().await?).clone()))
                    }
                    QueueDriver::Memory => Store::Memory(MemoryStore::default()),
                };
                let app = r.app();
                let clock = app.try_get::<dyn Clock>().await;
                let registrations = app
                    .contributions::<JobRegistration>()
                    .into_iter()
                    .map(|c| (*c.value).clone());
                let telemetry = Telemetry::from_app(&app).await;
                let policy = match app.try_get::<Config>().await {
                    Some(config) => TracePolicy::from_settings(
                        |key| config.get_string(key),
                        |name| std::env::var(name).ok(),
                    )?,
                    None => TracePolicy::default(),
                };
                Ok(Arc::new(
                    Queue::new(
                        store,
                        (*options).clone(),
                        clock,
                        registrations,
                        Some(app.downgrade()),
                    )
                    .instrumented(telemetry, policy),
                ))
            })
            .deps(queue_deps),
        )
        .exports::<Queue>()
        .factory(
            Factory::<dyn Dispatcher>::new(|r: Resolver| async move {
                Ok(r.get::<Queue>().await? as Arc<dyn Dispatcher>)
            })
            .deps(vec![ProviderKey::of::<Queue>()]),
        )
        .exports::<dyn Dispatcher>()
        .factory(
            Factory::<dyn JobMonitor>::new(|r: Resolver| async move {
                let queue = (*r.get::<Queue>().await?).clone();
                Ok(Arc::new(QueueMonitor { queue }) as Arc<dyn JobMonitor>)
            })
            .deps(vec![ProviderKey::of::<Queue>()]),
        )
        .exports::<dyn JobMonitor>()
        // Operations live in the app database; an app without
        // `DatabaseModule` still boots, and using them is
        // `QueueError::NoDatabase`.
        .factory(
            Factory::<Operations>::new(|r: Resolver| async move {
                let queue = (*r.get::<Queue>().await?).clone();
                let options = r.get::<QueueOptions>().await?;
                let app = r.app();
                let db = app.try_get::<Db>().await.map(|db| (*db).clone());
                Ok(Arc::new(Operations::new(
                    db,
                    queue,
                    options.operations.clone(),
                    Some(app.downgrade()),
                )))
            })
            .deps(vec![
                ProviderKey::of::<Queue>(),
                ProviderKey::of::<QueueOptions>(),
            ]),
        )
        .exports::<Operations>()
        .controller::<OperationsController>()
        .channel("operations.{id}", authorize_channel)
        .migrations(migration::migrations());
    for command in commands::commands() {
        m.command(command);
    }
}
