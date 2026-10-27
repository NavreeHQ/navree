//! [`ModelLifecycle`]: the lifecycle's model-level methods, for every
//! model.

use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;

use crate::column::Assignment;
use crate::error::Result;
use crate::events::without_events;
use crate::exec::Executor;
use crate::model::Model;
use crate::record::Record;
use crate::scope::GlobalScope;
use crate::soft_deletes::SoftDeleting;

/// The lifecycle's methods on models themselves (`Deal::create_quietly`,
/// `Note::force_destroy`, ...), implemented for every [`Model`]. Bring it
/// into scope with `use navree_orm::prelude::*;`.
pub trait ModelLifecycle: Model {
    /// Inserts `model` and returns its record without model events
    /// ([`Model::create`] inside [`without_events`]).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow) -> Result<()> {
    /// // An import: nobody is notified of each row.
    /// let tag = Tag::create_quietly(&uow, Tag { name: "vip".into(), ..Default::default() }).await?;
    /// # Ok(()) }
    /// ```
    fn create_quietly<E: Executor + ?Sized>(
        exec: &E,
        model: Self,
    ) -> impl Future<Output = Result<Record<Self>>> + Send {
        without_events(Self::create(exec, model))
    }

    /// Removes the records with these keys for good, soft-deleted ones
    /// included, loading each so its force-delete events fire; returns how
    /// many were removed. On a model without soft deletes it is
    /// [`destroy`](Model::destroy).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow) -> Result<()> {
    /// let removed = Note::force_destroy(&uow, [3_i64, 4, 5]).await?;
    /// # Ok(()) }
    /// ```
    fn force_destroy<E: Executor + ?Sized>(
        exec: &E,
        keys: impl IntoIterator<Item = impl Into<Self::Key>>,
    ) -> impl Future<Output = Result<u64>> + Send {
        let keys: Vec<Self::Key> = keys.into_iter().map(Into::into).collect();
        async move {
            if keys.is_empty() {
                return Ok(0);
            }
            let records = Self::query()
                .with_trashed()
                .where_key_in(keys)
                .get(exec)
                .await?;
            let mut removed = 0;
            for mut record in records {
                if record.force_delete(exec).await? {
                    removed += 1;
                }
            }
            Ok(removed)
        }
    }

    /// The record matching `attributes`, soft-deleted or not, restored when
    /// it was trashed; or a new one with them (and whatever `fill` sets)
    /// inserted. See [`Builder::restore_or_create`](crate::Builder::restore_or_create).
    fn restore_or_create<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut Self) + Send,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: SoftDeleting + Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        Self::query().restore_or_create(exec, attributes, fill)
    }

    /// Inserts a record with `attributes` (and whatever `fill` sets), or on
    /// a unique violation returns the record holding them, restored when it
    /// was trashed. See
    /// [`Builder::create_or_restore`](crate::Builder::create_or_restore).
    fn create_or_restore<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut Self) + Send,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: SoftDeleting + Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        Self::query().create_or_restore(exec, attributes, fill)
    }

    /// Whether the model has a global scope named `name` (declared,
    /// registered at boot, or `SoftDeletes`).
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// assert!(Note::has_global_scope("SoftDeletes"));
    /// assert!(!Deal::has_global_scope("SoftDeletes"));
    /// ```
    fn has_global_scope(name: &str) -> bool {
        crate::global_scopes::find::<Self>(name).is_some()
    }

    /// The global scope named `name`, if the model has one.
    fn global_scope(name: &str) -> Option<Arc<dyn GlobalScope<Self>>> {
        crate::global_scopes::find::<Self>(name)
    }

    /// The names of every global scope the model's queries apply, in
    /// order.
    fn global_scope_names() -> Vec<Cow<'static, str>> {
        crate::global_scopes::scopes_of::<Self>()
            .iter()
            .map(|scope| scope.name())
            .collect()
    }
}

impl<M: Model> ModelLifecycle for M {}
