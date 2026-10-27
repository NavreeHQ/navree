//! Defining relations: the [`Relationships`] methods every model has
//! (`self.belongs_to(..)`, `self.has_many(..)`, ...), and [`KeyColumn`],
//! the columns they take.

use std::borrow::Cow;

use crate::column::Column;
use crate::model::Model;
use crate::relations::belongs_to::BelongsTo;
use crate::relations::belongs_to_many::{BelongsToMany, Pivot};
use crate::relations::has::{HasMany, HasOne};
use crate::relations::morph::MorphTo;
use crate::relations::through::{HasManyThrough, HasOneThrough, PendingThrough};

/// A column of model `M` given to a relation: a typed column
/// (`Company::ID`) or a name (`"uuid"`).
pub trait KeyColumn<M> {
    /// The column's name.
    #[doc(hidden)]
    fn key_column(&self) -> Cow<'static, str>;
}

impl<M, T> KeyColumn<M> for Column<M, T> {
    fn key_column(&self) -> Cow<'static, str> {
        Cow::Owned(crate::ir::ColumnRef::parse(self.name().to_owned()).name.into_owned())
    }
}

impl<M> KeyColumn<M> for &'static str {
    fn key_column(&self) -> Cow<'static, str> {
        Cow::Borrowed(self)
    }
}

impl<M> KeyColumn<M> for String {
    fn key_column(&self) -> Cow<'static, str> {
        Cow::Owned(self.clone())
    }
}

/// The relation definitions of every model. Call them on `self` in a
/// `#[relations]` impl block (which names each relation after its method
/// and needs no import of this trait), or on a record for a one-off
/// relation.
///
/// ```
/// # use navree_orm::__doc_models::{Activity, Company, Tag, Uuid};
/// use navree_orm::prelude::*;
/// use navree_orm::relations::{BelongsTo, BelongsToMany, HasMany};
///
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// pub struct Deal {
///     pub id: Uuid,
///     pub company_id: Option<i64>,
///     pub title: String,
/// }
///
/// #[relations]
/// impl Deal {
///     /// The company the deal is with.
///     pub fn company(&self) -> BelongsTo<Deal, Company> {
///         self.belongs_to(Deal::COMPANY_ID)
///     }
///
///     /// The deal's activities, newest first.
///     pub fn activities(&self) -> HasMany<Deal, Activity> {
///         self.has_many(Activity::DEAL_ID).latest()
///     }
///
///     /// The deal's tags, through the `deal_tag` pivot table.
///     pub fn tags(&self) -> BelongsToMany<Deal, Tag> {
///         self.belongs_to_many("deal_tag").with_timestamps()
///     }
/// }
///
/// assert_eq!(Deal::relation_names(), ["company", "activities", "tags"]);
/// ```
pub trait Relationships: Model {
    /// The record of `R` this record's `foreign_key` column points at (by
    /// `R`'s key unless [`owner_key`](BelongsTo::owner_key) says
    /// otherwise): a deal's company.
    fn belongs_to<R: Model, T>(&self, foreign_key: Column<Self, T>) -> BelongsTo<Self, R> {
        BelongsTo::new(Some(self.clone()), KeyColumn::<Self>::key_column(&foreign_key))
    }

    /// The record of `R` whose `foreign_key` column holds this record's
    /// key (or [`local_key`](HasOne::local_key)): a user's profile.
    fn has_one<R: Model, T>(&self, foreign_key: Column<R, T>) -> HasOne<Self, R> {
        HasOne::new(Some(self.clone()), KeyColumn::<R>::key_column(&foreign_key))
    }

    /// The records of `R` whose `foreign_key` column holds this record's
    /// key: a deal's activities.
    fn has_many<R: Model, T>(&self, foreign_key: Column<R, T>) -> HasMany<Self, R> {
        HasMany::new(Some(self.clone()), KeyColumn::<R>::key_column(&foreign_key))
    }

    /// The records of `R` linked to this one by rows of pivot table
    /// `table`, whose columns `{self}_id` and `{related}_id` hold the two
    /// keys (see [`BelongsToMany`] for other names). The conventional
    /// table is both models' snake_case names in alphabetical order,
    /// joined by `_` (`deal_tag`).
    fn belongs_to_many<R: Model>(&self, table: impl Into<Cow<'static, str>>) -> BelongsToMany<Self, R, Pivot> {
        BelongsToMany::new(Some(self.clone()), table.into())
    }

    /// The record of `R` reached through model `T`: `T`'s `first_key`
    /// holds this record's key, and `R`'s `second_key` holds `T`'s key (a
    /// supplier's account history through its user).
    fn has_one_through<R: Model, T: Model, A, B>(
        &self,
        first_key: Column<T, A>,
        second_key: Column<R, B>,
    ) -> HasOneThrough<Self, R> {
        HasOneThrough::new(
            Some(self.clone()),
            T::meta(),
            KeyColumn::<T>::key_column(&first_key),
            KeyColumn::<R>::key_column(&second_key),
        )
    }

    /// The records of `R` reached through model `T`: a country's posts
    /// through its users.
    fn has_many_through<R: Model, T: Model, A, B>(
        &self,
        first_key: Column<T, A>,
        second_key: Column<R, B>,
    ) -> HasManyThrough<Self, R> {
        HasManyThrough::new(
            Some(self.clone()),
            T::meta(),
            KeyColumn::<T>::key_column(&first_key),
            KeyColumn::<R>::key_column(&second_key),
        )
    }

    /// The record of `R` whose `{name}_type` and `{name}_id` columns hold
    /// this record's morph name and key: a deal's image.
    fn morph_one<R: Model>(&self, name: &str) -> HasOne<Self, R> {
        HasOne::new_morph(Some(self.clone()), name)
    }

    /// The records of `R` whose `{name}_type` and `{name}_id` columns hold
    /// this record's morph name and key: a deal's comments.
    fn morph_many<R: Model>(&self, name: &str) -> HasMany<Self, R> {
        HasMany::new_morph(Some(self.clone()), name)
    }

    /// The record of any model this record's `{name}_type` and `{name}_id`
    /// columns point at: a comment's commentable.
    fn morph_to(&self, name: &str) -> MorphTo<Self> {
        MorphTo::new(Some(self.clone()), name)
    }

    /// The records of `R` linked to this one by rows of polymorphic pivot
    /// table `table` (`taggables`: `tag_id`, `taggable_id`,
    /// `taggable_type`), `name` being the polymorphic name (`taggable`).
    fn morph_to_many<R: Model>(
        &self,
        name: &str,
        table: impl Into<Cow<'static, str>>,
    ) -> BelongsToMany<Self, R, Pivot> {
        BelongsToMany::new_morph(Some(self.clone()), name, table.into(), false)
    }

    /// The inverse of [`morph_to_many`](Relationships::morph_to_many): the
    /// records of `R` that tagged this record (a tag's deals).
    fn morphed_by_many<R: Model>(
        &self,
        name: &str,
        table: impl Into<Cow<'static, str>>,
    ) -> BelongsToMany<Self, R, Pivot> {
        BelongsToMany::new_morph(Some(self.clone()), name, table.into(), true)
    }

    /// A through relation composed of two relations: `self.through(
    /// Deal::company).has(Company::contacts)` reaches a deal's contacts
    /// through its company.
    fn through<L>(&self, local: impl Fn(&Self) -> L) -> PendingThrough<Self, L> {
        PendingThrough::new(self.clone(), local(self))
    }
}

impl<M: Model> Relationships for M {}
