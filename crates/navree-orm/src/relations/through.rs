//! Relations through an intermediate model: [`HasOneThrough`] and
//! [`HasManyThrough`] (a country's posts through its users), and their
//! composition from two relations ([`PendingThrough`]).

use std::any::TypeId;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::collection::Collection;
use crate::column::Filter;
use crate::error::Result;
use crate::exec::{Access, Executor, Target};
use crate::ir::{ColumnRef, Expr, Operator, QueryIr, SelectItem, Where};
use crate::model::{Model, ModelMeta, snake_case};
use crate::query::Builder;
use crate::record::Record;
use crate::relations::belongs_to::{BelongsTo, DefaultModel};
use crate::relations::core::{
    Bound, RelationCore, column_value, column_values, constrain, distribute, qualified,
    relation_builder_traits, relation_query_methods, relation_retrieval_methods,
};
use crate::relations::define::KeyColumn;
use crate::relations::eager::{EagerNode, EagerTree, RelationParts, constrained, prepare_query};
use crate::relations::existence::{ExistsRequest, build_subquery, columns_equal};
use crate::relations::has::{HasMany, HasOne, drop_row_numbers, limit_per_parent};
use crate::relations::keys::{RelationKey, distinct_keys, in_keys, key_of};
use crate::relations::one_of_many::OneOfMany;
use crate::relations::registry::{ErasedRelation, RelationKind, RelationMeta};
use crate::relations::{Relation, RelatedTo, private};
use crate::value::{Value, is_null};

/// The column the intermediate model's first key is read back as.
const THROUGH_KEY: &str = "navree_through_key";

/// The keys of a through relation.
#[derive(Clone, Debug)]
pub(crate) struct ThroughDef {
    /// The intermediate model.
    pub(crate) through: &'static ModelMeta,
    /// On the intermediate model: holds the far parent's local key.
    pub(crate) first_key: Cow<'static, str>,
    /// On the related model: holds the intermediate model's second local
    /// key.
    pub(crate) second_key: Cow<'static, str>,
    /// On the far parent (its key by default).
    pub(crate) local_key: Cow<'static, str>,
    /// On the intermediate model (its key by default).
    pub(crate) second_local_key: Cow<'static, str>,
    /// Include soft-deleted intermediate records.
    pub(crate) with_trashed_parents: bool,
    /// Conditions of composed polymorphic relations (on the intermediate
    /// or the related table).
    pub(crate) conditions: Vec<Where>,
}

impl ThroughDef {
    fn new<P: Model>(
        through: &'static ModelMeta,
        first_key: Cow<'static, str>,
        second_key: Cow<'static, str>,
    ) -> Self {
        ThroughDef {
            through,
            first_key,
            second_key,
            local_key: Cow::Borrowed(P::meta().key_column()),
            second_local_key: Cow::Borrowed(through.key_column()),
            with_trashed_parents: false,
            conditions: Vec::new(),
        }
    }

    /// `query` joined with the intermediate table (named `through_alias`
    /// when it is aliased), without soft-deleted intermediate records
    /// unless they are included, plus the composed conditions.
    fn joined<R: Model>(&self, query: Builder<R>, through_alias: Option<&str>) -> Builder<R> {
        let related_qualifier = query.ir.table.qualifier().to_owned();
        let table = match through_alias {
            Some(alias) => format!("{} as {alias}", self.through.table()),
            None => self.through.table().to_owned(),
        };
        let through = through_alias.unwrap_or(self.through.table()).to_owned();
        let mut query = query.join(
            &table,
            qualified(&through, &self.second_local_key),
            "=",
            qualified(&related_qualifier, &self.second_key),
        );
        if !self.with_trashed_parents
            && let Some(deleted_at) = self.through.deleted_at()
        {
            query = query.where_(Filter::from(Where::Null {
                expr: Expr::Column(qualified(&through, deleted_at)),
                not: false,
            }));
        }
        for condition in &self.conditions {
            query = query.where_(Filter::from(condition.clone()));
        }
        query
    }

    /// `query` selecting the intermediate first key as the through key.
    fn with_through_key<R: Model>(&self, query: Builder<R>) -> Builder<R> {
        query.add_select([SelectItem {
            expr: Expr::Column(qualified(self.through.table(), &self.first_key)),
            alias: Some(Cow::Borrowed(THROUGH_KEY)),
        }])
    }

    /// The conditions of the related records of a parent with local key
    /// `value`.
    fn bound_conditions(&self, value: Value) -> Vec<Where> {
        vec![Where::Compare {
            left: Expr::Column(qualified(self.through.table(), &self.first_key)),
            op: Operator::Eq,
            right: Expr::Value(value),
        }]
    }

    /// The existence subquery.
    fn exists<P: Model, R: Model>(
        &self,
        definition: &Builder<R>,
        request: ExistsRequest,
        relation: &str,
        one_of_many: Option<&OneOfMany<R>>,
    ) -> Result<QueryIr> {
        let parent_table = P::meta().table();
        let related_table = R::meta().table();
        let through_table = self.through.table();
        let aliased = if related_table == parent_table {
            Some(related_table)
        } else if through_table == parent_table {
            Some(through_table)
        } else {
            None
        };
        let local = qualified(parent_table, &self.local_key);
        let first_key = self.first_key.clone();
        let through_aliased = through_table == parent_table && related_table != parent_table;
        let group_by = vec![qualified(through_table, &self.first_key)];
        build_subquery::<R>(
            P::meta(),
            definition,
            request,
            relation,
            aliased,
            |query, alias| {
                let query = self.joined(query, if through_aliased { alias } else { None });
                match one_of_many {
                    Some(one) => one.apply(query, relation, &group_by, Vec::new()),
                    None => query,
                }
            },
            |alias| {
                let through = if through_aliased {
                    alias.unwrap_or(through_table)
                } else {
                    through_table
                };
                vec![columns_equal(local, qualified(through, &first_key))]
            },
        )
    }

    /// The related records of `parents`, grouped by the parent key they
    /// belong to.
    async fn load<P: Model, R: Model>(
        &self,
        target: &Target<'_>,
        parents: &[Record<P>],
        query: Builder<R>,
        node: &EagerNode,
        relation: &str,
        one_of_many: Option<&OneOfMany<R>>,
    ) -> Result<(Vec<Option<RelationKey>>, HashMap<RelationKey, Vec<Record<R>>>)> {
        let values = column_values(parents, &self.local_key, relation)?;
        let parent_keys: Vec<Option<RelationKey>> = values.iter().map(key_of).collect();
        let keys = distinct_keys(values);
        let mut groups: HashMap<RelationKey, Vec<Record<R>>> = HashMap::new();
        if keys.is_empty() {
            return Ok((parent_keys, groups));
        }
        let query = prepare_query(query, node, R::meta().table(), &[])?;
        let query = self.with_through_key(self.joined(query, None));
        let first = qualified(self.through.table(), &self.first_key);
        let conditions = vec![in_keys(first.clone(), &keys)];
        let query = match one_of_many {
            Some(one) => one.apply(query, relation, std::slice::from_ref(&first), conditions),
            None => constrain(query, conditions),
        };
        let query = limit_per_parent(query, Expr::Column(first));
        let mut records = query.fetch(target).await?.into_vec();
        drop_row_numbers(&mut records);
        if node.autoload {
            crate::relations::lazy::group_records(records.iter_mut());
        }
        for mut record in records {
            let key = take_through_key(&mut record);
            if let Some(key) = key {
                groups.entry(key).or_default().push(record);
            }
        }
        Ok((parent_keys, groups))
    }
}

/// Removes the through key from `record`'s extras and returns it.
fn take_through_key<R: Model>(record: &mut Record<R>) -> Option<RelationKey> {
    let index = record
        .extras
        .iter()
        .position(|(column, _)| column == THROUGH_KEY)?;
    let (_, value) = record.extras.remove(index);
    key_of(&value)
}

/// The through-relation configuration methods (`self.def`).
macro_rules! through_methods {
    () => {
        /// The far parent's column the intermediate first key holds, when
        /// it is not its key.
        pub fn local_key(mut self, column: impl KeyColumn<P>) -> Self {
            self.def.local_key = column.key_column();
            self
        }

        /// The intermediate model's column the related second key holds,
        /// when it is not its key.
        pub fn second_local_key(mut self, column: impl Into<Cow<'static, str>>) -> Self {
            self.def.second_local_key = column.into();
            self
        }

        /// Includes related records reached through soft-deleted
        /// intermediate records (left out by default).
        pub fn with_trashed_parents(mut self) -> Self {
            self.def.with_trashed_parents = true;
            self
        }

        /// The intermediate model's column holding the far parent's key.
        pub fn first_key_name(&self) -> &str {
            &self.def.first_key
        }

        /// The first key, qualified by the intermediate table.
        pub fn qualified_first_key_name(&self) -> String {
            format!("{}.{}", self.def.through.table(), self.def.first_key)
        }

        /// The related model's column holding the intermediate key.
        pub fn foreign_key_name(&self) -> &str {
            &self.def.second_key
        }

        /// The second key, qualified by the related table.
        pub fn qualified_foreign_key_name(&self) -> String {
            format!("{}.{}", R::meta().table(), self.def.second_key)
        }

        /// The far parent's column the first key holds.
        pub fn local_key_name(&self) -> &str {
            &self.def.local_key
        }

        /// The intermediate model's column the second key holds.
        pub fn second_local_key_name(&self) -> &str {
            &self.def.second_local_key
        }

        /// The intermediate model's table.
        pub fn through_table(&self) -> &'static str {
            self.def.through.table()
        }

        /// The value of the parent's local key (`None` when the relation was
        /// not made from a record).
        pub fn parent_key(&self) -> Option<Value> {
            let parent = self.core.parent.as_ref()?;
            column_value(parent, &self.def.local_key, &self.core.name).ok()
        }

        /// The keys of every related record.
        pub async fn all_related_ids<E: Executor + ?Sized>(self, exec: &E) -> Result<Vec<Value>> {
            let key = qualified(R::meta().table(), R::meta().key_column());
            self.pluck(exec, Expr::Column(key)).await
        }

        /// Sets the update timestamp of every related record to now (no
        /// hooks run).
        pub async fn touch<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
            let target = exec.target(Access::Write).await?;
            let ids = self.all_related_ids(exec).await?;
            if ids.is_empty() {
                return Ok(0);
            }
            let keys = distinct_keys(ids);
            let query = Builder::<R>::new()
                .without_global_scopes()
                .where_(Filter::from(in_keys(
                    qualified(R::meta().table(), R::meta().key_column()),
                    &keys,
                )));
            crate::relations::touch::touch_related(Some(query), &target).await
        }

        /// The relation's query for its parent.
        pub(crate) fn bound(self) -> Result<Bound<R>> {
            let parent = self.core.parent()?.clone();
            let value = column_value(&parent, &self.def.local_key, &self.core.name)?;
            let query = self.def.with_through_key(self.def.joined(self.core.query.clone(), None));
            if is_null(&value) {
                return Ok(Bound::nothing(query));
            }
            let conditions = self.def.bound_conditions(value);
            let query = match self.one_of_many() {
                Some(one) => one.apply(
                    query,
                    &self.core.name,
                    &[qualified(self.def.through.table(), &self.def.first_key)],
                    conditions,
                ),
                None => constrain(query, conditions),
            };
            let finish: crate::relations::core::Finisher<R> = Box::new(|records: &mut [Record<R>]| {
                for record in records {
                    take_through_key(record);
                }
                Ok(())
            });
            Ok(Bound::new(query).finishing(Some(finish)))
        }
    };
}

/// The record of `R` reached through an intermediate model: a supplier's
/// account history through its user. Loaded, it holds an
/// `Option<Record<R>>`.
pub struct HasOneThrough<P: Model, R: Model> {
    pub(crate) core: RelationCore<P, R>,
    pub(crate) def: ThroughDef,
    pub(crate) default: Option<DefaultModel<P, R>>,
    pub(crate) one_of_many: Option<OneOfMany<R>>,
}

/// The records of `R` reached through an intermediate model: a country's
/// posts through its users. Loaded, it holds a [`Collection<R>`].
pub struct HasManyThrough<P: Model, R: Model> {
    pub(crate) core: RelationCore<P, R>,
    pub(crate) def: ThroughDef,
}

impl<P: Model, R: Model> Clone for HasOneThrough<P, R> {
    fn clone(&self) -> Self {
        HasOneThrough {
            core: self.core.clone(),
            def: self.def.clone(),
            default: self.default.clone(),
            one_of_many: self.one_of_many.clone(),
        }
    }
}

impl<P: Model, R: Model> Clone for HasManyThrough<P, R> {
    fn clone(&self) -> Self {
        HasManyThrough {
            core: self.core.clone(),
            def: self.def.clone(),
        }
    }
}

impl<P: Model, R: Model> fmt::Debug for HasOneThrough<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HasOneThrough")
            .field("parent", &P::meta().name())
            .field("through", &self.def.through.name())
            .field("related", &R::meta().name())
            .field("name", &self.core.name)
            .finish_non_exhaustive()
    }
}

impl<P: Model, R: Model> fmt::Debug for HasManyThrough<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HasManyThrough")
            .field("parent", &P::meta().name())
            .field("through", &self.def.through.name())
            .field("related", &R::meta().name())
            .field("name", &self.core.name)
            .finish_non_exhaustive()
    }
}

relation_builder_traits!(HasOneThrough);
relation_builder_traits!(HasManyThrough);

impl<P: Model, R: Model> RelationParts<P, R> for HasOneThrough<P, R> {
    fn core_mut(&mut self) -> &mut RelationCore<P, R> {
        &mut self.core
    }
}

impl<P: Model, R: Model> RelationParts<P, R> for HasManyThrough<P, R> {
    fn core_mut(&mut self) -> &mut RelationCore<P, R> {
        &mut self.core
    }
}

impl<P: Model, R: Model> HasOneThrough<P, R> {
    /// The relation of `parent` through `through`.
    pub(crate) fn new(
        parent: Option<P>,
        through: &'static ModelMeta,
        first_key: Cow<'static, str>,
        second_key: Cow<'static, str>,
    ) -> Self {
        HasOneThrough {
            core: RelationCore::new(parent, Cow::Owned(snake_case(R::meta().name()))),
            def: ThroughDef::new::<P>(through, first_key, second_key),
            default: None,
            one_of_many: None,
        }
    }

    relation_query_methods!();
    relation_retrieval_methods!();
    through_methods!();

    fn one_of_many(&self) -> Option<&OneOfMany<R>> {
        self.one_of_many.as_ref()
    }

    /// When nothing is related, the relation holds a new, unsaved `R`.
    pub fn with_default(mut self) -> Self
    where
        R: Default,
    {
        self.default = Some(Arc::new(|_| R::default()));
        self
    }

    /// When nothing is related, the relation holds a copy of `model`.
    pub fn with_default_value(mut self, model: R) -> Self {
        self.default = Some(Arc::new(move |_| model.clone()));
        self
    }

    /// When nothing is related, the relation holds what `make` makes from
    /// the parent.
    pub fn with_default_fn(mut self, make: impl Fn(&P) -> R + Send + Sync + 'static) -> Self {
        self.default = Some(Arc::new(make));
        self
    }

    /// Picks the record with the largest key.
    pub fn latest_of_many(mut self) -> Self {
        self.one_of_many = Some(OneOfMany::new(vec![(
            Cow::Borrowed(R::meta().key_column()),
            crate::ir::AggregateFunction::Max,
        )]));
        self
    }

    /// Picks the record with the smallest key.
    pub fn oldest_of_many(mut self) -> Self {
        self.one_of_many = Some(OneOfMany::new(vec![(
            Cow::Borrowed(R::meta().key_column()),
            crate::ir::AggregateFunction::Min,
        )]));
        self
    }

    /// Picks the record with the largest `column`.
    pub fn latest_of_many_by(mut self, column: impl KeyColumn<R>) -> Self {
        self.one_of_many = Some(OneOfMany::new(vec![(
            column.key_column(),
            crate::ir::AggregateFunction::Max,
        )]));
        self
    }

    /// Picks the record with the smallest `column`.
    pub fn oldest_of_many_by(mut self, column: impl KeyColumn<R>) -> Self {
        self.one_of_many = Some(OneOfMany::new(vec![(
            column.key_column(),
            crate::ir::AggregateFunction::Min,
        )]));
        self
    }

    /// Whether `record` is the related record (a query).
    pub async fn is<E: Executor + ?Sized>(&self, exec: &E, record: &Record<R>) -> Result<bool> {
        if self.parent_key().as_ref().is_none_or(is_null) {
            return Ok(false);
        }
        self.clone().where_key(record.key()).exists(exec).await
    }

    /// Whether `record` is not the related record.
    pub async fn is_not<E: Executor + ?Sized>(&self, exec: &E, record: &Record<R>) -> Result<bool> {
        Ok(!self.is(exec, record).await?)
    }

    /// What the relation holds once loaded: the related record, or the
    /// default (or `None`).
    pub async fn results<E: Executor + ?Sized>(self, exec: &E) -> Result<Option<Record<R>>> {
        let parent = self.core.parent()?.clone();
        let default = self.default.clone();
        let found = self.first(exec).await?;
        Ok(found.or_else(|| default.map(|make| Record::new(make(&parent)))))
    }
}

impl<P: Model, R: Model> HasManyThrough<P, R> {
    /// The relation of `parent` through `through`.
    pub(crate) fn new(
        parent: Option<P>,
        through: &'static ModelMeta,
        first_key: Cow<'static, str>,
        second_key: Cow<'static, str>,
    ) -> Self {
        HasManyThrough {
            core: RelationCore::new(parent, Cow::Borrowed(R::meta().table())),
            def: ThroughDef::new::<P>(through, first_key, second_key),
        }
    }

    relation_query_methods!();
    relation_retrieval_methods!();
    through_methods!();

    fn one_of_many(&self) -> Option<&OneOfMany<R>> {
        None
    }

    /// The same relation as a `has_one_through`.
    pub fn one(self) -> HasOneThrough<P, R> {
        HasOneThrough {
            core: self.core,
            def: self.def,
            default: None,
            one_of_many: None,
        }
    }

    /// The record with the largest key, as a `has_one_through`.
    pub fn latest_of_many(self) -> HasOneThrough<P, R> {
        self.one().latest_of_many()
    }

    /// The record with the smallest key, as a `has_one_through`.
    pub fn oldest_of_many(self) -> HasOneThrough<P, R> {
        self.one().oldest_of_many()
    }

    /// What the relation holds once loaded: the related records.
    pub async fn results<E: Executor + ?Sized>(self, exec: &E) -> Result<Collection<R>> {
        self.get(exec).await
    }
}

impl<P: Model, R: Model> private::Erase<P> for HasOneThrough<P, R> {
    fn erase(self) -> Arc<dyn ErasedRelation<P>> {
        let mut unbound = self;
        unbound.core = unbound.core.unbound();
        Arc::new(unbound)
    }

    fn rename(&mut self, name: Cow<'static, str>) {
        self.core.name = name;
    }
}

impl<P: Model, R: Model> private::Erase<P> for HasManyThrough<P, R> {
    fn erase(self) -> Arc<dyn ErasedRelation<P>> {
        let mut unbound = self;
        unbound.core = unbound.core.unbound();
        Arc::new(unbound)
    }

    fn rename(&mut self, name: Cow<'static, str>) {
        self.core.name = name;
    }
}

fn through_meta<P: Model, R: Model>(name: &str, kind: RelationKind, def: &ThroughDef) -> RelationMeta {
    let mut meta = RelationMeta::new::<P>(name, kind)
        .related::<R>()
        .keys(&def.first_key, &def.local_key);
    meta.through = Some(def.through.name());
    meta.related_pivot_key = Some(def.second_key.to_string());
    meta
}

impl<P: Model, R: Model> Relation<P> for HasOneThrough<P, R> {
    type Value = Option<Record<R>>;

    fn name(&self) -> &str {
        &self.core.name
    }

    fn kind(&self) -> RelationKind {
        RelationKind::HasOneThrough
    }

    fn meta(&self) -> RelationMeta {
        through_meta::<P, R>(&self.core.name, RelationKind::HasOneThrough, &self.def)
    }
}

impl<P: Model, R: Model> Relation<P> for HasManyThrough<P, R> {
    type Value = Collection<R>;

    fn name(&self) -> &str {
        &self.core.name
    }

    fn kind(&self) -> RelationKind {
        RelationKind::HasManyThrough
    }

    fn meta(&self) -> RelationMeta {
        through_meta::<P, R>(&self.core.name, RelationKind::HasManyThrough, &self.def)
    }
}

impl<P: Model, R: Model> RelatedTo<P> for HasOneThrough<P, R> {
    type Related = R;
}

impl<P: Model, R: Model> RelatedTo<P> for HasManyThrough<P, R> {
    type Related = R;
}

impl<P: Model, R: Model> ErasedRelation<P> for HasOneThrough<P, R> {
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
            let relation = constrained(self.clone(), node)?;
            let (parent_keys, groups) = relation
                .def
                .load(
                    target,
                    parents,
                    relation.core.query.clone(),
                    node,
                    &self.core.name,
                    relation.one_of_many.as_ref(),
                )
                .await?;
            let firsts: HashMap<RelationKey, Record<R>> = groups
                .into_iter()
                .filter_map(|(key, records)| records.into_iter().next().map(|record| (key, record)))
                .collect();
            for (parent, value) in parents.iter_mut().zip(distribute(firsts, &parent_keys)) {
                let value = value.or_else(|| {
                    self.default
                        .as_ref()
                        .map(|make| Record::new(make(&parent.model)))
                });
                parent.relations.set_relation(node.name.clone(), value);
            }
            Ok(())
        })
    }

    fn exists_query(&self, request: ExistsRequest) -> Result<QueryIr> {
        self.def
            .exists::<P, R>(&self.core.query, request, &self.core.name, self.one_of_many.as_ref())
    }

    fn touch<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parent: &'a Record<P>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut relation = self.clone();
            relation.core.parent = Some(parent.model.clone());
            let bound = relation.without_global_scopes().bound()?;
            if !bound.empty {
                let ids: Vec<Value> = bound
                    .query
                    .pluck(target, Expr::Column(qualified(R::meta().table(), R::meta().key_column())))
                    .await?;
                touch_ids::<R>(target, ids).await?;
            }
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

impl<P: Model, R: Model> ErasedRelation<P> for HasManyThrough<P, R> {
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
            let relation = constrained(self.clone(), node)?;
            let (parent_keys, groups) = relation
                .def
                .load(target, parents, relation.core.query.clone(), node, &self.core.name, None)
                .await?;
            for (parent, value) in parents.iter_mut().zip(distribute(groups, &parent_keys)) {
                parent.relations.set_relation(
                    node.name.clone(),
                    Collection::from(value.unwrap_or_default()),
                );
            }
            Ok(())
        })
    }

    fn exists_query(&self, request: ExistsRequest) -> Result<QueryIr> {
        self.def
            .exists::<P, R>(&self.core.query, request, &self.core.name, None)
    }

    fn touch<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parent: &'a Record<P>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut relation = self.clone();
            relation.core.parent = Some(parent.model.clone());
            let bound = relation.without_global_scopes().bound()?;
            if !bound.empty {
                let ids: Vec<Value> = bound
                    .query
                    .pluck(target, Expr::Column(qualified(R::meta().table(), R::meta().key_column())))
                    .await?;
                touch_ids::<R>(target, ids).await?;
            }
            Ok(())
        })
    }

    fn load_nested_missing<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        rest: &'a EagerTree,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(crate::relations::lazy::load_nested_many::<P, R>(
            target,
            parents,
            &self.core.name,
            rest,
        ))
    }
}

/// Sets the update timestamp of the records of `R` with keys `ids`.
async fn touch_ids<R: Model>(target: &Target<'_>, ids: Vec<Value>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let keys = distinct_keys(ids);
    let query = Builder::<R>::new().without_global_scopes().where_(Filter::from(in_keys(
        qualified(R::meta().table(), R::meta().key_column()),
        &keys,
    )));
    crate::relations::touch::touch_related(Some(query), target).await?;
    Ok(())
}

/// The cardinality of a relation in a composed through relation.
pub trait Cardinality {
    /// The cardinality of two relations in a row: one when both are one.
    type And<C: Cardinality>: Cardinality;
}

/// A relation to one record.
pub struct One;

/// A relation to several records.
pub struct Many;

impl Cardinality for One {
    type And<C: Cardinality> = C;
}

impl Cardinality for Many {
    type And<C: Cardinality> = Many;
}

/// The keys of one step of a composed through relation.
pub struct Leg {
    /// The column on the step's parent.
    parent_column: Cow<'static, str>,
    /// The column on the step's related model.
    child_column: Cow<'static, str>,
    /// A polymorphic step's condition on its related table.
    condition: Option<Where>,
}

/// A relation usable as a step of a composed through relation: `has_one`,
/// `has_many` (and their polymorphic forms) and `belongs_to`.
pub trait ThroughLeg<A: Model> {
    /// The model the step leads to.
    type Next: Model;
    /// Whether the step leads to one record or several.
    type Card: Cardinality;
    /// The step's keys.
    #[doc(hidden)]
    fn leg(&self) -> Leg;
}

impl<A: Model, B: Model> ThroughLeg<A> for HasOne<A, B> {
    type Next = B;
    type Card = One;

    fn leg(&self) -> Leg {
        has_leg::<A, B>(&self.local_key, &self.foreign_key, self.morph.as_deref())
    }
}

impl<A: Model, B: Model> ThroughLeg<A> for HasMany<A, B> {
    type Next = B;
    type Card = Many;

    fn leg(&self) -> Leg {
        has_leg::<A, B>(&self.local_key, &self.foreign_key, self.morph.as_deref())
    }
}

impl<A: Model, B: Model> ThroughLeg<A> for BelongsTo<A, B> {
    type Next = B;
    type Card = One;

    fn leg(&self) -> Leg {
        Leg {
            parent_column: self.foreign_key.clone(),
            child_column: self.owner_key.clone(),
            condition: None,
        }
    }
}

fn has_leg<A: Model, B: Model>(local: &Cow<'static, str>, foreign: &Cow<'static, str>, morph: Option<&str>) -> Leg {
    Leg {
        parent_column: local.clone(),
        child_column: foreign.clone(),
        condition: morph.map(|column| Where::Compare {
            left: Expr::Column(ColumnRef::new(Some(B::meta().table()), column.to_owned())),
            op: Operator::Eq,
            right: Expr::Value(Value::from(A::morph_name().into_owned())),
        }),
    }
}

/// The through relation a cardinality makes: `has_one_through` for one,
/// `has_many_through` for many.
pub trait ThroughOf<P: Model, R: Model> {
    /// The relation.
    type Relation;
    #[doc(hidden)]
    fn make(parent: P, through: &'static ModelMeta, local: Leg, distant: Leg) -> Self::Relation;
}

fn compose<P: Model>(through: &'static ModelMeta, local: Leg, distant: Leg) -> ThroughDef {
    let mut def = ThroughDef::new::<P>(through, local.child_column, distant.child_column);
    def.local_key = local.parent_column;
    def.second_local_key = distant.parent_column;
    def.conditions = local.condition.into_iter().chain(distant.condition).collect();
    def
}

impl<P: Model, R: Model> ThroughOf<P, R> for One {
    type Relation = HasOneThrough<P, R>;

    fn make(parent: P, through: &'static ModelMeta, local: Leg, distant: Leg) -> HasOneThrough<P, R> {
        let mut relation = HasOneThrough::new(Some(parent), through, Cow::Borrowed(""), Cow::Borrowed(""));
        relation.def = compose::<P>(through, local, distant);
        relation
    }
}

impl<P: Model, R: Model> ThroughOf<P, R> for Many {
    type Relation = HasManyThrough<P, R>;

    fn make(parent: P, through: &'static ModelMeta, local: Leg, distant: Leg) -> HasManyThrough<P, R> {
        let mut relation = HasManyThrough::new(Some(parent), through, Cow::Borrowed(""), Cow::Borrowed(""));
        relation.def = compose::<P>(through, local, distant);
        relation
    }
}

/// A through relation being composed: `self.through(Deal::company)`, to
/// finish with [`has`](PendingThrough::has).
pub struct PendingThrough<P: Model, L> {
    parent: P,
    local: L,
}

impl<P: Model, L> PendingThrough<P, L> {
    pub(crate) fn new(parent: P, local: L) -> Self {
        PendingThrough { parent, local }
    }
}

impl<P: Model, L: ThroughLeg<P>> PendingThrough<P, L> {
    /// The relation from the parent to the records `distant` reaches from
    /// the intermediate model: a `has_one_through` when both steps lead to
    /// one record, a `has_many_through` otherwise.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::relations::HasManyThrough;
    ///
    /// // A company's activities, through its deals.
    /// # fn demo(company: &Company) -> HasManyThrough<Company, Activity> {
    /// company.through(Company::deals).has(Deal::activities)
    /// # }
    /// ```
    pub fn has<D, F>(
        self,
        distant: F,
    ) -> <<L::Card as Cardinality>::And<D::Card> as ThroughOf<P, D::Next>>::Relation
    where
        F: Fn(&L::Next) -> D,
        D: ThroughLeg<L::Next>,
        L::Next: Default,
        <L::Card as Cardinality>::And<D::Card>: ThroughOf<P, D::Next>,
    {
        let distant = distant(&L::Next::default());
        <<L::Card as Cardinality>::And<D::Card> as ThroughOf<P, D::Next>>::make(
            self.parent,
            <L::Next as Model>::meta(),
            self.local.leg(),
            distant.leg(),
        )
    }
}

impl<P: Model, L: fmt::Debug> fmt::Debug for PendingThrough<P, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingThrough")
            .field("parent", &P::meta().name())
            .field("local", &self.local)
            .finish()
    }
}
