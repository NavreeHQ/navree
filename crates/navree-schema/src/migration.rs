//! Migrations written with the schema builder: the [`Migration`] trait,
//! the [`SchemaMigration`] adapter the migrator runs, the
//! [`MigrationContext`] the migrator provides, and the [`migration!`]
//! macro.
//!
//! [`migration!`]: crate::migration!

use std::future::Future;
use std::sync::{Arc, Mutex};

use sea_orm::DbErr;
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

use crate::error::SchemaResult;
use crate::options::SchemaOptions;
use crate::schema::Schema;

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

    /// Whether the migrator runs it in a transaction (`true`, the default,
    /// where the database's DDL is transactional). Return `false` for
    /// statements PostgreSQL refuses in a transaction, such as `CREATE
    /// INDEX CONCURRENTLY` (an index's `.online()`) or refreshing a
    /// continuous aggregate.
    fn within_transaction(&self) -> bool {
        true
    }
}

/// A [`Migration`] with its name, ready for a `MigrationSet`: it implements
/// the migrator's migration trait, so every runner runs it.
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

impl<M: Migration> MigrationName for SchemaMigration<M> {
    fn name(&self) -> &str {
        &self.name
    }
}

#[async_trait]
impl<M: Migration> MigrationTrait for SchemaMigration<M> {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(manager.get_connection());
        self.migration.up(&schema).await.map_err(DbErr::from)
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(manager.get_connection());
        self.migration.down(&schema).await.map_err(DbErr::from)
    }

    fn use_transaction(&self) -> Option<bool> {
        (!self.migration.within_transaction()).then_some(false)
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
/// [`SchemaOptions`], whether it is only pretending, and where notices go.
///
/// The migrator (in `navree-db`) sets it around each migration;
/// [`Schema::new`] reads the options from it, and every notice the schema
/// reports is collected here so the migrate commands can print them.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// use navree_schema::{MigrationContext, SchemaOptions};
///
/// let cx = MigrationContext::new("deals/m0001_create_deals", SchemaOptions::default());
/// cx.clone()
///     .scope(async {
///         MigrationContext::current().unwrap().notice("hello");
///     })
///     .await;
/// assert_eq!(cx.notices(), ["hello"]);
/// assert!(MigrationContext::current().is_none());
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct MigrationContext {
    inner: Arc<ContextInner>,
}

#[derive(Debug)]
struct ContextInner {
    migration: String,
    options: SchemaOptions,
    pretend: bool,
    notices: Mutex<Vec<String>>,
}

impl MigrationContext {
    /// The context of migration `migration` (`module/name`).
    pub fn new(migration: impl Into<String>, options: SchemaOptions) -> MigrationContext {
        MigrationContext {
            inner: Arc::new(ContextInner {
                migration: migration.into(),
                options,
                pretend: false,
                notices: Mutex::new(Vec::new()),
            }),
        }
    }

    /// The same context, only pretending (statements are printed, not
    /// run).
    pub fn pretending(self, pretend: bool) -> MigrationContext {
        MigrationContext {
            inner: Arc::new(ContextInner {
                migration: self.inner.migration.clone(),
                options: self.inner.options.clone(),
                pretend,
                notices: Mutex::new(self.notices()),
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

    /// Whether the migrator only pretends (prints the SQL).
    pub fn is_pretend(&self) -> bool {
        self.inner.pretend
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
