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
//!         .factory(DealFactory::default()) // binds {id}; uses workspaces
//!         .body("POST", "/api/v1/deals", json!({ "title": "Renewal" }))
//!         .run()
//!         .await;
//! }
//! ```

use std::str::FromStr;
use std::sync::Arc;

use navree_db::{Db, Factory, Repo};
use navree_kernel::{BoxError, BoxFuture};
use navree_testing::{IsolationSuite, RecordContext, Tenancy, Tenant, TestApp};
use sea_orm::{
    ActiveModelTrait, EntityName, EntityTrait, IntoActiveModel, Iterable, ModelTrait,
    PrimaryKeyToColumn, Value,
};
use serde::Serialize;
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

    /// Uses `field` as the body field that names a workspace (an entity
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
    /// Binds `{id}` to records made by `factory` (a `navree_db::Factory`
    /// whose entity is [`WorkspaceScoped`]), inserted into the suite's
    /// workspaces, and uses [`WorkspaceTenancy`] unless another tenancy is
    /// set. Under row isolation the record goes through
    /// [`WorkspaceRepo::insert`]; under schema and database isolation it is
    /// inserted in the workspace's own schema or database, with the
    /// workspace column set when the entity has one.
    fn factory<F>(self, factory: F) -> Self
    where
        F: Factory + 'static,
        F::Entity: WorkspaceScoped,
        <F::Entity as EntityTrait>::Model:
            Serialize + IntoActiveModel<<F::Entity as EntityTrait>::ActiveModel> + Sync,
        <F::Entity as EntityTrait>::ActiveModel: Send + Sync,
    {
        self.factory_for("id", factory)
    }

    /// Like [`factory`](Self::factory), for path parameter `param`
    /// (`deal` in `/deals/{deal}/notes/{note}`).
    fn factory_for<F>(self, param: &str, factory: F) -> Self
    where
        F: Factory + 'static,
        F::Entity: WorkspaceScoped,
        <F::Entity as EntityTrait>::Model:
            Serialize + IntoActiveModel<<F::Entity as EntityTrait>::ActiveModel> + Sync,
        <F::Entity as EntityTrait>::ActiveModel: Send + Sync;

    /// Uses [`WorkspaceTenancy`].
    fn workspaces(self) -> Self;
}

impl IsolationSuiteExt for IsolationSuite<'_> {
    fn factory_for<F>(self, param: &str, factory: F) -> Self
    where
        F: Factory + 'static,
        F::Entity: WorkspaceScoped,
        <F::Entity as EntityTrait>::Model:
            Serialize + IntoActiveModel<<F::Entity as EntityTrait>::ActiveModel> + Sync,
        <F::Entity as EntityTrait>::ActiveModel: Send + Sync,
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

/// Inserts `factory`'s next record into the workspace of `cx`; returns its
/// primary key as text.
async fn insert<F>(factory: Arc<F>, cx: RecordContext) -> Result<String, BoxError>
where
    F: Factory,
    F::Entity: WorkspaceScoped,
    <F::Entity as EntityTrait>::Model:
        Serialize + IntoActiveModel<<F::Entity as EntityTrait>::ActiveModel> + Sync,
    <F::Entity as EntityTrait>::ActiveModel: Send + Sync,
{
    let workspace = Uuid::parse_str(&cx.tenant().id)?;
    let app = cx.app();
    let config = app.get::<WorkspaceConfig>().await?;
    let model = factory.definition(cx.n());
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
        let model = match config.isolation {
            Isolation::Row => WorkspaceRepo::<F::Entity>::insert(&uow, workspace, model).await?,
            _ => {
                // A table in the workspace's own schema or database may still
                // keep the workspace column (a policy, a foreign key).
                let mut model = model;
                let column = <F::Entity as EntityTrait>::Column::from_str(
                    <F::Entity as WorkspaceScoped>::WORKSPACE_COLUMN,
                )
                .ok();
                if let Some(column) = column {
                    model.set(column, workspace.into());
                }
                Repo::<F::Entity>::insert(&uow, model).await?
            }
        };
        Ok::<_, BoxError>(model)
    }
    .await;
    match inserted {
        Ok(model) => {
            uow.commit().await?;
            primary_key(&model).ok_or_else(|| {
                format!(
                    "the primary key of `{}` is not a single text, UUID or integer column",
                    F::Entity::default().table_name()
                )
                .into()
            })
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error)
        }
    }
}

/// A model's single-column primary key as the path shows it.
fn primary_key<M: ModelTrait>(model: &M) -> Option<String> {
    let mut keys = <<M::Entity as EntityTrait>::PrimaryKey as Iterable>::iter();
    let key = keys.next()?;
    if keys.next().is_some() {
        return None;
    }
    match model.get(key.into_column()) {
        Value::Uuid(Some(id)) => Some(id.to_string()),
        Value::String(Some(text)) => Some(text),
        Value::TinyInt(Some(n)) => Some(n.to_string()),
        Value::SmallInt(Some(n)) => Some(n.to_string()),
        Value::Int(Some(n)) => Some(n.to_string()),
        Value::BigInt(Some(n)) => Some(n.to_string()),
        Value::TinyUnsigned(Some(n)) => Some(n.to_string()),
        Value::SmallUnsigned(Some(n)) => Some(n.to_string()),
        Value::Unsigned(Some(n)) => Some(n.to_string()),
        Value::BigUnsigned(Some(n)) => Some(n.to_string()),
        _ => None,
    }
}
