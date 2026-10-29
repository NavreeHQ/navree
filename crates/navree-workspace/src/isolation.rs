//! Keeping workspaces apart: [`WorkspaceScoped`] models and
//! [`WorkspaceRepo`] (row isolation), [`RlsPolicy`] (PostgreSQL row-level
//! security), and the per-transaction settings of each isolation strategy.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_db::{Backend, Executor as _, Statement, Uow, Value};
use navree_kernel::WeakAppContext;
use navree_orm::{Builder, Executor, Model, OrmError, PrimaryKey, Record, col};
use uuid::Uuid;

use crate::tenant::{WorkspaceDatabases, migrate_in, workspace_sets};
use crate::{Isolation, WorkspaceError};

/// A navree-orm model whose rows belong to a workspace (row isolation): its
/// table has a workspace id column, `workspace_id` unless
/// [`WORKSPACE_COLUMN`](WorkspaceScoped::WORKSPACE_COLUMN) says otherwise.
///
/// ```ignore
/// impl WorkspaceScoped for Deal {}
/// ```
///
/// A model declared `#[model(workspace)]` scopes its own queries to the
/// current workspace; `WorkspaceScoped` adds [`WorkspaceRepo`] (queries
/// and writes for a workspace named explicitly), the
/// [`InWorkspace`](crate::InWorkspace) guard's record check
/// ([`workspace_scoped`](crate::ModuleBuilderWorkspaceExt::workspace_scoped))
/// and the `navree check` classification. Give both the same column.
pub trait WorkspaceScoped: Model {
    /// The column holding the workspace id.
    const WORKSPACE_COLUMN: &'static str = "workspace_id";
}

/// The workspace column of `M`, qualified with its table, as a filter on
/// `workspace`.
fn in_workspace<M: WorkspaceScoped>(workspace: Uuid) -> navree_orm::Filter {
    col(format!("{}.{}", M::meta().table(), M::WORKSPACE_COLUMN)).eq(workspace)
}

/// Queries and writes of model `M` scoped to one workspace (row
/// isolation): every query filters by the model's workspace column (in
/// place of the model's own workspace scope, if it has one), inserts set
/// it, and updates and deletes only touch rows of that workspace, so a
/// record of another workspace is [`OrmError::ModelNotFound`] (a 404 over
/// HTTP). Writes go through the model API (its hooks, and the change
/// events the audit trail reads), inside
/// [`navree_orm::workspace::scope`] for that workspace.
///
/// ```ignore
/// type Deals = WorkspaceRepo<Deal>;
///
/// async fn show(ws: CurrentWorkspace, uow: Uow, Path(id): Path<Uuid>) -> Result<Json<Record<Deal>>> {
///     Ok(Json(Deals::find_or_fail(&uow, ws.id(), id).await?))
/// }
///
/// async fn index(ws: CurrentWorkspace, uow: Uow, page: PageRequest) -> Result<Json<Page<Deal>>> {
///     Ok(Json(Deals::query(ws.id()).paginate(&uow, page).await?))
/// }
/// ```
pub struct WorkspaceRepo<M>(PhantomData<M>);

impl<M: WorkspaceScoped> WorkspaceRepo<M> {
    /// `query` restricted to the rows of `workspace`.
    pub fn scope(workspace: Uuid, query: Builder<M>) -> Builder<M> {
        query
            .without_workspace_scope()
            .where_(in_workspace::<M>(workspace))
    }

    /// Every row of `workspace` (`M::query()` scoped).
    pub fn query(workspace: Uuid) -> Builder<M> {
        Self::scope(workspace, M::query())
    }

    /// The record of `workspace` with key `key`.
    pub async fn find<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        key: impl Into<M::Key>,
    ) -> Result<Option<Record<M>>, OrmError> {
        Self::query(workspace).where_key(key).first(exec).await
    }

    /// Like [`find`](WorkspaceRepo::find), but [`OrmError::ModelNotFound`]
    /// (a 404) when `workspace` has no such record.
    pub async fn find_or_fail<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        key: impl Into<M::Key>,
    ) -> Result<Record<M>, OrmError> {
        let key = key.into();
        Self::find(exec, workspace, key.clone())
            .await?
            .ok_or_else(|| not_found::<M>(&key))
    }

    /// Creates `model` in `workspace` (its workspace column is set).
    pub async fn insert<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        mut model: M,
    ) -> Result<Record<M>, OrmError> {
        model.set_value(M::WORKSPACE_COLUMN, Value::from(workspace))?;
        navree_orm::workspace::scope(workspace, M::create(exec, model)).await
    }

    /// Saves `record`, a record of `workspace` ([`OrmError::ModelNotFound`]
    /// otherwise); its workspace column cannot change.
    pub async fn update<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        record: &mut Record<M>,
    ) -> Result<(), OrmError> {
        let key = record.key();
        if !Self::query(workspace)
            .where_key(key.clone())
            .exists(exec)
            .await?
        {
            return Err(not_found::<M>(&key));
        }
        record
            .model_mut()
            .set_value(M::WORKSPACE_COLUMN, Value::from(workspace))?;
        navree_orm::workspace::scope(workspace, record.save(exec)).await?;
        Ok(())
    }

    /// Deletes `record`, a record of `workspace` ([`OrmError::ModelNotFound`]
    /// otherwise).
    pub async fn delete<E: Executor + ?Sized>(
        exec: &E,
        workspace: Uuid,
        mut record: Record<M>,
    ) -> Result<(), OrmError> {
        let key = record.key();
        let ours = record.model().value(M::WORKSPACE_COLUMN)? == Some(Value::from(workspace));
        if !ours
            || !Self::query(workspace)
                .where_key(key.clone())
                .exists(exec)
                .await?
        {
            return Err(not_found::<M>(&key));
        }
        navree_orm::workspace::scope(workspace, record.delete(exec)).await?;
        Ok(())
    }
}

impl<M> fmt::Debug for WorkspaceRepo<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WorkspaceRepo<{}>", std::any::type_name::<M>())
    }
}

/// The 404 for record `key` of `M` that the workspace (or organization,
/// partition) does not have.
pub(crate) fn not_found<M: Model>(key: &M::Key) -> OrmError {
    OrmError::not_found(M::meta().name(), vec![key.to_key_string()])
}

/// PostgreSQL row-level security for a workspace-owned table: rows are
/// visible and writable only while the transaction's `navree.workspace`
/// setting holds their workspace id, which every workspace request sets
/// (and [`Workspaces::enter`](crate::Workspaces::enter) sets for jobs and
/// commands).
///
/// ```
/// use navree_workspace::RlsPolicy;
///
/// let sql = RlsPolicy::new("deals").create_sql();
/// assert!(sql.contains("ALTER TABLE \"deals\" ENABLE ROW LEVEL SECURITY"));
/// assert!(sql.contains("current_setting('navree.workspace', true)"));
/// ```
///
/// The policy is `FORCE`d, so it binds the table's owner too (the role the
/// app usually connects as). Without the setting no row is visible: maintenance
/// across workspaces runs as a role with `BYPASSRLS`. Run the SQL in a
/// migration (`schema.statement(&sql)`, after checking that
/// `schema.target().await?.dialect()` is PostgreSQL) on PostgreSQL only.
///
/// Two options widen and narrow it:
///
/// - [`organizations`](RlsPolicy::organizations): a second policy, for
///   `SELECT` only, makes the rows of every workspace of the organization
///   in `navree.organization` (set by
///   [`CurrentOrganization`](crate::CurrentOrganization)) readable, so
///   organization dashboards read through [`OrgRepo`](crate::OrgRepo).
///   Inserts, updates and deletes still need `navree.workspace`.
/// - [`partitioned`](RlsPolicy::partitioned): while `navree.partition` is
///   set (by [`CurrentPartition`](crate::CurrentPartition) and
///   [`PartitionScope`](crate::PartitionScope) when the request names a
///   partition), only that partition's rows are visible and writable.
///
/// ```
/// use navree_workspace::RlsPolicy;
///
/// let sql = RlsPolicy::new("contacts").organizations().partitioned().create_sql();
/// assert!(sql.contains("current_setting('navree.organization', true)"));
/// assert!(sql.contains("\"partition_id\" = NULLIF(current_setting('navree.partition', true), '')::uuid"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RlsPolicy {
    table: String,
    column: String,
    organizations: bool,
    partition_column: Option<String>,
}

impl RlsPolicy {
    /// The policy for `table`, on its `workspace_id` column.
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_owned(),
            column: "workspace_id".to_owned(),
            organizations: false,
            partition_column: None,
        }
    }

    /// Uses `column` instead of `workspace_id`.
    pub fn column(mut self, column: &str) -> Self {
        self.column = column.to_owned();
        self
    }

    /// Also lets organization requests read (`SELECT` only) the rows of
    /// their organization's workspaces (`navree.organization`).
    pub fn organizations(mut self) -> Self {
        self.organizations = true;
        self
    }

    /// Limits rows to the request's partition (`navree.partition`, on the
    /// `partition_id` column) whenever one is set.
    pub fn partitioned(self) -> Self {
        self.partition_column("partition_id")
    }

    /// [`partitioned`](RlsPolicy::partitioned) on `column`.
    pub fn partition_column(mut self, column: &str) -> Self {
        self.partition_column = Some(column.to_owned());
        self
    }

    /// The policy name: `<table>_workspace_isolation`.
    pub fn name(&self) -> String {
        format!("{}_workspace_isolation", self.table)
    }

    /// The name of the read-only policy that
    /// [`organizations`](RlsPolicy::organizations) adds:
    /// `<table>_organization_read`.
    pub fn organization_name(&self) -> String {
        format!("{}_organization_read", self.table)
    }

    /// Enables and forces row-level security on the table and creates the
    /// policy: `<table>_workspace_isolation` for every command and, with
    /// [`organizations`](RlsPolicy::organizations), `<table>_organization_read`
    /// for `SELECT` only, so an organization request reads its workspaces'
    /// rows but can't update or delete them.
    pub fn create_sql(&self) -> String {
        let table = quote(&self.table);
        let column = quote(&self.column);
        let partition = self.partition_column.as_ref().map(|partition| {
            format!(
                "(NULLIF(current_setting('navree.partition', true), '') IS NULL OR {} = NULLIF(current_setting('navree.partition', true), '')::uuid)",
                quote(partition)
            )
        });
        let narrowed = |condition: String| match &partition {
            Some(partition) => format!("({condition}) AND {partition}"),
            None => condition,
        };
        let workspace = narrowed(format!(
            "{column} = NULLIF(current_setting('navree.workspace', true), '')::uuid"
        ));
        let mut sql = format!(
            "ALTER TABLE {table} ENABLE ROW LEVEL SECURITY;\n\
             ALTER TABLE {table} FORCE ROW LEVEL SECURITY;\n\
             CREATE POLICY {name} ON {table}\n    USING ({workspace})\n    WITH CHECK ({workspace});",
            name = quote(&self.name()),
        );
        if self.organizations {
            let organization = narrowed(format!(
                "{column} IN (SELECT \"id\" FROM \"workspaces\" WHERE \"organization_id\" = NULLIF(current_setting('navree.organization', true), '')::uuid)"
            ));
            sql.push_str(&format!(
                "\nCREATE POLICY {name} ON {table} FOR SELECT\n    USING ({organization});",
                name = quote(&self.organization_name()),
            ));
        }
        sql
    }

    /// Drops the policy (both, with
    /// [`organizations`](RlsPolicy::organizations)) and turns row-level
    /// security off again.
    pub fn drop_sql(&self) -> String {
        let table = quote(&self.table);
        let mut sql = String::new();
        if self.organizations {
            sql.push_str(&format!(
                "DROP POLICY IF EXISTS {name} ON {table};\n",
                name = quote(&self.organization_name()),
            ));
        }
        sql.push_str(&format!(
            "DROP POLICY IF EXISTS {name} ON {table};\n\
             ALTER TABLE {table} NO FORCE ROW LEVEL SECURITY;\n\
             ALTER TABLE {table} DISABLE ROW LEVEL SECURITY;",
            name = quote(&self.name()),
        ));
        sql
    }
}

/// [`RlsPolicy::new(table).create_sql()`](RlsPolicy::create_sql): the
/// row-level security policy SQL for `table`'s `workspace_id` column.
pub fn rls_policy_sql(table: &str) -> String {
    RlsPolicy::new(table).create_sql()
}

/// `"name"` with embedded quotes doubled.
fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Applies an isolation strategy to units of work.
pub(crate) struct IsolationDriver {
    pub(crate) isolation: Isolation,
    schema_prefix: String,
    pub(crate) databases: Arc<WorkspaceDatabases>,
    app: WeakAppContext,
}

impl IsolationDriver {
    pub(crate) fn new(
        isolation: Isolation,
        schema_prefix: String,
        databases: Arc<WorkspaceDatabases>,
        app: WeakAppContext,
    ) -> Self {
        Self {
            isolation,
            schema_prefix,
            databases,
            app,
        }
    }

    /// The schema of `workspace` under schema isolation.
    pub(crate) fn schema_name(&self, workspace: Uuid) -> String {
        format!("{}{}", self.schema_prefix, workspace.simple())
    }

    /// Enters `workspace` on `uow` (a unit of work on the app's database):
    /// records it on every database (`Uow::workspace`, which scopes
    /// workspace-scoped ORM models), and on PostgreSQL sets the
    /// transaction-local `navree.workspace` (row-level security) and, under
    /// schema isolation, the `search_path`, beginning the transaction.
    pub(crate) async fn apply(&self, uow: &Uow, workspace: Uuid) -> Result<(), WorkspaceError> {
        uow.set_workspace(Some(workspace));
        if uow.backend() != Backend::Postgres {
            return Ok(());
        }
        uow.set_local("navree.workspace", &workspace.to_string())
            .await?;
        if self.isolation == Isolation::Schema {
            let path = format!("{}, public", quote(&self.schema_name(workspace)));
            uow.set_local("search_path", &path).await?;
        }
        Ok(())
    }

    /// Sets the transaction-local setting `name` to `value` on `uow` (a
    /// unit of work on the app's database) when it runs on PostgreSQL.
    /// Begins the transaction. Nothing on other databases.
    pub(crate) async fn apply_setting(
        &self,
        uow: &Uow,
        name: &'static str,
        value: Uuid,
    ) -> Result<(), WorkspaceError> {
        if uow.backend() != Backend::Postgres {
            return Ok(());
        }
        uow.set_local(name, &value.to_string()).await?;
        Ok(())
    }

    /// Prepares a new workspace inside `uow`: under schema isolation,
    /// creates its schema and runs the workspace migrations in it.
    pub(crate) async fn created(&self, uow: &Uow, workspace: Uuid) -> Result<(), WorkspaceError> {
        if self.isolation != Isolation::Schema {
            return Ok(());
        }
        let schema = quote(&self.schema_name(workspace));
        uow.execute_script(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .await?;
        let previous: String = uow
            .fetch_optional(&Statement::text(
                "SELECT current_setting('search_path') AS path",
            ))
            .await?
            .map(|row| row.get::<String>("path"))
            .transpose()?
            .unwrap_or_else(|| "public".to_owned());
        uow.set_local("search_path", &format!("{schema}, public"))
            .await?;
        if let Some(app) = self.app.upgrade() {
            migrate_in(uow, &workspace_sets(&app)).await?;
        }
        uow.set_local("search_path", &previous).await?;
        Ok(())
    }

    /// Cleans up after a workspace is deleted in `uow`: under schema
    /// isolation drops its schema; under database isolation closes its pool
    /// once `uow` commits (the database is left in place).
    pub(crate) async fn deleted(&self, uow: &Uow, workspace: Uuid) -> Result<(), WorkspaceError> {
        match self.isolation {
            Isolation::Row => {}
            Isolation::Schema => {
                let schema = quote(&self.schema_name(workspace));
                uow.execute_script(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
                    .await?;
            }
            Isolation::Database => {
                let databases = self.databases.clone();
                uow.after_commit(move || {
                    Box::pin(async move { databases.forget(workspace).await })
                });
            }
        }
        Ok(())
    }

    /// Runs the pending workspace migrations of `workspace` (schema or
    /// database isolation); returns `module/name` of each.
    pub(crate) async fn migrate(
        &self,
        db: &navree_db::Db,
        workspace: Uuid,
    ) -> Result<Vec<String>, WorkspaceError> {
        match self.isolation {
            Isolation::Row => Ok(Vec::new()),
            Isolation::Schema => {
                let uow = db.uow(None, None).await;
                let result = async {
                    let schema = quote(&self.schema_name(workspace));
                    uow.execute_script(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
                        .await?;
                    uow.set_local("search_path", &format!("{schema}, public"))
                        .await?;
                    let sets = match self.app.upgrade() {
                        Some(app) => workspace_sets(&app),
                        None => Vec::new(),
                    };
                    migrate_in(&uow, &sets).await
                }
                .await;
                match result {
                    Ok(ran) => {
                        uow.commit().await?;
                        Ok(ran)
                    }
                    Err(error) => {
                        uow.rollback().await?;
                        Err(error)
                    }
                }
            }
            Isolation::Database => {
                let (tenant, ran) = self.databases.open(workspace).await?;
                if !ran.is_empty() {
                    return Ok(ran);
                }
                let sets = match self.app.upgrade() {
                    Some(app) => workspace_sets(&app),
                    None => Vec::new(),
                };
                crate::tenant::migrate_db(&tenant, &sets).await
            }
        }
    }
}

impl fmt::Debug for IsolationDriver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IsolationDriver")
            .field("isolation", &self.isolation)
            .field("schema_prefix", &self.schema_prefix)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rls_policy_sql_scopes_by_the_workspace_setting() {
        let sql = rls_policy_sql("deals");
        assert_eq!(
            sql,
            "ALTER TABLE \"deals\" ENABLE ROW LEVEL SECURITY;\n\
             ALTER TABLE \"deals\" FORCE ROW LEVEL SECURITY;\n\
             CREATE POLICY \"deals_workspace_isolation\" ON \"deals\"\n    \
             USING (\"workspace_id\" = NULLIF(current_setting('navree.workspace', true), '')::uuid)\n    \
             WITH CHECK (\"workspace_id\" = NULLIF(current_setting('navree.workspace', true), '')::uuid);"
        );
        let policy = RlsPolicy::new("we\"ird").column("tenant_id");
        assert!(policy.create_sql().contains("\"we\"\"ird\""));
        assert!(policy.create_sql().contains("\"tenant_id\" = "));
        assert_eq!(
            policy.drop_sql(),
            "DROP POLICY IF EXISTS \"we\"\"ird_workspace_isolation\" ON \"we\"\"ird\";\n\
             ALTER TABLE \"we\"\"ird\" NO FORCE ROW LEVEL SECURITY;\n\
             ALTER TABLE \"we\"\"ird\" DISABLE ROW LEVEL SECURITY;"
        );
    }

    #[test]
    fn organization_and_partition_options_extend_the_policy() {
        let workspace =
            "\"workspace_id\" = NULLIF(current_setting('navree.workspace', true), '')::uuid";
        let organization = "\"workspace_id\" IN (SELECT \"id\" FROM \"workspaces\" WHERE \"organization_id\" = NULLIF(current_setting('navree.organization', true), '')::uuid)";
        let partition = "(NULLIF(current_setting('navree.partition', true), '') IS NULL OR \"partition_id\" = NULLIF(current_setting('navree.partition', true), '')::uuid)";

        // Organizations read through a second, SELECT-only policy: the
        // workspace policy alone decides inserts, updates and deletes.
        let orgs = RlsPolicy::new("deals").organizations();
        assert_eq!(
            orgs.create_sql(),
            format!(
                "ALTER TABLE \"deals\" ENABLE ROW LEVEL SECURITY;\n\
                 ALTER TABLE \"deals\" FORCE ROW LEVEL SECURITY;\n\
                 CREATE POLICY \"deals_workspace_isolation\" ON \"deals\"\n    \
                 USING ({workspace})\n    \
                 WITH CHECK ({workspace});\n\
                 CREATE POLICY \"deals_organization_read\" ON \"deals\" FOR SELECT\n    \
                 USING ({organization});"
            )
        );
        assert_eq!(orgs.organization_name(), "deals_organization_read");
        assert_eq!(
            orgs.drop_sql(),
            "DROP POLICY IF EXISTS \"deals_organization_read\" ON \"deals\";\n\
             DROP POLICY IF EXISTS \"deals_workspace_isolation\" ON \"deals\";\n\
             ALTER TABLE \"deals\" NO FORCE ROW LEVEL SECURITY;\n\
             ALTER TABLE \"deals\" DISABLE ROW LEVEL SECURITY;"
        );

        let parts = RlsPolicy::new("deals").partitioned().create_sql();
        assert!(parts.contains(&format!("USING (({workspace}) AND {partition})")));
        assert!(parts.contains(&format!("WITH CHECK (({workspace}) AND {partition});")));
        assert!(!parts.contains("FOR SELECT"));

        // Both: the partition narrows each policy.
        let both = RlsPolicy::new("deals")
            .organizations()
            .partition_column("domain_id")
            .create_sql();
        let domain = partition.replace("\"partition_id\"", "\"domain_id\"");
        assert!(both.contains(&format!("USING (({workspace}) AND {domain})")));
        assert!(both.contains(&format!(
            "FOR SELECT\n    USING (({organization}) AND {domain});"
        )));
        assert_eq!(
            RlsPolicy::new("deals").partitioned().name(),
            "deals_workspace_isolation"
        );
    }
}
