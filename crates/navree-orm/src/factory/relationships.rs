//! How factories connect records: [`BelongsTo`] (a record points at its
//! parent), [`HasMany`] (children point at the record), [`Pivot`]
//! (many-to-many through a pivot table), the named [`Relationships`] a
//! factory declares, and the child links `has` and `has_attached` run after
//! a record is stored.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::column::{Column, ColumnName};
use crate::error::{OrmError, Result};
use crate::exec::Target;
use crate::factory::sources::Recycle;
use crate::factory::{Factory, ModelFactory};
use crate::model::Model;
use crate::query::table;
use crate::record::Record;
use crate::value::{Field, Value, ValueFor};

/// How records of model `M` point at a parent of model `P`: the foreign
/// key column of `M` holding the parent's key (or another column of the
/// parent, [`owner_key`](BelongsTo::owner_key)), and for a polymorphic
/// parent the column holding the parent's type.
///
/// A typed foreign key column converts into one, so
/// `Deal::factory().for_(Company::factory(), Deal::COMPANY_ID)` reads as
/// "deals belong to a company through `company_id`"; the column's type must
/// accept the parent's key.
///
/// ```
/// # use navree_orm::__doc_factories::*;
/// use navree_orm::factory::BelongsTo;
///
/// let link: BelongsTo<Deal, Company> = BelongsTo::new(Deal::COMPANY_ID);
/// assert_eq!(link.foreign_key(), "company_id");
/// let polymorphic: BelongsTo<Activity, Deal> =
///     BelongsTo::morph(Activity::SUBJECT_TYPE, Activity::SUBJECT_ID, "deal");
/// assert_eq!(polymorphic.morph_type(), Some(("subject_type", "deal")));
/// ```
pub struct BelongsTo<M, P> {
    pub(crate) foreign_key: Cow<'static, str>,
    pub(crate) owner_key: Option<Cow<'static, str>>,
    pub(crate) morph: Option<(Cow<'static, str>, Cow<'static, str>)>,
    pub(crate) named: Option<&'static str>,
    _models: PhantomData<fn() -> (M, P)>,
}

impl<M: Model, P: Model> BelongsTo<M, P> {
    /// `M` belongs to `P` through `foreign_key`, which holds `P`'s key.
    pub fn new<T>(foreign_key: Column<M, T>) -> Self
    where
        P::Key: ValueFor<T>,
    {
        BelongsTo {
            foreign_key: Cow::Owned(foreign_key.column_name().into_owned()),
            owner_key: None,
            morph: None,
            named: None,
            _models: PhantomData,
        }
    }

    /// `M` belongs to a `P` of type `morph_type` through `type_column`
    /// (holding the type) and `id_column` (holding the key): a polymorphic
    /// parent.
    pub fn morph<S, T>(
        type_column: Column<M, S>,
        id_column: Column<M, T>,
        morph_type: impl Into<Cow<'static, str>>,
    ) -> Self
    where
        String: ValueFor<S>,
        P::Key: ValueFor<T>,
    {
        BelongsTo {
            foreign_key: Cow::Owned(id_column.column_name().into_owned()),
            owner_key: None,
            morph: Some((
                Cow::Owned(type_column.column_name().into_owned()),
                morph_type.into(),
            )),
            named: None,
            _models: PhantomData,
        }
    }

    /// The foreign key holds `owner_key` of the parent instead of its key.
    pub fn owner_key<T>(mut self, owner_key: Column<P, T>) -> Self {
        self.owner_key = Some(Cow::Owned(owner_key.column_name().into_owned()));
        self
    }

    /// The foreign key column.
    pub fn foreign_key(&self) -> &str {
        &self.foreign_key
    }

    /// The parent's column the foreign key holds, when it is not the key.
    pub fn owner_key_column(&self) -> Option<&str> {
        self.owner_key.as_deref()
    }

    /// The type column and the type value of a polymorphic parent.
    pub fn morph_type(&self) -> Option<(&str, &str)> {
        self.morph
            .as_ref()
            .map(|(column, value)| (column.as_ref(), value.as_ref()))
    }
}

impl<M, P> Clone for BelongsTo<M, P> {
    fn clone(&self) -> Self {
        BelongsTo {
            foreign_key: self.foreign_key.clone(),
            owner_key: self.owner_key.clone(),
            morph: self.morph.clone(),
            named: self.named,
            _models: PhantomData,
        }
    }
}

impl<M, P> fmt::Debug for BelongsTo<M, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BelongsTo")
            .field("foreign_key", &self.foreign_key)
            .field("owner_key", &self.owner_key)
            .field("morph", &self.morph)
            .finish()
    }
}

impl<M: Model, P: Model, T> From<Column<M, T>> for BelongsTo<M, P>
where
    P::Key: ValueFor<T>,
{
    fn from(foreign_key: Column<M, T>) -> Self {
        BelongsTo::new(foreign_key)
    }
}

/// A relationship the factory of `M` names in
/// [`ModelFactory::relationships`]: `for_(Company::factory(), "company")`.
impl<M, P> From<&'static str> for BelongsTo<M, P> {
    fn from(name: &'static str) -> Self {
        BelongsTo {
            foreign_key: Cow::Borrowed(""),
            owner_key: None,
            morph: None,
            named: Some(name),
            _models: PhantomData,
        }
    }
}

/// How records of a child model `C` point at a record of model `M`: the
/// child's foreign key column holding `M`'s key (or another column of `M`,
/// [`local_key`](HasMany::local_key)), and for polymorphic children the
/// column holding `M`'s type. A has-one relation is the same link with one
/// child.
///
/// A typed foreign key column of the child converts into one:
/// `Deal::factory().has(Activity::factory().count(3), Activity::DEAL_ID)`.
pub struct HasMany<M, C> {
    pub(crate) foreign_key: Cow<'static, str>,
    pub(crate) local_key: Option<Cow<'static, str>>,
    pub(crate) morph: Option<(Cow<'static, str>, Cow<'static, str>)>,
    pub(crate) named: Option<&'static str>,
    _models: PhantomData<fn() -> (M, C)>,
}

impl<M: Model, C: Model> HasMany<M, C> {
    /// `C` records point at `M` through `foreign_key`, which holds `M`'s
    /// key.
    pub fn new<T>(foreign_key: Column<C, T>) -> Self
    where
        M::Key: ValueFor<T>,
    {
        HasMany {
            foreign_key: Cow::Owned(foreign_key.column_name().into_owned()),
            local_key: None,
            morph: None,
            named: None,
            _models: PhantomData,
        }
    }

    /// Polymorphic children: `C` records point at an `M` of type
    /// `morph_type` through `type_column` and `id_column`.
    pub fn morph<S, T>(
        type_column: Column<C, S>,
        id_column: Column<C, T>,
        morph_type: impl Into<Cow<'static, str>>,
    ) -> Self
    where
        String: ValueFor<S>,
        M::Key: ValueFor<T>,
    {
        HasMany {
            foreign_key: Cow::Owned(id_column.column_name().into_owned()),
            local_key: None,
            morph: Some((
                Cow::Owned(type_column.column_name().into_owned()),
                morph_type.into(),
            )),
            named: None,
            _models: PhantomData,
        }
    }

    /// The foreign key holds `local_key` of the parent instead of its key.
    pub fn local_key<T>(mut self, local_key: Column<M, T>) -> Self {
        self.local_key = Some(Cow::Owned(local_key.column_name().into_owned()));
        self
    }

    /// The child's foreign key column.
    pub fn foreign_key(&self) -> &str {
        &self.foreign_key
    }

    /// The column values that point a child at `parent`.
    pub(crate) fn assignments(&self, parent: &Record<M>) -> Result<Vec<(Cow<'static, str>, Value)>> {
        let value = match &self.local_key {
            None => parent.key().to_value(),
            Some(column) => parent
                .model()
                .value(column)?
                .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(column))?,
        };
        let mut values = vec![(self.foreign_key.clone(), value)];
        if let Some((column, morph_type)) = &self.morph {
            values.push((column.clone(), Value::from(morph_type.to_string())));
        }
        Ok(values)
    }
}

impl<M, C> Clone for HasMany<M, C> {
    fn clone(&self) -> Self {
        HasMany {
            foreign_key: self.foreign_key.clone(),
            local_key: self.local_key.clone(),
            morph: self.morph.clone(),
            named: self.named,
            _models: PhantomData,
        }
    }
}

impl<M, C> fmt::Debug for HasMany<M, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HasMany")
            .field("foreign_key", &self.foreign_key)
            .field("local_key", &self.local_key)
            .field("morph", &self.morph)
            .finish()
    }
}

impl<M: Model, C: Model, T> From<Column<C, T>> for HasMany<M, C>
where
    M::Key: ValueFor<T>,
{
    fn from(foreign_key: Column<C, T>) -> Self {
        HasMany::new(foreign_key)
    }
}

/// A relationship the factory of `M` names in
/// [`ModelFactory::relationships`]: `has(Activity::factory(), "activities")`.
impl<M, C> From<&'static str> for HasMany<M, C> {
    fn from(name: &'static str) -> Self {
        HasMany {
            foreign_key: Cow::Borrowed(""),
            local_key: None,
            morph: None,
            named: Some(name),
            _models: PhantomData,
        }
    }
}

/// A many-to-many link between `M` and `R` through a pivot table: one row
/// per pair, holding `M`'s key and `R`'s key (or other columns of each,
/// [`parent_key`](Pivot::parent_key) and [`related_key`](Pivot::related_key)),
/// optionally with timestamps.
///
/// ```
/// # use navree_orm::__doc_factories::*;
/// use navree_orm::factory::Pivot;
///
/// let tags: Pivot<Deal, Tag> = Pivot::new("deal_tag", "deal_id", "tag_id").with_timestamps();
/// assert_eq!(tags.table(), "deal_tag");
/// ```
pub struct Pivot<M, R> {
    pub(crate) table: Cow<'static, str>,
    pub(crate) foreign_pivot_key: Cow<'static, str>,
    pub(crate) related_pivot_key: Cow<'static, str>,
    pub(crate) parent_key: Option<Cow<'static, str>>,
    pub(crate) related_key: Option<Cow<'static, str>>,
    pub(crate) timestamps: bool,
    pub(crate) named: Option<&'static str>,
    _models: PhantomData<fn() -> (M, R)>,
}

impl<M: Model, R: Model> Pivot<M, R> {
    /// The pivot table `table`, whose `foreign_pivot_key` column holds
    /// `M`'s key and `related_pivot_key` column holds `R`'s key.
    pub fn new(
        table: impl Into<Cow<'static, str>>,
        foreign_pivot_key: impl Into<Cow<'static, str>>,
        related_pivot_key: impl Into<Cow<'static, str>>,
    ) -> Self {
        Pivot {
            table: table.into(),
            foreign_pivot_key: foreign_pivot_key.into(),
            related_pivot_key: related_pivot_key.into(),
            parent_key: None,
            related_key: None,
            timestamps: false,
            named: None,
            _models: PhantomData,
        }
    }

    /// The pivot holds `column` of `M` instead of its key.
    pub fn parent_key<T>(mut self, column: Column<M, T>) -> Self {
        self.parent_key = Some(Cow::Owned(column.column_name().into_owned()));
        self
    }

    /// The pivot holds `column` of `R` instead of its key.
    pub fn related_key<T>(mut self, column: Column<R, T>) -> Self {
        self.related_key = Some(Cow::Owned(column.column_name().into_owned()));
        self
    }

    /// The pivot table has `created_at` and `updated_at` columns, set when
    /// a row is attached.
    pub fn with_timestamps(mut self) -> Self {
        self.timestamps = true;
        self
    }

    /// The pivot table.
    pub fn table(&self) -> &str {
        &self.table
    }
}

impl<M, R> Clone for Pivot<M, R> {
    fn clone(&self) -> Self {
        Pivot {
            table: self.table.clone(),
            foreign_pivot_key: self.foreign_pivot_key.clone(),
            related_pivot_key: self.related_pivot_key.clone(),
            parent_key: self.parent_key.clone(),
            related_key: self.related_key.clone(),
            timestamps: self.timestamps,
            named: self.named,
            _models: PhantomData,
        }
    }
}

impl<M, R> fmt::Debug for Pivot<M, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pivot")
            .field("table", &self.table)
            .field("foreign_pivot_key", &self.foreign_pivot_key)
            .field("related_pivot_key", &self.related_pivot_key)
            .field("timestamps", &self.timestamps)
            .finish_non_exhaustive()
    }
}

/// A relationship the factory of `M` names in
/// [`ModelFactory::relationships`]: `has_attached(Tag::factory(), [], "tags")`.
impl<M, R> From<&'static str> for Pivot<M, R> {
    fn from(name: &'static str) -> Self {
        Pivot {
            table: Cow::Borrowed(""),
            foreign_pivot_key: Cow::Borrowed(""),
            related_pivot_key: Cow::Borrowed(""),
            parent_key: None,
            related_key: None,
            timestamps: false,
            named: Some(name),
            _models: PhantomData,
        }
    }
}

/// The named relationships of model `M`, declared by its factory in
/// [`ModelFactory::relationships`], so `has`, `for_` and `has_attached`
/// can name them: `.has(Activity::factory().count(3), "activities")`.
///
/// ```
/// # use navree_orm::__doc_factories::*;
/// use navree_orm::factory::{Pivot, Relationships};
///
/// fn deal_relationships(relationships: &mut Relationships<Deal>) {
///     relationships
///         .belongs_to::<Company>("company", Deal::COMPANY_ID)
///         .has_many::<Activity>("activities", Activity::DEAL_ID)
///         .belongs_to_many::<Tag>("tags", Pivot::new("deal_tag", "deal_id", "tag_id"));
/// }
/// ```
pub struct Relationships<M> {
    entries: Vec<Entry>,
    _model: PhantomData<fn() -> M>,
}

/// One named relationship: its kind, the other model and the typed link.
struct Entry {
    name: &'static str,
    kind: Kind,
    related: TypeId,
    related_name: &'static str,
    /// The foreign key column, or the pivot table.
    via: String,
    link: Arc<dyn Any + Send + Sync>,
}

/// The kind of a named relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    BelongsTo,
    HasMany,
    BelongsToMany,
}

impl Kind {
    fn describe(self) -> &'static str {
        match self {
            Kind::BelongsTo => "a belongs-to relationship (use it with `for_`)",
            Kind::HasMany => "a has-many relationship (use it with `has`)",
            Kind::BelongsToMany => "a many-to-many relationship (use it with `has_attached`)",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Kind::BelongsTo => "belongs_to",
            Kind::HasMany => "has_many",
            Kind::BelongsToMany => "belongs_to_many",
        }
    }
}

/// A relationship a factory declares, as `model:show` lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RelationshipInfo {
    /// The relationship's name (`company`).
    pub name: &'static str,
    /// `belongs_to`, `has_many` or `belongs_to_many`.
    pub kind: &'static str,
    /// The other model (`Company`).
    pub related: &'static str,
    /// The foreign key column (`company_id`), or the pivot table.
    pub via: String,
}

impl<M: Model> Relationships<M> {
    pub(crate) fn of<F: ModelFactory<Model = M>>() -> Self {
        let mut relationships = Relationships {
            entries: Vec::new(),
            _model: PhantomData,
        };
        F::relationships(&mut relationships);
        relationships
    }

    fn push<O: Model>(
        &mut self,
        name: &'static str,
        kind: Kind,
        via: String,
        link: Arc<dyn Any + Send + Sync>,
    ) {
        self.entries.retain(|entry| entry.name != name);
        self.entries.push(Entry {
            name,
            kind,
            related: TypeId::of::<O>(),
            related_name: O::meta().name(),
            via,
            link,
        });
    }

    /// `name` is the parent of model `P` that `M` records point at through
    /// `link` (a foreign key column of `M`, or a [`BelongsTo`]).
    pub fn belongs_to<P: Model>(
        &mut self,
        name: &'static str,
        link: impl Into<BelongsTo<M, P>>,
    ) -> &mut Self {
        let link = link.into();
        let via = link.foreign_key().to_owned();
        self.push::<P>(name, Kind::BelongsTo, via, Arc::new(link));
        self
    }

    /// `name` are the `C` records pointing at an `M` record through `link`
    /// (a foreign key column of `C`, or a [`HasMany`]); also for has-one.
    pub fn has_many<C: Model>(
        &mut self,
        name: &'static str,
        link: impl Into<HasMany<M, C>>,
    ) -> &mut Self {
        let link = link.into();
        let via = link.foreign_key().to_owned();
        self.push::<C>(name, Kind::HasMany, via, Arc::new(link));
        self
    }

    /// `name` are the `R` records attached to an `M` record through
    /// `pivot`.
    pub fn belongs_to_many<R: Model>(&mut self, name: &'static str, pivot: Pivot<M, R>) -> &mut Self {
        let via = pivot.table().to_owned();
        self.push::<R>(name, Kind::BelongsToMany, via, Arc::new(pivot));
        self
    }

    /// The names, in declaration order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|entry| entry.name)
    }

    /// Every relationship, in declaration order.
    pub fn infos(&self) -> Vec<RelationshipInfo> {
        self.entries
            .iter()
            .map(|entry| RelationshipInfo {
                name: entry.name,
                kind: entry.kind.key(),
                related: entry.related_name,
                via: entry.via.clone(),
            })
            .collect()
    }

    /// Relationship `name` as a link of type `L` to model `O`.
    fn find<O: Model, L: Clone + 'static>(&self, name: &str, kind: Kind) -> Result<L> {
        let model = M::meta().name();
        let Some(entry) = self.entries.iter().find(|entry| entry.name == name) else {
            let known: Vec<&str> = self.names().collect();
            return Err(OrmError::factory(
                model,
                format!(
                    "{model} has no relationship named `{name}`; its factory declares {} (in ModelFactory::relationships)",
                    if known.is_empty() {
                        "none".to_owned()
                    } else {
                        known.join(", ")
                    }
                ),
            ));
        };
        if entry.kind != kind {
            return Err(OrmError::factory(
                model,
                format!("`{name}` of {model} is {}", entry.kind.describe()),
            ));
        }
        if entry.related != TypeId::of::<O>() {
            return Err(OrmError::factory(
                model,
                format!(
                    "`{name}` of {model} links {} records, not {}",
                    entry.related_name,
                    O::meta().name()
                ),
            ));
        }
        entry.link.downcast_ref::<L>().cloned().ok_or_else(|| {
            OrmError::factory(model, format!("`{name}` of {model} has an unexpected link"))
        })
    }

    pub(crate) fn belongs_to_link<P: Model>(&self, name: &str) -> Result<BelongsTo<M, P>> {
        self.find::<P, BelongsTo<M, P>>(name, Kind::BelongsTo)
    }

    pub(crate) fn has_many_link<C: Model>(&self, name: &str) -> Result<HasMany<M, C>> {
        self.find::<C, HasMany<M, C>>(name, Kind::HasMany)
    }

    pub(crate) fn pivot_link<R: Model>(&self, name: &str) -> Result<Pivot<M, R>> {
        self.find::<R, Pivot<M, R>>(name, Kind::BelongsToMany)
    }
}

impl<M> fmt::Debug for Relationships<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|entry| entry.name))
            .finish()
    }
}

/// Records created (or attached) for a stored record of `M`: `has` and
/// `has_attached`, type-erased over the related factory.
pub(crate) trait ChildLink<M: Model>: Send + Sync {
    /// Creates (or attaches) the related records of `parent`.
    fn create_for<'a>(
        &'a self,
        parent: &'a Record<M>,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
    ) -> BoxFuture<'a, Result<()>>;
}

/// `has(children, link)`: children created for each record.
pub(crate) struct HasChildren<M: Model, CF: ModelFactory> {
    pub(crate) factory: Factory<CF>,
    pub(crate) link: HasMany<M, CF::Model>,
}

impl<M: Model, CF: ModelFactory> ChildLink<M> for HasChildren<M, CF> {
    fn create_for<'a>(
        &'a self,
        parent: &'a Record<M>,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let assignments = self.link.assignments(parent)?;
            let mut factory = self.factory.clone().state_values(assignments);
            factory.recycle.extend(recycle);
            factory.create_on(target).await?;
            Ok(())
        })
    }
}

/// A factory of `R` records, type-erased over the factory.
pub(crate) trait CreateRecords<R: Model>: Send + Sync {
    /// Creates the records on `target`, with `recycle` added to the
    /// factory's pool.
    fn create_with<'a>(
        &'a self,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
    ) -> BoxFuture<'a, Result<Vec<Record<R>>>>;
}

impl<F: ModelFactory> CreateRecords<F::Model> for Factory<F> {
    fn create_with<'a>(
        &'a self,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
    ) -> BoxFuture<'a, Result<Vec<Record<F::Model>>>> {
        Box::pin(async move {
            let mut factory = self.clone();
            factory.recycle.extend(recycle);
            Ok(factory.create_on(target).await?.into_vec())
        })
    }
}

/// What `has_attached` attaches: records a factory creates, or stored
/// ones.
pub(crate) enum Attachable<R: Model> {
    Factory(Arc<dyn CreateRecords<R>>),
    Records(Vec<Record<R>>),
}

/// `has_attached(related, pivot_values, pivot)`: related records attached
/// to each record through a pivot table.
pub(crate) struct AttachRelated<M: Model, R: Model> {
    pub(crate) related: Attachable<R>,
    pub(crate) pivot: Pivot<M, R>,
    pub(crate) values: Vec<(Cow<'static, str>, Value)>,
}

impl<M: Model, R: Model> ChildLink<M> for AttachRelated<M, R> {
    fn create_for<'a>(
        &'a self,
        parent: &'a Record<M>,
        target: &'a Target<'a>,
        recycle: &'a Recycle,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let related: Vec<Record<R>> = match &self.related {
                Attachable::Records(records) => records.clone(),
                Attachable::Factory(factory) => factory.create_with(target, recycle).await?,
            };
            if related.is_empty() {
                return Ok(());
            }
            let parent_value = match &self.pivot.parent_key {
                None => parent.key().to_value(),
                Some(column) => parent
                    .model()
                    .value(column)?
                    .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(column))?,
            };
            let now = target.context().now().await;
            let mut rows = Vec::with_capacity(related.len());
            for record in &related {
                let related_value = match &self.pivot.related_key {
                    None => record.key().to_value(),
                    Some(column) => record
                        .model()
                        .value(column)?
                        .ok_or_else(|| crate::model::derive_support::unknown_column::<R>(column))?,
                };
                let mut row: Vec<(Cow<'static, str>, Value)> = vec![
                    (self.pivot.foreign_pivot_key.clone(), parent_value.clone()),
                    (self.pivot.related_pivot_key.clone(), related_value),
                ];
                row.extend(self.values.iter().cloned());
                if self.pivot.timestamps {
                    for column in ["created_at", "updated_at"] {
                        if !row.iter().any(|(name, _)| name == column) {
                            row.push((Cow::Borrowed(column), now.to_value()));
                        }
                    }
                }
                rows.push(row);
            }
            table(&self.pivot.table).insert(target, rows).await?;
            Ok(())
        })
    }
}
