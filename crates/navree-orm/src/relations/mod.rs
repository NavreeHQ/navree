//! Relationships between models.
//!
//! A relation is a method of the model returning a typed relation, in a
//! `#[relations]` impl block (which names each relation after its method,
//! so `"company"` and `Deal::company` mean the same relation):
//!
//! ```
//! # use navree_orm::__doc_models::{Activity, Company, Tag, Uuid};
//! use navree_orm::prelude::*;
//! use navree_orm::relations::{BelongsTo, BelongsToMany, HasMany, HasOne};
//!
//! #[derive(Model, Clone, Debug, Default, serde::Serialize)]
//! pub struct Deal {
//!     pub id: Uuid,
//!     pub company_id: Option<i64>,
//!     pub title: String,
//! }
//!
//! #[relations]
//! impl Deal {
//!     pub fn company(&self) -> BelongsTo<Deal, Company> {
//!         self.belongs_to(Deal::COMPANY_ID)
//!     }
//!
//!     pub fn activities(&self) -> HasMany<Deal, Activity> {
//!         self.has_many(Activity::DEAL_ID)
//!     }
//!
//!     pub fn latest_activity(&self) -> HasOne<Deal, Activity> {
//!         self.has_many(Activity::DEAL_ID).latest_of_many_by(Activity::CREATED_AT)
//!     }
//!
//!     pub fn tags(&self) -> BelongsToMany<Deal, Tag> {
//!         self.belongs_to_many("deal_tag").with_pivot(["role"]).with_timestamps()
//!     }
//! }
//! # let _ = Deal::query().with("company");
//! ```
//!
//! The kinds: [`BelongsTo`], [`HasOne`] and [`HasMany`] (polymorphic:
//! [`MorphOne`], [`MorphMany`]), [`BelongsToMany`] with its pivots
//! ([`Pivot`], typed pivot models, [`MorphToMany`]), [`HasOneThrough`] and
//! [`HasManyThrough`] (also composed: `self.through(Deal::company)
//! .has(Company::contacts)`), and [`MorphTo`] with the [`MorphMap`].
//!
//! What relations do:
//!
//! - **Query** the related records of a record: every builder method, then
//!   any retrieval: `deal.activities().where_(..).latest().get(&db)`. The
//!   relation's own condition comes first and the caller's are grouped, so
//!   an `or_where` never reaches other parents' records.
//! - **Eager load** into a query's records, one batched query per relation
//!   and level: `with("company.owner")`, constrained
//!   (`with_constrained(Deal::activities, |q| q.latest().limit(3))`, the
//!   limit per parent), with a column selection (`"company:id,name"`); or
//!   afterwards onto a record or collection (`load`, `load_missing`).
//! - **Check existence** in queries: `has`, `where_has`, `doesnt_have`,
//!   `where_relation`, `where_belongs_to`, `where_attached_to`, and the
//!   polymorphic forms (`where_has_morph`, `where_morphed_to`).
//! - **Aggregate**: `with_count`, `with_sum`, `with_avg`, `with_min`,
//!   `with_max`, `with_exists` (extras of the records), and `load_count`
//!   and friends afterwards.
//! - **Write**: `save`, `create`, `make` through to-many relations;
//!   `associate` and `dissociate` on records; `attach`, `detach`, `sync`,
//!   `toggle` and `update_existing_pivot` on many-to-many relations; parents
//!   touched by `#[model(touches = [..])]`.
//!
//! Loaded relations live on the record ([`Record::relation`],
//! [`Record::loaded`]), serialize with it, and are saved by
//! [`Record::push`]. [`LazyLoading`] decides what loading a relation of one
//! record at a time does.
//!
//! [`Record::relation`]: crate::Record::relation
//! [`Record::loaded`]: crate::Record::loaded
//! [`Record::push`]: crate::Record::push

use crate::model::Model;

pub(crate) mod access;
pub(crate) mod aggregates;
mod any;
mod belongs_to;
mod belongs_to_many;
pub(crate) mod core;
mod define;
pub(crate) mod eager;
pub(crate) mod existence;
mod has;
pub(crate) mod keys;
pub(crate) mod lazy;
pub(crate) mod morph;
mod morph_map;
mod one_of_many;
mod path;
mod pivot;
pub(crate) mod registry;
mod requalify;
mod store;
mod through;
pub(crate) mod touch;

pub use access::AccessFilter;
pub use any::AnyRecord;
pub use belongs_to::BelongsTo;
pub use belongs_to_many::{BelongsToMany, MorphToMany, joining_table};
pub use define::{KeyColumn, Relationships};
pub use has::{HasMany, HasOne, MorphMany, MorphOne};
pub use lazy::{Associate, Dissociate, LazyLoading, LazyLoadingViolation, load_missing_chain};
pub use morph::{MorphRef, MorphTargets, MorphTo, MorphToOps, MorphTypes, MorphWith};
pub use morph_map::MorphMap;
pub use path::{ConstrainPath, RelationPath, RelationPathTo};
pub use pivot::{Pivot, PivotKind, RelatedId, SyncChanges};
pub use registry::{DynRelation, RelationKind, RelationMeta, RelationRegistry};
pub use store::{RelationValue, Relations};
pub use through::{
    Cardinality, HasManyThrough, HasOneThrough, Many, One, PendingThrough, ThroughLeg, ThroughOf,
};
pub use touch::{TouchList, is_ignoring_touch, without_touching, without_touching_on};

pub(crate) mod private {
    use std::borrow::Cow;
    use std::sync::Arc;

    use crate::model::Model;
    use crate::relations::registry::ErasedRelation;

    /// What only the ORM's relation types do: erase their types, take a
    /// name.
    pub trait Erase<P: Model> {
        /// The relation, its types erased, without its parent.
        fn erase(self) -> Arc<dyn ErasedRelation<P>>;

        /// Renames the relation.
        fn rename(&mut self, name: Cow<'static, str>);
    }
}

/// A relation from model `P`: one of the relation types ([`BelongsTo`],
/// [`HasMany`], [`BelongsToMany`], [`MorphTo`], ...), as a relation method
/// returns it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a relation of `{P}`",
    label = "a relation method returns a relation",
    note = "relation methods return a relation built from `self`: `self.belongs_to(..)` (`BelongsTo<{P}, R>`), `self.has_many(..)` (`HasMany<{P}, R>`), `self.belongs_to_many(..)`, `self.morph_to(..)`, ..."
)]
pub trait Relation<P: Model>: Clone + Send + Sync + 'static + private::Erase<P> {
    /// What the relation holds once loaded on a record:
    /// `Option<Record<R>>`, [`Collection<R>`](crate::Collection), or
    /// `Option<AnyRecord>`.
    type Value: RelationValue;

    /// The relation's name: what it is loaded and serialized under, and its
    /// step in relation paths.
    fn name(&self) -> &str;

    /// The relation's kind.
    fn kind(&self) -> RelationKind;

    /// What the relation is: its models, tables and keys.
    fn meta(&self) -> RelationMeta;
}

/// A relation from `P` to one model, [`Related`](RelatedTo::Related) (every
/// relation but `morph_to`).
pub trait RelatedTo<P: Model>: Relation<P> {
    /// The related model.
    type Related: Model;
}

/// What `#[relations]` expands to. Not part of the API.
#[doc(hidden)]
pub mod derive_support {
    use std::borrow::Cow;

    use crate::model::Model;
    use crate::relations::{DynRelation, Relation};

    /// Names `relation` (after its method).
    pub fn named<P: Model, R: Relation<P>>(mut relation: R, name: &'static str) -> R {
        crate::relations::private::Erase::rename(&mut relation, Cow::Borrowed(name));
        relation
    }

    /// `relation`, its types erased.
    pub fn erased<P: Model, R: Relation<P>>(relation: R) -> DynRelation<P> {
        DynRelation::new(relation)
    }

    /// The fallback when a model has no `#[relations]` block: no relations.
    pub trait NoRelations: Model {
        /// No relation is declared.
        fn __navree_relations(name: &str) -> Option<DynRelation<Self>> {
            let _ = name;
            None
        }

        /// No relation is declared.
        fn __navree_relation_names() -> &'static [&'static str] {
            &[]
        }
    }

    impl<M: Model> NoRelations for M {}

    /// Models with relations implement `Default`: relation definitions are
    /// read from a default instance.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` has relations, so it must implement `Default`",
        note = "relation definitions are read from a default instance: add `#[derive(Default)]` (or implement `Default`) for `{Self}`"
    )]
    pub trait ModelWithRelations: Model + Default {}

    impl<M: Model + Default> ModelWithRelations for M {}

    /// A default instance of `M`, for reading relation definitions.
    pub fn template<M: ModelWithRelations>() -> M {
        M::default()
    }

    /// The relations of `M` other modules defined, by name.
    pub fn defined<M: Model>(name: &str) -> Option<DynRelation<M>> {
        crate::relations::registry::defined::<M>(name)
    }

    /// The names of the relations other modules defined for `M`.
    pub fn defined_names<M: Model>() -> Vec<Cow<'static, str>> {
        crate::relations::registry::defined_names::<M>()
    }
}
