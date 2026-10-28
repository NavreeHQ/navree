//! Migrations written with the schema builder: the [`Migration`] trait,
//! [`SchemaMigration`] (a migration with its name) and the object-safe
//! [`NamedMigration`] a module's migration set holds, the
//! [`MigrationContext`] the migrator provides, and the [`migration!`]
//! macro.
//!
//! [`migration!`]: crate::migration!

use std::future::Future;
use std::sync::{Arc, Mutex};

use crate::connection::{BoxFuture, SchemaConnection};
use crate::error::SchemaResult;
use crate::options::SchemaOptions;
use crate::schema::Schema;
use crate::target::Dialect;

/// A migration: what [`up`](Migration::up) does to the schema,
/// [`down`](Migration::down) undoes.
///
/// Write the methods as `async fn`:
///
/// ```
/// use navree_schema::{Schema, SchemaResult};
///
/// pub struct Migration;
///
/// impl navree_schema::Migration for Migration {
///     async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
///         schema
///             .create("companies", |t| {
///                 t.uuid_primary();
///                 t.string("name");
///                 t.timestamps_tz();
///             })
///             .await
///     }
///
///     async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
///         schema.drop_if_exists("companies").await
///     }
/// }
/// ```
///
/// Register it in the module's migration set with
/// [`migration!`](crate::migration!) (the name recorded in
/// `navree_migrations` is the file's name), or with
/// [`SchemaMigration::new`].
pub trait Migration: Send + Sync + 'static {
    /// Applies the migration.
    fn up(&self, schema: &Schema<'_>) -> impl Future<Output = SchemaResult> + Send;

    /// Reverts the migration.
    fn down(&self, schema: &Schema<'_>) -> impl Future<Output = SchemaResult> + Send;

    /// Whether the migrator runs it in a transaction, with its
    /// `navree_migrations` row, where the database's DDL is transactional
    /// (PostgreSQL and SQLite): `true` by default, so a migration that fails
    /// leaves neither its changes nor its row. Return `false` for
    /// statements PostgreSQL refuses in a transaction, such as `CREATE
    /// INDEX CONCURRENTLY` (an index's `.online()`) or refreshing a
    /// continuous aggregate.
    fn within_transaction(&self) -> bool {
        true
    }

    /// Whether the migrator runs it in a transaction on MySQL and MariaDB
    /// too (`false` by default). Those databases commit every DDL
    /// statement on the spot, so a transaction only helps a data
    /// migration: its `INSERT`s and `UPDATE`s then roll back with its
    /// `navree_migrations` row when it fails. Ignored when
    /// [`within_transaction`](Migration::within_transaction) is `false`.
    fn within_transaction_on_mysql(&self) -> bool {
        false
    }
}

/// A migration with its name, as a module's migration set holds it: the
/// object-safe face of [`SchemaMigration`], which the migrator runs.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> navree_schema::SchemaResult {
/// use navree_schema::{Dialect, NamedMigration, Schema, SchemaMigration, SchemaResult, Target};
///
/// struct CreateDeals;
///
/// impl navree_schema::Migration for CreateDeals {
///     async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
///         schema.create("deals", |t| { t.id(); }).await
///     }
///     async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
///         schema.drop_if_exists("deals").await
///     }
/// }
///
/// let migration: Box<dyn NamedMigration> =
///     Box::new(SchemaMigration::new("m0001_create_deals", CreateDeals));
/// let schema = Schema::offline(Target::sqlite((3, 46)));
/// migration.up(&schema).await?;
/// assert_eq!(migration.name(), "m0001_create_deals");
/// assert!(migration.transactional(Dialect::Postgres));
/// assert!(!migration.transactional(Dialect::MySql));
/// assert_eq!(schema.statements().len(), 1);
/// # Ok(())
/// # }
/// ```
pub trait NamedMigration: Send + Sync + 'static {
    /// The name recorded in `navree_migrations`.
    fn name(&self) -> &str;

    /// Applies the migration.
    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult>;

    /// Reverts the migration.
    fn down<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult>;

    /// Whether the migrator runs it, with its `navree_migrations` row, in a
    /// transaction on `dialect`: PostgreSQL and SQLite unless the
    /// migration opts out, MySQL and MariaDB when it asks.
    fn transactional(&self, dialect: Dialect) -> bool;
}

/// A [`Migration`] with its name, ready for a `MigrationSet`: the
/// migrator runs it as a [`NamedMigration`].
///
/// ```
/// use navree_schema::{Schema, SchemaMigration, SchemaResult};
///
/// struct AddStage;
///
/// impl navree_schema::Migration for AddStage {
///     async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
///         schema.table("deals", |t| { t.string("stage").default("lead"); }).await
///     }
///     async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
///         schema.table("deals", |t| t.drop_column("stage")).await
///     }
/// }
///
/// let migration = SchemaMigration::new("m20261027_093000_add_stage_to_deals", AddStage);
/// assert_eq!(migration.name(), "m20261027_093000_add_stage_to_deals");
/// ```
pub struct SchemaMigration<M> {
    name: String,
    migration: M,
}

impl<M: Migration> SchemaMigration<M> {
    /// `migration`, recorded under `name`.
    pub fn new(name: impl Into<String>, migration: M) -> SchemaMigration<M> {
        SchemaMigration {
            name: name.into(),
            migration,
        }
    }

    /// The name recorded in `navree_migrations`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The migration.
    pub fn migration(&self) -> &M {
        &self.migration
    }
}

impl<M> std::fmt::Debug for SchemaMigration<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaMigration")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl<M: Migration> NamedMigration for SchemaMigration<M> {
    fn name(&self) -> &str {
        &self.name
    }

    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(self.migration.up(schema))
    }

    fn down<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(self.migration.down(schema))
    }

    fn transactional(&self, dialect: Dialect) -> bool {
        self.migration.within_transaction()
            && (dialect != Dialect::MySql || self.migration.within_transaction_on_mysql())
    }
}

/// A migration file's migration, named after the file:
/// `migration!(m20261026_120000_create_deals_table)` is
/// `SchemaMigration::new("m20261026_120000_create_deals_table",
/// m20261026_120000_create_deals_table::Migration)`. Name the type when it
/// is not `Migration`: `migration!(m20261026_create_deals::CreateDeals)`.
///
/// ```
/// # mod m20261026_120000_create_deals_table {
/// #     use navree_schema::{Schema, SchemaResult};
/// #     pub struct Migration;
/// #     impl navree_schema::Migration for Migration {
/// #         async fn up(&self, schema: &Schema<'_>) -> SchemaResult { schema.create("deals", |t| { t.id(); }).await }
/// #         async fn down(&self, schema: &Schema<'_>) -> SchemaResult { schema.drop_if_exists("deals").await }
/// #     }
/// # }
/// let migration = navree_schema::migration!(m20261026_120000_create_deals_table);
/// assert_eq!(migration.name(), "m20261026_120000_create_deals_table");
/// ```
#[macro_export]
macro_rules! migration {
    ($file:ident) => {
        $crate::SchemaMigration::new(::core::stringify!($file), $file::Migration)
    };
    ($file:ident :: $migration:ident) => {
        $crate::SchemaMigration::new(::core::stringify!($file), $file::$migration)
    };
}

tokio::task_local! {
    static CONTEXT: MigrationContext;
}

/// What the migrator tells a running migration: the app's
/// [`SchemaOptions`], whether it only pretends, and where its statements
/// and notices go.
///
/// The migrator (in `navree-db`) runs each migration inside
/// [`scope`](MigrationContext::scope). [`Schema::new`] picks the options
/// up from it, records every statement it runs and every notice it reports
/// here (so the migrate commands can print them), and, when the migrator
/// pretends, reads the database through the context's reader while only
/// recording what it would change.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_schema::SchemaError> {
/// use navree_schema::{MigrationContext, Schema, SchemaOptions, Target};
///
/// let cx = MigrationContext::new("deals/m0001_create_deals", SchemaOptions::default());
/// cx.clone()
///     .scope(async {
///         let schema = Schema::offline(Target::sqlite((3, 46)));
///         schema.create("deals", |t| { t.id(); }).await
///     })
///     .await?;
/// assert_eq!(
///     cx.statements(),
///     ["CREATE TABLE \"deals\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL)"]
/// );
/// assert!(MigrationContext::current().is_none());
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct MigrationContext {
    inner: Arc<ContextInner>,
}

struct ContextInner {
    migration: String,
    options: SchemaOptions,
    reader: Option<Arc<dyn SchemaConnection>>,
    statements: Mutex<Vec<String>>,
    notices: Mutex<Vec<String>>,
}

impl std::fmt::Debug for MigrationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MigrationContext")
            .field("migration", &self.inner.migration)
            .field("options", &self.inner.options)
            .field("pretend", &self.is_pretend())
            .field("statements", &self.statements())
            .field("notices", &self.notices())
            .finish()
    }
}

impl MigrationContext {
    /// The context of migration `migration` (`module/name`), run for real.
    pub fn new(migration: impl Into<String>, options: SchemaOptions) -> MigrationContext {
        MigrationContext::build(migration.into(), options, None)
    }

    /// The context of migration `migration` when the migrator only
    /// pretends: a [`Schema`] reads the database through `reader` (so it
    /// sees the real tables, and SQLite rebuilds and hypertable conversions
    /// plan against them) and records the statements it would run instead
    /// of running them.
    pub fn pretending(
        migration: impl Into<String>,
        options: SchemaOptions,
        reader: Arc<dyn SchemaConnection>,
    ) -> MigrationContext {
        MigrationContext::build(migration.into(), options, Some(reader))
    }

    fn build(
        migration: String,
        options: SchemaOptions,
        reader: Option<Arc<dyn SchemaConnection>>,
    ) -> MigrationContext {
        MigrationContext {
            inner: Arc::new(ContextInner {
                migration,
                options,
                reader,
                statements: Mutex::new(Vec::new()),
                notices: Mutex::new(Vec::new()),
            }),
        }
    }

    /// The context of the migration running on this task, if any.
    pub fn current() -> Option<MigrationContext> {
        CONTEXT.try_with(Clone::clone).ok()
    }

    /// Runs `future` with this context as [`current`](Self::current).
    pub async fn scope<F: Future>(self, future: F) -> F::Output {
        CONTEXT.scope(self, future).await
    }

    /// The migration (`module/name`).
    pub fn migration(&self) -> &str {
        &self.inner.migration
    }

    /// The schema options.
    pub fn options(&self) -> &SchemaOptions {
        &self.inner.options
    }

    /// Whether the migrator only pretends (records the SQL, runs nothing).
    pub fn is_pretend(&self) -> bool {
        self.inner.reader.is_some()
    }

    /// The connection a pretending migration reads through.
    pub fn reader(&self) -> Option<Arc<dyn SchemaConnection>> {
        self.inner.reader.clone()
    }

    /// Records a statement the migration ran (or, pretending, would run).
    pub fn record(&self, statement: impl Into<String>) {
        self.inner
            .statements
            .lock()
            .expect("statements lock")
            .push(statement.into());
    }

    /// The statements so far, in order.
    pub fn statements(&self) -> Vec<String> {
        self.inner
            .statements
            .lock()
            .expect("statements lock")
            .clone()
    }

    /// Records a notice.
    pub fn notice(&self, message: impl Into<String>) {
        self.inner
            .notices
            .lock()
            .expect("notices lock")
            .push(message.into());
    }

    /// The notices so far.
    pub fn notices(&self) -> Vec<String> {
        self.inner.notices.lock().expect("notices lock").clone()
    }
}
