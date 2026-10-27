//! [`HasOne`] and [`HasMany`]: records of the related model that hold the
//! parent's key, and their polymorphic forms (`morph_one`, `morph_many`),
//! which also hold the parent's type.

use std::any::TypeId;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::collection::Collection;
use crate::column::{Assignment, Column, Filter};
use crate::error::Result;
use crate::exec::{Access, Executor, Target};
use crate::ir::{AggregateFunction, ColumnRef, Expr, Operator, Order, QueryIr, SelectItem, TableRef, Where};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::relations::belongs_to::DefaultModel;
use crate::relations::core::{
    Bound, Finisher, RelationCore, column_value, column_values, constrain, distribute, matching,
    new_with, qualified, relation_builder_traits, relation_query_methods,
    relation_retrieval_methods, restore_record, save_atomically,
};
use crate::relations::define::KeyColumn;
use crate::relations::eager::{EagerNode, EagerTree, RelationParts, constrained, prepare_query};
use crate::relations::existence::{ExistsRequest, build_subquery, columns_equal};
use crate::relations::keys::{RelationKey, distinct_keys, in_keys_on, key_of};
use crate::relations::morph::stored_type_of;
use crate::relations::one_of_many::OneOfMany;
use crate::relations::registry::{ErasedRelation, RelationKind, RelationMeta};
use crate::relations::{Relation, RelatedTo, private};
use crate::value::{Numeric, Value, is_null};

/// The column an eager load's per-parent row numbers are read from (and
/// removed).
pub(crate) const ROW_COLUMN: &str = "navree_row";

/// `query` with a limit or offset applying to each parent's records
/// (`partition`: what groups them), through `ROW_NUMBER() OVER (PARTITION
/// BY ..)`; unchanged without a limit or offset.
pub(crate) fn limit_per_parent<R: Model>(query: Builder<R>, partition: Expr) -> Builder<R> {
    if query.ir.limit.is_none() && query.ir.offset.is_none() {
        return query;
    }
    let mut query = query;
    let tree = query.take_eager();
    let limit = query.ir.limit.take();
    let offset = query.ir.offset.take().unwrap_or(0);
    let mut inner = query.into_ir();
    let orders = std::mem::take(&mut inner.orders);
    inner.columns.push(SelectItem {
        expr: Expr::RowNumber {
            partition: vec![partition],
            order: orders,
        },
        alias: Some(Cow::Borrowed(ROW_COLUMN)),
    });
    let mut outer = Builder::<R>::new();
    outer.scopes_applied = true;
    outer.ir.table = TableRef::subquery(inner, "navree_table");
    outer.ir.columns = vec![SelectItem {
        expr: Expr::All { table: None },
        alias: None,
    }];
    let row = Expr::Column(ColumnRef::new(None::<&str>, ROW_COLUMN));
    if offset > 0 {
        outer = outer.where_(Filter::from(Where::Compare {
            left: row.clone(),
            op: Operator::Gt,
            right: Expr::Value(Value::BigInt(Some(i64::try_from(offset).unwrap_or(i64::MAX)))),
        }));
    }
    if let Some(limit) = limit {
        let last = offset.saturating_add(limit);
        outer = outer.where_(Filter::from(Where::Compare {
            left: row.clone(),
            op: Operator::Lte,
            right: Expr::Value(Value::BigInt(Some(i64::try_from(last).unwrap_or(i64::MAX)))),
        }));
    }
    outer.ir.orders = vec![Order::asc(row)];
    outer.eager.tree = tree;
    outer
}

/// Removes the row-number column of a per-parent limit from `records`.
pub(crate) fn drop_row_numbers<R: Model>(records: &mut [Record<R>]) {
    for record in records {
        record.extras.retain(|(column, _)| column != ROW_COLUMN);
    }
}

/// The keys of a `has_one`/`has_many`: the foreign key on `R`, the local
/// key on the parent, and the type column of a morph relation.
struct HasKeys<'a> {
    name: &'a str,
    foreign_key: &'a str,
    local_key: &'a str,
    morph: Option<&'a str>,
}

impl HasKeys<'_> {
    /// `R.type = P's morph name`, for a morph relation.
    fn morph_condition<P: Model, R: Model>(&self, table: &str) -> Option<Where> {
        self.morph.map(|column| Where::Compare {
            left: Expr::Column(qualified(table, column)),
            op: Operator::Eq,
            right: Expr::Value(Value::from(P::morph_name().into_owned())),
        })
    }

    /// The conditions of the records related to `parent`; `None` when its
    /// local key is NULL.
    fn bound_conditions<P: Model, R: Model>(&self, parent: &P) -> Result<Option<Vec<Where>>> {
        let value = column_value(parent, self.local_key, self.name)?;
        if is_null(&value) {
            return Ok(None);
        }
        let table = R::meta().table();
        let foreign = qualified(table, self.foreign_key);
        let mut conditions = vec![
            Where::Compare {
                left: Expr::Column(foreign.clone()),
                op: Operator::Eq,
                right: Expr::Value(value),
            },
            Where::Null {
                expr: Expr::Column(foreign),
                not: true,
            },
        ];
        conditions.extend(self.morph_condition::<P, R>(table));
        Ok(Some(conditions))
    }

    /// The conditions of the records related to parents with `keys`, for a
    /// query on `target`.
    fn eager_conditions<P: Model, R: Model>(
        &self,
        target: &Target<'_>,
        keys: &[(RelationKey, Value)],
    ) -> Vec<Where> {
        let table = R::meta().table();
        let mut conditions = vec![in_keys_on(target, qualified(table, self.foreign_key), keys)];
        conditions.extend(self.morph_condition::<P, R>(table));
        conditions
    }

    /// The existence subquery of the relation.
    fn exists<P: Model, R: Model>(
        &self,
        definition: &Builder<R>,
        request: ExistsRequest,
        one_of_many: Option<&OneOfMany<R>>,
    ) -> Result<QueryIr> {
        let parent_table = P::meta().table();
        let table = R::meta().table();
        let aliased = (table == parent_table).then_some(table);
        let local = qualified(parent_table, self.local_key);
        let foreign_key = self.foreign_key.to_owned();
        let morph = self.morph_condition::<P, R>(table);
        let group_by = self.group_by::<R>();
        build_subquery::<R>(
            P::meta(),
            definition,
            request,
            self.name,
            aliased,
            |query, _| {
                let query = match one_of_many {
                    Some(one) => one.apply(query, self.name, &group_by, Vec::new()),
                    None => query,
                };
                match morph {
                    Some(morph) => query.where_(Filter::from(morph)),
                    None => query,
                }
            },
            |alias| {
                vec![columns_equal(
                    local,
                    qualified(alias.unwrap_or(table), &foreign_key),
                )]
            },
        )
    }

    /// The columns one-of-many groups by: the foreign key (and the type).
    fn group_by<R: Model>(&self) -> Vec<ColumnRef> {
        let table = R::meta().table();
        let mut columns = vec![qualified(table, self.foreign_key)];
        if let Some(morph) = self.morph {
            columns.push(qualified(table, morph));
        }
        columns
    }

    /// Sets the foreign key (and type) of `record` to `parent`'s.
    fn fill<P: Model, R: Model>(&self, parent: &P, record: &mut Record<R>) -> Result<()> {
        let value = column_value(parent, self.local_key, self.name)?;
        record.model.set_value(self.foreign_key, value)?;
        if let Some(morph) = self.morph {
            record.model.set_value(morph, stored_type_of::<P>()?)?;
        }
        Ok(())
    }

    /// The related records of `parents` (one query), grouped by the key
    /// each belongs to, for an eager load of the relation.
    async fn load<P: Model, R: Model>(
        &self,
        target: &Target<'_>,
        parents: &[Record<P>],
        query: Builder<R>,
        node: &EagerNode,
        one_of_many: Option<&OneOfMany<R>>,
    ) -> Result<(Vec<Option<RelationKey>>, HashMap<RelationKey, Vec<Record<R>>>)> {
        let values = column_values(parents, self.local_key, self.name)?;
        let parent_keys: Vec<Option<RelationKey>> = values.iter().map(key_of).collect();
        let keys = distinct_keys(values);
        let mut groups: HashMap<RelationKey, Vec<Record<R>>> = HashMap::new();
        if keys.is_empty() {
            return Ok((parent_keys, groups));
        }
        let table = R::meta().table();
        let query = prepare_query(query, node, table, &[self.foreign_key])?;
        let conditions = self.eager_conditions::<P, R>(target, &keys);
        let query = match one_of_many {
            Some(one) => one.apply(query, self.name, &self.group_by::<R>(), conditions),
            None => constrain(query, conditions),
        };
        let query = limit_per_parent(query, Expr::Column(qualified(table, self.foreign_key)));
        let mut records = query.fetch(target).await?.into_vec();
        drop_row_numbers(&mut records);
        if node.autoload {
            crate::relations::lazy::group_records(records.iter_mut());
        }
        for record in records {
            let value = column_value(&record.model, self.foreign_key, self.name)?;
            if let Some(key) = key_of(&value) {
                groups.entry(key).or_default().push(record);
            }
        }
        Ok((parent_keys, groups))
    }
}

/// Sets `inverse` on each of `children` to a snapshot of `parent`.
fn set_inverse<P: Model, R: Model>(inverse: &str, parent: &Record<P>, children: &mut [Record<R>]) {
    let snapshot = parent.without_relations();
    for child in children {
        child
            .relations
            .set_relation(inverse.to_owned(), Some(snapshot.clone()));
    }
}

/// The finisher of a bound relation with an inverse: the children get a
/// snapshot of the parent.
fn inverse_finisher<P: Model, R: Model>(inverse: &Option<Cow<'static, str>>, parent: &P) -> Option<Finisher<R>> {
    let inverse = inverse.clone()?;
    let snapshot = Record::stored(parent.clone());
    Some(Box::new(move |records: &mut [Record<R>]| {
        set_inverse(&inverse, &snapshot, records);
        Ok(())
    }))
}

/// The write and find-or-create methods of `has_one` and `has_many`
/// (`self` has `core`, `foreign_key`, `local_key`, `morph`, `inverse`).
macro_rules! has_write_methods {
    () => {
        fn keys(&self) -> HasKeys<'_> {
            HasKeys {
                name: &self.core.name,
                foreign_key: &self.foreign_key,
                local_key: &self.local_key,
                morph: self.morph.as_deref(),
            }
        }

        /// Sets the foreign key (and type, and inverse relation) of
        /// `record` for the parent.
        fn fill_foreign(&self, record: &mut Record<R>) -> Result<()> {
            let parent = self.core.parent()?;
            self.keys().fill::<P, R>(parent, record)?;
            if let Some(inverse) = &self.inverse {
                record
                    .relations
                    .set_relation(inverse.to_string(), Some(Record::stored(parent.clone())));
            }
            Ok(())
        }

        /// `model` as a new, unsaved related record: the foreign key (and
        /// type) set to the parent's.
        pub fn make(&self, model: R) -> Result<Record<R>> {
            let mut record = Record::new(model);
            self.fill_foreign(&mut record)?;
            Ok(record)
        }

        /// Each of `models` as a new, unsaved related record.
        pub fn make_many(&self, models: impl IntoIterator<Item = R>) -> Result<Collection<R>> {
            models.into_iter().map(|model| self.make(model)).collect()
        }

        /// Saves `record` as a related record: sets its foreign key (and
        /// type) to the parent's, then inserts or updates it. `false` when a
        /// hook cancelled the save.
        pub async fn save<E: Executor + ?Sized>(&self, exec: &E, record: &mut Record<R>) -> Result<bool> {
            self.fill_foreign(record)?;
            record.save(exec).await
        }

        /// [`save`](Self::save) without lifecycle hooks.
        pub async fn save_quietly<E: Executor + ?Sized>(
            &self,
            exec: &E,
            record: &mut Record<R>,
        ) -> Result<bool> {
            crate::hooks::quietly(self.save(exec, record)).await
        }

        /// Saves each of `records` as a related record; `false` when a hook
        /// cancelled one of the saves (the others are saved).
        pub async fn save_many<E: Executor + ?Sized>(
            &self,
            exec: &E,
            records: &mut [Record<R>],
        ) -> Result<bool> {
            let mut all = true;
            for record in records {
                all &= self.save(exec, record).await?;
            }
            Ok(all)
        }

        /// [`save_many`](Self::save_many) without lifecycle hooks.
        pub async fn save_many_quietly<E: Executor + ?Sized>(
            &self,
            exec: &E,
            records: &mut [Record<R>],
        ) -> Result<bool> {
            crate::hooks::quietly(self.save_many(exec, records)).await
        }

        /// Inserts `model` as a related record and returns it.
        pub async fn create<E: Executor + ?Sized>(&self, exec: &E, model: R) -> Result<Record<R>> {
            let mut record = self.make(model)?;
            record.save(exec).await?;
            Ok(record)
        }

        /// [`create`](Self::create) without lifecycle hooks.
        pub async fn create_quietly<E: Executor + ?Sized>(
            &self,
            exec: &E,
            model: R,
        ) -> Result<Record<R>> {
            crate::hooks::quietly(self.create(exec, model)).await
        }

        /// Inserts each of `models` as a related record.
        pub async fn create_many<E: Executor + ?Sized>(
            &self,
            exec: &E,
            models: impl IntoIterator<Item = R>,
        ) -> Result<Collection<R>> {
            let mut records = Collection::new();
            for model in models {
                records.push(self.create(exec, model).await?);
            }
            Ok(records)
        }

        /// [`create_many`](Self::create_many) without lifecycle hooks.
        pub async fn create_many_quietly<E: Executor + ?Sized>(
            &self,
            exec: &E,
            models: impl IntoIterator<Item = R>,
        ) -> Result<Collection<R>> {
            crate::hooks::quietly(self.create_many(exec, models)).await
        }

        /// Inserts a related record from untyped `attributes` (set on a
        /// default `R`, whatever its mass-assignment rules).
        pub async fn force_create<E: Executor + ?Sized>(
            &self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let mut record = new_with::<R>(&attributes, |_| {})?;
            self.fill_foreign(&mut record)?;
            record.save(exec).await?;
            Ok(record)
        }

        /// [`force_create`](Self::force_create) without lifecycle hooks.
        pub async fn force_create_quietly<E: Executor + ?Sized>(
            &self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            crate::hooks::quietly(self.force_create(exec, attributes)).await
        }

        /// Inserts a related record for each list of untyped attributes.
        pub async fn force_create_many<E, A>(
            &self,
            exec: &E,
            rows: impl IntoIterator<Item = A>,
        ) -> Result<Collection<R>>
        where
            E: Executor + ?Sized,
            A: IntoIterator<Item = Assignment>,
            R: Default,
        {
            let mut records = Collection::new();
            for attributes in rows {
                records.push(self.force_create(exec, attributes).await?);
            }
            Ok(records)
        }

        /// [`force_create_many`](Self::force_create_many) without lifecycle
        /// hooks.
        pub async fn force_create_many_quietly<E, A>(
            &self,
            exec: &E,
            rows: impl IntoIterator<Item = A>,
        ) -> Result<Collection<R>>
        where
            E: Executor + ?Sized,
            A: IntoIterator<Item = Assignment>,
            R: Default,
        {
            let rows: Vec<Vec<Assignment>> = rows.into_iter().map(|row| row.into_iter().collect()).collect();
            crate::hooks::quietly(self.force_create_many(exec, rows)).await
        }

        /// The related record with key `key`, or a new, unsaved one with the
        /// foreign key set.
        pub async fn find_or_new<E: Executor + ?Sized>(
            self,
            exec: &E,
            key: impl Into<R::Key>,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let blank = self.make(R::default())?;
            Ok(self.find(exec, key).await?.unwrap_or(blank))
        }

        /// The first related record matching `attributes`, or a new, unsaved
        /// one with them (and whatever `fill` sets), its foreign key set.
        pub async fn first_or_new<E: Executor + ?Sized>(
            self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
            fill: impl FnOnce(&mut R) + Send,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let found = self.clone().scope(|query| matching(query, &attributes)).first(exec).await?;
            match found {
                Some(record) => Ok(record),
                None => {
                    let mut record = new_with::<R>(&attributes, fill)?;
                    self.fill_foreign(&mut record)?;
                    Ok(record)
                }
            }
        }

        /// The first related record matching `attributes`, or a new one
        /// with them (and whatever `fill` sets) inserted. A concurrent
        /// insert of the same record (a unique violation) returns the one
        /// that won.
        pub async fn first_or_create<E: Executor + ?Sized>(
            self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
            fill: impl FnOnce(&mut R) + Send,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let target = exec.target(Access::Write).await?;
            if let Some(found) = self.clone().first_matching(&target, &attributes).await? {
                return Ok(found);
            }
            self.create_or_first_on(&target, &attributes, fill).await
        }

        /// Inserts a related record with `attributes` (and whatever `fill`
        /// sets); on a unique violation, returns the related record
        /// matching `attributes` instead.
        pub async fn create_or_first<E: Executor + ?Sized>(
            self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
            fill: impl FnOnce(&mut R) + Send,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let target = exec.target(Access::Write).await?;
            self.create_or_first_on(&target, &attributes, fill).await
        }

        /// Updates the first related record matching `attributes` with
        /// `fill` (and saves it), or creates one with both.
        pub async fn update_or_create<E: Executor + ?Sized>(
            self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
            fill: impl Fn(&mut R) + Send + Sync,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let target = exec.target(Access::Write).await?;
            let mut record = match self.clone().first_matching(&target, &attributes).await? {
                Some(found) => found,
                None => self.create_or_first_on(&target, &attributes, &fill).await?,
            };
            if !record.was_recently_created() && record.exists() {
                fill(&mut record.model);
                crate::persist::save(&target, &mut record).await?;
            }
            Ok(record)
        }

        /// Adds `step` to `column` of the related record matching
        /// `attributes`, or creates it with `column` set to `default`.
        pub async fn increment_or_create<E, T>(
            self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
            column: Column<R, T>,
            default: T,
            step: T,
        ) -> Result<Record<R>>
        where
            E: Executor + ?Sized,
            T: Numeric,
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let mut with_default = attributes.clone();
            with_default.push(column.set(default));
            let target = exec.target(Access::Write).await?;
            let mut record = match self.clone().first_matching(&target, &attributes).await? {
                Some(found) => found,
                None => self.create_or_first_on(&target, &with_default, |_| {}).await?,
            };
            if !record.was_recently_created() && record.exists() {
                crate::persist::increment(&target, &mut record, &column, step, false, Vec::new())
                    .await?;
            }
            Ok(record)
        }

        /// Like [`first_or_create`](Self::first_or_create), soft-deleted
        /// records included: a trashed match is restored.
        pub async fn restore_or_create<E: Executor + ?Sized>(
            self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
            fill: impl FnOnce(&mut R) + Send,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let target = exec.target(Access::Write).await?;
            let relation = self.with_trashed();
            let mut record = match relation.clone().first_matching(&target, &attributes).await? {
                Some(found) => found,
                None => relation.create_or_first_on(&target, &attributes, fill).await?,
            };
            restore_record(&target, &mut record).await?;
            Ok(record)
        }

        /// Like [`create_or_first`](Self::create_or_first), soft-deleted
        /// records included: a trashed match is restored.
        pub async fn create_or_restore<E: Executor + ?Sized>(
            self,
            exec: &E,
            attributes: impl IntoIterator<Item = Assignment>,
            fill: impl FnOnce(&mut R) + Send,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let attributes: Vec<Assignment> = attributes.into_iter().collect();
            let target = exec.target(Access::Write).await?;
            let mut record = self
                .with_trashed()
                .create_or_first_on(&target, &attributes, fill)
                .await?;
            restore_record(&target, &mut record).await?;
            Ok(record)
        }

        /// The first related record matching `attributes`, on `target`.
        async fn first_matching(
            self,
            target: &Target<'_>,
            attributes: &[Assignment],
        ) -> Result<Option<Record<R>>> {
            let bound = self.scope(|query| matching(query, attributes)).limit(1).bound()?;
            if bound.empty {
                return Ok(None);
            }
            let mut records = bound.query.clone().fetch(target).await?.into_vec();
            bound.finish(&mut records)?;
            Ok(records.into_iter().next())
        }

        async fn create_or_first_on(
            self,
            target: &Target<'_>,
            attributes: &[Assignment],
            fill: impl FnOnce(&mut R) + Send,
        ) -> Result<Record<R>>
        where
            R: Default,
        {
            let mut record = new_with::<R>(attributes, fill)?;
            self.fill_foreign(&mut record)?;
            match save_atomically(target, &mut record).await {
                Ok(_) => Ok(record),
                Err(error) if error.is_unique_violation() => {
                    match self.first_matching(target, attributes).await? {
                        Some(found) => Ok(found),
                        None => Err(error),
                    }
                }
                Err(error) => Err(error),
            }
        }

        /// The foreign key column (on the related model).
        pub fn foreign_key_name(&self) -> &str {
            &self.foreign_key
        }

        /// The foreign key column, qualified by the related table.
        pub fn qualified_foreign_key_name(&self) -> String {
            format!("{}.{}", R::meta().table(), self.foreign_key)
        }

        /// The parent's column the foreign key holds.
        pub fn local_key_name(&self) -> &str {
            &self.local_key
        }

        /// The value of the parent's local key (`None` when the relation was
        /// not made from a record).
        pub fn parent_key(&self) -> Option<Value> {
            let parent = self.core.parent.as_ref()?;
            column_value(parent, &self.local_key, &self.core.name).ok()
        }

        /// The type column of a polymorphic relation.
        pub fn morph_type(&self) -> Option<&str> {
            self.morph.as_deref()
        }

        /// The name a polymorphic relation stores in its type column for
        /// the parent.
        pub fn morph_name(&self) -> Option<Cow<'static, str>> {
            self.morph.as_ref().map(|_| P::morph_name())
        }

        /// The parent's column, when it is not its key.
        pub fn local_key(mut self, column: impl KeyColumn<P>) -> Self {
            self.local_key = column.key_column();
            self
        }

        /// The type column of a polymorphic relation, when it is not
        /// `{name}_type`.
        pub fn type_column(mut self, column: impl KeyColumn<R>) -> Self {
            self.morph = Some(column.key_column());
            self
        }

        /// The id column of a polymorphic relation (the foreign key), when
        /// it is not `{name}_id`.
        pub fn id_column(mut self, column: impl KeyColumn<R>) -> Self {
            self.foreign_key = column.key_column();
            self
        }

        /// Loaded related records get relation `name` set to a snapshot of
        /// their parent (without its relations), so `activity.deal` needs
        /// no query. `name` must be a relation of the related model.
        pub fn inverse(mut self, name: impl Into<Cow<'static, str>>) -> Self {
            self.inverse = Some(name.into());
            self
        }

        /// The same as [`inverse`](Self::inverse).
        pub fn chaperone(self, name: impl Into<Cow<'static, str>>) -> Self {
            self.inverse(name)
        }

        /// No inverse relation.
        pub fn without_inverse(mut self) -> Self {
            self.inverse = None;
            self
        }

        /// The same as [`without_inverse`](Self::without_inverse).
        pub fn without_chaperone(self) -> Self {
            self.without_inverse()
        }

        /// Sets the update timestamp of every related record to now (no
        /// hooks run; nothing when `R` keeps no update timestamp).
        pub async fn touch<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
            let target = exec.target(Access::Write).await?;
            let bound = self.without_global_scopes().bound()?;
            if bound.empty {
                return Ok(0);
            }
            crate::relations::touch::touch_related(Some(bound.query), &target).await
        }
    };
}

/// The record of `R` that holds the parent's key: a user's profile. With a
/// type column (`morph_one`), it also holds the parent's morph name.
/// Loaded, it holds an `Option<Record<R>>`.
///
/// `latest_of_many`, `oldest_of_many` and `of_many` make it pick one record
/// among many (a deal's latest activity).
pub struct HasOne<P: Model, R: Model> {
    pub(crate) core: RelationCore<P, R>,
    pub(crate) foreign_key: Cow<'static, str>,
    pub(crate) local_key: Cow<'static, str>,
    pub(crate) morph: Option<Cow<'static, str>>,
    pub(crate) inverse: Option<Cow<'static, str>>,
    pub(crate) default: Option<DefaultModel<P, R>>,
    pub(crate) one_of_many: Option<OneOfMany<R>>,
}

/// A polymorphic [`HasOne`] (`morph_one`): the related record holds the
/// parent's morph name and key.
pub type MorphOne<P, R> = HasOne<P, R>;

/// The records of `R` that hold the parent's key: a deal's activities.
/// With a type column (`morph_many`), they also hold the parent's morph
/// name. Loaded, it holds a [`Collection<R>`].
pub struct HasMany<P: Model, R: Model> {
    pub(crate) core: RelationCore<P, R>,
    pub(crate) foreign_key: Cow<'static, str>,
    pub(crate) local_key: Cow<'static, str>,
    pub(crate) morph: Option<Cow<'static, str>>,
    pub(crate) inverse: Option<Cow<'static, str>>,
}

/// A polymorphic [`HasMany`] (`morph_many`): the related records hold the
/// parent's morph name and key.
pub type MorphMany<P, R> = HasMany<P, R>;

impl<P: Model, R: Model> Clone for HasOne<P, R> {
    fn clone(&self) -> Self {
        HasOne {
            core: self.core.clone(),
            foreign_key: self.foreign_key.clone(),
            local_key: self.local_key.clone(),
            morph: self.morph.clone(),
            inverse: self.inverse.clone(),
            default: self.default.clone(),
            one_of_many: self.one_of_many.clone(),
        }
    }
}

impl<P: Model, R: Model> Clone for HasMany<P, R> {
    fn clone(&self) -> Self {
        HasMany {
            core: self.core.clone(),
            foreign_key: self.foreign_key.clone(),
            local_key: self.local_key.clone(),
            morph: self.morph.clone(),
            inverse: self.inverse.clone(),
        }
    }
}

impl<P: Model, R: Model> fmt::Debug for HasOne<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(if self.morph.is_some() { "MorphOne" } else { "HasOne" })
            .field("parent", &P::meta().name())
            .field("related", &R::meta().name())
            .field("name", &self.core.name)
            .field("foreign_key", &self.foreign_key)
            .field("local_key", &self.local_key)
            .finish_non_exhaustive()
    }
}

impl<P: Model, R: Model> fmt::Debug for HasMany<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(if self.morph.is_some() { "MorphMany" } else { "HasMany" })
            .field("parent", &P::meta().name())
            .field("related", &R::meta().name())
            .field("name", &self.core.name)
            .field("foreign_key", &self.foreign_key)
            .field("local_key", &self.local_key)
            .finish_non_exhaustive()
    }
}

relation_builder_traits!(HasOne);
relation_builder_traits!(HasMany);

impl<P: Model, R: Model> RelationParts<P, R> for HasOne<P, R> {
    fn core_mut(&mut self) -> &mut RelationCore<P, R> {
        &mut self.core
    }
}

impl<P: Model, R: Model> RelationParts<P, R> for HasMany<P, R> {
    fn core_mut(&mut self) -> &mut RelationCore<P, R> {
        &mut self.core
    }
}

/// A morph relation's columns: `{name}_type` and `{name}_id`.
fn morph_columns(name: &str) -> (Cow<'static, str>, Cow<'static, str>) {
    (
        Cow::Owned(format!("{name}_type")),
        Cow::Owned(format!("{name}_id")),
    )
}

impl<P: Model, R: Model> HasOne<P, R> {
    /// The relation of `parent` through `R`'s column `foreign_key`, named
    /// after `R` (`profile`).
    pub(crate) fn new(parent: Option<P>, foreign_key: Cow<'static, str>) -> Self {
        HasOne {
            core: RelationCore::new(
                parent,
                Cow::Owned(crate::model::snake_case(R::meta().name())),
            ),
            foreign_key,
            local_key: Cow::Borrowed(P::meta().key_column()),
            morph: None,
            inverse: None,
            default: None,
            one_of_many: None,
        }
    }

    /// The `morph_one` named `name`.
    pub(crate) fn new_morph(parent: Option<P>, name: &str) -> Self {
        let (type_column, id_column) = morph_columns(name);
        let mut relation = HasOne::new(parent, id_column);
        relation.morph = Some(type_column);
        relation
    }

    relation_query_methods!();
    relation_retrieval_methods!();
    has_write_methods!();

    /// When nothing is related, the relation holds a new, unsaved `R`
    /// (its `Default`, the foreign key set) instead of `None`.
    pub fn with_default(mut self) -> Self
    where
        R: Default,
    {
        self.default = Some(Arc::new(|_| R::default()));
        self
    }

    /// When nothing is related, the relation holds a new, unsaved copy of
    /// `model` (the foreign key set).
    pub fn with_default_value(mut self, model: R) -> Self {
        self.default = Some(Arc::new(move |_| model.clone()));
        self
    }

    /// When nothing is related, the relation holds a new, unsaved record
    /// made by `make` from the parent (the foreign key set).
    pub fn with_default_fn(mut self, make: impl Fn(&P) -> R + Send + Sync + 'static) -> Self {
        self.default = Some(Arc::new(make));
        self
    }

    /// The default record for `parent`, if the relation has one.
    fn default_for(&self, parent: &P) -> Option<Record<R>> {
        let make = self.default.as_ref()?;
        let mut record = Record::new(make(parent));
        // A default carries the parent's key (and type), like a new record
        // made through the relation.
        let _ = self.keys().fill::<P, R>(parent, &mut record);
        Some(record)
    }

    /// Picks the record with the largest key.
    pub fn latest_of_many(self) -> Self {
        self.of_many_columns(vec![(
            Cow::Borrowed(R::meta().key_column()),
            AggregateFunction::Max,
        )])
    }

    /// Picks the record with the smallest key.
    pub fn oldest_of_many(self) -> Self {
        self.of_many_columns(vec![(
            Cow::Borrowed(R::meta().key_column()),
            AggregateFunction::Min,
        )])
    }

    /// Picks the record with the largest `column` (the largest key among
    /// ties).
    pub fn latest_of_many_by(self, column: impl KeyColumn<R>) -> Self {
        self.of_many_columns(vec![(column.key_column(), AggregateFunction::Max)])
    }

    /// Picks the record with the smallest `column` (the largest key among
    /// ties).
    pub fn oldest_of_many_by(self, column: impl KeyColumn<R>) -> Self {
        self.of_many_columns(vec![(column.key_column(), AggregateFunction::Min)])
    }

    /// Picks the record by aggregates of `columns` (`MAX` or `MIN`), each
    /// breaking the ties of the ones before (the key, by `MAX`, last), among
    /// the records `constraint` keeps.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::ir::AggregateFunction;
    /// use navree_orm::relations::HasOne;
    ///
    /// # fn demo(deal: &Deal) -> HasOne<Deal, Activity> {
    /// // The activity due last among the open ones.
    /// deal.has_one(Activity::DEAL_ID).of_many(
    ///     [(Activity::DUE_AT, AggregateFunction::Max)],
    ///     |q| q.where_(Activity::SUBJECT.ne("closed")),
    /// )
    /// # }
    /// ```
    pub fn of_many<C: KeyColumn<R>>(
        mut self,
        columns: impl IntoIterator<Item = (C, AggregateFunction)>,
        constraint: impl Fn(Builder<R>) -> Builder<R> + Send + Sync + 'static,
    ) -> Self {
        let columns: Vec<(Cow<'static, str>, AggregateFunction)> = columns
            .into_iter()
            .map(|(column, function)| (column.key_column(), function))
            .collect();
        self = self.of_many_columns(columns);
        if let Some(one) = &mut self.one_of_many {
            one.constraint = Some(Arc::new(constraint));
        }
        self
    }

    fn of_many_columns(mut self, columns: Vec<(Cow<'static, str>, AggregateFunction)>) -> Self {
        if let Some((column, function)) = columns.iter().find(|(_, function)| {
            !matches!(function, AggregateFunction::Max | AggregateFunction::Min)
        }) {
            let message = format!(
                "one of many picks by MAX or MIN, not {} of `{column}`",
                function.sql_name()
            );
            self.core.query = self.core.query.with_error(message);
            return self;
        }
        self.one_of_many = Some(OneOfMany::new(columns));
        self
    }

    /// The alias of a one-of-many join, when it is not the relation's
    /// name.
    pub fn of_many_alias(mut self, alias: impl Into<Cow<'static, str>>) -> Self {
        if let Some(one) = &mut self.one_of_many {
            one.alias = Some(alias.into());
        }
        self
    }

    /// Whether the relation picks one record among many.
    pub fn is_one_of_many(&self) -> bool {
        self.one_of_many.is_some()
    }

    /// Whether `record` is the related record: for a plain relation, its
    /// foreign key holds the parent's key (no query); for one of many, it
    /// is also the one picked (a query).
    pub async fn is<E: Executor + ?Sized>(&self, exec: &E, record: &Record<R>) -> Result<bool> {
        let Some(parent_key) = self.parent_key() else {
            return Ok(false);
        };
        let foreign = column_value(&record.model, &self.foreign_key, &self.core.name)?;
        if key_of(&parent_key).is_none() || key_of(&parent_key) != key_of(&foreign) {
            return Ok(false);
        }
        if self.one_of_many.is_none() {
            return Ok(true);
        }
        self.clone().where_key(record.key()).exists(exec).await
    }

    /// Whether `record` is not the related record.
    pub async fn is_not<E: Executor + ?Sized>(&self, exec: &E, record: &Record<R>) -> Result<bool> {
        Ok(!self.is(exec, record).await?)
    }

    /// What the relation holds once loaded: the related record, or the
    /// default (or `None`) when nothing is related.
    pub async fn results<E: Executor + ?Sized>(self, exec: &E) -> Result<Option<Record<R>>> {
        let parent = self.core.parent()?.clone();
        let default = self.default_for(&parent);
        Ok(self.first(exec).await?.or(default))
    }

    /// The relation's query for its parent.
    pub(crate) fn bound(self) -> Result<Bound<R>> {
        let parent = self.core.parent()?.clone();
        let keys = self.keys();
        let Some(conditions) = keys.bound_conditions::<P, R>(&parent)? else {
            return Ok(Bound::nothing(self.core.query));
        };
        let group_by = keys.group_by::<R>();
        let name = self.core.name.clone();
        let finish = inverse_finisher(&self.inverse, &parent);
        let query = match &self.one_of_many {
            Some(one) => one.apply(self.core.query, &name, &group_by, conditions),
            None => constrain(self.core.query, conditions),
        };
        Ok(Bound::new(query).finishing(finish))
    }
}

impl<P: Model, R: Model> HasMany<P, R> {
    /// The relation of `parent` through `R`'s column `foreign_key`, named
    /// after `R`'s table (`activities`).
    pub(crate) fn new(parent: Option<P>, foreign_key: Cow<'static, str>) -> Self {
        HasMany {
            core: RelationCore::new(parent, Cow::Borrowed(R::meta().table())),
            foreign_key,
            local_key: Cow::Borrowed(P::meta().key_column()),
            morph: None,
            inverse: None,
        }
    }

    /// The `morph_many` named `name`.
    pub(crate) fn new_morph(parent: Option<P>, name: &str) -> Self {
        let (type_column, id_column) = morph_columns(name);
        let mut relation = HasMany::new(parent, id_column);
        relation.morph = Some(type_column);
        relation
    }

    relation_query_methods!();
    relation_retrieval_methods!();
    has_write_methods!();

    /// The same relation as a `has_one` (one of the records; pick which
    /// with `latest_of_many` and friends).
    pub fn one(self) -> HasOne<P, R> {
        HasOne {
            core: self.core,
            foreign_key: self.foreign_key,
            local_key: self.local_key,
            morph: self.morph,
            inverse: self.inverse,
            default: None,
            one_of_many: None,
        }
    }

    /// The record with the largest key, as a `has_one`.
    pub fn latest_of_many(self) -> HasOne<P, R> {
        self.one().latest_of_many()
    }

    /// The record with the smallest key, as a `has_one`.
    pub fn oldest_of_many(self) -> HasOne<P, R> {
        self.one().oldest_of_many()
    }

    /// The record with the largest `column`, as a `has_one`.
    pub fn latest_of_many_by(self, column: impl KeyColumn<R>) -> HasOne<P, R> {
        self.one().latest_of_many_by(column)
    }

    /// The record with the smallest `column`, as a `has_one`.
    pub fn oldest_of_many_by(self, column: impl KeyColumn<R>) -> HasOne<P, R> {
        self.one().oldest_of_many_by(column)
    }

    /// The record picked by aggregates of `columns`, as a `has_one` (see
    /// [`HasOne::of_many`]).
    pub fn of_many<C: KeyColumn<R>>(
        self,
        columns: impl IntoIterator<Item = (C, AggregateFunction)>,
        constraint: impl Fn(Builder<R>) -> Builder<R> + Send + Sync + 'static,
    ) -> HasOne<P, R> {
        self.one().of_many(columns, constraint)
    }

    /// What the relation holds once loaded: the related records.
    pub async fn results<E: Executor + ?Sized>(self, exec: &E) -> Result<Collection<R>> {
        self.get(exec).await
    }

    /// The relation's query for its parent.
    pub(crate) fn bound(self) -> Result<Bound<R>> {
        let parent = self.core.parent()?.clone();
        let Some(conditions) = self.keys().bound_conditions::<P, R>(&parent)? else {
            return Ok(Bound::nothing(self.core.query));
        };
        let finish = inverse_finisher(&self.inverse, &parent);
        Ok(Bound::new(constrain(self.core.query, conditions)).finishing(finish))
    }
}

impl<P: Model, R: Model> private::Erase<P> for HasOne<P, R> {
    fn erase(self) -> Arc<dyn ErasedRelation<P>> {
        let mut unbound = self;
        unbound.core = unbound.core.unbound();
        Arc::new(unbound)
    }

    fn rename(&mut self, name: Cow<'static, str>) {
        self.core.name = name;
    }
}

impl<P: Model, R: Model> private::Erase<P> for HasMany<P, R> {
    fn erase(self) -> Arc<dyn ErasedRelation<P>> {
        let mut unbound = self;
        unbound.core = unbound.core.unbound();
        Arc::new(unbound)
    }

    fn rename(&mut self, name: Cow<'static, str>) {
        self.core.name = name;
    }
}

impl<P: Model, R: Model> Relation<P> for HasOne<P, R> {
    type Value = Option<Record<R>>;

    fn name(&self) -> &str {
        &self.core.name
    }

    fn kind(&self) -> RelationKind {
        if self.morph.is_some() {
            RelationKind::MorphOne
        } else {
            RelationKind::HasOne
        }
    }

    fn meta(&self) -> RelationMeta {
        let mut meta = RelationMeta::new::<P>(&self.core.name, self.kind())
            .related::<R>()
            .keys(&self.foreign_key, &self.local_key);
        meta.morph_type = self.morph.as_ref().map(ToString::to_string);
        meta.morph_name = self.morph.as_ref().map(|_| P::morph_name().into_owned());
        meta
    }
}

impl<P: Model, R: Model> Relation<P> for HasMany<P, R> {
    type Value = Collection<R>;

    fn name(&self) -> &str {
        &self.core.name
    }

    fn kind(&self) -> RelationKind {
        if self.morph.is_some() {
            RelationKind::MorphMany
        } else {
            RelationKind::HasMany
        }
    }

    fn meta(&self) -> RelationMeta {
        let mut meta = RelationMeta::new::<P>(&self.core.name, self.kind())
            .related::<R>()
            .keys(&self.foreign_key, &self.local_key);
        meta.morph_type = self.morph.as_ref().map(ToString::to_string);
        meta.morph_name = self.morph.as_ref().map(|_| P::morph_name().into_owned());
        meta
    }
}

impl<P: Model, R: Model> RelatedTo<P> for HasOne<P, R> {
    type Related = R;
}

impl<P: Model, R: Model> RelatedTo<P> for HasMany<P, R> {
    type Related = R;
}

impl<P: Model, R: Model> ErasedRelation<P> for HasOne<P, R> {
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
            let (parent_keys, groups) = self
                .keys()
                .load(
                    target,
                    parents,
                    relation.core.query,
                    node,
                    self.one_of_many.as_ref(),
                )
                .await?;
            let firsts: HashMap<RelationKey, Record<R>> = groups
                .into_iter()
                .filter_map(|(key, records)| records.into_iter().next().map(|record| (key, record)))
                .collect();
            let values = distribute(firsts, &parent_keys);
            for (parent, value) in parents.iter_mut().zip(values) {
                let mut value = value.or_else(|| self.default_for(&parent.model));
                if let (Some(inverse), Some(child)) = (&self.inverse, &mut value) {
                    set_inverse(inverse, parent, std::slice::from_mut(child));
                }
                parent.relations.set_relation(node.name.clone(), value);
            }
            Ok(())
        })
    }

    fn exists_query(&self, request: ExistsRequest) -> Result<QueryIr> {
        self.keys()
            .exists::<P, R>(&self.core.query, request, self.one_of_many.as_ref())
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
                crate::relations::touch::touch_related(Some(bound.query), target).await?;
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

impl<P: Model, R: Model> ErasedRelation<P> for HasMany<P, R> {
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
            let (parent_keys, groups) = self
                .keys()
                .load(target, parents, relation.core.query, node, None)
                .await?;
            let values = distribute(groups, &parent_keys);
            for (parent, value) in parents.iter_mut().zip(values) {
                let mut children = value.unwrap_or_default();
                if let Some(inverse) = &self.inverse {
                    set_inverse(inverse, parent, &mut children);
                }
                parent
                    .relations
                    .set_relation(node.name.clone(), Collection::from(children));
            }
            Ok(())
        })
    }

    fn exists_query(&self, request: ExistsRequest) -> Result<QueryIr> {
        self.keys().exists::<P, R>(&self.core.query, request, None)
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
                crate::relations::touch::touch_related(Some(bound.query), target).await?;
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
