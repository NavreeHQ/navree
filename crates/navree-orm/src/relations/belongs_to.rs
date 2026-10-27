//! [`BelongsTo`]: the record a foreign key on the parent points at.

use std::any::TypeId;
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::error::Result;
use crate::exec::{Access, Executor, Target};
use crate::ir::{Expr, Operator, QueryIr, Where};
use crate::model::{Model, snake_case};
use crate::query::Builder;
use crate::record::Record;
use crate::relations::core::{
    Bound, RelationCore, column_value, column_values, constrain, qualified,
    relation_builder_traits, relation_query_methods, relation_retrieval_methods,
};
use crate::relations::define::KeyColumn;
use crate::relations::eager::{EagerNode, EagerTree, RelationParts, constrained, prepare_query};
use crate::relations::existence::{ExistsRequest, build_subquery, columns_equal};
use crate::relations::keys::{RelationKey, distinct_keys, in_keys_on, key_of};
use crate::relations::registry::{ErasedRelation, RelationKind, RelationMeta};
use crate::relations::{RelatedTo, Relation, private};
use crate::value::{Value, is_null};

/// Makes a default related record for a parent.
pub(crate) type DefaultModel<P, R> = Arc<dyn Fn(&P) -> R + Send + Sync>;

/// The record of `R` a foreign key on the parent points at: a deal's
/// company (`deals.company_id` → `companies.id`). Loaded, it holds an
/// `Option<Record<R>>`.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, deal: Record<Deal>) -> Result<()> {
/// // #[relations] impl Deal {
/// //     fn company(&self) -> BelongsTo<Deal, Company> { self.belongs_to(Deal::COMPANY_ID) }
/// // }
/// let company: Option<Record<Company>> = deal.company().first(&db).await?;
/// let active = deal.company().where_(Company::ACTIVE.eq(true)).exists(&db).await?;
/// # Ok(()) }
/// ```
pub struct BelongsTo<P: Model, R: Model> {
    pub(crate) core: RelationCore<P, R>,
    pub(crate) foreign_key: Cow<'static, str>,
    pub(crate) owner_key: Cow<'static, str>,
    pub(crate) default: Option<DefaultModel<P, R>>,
}

impl<P: Model, R: Model> Clone for BelongsTo<P, R> {
    fn clone(&self) -> Self {
        BelongsTo {
            core: self.core.clone(),
            foreign_key: self.foreign_key.clone(),
            owner_key: self.owner_key.clone(),
            default: self.default.clone(),
        }
    }
}

impl<P: Model, R: Model> fmt::Debug for BelongsTo<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BelongsTo")
            .field("parent", &P::meta().name())
            .field("related", &R::meta().name())
            .field("name", &self.core.name)
            .field("foreign_key", &self.foreign_key)
            .field("owner_key", &self.owner_key)
            .finish_non_exhaustive()
    }
}

relation_builder_traits!(BelongsTo);

impl<P: Model, R: Model> RelationParts<P, R> for BelongsTo<P, R> {
    fn core_mut(&mut self) -> &mut RelationCore<P, R> {
        &mut self.core
    }
}

impl<P: Model, R: Model> BelongsTo<P, R> {
    /// The relation of `parent` through its column `foreign_key`, named
    /// after `R` (`company`).
    pub(crate) fn new(parent: Option<P>, foreign_key: Cow<'static, str>) -> Self {
        BelongsTo {
            core: RelationCore::new(parent, Cow::Owned(snake_case(R::meta().name()))),
            foreign_key,
            owner_key: Cow::Borrowed(R::meta().key_column()),
            default: None,
        }
    }

    /// The column of `R` the foreign key points at, when it is not `R`'s
    /// key.
    pub fn owner_key(mut self, column: impl KeyColumn<R>) -> Self {
        self.owner_key = column.key_column();
        self
    }

    /// When nothing is related, the relation holds a new, unsaved `R`
    /// (its `Default`) instead of `None`.
    pub fn with_default(mut self) -> Self
    where
        R: Default,
    {
        self.default = Some(Arc::new(|_| R::default()));
        self
    }

    /// When nothing is related, the relation holds a new, unsaved copy of
    /// `model`.
    pub fn with_default_value(mut self, model: R) -> Self {
        self.default = Some(Arc::new(move |_| model.clone()));
        self
    }

    /// When nothing is related, the relation holds a new, unsaved record
    /// made by `make` from the parent.
    pub fn with_default_fn(mut self, make: impl Fn(&P) -> R + Send + Sync + 'static) -> Self {
        self.default = Some(Arc::new(make));
        self
    }

    relation_query_methods!();
    relation_retrieval_methods!();

    /// The foreign key column (on the parent).
    pub fn foreign_key_name(&self) -> &str {
        &self.foreign_key
    }

    /// The foreign key column, qualified by the parent's table.
    pub fn qualified_foreign_key_name(&self) -> String {
        format!("{}.{}", P::meta().table(), self.foreign_key)
    }

    /// The column of `R` the foreign key points at.
    pub fn owner_key_name(&self) -> &str {
        &self.owner_key
    }

    /// The owner key column, qualified by `R`'s table.
    pub fn qualified_owner_key_name(&self) -> String {
        format!("{}.{}", R::meta().table(), self.owner_key)
    }

    /// The value of the parent's foreign key (`None` when the relation was
    /// not made from a record).
    pub fn parent_key(&self) -> Option<Value> {
        let parent = self.core.parent.as_ref()?;
        column_value(parent, &self.foreign_key, &self.core.name).ok()
    }

    /// Whether `record` is the record the parent's foreign key points at
    /// (no query: the keys are compared).
    pub fn is(&self, record: &Record<R>) -> bool {
        let (Some(parent_key), Ok(owner)) = (
            self.parent_key(),
            column_value(&record.model, &self.owner_key, &self.core.name),
        ) else {
            return false;
        };
        key_of(&parent_key).is_some() && key_of(&parent_key) == key_of(&owner)
    }

    /// Whether `record` is not the record the parent's foreign key points
    /// at.
    pub fn is_not(&self, record: &Record<R>) -> bool {
        !self.is(record)
    }

    /// What the relation holds once loaded: the related record, or the
    /// default (or `None`) when nothing is related.
    pub async fn results<E: Executor + ?Sized>(self, exec: &E) -> Result<Option<Record<R>>> {
        let default = self.default.clone();
        let parent = self.core.parent()?.clone();
        let found = self.first(exec).await?;
        Ok(found.or_else(|| default.map(|make| Record::new(make(&parent)))))
    }

    /// Sets the update timestamp of the related record to now (no hooks
    /// run). Nothing happens when the foreign key is NULL or `R` keeps no
    /// update timestamp.
    pub async fn touch<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        let target = exec.target(Access::Write).await?;
        let parent = Record::stored(self.core.parent()?.clone());
        crate::relations::touch::touch_related(target_query(&self, &parent)?, &target).await
    }

    /// The relation's query for its parent.
    pub(crate) fn bound(self) -> Result<Bound<R>> {
        let value = column_value(self.core.parent()?, &self.foreign_key, &self.core.name)?;
        if is_null(&value) {
            return Ok(Bound::nothing(self.core.query));
        }
        let owner = qualified(R::meta().table(), &self.owner_key);
        Ok(Bound::new(constrain(
            self.core.query,
            vec![Where::Compare {
                left: Expr::Column(owner),
                op: Operator::Eq,
                right: Expr::Value(value),
            }],
        )))
    }
}

/// The related query of `relation` for `parent`, without global scopes
/// (what touching updates); `None` when nothing is related.
fn target_query<P: Model, R: Model>(
    relation: &BelongsTo<P, R>,
    parent: &Record<P>,
) -> Result<Option<Builder<R>>> {
    let value = column_value(&parent.model, &relation.foreign_key, &relation.core.name)?;
    if is_null(&value) {
        return Ok(None);
    }
    Ok(Some(Builder::<R>::new().without_global_scopes().where_(
        crate::column::Filter::from(Where::Compare {
            left: Expr::Column(qualified(R::meta().table(), &relation.owner_key)),
            op: Operator::Eq,
            right: Expr::Value(value),
        }),
    )))
}

impl<P: Model, R: Model> private::Erase<P> for BelongsTo<P, R> {
    fn erase(self) -> Arc<dyn ErasedRelation<P>> {
        let mut unbound = self;
        unbound.core = unbound.core.unbound();
        Arc::new(unbound)
    }

    fn rename(&mut self, name: Cow<'static, str>) {
        self.core.name = name;
    }
}

impl<P: Model, R: Model> Relation<P> for BelongsTo<P, R> {
    type Value = Option<Record<R>>;

    fn name(&self) -> &str {
        &self.core.name
    }

    fn kind(&self) -> RelationKind {
        RelationKind::BelongsTo
    }

    fn meta(&self) -> RelationMeta {
        RelationMeta::new::<P>(&self.core.name, RelationKind::BelongsTo)
            .related::<R>()
            .keys(&self.foreign_key, &self.owner_key)
    }
}

impl<P: Model, R: Model> RelatedTo<P> for BelongsTo<P, R> {
    type Related = R;
}

impl<P: Model, R: Model> ErasedRelation<P> for BelongsTo<P, R> {
    fn name(&self) -> &str {
        &self.core.name
    }

    fn meta(&self) -> RelationMeta {
        Relation::meta(self)
    }

    fn related_type(&self) -> Option<TypeId> {
        Some(TypeId::of::<R>())
    }

    fn eager_load<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        node: &'a EagerNode,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let values = column_values(parents, &self.foreign_key, &self.core.name)?;
            let keys = distinct_keys(values.iter().cloned());
            let mut related: Vec<(RelationKey, Record<R>)> = Vec::new();
            if !keys.is_empty() {
                let relation = constrained(self.clone(), node)?;
                let table = R::meta().table();
                let query = prepare_query(relation.core.query, node, table, &[&self.owner_key])?;
                let query = constrain(
                    query,
                    vec![in_keys_on(target, qualified(table, &self.owner_key), &keys)],
                );
                for record in query.fetch(target).await? {
                    let owner = column_value(&record.model, &self.owner_key, &self.core.name)?;
                    if let Some(key) = key_of(&owner) {
                        related.push((key, record));
                    }
                }
                if node.autoload {
                    crate::relations::lazy::group_records(
                        related.iter_mut().map(|(_, record)| record),
                    );
                }
            }
            for (parent, value) in parents.iter_mut().zip(values) {
                let key = key_of(&value);
                let found = key.and_then(|key| {
                    related
                        .iter()
                        .find(|(owner, _)| *owner == key)
                        .map(|(_, record)| record.clone())
                });
                let found = found.or_else(|| {
                    self.default
                        .as_ref()
                        .map(|make| Record::new(make(&parent.model)))
                });
                parent.relations.set_relation(node.name.clone(), found);
            }
            Ok(())
        })
    }

    fn exists_query(&self, request: ExistsRequest) -> Result<QueryIr> {
        let parent_table = P::meta().table();
        let table = R::meta().table();
        let aliased = (table == parent_table).then_some(table);
        let foreign = qualified(parent_table, &self.foreign_key);
        let owner_key = self.owner_key.clone();
        build_subquery::<R>(
            P::meta(),
            &self.core.query,
            request,
            &self.core.name,
            aliased,
            |query, _| query,
            |alias| {
                vec![columns_equal(
                    foreign,
                    qualified(alias.unwrap_or(table), &owner_key),
                )]
            },
        )
    }

    fn touch<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parent: &'a Record<P>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            crate::relations::touch::touch_related(target_query(self, parent)?, target).await?;
            Ok(())
        })
    }

    fn load_nested_missing<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        rest: &'a EagerTree,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(crate::relations::lazy::load_nested_one::<P, R>(
            target,
            parents,
            &self.core.name,
            rest,
        ))
    }
}
