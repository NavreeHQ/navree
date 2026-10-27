//! Soft deletes: trashing, restoring and removing records for good.
//!
//! A soft-deleting model (`#[model(soft_deletes)]`) keeps a nullable
//! deleted-at column. Deleting a record sets it (and the update timestamp)
//! instead of removing the row, and fires `deleting`, `trashed`, `deleted`;
//! queries leave trashed rows out through the [`SoftDeletes`] scope.
//!
//! | Record | Builder (every matching row, one statement, no events) |
//! |---|---|
//! | [`delete`](Record::delete): trash | [`delete`](Builder::delete): trash |
//! | [`restore`](Record::restore): `restoring`, save, `restored` | [`restore`](Builder::restore) |
//! | [`force_delete`](Record::force_delete): `force_deleting`, `deleting`, delete, `deleted`, `force_deleted` | [`force_delete`](Builder::force_delete) |
//! | [`trashed`](Record::trashed), [`is_force_deleting`](Record::is_force_deleting) | [`with_trashed`](Builder::with_trashed), [`with_trashed_if`](Builder::with_trashed_if), [`without_trashed`](Builder::without_trashed), [`only_trashed`](Builder::only_trashed) |
//!
//! Restoring needs a soft-deleting model ([`SoftDeleting`], written by the
//! derive), so `company.restore(..)` does not compile for a model without
//! soft deletes. Force deleting works on every model: without soft deletes
//! it is a plain delete. Generic code may call `with_trashed`,
//! `with_trashed_if` and `without_trashed` on any model (they do nothing
//! without soft deletes).
//!
//! [`SoftDeletes`]: crate::SoftDeletes

use std::borrow::Cow;

use crate::column::Assignment;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::events::without_events;
use crate::hooks::{Flow, Hooks, ModelEvent};
use crate::model::Model;
use crate::persist;
use crate::query::Builder;
use crate::record::Record;
use crate::scope::Trashed;

/// A model that soft-deletes: `#[model(soft_deletes)]` implements it. It
/// unlocks [`Record::restore`], [`Builder::only_trashed`],
/// [`Builder::restore`] and the restore-or-create family.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not soft-delete",
    note = "add `soft_deletes` to `#[model(..)]` on `{Self}` (with an `Option<DateTime<Utc>>` deleted-at field)"
)]
pub trait SoftDeleting: Model {}

impl<M: Model> Record<M> {
    /// Removes the stored record for good, also when its model
    /// soft-deletes: `force_deleting` (cancellable), then the delete flow
    /// (`deleting`, `DELETE`, `deleted`, with
    /// [`is_force_deleting`](Record::is_force_deleting) `true`), then
    /// `force_deleted`. On a model without soft deletes it is
    /// [`delete`](Record::delete). Returns `false` for a record that is not
    /// stored, or when a listener cancelled it.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, mut note: Record<Note>) -> Result<()> {
    /// note.delete(&uow).await?;       // trashed: the row stays
    /// note.force_delete(&uow).await?; // gone
    /// # Ok(()) }
    /// ```
    pub async fn force_delete<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        force_delete(&target, self).await
    }

    /// [`force_delete`](Record::force_delete) without model events.
    pub async fn force_delete_quietly<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        without_events(self.force_delete(exec)).await
    }

    /// Whether a force delete of this record is under way: `true` while
    /// listeners hear its `deleting` and `deleted` events, so they can tell
    /// a removal for good from a soft delete.
    pub fn is_force_deleting(&self) -> bool {
        self.force_deleting
    }
}

impl<M: SoftDeleting> Record<M> {
    /// Brings a soft-deleted record back: `restoring` (cancellable), then
    /// the deleted-at column is cleared and the record saved (`saving`,
    /// `updating`, `updated`, `saved`, with a new update timestamp), then
    /// `restored`. Returns `false` for a record that is not stored, or when
    /// a listener cancelled it.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, id: i64) -> Result<()> {
    /// let mut note = Note::query().only_trashed().find_or_fail(&uow, id).await?;
    /// note.restore(&uow).await?;
    /// # Ok(()) }
    /// ```
    pub async fn restore<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        restore(&target, self).await
    }

    /// [`restore`](Record::restore) without model events.
    pub async fn restore_quietly<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        without_events(self.restore(exec)).await
    }
}

/// The force delete flow.
async fn force_delete<M: Model>(target: &Target<'_>, record: &mut Record<M>) -> Result<bool> {
    if !M::meta().soft_deletes_enabled() {
        return persist::delete(target, record).await;
    }
    if !record.exists {
        return Ok(false);
    }
    let ctx = target.context();
    let hooks = Hooks::<M>::resolve(&ctx).await?;
    if hooks.before(ModelEvent::ForceDeleting, &ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    record.force_deleting = true;
    let deleted = persist::delete(target, record).await;
    record.force_deleting = false;
    if !deleted? {
        return Ok(false);
    }
    hooks.after(ModelEvent::ForceDeleted, &ctx, record).await?;
    Ok(true)
}

/// The restore flow.
async fn restore<M: SoftDeleting>(target: &Target<'_>, record: &mut Record<M>) -> Result<bool> {
    let deleted_at = deleted_at_column::<M>()?;
    if !record.exists {
        return Ok(false);
    }
    let ctx = target.context();
    let hooks = Hooks::<M>::resolve(&ctx).await?;
    if hooks.before(ModelEvent::Restoring, &ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    record.model.clear_timestamp(deleted_at);
    if !persist::save(target, record).await? {
        return Ok(false);
    }
    hooks.after(ModelEvent::Restored, &ctx, record).await?;
    Ok(true)
}

/// The deleted-at column of a soft-deleting model.
fn deleted_at_column<M: SoftDeleting>() -> Result<&'static str> {
    M::meta().deleted_at().ok_or_else(|| {
        OrmError::invalid(format!(
            "{} implements SoftDeleting but declares no soft-delete column: add `soft_deletes` to its #[model(..)]",
            M::meta().name()
        ))
    })
}

impl<M: Model> Builder<M> {
    /// Includes soft-deleted records when `with` is `true`, leaves them out
    /// otherwise (as [`without_trashed`](Builder::without_trashed)).
    pub fn with_trashed_if(self, with: bool) -> Self {
        if with {
            self.with_trashed()
        } else {
            self.without_trashed()
        }
    }

    /// Leaves soft-deleted records out, also when other global scopes are
    /// removed (undoes [`with_trashed`](Builder::with_trashed)). Does
    /// nothing for a model without soft deletes.
    pub fn without_trashed(mut self) -> Self {
        self.scopes.trashed = Trashed::Excluded;
        self
    }

    /// Removes every matching row for good in one statement, soft-deleted
    /// rows included (unless the query chose otherwise with
    /// [`without_trashed`](Builder::without_trashed) or
    /// [`only_trashed`](Builder::only_trashed)); the other global scopes
    /// still apply. No model events fire. On a model without soft deletes
    /// it is [`delete`](Builder::delete).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let purged = Note::query().only_trashed().force_delete(&db).await?;
    /// # Ok(()) }
    /// ```
    pub async fn force_delete<E: Executor + ?Sized>(mut self, exec: &E) -> Result<u64> {
        if self.scopes.trashed == Trashed::Hidden {
            self.scopes.trashed = Trashed::Included;
        }
        self.delete_rows(exec).await
    }
}

impl<M: SoftDeleting> Builder<M> {
    /// Only soft-deleted records.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// # fn main() -> Result<()> {
    /// let sql = Note::query().only_trashed().to_sql(Dialect::Postgres)?.sql;
    /// assert!(sql.ends_with(r#"WHERE "notes"."deleted_at" IS NOT NULL"#));
    /// # Ok(()) }
    /// ```
    pub fn only_trashed(mut self) -> Self {
        self.scopes.trashed = Trashed::Only;
        self
    }

    /// Restores every soft-deleted row the query matches in one statement
    /// (clearing the deleted-at column and setting the update timestamp);
    /// live rows are left alone. Returns how many rows were restored. No
    /// model events fire: restore records one by one when listeners must
    /// hear about each.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, since: DateTime<Utc>) -> Result<()> {
    /// let restored = Note::query()
    ///     .where_(Note::DELETED_AT.gte(since))
    ///     .restore(&uow)
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn restore<E: Executor + ?Sized>(mut self, exec: &E) -> Result<u64> {
        let deleted_at = deleted_at_column::<M>()?;
        let null = M::timestamp_value(deleted_at, chrono::DateTime::UNIX_EPOCH)
            .map(|value| value.as_null())
            .ok_or_else(|| crate::model::derive_support::not_a_timestamp::<M>(deleted_at))?;
        if self.scopes.trashed != Trashed::Excluded {
            self.scopes.trashed = Trashed::Only;
        }
        self.update(exec, [Assignment::value(Cow::Borrowed(deleted_at), null)])
            .await
    }

    /// The record matching `attributes`, soft-deleted or not, restored when
    /// it was trashed; or a new one with them (and whatever `fill` sets)
    /// inserted.
    pub async fn restore_or_create<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut M) + Send,
    ) -> Result<Record<M>>
    where
        M: Default,
    {
        let mut record = self.with_trashed().first_or_create(exec, attributes, fill).await?;
        if record.trashed() {
            record.restore(exec).await?;
        }
        Ok(record)
    }

    /// Inserts a record with `attributes` (and whatever `fill` sets); when
    /// that violates a unique constraint, the record that holds them
    /// instead, restored when it was trashed.
    pub async fn create_or_restore<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut M) + Send,
    ) -> Result<Record<M>>
    where
        M: Default,
    {
        let mut record = self.with_trashed().create_or_first(exec, attributes, fill).await?;
        if record.trashed() {
            record.restore(exec).await?;
        }
        Ok(record)
    }
}
