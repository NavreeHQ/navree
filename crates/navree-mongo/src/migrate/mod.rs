//! MongoDB migrations: [`MongoMigration`] (what a migration file
//! implements), [`MongoMigrationSet`] (a module's migrations) and
//! [`MongoMigrator`] (runs them, recording each in a collection).

pub(crate) mod commands;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bson::{Bson, Document, doc};
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use navree_kernel::{AppContext, BoxFuture};
use serde::Serialize;

use crate::connection::MongoDb;
use crate::error::{MongoError, MongoResult};
use crate::schema::MongoSchema;

/// A MongoDB migration: what [`up`](MongoMigration::up) does to the
/// database, [`down`](MongoMigration::down) undoes. Write the methods as
/// `async fn`:
///
/// ```
/// use navree_mongo::{MongoMigration, MongoResult, MongoSchema};
///
/// pub struct Migration;
///
/// impl MongoMigration for Migration {
///     async fn up(&self, schema: &MongoSchema) -> MongoResult {
///         schema
///             .create("deals", |t| {
///                 t.object_id_primary();
///                 t.string("title");
///                 t.timestamps_tz();
///             })
///             .await
///     }
///
///     async fn down(&self, schema: &MongoSchema) -> MongoResult {
///         schema.drop_if_exists("deals").await
///     }
/// }
/// ```
///
/// Register it in a [`MongoMigrationSet`] with
/// [`mongo_migration!`](crate::mongo_migration!).
pub trait MongoMigration: Send + Sync + 'static {
    /// Applies the migration.
    fn up(&self, schema: &MongoSchema) -> impl Future<Output = MongoResult> + Send;

    /// Reverts the migration.
    fn down(&self, schema: &MongoSchema) -> impl Future<Output = MongoResult> + Send;

    /// Whether it runs in a transaction with its record in the migrations
    /// collection (`false`, the default). MongoDB refuses most collection
    /// changes in a transaction (`drop`, `collMod`, `renameCollection`,
    /// index drops, time-series collections), so a schema migration runs
    /// without one; return `true` for a data migration, whose writes
    /// through [`MongoSchema::uow`] then commit with its record. A
    /// standalone server (no transactions) runs it without one, with a
    /// notice.
    fn within_transaction(&self) -> bool {
        false
    }
}

/// A migration with its name (what `mongo_migration!` builds).
///
/// ```
/// use navree_mongo::{
///     MongoMigration, MongoMigrationSet, MongoResult, MongoSchema, NamedMongoMigration,
/// };
///
/// struct CreateEvents;
///
/// impl MongoMigration for CreateEvents {
///     async fn up(&self, schema: &MongoSchema) -> MongoResult {
///         schema
///             .create("events", |t| {
///                 t.object_id_primary();
///                 t.string("kind");
///             })
///             .await
///     }
///
///     async fn down(&self, schema: &MongoSchema) -> MongoResult {
///         schema.drop_if_exists("events").await
///     }
/// }
///
/// let set = MongoMigrationSet::new("events").migration(NamedMongoMigration::new(
///     "m20261027_000001_create_events",
///     CreateEvents,
/// ));
/// assert_eq!(set.names(), ["m20261027_000001_create_events"]);
/// ```
pub struct NamedMongoMigration<M> {
    name: String,
    migration: M,
}

impl<M: MongoMigration> NamedMongoMigration<M> {
    /// `migration`, recorded under `name`.
    pub fn new(name: impl Into<String>, migration: M) -> NamedMongoMigration<M> {
        NamedMongoMigration {
            name: name.into(),
            migration,
        }
    }

    /// The name recorded in the migrations collection.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A migration file's migration, named after the file:
/// `mongo_migration!(m20261027_120000_create_events)` is
/// `NamedMongoMigration::new("m20261027_120000_create_events",
/// m20261027_120000_create_events::Migration)`. Name the type when it is
/// not `Migration`: `mongo_migration!(m20261027_create_events::CreateEvents)`.
#[macro_export]
macro_rules! mongo_migration {
    ($file:ident) => {
        $crate::NamedMongoMigration::new(::core::stringify!($file), $file::Migration)
    };
    ($file:ident :: $migration:ident) => {
        $crate::NamedMongoMigration::new(::core::stringify!($file), $file::$migration)
    };
}

/// A migration without its type: what a set holds.
trait Erased: Send + Sync {
    fn name(&self) -> &str;
    fn up<'a>(&'a self, schema: &'a MongoSchema) -> BoxFuture<'a, MongoResult>;
    fn down<'a>(&'a self, schema: &'a MongoSchema) -> BoxFuture<'a, MongoResult>;
    fn within_transaction(&self) -> bool;
}

impl<M: MongoMigration> Erased for NamedMongoMigration<M> {
    fn name(&self) -> &str {
        &self.name
    }

    fn up<'a>(&'a self, schema: &'a MongoSchema) -> BoxFuture<'a, MongoResult> {
        Box::pin(self.migration.up(schema))
    }

    fn down<'a>(&'a self, schema: &'a MongoSchema) -> BoxFuture<'a, MongoResult> {
        Box::pin(self.migration.down(schema))
    }

    fn within_transaction(&self) -> bool {
        self.migration.within_transaction()
    }
}

/// The MongoDB migrations a module owns, contributed with
/// [`ModuleBuilderMongoExt::mongo_migrations`](crate::ModuleBuilderMongoExt::mongo_migrations).
///
/// Sets run in dependency order (a set after every set named in
/// [`after`](Self::after); otherwise by module name), migrations by name.
///
/// ```
/// use navree_mongo::MongoMigrationSet;
///
/// let set = MongoMigrationSet::new("events").after(["users"]);
/// assert_eq!(set.module, "events");
/// assert!(set.is_empty());
/// ```
pub struct MongoMigrationSet {
    /// The module name recorded with each migration and used in `after`
    /// lists.
    pub module: &'static str,
    /// Modules whose migrations must run first.
    pub after: Vec<&'static str>,
    migrations: Vec<Box<dyn Erased>>,
}

impl MongoMigrationSet {
    /// An empty set for `module`.
    pub fn new(module: &'static str) -> MongoMigrationSet {
        MongoMigrationSet {
            module,
            after: Vec::new(),
            migrations: Vec::new(),
        }
    }

    /// Runs after the migrations of `modules`.
    pub fn after(mut self, modules: impl IntoIterator<Item = &'static str>) -> Self {
        self.after.extend(modules);
        self
    }

    /// Adds a migration.
    pub fn migration<M: MongoMigration>(mut self, migration: NamedMongoMigration<M>) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }

    /// The names of the migrations, in run order.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.migrations.iter().map(|m| m.name()).collect();
        names.sort_unstable();
        names
    }

    /// Whether the set has no migration.
    pub fn is_empty(&self) -> bool {
        self.migrations.is_empty()
    }
}

impl std::fmt::Debug for MongoMigrationSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoMigrationSet")
            .field("module", &self.module)
            .field("after", &self.after)
            .field("migrations", &self.names())
            .finish()
    }
}

/// The migration sets the modules of `ctx` contributed.
pub(crate) fn sets_of(ctx: &AppContext) -> Vec<Arc<MongoMigrationSet>> {
    ctx.contributions::<MongoMigrationSet>()
        .into_iter()
        .map(|contributed| contributed.value)
        .collect()
}

/// How to migrate (`mongo:migrate`).
///
/// ```no_run
/// # async fn demo(migrator: navree_mongo::MongoMigrator) -> navree_mongo::MongoResult {
/// use navree_mongo::MongoMigrateOptions;
///
/// let planned = migrator
///     .up(&MongoMigrateOptions { pretend: true, ..MongoMigrateOptions::default() })
///     .await?;
/// for run in &planned.migrations {
///     println!("{}:\n  {}", run.label(), run.statements.join("\n  "));
/// }
/// # Ok(()) }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MongoMigrateOptions {
    /// Record the commands instead of running them.
    pub pretend: bool,
    /// One batch per migration, so each rolls back on its own.
    pub step: bool,
}

/// What to roll back (`mongo:migrate:rollback`).
///
/// ```no_run
/// # async fn demo(migrator: navree_mongo::MongoMigrator) -> navree_mongo::MongoResult {
/// use navree_mongo::MongoRollbackOptions;
///
/// let undone = migrator
///     .rollback(&MongoRollbackOptions { step: Some(2), ..MongoRollbackOptions::default() })
///     .await?;
/// println!("rolled back {}", undone.labels().join(", "));
/// # Ok(()) }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MongoRollbackOptions {
    /// The last `step` migrations, across batches.
    pub step: Option<u32>,
    /// Batch `batch`.
    pub batch: Option<i64>,
    /// Record the commands instead of running them.
    pub pretend: bool,
}

/// One migration that ran (or would run).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MongoMigrationRun {
    /// The module.
    pub module: String,
    /// The migration.
    pub name: String,
    /// The commands it ran (pretending: would run).
    pub statements: Vec<String>,
    /// What MongoDB did differently from an SQL database.
    pub notices: Vec<String>,
    /// How long it took.
    pub duration: Duration,
}

impl MongoMigrationRun {
    /// `module/name`.
    pub fn label(&self) -> String {
        format!("{}/{}", self.module, self.name)
    }
}

/// What a migrator run did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MongoMigrationReport {
    /// The migrations, in the order they ran.
    pub migrations: Vec<MongoMigrationRun>,
}

impl MongoMigrationReport {
    /// `module/name` of each migration.
    pub fn labels(&self) -> Vec<String> {
        self.migrations
            .iter()
            .map(MongoMigrationRun::label)
            .collect()
    }
}

/// The state of one migration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MongoMigrationStatus {
    /// The module.
    pub module: String,
    /// The migration.
    pub name: String,
    /// Whether it has been applied.
    pub applied: bool,
    /// The batch it was applied in.
    pub batch: Option<i64>,
    /// When it was applied.
    pub applied_at: Option<DateTime<Utc>>,
}

/// One migration in run order.
struct Step<'a> {
    module: &'static str,
    migration: &'a dyn Erased,
}

impl Step<'_> {
    fn label(&self) -> String {
        format!("{}/{}", self.module, self.migration.name())
    }
}

/// A recorded migration.
#[derive(Clone, Debug)]
struct Applied {
    batch: i64,
    applied_at: Option<DateTime<Utc>>,
}

/// Runs the MongoDB migrations of an app, recording each in the
/// migrations collection (`database.mongodb.migrations`,
/// `navree_migrations` by default) as `{_id: "module/name", module, name,
/// batch, applied_at}`. Each [`up`](Self::up) that applies anything is one
/// batch (one per migration with `step`); [`rollback`](Self::rollback)
/// reverts the last.
///
/// ```no_run
/// # async fn deploy(ctx: &navree_kernel::AppContext) -> Result<(), navree_mongo::MongoError> {
/// use navree_mongo::{MongoMigrateOptions, MongoMigrator};
///
/// let report = MongoMigrator::of(ctx).await?.up(&MongoMigrateOptions::default()).await?;
/// for run in &report.migrations {
///     println!("migrated {}", run.label());
/// }
/// # Ok(()) }
/// ```
pub struct MongoMigrator {
    db: MongoDb,
    sets: Vec<Arc<MongoMigrationSet>>,
}

impl MongoMigrator {
    /// A migrator of `sets` on `db`.
    pub fn new(db: &MongoDb, sets: Vec<Arc<MongoMigrationSet>>) -> MongoMigrator {
        MongoMigrator {
            db: db.clone(),
            sets,
        }
    }

    /// The migrator of the app: its [`MongoDb`] and every set its modules
    /// contributed.
    pub async fn of(ctx: &AppContext) -> MongoResult<MongoMigrator> {
        let db = ctx.get::<MongoDb>().await?;
        Ok(MongoMigrator::new(&db, sets_of(ctx)))
    }

    /// Every migration in run order: sets by `after` (then module name),
    /// migrations by name.
    fn plan(&self) -> MongoResult<Vec<Step<'_>>> {
        let mut by_module: BTreeMap<&'static str, &MongoMigrationSet> = BTreeMap::new();
        for set in &self.sets {
            if by_module.insert(set.module, set).is_some() {
                return Err(MongoError::Migrations(format!(
                    "two MongoDB migration sets are named `{}`; module names must be unique",
                    set.module
                )));
            }
            let mut names = HashSet::new();
            for migration in &set.migrations {
                if !names.insert(migration.name()) {
                    return Err(MongoError::Migrations(format!(
                        "MongoDB migration `{}/{}` is registered twice",
                        set.module,
                        migration.name()
                    )));
                }
            }
        }
        let mut waiting: BTreeMap<&str, BTreeSet<&str>> = by_module
            .iter()
            .map(|(module, set)| {
                let deps = set
                    .after
                    .iter()
                    .copied()
                    .filter(|dep| by_module.contains_key(dep) && dep != module)
                    .collect();
                (*module, deps)
            })
            .collect();
        let mut order = Vec::with_capacity(by_module.len());
        while let Some(next) = waiting
            .iter()
            .find(|(_, deps)| deps.is_empty())
            .map(|(module, _)| *module)
        {
            waiting.remove(next);
            for deps in waiting.values_mut() {
                deps.remove(next);
            }
            order.push(next);
        }
        if !waiting.is_empty() {
            return Err(MongoError::Migrations(format!(
                "MongoDB migration order cycle between modules {}",
                waiting
                    .keys()
                    .map(|module| format!("`{module}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let mut steps = Vec::new();
        for module in order {
            let set = by_module[module];
            let mut migrations: Vec<&dyn Erased> =
                set.migrations.iter().map(|m| m.as_ref()).collect();
            migrations.sort_by(|a, b| a.name().cmp(b.name()));
            steps.extend(migrations.into_iter().map(|migration| Step {
                module: set.module,
                migration,
            }));
        }
        Ok(steps)
    }

    fn records(&self) -> mongodb::Collection<Document> {
        self.db.collection(&self.db.options().migrations)
    }

    /// The recorded migrations.
    async fn applied(&self) -> MongoResult<HashMap<String, Applied>> {
        let documents: Vec<Document> = self.records().find(doc! {}).await?.try_collect().await?;
        let mut applied = HashMap::with_capacity(documents.len());
        for document in documents {
            let Ok(id) = document.get_str("_id") else {
                continue;
            };
            let batch = match document.get("batch") {
                Some(Bson::Int64(batch)) => *batch,
                Some(Bson::Int32(batch)) => i64::from(*batch),
                _ => 0,
            };
            let applied_at = document
                .get_datetime("applied_at")
                .ok()
                .map(|at| at.to_chrono());
            applied.insert(id.to_owned(), Applied { batch, applied_at });
        }
        Ok(applied)
    }

    /// Applies every pending migration (one new batch, or one batch each
    /// with `step`).
    pub async fn up(&self, options: &MongoMigrateOptions) -> MongoResult<MongoMigrationReport> {
        let steps = self.plan()?;
        let applied = self.applied().await?;
        let mut batch = applied.values().map(|a| a.batch).max().unwrap_or(0) + 1;
        let mut report = MongoMigrationReport::default();
        let transactions = self.db.supports_transactions().await?;
        for step in steps
            .iter()
            .filter(|step| !applied.contains_key(&step.label()))
        {
            let run = self
                .run(step, true, batch, options.pretend, transactions)
                .await
                .map_err(|error| error.in_migration(step.label()))?;
            report.migrations.push(run);
            if options.step {
                batch += 1;
            }
        }
        Ok(report)
    }

    /// Reverts the last batch, the last `step` migrations or batch `batch`,
    /// newest first.
    pub async fn rollback(
        &self,
        options: &MongoRollbackOptions,
    ) -> MongoResult<MongoMigrationReport> {
        let steps = self.plan()?;
        let applied = self.applied().await?;
        let known: HashSet<String> = steps.iter().map(Step::label).collect();
        if let Some(unknown) = applied.keys().find(|label| !known.contains(*label)) {
            return Err(MongoError::Migrations(format!(
                "MongoDB migration `{unknown}` is recorded as applied but no module registers it"
            )));
        }
        // Applied migrations, newest first: by batch, then run order.
        let mut done: Vec<(&Step<'_>, i64)> = steps
            .iter()
            .filter_map(|step| applied.get(&step.label()).map(|a| (step, a.batch)))
            .collect();
        done.reverse();
        done.sort_by_key(|(_, batch)| std::cmp::Reverse(*batch));
        let chosen: Vec<&Step<'_>> = match (options.step, options.batch) {
            (Some(step), _) => done
                .iter()
                .take(usize::try_from(step).unwrap_or(usize::MAX))
                .map(|(step, _)| *step)
                .collect(),
            (None, Some(batch)) => done
                .iter()
                .filter(|(_, b)| *b == batch)
                .map(|(step, _)| *step)
                .collect(),
            (None, None) => {
                let last = done.first().map(|(_, batch)| *batch);
                done.iter()
                    .filter(|(_, b)| Some(*b) == last)
                    .map(|(step, _)| *step)
                    .collect()
            }
        };
        self.revert(chosen, options.pretend).await
    }

    /// Reverts every migration, newest first.
    pub async fn reset(&self, pretend: bool) -> MongoResult<MongoMigrationReport> {
        let steps = self.plan()?;
        let applied = self.applied().await?;
        let mut done: Vec<(&Step<'_>, i64)> = steps
            .iter()
            .filter_map(|step| applied.get(&step.label()).map(|a| (step, a.batch)))
            .collect();
        done.reverse();
        done.sort_by_key(|(_, batch)| std::cmp::Reverse(*batch));
        self.revert(done.into_iter().map(|(step, _)| step).collect(), pretend)
            .await
    }

    async fn revert(
        &self,
        steps: Vec<&Step<'_>>,
        pretend: bool,
    ) -> MongoResult<MongoMigrationReport> {
        let transactions = self.db.supports_transactions().await?;
        let mut report = MongoMigrationReport::default();
        for step in steps {
            let run = self
                .run(step, false, 0, pretend, transactions)
                .await
                .map_err(|error| error.in_migration(step.label()))?;
            report.migrations.push(run);
        }
        Ok(report)
    }

    /// Reverts every migration (or the last `step`), then migrates.
    pub async fn refresh(
        &self,
        step: Option<u32>,
    ) -> MongoResult<(MongoMigrationReport, MongoMigrationReport)> {
        let reverted = match step {
            Some(step) => {
                self.rollback(&MongoRollbackOptions {
                    step: Some(step),
                    ..MongoRollbackOptions::default()
                })
                .await?
            }
            None => self.reset(false).await?,
        };
        let migrated = self.up(&MongoMigrateOptions::default()).await?;
        Ok((reverted, migrated))
    }

    /// Drops every collection and view of the database, then applies every
    /// migration as batch 1; returns the dropped names and the report.
    pub async fn fresh(&self) -> MongoResult<(Vec<String>, MongoMigrationReport)> {
        let dropped = self.db.schema().drop_all().await?;
        let report = self.up(&MongoMigrateOptions::default()).await?;
        Ok((dropped, report))
    }

    /// Every registered migration in run order, with its state.
    pub async fn status(&self) -> MongoResult<Vec<MongoMigrationStatus>> {
        let steps = self.plan()?;
        let applied = self.applied().await?;
        Ok(steps
            .iter()
            .map(|step| {
                let record = applied.get(&step.label());
                MongoMigrationStatus {
                    module: step.module.to_owned(),
                    name: step.migration.name().to_owned(),
                    applied: record.is_some(),
                    batch: record.map(|a| a.batch),
                    applied_at: record.and_then(|a| a.applied_at),
                }
            })
            .collect())
    }

    /// Runs one migration up (recording it in `batch`) or down (removing
    /// its record).
    async fn run(
        &self,
        step: &Step<'_>,
        up: bool,
        batch: i64,
        pretend: bool,
        transactions: bool,
    ) -> MongoResult<MongoMigrationRun> {
        let started = Instant::now();
        let label = step.label();
        let mut schema = self.db.schema();
        if pretend {
            schema = schema.pretending();
        }
        let mut notices = Vec::new();
        let uow = if step.migration.within_transaction() && !pretend {
            if transactions {
                Some(self.db.begin().await?)
            } else {
                notices.push(format!(
                    "`{label}` asks for a transaction, but this MongoDB deployment is a standalone server without transactions: it ran without one"
                ));
                None
            }
        } else {
            None
        };
        if let Some(uow) = &uow {
            schema = schema.in_uow(uow.clone())?;
        }
        let result = if up {
            step.migration.up(&schema).await
        } else {
            step.migration.down(&schema).await
        };
        if let Err(error) = result {
            if let Some(uow) = &uow
                && let Err(rollback) = uow.rollback().await
            {
                tracing::warn!(target: "navree_mongo::migrate", error = %rollback, "rolling back a failed migration's transaction failed");
            }
            return Err(error);
        }
        if !pretend {
            let record = doc! {
                "_id": label.clone(),
                "module": step.module,
                "name": step.migration.name(),
                "batch": batch,
                "applied_at": bson::DateTime::now(),
            };
            let recorded = match &uow {
                // In the migration's transaction: commands in its session.
                Some(uow) => {
                    let collection = self.db.options().migrations.clone();
                    let command = if up {
                        doc! { "insert": collection, "documents": [record] }
                    } else {
                        doc! { "delete": collection, "deletes": [{ "q": { "_id": label.clone() }, "limit": 1 }] }
                    };
                    match crate::MongoExecutor::mongo_driver(uow)?
                        .run_command(command)
                        .await
                    {
                        Ok(reply) => match crate::schema::write_problem(&reply) {
                            Some(problem) => Err(MongoError::Migrations(format!(
                                "recording `{label}` in the migrations collection failed: {problem}"
                            ))),
                            None => Ok(()),
                        },
                        Err(error) => Err(error),
                    }
                }
                None if up => self
                    .records()
                    .insert_one(record)
                    .await
                    .map(drop)
                    .map_err(MongoError::from),
                None => self
                    .records()
                    .delete_one(doc! { "_id": label.clone() })
                    .await
                    .map(drop)
                    .map_err(MongoError::from),
            };
            if let Err(error) = recorded {
                if let Some(uow) = &uow
                    && let Err(rollback) = uow.rollback().await
                {
                    tracing::warn!(target: "navree_mongo::migrate", error = %rollback, "rolling back a migration whose record failed failed too");
                }
                return Err(error);
            }
            if let Some(uow) = &uow {
                uow.commit().await?;
            }
        }
        notices.extend(schema.notices());
        Ok(MongoMigrationRun {
            module: step.module.to_owned(),
            name: step.migration.name().to_owned(),
            statements: schema.statements(),
            notices,
            duration: started.elapsed(),
        })
    }
}

impl std::fmt::Debug for MongoMigrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoMigrator")
            .field("db", &self.db)
            .field("sets", &self.sets)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noop;

    impl MongoMigration for Noop {
        async fn up(&self, _schema: &MongoSchema) -> MongoResult {
            Ok(())
        }

        async fn down(&self, _schema: &MongoSchema) -> MongoResult {
            Ok(())
        }
    }

    async fn migrator(sets: Vec<MongoMigrationSet>) -> MongoMigrator {
        let db = MongoDb::connect(&crate::MongoOptions::new("mongodb://127.0.0.1:1/plan"))
            .await
            .unwrap();
        MongoMigrator::new(&db, sets.into_iter().map(Arc::new).collect())
    }

    #[tokio::test]
    async fn plans_follow_after_then_names() {
        let migrator = migrator(vec![
            MongoMigrationSet::new("deals")
                .after(["companies", "absent"])
                .migration(NamedMongoMigration::new("m2_b", Noop))
                .migration(NamedMongoMigration::new("m1_a", Noop)),
            MongoMigrationSet::new("companies").migration(NamedMongoMigration::new("m9", Noop)),
        ])
        .await;
        let labels: Vec<String> = migrator.plan().unwrap().iter().map(Step::label).collect();
        assert_eq!(labels, ["companies/m9", "deals/m1_a", "deals/m2_b"]);
    }

    #[tokio::test]
    async fn plans_refuse_duplicates_and_cycles() {
        let duplicate = migrator(vec![
            MongoMigrationSet::new("a").migration(NamedMongoMigration::new("m1", Noop)),
            MongoMigrationSet::new("a"),
        ])
        .await;
        assert!(
            duplicate
                .plan()
                .err()
                .unwrap()
                .to_string()
                .contains("named `a`")
        );
        let twice = migrator(vec![
            MongoMigrationSet::new("a")
                .migration(NamedMongoMigration::new("m1", Noop))
                .migration(NamedMongoMigration::new("m1", Noop)),
        ])
        .await;
        assert!(
            twice
                .plan()
                .err()
                .unwrap()
                .to_string()
                .contains("registered twice")
        );
        let cycle = migrator(vec![
            MongoMigrationSet::new("a").after(["b"]),
            MongoMigrationSet::new("b").after(["a"]),
        ])
        .await;
        assert!(cycle.plan().err().unwrap().to_string().contains("cycle"));
    }
}
