//! Pivot rows of many-to-many relations: [`Pivot`] (a row of the pivot
//! table, loaded with each related record), [`PivotKind`] (a plain pivot
//! or a typed pivot model), the ids `attach`, `detach` and `sync` take
//! ([`RelatedId`]), and what `sync` reports ([`SyncChanges`]).

use std::borrow::Cow;
use std::fmt;

use navree_kernel::BoxFuture;
use serde::{Serialize, Serializer};

use crate::column::Assignment;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::ir::{Expr, Operator, Where, WhereClause};
use crate::model::Model;
use crate::query::{Builder, TableBuilder};
use crate::record::Record;
use crate::relations::any::AnyRecord;
use crate::relations::store::{RelationValue, sealed};
use crate::row::Row;
use crate::value::{Field, Value, same_value};

/// What a pivot row knows of its relation: its table, its two key columns,
/// the morph type it carries, its timestamp columns.
#[derive(Clone, Debug)]
pub struct PivotSpec {
    pub(crate) table: Cow<'static, str>,
    pub(crate) foreign_key: Cow<'static, str>,
    pub(crate) related_key: Cow<'static, str>,
    /// The morph type column and the name it holds (morph to many).
    pub(crate) morph: Option<(Cow<'static, str>, String)>,
    pub(crate) created_at: Option<Cow<'static, str>>,
    pub(crate) updated_at: Option<Cow<'static, str>>,
}

impl PivotSpec {
    /// The conditions identifying the row with these key values.
    fn identity(&self, foreign: &Value, related: &Value) -> Vec<WhereClause> {
        let column = |name: &str| Expr::Column(crate::ir::ColumnRef::new(None::<&str>, name.to_owned()));
        let mut clauses = vec![
            WhereClause::and(Where::Compare {
                left: column(&self.foreign_key),
                op: Operator::Eq,
                right: Expr::Value(foreign.clone()),
            }),
            WhereClause::and(Where::Compare {
                left: column(&self.related_key),
                op: Operator::Eq,
                right: Expr::Value(related.clone()),
            }),
        ];
        if let Some((type_column, name)) = &self.morph {
            clauses.push(WhereClause::and(Where::Compare {
                left: column(type_column),
                op: Operator::Eq,
                right: Expr::Value(Value::from(name.clone())),
            }));
        }
        clauses
    }
}

/// A row of a many-to-many relation's pivot table: loaded with each
/// related record (`tag.pivot()`, under `pivot` unless the relation names
/// it with `as_`), or made by `new_pivot`.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, deal: Record<Deal>) -> Result<()> {
/// for tag in deal.tags().get(&db).await? {
///     let pivot = tag.pivot().unwrap();
///     let added: Option<DateTime<Utc>> = pivot.value("created_at")?;
///     println!("{} added {added:?}", tag.name);
/// }
/// # Ok(()) }
/// ```
#[derive(Clone)]
pub struct Pivot {
    spec: PivotSpec,
    values: Vec<(String, Value)>,
    original: Vec<(String, Value)>,
    exists: bool,
    parent: Option<AnyRecord>,
    related: Option<AnyRecord>,
}

impl Pivot {
    /// A pivot of `spec` with `values`; stored when `exists`.
    pub(crate) fn from_values(spec: PivotSpec, values: Vec<(String, Value)>, exists: bool) -> Self {
        Pivot {
            spec,
            original: if exists { values.clone() } else { Vec::new() },
            values,
            exists,
            parent: None,
            related: None,
        }
    }

    /// The pivot table.
    pub fn table(&self) -> &str {
        &self.spec.table
    }

    /// The column holding the parent's key (`deal_id`).
    pub fn foreign_key(&self) -> &str {
        &self.spec.foreign_key
    }

    /// The column holding the related record's key (`tag_id`).
    pub fn related_key(&self) -> &str {
        &self.spec.related_key
    }

    /// The morph type column of a polymorphic pivot.
    pub fn morph_type(&self) -> Option<&str> {
        self.spec.morph.as_ref().map(|(column, _)| column.as_ref())
    }

    /// The name a polymorphic pivot's type column holds.
    pub fn morph_name(&self) -> Option<&str> {
        self.spec.morph.as_ref().map(|(_, name)| name.as_str())
    }

    /// Column `column` as stored.
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(name, _)| name == column)
            .map(|(_, value)| value)
    }

    /// Column `column` read as `T`; `None` when the pivot has no such
    /// column.
    pub fn value<T: Field>(&self, column: &str) -> Result<Option<T>> {
        match self.get(column) {
            Some(value) => T::from_value(value.clone()).map(Some).map_err(|error| {
                crate::row::decode_error(
                    self.spec.table.to_string(),
                    column,
                    T::type_name(),
                    &error,
                )
            }),
            None => Ok(None),
        }
    }

    /// Every column, as stored.
    pub fn values(&self) -> &[(String, Value)] {
        &self.values
    }

    /// Sets column `column` (saved with [`save`](Pivot::save)).
    pub fn set(&mut self, column: impl Into<String>, value: impl Into<Value>) {
        let column = column.into();
        let value = value.into();
        match self.values.iter_mut().find(|(name, _)| *name == column) {
            Some(existing) => existing.1 = value,
            None => self.values.push((column, value)),
        }
    }

    /// Whether the row is stored.
    pub fn exists(&self) -> bool {
        self.exists
    }

    /// Whether a column changed since the row was read.
    pub fn is_dirty(&self) -> bool {
        !self.dirty().is_empty()
    }

    fn dirty(&self) -> Vec<(String, Value)> {
        self.values
            .iter()
            .filter(|(column, value)| {
                !self
                    .original
                    .iter()
                    .any(|(name, original)| name == column && same_value(original, value))
            })
            .cloned()
            .collect()
    }

    /// Whether the row has the creation timestamp column.
    pub fn has_timestamp_attributes(&self) -> bool {
        self.spec
            .created_at
            .as_deref()
            .is_some_and(|column| self.get(column).is_some())
    }

    /// The parent record the row was loaded for, when the relation keeps
    /// it (`chaperone_pivot`).
    pub fn parent<P: Model>(&self) -> Option<&Record<P>> {
        self.parent.as_ref()?.downcast_ref::<P>()
    }

    /// The related record the row belongs to, when the relation keeps it
    /// (`chaperone_pivot`).
    pub fn related<R: Model>(&self) -> Option<&Record<R>> {
        self.related.as_ref()?.downcast_ref::<R>()
    }

    pub(crate) fn set_parent(&mut self, parent: AnyRecord) {
        self.parent = Some(parent);
    }

    pub(crate) fn set_related(&mut self, related: AnyRecord) {
        self.related = Some(related);
    }

    /// The row's key values: the parent's and the related record's.
    fn keys(&self) -> Result<(Value, Value)> {
        let read = |column: &str| {
            self.original
                .iter()
                .chain(self.values.iter())
                .find(|(name, _)| name == column)
                .map(|(_, value)| value.clone())
                .ok_or_else(|| {
                    OrmError::invalid(format!(
                        "a pivot row of `{}` needs its `{column}` column",
                        self.spec.table
                    ))
                })
        };
        Ok((read(&self.spec.foreign_key)?, read(&self.spec.related_key)?))
    }

    /// Inserts the row when it is new; updates its changed columns (and its
    /// update timestamp) when it is stored. `false` when nothing changed.
    pub async fn save<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        self.save_on(&target).await
    }

    pub(crate) async fn save_on(&mut self, target: &Target<'_>) -> Result<bool> {
        let now = Value::ChronoDateTimeUtc(Some(target.context().now().await));
        if !self.exists {
            let timestamps: Vec<Cow<'static, str>> = [&self.spec.created_at, &self.spec.updated_at]
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            for column in timestamps {
                if self.get(&column).is_none() {
                    self.set(column.into_owned(), now.clone());
                }
            }
            let row: Vec<(String, Value)> = self.values.clone();
            let insert = crate::ir::InsertIr {
                table: self.spec.table.clone(),
                columns: row.iter().map(|(column, _)| Cow::Owned(column.clone())).collect(),
                rows: vec![row.into_iter().map(|(_, value)| Expr::Value(value)).collect()],
                returning: Vec::new(),
            };
            target.insert(&insert).await?;
            self.exists = true;
            self.original = self.values.clone();
            return Ok(true);
        }
        let mut dirty = self.dirty();
        if dirty.is_empty() {
            return Ok(false);
        }
        if let Some(updated_at) = self.spec.updated_at.clone()
            && !dirty.iter().any(|(column, _)| *column == updated_at)
        {
            self.set(updated_at.to_string(), now.clone());
            dirty.push((updated_at.into_owned(), now));
        }
        let (foreign, related) = self.keys()?;
        let update = crate::ir::UpdateIr {
            table: crate::ir::TableRef::new(self.spec.table.clone()),
            sets: dirty
                .into_iter()
                .map(|(column, value)| (Cow::Owned(column), Expr::Value(value)))
                .collect(),
            wheres: self.spec.identity(&foreign, &related),
        };
        target.update(&update).await?;
        self.original = self.values.clone();
        Ok(true)
    }

    /// Deletes the row (by both keys, and the morph type); returns how many
    /// rows went.
    pub async fn delete<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<u64> {
        if !self.exists {
            return Ok(0);
        }
        let (foreign, related) = self.keys()?;
        let target = exec.target(Access::Write).await?;
        let deleted = target
            .delete(&crate::ir::DeleteIr {
                table: crate::ir::TableRef::new(self.spec.table.clone()),
                wheres: self.spec.identity(&foreign, &related),
            })
            .await?;
        self.exists = false;
        Ok(deleted)
    }
}

impl fmt::Debug for Pivot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pivot")
            .field("table", &self.spec.table)
            .field("values", &self.values)
            .field("exists", &self.exists)
            .finish_non_exhaustive()
    }
}

/// The pivot's columns, as a JSON object.
impl Serialize for Pivot {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serde_json::Value::Object(
            self.values
                .iter()
                .map(|(column, value)| (column.clone(), crate::value::to_json(value)))
                .collect(),
        )
        .serialize(serializer)
    }
}

impl sealed::Sealed for Pivot {}

impl RelationValue for Pivot {
    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.save_on(target).await?;
            Ok(true)
        })
    }

    fn column(&self, column: &str) -> Option<Value> {
        self.get(column).cloned()
    }
}

/// What a many-to-many relation's pivot rows are: plain [`Pivot`]s, or
/// records of a typed pivot model (`.using::<DealTag>()`, any model with
/// `Default`), whose casts, hooks and events apply when rows are attached,
/// updated and detached.
pub trait PivotKind: Send + Sync + 'static {
    /// What a loaded related record carries under the pivot accessor.
    type Row: RelationValue;

    /// The columns selected besides the keys and `with_pivot`'s (a typed
    /// model's own columns).
    #[doc(hidden)]
    fn columns() -> Vec<&'static str>;

    /// A pivot row from its columns.
    #[doc(hidden)]
    fn row(spec: &PivotSpec, values: Vec<(String, Value)>, exists: bool) -> Result<Self::Row>;

    /// Keeps the parent and related snapshots on a row
    /// (`chaperone_pivot`).
    #[doc(hidden)]
    fn chaperone(row: &mut Self::Row, parent: AnyRecord, related: AnyRecord);

    /// Inserts `rows` (each a list of columns).
    #[doc(hidden)]
    fn insert<'a, 'b>(
        target: &'a Target<'b>,
        spec: &'a PivotSpec,
        rows: Vec<Vec<(String, Value)>>,
    ) -> BoxFuture<'a, Result<()>>;

    /// Deletes the rows `rows` selects; returns how many.
    #[doc(hidden)]
    fn delete<'a, 'b>(target: &'a Target<'b>, rows: TableBuilder) -> BoxFuture<'a, Result<u64>>;

    /// Sets `values` on the rows `rows` selects; returns how many changed.
    #[doc(hidden)]
    fn update<'a, 'b>(
        target: &'a Target<'b>,
        spec: &'a PivotSpec,
        rows: TableBuilder,
        values: Vec<(String, Value)>,
    ) -> BoxFuture<'a, Result<u64>>;
}

/// Inserts `rows` into `table`, one statement per set of columns.
async fn insert_rows(
    target: &Target<'_>,
    table_name: &str,
    rows: Vec<Vec<(String, Value)>>,
) -> Result<()> {
    let mut batches: Vec<(Vec<String>, Vec<Vec<Expr>>)> = Vec::new();
    for row in rows {
        let columns: Vec<String> = row.iter().map(|(column, _)| column.clone()).collect();
        let values: Vec<Expr> = row.into_iter().map(|(_, value)| Expr::Value(value)).collect();
        match batches.iter_mut().find(|(existing, _)| *existing == columns) {
            Some((_, batch)) => batch.push(values),
            None => batches.push((columns, vec![values])),
        }
    }
    for (columns, rows) in batches {
        let insert = crate::ir::InsertIr {
            table: Cow::Owned(table_name.to_owned()),
            columns: columns.into_iter().map(Cow::Owned).collect(),
            rows,
            returning: Vec::new(),
        };
        target.insert(&insert).await?;
    }
    Ok(())
}

impl PivotKind for Pivot {
    type Row = Pivot;

    fn columns() -> Vec<&'static str> {
        Vec::new()
    }

    fn row(spec: &PivotSpec, values: Vec<(String, Value)>, exists: bool) -> Result<Pivot> {
        Ok(Pivot::from_values(spec.clone(), values, exists))
    }

    fn chaperone(row: &mut Pivot, parent: AnyRecord, related: AnyRecord) {
        row.set_parent(parent);
        row.set_related(related);
    }

    fn insert<'a, 'b>(
        target: &'a Target<'b>,
        spec: &'a PivotSpec,
        rows: Vec<Vec<(String, Value)>>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(insert_rows(target, &spec.table, rows))
    }

    fn delete<'a, 'b>(target: &'a Target<'b>, rows: TableBuilder) -> BoxFuture<'a, Result<u64>> {
        Box::pin(async move {
            let ir = rows.into_ir();
            target
                .delete(&crate::ir::DeleteIr {
                    table: crate::ir::TableRef::new(ir.table.name),
                    wheres: ir.wheres,
                })
                .await
        })
    }

    fn update<'a, 'b>(
        target: &'a Target<'b>,
        _spec: &'a PivotSpec,
        rows: TableBuilder,
        values: Vec<(String, Value)>,
    ) -> BoxFuture<'a, Result<u64>> {
        Box::pin(async move {
            if values.is_empty() {
                return Ok(0);
            }
            let ir = rows.into_ir();
            target
                .update(&crate::ir::UpdateIr {
                    table: crate::ir::TableRef::new(ir.table.name),
                    sets: values
                        .into_iter()
                        .map(|(column, value)| (Cow::Owned(column), Expr::Value(value)))
                        .collect(),
                    wheres: ir.wheres,
                })
                .await
        })
    }
}

/// The typed pivot records `rows` selects.
async fn typed_rows<M: Model>(target: &Target<'_>, rows: TableBuilder) -> Result<Vec<Record<M>>> {
    let mut query = Builder::<M>::new();
    query.ir.wheres = rows.into_ir().wheres;
    Ok(query.fetch(target).await?.into_vec())
}

impl<M: Model + Default> PivotKind for M {
    type Row = Record<M>;

    fn columns() -> Vec<&'static str> {
        M::meta().all_columns().iter().map(|column| column.name()).collect()
    }

    fn row(_spec: &PivotSpec, values: Vec<(String, Value)>, exists: bool) -> Result<Record<M>> {
        let row = Row::from_values(values);
        if exists {
            Record::from_row(&row)
        } else {
            let mut model = M::default();
            for (column, value) in row.values()? {
                if M::meta().has_column(&column) {
                    model.set_value(&column, value)?;
                }
            }
            Ok(Record::new(model))
        }
    }

    fn chaperone(row: &mut Record<M>, parent: AnyRecord, related: AnyRecord) {
        let parent_name = crate::model::snake_case(parent.model_name());
        let related_name = crate::model::snake_case(related.model_name());
        row.relations.set_relation(parent_name, Some(parent));
        row.relations.set_relation(related_name, Some(related));
    }

    fn insert<'a, 'b>(
        target: &'a Target<'b>,
        _spec: &'a PivotSpec,
        rows: Vec<Vec<(String, Value)>>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            for row in rows {
                let mut model = M::default();
                for (column, value) in row {
                    model.set_value(&column, value)?;
                }
                let mut record = Record::new(model);
                crate::persist::save(target, &mut record).await?;
            }
            Ok(())
        })
    }

    fn delete<'a, 'b>(target: &'a Target<'b>, rows: TableBuilder) -> BoxFuture<'a, Result<u64>> {
        Box::pin(async move {
            let mut deleted = 0;
            for mut record in typed_rows::<M>(target, rows).await? {
                if crate::persist::delete(target, &mut record).await? {
                    deleted += 1;
                }
            }
            Ok(deleted)
        })
    }

    fn update<'a, 'b>(
        target: &'a Target<'b>,
        _spec: &'a PivotSpec,
        rows: TableBuilder,
        values: Vec<(String, Value)>,
    ) -> BoxFuture<'a, Result<u64>> {
        Box::pin(async move {
            let mut updated = 0;
            for mut record in typed_rows::<M>(target, rows).await? {
                for (column, value) in &values {
                    record.model.set_value(column, value.clone())?;
                }
                if record.is_dirty() && crate::persist::save(target, &mut record).await? {
                    updated += 1;
                }
            }
            Ok(updated)
        })
    }
}

/// A related record's id for `attach`, `detach`, `sync` and `toggle`: a
/// key value (`7`, a `Uuid`), or a record (its related key is read).
pub trait RelatedId<R: Model> {
    /// The value of the related key `column`.
    #[doc(hidden)]
    fn related_value(&self, column: &str) -> Result<Value>;
}

impl<R: Model, T: Field> RelatedId<R> for T {
    fn related_value(&self, _column: &str) -> Result<Value> {
        Ok(self.to_value())
    }
}

impl<R: Model> RelatedId<R> for &Record<R> {
    fn related_value(&self, column: &str) -> Result<Value> {
        crate::relations::core::column_value(&self.model, column, "pivot")
    }
}

impl<R: Model> RelatedId<R> for Record<R> {
    fn related_value(&self, column: &str) -> Result<Value> {
        crate::relations::core::column_value(&self.model, column, "pivot")
    }
}

/// What a `sync` (or `toggle`) changed, by related id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncChanges {
    /// The ids attached.
    pub attached: Vec<Value>,
    /// The ids detached.
    pub detached: Vec<Value>,
    /// The ids whose pivot values were updated.
    pub updated: Vec<Value>,
}

impl SyncChanges {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.attached.is_empty() && self.detached.is_empty() && self.updated.is_empty()
    }

    /// The attached ids read as `T`.
    pub fn attached_as<T: Field>(&self) -> Result<Vec<T>> {
        read_ids(&self.attached)
    }

    /// The detached ids read as `T`.
    pub fn detached_as<T: Field>(&self) -> Result<Vec<T>> {
        read_ids(&self.detached)
    }

    /// The updated ids read as `T`.
    pub fn updated_as<T: Field>(&self) -> Result<Vec<T>> {
        read_ids(&self.updated)
    }
}

fn read_ids<T: Field>(ids: &[Value]) -> Result<Vec<T>> {
    ids.iter()
        .map(|value| {
            T::from_value(value.clone())
                .map_err(|error| crate::row::decode_error("pivot", "id", T::type_name(), &error))
        })
        .collect()
}

/// `id` in the form `R`'s key stores it, when the related key is `R`'s key
/// (so ids compare whatever integer width they were given in).
pub(crate) fn canonical_id<R: Model>(related_key: &str, id: Value) -> Value {
    if related_key != R::meta().key_column() {
        return id;
    }
    match R::Key::from_value(id.clone()) {
        Ok(key) => key.to_value(),
        Err(_) => id,
    }
}

/// Pivot values given as assignments, as `(column, value)` pairs.
pub(crate) fn assigned(values: impl IntoIterator<Item = Assignment>) -> Result<Vec<(String, Value)>> {
    values
        .into_iter()
        .map(|assignment| match assignment.assigned_value() {
            Some(value) => Ok((
                crate::ir::ColumnRef::parse(assignment.column().to_owned())
                    .name
                    .into_owned(),
                value.clone(),
            )),
            None => Err(OrmError::invalid(format!(
                "the pivot value of `{}` must be a value, not an expression",
                assignment.column()
            ))),
        })
        .collect()
}
