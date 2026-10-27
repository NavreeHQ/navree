//! Relations by name: [`DynRelation`] (a relation with its types erased,
//! what relation paths such as `"company.owner"` resolve to),
//! [`RelationMeta`], and the [`RelationRegistry`] of relations other
//! modules add to a model.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock, RwLock};

use navree_kernel::BoxFuture;

use crate::error::{OrmError, Result};
use crate::exec::Target;
use crate::ir::QueryIr;
use crate::model::Model;
use crate::record::Record;
use crate::relations::eager::{EagerNode, EagerTree};
use crate::relations::existence::ExistsRequest;
use crate::relations::morph::MorphToOps;
use crate::relations::{Relation, private};

/// The kind of a relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RelationKind {
    /// The parent holds the key of one related record (`belongs_to`).
    BelongsTo,
    /// One related record holds the parent's key (`has_one`).
    HasOne,
    /// Related records hold the parent's key (`has_many`).
    HasMany,
    /// Records related through a pivot table (`belongs_to_many`).
    BelongsToMany,
    /// One record reached through an intermediate model
    /// (`has_one_through`).
    HasOneThrough,
    /// Records reached through an intermediate model
    /// (`has_many_through`).
    HasManyThrough,
    /// One related record holds the parent's key and type (`morph_one`).
    MorphOne,
    /// Related records hold the parent's key and type (`morph_many`).
    MorphMany,
    /// The parent holds the key and type of one record of any model
    /// (`morph_to`).
    MorphTo,
    /// Records related through a polymorphic pivot table
    /// (`morph_to_many`).
    MorphToMany,
    /// The inverse of `morph_to_many` (`morphed_by_many`).
    MorphedByMany,
}

impl RelationKind {
    /// Whether the relation holds several records (a collection).
    pub fn is_to_many(self) -> bool {
        matches!(
            self,
            RelationKind::HasMany
                | RelationKind::BelongsToMany
                | RelationKind::HasManyThrough
                | RelationKind::MorphMany
                | RelationKind::MorphToMany
                | RelationKind::MorphedByMany
        )
    }

    /// The kind's name as written in relation definitions (`belongs_to`,
    /// `has_many`, ...).
    pub fn name(self) -> &'static str {
        match self {
            RelationKind::BelongsTo => "belongs_to",
            RelationKind::HasOne => "has_one",
            RelationKind::HasMany => "has_many",
            RelationKind::BelongsToMany => "belongs_to_many",
            RelationKind::HasOneThrough => "has_one_through",
            RelationKind::HasManyThrough => "has_many_through",
            RelationKind::MorphOne => "morph_one",
            RelationKind::MorphMany => "morph_many",
            RelationKind::MorphTo => "morph_to",
            RelationKind::MorphToMany => "morph_to_many",
            RelationKind::MorphedByMany => "morphed_by_many",
        }
    }
}

impl fmt::Display for RelationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a relation is: its kind, the models and tables it connects and
/// the columns it matches by (for tooling: `model:show`, API docs).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelationMeta {
    /// The relation's name.
    pub name: String,
    /// Its kind.
    pub kind: RelationKind,
    /// The model it is declared on.
    pub parent: &'static str,
    /// The related model; `None` for `morph_to`, whose model varies.
    pub related: Option<&'static str>,
    /// The related model's table.
    pub related_table: Option<&'static str>,
    /// The column holding the reference: on the parent (`belongs_to`,
    /// `morph_to`), on the related model (`has_one`, `has_many`, morph
    /// one/many), on the pivot table (many to many: the parent's), or on
    /// the intermediate model (through: the far parent's).
    pub foreign_key: Option<String>,
    /// The column the reference points at.
    pub owner_key: Option<String>,
    /// The pivot table of a many-to-many relation.
    pub pivot_table: Option<String>,
    /// The pivot column holding the related record's key.
    pub related_pivot_key: Option<String>,
    /// The intermediate model of a through relation.
    pub through: Option<&'static str>,
    /// The polymorphic type column.
    pub morph_type: Option<String>,
    /// The type name a polymorphic relation stores for its parent.
    pub morph_name: Option<String>,
}

impl RelationMeta {
    /// Metadata of relation `name` of kind `kind` declared on `P`.
    pub(crate) fn new<P: Model>(name: &str, kind: RelationKind) -> Self {
        RelationMeta {
            name: name.to_owned(),
            kind,
            parent: P::meta().name(),
            related: None,
            related_table: None,
            foreign_key: None,
            owner_key: None,
            pivot_table: None,
            related_pivot_key: None,
            through: None,
            morph_type: None,
            morph_name: None,
        }
    }

    /// The related model `R`.
    pub(crate) fn related<R: Model>(mut self) -> Self {
        self.related = Some(R::meta().name());
        self.related_table = Some(R::meta().table());
        self
    }

    /// The reference and the column it points at.
    pub(crate) fn keys(mut self, foreign_key: &str, owner_key: &str) -> Self {
        self.foreign_key = Some(foreign_key.to_owned());
        self.owner_key = Some(owner_key.to_owned());
        self
    }
}

/// A relation with its types erased: what the operations driven by
/// relation names run on. Implemented by every relation type.
pub trait ErasedRelation<P: Model>: Send + Sync + 'static {
    /// The relation's name.
    fn name(&self) -> &str;

    /// What the relation is.
    fn meta(&self) -> RelationMeta;

    /// The related model's type; `None` for `morph_to`.
    fn related_type(&self) -> Option<TypeId>;

    /// Loads the relation into `parents` with one batched query (more for
    /// `morph_to`: one per type), applying `node`'s constraints, column
    /// selection and nested loads.
    fn eager_load<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        node: &'a EagerNode,
    ) -> BoxFuture<'a, Result<()>>;

    /// The subquery of the related records of an outer query's row:
    /// existence checks, relation counts and aggregates.
    fn exists_query(&self, request: ExistsRequest) -> Result<QueryIr>;

    /// Sets the update timestamp of `parent`'s related records to now.
    fn touch<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parent: &'a Record<P>,
    ) -> BoxFuture<'a, Result<()>>;

    /// For the parents that have the relation loaded, loads the missing
    /// relations of `rest` into the related records.
    fn load_nested_missing<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        rest: &'a EagerTree,
    ) -> BoxFuture<'a, Result<()>>;

    /// The polymorphic operations of a `morph_to` relation.
    fn morph_to(&self) -> Option<&dyn MorphToOps<P>> {
        None
    }
}

/// A relation of model `P` with its types erased: what a relation path
/// (`"company.owner"`) resolves to. [`Model::relation`] hands one out by
/// name.
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::RelationKind;
///
/// let activities = Deal::relation("activities").unwrap();
/// assert_eq!(activities.name(), "activities");
/// assert_eq!(activities.kind(), RelationKind::HasMany);
/// ```
pub struct DynRelation<P: Model> {
    pub(crate) inner: Arc<dyn ErasedRelation<P>>,
}

impl<P: Model> DynRelation<P> {
    /// `relation` with its types erased (without the parent it was made
    /// from).
    pub fn new<R: Relation<P>>(relation: R) -> Self {
        DynRelation {
            inner: private::Erase::erase(relation),
        }
    }

    /// The relation's name.
    pub fn name(&self) -> &str {
        self.inner.name()
    }

    /// The relation's kind.
    pub fn kind(&self) -> RelationKind {
        self.inner.meta().kind
    }

    /// What the relation is.
    pub fn meta(&self) -> RelationMeta {
        self.inner.meta()
    }
}

impl<P: Model> Clone for DynRelation<P> {
    fn clone(&self) -> Self {
        DynRelation {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<P: Model> fmt::Debug for DynRelation<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let meta = self.inner.meta();
        f.debug_struct("DynRelation")
            .field("model", &P::meta().name())
            .field("name", &meta.name)
            .field("kind", &meta.kind)
            .field("related", &meta.related)
            .finish()
    }
}

/// Relations defined at run time, by model type: `(name, relation)`.
type Defined = HashMap<TypeId, Vec<(Cow<'static, str>, Arc<dyn Any + Send + Sync>)>>;

fn defined_relations() -> &'static RwLock<Defined> {
    static DEFINED: OnceLock<RwLock<Defined>> = OnceLock::new();
    DEFINED.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Relations added to a model by another module: a module that cannot
/// edit the model's `#[relations]` block (a billing module adding a deal's
/// invoices) defines the relation here, once at boot, and gives it a typed
/// accessor with an extension trait. Defined relations work everywhere a
/// relation name does (`with`, `has`, `load`, ...).
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::{HasMany, RelationRegistry, Relationships};
///
/// /// A deal's events, added by an events module.
/// pub trait DealEvents {
///     fn feed(&self) -> HasMany<Deal, Activity>;
/// }
///
/// impl DealEvents for Deal {
///     fn feed(&self) -> HasMany<Deal, Activity> {
///         self.has_many(Activity::DEAL_ID).latest()
///     }
/// }
///
/// RelationRegistry::define("feed", <Deal as DealEvents>::feed);
/// assert!(Deal::relation("feed").is_some());
/// let query = Deal::query().with("feed");
/// # let _ = query;
/// ```
pub struct RelationRegistry;

impl RelationRegistry {
    /// Defines relation `name` of model `M` as `relation` (a function
    /// building it from a record, as a relation method does). Defining a
    /// name again replaces the earlier definition.
    pub fn define<M, R>(name: &'static str, relation: fn(&M) -> R)
    where
        M: Model + Default,
        R: Relation<M>,
    {
        let mut relation = relation(&M::default());
        private::Erase::rename(&mut relation, Cow::Borrowed(name));
        let erased: Arc<dyn Any + Send + Sync> = Arc::new(DynRelation::new(relation));
        let mut defined = defined_relations()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entries = defined.entry(TypeId::of::<M>()).or_default();
        match entries.iter_mut().find(|(existing, _)| existing == name) {
            Some(entry) => entry.1 = erased,
            None => entries.push((Cow::Borrowed(name), erased)),
        }
    }

    /// Removes relation `name` of model `M`, if it was defined here.
    pub fn remove<M: Model>(name: &str) {
        let mut defined = defined_relations()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entries) = defined.get_mut(&TypeId::of::<M>()) {
            entries.retain(|(existing, _)| existing != name);
        }
    }
}

/// Relation `name` of `M` defined in the [`RelationRegistry`].
pub(crate) fn defined<M: Model>(name: &str) -> Option<DynRelation<M>> {
    let defined = defined_relations()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    defined
        .get(&TypeId::of::<M>())?
        .iter()
        .find(|(existing, _)| existing == name)
        .and_then(|(_, relation)| relation.downcast_ref::<DynRelation<M>>().cloned())
}

/// The names of the relations of `M` defined in the [`RelationRegistry`].
pub(crate) fn defined_names<M: Model>() -> Vec<Cow<'static, str>> {
    let defined = defined_relations()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    defined
        .get(&TypeId::of::<M>())
        .map(|entries| entries.iter().map(|(name, _)| name.clone()).collect())
        .unwrap_or_default()
}

/// Relation `name` of `M`, or [`OrmError::RelationNotFound`] listing the
/// relations `M` has.
pub(crate) fn find<M: Model>(name: &str) -> Result<DynRelation<M>> {
    M::relation(name).ok_or_else(|| OrmError::RelationNotFound {
        model: Cow::Borrowed(M::meta().name()),
        relation: name.to_owned(),
        known: M::relation_names()
            .into_iter()
            .map(Cow::into_owned)
            .collect(),
    })
}
