//! [`DatabaseModule`], [`ModuleBuilderDbExt`] and the database commands.

use std::sync::Arc;

use navree_config::Config;
use navree_contracts::{HealthCheck, HealthStatus};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Factory as ProviderFactory, Injectable,
    Lifecycle, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver,
};
use navree_telemetry::Telemetry;
use sea_orm::DbErr;

use crate::http::{map_db_error, uow_layer};
use crate::migrate::{self, MigrationSet, commands};
use crate::seed::run_seeders;
use crate::{ChangeObserver, Db, DbConnections, DbOptions, Seeder};

/// The database module. Global; provides and exports [`Db`],
/// [`DbOptions`] and [`DbConnections`] (the named connections of
/// `[database.connections.<name>]`); contributes the unit-of-work layer
/// ([`LayerSlot::Uow`](navree_http::LayerSlot::Uow)), the `DbErr` → error
/// mapping and the `database` health check (a `dyn HealthCheck`
/// multi-binding); registers the commands `migrate`, `migrate:install`,
/// `migrate:rollback`, `migrate:reset`, `migrate:refresh`, `migrate:fresh`,
/// `migrate:status`, `schema:dump` and `db:seed`; closes the pool at
/// shutdown. With `migrate_on_boot`, pending migrations run while [`Db`] is
/// constructed, before any provider that depends on it.
///
/// ```no_run
/// use navree_db::{DatabaseModule, DbOptions};
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(DatabaseModule::for_root(DbOptions {
///         url: "sqlite://storage/app.db?mode=rwc".into(),
///         ..DbOptions::default()
///     }))
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(DatabaseModule)`) or through
/// [`from_config`](DatabaseModule::from_config), it reads the `database`
/// configuration section, so the app must include `ConfigModule`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DatabaseModule;

impl DatabaseModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: DbOptions) -> impl Module {
        ConfiguredDatabaseModule {
            options: Some(options),
        }
    }

    /// The module configured from the `database` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredDatabaseModule { options: None }
    }
}

impl Module for DatabaseModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `DatabaseModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredDatabaseModule {
    options: Option<DbOptions>,
}

impl Module for ConfiguredDatabaseModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<DatabaseModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<DbOptions>) {
    m.global();
    match options {
        Some(options) => m.value(options),
        None => m.factory(
            ProviderFactory::<DbOptions>::new(|r: Resolver| async move {
                let config = r.get::<Config>().await?;
                Ok(Arc::new(DbOptions::from_config(&config)?))
            })
            .deps(vec![ProviderKey::of::<Config>()]),
        ),
    };
    m.exports::<DbOptions>()
        .factory(
            ProviderFactory::<Db>::new(|r: Resolver| async move {
                let options = r.get::<DbOptions>().await?;
                let telemetry = Telemetry::from_app(&r.app()).await;
                let db = Db::connect_with_telemetry(&options, telemetry)
                    .await?
                    .with_app(r.app().downgrade());
                if options.migrate_on_boot {
                    migrate::migrate_on_boot(&db, &r.app(), &options.url).await?;
                }
                Ok(Arc::new(db))
            })
            .deps(vec![ProviderKey::of::<DbOptions>()]),
        )
        .exports::<Db>()
        .factory(
            ProviderFactory::<DbConnections>::new(|r: Resolver| async move {
                let options = r.get::<DbOptions>().await?;
                let db = r.get::<Db>().await?;
                let telemetry = Telemetry::from_app(&r.app()).await;
                Ok(Arc::new(DbConnections::new(
                    db,
                    &options,
                    Some(r.app().downgrade()),
                    telemetry,
                )))
            })
            .deps(vec![ProviderKey::of::<DbOptions>(), ProviderKey::of::<Db>()]),
        )
        .exports::<DbConnections>()
        .lifecycle::<DbConnections>()
        .factory(
            ProviderFactory::<PoolCloser>::new(|r: Resolver| async move {
                Ok(Arc::new(PoolCloser(r.get::<Db>().await?)))
            })
            .deps(vec![ProviderKey::of::<Db>()]),
        )
        .lifecycle::<PoolCloser>()
        .multi(
            ProviderFactory::<dyn HealthCheck>::new(|r: Resolver| async move {
                Ok(Arc::new(DatabaseHealth(r.get::<Db>().await?)) as Arc<dyn HealthCheck>)
            })
            .deps(vec![ProviderKey::of::<Db>()]),
        )
        .http_layer(uow_layer())
        .map_error::<DbErr>(map_db_error)
        .command(CommandSpec::new(
            "migrate",
            "Run every pending migration ([--pretend] [--step] [--seed] [--schema-path <path>])",
            commands::migrate,
        ))
        .command(CommandSpec::new(
            "migrate:install",
            "Create the migrations table",
            commands::install,
        ))
        .command(CommandSpec::new(
            "migrate:rollback",
            "Roll back the last batch ([--step <n> | --batch <n>] [--pretend] [--force])",
            commands::rollback,
        ))
        .command(CommandSpec::new(
            "migrate:reset",
            "Roll back every migration ([--pretend] [--force])",
            commands::reset,
        ))
        .command(CommandSpec::new(
            "migrate:refresh",
            "Roll back every migration (or --step <n>), then migrate ([--seed] [--force])",
            commands::refresh,
        ))
        .command(CommandSpec::new(
            "migrate:fresh",
            "Drop every table and run every migration ([--seed] [--drop-views] [--drop-types] [--force])",
            commands::fresh,
        ))
        .command(CommandSpec::new(
            "migrate:status",
            "Show which migrations have run ([--pending])",
            commands::status,
        ))
        .command(CommandSpec::new(
            "schema:dump",
            "Write the schema and the migrations table to database/schema/<driver>-schema.sql ([--path <path>])",
            commands::dump,
        ))
        .command(CommandSpec::new(
            "db:seed",
            "Run the seeders ([name...] to run only some)",
            seed_command,
        ));
}

/// Closes the pool when the app shuts down (SQLite then checkpoints its WAL).
struct PoolCloser(Arc<Db>);

impl Lifecycle for PoolCloser {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.conn().close_by_ref().await?;
            Ok(())
        })
    }
}

/// The `database` readiness check: pings the pool.
struct DatabaseHealth(Arc<Db>);

impl HealthCheck for DatabaseHealth {
    fn name(&self) -> &str {
        "database"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async move {
            match self.0.conn().ping().await {
                Ok(()) => HealthStatus::Up,
                Err(error) => {
                    tracing::warn!(target: "navree_db", %error, "database health check failed");
                    HealthStatus::Down("the database did not answer a ping".to_owned())
                }
            }
        })
    }
}

/// Database registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderDbExt {
    /// Contributes the module's migrations.
    fn migrations(&mut self, set: MigrationSet) -> &mut Self;

    /// Registers seeder `S` (a `dyn Seeder` multi-binding) for `db:seed`.
    fn seeder<S: Seeder + Injectable>(&mut self) -> &mut Self;

    /// Registers `O` as a `dyn ChangeObserver` multi-binding: it sees the
    /// changes of every unit of work inside its transaction, right before
    /// commit.
    fn change_observer<O: ChangeObserver + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderDbExt for ModuleBuilder {
    fn migrations(&mut self, set: MigrationSet) -> &mut Self {
        self.contribute(set)
    }

    fn seeder<S: Seeder + Injectable>(&mut self) -> &mut Self {
        self.multi(
            ProviderFactory::<dyn Seeder>::new(|r: Resolver| async move {
                Ok(Arc::new(S::construct(r).await?) as Arc<dyn Seeder>)
            })
            .deps(S::dependencies()),
        )
    }

    fn change_observer<O: ChangeObserver + Injectable>(&mut self) -> &mut Self {
        self.multi(
            ProviderFactory::<dyn ChangeObserver>::new(|r: Resolver| async move {
                Ok(Arc::new(O::construct(r).await?) as Arc<dyn ChangeObserver>)
            })
            .deps(O::dependencies()),
        )
    }
}

fn print_list(verb: &str, items: &[String], empty: &str) {
    if items.is_empty() {
        println!("{empty}");
    }
    for item in items {
        println!("{verb}: {item}");
    }
}

async fn seed_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let ran = run_seeders(&ctx, &args).await?;
    let ran: Vec<String> = ran.into_iter().map(str::to_owned).collect();
    print_list("Seeded", &ran, "No seeders are registered.");
    Ok(())
}
