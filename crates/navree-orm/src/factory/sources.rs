//! Where related records come from: [`ParentSource`] (a factory, a lazy
//! factory or a stored record), [`IntoRecords`] (stored records to recycle
//! or attach), [`LazyFactory`] and the recycle pool.

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::Arc;

use navree_kernel::BoxFuture;
use tokio::sync::OnceCell;

use crate::collection::Collection;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::factory::{Factory, ModelFactory};
use crate::model::Model;
use crate::record::Record;
use crate::value::{Field, Value};

/// Stored records of model `P` a factory can use: one record, several, or
/// a collection. [`Factory::recycle`](crate::factory::Factory::recycle) and
/// [`Factory::has_attached`](crate::factory::Factory::has_attached) take
/// these.
pub trait IntoRecords<P: Model> {
    /// The records.
    fn into_records(self) -> Vec<Record<P>>;
}

impl<P: Model> IntoRecords<P> for Record<P> {
    fn into_records(self) -> Vec<Record<P>> {
        vec![self]
    }
}

impl<P: Model> IntoRecords<P> for &Record<P> {
    fn into_records(self) -> Vec<Record<P>> {
        vec![self.clone()]
    }
}

impl<P: Model> IntoRecords<P> for Collection<P> {
    fn into_records(self) -> Vec<Record<P>> {
        self.into_vec()
    }
}

impl<P: Model> IntoRecords<P> for &Collection<P> {
    fn into_records(self) -> Vec<Record<P>> {
        self.to_vec()
    }
}

impl<P: Model> IntoRecords<P> for Vec<Record<P>> {
    fn into_records(self) -> Vec<Record<P>> {
        self
    }
}

impl<P: Model> IntoRecords<P> for &[Record<P>] {
    fn into_records(self) -> Vec<Record<P>> {
        self.to_vec()
    }
}

/// Records kept by the factories of one `create` (or `make`) call to use
/// instead of creating new parents: model type → its records.
#[derive(Clone, Default)]
pub(crate) struct Recycle {
    pools: Vec<(TypeId, &'static str, Arc<dyn Any + Send + Sync>)>,
}

impl Recycle {
    /// Adds `records` to the pool of their model.
    pub(crate) fn add<P: Model>(&mut self, records: Vec<Record<P>>) {
        if records.is_empty() {
            return;
        }
        let type_id = TypeId::of::<P>();
        match self.pools.iter_mut().find(|(id, _, _)| *id == type_id) {
            Some((_, _, pool)) => {
                let mut merged: Vec<Record<P>> = pool
                    .downcast_ref::<Vec<Record<P>>>()
                    .cloned()
                    .unwrap_or_default();
                merged.extend(records);
                *pool = Arc::new(merged);
            }
            None => self
                .pools
                .push((type_id, P::meta().name(), Arc::new(records))),
        }
    }

    /// Adds every pool of `other` (the parent factory's pool reaches its
    /// nested factories).
    pub(crate) fn extend(&mut self, other: &Recycle) {
        for (type_id, name, pool) in &other.pools {
            if !self.pools.iter().any(|(id, _, _)| id == type_id) {
                self.pools.push((*type_id, name, pool.clone()));
            }
        }
    }

    /// The recycled records of `P`.
    pub(crate) fn records<P: Model>(&self) -> Option<&[Record<P>]> {
        self.pools
            .iter()
            .find(|(id, _, _)| *id == TypeId::of::<P>())
            .and_then(|(_, _, pool)| pool.downcast_ref::<Vec<Record<P>>>())
            .map(Vec::as_slice)
            .filter(|records| !records.is_empty())
    }

    /// One recycled record of `P` chosen with `pick` (a random number).
    pub(crate) fn pick<P: Model>(&self, pick: u64) -> Option<Record<P>> {
        let records = self.records::<P>()?;
        let index = (pick % records.len() as u64) as usize;
        Some(records[index].clone())
    }

    /// The names of the recycled models (for `Debug`).
    fn names(&self) -> Vec<&'static str> {
        self.pools.iter().map(|(_, name, _)| *name).collect()
    }
}

impl fmt::Debug for Recycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

/// A parent record for [`Factory::for_`](crate::factory::Factory::for_) and
/// [`FactoryContext::belongs_to`](crate::factory::FactoryContext::belongs_to):
/// a [`Factory`] (a parent is created, or recycled), a [`LazyFactory`] (one
/// parent created on first use and shared), or a stored [`Record`].
///
/// Implemented by those types only.
pub trait ParentSource: Send + Sync + sealed::Sealed {
    /// The parent's model.
    type Parent: Model;

    /// The source, type-erased (what factories keep).
    #[doc(hidden)]
    fn into_parent(self) -> ParentHandle;
}

mod sealed {
    /// Keeps [`ParentSource`](super::ParentSource) to the factory types.
    pub trait Sealed {}
}

impl<F: ModelFactory> sealed::Sealed for Factory<F> {}
impl<F: ModelFactory> sealed::Sealed for LazyFactory<F> {}
impl<P: Model> sealed::Sealed for Record<P> {}
impl<P: Model> sealed::Sealed for &Record<P> {}

/// A parent source, type-erased. Not API.
#[doc(hidden)]
pub struct ParentHandle(pub(crate) Arc<dyn ResolveParent>);

impl fmt::Debug for ParentHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ParentHandle")
            .field(&self.0.model_name())
            .finish()
    }
}

/// A source of one parent record, type-erased.
pub(crate) trait ResolveParent: Send + Sync {
    /// The parent model's name.
    fn model_name(&self) -> &'static str;

    /// The value of `column` (the key when `None`) of a parent known
    /// without the database: a stored record, a lazily created one that
    /// exists, or a recycled one (chosen with `pick`). Factories have none.
    fn known(&self, column: Option<&str>, recycle: &Recycle, pick: u64) -> Result<Option<Value>>;

    /// The value of `column` (the key when `None`) of the parent, created on
    /// `target` when needed (unless `expand` is false: then `None`).
    fn resolve<'a>(
        &'a self,
        column: Option<&'a str>,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
        pick: u64,
        expand: bool,
    ) -> BoxFuture<'a, Result<Option<Value>>>;
}

/// The value of `column` (the key when `None`) of `record`.
fn column_value<P: Model>(record: &Record<P>, column: Option<&str>) -> Result<Value> {
    match column {
        None => Ok(record.key().to_value()),
        Some(column) => record
            .model()
            .value(column)?
            .ok_or_else(|| crate::model::derive_support::unknown_column::<P>(column)),
    }
}

impl<F: ModelFactory> ParentSource for Factory<F> {
    type Parent = F::Model;

    fn into_parent(self) -> ParentHandle {
        ParentHandle(Arc::new(self))
    }
}

impl<F: ModelFactory> ResolveParent for Factory<F> {
    fn model_name(&self) -> &'static str {
        F::Model::meta().name()
    }

    fn known(&self, column: Option<&str>, recycle: &Recycle, pick: u64) -> Result<Option<Value>> {
        recycle
            .pick::<F::Model>(pick)
            .map(|record| column_value(&record, column))
            .transpose()
    }

    fn resolve<'a>(
        &'a self,
        column: Option<&'a str>,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
        pick: u64,
        expand: bool,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        Box::pin(async move {
            if let Some(record) = recycle.pick::<F::Model>(pick) {
                return column_value(&record, column).map(Some);
            }
            if !expand {
                return Ok(None);
            }
            let mut factory = self.clone().count(1);
            factory.recycle.extend(recycle);
            let created = factory.create_on(target).await?;
            let record = created.into_vec().into_iter().next().ok_or_else(|| {
                OrmError::factory(F::Model::meta().name(), "the parent factory created no record")
            })?;
            column_value(&record, column).map(Some)
        })
    }
}

impl<P: Model> ParentSource for Record<P> {
    type Parent = P;

    fn into_parent(self) -> ParentHandle {
        ParentHandle(Arc::new(StoredParent(self)))
    }
}

impl<P: Model> ParentSource for &Record<P> {
    type Parent = P;

    fn into_parent(self) -> ParentHandle {
        ParentHandle(Arc::new(StoredParent(self.clone())))
    }
}

/// A stored record as a parent.
struct StoredParent<P: Model>(Record<P>);

impl<P: Model> ResolveParent for StoredParent<P> {
    fn model_name(&self) -> &'static str {
        P::meta().name()
    }

    fn known(&self, column: Option<&str>, _recycle: &Recycle, _pick: u64) -> Result<Option<Value>> {
        column_value(&self.0, column).map(Some)
    }

    fn resolve<'a>(
        &'a self,
        column: Option<&'a str>,
        _target: &'a Target<'a>,
        _recycle: &'a Recycle,
        _pick: u64,
        _expand: bool,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        Box::pin(async move { column_value(&self.0, column).map(Some) })
    }
}

/// A factory whose record is created the first time it is needed, then
/// shared: give one to several definitions or `for_` calls and they all
/// point at the same parent. Made with [`Factory::lazy`](crate::factory::Factory::lazy).
///
/// ```no_run
/// # use navree_orm::__doc_factories::*;
/// # async fn demo(db: Db) -> Result<()> {
/// let acme = Company::factory().set(Company::NAME, "Acme").lazy();
/// // Three deals, one company (created with the first deal).
/// let deals = Deal::factory().count(3).for_(acme.clone(), Deal::COMPANY_ID).create(&db).await?;
/// let company = acme.get(&db).await?;
/// assert!(deals.iter().all(|deal| deal.company_id == Some(company.id)));
/// # Ok(()) }
/// ```
pub struct LazyFactory<F: ModelFactory> {
    factory: Factory<F>,
    record: Arc<OnceCell<Record<F::Model>>>,
}

impl<F: ModelFactory> LazyFactory<F> {
    pub(crate) fn new(factory: Factory<F>) -> Self {
        LazyFactory {
            factory: factory.count(1),
            record: Arc::new(OnceCell::new()),
        }
    }

    /// The record: created on `exec` by the first call, the same record
    /// afterwards.
    pub async fn get<E: Executor + ?Sized>(&self, exec: &E) -> Result<Record<F::Model>> {
        let target = exec.target(Access::Write).await?;
        self.get_on(&target, &Recycle::default()).await
    }

    /// The record, if it was created already.
    pub fn created(&self) -> Option<&Record<F::Model>> {
        self.record.get()
    }

    async fn get_on(&self, target: &Target<'_>, recycle: &Recycle) -> Result<Record<F::Model>> {
        self.record
            .get_or_try_init(|| async {
                let mut factory = self.factory.clone();
                factory.recycle.extend(recycle);
                let created = factory.create_on(target).await?;
                created.into_vec().into_iter().next().ok_or_else(|| {
                    OrmError::factory(F::Model::meta().name(), "the lazy factory created no record")
                })
            })
            .await
            .cloned()
    }
}

impl<F: ModelFactory> Clone for LazyFactory<F> {
    fn clone(&self) -> Self {
        LazyFactory {
            factory: self.factory.clone(),
            record: self.record.clone(),
        }
    }
}

impl<F: ModelFactory> fmt::Debug for LazyFactory<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyFactory")
            .field("model", &F::Model::meta().name())
            .field("created", &self.record.initialized())
            .finish()
    }
}

impl<F: ModelFactory> ParentSource for LazyFactory<F> {
    type Parent = F::Model;

    fn into_parent(self) -> ParentHandle {
        ParentHandle(Arc::new(self))
    }
}

impl<F: ModelFactory> ResolveParent for LazyFactory<F> {
    fn model_name(&self) -> &'static str {
        F::Model::meta().name()
    }

    fn known(&self, column: Option<&str>, _recycle: &Recycle, _pick: u64) -> Result<Option<Value>> {
        self.record
            .get()
            .map(|record| column_value(record, column))
            .transpose()
    }

    fn resolve<'a>(
        &'a self,
        column: Option<&'a str>,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
        _pick: u64,
        _expand: bool,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        Box::pin(async move {
            let record = self.get_on(target, recycle).await?;
            column_value(&record, column).map(Some)
        })
    }
}
