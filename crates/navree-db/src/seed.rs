//! [`Seeder`]s: filling the database (`db:seed`). Test records come from navree-orm factories.

use std::any::TypeId;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use navree_kernel::{AppContext, BoxError, BoxFuture, Injectable};

use crate::Db;

/// Fills the database with data (`db:seed`, `migrate:fresh --seed`).
/// Registered with [`ModuleBuilderDbExt::seeder`](crate::ModuleBuilderDbExt::seeder);
/// seeders run in module dependency order, then registration order.
///
/// A seeder can run others ([`SeederCalls`]: `ctx.call::<UsersSeeder>()`),
/// run without model events ([`without_model_events`](Seeder::without_model_events)),
/// and be a root seeder ([`is_root`](Seeder::is_root)) that `db:seed` runs
/// alone, calling the rest.
///
/// ```no_run
/// use navree_db::{Db, Seeder, SeederCalls};
/// use navree_kernel::{AppContext, BoxError, BoxFuture};
///
/// struct DatabaseSeeder;
///
/// impl Seeder for DatabaseSeeder {
///     fn name(&self) -> &'static str {
///         "database"
///     }
///
///     fn is_root(&self) -> bool {
///         true
///     }
///
///     fn run<'a>(&'a self, ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
///         Box::pin(async move {
///             ctx.call_seeder(&UsersSeeder { count: 10 }).await?;
///             let _ = db;
///             Ok(())
///         })
///     }
/// }
///
/// struct UsersSeeder {
///     count: usize,
/// }
///
/// impl Seeder for UsersSeeder {
///     fn name(&self) -> &'static str {
///         "users"
///     }
///
///     fn without_model_events(&self) -> bool {
///         true
///     }
///
///     fn run<'a>(&'a self, _ctx: &'a AppContext, _db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
///         Box::pin(async move {
///             // e.g. User::factory().count(self.count).create(db).await?;
///             let _ = self.count;
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait Seeder: Send + Sync + 'static {
    /// The name `db:seed <name>` selects.
    fn name(&self) -> &'static str;

    /// Seeds the database.
    fn run<'a>(&'a self, ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>>;

    /// Whether the seeder runs without model events: while it runs, the
    /// ORM skips the app's model hooks (observers, listeners); generated
    /// keys, timestamps, workspaces and the audit log's change events stay.
    /// `false` by default.
    fn without_model_events(&self) -> bool {
        false
    }

    /// Whether the seeder is a root: when any registered seeder is,
    /// `db:seed` without names runs the roots only (which call the others
    /// with [`SeederCalls`]); without roots it runs every seeder. `false` by
    /// default.
    fn is_root(&self) -> bool {
        false
    }

    /// The seeder's type, to find a registered instance. Not API.
    #[doc(hidden)]
    fn seeder_type(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

/// What one `db:seed` (or `migrate --seed`, or a test's seeding) is doing:
/// the database seeders write to, and the seeders that ran.
struct SeedRun {
    db: Arc<Db>,
    ran: Mutex<Vec<TypeId>>,
}

tokio::task_local! {
    /// The seeding run of the task.
    static RUN: Arc<SeedRun>;
    /// Set while a seeder that asked for it runs without model events.
    static WITHOUT_MODEL_EVENTS: ();
}

/// Whether the running task is inside a seeder that runs without model
/// events ([`Seeder::without_model_events`]). The ORM skips the app's model
/// hooks while it is.
pub fn seeding_without_model_events() -> bool {
    WITHOUT_MODEL_EVENTS.try_with(|_| ()).is_ok()
}

/// Which seeders to run, and where.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SeedOptions {
    /// The seeders to run by name; empty: the root seeders, or every seeder
    /// when there is no root.
    pub names: Vec<String>,
    /// The named database connection to seed (a named `Db` provider); the
    /// default database when `None`.
    pub database: Option<String>,
}

impl SeedOptions {
    /// Every seeder (or the roots) on the default database.
    pub fn new() -> Self {
        SeedOptions::default()
    }

    /// Only the seeders named `names`.
    pub fn names(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.names = names.into_iter().map(Into::into).collect();
        self
    }

    /// On the database connection `name`.
    pub fn database(mut self, name: impl Into<String>) -> Self {
        self.database = Some(name.into());
        self
    }
}

/// Runs the registered seeders (all of them, or the roots, when `names` is
/// empty, else the named ones), in order; returns the names that ran.
pub async fn run_seeders(
    ctx: &AppContext,
    names: &[String],
) -> Result<Vec<&'static str>, BoxError> {
    run_seeders_with(ctx, &SeedOptions::new().names(names.iter().cloned())).await
}

/// [`run_seeders`] with [`SeedOptions`]: the names, and the database
/// connection to seed.
pub async fn run_seeders_with(
    ctx: &AppContext,
    options: &SeedOptions,
) -> Result<Vec<&'static str>, BoxError> {
    let seeders = ctx.get_all::<dyn Seeder>().await?;
    if let Some(unknown) = options
        .names
        .iter()
        .find(|name| !seeders.iter().any(|s| s.name() == name.as_str()))
    {
        let known: Vec<&str> = seeders.iter().map(|s| s.name()).collect();
        return Err(format!(
            "unknown seeder `{unknown}`; registered seeders: {}",
            if known.is_empty() {
                "none".to_owned()
            } else {
                known.join(", ")
            }
        )
        .into());
    }
    let db = match &options.database {
        Some(name) => ctx
            .get_named::<Db>(name)
            .await
            .map_err(|error| format!("no database connection named `{name}` to seed: {error}"))?,
        None => ctx.get::<Db>().await?,
    };
    let roots = seeders.iter().any(|s| s.is_root());
    let selected: Vec<&Arc<dyn Seeder>> = seeders
        .iter()
        .filter(|s| {
            if options.names.is_empty() {
                !roots || s.is_root()
            } else {
                options.names.iter().any(|n| n == s.name())
            }
        })
        .collect();
    let run = Arc::new(SeedRun {
        db,
        ran: Mutex::new(Vec::new()),
    });
    RUN.scope(run.clone(), async {
        let mut ran = Vec::new();
        for seeder in selected {
            run_one(ctx, &run, seeder.as_ref(), false).await?;
            ran.push(seeder.name());
        }
        Ok(ran)
    })
    .await
}

/// Runs `seeder` in `run`: logs it (unless `silent`), without model events
/// when it asks, and records that it ran.
async fn run_one(
    ctx: &AppContext,
    run: &SeedRun,
    seeder: &dyn Seeder,
    silent: bool,
) -> Result<(), BoxError> {
    if silent {
        tracing::debug!(target: "navree_db::seed", seeder = seeder.name(), "seeding");
    } else {
        tracing::info!(target: "navree_db::seed", seeder = seeder.name(), "seeding");
    }
    let result = if seeder.without_model_events() {
        WITHOUT_MODEL_EVENTS
            .scope((), seeder.run(ctx, &run.db))
            .await
    } else {
        seeder.run(ctx, &run.db).await
    };
    result.map_err(|error| format!("seeder `{}` failed: {error}", seeder.name()))?;
    run.ran
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(seeder.seeder_type());
    Ok(())
}

/// The seeding run of the task, or a new one on the app's database (a
/// seeder called outside `db:seed`, e.g. from a test).
async fn current_run(ctx: &AppContext) -> Result<Arc<SeedRun>, BoxError> {
    if let Ok(run) = RUN.try_with(Arc::clone) {
        return Ok(run);
    }
    Ok(Arc::new(SeedRun {
        db: ctx.get::<Db>().await?,
        ran: Mutex::new(Vec::new()),
    }))
}

/// The registered seeder of type `S`, or one built for the call.
async fn instance<S: Seeder + Injectable>(ctx: &AppContext) -> Result<Arc<dyn Seeder>, BoxError> {
    let seeders = ctx.get_all::<dyn Seeder>().await?;
    if let Some(seeder) = seeders
        .into_iter()
        .find(|seeder| seeder.seeder_type() == TypeId::of::<S>())
    {
        return Ok(seeder);
    }
    let root = ctx
        .modules()
        .last()
        .map(|module| module.id)
        .ok_or("the app has no modules to build a seeder in")?;
    Ok(Arc::new(S::construct(ctx.resolver_for(root)).await?))
}

/// Runs a seeder from another one (or from a test): in the same seeding
/// run, on the same database. Seeders that are not registered are built
/// with their [`Injectable`] constructor.
pub trait SeederCalls {
    /// Runs seeder `S`.
    fn call<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>>;

    /// Runs seeder `S` unless it already ran in this seeding run.
    fn call_once<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>>;

    /// Runs seeder `S` without logging it.
    fn call_silent<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>>;

    /// Runs `seeder`, a value (its fields are its parameters:
    /// `ctx.call_seeder(&UsersSeeder { count: 10 })`).
    fn call_seeder<'a, S: Seeder>(&'a self, seeder: &'a S) -> BoxFuture<'a, Result<(), BoxError>>;
}

impl SeederCalls for AppContext {
    fn call<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let run = current_run(self).await?;
            let seeder = instance::<S>(self).await?;
            in_run(&run, run_one(self, &run, seeder.as_ref(), false)).await
        })
    }

    fn call_once<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let run = current_run(self).await?;
            let done = run
                .ran
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(&TypeId::of::<S>());
            if done {
                return Ok(());
            }
            let seeder = instance::<S>(self).await?;
            in_run(&run, run_one(self, &run, seeder.as_ref(), false)).await
        })
    }

    fn call_silent<S: Seeder + Injectable>(&self) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let run = current_run(self).await?;
            let seeder = instance::<S>(self).await?;
            in_run(&run, run_one(self, &run, seeder.as_ref(), true)).await
        })
    }

    fn call_seeder<'a, S: Seeder>(&'a self, seeder: &'a S) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let run = current_run(self).await?;
            in_run(&run, run_one(self, &run, seeder, false)).await
        })
    }
}

/// Runs `future` inside `run` (so seeders it calls share the run).
async fn in_run<F: Future>(run: &Arc<SeedRun>, future: F) -> F::Output {
    if RUN.try_with(|_| ()).is_ok() {
        future.await
    } else {
        RUN.scope(run.clone(), future).await
    }
}
