//! Workspace migrations: the migration sets that run in every workspace's
//! own schema or database (schema and database isolation), and the
//! per-workspace database registry ([`WorkspaceDatabases`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;

use navree_db::{
    Db, DbError, DbOptions, Executor, MigrationSet, SetMigration, record_migration,
    recorded_migrations,
};
use navree_kernel::AppContext;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::WorkspaceError;

/// A [`MigrationSet`] that runs in each workspace's schema or database,
/// contributed with
/// [`ModuleBuilderWorkspaceExt::workspace_migrations`](crate::ModuleBuilderWorkspaceExt::workspace_migrations).
pub(crate) struct WorkspaceMigrationSet(pub(crate) MigrationSet);

impl fmt::Debug for WorkspaceMigrationSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The workspace migration sets of `ctx`.
pub(crate) fn workspace_sets(ctx: &AppContext) -> Vec<Arc<WorkspaceMigrationSet>> {
    ctx.contributions::<WorkspaceMigrationSet>()
        .into_iter()
        .map(|c| c.value)
        .collect()
}

/// One migration in run order.
struct Step<'a> {
    module: &'static str,
    migration: SetMigration<'a>,
}

impl Step<'_> {
    fn label(&self) -> String {
        format!("{}/{}", self.module, self.migration.name())
    }
}

/// Every migration in run order: sets topologically by `after` (ties by
/// module name; names of absent sets are ignored), migrations by name.
fn plan(sets: &[Arc<WorkspaceMigrationSet>]) -> Result<Vec<Step<'_>>, WorkspaceError> {
    let mut by_module: BTreeMap<&'static str, &MigrationSet> = BTreeMap::new();
    for set in sets {
        if by_module.insert(set.0.module, &set.0).is_some() {
            return Err(WorkspaceError::Other(
                format!(
                    "two workspace migration sets are named `{}`; module names must be unique",
                    set.0.module
                )
                .into(),
            ));
        }
    }
    let mut waiting_on: BTreeMap<&str, BTreeSet<&str>> = by_module
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
    while let Some(next) = waiting_on
        .iter()
        .find(|(_, deps)| deps.is_empty())
        .map(|(module, _)| *module)
    {
        waiting_on.remove(next);
        for deps in waiting_on.values_mut() {
            deps.remove(next);
        }
        order.push(next);
    }
    if !waiting_on.is_empty() {
        let modules: Vec<&str> = waiting_on.keys().copied().collect();
        return Err(WorkspaceError::Other(
            format!(
                "workspace migration order cycle between {}",
                modules.join(", ")
            )
            .into(),
        ));
    }
    let mut steps = Vec::new();
    for module in order {
        let set = by_module[module];
        steps.extend(set.entries().into_iter().map(|migration| Step {
            module: set.module,
            migration,
        }));
    }
    Ok(steps)
}

/// The applied migrations recorded in `navree_migrations` (created when
/// missing) as seen through `conn`, and the next batch number.
async fn applied(conn: &dyn Executor) -> Result<(HashMap<(String, String), i64>, i64), DbError> {
    let done = recorded_migrations(conn).await?;
    let batch = done.values().copied().max().unwrap_or(0);
    Ok((done, batch + 1))
}

/// Applies `step` on `conn` and records it there.
async fn run_step(conn: &dyn Executor, step: &Step<'_>, batch: i64) -> Result<(), DbError> {
    step.migration.up_on(conn).await?;
    record_migration(conn, step.module, step.migration.name(), batch).await
}

fn failed(step: &Step<'_>, error: DbError) -> WorkspaceError {
    WorkspaceError::Other(format!("workspace migration `{}` failed: {error}", step.label()).into())
}

/// Runs every pending workspace migration on `conn`, a unit of work on the
/// app's database whose `search_path` starts with the workspace's schema
/// (schema isolation); returns `module/name` of each.
pub(crate) async fn migrate_in(
    conn: &dyn Executor,
    sets: &[Arc<WorkspaceMigrationSet>],
) -> Result<Vec<String>, WorkspaceError> {
    let steps = plan(sets)?;
    let (done, batch) = applied(conn).await?;
    let mut ran = Vec::new();
    for step in &steps {
        if done.contains_key(&(step.module.to_owned(), step.migration.name().to_owned())) {
            continue;
        }
        run_step(conn, step, batch)
            .await
            .map_err(|error| failed(step, error))?;
        ran.push(step.label());
    }
    Ok(ran)
}

/// Runs every pending workspace migration against `db` (database
/// isolation), each in its own transaction; returns `module/name` of each.
pub(crate) async fn migrate_db(
    db: &Db,
    sets: &[Arc<WorkspaceMigrationSet>],
) -> Result<Vec<String>, WorkspaceError> {
    let steps = plan(sets)?;
    let (done, batch) = applied(db).await?;
    let mut ran = Vec::new();
    for step in &steps {
        if done.contains_key(&(step.module.to_owned(), step.migration.name().to_owned())) {
            continue;
        }
        tracing::info!(target: "navree::workspace", migration = %step.label(), "migrating a workspace database");
        let tx = db.begin().await?;
        match run_step(&tx, step, batch).await {
            Ok(()) => tx.commit().await.map_err(|error| failed(step, error))?,
            Err(error) => {
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(target: "navree::workspace", error = %rollback, "rollback failed");
                }
                return Err(failed(step, error));
            }
        }
        ran.push(step.label());
    }
    Ok(ran)
}

/// The databases of workspaces under database isolation: one pool per
/// workspace, connected on first use from `workspace.database_url` (with
/// `{id}` replaced by the workspace id) and migrated with the workspace
/// migrations the first time it is opened by this process. Provided and
/// exported by the workspace module.
///
/// ```no_run
/// # async fn demo(databases: &navree_workspace::WorkspaceDatabases, id: uuid::Uuid) -> Result<(), navree_workspace::WorkspaceError> {
/// let db = databases.db(id).await?;      // Arc<navree_db::Db>
/// let uow = db.uow(None, None).await;    // a unit of work in that database
/// # Ok(()) }
/// ```
pub struct WorkspaceDatabases {
    template: Option<String>,
    options: DbOptions,
    app: navree_kernel::WeakAppContext,
    pools: Mutex<HashMap<Uuid, Arc<Db>>>,
}

impl WorkspaceDatabases {
    pub(crate) fn new(
        template: Option<String>,
        options: DbOptions,
        app: navree_kernel::WeakAppContext,
    ) -> Self {
        Self {
            template,
            options,
            app,
            pools: Mutex::new(HashMap::new()),
        }
    }

    /// The database URL of `workspace`, if `workspace.database_url` is set.
    pub fn url(&self, workspace: Uuid) -> Option<String> {
        self.template
            .as_deref()
            .map(|template| database_url(template, workspace))
    }

    /// The database of `workspace`: connected (and migrated) on first use,
    /// then shared.
    pub async fn db(&self, workspace: Uuid) -> Result<Arc<Db>, WorkspaceError> {
        Ok(self.open(workspace).await?.0)
    }

    /// The database of `workspace` and the migrations that ran because this
    /// call connected it.
    pub(crate) async fn open(
        &self,
        workspace: Uuid,
    ) -> Result<(Arc<Db>, Vec<String>), WorkspaceError> {
        let mut pools = self.pools.lock().await;
        if let Some(db) = pools.get(&workspace) {
            return Ok((db.clone(), Vec::new()));
        }
        let url = self.url(workspace).ok_or_else(|| {
            WorkspaceError::Other(
                "workspace.database_url is not set: database isolation needs a URL template".into(),
            )
        })?;
        // The main database's settings, without its read replicas.
        let options = self.options.for_database(url);
        let db = Arc::new(Db::connect(&options).await.map_err(WorkspaceError::db)?);
        let ran = match self.app.upgrade() {
            Some(app) => migrate_db(&db, &workspace_sets(&app)).await?,
            None => Vec::new(),
        };
        pools.insert(workspace, db.clone());
        Ok((db, ran))
    }

    /// Closes and forgets the pool of `workspace` (the database itself is
    /// left in place).
    pub async fn forget(&self, workspace: Uuid) {
        let db = self.pools.lock().await.remove(&workspace);
        if let Some(db) = db
            && let Err(error) = db.close().await
        {
            tracing::warn!(target: "navree::workspace", %error, "closing a workspace database failed");
        }
    }

    /// Closes every pool (at shutdown).
    pub(crate) async fn close_all(&self) {
        let pools: Vec<Arc<Db>> = self.pools.lock().await.drain().map(|(_, db)| db).collect();
        for db in pools {
            if let Err(error) = db.close().await {
                tracing::warn!(target: "navree::workspace", %error, "closing a workspace database failed");
            }
        }
    }
}

impl fmt::Debug for WorkspaceDatabases {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkspaceDatabases")
            .field("template", &self.template.as_deref().map(redact))
            .finish_non_exhaustive()
    }
}

/// `template` with `{id}` replaced by `workspace` (simple form).
fn database_url(template: &str, workspace: Uuid) -> String {
    template.replace("{id}", &workspace.simple().to_string())
}

/// The URL without a password (`scheme://user:***@host/...`).
fn redact(url: &str) -> String {
    let Some(start) = url.find("://").map(|i| i + 3) else {
        return url.to_owned();
    };
    let rest = &url[start..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    match (authority.rfind('@'), authority.find(':')) {
        (Some(at), Some(colon)) if colon < at => format!(
            "{}{}***{}",
            &url[..start],
            &authority[..=colon],
            &url[start + at..]
        ),
        _ => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_redacted() {
        assert_eq!(
            redact("postgres://navree:secret@db/ws_{id}"),
            "postgres://navree:***@db/ws_{id}"
        );
        assert_eq!(
            redact("sqlite://storage/{id}.db?mode=rwc"),
            "sqlite://storage/{id}.db?mode=rwc"
        );
    }

    #[test]
    fn urls_name_the_workspace() {
        let id = Uuid::parse_str("0199a5b2-0000-7000-8000-000000000001").unwrap();
        assert_eq!(
            database_url("sqlite://storage/workspaces/{id}.db?mode=rwc", id),
            "sqlite://storage/workspaces/0199a5b2000070008000000000000001.db?mode=rwc"
        );
    }
}
