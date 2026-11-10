//! Global scopes registered at runtime, the scopes a model's queries
//! apply, and removing them by name.
//!
//! A model's global scopes are, in the order they apply: the ones declared
//! on it (`#[model(global_scopes(..))]`), the ones modules registered at
//! boot (`m.global_scope::<Deal>("active", |q| ..)`), then
//! [`SoftDeletes`] for a soft-deleting model, then
//! [`WorkspaceScope`](crate::workspace::WorkspaceScope) for a
//! workspace-scoped one.
//!
//! Scopes apply where no executor is at hand (`to_sql`, subqueries), so the
//! runtime registrations are process-wide: `OrmModule` installs them when
//! the app boots (an app per process; a later registration of the same
//! name for the same model replaces the earlier one).

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use crate::model::Model;
use crate::query::Builder;
use crate::scope::{GlobalScope, SoftDeletes};

/// A scope registered at runtime: `Arc<dyn GlobalScope<M>>` for the model
/// it is keyed by, with its name.
struct Registered {
    name: Cow<'static, str>,
    scope: Box<dyn Any + Send + Sync>,
}

type Registry = RwLock<HashMap<TypeId, Vec<Registered>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::default)
}

/// Registers `scope` for every query of `M` (replacing a scope of the same
/// name).
pub(crate) fn register<M: Model>(scope: Arc<dyn GlobalScope<M>>) {
    let name = scope.name();
    let mut registry = registry().write().unwrap_or_else(PoisonError::into_inner);
    let scopes = registry.entry(TypeId::of::<M>()).or_default();
    scopes.retain(|registered| registered.name != name);
    scopes.push(Registered {
        name,
        scope: Box::new(scope),
    });
}

/// The scopes registered at runtime for `M`, in registration order.
fn registered<M: Model>() -> Vec<Arc<dyn GlobalScope<M>>> {
    let registry = registry().read().unwrap_or_else(PoisonError::into_inner);
    registry
        .get(&TypeId::of::<M>())
        .into_iter()
        .flatten()
        .filter_map(|registered| {
            registered
                .scope
                .downcast_ref::<Arc<dyn GlobalScope<M>>>()
                .cloned()
        })
        .collect()
}

/// Every global scope of `M`, in the order they apply: declared, registered
/// at runtime, then [`SoftDeletes`] for a soft-deleting model and
/// [`WorkspaceScope`](crate::workspace::WorkspaceScope) for a
/// workspace-scoped one.
pub(crate) fn scopes_of<M: Model>() -> Vec<Arc<dyn GlobalScope<M>>> {
    let mut scopes = M::global_scopes();
    scopes.extend(registered::<M>());
    if M::meta().soft_deletes_enabled() {
        scopes.push(Arc::new(SoftDeletes));
    }
    if M::meta().is_workspace_scoped() {
        scopes.push(Arc::new(crate::workspace::WorkspaceScope));
    }
    scopes
}

/// A global scope that is a named function of the builder:
/// what [`ModuleBuilderOrmExt::global_scope`](crate::ModuleBuilderOrmExt::global_scope)
/// registers.
///
/// ```
/// use navree_orm::{Builder, ClosureScope, GlobalScope, Model, col};
/// # use navree_orm::__doc_models::Company;
///
/// let active = ClosureScope::new("active", |q: Builder<Company>| q.where_(col("active").eq(true)));
/// assert_eq!(GlobalScope::<Company>::name(&active), "active");
/// ```
pub struct ClosureScope<M, F> {
    name: Cow<'static, str>,
    scope: F,
    _model: PhantomData<fn() -> M>,
}

impl<M, F> ClosureScope<M, F>
where
    M: Model,
    F: Fn(Builder<M>) -> Builder<M> + Send + Sync + 'static,
{
    /// The scope `scope`, named `name`.
    pub fn new(name: impl Into<Cow<'static, str>>, scope: F) -> Self {
        ClosureScope {
            name: name.into(),
            scope,
            _model: PhantomData,
        }
    }
}

impl<M, F> GlobalScope<M> for ClosureScope<M, F>
where
    M: Model,
    F: Fn(Builder<M>) -> Builder<M> + Send + Sync + 'static,
{
    fn apply(&self, query: Builder<M>) -> Builder<M> {
        (self.scope)(query)
    }

    fn name(&self) -> Cow<'static, str> {
        self.name.clone()
    }
}

impl<M: Model> Builder<M> {
    /// Leaves out the global scope named `name` for this query (a named
    /// closure scope, or a scope type's name such as `"Published"`).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let all = Company::query().without_global_scope_named("active").get(&db).await?;
    /// # Ok(()) }
    /// ```
    pub fn without_global_scope_named(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.scopes.names.push(name.into());
        self
    }

    /// Leaves out every global scope but the ones named in `keep`.
    pub fn without_global_scopes_except<N>(mut self, keep: impl IntoIterator<Item = N>) -> Self
    where
        N: Into<Cow<'static, str>>,
    {
        self.scopes.keep = Some(keep.into_iter().map(Into::into).collect());
        self
    }

    /// The names of the model's global scopes this query leaves out, in
    /// the order they would apply.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let query = Deal::query().without_global_scopes();
    /// assert!(query.removed_scopes().is_empty(), "Deal has no global scope");
    /// ```
    pub fn removed_scopes(&self) -> Vec<Cow<'static, str>> {
        scopes_of::<M>()
            .iter()
            .filter(|scope| self.scopes.removes(scope.as_ref()))
            .map(|scope| scope.name())
            .collect()
    }
}

/// The global scope of `M` named `name`, if it has one.
pub(crate) fn find<M: Model>(name: &str) -> Option<Arc<dyn GlobalScope<M>>> {
    scopes_of::<M>()
        .into_iter()
        .find(|scope| scope.name() == name)
}
