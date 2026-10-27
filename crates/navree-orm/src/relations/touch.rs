//! Touching: a record whose model declares `#[model(touches = ["company"])]`
//! sets the update timestamp of the records of those relations when it is
//! saved with changes or deleted (a deal's company shows it changed when
//! the deal did), and theirs in turn. [`without_touching`] and
//! [`without_touching_on`] suspend it for a scope.

use std::any::TypeId;
use std::cell::RefCell;
use std::future::Future;

use navree_kernel::BoxFuture;

use crate::error::Result;
use crate::exec::Target;
use crate::model::{Model, PrimaryKey};
use crate::query::Builder;
use crate::record::Record;
use crate::relations::any::AnyRecord;
use crate::relations::registry;

/// Which models are not touched in the current task: every model, or the
/// listed ones.
#[derive(Clone, Debug, Default)]
struct Ignoring {
    all: bool,
    models: Vec<TypeId>,
}

tokio::task_local! {
    static IGNORING: Ignoring;
    static TOUCHING: RefCell<Vec<(TypeId, String)>>;
}

fn ignoring() -> Ignoring {
    IGNORING.try_with(Clone::clone).unwrap_or_default()
}

/// Runs `future` without touching any model's owners.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(uow: Uow, mut deal: Record<Deal>) -> Result<()> {
/// use navree_orm::relations::without_touching;
///
/// deal.title = "Renamed".into();
/// without_touching(deal.save(&uow)).await?; // the company is not touched
/// # Ok(()) }
/// ```
pub async fn without_touching<F: Future>(future: F) -> F::Output {
    let mut current = ignoring();
    current.all = true;
    IGNORING.scope(current, future).await
}

/// Runs `future` without touching the models of tuple `T`
/// (`without_touching_on::<(Company,)>(..)`).
pub async fn without_touching_on<T: TouchList, F: Future>(future: F) -> F::Output {
    let mut current = ignoring();
    current.models.extend(T::type_ids());
    IGNORING.scope(current, future).await
}

/// Whether model `M` is not touched now: it keeps no update timestamp, or
/// a [`without_touching`] scope covers it.
pub fn is_ignoring_touch<M: Model>() -> bool {
    is_ignoring_touch_of::<M>()
}

pub(crate) fn is_ignoring_touch_of<M: Model>() -> bool {
    if M::meta().updated_at().is_none() {
        return true;
    }
    let current = ignoring();
    current.all || current.models.contains(&TypeId::of::<M>())
}

/// Tuples of models, for [`without_touching_on`].
pub trait TouchList {
    /// The models' type ids.
    #[doc(hidden)]
    fn type_ids() -> Vec<TypeId>;
}

macro_rules! touch_list {
    ($($model:ident),+) => {
        impl<$($model: Model),+> TouchList for ($($model,)+) {
            fn type_ids() -> Vec<TypeId> {
                vec![$(TypeId::of::<$model>()),+]
            }
        }
    };
}

touch_list!(A);
touch_list!(A, B);
touch_list!(A, B, C);
touch_list!(A, B, C, D);
touch_list!(A, B, C, D, E);
touch_list!(A, B, C, D, E, F);

/// Touches the records of `record`'s touched relations (and, through
/// theirs, further up). Each record is touched once per save, so cycles
/// end.
pub(crate) fn touch_owners<'a, 'b, M: Model>(
    target: &'a Target<'b>,
    record: &'a Record<M>,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let touches = M::touched_relations();
        if touches.is_empty() {
            return Ok(());
        }
        if TOUCHING.try_with(|_| ()).is_err() {
            return TOUCHING
                .scope(
                    RefCell::new(Vec::new()),
                    touch_owners_in_scope(target, record),
                )
                .await;
        }
        touch_owners_in_scope(target, record).await
    })
}

fn touch_owners_in_scope<'a, 'b, M: Model>(
    target: &'a Target<'b>,
    record: &'a Record<M>,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let me = (TypeId::of::<M>(), record.key().to_key_string());
        let seen = TOUCHING
            .try_with(|visited| {
                let mut visited = visited.borrow_mut();
                if visited.contains(&me) {
                    true
                } else {
                    visited.push(me);
                    false
                }
            })
            .unwrap_or(false);
        if seen {
            return Ok(());
        }
        for name in M::touched_relations() {
            let relation = registry::find::<M>(name)?;
            relation.inner.touch(target, record).await?;
        }
        Ok(())
    })
}

/// Sets the update timestamp of the records `query` selects to now (one
/// statement, no hooks; nothing when `R` keeps no update timestamp or a
/// scope ignores it), then touches their own touched relations. Returns
/// how many rows changed.
pub(crate) async fn touch_related<R: Model>(
    query: Option<Builder<R>>,
    target: &Target<'_>,
) -> Result<u64> {
    let Some(query) = query else {
        return Ok(0);
    };
    if is_ignoring_touch_of::<R>() {
        return Ok(0);
    }
    let touched = query.clone().touch(target).await?;
    if touched > 0 && !R::touched_relations().is_empty() {
        let owners = query.fetch(target).await?;
        for owner in owners.iter() {
            touch_owners(target, owner).await?;
        }
    }
    Ok(touched)
}

/// Touches `record` (a `morph_to`'s related record): its update timestamp
/// set and saved, its own touched relations touched.
pub(crate) async fn touch_any(target: &Target<'_>, record: &mut AnyRecord) -> Result<()> {
    record.touch_on(target).await?;
    Ok(())
}
