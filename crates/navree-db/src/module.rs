//! [`DatabaseModule`], [`ModuleBuilderDbExt`] and the database commands.

use std::sync::Arc;

use navree_config::Config;
use navree_contracts::{HealthCheck, HealthStatus};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Factory as ProviderFactory, Injectable,
    Lifecycle, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver, Scope,
};
use navree_telemetry::Telemetry;
use sea_orm::DbErr;

use crate::events::publish;
use crate::http::{map_db_error, uow_layer};
use crate::migrate::{self, MigrationSet, commands};
use crate::seed::{SeedOptions, run_seeders_with};
use crate::{
    ChangeObserver, ClientCommand, ConnectionEstablished, DEFAULT_CONNECTION, Db, DbConnections,
    DbOptions, Seeder,
};

/// The database module. Global; provides and exports [`Db`],
/// [`DbOptions`] and [`DbConnections`] (the named connections of
/// `[database.connections.<name>]`), and each connection as a named `Db`
/// (`ctx.get_named::<Db>("default")`; the named ones too when
/// [`for_root`](DatabaseModule::for_root) gives them, since configured ones
/// are known only at boot); contributes the unit-of-work layer
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
///         // TimescaleDB, as `navree dock up` runs it (the `postgres` feature).
///         url: "postgres://navree:navree@127.0.0.1:5432/my_app".into(),
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
    // Each connection is also a named `Db` provider: `default` always, the
    // named ones when the options are given in code (configured ones are
    // known only at boot; `DbConnections` reaches them). Transient, so a
    // named connection still opens on first use, not at boot; every
    // resolution hands out the same pool.
    let mut names = vec![DEFAULT_CONNECTION.to_owned()];
    if let Some(options) = &options {
        names.extend(options.connections.keys().cloned());
    }
    for name in names {
        let key = ProviderKey::named::<Db>(name.clone());
        let lookup = name.clone();
        m.factory(
            ProviderFactory::<Db>::new(move |r: Resolver| {
                let name = lookup.clone();
                async move {
                    let connections = r.get::<DbConnections>().await?;
                    Ok(connections.get(&name).await?)
                }
            })
            .named(name)
            .scope(Scope::Transient)
            .deps(vec![ProviderKey::of::<DbConnections>()]),
        )
        .export_key(key);
    }
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
                    .with_app(&r.app());
                if options.migrate_on_boot {
                    let direct = db.direct().await?;
                    let url = options.direct_url.as_deref().unwrap_or(&options.url);
                    migrate::migrate_on_boot(&direct, &r.app(), url).await?;
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
            "Show which migrations have run ([--pending] [--json])",
            commands::status,
        ))
        .command(CommandSpec::new(
            "schema:dump",
            "Write the schema and the migrations table to database/schema/<driver>-schema.sql ([--path <path>])",
            commands::dump,
        ))
        .command(CommandSpec::new(
            "db:seed",
            "Run the seeders: the root seeders, or every seeder ([name...] to run only some) ([--database <name>] [--force])",
            seed_command,
        ))
        .command(CommandSpec::new(
            "db:monitor",
            "Show the connections each database has open; publish db.busy above the limit ([--databases a,b] [--max <n>])",
            monitor_command,
        ))
        .command(CommandSpec::new(
            "db",
            "Open the database's own client: psql, mysql, mariadb, sqlite3 or mongosh ([connection] [--read | --write])",
            client_command,
        ));
}

/// Announces the default connection once the app is up, and closes its
/// pool when the app shuts down (SQLite then checkpoints its WAL).
struct PoolCloser(Arc<Db>);

impl Lifecycle for PoolCloser {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let established = ConnectionEstablished {
                connection: self.0.name().to_owned(),
            };
            publish(&ctx, ConnectionEstablished::NAME, &established).await;
            Ok(())
        })
    }

    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.close().await?;
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

/// `db:seed [name...] [--database <name>] [--force]`.
async fn seed_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let mut options = SeedOptions::new();
    let mut force = false;
    let mut names = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" => force = true,
            "--database" => {
                let name = args
                    .next()
                    .ok_or("`--database` needs the name of a database connection")?;
                options = options.database(name);
            }
            _ if arg.starts_with("--database=") => {
                options = options.database(&arg["--database=".len()..]);
            }
            _ if arg.starts_with("--") => {
                return Err(format!(
                    "unknown option `{arg}` for db:seed; it takes seeder names, --database <name>, --force"
                )
                .into());
            }
            _ => names.push(arg),
        }
    }
    if !force
        && let Some(env) = crate::migrate::commands::environment(&ctx).await
        && env.eq_ignore_ascii_case("production")
    {
        return Err(format!(
            "db:seed would write to the database and this app runs in production (`env = \"{env}\"`); pass --force to run it anyway"
        )
        .into());
    }
    let ran = run_seeders_with(&ctx, &options.names(names)).await?;
    let ran: Vec<String> = ran.into_iter().map(str::to_owned).collect();
    print_list("Seeded", &ran, "No seeders are registered.");
    Ok(())
}

/// `db:monitor [--databases a,b] [--max <n>]`.
async fn monitor_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let mut databases = Vec::new();
    let mut max = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--databases" => {
                let list = args
                    .next()
                    .ok_or("--databases takes a list: --databases a,b")?;
                databases.extend(
                    list.split(',')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_owned),
                );
            }
            "--max" => {
                let value = args.next().ok_or("--max takes a number")?;
                max = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| format!("--max takes a number, not `{value}`"))?,
                );
            }
            other => return Err(format!("unknown argument `{other}`").into()),
        }
    }
    let connections = ctx.get::<DbConnections>().await?;
    for status in connections.monitor(&databases, max).await? {
        let open = status
            .open
            .map_or_else(|| "n/a (no server)".to_owned(), |open| open.to_string());
        let busy = if status.busy { "  busy" } else { "" };
        println!("{}: {open} open{busy}", status.connection);
    }
    Ok(())
}

/// `db [connection] [--read | --write]`: the connection's own client with
/// its credentials, attached to this terminal.
async fn client_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let mut connection = DEFAULT_CONNECTION.to_owned();
    let mut read = false;
    for arg in args {
        match arg.as_str() {
            "--read" => read = true,
            "--write" => read = false,
            other if !other.starts_with('-') => connection = other.to_owned(),
            other => return Err(format!("unknown argument `{other}`").into()),
        }
    }
    let options = ctx.get::<DbOptions>().await?;
    let options = if connection == DEFAULT_CONNECTION {
        DbOptions::clone(&options)
    } else {
        options
            .connections
            .get(&connection)
            .cloned()
            .ok_or_else(|| format!("no database connection named `{connection}`"))?
    };
    let url = match options.read_urls.first() {
        Some(replica) if read => replica,
        _ => &options.url,
    };
    let client = ClientCommand::for_url(url)?;
    let status = tokio::process::Command::new(&client.program)
        .args(&client.args)
        .envs(client.env.iter().map(|(name, value)| (name, value)))
        .status()
        .await
        .map_err(|error| {
            format!(
                "could not start `{}` ({error}); is it installed and on PATH?",
                client.program
            )
        })?;
    if !status.success() {
        return Err(format!("`{}` exited with {status}", client.program).into());
    }
    Ok(())
}
