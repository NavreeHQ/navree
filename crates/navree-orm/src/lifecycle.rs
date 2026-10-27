//! [`ModelLifecycle`]: the lifecycle's model-level methods, for every
//! model.

use std::future::Future;

use crate::error::Result;
use crate::events::without_events;
use crate::exec::Executor;
use crate::model::Model;
use crate::record::Record;

/// The lifecycle's methods on models themselves (`Deal::create_quietly`,
/// `Deal::force_destroy`, ...), implemented for every [`Model`]. Bring it
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
}

impl<M: Model> ModelLifecycle for M {}
