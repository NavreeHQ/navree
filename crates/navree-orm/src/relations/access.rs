//! Eager loads limited to the records a viewer may see: [`AccessFilter`]
//! (what decides it) and [`Builder::with_accessible`].

use std::borrow::Cow;
use std::sync::Arc;

use crate::column::Filter;
use crate::error::Result;
use crate::model::{Model, ModelMeta};
use crate::query::Builder;
use crate::relations::eager::Constraint;
use crate::relations::path::{ParsedPath, RelationPath};

/// What a viewer may see of each model's records: the access layer's
/// abilities (their rules turned into a condition), a workspace, a rule
/// written by hand. [`Builder::with_accessible`] adds its condition to
/// every relation an eager load reads.
///
/// Closures `Fn(&str, &'static ModelMeta) -> Result<Option<Filter>>`
/// implement it, and so does `Arc<A>` of an implementation.
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::ModelMeta;
/// use navree_orm::relations::AccessFilter;
///
/// /// A viewer who sees only won deals, and every record of other models.
/// #[derive(Clone)]
/// struct WonOnly;
///
/// impl AccessFilter for WonOnly {
///     fn filter(&self, _action: &str, model: &'static ModelMeta) -> Result<Option<Filter>> {
///         Ok((model.name() == "Deal").then(|| Deal::STAGE.eq(Stage::Won)))
///     }
/// }
///
/// let companies = Company::query().with_accessible("deals", &WonOnly);
/// # let _ = companies;
/// ```
pub trait AccessFilter: Send + Sync + 'static {
    /// The condition keeping the records of `model` on which `action`
    /// (`"read"` for loads) is allowed: `None` keeps every record; a
    /// condition that matches nothing hides them all; an error refuses
    /// the load.
    fn filter(&self, action: &str, model: &'static ModelMeta) -> Result<Option<Filter>>;
}

impl<F> AccessFilter for F
where
    F: Fn(&str, &'static ModelMeta) -> Result<Option<Filter>> + Send + Sync + 'static,
{
    fn filter(&self, action: &str, model: &'static ModelMeta) -> Result<Option<Filter>> {
        self(action, model)
    }
}

impl<A: AccessFilter + ?Sized> AccessFilter for Arc<A> {
    fn filter(&self, action: &str, model: &'static ModelMeta) -> Result<Option<Filter>> {
        (**self).filter(action, model)
    }
}

/// An eager-load constraint asking `filter` which records of the related
/// model `action` may read.
#[derive(Clone)]
pub(crate) struct AccessConstraint {
    pub(crate) filter: Arc<dyn AccessFilter>,
    pub(crate) action: Cow<'static, str>,
}

impl AccessConstraint {
    /// `query` with the condition `filter` sets for `R` (none: unchanged).
    pub(crate) fn apply<R: Model>(&self, query: Builder<R>) -> Result<Builder<R>> {
        Ok(match self.filter.filter(&self.action, R::meta())? {
            Some(condition) => query.where_(condition),
            None => query,
        })
    }
}

impl<M: Model> Builder<M> {
    /// Eager loads relation `path` with only the records `access` lets the
    /// viewer read, at every step of the path (a deal's company, then the
    /// company's contacts): its condition for each related model is added
    /// to that relation's query.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::ModelMeta;
    ///
    /// # async fn demo(db: Db, viewer: i64) -> Result<()> {
    /// // Each viewer sees the deals they own.
    /// let mine = move |_action: &str, model: &'static ModelMeta| -> Result<Option<Filter>> {
    ///     Ok((model.name() == "Deal").then(|| Deal::OWNER_ID.eq(Some(viewer))))
    /// };
    /// let companies = Company::query().with_accessible("deals", &mine).get(&db).await?;
    /// # Ok(()) }
    /// ```
    pub fn with_accessible<A: AccessFilter + Clone>(
        self,
        path: impl RelationPath<M>,
        access: &A,
    ) -> Self {
        self.with_accessible_for(path, "read", access)
    }

    /// [`with_accessible`](Builder::with_accessible) for `action` instead
    /// of `"read"`.
    pub fn with_accessible_for<A: AccessFilter + Clone>(
        mut self,
        path: impl RelationPath<M>,
        action: impl Into<Cow<'static, str>>,
        access: &A,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let parsed = match ParsedPath::parse(&text) {
            Ok(parsed) => parsed,
            Err(error) => return self.with_error(error.to_string()),
        };
        let constraint = AccessConstraint {
            filter: Arc::new(access.clone()),
            action: action.into(),
        };
        for depth in 1..=parsed.segments.len() {
            let node = self.eager.tree.entry(&parsed.segments[..depth]);
            if depth == parsed.segments.len() && parsed.columns.is_some() {
                node.columns.clone_from(&parsed.columns);
            }
            node.constraints
                .push(Constraint::new_access(constraint.clone()));
        }
        self
    }
}
