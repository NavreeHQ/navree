//! Isolation suites for workspace modules: [`WorkspaceTenancy`] creates the
//! two workspaces, and [`IsolationSuiteExt::factory`] binds a path
//! parameter to a model factory.
//!
//! ```ignore
//! use navree::testing::TestApp;
//! use navree::workspace::testing::IsolationSuiteExt;
//!
//! #[tokio::test]
//! async fn deals_stay_in_their_workspace() {
//!     let app = app().await;
//!     app.isolation_suite::<DealsModule>()
//!         .factory(Deal::factory()) // binds {id}; uses workspaces
//!         .body("POST", "/api/v1/deals", json!({ "title": "Renewal" }))
//!         .run()
//!         .await;
//! }
//! ```

use std::sync::Arc;

use navree_db::Db;
use navree_kernel::{BoxError, BoxFuture};
use navree_orm::factory::{Factory, ModelFactory};
use navree_orm::{Model, PrimaryKey, Value};
use navree_testing::{IsolationSuite, RecordContext, Tenancy, Tenant, TestApp};
use uuid::Uuid;

use crate::{
    Isolation, NewWorkspace, ResolverKind, WorkspaceConfig, WorkspaceDatabases, WorkspaceRepo,
    WorkspaceScoped, Workspaces,
};

/// The [`Tenancy`] of workspaces: each tenant is a new workspace owned by
/// the suite's member, and requests name it in the `workspace.header`
/// header (`X-Workspace: <id>`), so the app's `workspace.resolvers` must
/// include `header` (the default). Create routes are sent with the other
/// workspace's id in `workspace_id`.
#[derive(Clone, Debug)]
pub struct WorkspaceTenancy {
    owner_field: String,
}

impl WorkspaceTenancy {
    /// Workspaces, with `workspace_id` as the owner field.
    pub fn new() -> Self {
        Self {
            owner_field: "workspace_id".to_owned(),
        }
    }

    /// Uses `field` as the body field that names a workspace (a model
    /// whose `WorkspaceScoped::WORKSPACE_COLUMN` is not `workspace_id`).
    pub fn owner_field(mut self, field: &str) -> Self {
        self.owner_field = field.to_owned();
        self
    }
}

impl Default for WorkspaceTenancy {
    fn default() -> Self {
        Self::new()
    }
}

impl Tenancy for WorkspaceTenancy {
    fn create<'a>(
        &'a self,
        app: &'a TestApp,
        name: &'a str,
        member: Uuid,
    ) -> BoxFuture<'a, Result<Tenant, BoxError>> {
        Box::pin(async move {
            let ctx = app.ctx();
            let config = ctx.get::<WorkspaceConfig>().await?;
            if !config.resolvers.contains(&ResolverKind::Header) {
                return Err(format!(
                    "isolation suites name the workspace in the `{}` header, but workspace.resolvers has no `header`: add it in the test app",
                    config.header
                )
                .into());
            }
            let workspaces = ctx.get::<Workspaces>().await?;
            let db = ctx.get::<Db>().await?;
            let uow = db.uow(Some(member), None).await;
            let created = uow
                .scope(workspaces.create(&uow, member, NewWorkspace::named(name)))
                .await;
            let workspace = match created {
                Ok(workspace) => {
                    uow.commit().await?;
                    workspace
                }
                Err(error) => {
                    uow.rollback().await?;
                    return Err(error.into());
                }
            };
            let id = workspace.id.to_string();
            Ok(Tenant::new(id.clone(), workspace.name).header(&config.header, &id))
        })
    }

    fn owner_field(&self) -> Option<&str> {
        Some(&self.owner_field)
    }

    fn noun(&self) -> &str {
        "workspace"
    }
}

/// Workspace bindings for [`IsolationSuite`]: import the trait, then call
/// `.factory(..)` on `app.isolation_suite::<M>()`.
pub trait IsolationSuiteExt: Sized {
    /// Binds `{id}` to records made by `factory` (a navree-orm factory
    /// whose model is [`WorkspaceScoped`]), inserted into the suite's
    /// workspaces, and uses [`WorkspaceTenancy`] unless another tenancy is
    /// set. Under row isolation the record goes through
    /// [`WorkspaceRepo::insert`]; under schema and database isolation it is
    /// created in the workspace's own schema or database, with the
    /// workspace column set when the model has one.
    fn factory<F>(self, factory: Factory<F>) -> Self
    where
        F: ModelFactory,
        F::Model: WorkspaceScoped,
    {
        self.factory_for("id", factory)
    }

    /// Like [`factory`](Self::factory), for path parameter `param`
    /// (`deal` in `/deals/{deal}/notes/{note}`).
    fn factory_for<F>(self, param: &str, factory: Factory<F>) -> Self
    where
        F: ModelFactory,
        F::Model: WorkspaceScoped;

    /// Uses [`WorkspaceTenancy`].
    fn workspaces(self) -> Self;
}

impl IsolationSuiteExt for IsolationSuite<'_> {
    fn factory_for<F>(self, param: &str, factory: Factory<F>) -> Self
    where
        F: ModelFactory,
        F::Model: WorkspaceScoped,
    {
        let factory = Arc::new(factory);
        let suite = if self.has_tenancy() {
            self
        } else {
            self.workspaces()
        };
        suite.record(param, move |cx| insert(factory.clone(), cx))
    }

    fn workspaces(self) -> Self {
        self.tenancy(WorkspaceTenancy::new())
    }
}

/// Creates `factory`'s next record in the workspace of `cx`; returns its
/// key as text.
async fn insert<F>(factory: Arc<Factory<F>>, cx: RecordContext) -> Result<String, BoxError>
where
    F: ModelFactory,
    F::Model: WorkspaceScoped,
{
    let workspace = Uuid::parse_str(&cx.tenant().id)?;
    let app = cx.app();
    let config = app.get::<WorkspaceConfig>().await?;
    let model = (*factory).clone().make_one()?.into_inner();
    let uow = match config.isolation {
        Isolation::Database => {
            let db = app.get::<WorkspaceDatabases>().await?.db(workspace).await?;
            db.uow(None, None).await
        }
        Isolation::Row | Isolation::Schema => app.get::<Db>().await?.uow(None, None).await,
    };
    let inserted = async {
        if config.isolation != Isolation::Database {
            // Row-level security and the schema's search_path.
            app.get::<Workspaces>()
                .await?
                .enter(&uow, workspace)
                .await?;
        }
        let record = match config.isolation {
            Isolation::Row => WorkspaceRepo::<F::Model>::insert(&uow, workspace, model).await?,
            _ => {
                // A table in the workspace's own schema or database may still
                // keep the workspace column (a policy, a foreign key).
                let mut model = model;
                if F::Model::meta()
                    .column(<F::Model as WorkspaceScoped>::WORKSPACE_COLUMN)
                    .is_some()
                {
                    model.set_value(
                        <F::Model as WorkspaceScoped>::WORKSPACE_COLUMN,
                        Value::from(workspace),
                    )?;
                }
                navree_orm::workspace::scope(workspace, F::Model::create(&uow, model)).await?
            }
        };
        Ok::<_, BoxError>(record)
    }
    .await;
    match inserted {
        Ok(record) => {
            uow.commit().await?;
            Ok(record.key().to_key_string())
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error)
        }
    }
}
