//! [`MongoModule`], the `mongodb` health check, and
//! [`ModuleBuilderMongoExt`].

use std::sync::Arc;
use std::time::Duration;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_contracts::{HealthCheck, HealthStatus};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Factory, Injectable, Lifecycle, Module,
    ModuleBuilder, ModuleId, ProviderKey, Resolver,
};
use navree_telemetry::Telemetry;

use crate::connection::MongoDb;
use crate::migrate::{self, MongoMigrationSet, commands};
use crate::options::MongoOptions;
use crate::uow::MongoChangeObserver;

/// The MongoDB module. Global; provides and exports [`MongoDb`] (the
/// connection the app's models run on) and [`MongoOptions`]; registers the
/// `mongodb` readiness check, the migration commands (`mongo:migrate`,
/// `mongo:migrate:rollback`, `mongo:migrate:reset`, `mongo:migrate:refresh`,
/// `mongo:migrate:fresh`, `mongo:migrate:status`) and, with
/// `migrate_on_boot`, runs pending migrations while [`MongoDb`] is built.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
/// use navree_mongo::MongoModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// // config/app.toml: [database.mongodb] url = "mongodb://127.0.0.1:27017/crm?replicaSet=rs0"
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(MongoModule::from_config()) // section `database.mongodb`
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(MongoModule)`) or through
/// [`from_config`](MongoModule::from_config), it reads `database.mongodb`,
/// so the app must include `ConfigModule`. Invalid options fail the boot;
/// an unreachable server does not (the client connects on first use).
#[derive(Clone, Copy, Debug, Default)]
pub struct MongoModule;

impl MongoModule {
    /// The module with explicit options (configure it once, in the app
    /// root).
    pub fn for_root(options: MongoOptions) -> impl Module {
        ConfiguredMongoModule {
            options: Some(options),
        }
    }

    /// The module configured from the `database.mongodb` section at boot.
    pub fn from_config() -> impl Module {
        ConfiguredMongoModule { options: None }
    }
}

impl Module for MongoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `MongoModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredMongoModule {
    options: Option<MongoOptions>,
}

impl Module for ConfiguredMongoModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<MongoModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<MongoOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<MongoOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check().map_err(|problems| {
                    format!("invalid MongoDB options: {}", problems.join("; "))
                })?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<MongoOptions>(),
    };
    let mongo = ProviderKey::of::<MongoDb>();
    m.exports::<MongoOptions>()
        .factory(
            Factory::<MongoDb>::new(|r: Resolver| async move {
                let mut options = (*r.get::<MongoOptions>().await?).clone();
                let app = r.app();
                if options.app_name.is_none()
                    && let Some(config) = app.try_get::<Config>().await
                {
                    options.app_name = config.get_string("app.name");
                }
                let telemetry = Telemetry::from_app(&app).await;
                let db = MongoDb::connect_with(&options, telemetry)
                    .await?
                    .with_app(app.downgrade());
                if options.migrate_on_boot {
                    migrate::MongoMigrator::new(&db, migrate::sets_of(&app))
                        .up(&migrate::MongoMigrateOptions::default())
                        .await?;
                }
                Ok(Arc::new(db))
            })
            .deps(vec![ProviderKey::of::<MongoOptions>()]),
        )
        .exports::<MongoDb>()
        .factory(
            Factory::<ClientCloser>::new(|r: Resolver| async move {
                Ok(Arc::new(ClientCloser(r.get::<MongoDb>().await?)))
            })
            .deps(vec![mongo.clone()]),
        )
        .lifecycle::<ClientCloser>()
        .multi(
            Factory::<dyn HealthCheck>::new(|r: Resolver| async move {
                Ok(Arc::new(MongoHealth(r.get::<MongoDb>().await?)) as Arc<dyn HealthCheck>)
            })
            .deps(vec![mongo]),
        )
        .command(CommandSpec::new(
            "mongo:migrate",
            "Run every pending MongoDB migration ([--pretend] [--step])",
            commands::migrate,
        ))
        .command(CommandSpec::new(
            "mongo:migrate:rollback",
            "Roll back the last batch of MongoDB migrations ([--step <n> | --batch <n>] [--pretend] [--force])",
            commands::rollback,
        ))
        .command(CommandSpec::new(
            "mongo:migrate:reset",
            "Roll back every MongoDB migration ([--pretend] [--force])",
            commands::reset,
        ))
        .command(CommandSpec::new(
            "mongo:migrate:refresh",
            "Roll back every MongoDB migration (or --step <n>), then migrate ([--force])",
            commands::refresh,
        ))
        .command(CommandSpec::new(
            "mongo:migrate:fresh",
            "Drop every collection of the database and run every MongoDB migration ([--force])",
            commands::fresh,
        ))
        .command(CommandSpec::new(
            "mongo:migrate:status",
            "Show which MongoDB migrations have run ([--pending])",
            commands::status,
        ));
}

/// Shuts the client's pools down with the app.
struct ClientCloser(Arc<MongoDb>);

impl Lifecycle for ClientCloser {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.client().clone().shutdown().immediate(true).await;
            Ok(())
        })
    }
}

/// How long the readiness check waits for the server.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

/// The `mongodb` readiness check: the server answers `ping`.
struct MongoHealth(Arc<MongoDb>);

impl HealthCheck for MongoHealth {
    fn name(&self) -> &str {
        "mongodb"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async move {
            match tokio::time::timeout(HEALTH_TIMEOUT, self.0.ping()).await {
                Ok(Ok(())) => HealthStatus::Up,
                Ok(Err(error)) => HealthStatus::Down(format!(
                    "MongoDB {} does not answer: {error}",
                    self.0.options().redacted_url()
                )),
                Err(_) => HealthStatus::Down(format!(
                    "MongoDB {} did not answer within {} s",
                    self.0.options().redacted_url(),
                    HEALTH_TIMEOUT.as_secs()
                )),
            }
        })
    }
}

/// Registers MongoDB migrations and change observers on a
/// [`ModuleBuilder`].
///
/// ```
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_mongo::{ModuleBuilderMongoExt, MongoMigrationSet};
///
/// struct Billing;
///
/// impl Module for Billing {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // The module's migrations run after the users module's.
///         m.mongo_migrations(MongoMigrationSet::new("billing").after(["users"]));
///     }
/// }
/// ```
pub trait ModuleBuilderMongoExt {
    /// Contributes the module's MongoDB migrations (run by
    /// `mongo:migrate`).
    fn mongo_migrations(&mut self, set: MongoMigrationSet) -> &mut Self;

    /// Registers `O` as a `dyn MongoChangeObserver` multi-binding: it sees
    /// the changes of every MongoDB unit of work inside its transaction,
    /// right before commit.
    fn mongo_change_observer<O: MongoChangeObserver + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderMongoExt for ModuleBuilder {
    fn mongo_migrations(&mut self, set: MongoMigrationSet) -> &mut Self {
        self.contribute(set)
    }

    fn mongo_change_observer<O: MongoChangeObserver + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn MongoChangeObserver>::new(|r: Resolver| async move {
                Ok(Arc::new(O::construct(r).await?) as Arc<dyn MongoChangeObserver>)
            })
            .deps(O::dependencies()),
        )
    }
}
