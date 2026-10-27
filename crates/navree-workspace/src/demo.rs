//! Demo workspaces: [`DemoSeeder`]s fill them from fixtures,
//! [`DemoWorkspaces`] creates, lists and resets them, and every request to
//! one is answered with the `X-Navree-Demo` header.

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_db::Uow;
use navree_kernel::{AppContext, BoxError, BoxFuture};
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Query};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder,
};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{Workspace, WorkspaceKind, workspace};
use crate::events::DemoWorkspaceSeeded;
use crate::isolation::WorkspaceScoped;
use crate::{DemoMode, DemoOptions, Isolation, NewWorkspace, WorkspaceError, Workspaces, fixtures};

/// The response header that marks a demo workspace's responses:
/// `X-Navree-Demo: read_only` or `X-Navree-Demo: reset` (the
/// [`DemoMode`]). Responses about live workspaces don't carry it. A front
/// end shows its Live/Demo switch from it; it is in the CORS
/// `Access-Control-Expose-Headers` list.
pub const DEMO_HEADER: &str = "x-navree-demo";

/// Fills a demo workspace with a module's sample data.
///
/// Register one per module with
/// [`ModuleBuilderWorkspaceExt::demo_seeder`](crate::ModuleBuilderWorkspaceExt::demo_seeder);
/// [`DemoWorkspaces`] runs them, in module dependency order (then
/// registration order), when a demo is created and when it is reset. A
/// reset first clears every seeder's data, in the reverse order.
///
/// ```
/// use navree_kernel::{BoxError, BoxFuture};
/// use navree_workspace::{DemoSeed, DemoSeeder};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Company {
///     name: String,
/// }
///
/// struct CrmDemo;
///
/// impl DemoSeeder for CrmDemo {
///     fn name(&self) -> &'static str {
///         "crm"
///     }
///
///     // Cleared (this workspace's rows) before a reset seeds again.
///     fn tables(&self) -> &'static [&'static str] {
///         &["companies"]
///     }
///
///     fn seed<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
///         Box::pin(async move {
///             // `fixtures/demo/companies.json` (`workspace.demo.fixtures`).
///             let companies: Vec<Company> = demo.fixture("companies")?;
///             for company in companies {
///                 // Insert with WorkspaceRepo::<company::Entity>::insert(demo.uow(), demo.id(), ..).
///                 let _ = company.name;
///             }
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait DemoSeeder: Send + Sync + 'static {
    /// The seeder's name (`crm`), unique in the app; `workspace:demo-reset`
    /// prints it.
    fn name(&self) -> &'static str;

    /// The workspace-owned tables this seeder fills, children before
    /// parents: the default [`clear`](DemoSeeder::clear) empties them, in
    /// this order, before a reset seeds again. Empty by default.
    fn tables(&self) -> &'static [&'static str] {
        &[]
    }

    /// Writes the seeder's data into the demo workspace through
    /// [`DemoSeed::uow`].
    fn seed<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>>;

    /// Removes what [`seed`](DemoSeeder::seed) wrote, before a reset seeds
    /// again. The default runs [`DemoSeed::clear_table`] on each of
    /// [`tables`](DemoSeeder::tables); override it for tables with
    /// another workspace column ([`DemoSeed::clear`]) or data elsewhere.
    fn clear<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            for table in self.tables() {
                demo.clear_table(table).await?;
            }
            Ok(())
        })
    }
}

/// What a [`DemoSeeder`] seeds: the demo workspace, the unit of work to
/// write in, the time, and the fixtures.
pub struct DemoSeed {
    app: AppContext,
    workspace: Workspace,
    uow: Uow,
    isolation: Isolation,
    schema: Option<String>,
    fixtures: PathBuf,
    now: DateTime<Utc>,
    reset: bool,
}

impl DemoSeed {
    /// The app, for services the seeder needs (it may also take them as
    /// injected dependencies).
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The demo workspace.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The demo workspace's id.
    pub fn id(&self) -> Uuid {
        self.workspace.id
    }

    /// The unit of work for the workspace's data, already scoped to it:
    /// the app database's under row and schema isolation (with
    /// `navree.workspace` and, for schemas, the `search_path` set on
    /// PostgreSQL), the workspace's own database under database isolation.
    /// Write with `WorkspaceRepo` (row isolation) or `Repo`; it is also
    /// the task's current unit of work (`Uow::current()`).
    pub fn uow(&self) -> &Uow {
        &self.uow
    }

    /// The time of seeding, from the app's `dyn Clock`: write dates
    /// relative to it so a demo seeded tonight looks current tomorrow.
    pub fn now(&self) -> DateTime<Utc> {
        self.now
    }

    /// The configured isolation strategy.
    pub fn isolation(&self) -> Isolation {
        self.isolation
    }

    /// Whether this run follows a clear (`workspace:demo-reset`), rather
    /// than the workspace's creation.
    pub fn is_reset(&self) -> bool {
        self.reset
    }

    /// Reads fixture `name`: `<workspace.demo.fixtures>/<name>.json`
    /// (`name` may hold directories: `crm/deals`), with its date
    /// placeholders (`{{ now }}`, `{{ now - 3d }}`, `{{ today + 1w }}`)
    /// resolved against [`now`](Self::now), deserialized into `T`.
    pub fn fixture<T: DeserializeOwned>(&self, name: &str) -> Result<T, BoxError> {
        fixtures::read(&self.fixtures, name, self.now)
    }

    /// Parses a fixture held in the code (`include_str!`) the way
    /// [`fixture`](Self::fixture) reads a file.
    pub fn parse_fixture<T: DeserializeOwned>(&self, json: &str) -> Result<T, BoxError> {
        fixtures::parse(json, self.now)
    }

    /// Deletes the demo workspace's rows from `table`, in
    /// [`uow`](Self::uow): those with its id in `workspace_id` under row
    /// isolation, every row of the workspace's own table under schema and
    /// database isolation. Returns the number deleted.
    pub async fn clear_table(&self, table: &str) -> Result<u64, DbErr> {
        self.delete_rows(table, "workspace_id").await
    }

    /// [`clear_table`](Self::clear_table) for entity `E`, with its
    /// [`WORKSPACE_COLUMN`](WorkspaceScoped::WORKSPACE_COLUMN).
    pub async fn clear<E: WorkspaceScoped>(&self) -> Result<u64, DbErr> {
        self.delete_rows(E::default().table_name(), E::WORKSPACE_COLUMN)
            .await
    }

    async fn delete_rows(&self, table: &str, column: &str) -> Result<u64, DbErr> {
        let mut delete = Query::delete();
        match (&self.isolation, &self.schema) {
            (Isolation::Schema, Some(schema)) => {
                delete.from_table((Alias::new(schema.as_str()), Alias::new(table)));
            }
            (Isolation::Database, _) => {
                delete.from_table(Alias::new(table));
            }
            _ => {
                delete
                    .from_table(Alias::new(table))
                    .and_where(Expr::col(Alias::new(column)).eq(self.workspace.id));
            }
        }
        let tx = self.uow.tx().await?;
        Ok(tx.execute(&delete).await?.rows_affected())
    }
}

impl fmt::Debug for DemoSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DemoSeed")
            .field("workspace", &self.workspace.slug)
            .field("isolation", &self.isolation)
            .field("fixtures", &self.fixtures)
            .field("now", &self.now)
            .field("reset", &self.reset)
            .finish_non_exhaustive()
    }
}

/// A demo workspace that was seeded again ([`DemoWorkspaces::reset`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoReset {
    /// The workspace.
    pub workspace: Workspace,
    /// The seeders that ran, in order.
    pub seeders: Vec<String>,
}

/// Creates, lists and resets demo workspaces. Provided and exported by the
/// workspace module.
///
/// A demo workspace is an ordinary workspace of kind
/// [`WorkspaceKind::Demo`], filled by the app's [`DemoSeeder`]s. With
/// `workspace.demo.mode = "read_only"` (the default) requests that would
/// write to it are refused (403, code `demo_read_only`); with `"reset"`
/// they are allowed, and `workspace:demo-reset` (nightly at
/// `workspace.demo.reset_at` when `ScheduleModule` is in the app) clears
/// and seeds it again.
pub struct DemoWorkspaces {
    core: Arc<Core>,
    workspaces: Arc<Workspaces>,
}

impl DemoWorkspaces {
    pub(crate) fn new(core: Arc<Core>, workspaces: Arc<Workspaces>) -> Self {
        Self { core, workspaces }
    }

    /// The `workspace.demo` options.
    pub fn options(&self) -> &DemoOptions {
        &self.core.config.demo
    }

    /// `workspace.demo.mode`.
    pub fn mode(&self) -> DemoMode {
        self.core.config.demo.mode
    }

    /// The names of the registered seeders, in the order they run. Two
    /// seeders with one name are an error.
    pub async fn seeders(&self) -> Result<Vec<&'static str>, WorkspaceError> {
        Ok(self
            .registered()
            .await?
            .iter()
            .map(|seeder| seeder.name())
            .collect())
    }

    /// Creates demo workspace `input`, owned by `owner` (who joins it as
    /// `owner`), and seeds it with every [`DemoSeeder`]: in `uow`, which
    /// then stays scoped to the new workspace (`Workspaces::enter`), under
    /// row and schema isolation; in the workspace's own database, committed
    /// before this returns, under database isolation. Emits
    /// [`WorkspaceCreated`](crate::events::WorkspaceCreated) and
    /// [`DemoWorkspaceSeeded`] once `uow` commits. A failing seeder fails
    /// the call; roll `uow` back.
    pub async fn create(
        &self,
        uow: &Uow,
        owner: Uuid,
        input: NewWorkspace,
    ) -> Result<Workspace, WorkspaceError> {
        let seeders = self.registered().await?;
        let workspace = self
            .workspaces
            .create_kind(uow, owner, input, WorkspaceKind::Demo)
            .await?;
        let names = match self.core.isolation.isolation {
            Isolation::Database => self.seed_tenant(&workspace, &seeders, false).await?,
            _ => {
                self.core.isolation.apply(uow, workspace.id).await?;
                self.run(uow, &workspace, &seeders, false).await?
            }
        };
        self.seeded(uow, &workspace, names, false).await;
        Ok(workspace)
    }

    /// Clears and seeds demo workspace `id` again, in a unit of work of its
    /// own, and emits [`DemoWorkspaceSeeded`] (with `reset: true`) once it
    /// commits. [`WorkspaceError::NotDemo`] for a live workspace.
    pub async fn reset(&self, id: Uuid) -> Result<DemoReset, WorkspaceError> {
        let workspace = self
            .workspaces
            .find(id)
            .await?
            .ok_or_else(|| WorkspaceError::WorkspaceNotFound(id.to_string()))?;
        if !workspace.is_demo() {
            return Err(WorkspaceError::NotDemo(workspace.slug));
        }
        let seeders = self.registered().await?;
        self.reset_workspace(workspace, &seeders).await
    }

    /// [`reset`](Self::reset)s every demo workspace, by creation; stops at
    /// the first failure (each reset is its own transaction).
    pub async fn reset_all(&self) -> Result<Vec<DemoReset>, WorkspaceError> {
        let seeders = self.registered().await?;
        let mut reset = Vec::new();
        for workspace in self.all().await? {
            reset.push(self.reset_workspace(workspace, &seeders).await?);
        }
        Ok(reset)
    }

    /// Every demo workspace, by creation.
    pub async fn all(&self) -> Result<Vec<Workspace>, WorkspaceError> {
        Ok(workspace::Entity::find()
            .filter(workspace::Column::Kind.eq(WorkspaceKind::Demo.as_str()))
            .order_by_asc(workspace::Column::CreatedAt)
            .all(&self.core.db.reader().await)
            .await?)
    }

    /// The registered seeders, checked for unique names.
    async fn registered(&self) -> Result<Vec<Arc<dyn DemoSeeder>>, WorkspaceError> {
        let app = self.app()?;
        let seeders = app
            .get_all::<dyn DemoSeeder>()
            .await
            .map_err(|error| WorkspaceError::Other(error.into()))?;
        let mut names = BTreeSet::new();
        for seeder in &seeders {
            if !names.insert(seeder.name()) {
                return Err(WorkspaceError::Other(
                    format!(
                        "two demo seeders are named `{}`; seeder names must be unique",
                        seeder.name()
                    )
                    .into(),
                ));
            }
        }
        Ok(seeders)
    }

    fn app(&self) -> Result<AppContext, WorkspaceError> {
        self.core
            .app
            .upgrade()
            .ok_or_else(|| WorkspaceError::Other("the app has shut down".into()))
    }

    /// Clears and seeds `workspace` in a new unit of work, committed here.
    async fn reset_workspace(
        &self,
        workspace: Workspace,
        seeders: &[Arc<dyn DemoSeeder>],
    ) -> Result<DemoReset, WorkspaceError> {
        let uow = match self.core.isolation.isolation {
            Isolation::Database => {
                self.core
                    .isolation
                    .databases
                    .db(workspace.id)
                    .await?
                    .uow(None, None)
                    .await
            }
            _ => self.core.db.uow(None, None).await,
        };
        let result = async {
            if self.core.isolation.isolation != Isolation::Database {
                self.core.isolation.apply(&uow, workspace.id).await?;
            }
            self.run(&uow, &workspace, seeders, true).await
        }
        .await;
        match result {
            Ok(names) => {
                self.seeded(&uow, &workspace, names.clone(), true).await;
                uow.commit().await?;
                tracing::info!(target: "navree::workspace", workspace = %workspace.slug, seeders = ?names, "demo workspace reset");
                Ok(DemoReset {
                    workspace,
                    seeders: names,
                })
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::workspace", error = %rollback, "rolling back a demo reset failed");
                }
                Err(error)
            }
        }
    }

    /// Seeds `workspace`'s own database (database isolation) in a unit of
    /// work committed here.
    async fn seed_tenant(
        &self,
        workspace: &Workspace,
        seeders: &[Arc<dyn DemoSeeder>],
        reset: bool,
    ) -> Result<Vec<String>, WorkspaceError> {
        let tenant = self.core.isolation.databases.db(workspace.id).await?;
        let uow = tenant.uow(None, None).await;
        match self.run(&uow, workspace, seeders, reset).await {
            Ok(names) => {
                uow.commit().await?;
                Ok(names)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::workspace", error = %rollback, "rolling back a demo seed failed");
                }
                Err(error)
            }
        }
    }

    /// Runs the seeders in `uow` (clearing first, in reverse, when
    /// `reset`); returns their names.
    async fn run(
        &self,
        uow: &Uow,
        workspace: &Workspace,
        seeders: &[Arc<dyn DemoSeeder>],
        reset: bool,
    ) -> Result<Vec<String>, WorkspaceError> {
        let demo = DemoSeed {
            app: self.app()?,
            workspace: workspace.clone(),
            uow: uow.clone(),
            isolation: self.core.isolation.isolation,
            schema: (self.core.isolation.isolation == Isolation::Schema)
                .then(|| self.core.isolation.schema_name(workspace.id)),
            fixtures: PathBuf::from(self.core.config.demo.fixtures.trim()),
            now: self.core.now(),
            reset,
        };
        uow.scope(async {
            if reset {
                for seeder in seeders.iter().rev() {
                    seeder
                        .clear(&demo)
                        .await
                        .map_err(|error| failed("clearing", seeder.name(), error))?;
                }
            }
            let mut names = Vec::with_capacity(seeders.len());
            for seeder in seeders {
                seeder
                    .seed(&demo)
                    .await
                    .map_err(|error| failed("seeding", seeder.name(), error))?;
                names.push(seeder.name().to_owned());
            }
            Ok(names)
        })
        .await
    }

    /// Emits [`DemoWorkspaceSeeded`] once `uow` commits.
    async fn seeded(&self, uow: &Uow, workspace: &Workspace, seeders: Vec<String>, reset: bool) {
        self.core
            .emit_after_commit(
                uow,
                DemoWorkspaceSeeded {
                    workspace_id: workspace.id,
                    slug: workspace.slug.clone(),
                    seeders,
                    reset,
                },
            )
            .await;
    }
}

impl fmt::Debug for DemoWorkspaces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DemoWorkspaces")
            .field("mode", &self.mode())
            .finish_non_exhaustive()
    }
}

fn failed(what: &str, seeder: &str, error: BoxError) -> WorkspaceError {
    WorkspaceError::Other(format!("demo seeder `{seeder}` failed {what}: {error}").into())
}
