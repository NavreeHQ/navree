//! [`Record`]: a model instance together with what the ORM tracks about it.

use std::borrow::Cow;
use std::fmt;
use std::ops::{Deref, DerefMut};

use serde::{Serialize, Serializer};

use crate::attributes::CastContext;
use crate::attributes::state::RecordState;
use crate::column::{Assignment, Column, ColumnName};
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor};
use crate::model::Model;
use crate::persist;
use crate::relations::Relations;
use crate::row::Row;
use crate::value::{Field, Numeric, Value};

/// A model instance as the ORM knows it: the model itself (a `Record<M>`
/// dereferences to `M`, so fields read and write directly), whether it is
/// stored, the values it was last synced with (for dirty tracking), the
/// changes its last save made, the extra columns its query selected
/// (aggregates, computed columns) and its loaded relations.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, uow: Uow, id: Uuid) -> Result<()> {
/// let mut deal = Deal::find_or_fail(&db, id).await?;
/// deal.stage = Stage::Won;                 // DerefMut to the model
/// assert!(deal.is_dirty_on([Deal::STAGE]));
/// deal.save(&uow).await?;                  // UPDATE deals SET stage = .., updated_at = .. WHERE id = ..
/// assert!(deal.was_changed_on([Deal::STAGE]));
/// # Ok(()) }
/// ```
///
/// Records serialize as their model, followed by their extra columns and
/// loaded relations.
pub struct Record<M: Model> {
    pub(crate) model: M,
    pub(crate) original: Option<M>,
    pub(crate) exists: bool,
    pub(crate) recently_created: bool,
    pub(crate) changes: Vec<(&'static str, Value)>,
    pub(crate) previous: Vec<(&'static str, Value)>,
    pub(crate) extras: Vec<(String, Value)>,
    pub(crate) relations: Relations,
    /// Set while `force_delete` runs (see `is_force_deleting`).
    pub(crate) force_deleting: bool,
    /// The lazy-loading rules' marks (see
    /// [`LazyLoading`](crate::relations::LazyLoading)).
    pub(crate) loading: crate::relations::lazy::LoadState<M>,
    pub(crate) attributes: RecordState,
}

impl<M: Model> Record<M> {
    /// A new, unsaved record of `model`. Every column is dirty until it is
    /// saved.
    pub fn new(model: M) -> Self {
        Record {
            model,
            original: None,
            exists: false,
            recently_created: false,
            changes: Vec::new(),
            previous: Vec::new(),
            extras: Vec::new(),
            relations: Relations::new(),
            force_deleting: false,
            loading: crate::relations::lazy::LoadState::default(),
            attributes: RecordState::default(),
        }
    }

    /// A record of `model` as it is stored (a parent a relation was made
    /// from).
    pub(crate) fn stored(model: M) -> Self {
        Record {
            original: Some(model.clone()),
            model,
            exists: true,
            recently_created: false,
            changes: Vec::new(),
            previous: Vec::new(),
            extras: Vec::new(),
            relations: Relations::new(),
            force_deleting: false,
            loading: crate::relations::lazy::LoadState::default(),
            attributes: RecordState::default(),
        }
    }

    /// A stored record read from `row`: the model's columns, and the other
    /// selected columns as extras.
    pub(crate) fn from_row(row: &Row) -> Result<Self> {
        let model = M::from_row(row)?;
        let meta = M::meta();
        let mut extras = Vec::new();
        for column in row.columns() {
            if !meta.has_column(&column) {
                let value = row.value(&column)?;
                extras.push((column, value));
            }
        }
        Ok(Record {
            original: Some(model.clone()),
            model,
            exists: true,
            recently_created: false,
            changes: Vec::new(),
            previous: Vec::new(),
            extras,
            relations: Relations::new(),
            force_deleting: false,
            loading: crate::relations::lazy::LoadState::default(),
            attributes: RecordState {
                stale: M::stale_columns(row),
                context: if meta.uses_cast_context() {
                    CastContext::scoped()
                } else {
                    None
                },
                ..RecordState::default()
            },
        })
    }

    /// Runs `f` with the cast context the record was read or saved under,
    /// when no scope is running (see [`CastContext`]).
    pub(crate) fn in_cast_context<R>(&self, f: impl FnOnce() -> R) -> R {
        CastContext::with_record(self.attributes.context.as_ref(), f)
    }

    /// Keeps the running scope's cast context as the record's (after a
    /// save), for models whose casts use one.
    pub(crate) fn keep_cast_context(&mut self) {
        if M::meta().uses_cast_context()
            && let Some(context) = CastContext::scoped()
        {
            self.attributes.context = Some(context);
        }
    }

    /// The model.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// The model, for changes.
    pub fn model_mut(&mut self) -> &mut M {
        &mut self.model
    }

    /// The model, without the record's bookkeeping.
    pub fn into_inner(self) -> M {
        self.model
    }

    /// The key.
    pub fn key(&self) -> M::Key {
        self.model.key()
    }

    /// Whether the record is stored in the database.
    pub fn exists(&self) -> bool {
        self.exists
    }

    /// Whether the record was inserted by this instance's last save.
    pub fn was_recently_created(&self) -> bool {
        self.recently_created
    }

    /// Whether `other` is the same stored record (same model, same key).
    pub fn is(&self, other: &Record<M>) -> bool {
        self.key() == other.key()
    }

    /// Whether `other` is a different record.
    pub fn is_not(&self, other: &Record<M>) -> bool {
        !self.is(other)
    }

    /// Whether the record is soft-deleted (its model soft-deletes and its
    /// deleted-at column is set).
    pub fn trashed(&self) -> bool {
        M::meta()
            .deleted_at()
            .is_some_and(|column| !self.model.timestamp_is_empty(column))
    }

    // ---------------------------------------------------------------------
    // Dirty tracking
    // ---------------------------------------------------------------------

    /// The columns whose values differ from the original (every column of
    /// a record that was never saved), with their current stored values.
    ///
    /// Values are compared as stored: decimals numerically, floats within
    /// a small tolerance, date-times by instant, JSON structurally, cast
    /// fields as their cast compares them (encrypted fields by their plain
    /// value). An encrypted column read under a previous key is dirty until
    /// it is saved again (and so re-encrypted with the current key).
    pub fn get_dirty(&self) -> Result<Vec<(&'static str, Value)>> {
        self.in_cast_context(|| {
            let columns = match &self.original {
                Some(_) => self.dirty_columns()?,
                None => return self.model.values(),
            };
            columns
                .into_iter()
                .map(|column| {
                    let value = self
                        .model
                        .value(column)?
                        .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(column))?;
                    Ok((column, value))
                })
                .collect()
        })
    }

    /// The dirty columns, in field order, without encoding their values.
    fn dirty_columns(&self) -> Result<Vec<&'static str>> {
        let Some(original) = &self.original else {
            return Ok(M::meta().all_columns().iter().map(|c| c.name()).collect());
        };
        let mut columns = self.in_cast_context(|| self.model.changed_columns(original))?;
        if !self.attributes.stale.is_empty() {
            for &column in &self.attributes.stale {
                if !columns.contains(&column) {
                    columns.push(column);
                }
            }
            let order = M::meta().all_columns();
            columns.sort_by_key(|column| order.iter().position(|meta| meta.name() == *column));
        }
        Ok(columns)
    }

    /// Whether any column changed since the record was read or saved. A
    /// column that cannot be compared counts as changed (saving reports
    /// the error).
    pub fn is_dirty(&self) -> bool {
        self.dirty_columns().map_or(true, |dirty| !dirty.is_empty())
    }

    /// Whether any of `columns` changed since the record was read or saved.
    pub fn is_dirty_on<C: ColumnName>(&self, columns: impl IntoIterator<Item = C>) -> bool {
        match self.dirty_columns() {
            Ok(dirty) => columns
                .into_iter()
                .any(|column| dirty.iter().any(|name| *name == column.column_name())),
            Err(_) => true,
        }
    }

    /// Whether no column changed since the record was read or saved.
    pub fn is_clean(&self) -> bool {
        !self.is_dirty()
    }

    /// Whether none of `columns` changed since the record was read or saved.
    pub fn is_clean_on<C: ColumnName>(&self, columns: impl IntoIterator<Item = C>) -> bool {
        !self.is_dirty_on(columns)
    }

    /// Whether the last save changed any column.
    pub fn was_changed(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Whether the last save changed any of `columns`.
    pub fn was_changed_on<C: ColumnName>(&self, columns: impl IntoIterator<Item = C>) -> bool {
        columns.into_iter().any(|column| {
            let column = column.column_name();
            self.changes.iter().any(|(name, _)| *name == column)
        })
    }

    /// The columns the last save changed, with their new values.
    pub fn get_changes(&self) -> &[(&'static str, Value)] {
        &self.changes
    }

    /// The columns the last save changed, with the values they had before.
    pub fn get_previous(&self) -> &[(&'static str, Value)] {
        &self.previous
    }

    /// The model as it was last read or saved; `None` for a record that
    /// was never stored.
    pub fn get_original(&self) -> Option<&M> {
        self.original.as_ref()
    }

    /// The stored value column `column` had when the record was last read
    /// or saved; `None` for a new record or a column the model lacks.
    pub fn get_original_value(&self, column: impl ColumnName) -> Result<Option<Value>> {
        match &self.original {
            Some(original) => self.in_cast_context(|| original.value(&column.column_name())),
            None => Ok(None),
        }
    }

    /// Accepts the current values as the original ones (nothing is dirty
    /// afterwards).
    pub fn sync_original(&mut self) {
        self.original = Some(self.model.clone());
        self.attributes.stale.clear();
    }

    /// Accepts the current values of `columns` as their original ones.
    pub fn sync_original_on<C: ColumnName>(
        &mut self,
        columns: impl IntoIterator<Item = C>,
    ) -> Result<()> {
        let Some(original) = &mut self.original else {
            return Ok(());
        };
        let context = self.attributes.context.clone();
        for column in columns {
            let name = column.column_name();
            CastContext::with_record(context.as_ref(), || -> Result<()> {
                if let Some(value) = self.model.value(&name)? {
                    original.set_value(&name, value)?;
                }
                Ok(())
            })?;
            self.attributes.stale.retain(|stale| *stale != name);
        }
        Ok(())
    }

    /// Puts the original values back, undoing unsaved changes (a new
    /// record is left as it is).
    pub fn discard_changes(&mut self) {
        if let Some(original) = &self.original {
            self.model = original.clone();
        }
    }

    /// Records the dirty columns as the changes of a save.
    pub(crate) fn sync_changes(&mut self, dirty: Vec<(&'static str, Value)>) -> Result<()> {
        let mut previous = Vec::with_capacity(dirty.len());
        for (column, _) in &dirty {
            let before = match &self.original {
                Some(original) => original.value(column)?.unwrap_or(Value::String(None)),
                None => Value::String(None),
            };
            previous.push((*column, before));
        }
        self.changes = dirty;
        self.previous = previous;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Extras and relations
    // ---------------------------------------------------------------------

    /// The selected columns that are not model columns (aggregates,
    /// expressions, joined columns), in the order of the select list.
    pub fn extras(&self) -> &[(String, Value)] {
        &self.extras
    }

    /// Extra column `name` read as `T`; `None` when the query did not
    /// select it.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let deals = Deal::query().select_raw("amount_cents * 2 AS doubled", [] as [i64; 0]).get(&db).await?;
    /// let doubled: Option<i64> = deals[0].extra("doubled")?;
    /// # Ok(()) }
    /// ```
    pub fn extra<T: Field>(&self, name: &str) -> Result<Option<T>> {
        match self.extras.iter().find(|(column, _)| column == name) {
            Some((_, value)) => T::from_value(value.clone()).map(Some).map_err(|error| {
                crate::row::decode_error(M::meta().name(), name, T::type_name(), &error)
            }),
            None => Ok(None),
        }
    }

    /// Sets extra column `name` (replacing it).
    pub fn set_extra(&mut self, name: impl Into<String>, value: impl Into<Value>) {
        let name = name.into();
        let value = value.into();
        match self.extras.iter_mut().find(|(column, _)| *column == name) {
            Some(existing) => existing.1 = value,
            None => self.extras.push((name, value)),
        }
    }

    /// The loaded relations.
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// The loaded relations, for changes.
    pub fn relations_mut(&mut self) -> &mut Relations {
        &mut self.relations
    }

    // ---------------------------------------------------------------------
    // Persistence
    // ---------------------------------------------------------------------

    /// Inserts the record when it is new, updates its dirty columns when
    /// it is stored (a clean record writes nothing). Returns `false` when a
    /// lifecycle hook cancelled the save.
    ///
    /// Inserting generates a missing key (UUID v7, UUID v4 or ULID, by the
    /// model's key type; the database assigns incrementing keys, which are
    /// read back) and fills empty `created_at` / `updated_at` columns;
    /// updating sets `updated_at` unless the caller changed it. Inside a
    /// unit of work, the change is recorded for its observers (the audit
    /// trail).
    pub async fn save<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        persist::save(&target, self).await
    }

    /// Changes the stored record with `change`, then saves it. Returns
    /// `false` (and changes nothing) for a record that is not stored, or
    /// when a hook cancelled the save.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, mut deal: Record<Deal>) -> Result<()> {
    /// deal.update(&uow, |d| d.stage = Stage::Won).await?;
    /// # Ok(()) }
    /// ```
    pub async fn update<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        change: impl FnOnce(&mut M) + Send,
    ) -> Result<bool> {
        if !self.exists {
            return Ok(false);
        }
        change(&mut self.model);
        self.save(exec).await
    }

    /// Deletes the stored record (for a soft-deleting model: sets its
    /// deleted-at column). Returns `false` for a record that is not stored,
    /// or when a hook cancelled the delete.
    pub async fn delete<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        persist::delete(&target, self).await
    }

    /// Reloads the record's columns from the database (ignoring global
    /// scopes, so a soft-deleted record reloads too); nothing happens for a
    /// record that is not stored. [`OrmError::ModelNotFound`] when the row
    /// is gone.
    pub async fn refresh<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        persist::refresh(&target, self).await
    }

    /// The record as it is now in the database (ignoring global scopes);
    /// `None` when it is gone or was never stored.
    pub async fn fresh<E: Executor + ?Sized>(&self, exec: &E) -> Result<Option<Record<M>>> {
        let target = exec.target(Access::Read).await?;
        persist::fresh(&target, self).await
    }

    /// An unsaved copy of the record: the same column values, without the
    /// key (a new one is generated when it is saved) and the creation and
    /// update timestamps. Loaded relations and extras are not copied.
    pub fn replicate(&self) -> Record<M> {
        let mut model = self.model.clone();
        model.set_key(<M::Key as crate::model::PrimaryKey>::empty());
        let meta = M::meta();
        for column in [meta.created_at(), meta.updated_at()].into_iter().flatten() {
            model.clear_timestamp(column);
        }
        Record::new(model)
    }

    /// Sets the update timestamp to now and saves the record (with any
    /// other changes). Returns `false` when the model keeps no update
    /// timestamp.
    pub async fn touch<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        persist::touch(&target, self, None).await
    }

    /// Sets date-time column `column` to now and saves the record.
    pub async fn touch_column<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        column: impl ColumnName,
    ) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        let column = column.column_name().into_owned();
        persist::touch(&target, self, Some(&column)).await
    }

    /// Adds `amount` to `column` in the database (`SET column = column +
    /// amount`, plus the update timestamp) and in the record. Returns
    /// `false` when a hook cancelled it.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, mut deal: Record<Deal>) -> Result<()> {
    /// deal.increment(&db, Deal::VIEWS, 1).await?;
    /// # Ok(()) }
    /// ```
    pub async fn increment<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        self.increment_with(exec, column, amount, std::iter::empty())
            .await
    }

    /// Like [`increment`](Record::increment), also setting `extra`
    /// columns.
    pub async fn increment_with<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
        extra: impl IntoIterator<Item = Assignment>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        let extra: Vec<Assignment> = extra.into_iter().collect();
        let target = exec.target(Access::Write).await?;
        persist::increment(&target, self, vec![(column, amount)], false, extra).await
    }

    /// Adds each amount to its column in one statement (`SET a = a + 1, b
    /// = b + 5`, plus the update timestamp), in the database and in the
    /// record. A column may appear once. Returns `false` when a hook
    /// cancelled it.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, mut deal: Record<Deal>) -> Result<()> {
    /// deal.increment_each(&db, [(Deal::VIEWS, 1), (Deal::AMOUNT_CENTS, 500)]).await?;
    /// # Ok(()) }
    /// ```
    pub async fn increment_each<E, T>(
        &mut self,
        exec: &E,
        steps: impl IntoIterator<Item = (Column<M, T>, T)>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        let steps: Vec<(Column<M, T>, T)> = steps.into_iter().collect();
        let target = exec.target(Access::Write).await?;
        persist::increment(&target, self, steps, false, Vec::new()).await
    }

    /// Subtracts each amount from its column in one statement, in the
    /// database and in the record.
    pub async fn decrement_each<E, T>(
        &mut self,
        exec: &E,
        steps: impl IntoIterator<Item = (Column<M, T>, T)>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        let steps: Vec<(Column<M, T>, T)> = steps.into_iter().collect();
        let target = exec.target(Access::Write).await?;
        persist::increment(&target, self, steps, true, Vec::new()).await
    }

    /// Subtracts `amount` from `column` in the database and in the record.
    pub async fn decrement<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        self.decrement_with(exec, column, amount, std::iter::empty())
            .await
    }

    /// Like [`decrement`](Record::decrement), also setting `extra`
    /// columns.
    pub async fn decrement_with<E, T>(
        &mut self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
        extra: impl IntoIterator<Item = Assignment>,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        let extra: Vec<Assignment> = extra.into_iter().collect();
        let target = exec.target(Access::Write).await?;
        persist::increment(&target, self, vec![(column, amount)], true, extra).await
    }

    /// The error for an operation that needs a stored record.
    pub(crate) fn not_persisted(action: &'static str) -> OrmError {
        OrmError::NotPersisted {
            model: Cow::Borrowed(M::meta().name()),
            action,
        }
    }
}

impl<M: Model> Deref for Record<M> {
    type Target = M;

    fn deref(&self) -> &M {
        &self.model
    }
}

impl<M: Model> DerefMut for Record<M> {
    fn deref_mut(&mut self) -> &mut M {
        &mut self.model
    }
}

impl<M: Model> From<M> for Record<M> {
    fn from(model: M) -> Self {
        Record::new(model)
    }
}

impl<M: Model> Clone for Record<M> {
    fn clone(&self) -> Self {
        Record {
            model: self.model.clone(),
            original: self.original.clone(),
            exists: self.exists,
            recently_created: self.recently_created,
            changes: self.changes.clone(),
            previous: self.previous.clone(),
            extras: self.extras.clone(),
            relations: self.relations.clone(),
            force_deleting: self.force_deleting,
            loading: self.loading.clone(),
            attributes: self.attributes.clone(),
        }
    }
}

impl<M: Model + fmt::Debug> fmt::Debug for Record<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Record");
        debug
            .field("model", &self.model)
            .field("exists", &self.exists);
        if !self.extras.is_empty() {
            debug.field("extras", &self.extras);
        }
        if !self.relations.is_empty() {
            debug.field("relations", &self.relations);
        }
        debug.finish_non_exhaustive()
    }
}

/// The model (as its own `Serialize` writes it, with cast outputs and the
/// model's date format), then the extra columns, the appended accessors and
/// the loaded relations, after the record's visibility (see
/// [`attributes::serialize`](crate::attributes::serialize)).
impl<M: Model> Serialize for Record<M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        crate::attributes::serialize::serialize_record(self, serializer)
    }
}
