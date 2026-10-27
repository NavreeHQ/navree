//! Polymorphic relations: `morph_to` ([`MorphTo`]: a comment's
//! `commentable` is a deal or a contact), the per-type loads of its eager
//! loads ([`MorphWith`]), and the existence checks per type (`has_morph`,
//! `where_has_morph`, `where_morphed_to`, ...).

use std::any::TypeId;
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::collection::Collection;
use crate::column::Filter;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::ir::{Boolean, Expr, Operator, QueryIr, Where, WhereClause};
use crate::model::{Model, ModelMeta};
use crate::query::filters::HasWheres;
use crate::query::{Builder, WhereGroup};
use crate::record::Record;
use crate::relations::access::AccessConstraint;
use crate::relations::any::AnyRecord;
use crate::relations::core::{column_value, qualified};
use crate::relations::eager::{Constraint, ConstraintFn, EagerNode, EagerTree};
use crate::relations::existence::{
    ExistsRequest, ExistsSelect, Refine, build_subquery, columns_equal,
};
use crate::relations::keys::{RelationKey, distinct_keys, in_keys, in_keys_on, key_of};
use crate::relations::morph_map::{MorphMap, type_name_of};
use crate::relations::path::{ParsedPath, RelationPath};
use crate::relations::registry::{ErasedRelation, RelationKind, RelationMeta, find};
use crate::relations::{Relation, private};
use crate::value::{Value, is_null};

/// Which soft-deleted records a `morph_to` loads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Trashed {
    /// Not the soft-deleted ones (the default).
    #[default]
    Without,
    /// Soft-deleted ones too.
    With,
    /// Only soft-deleted ones (of models that soft-delete).
    Only,
}

impl Trashed {
    fn apply<M: Model>(self, query: Builder<M>) -> Builder<M> {
        match self {
            Trashed::Without => query,
            Trashed::With => query.with_trashed(),
            Trashed::Only => match M::meta().deleted_at() {
                Some(column) => query.with_trashed().where_(Filter::from(Where::Null {
                    expr: Expr::Column(qualified(M::meta().table(), column)),
                    not: true,
                })),
                None => query,
            },
        }
    }
}

/// What a `morph_to` eager load does for one of its models.
#[derive(Clone, Debug)]
pub struct MorphTypeLoads {
    type_id: TypeId,
    model: &'static str,
    with: EagerTree,
    counts: Vec<String>,
    constraints: Vec<Constraint>,
    errors: Vec<String>,
}

impl MorphTypeLoads {
    fn new<M: Model>() -> Self {
        MorphTypeLoads {
            type_id: TypeId::of::<M>(),
            model: M::meta().name(),
            with: EagerTree::default(),
            counts: Vec::new(),
            constraints: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn merge(&mut self, other: &MorphTypeLoads) {
        self.with.merge(&other.with);
        self.counts.extend(other.counts.iter().cloned());
        self.constraints.extend(other.constraints.iter().cloned());
        self.errors.extend(other.errors.iter().cloned());
    }

    /// `query` with these loads: constraints, eager loads and counts.
    fn apply<M: Model>(&self, mut query: Builder<M>) -> Result<Builder<M>> {
        if let Some(error) = self.errors.first() {
            return Err(OrmError::invalid(error.clone()));
        }
        for constraint in &self.constraints {
            match constraint
                .closure
                .downcast_ref::<ConstraintFn<Builder<M>>>()
            {
                Some(apply) => query = (apply.0)(query),
                None => {
                    return Err(OrmError::invalid(format!(
                        "a morph constraint for {} takes `{}`",
                        self.model, constraint.takes
                    )));
                }
            }
        }
        query.eager.tree.merge(&self.with);
        for count in &self.counts {
            query = query.with_count(count.as_str());
        }
        Ok(query)
    }
}

/// What a `morph_to` eager load loads into each of its models: nested
/// relations ([`on`](MorphWith::on)), relation counts
/// ([`count_on`](MorphWith::count_on)) and constraints
/// ([`constrain`](MorphWith::constrain)), per model.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::MorphWith;
///
/// # async fn demo(db: Db) -> Result<()> {
/// let notes = Note::query()
///     .with_morph(
///         "notable",
///         MorphWith::new()
///             .on::<Deal>(["company"])
///             .count_on::<Deal>(["activities"])
///             .constrain::<Company>(|q| q.where_(Company::ACTIVE.eq(true))),
///     )
///     .get(&db)
///     .await?;
/// # Ok(()) }
/// ```
#[derive(Clone, Debug, Default)]
pub struct MorphWith {
    types: Vec<MorphTypeLoads>,
}

impl MorphWith {
    /// Nothing per type yet.
    pub fn new() -> Self {
        MorphWith::default()
    }

    fn loads<M: Model>(&mut self) -> &mut MorphTypeLoads {
        let index = match self
            .types
            .iter()
            .position(|loads| loads.type_id == TypeId::of::<M>())
        {
            Some(index) => index,
            None => {
                self.types.push(MorphTypeLoads::new::<M>());
                self.types.len() - 1
            }
        };
        &mut self.types[index]
    }

    /// Eager loads `paths` into the loaded records of model `M`.
    pub fn on<M: Model>(mut self, paths: impl IntoIterator<Item = impl RelationPath<M>>) -> Self {
        let loads = self.loads::<M>();
        for path in paths {
            let text = path.relation_path().into_owned();
            match ParsedPath::parse(&text) {
                Ok(parsed) => {
                    let node = loads.with.entry(&parsed.segments);
                    if parsed.columns.is_some() {
                        node.columns = parsed.columns;
                    }
                }
                Err(error) => loads.errors.push(error.to_string()),
            }
        }
        self
    }

    /// Counts the related records of `paths` on the loaded records of
    /// model `M` (`{relation}_count` extras).
    pub fn count_on<M: Model>(
        mut self,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Self {
        let loads = self.loads::<M>();
        for path in paths {
            loads.counts.push(path.relation_path().into_owned());
        }
        self
    }

    /// Constrains the query of model `M`'s records.
    pub fn constrain<M: Model>(
        mut self,
        constraint: impl Fn(Builder<M>) -> Builder<M> + Send + Sync + 'static,
    ) -> Self {
        self.loads::<M>()
            .constraints
            .push(Constraint::new(constraint));
        self
    }

    /// Adds `other`'s loads.
    pub(crate) fn merge(&mut self, other: &MorphWith) {
        for loads in &other.types {
            match self
                .types
                .iter_mut()
                .find(|existing| existing.type_id == loads.type_id)
            {
                Some(existing) => existing.merge(loads),
                None => self.types.push(loads.clone()),
            }
        }
    }

    /// The loads of the model with type id `type_id`.
    pub(crate) fn for_type(&self, type_id: TypeId) -> Option<&MorphTypeLoads> {
        self.types.iter().find(|loads| loads.type_id == type_id)
    }
}

/// What a `morph_to` load asks of one of its models.
pub struct MorphLoad<'a> {
    /// The column the stored ids point at; the model's key when `None`.
    pub(crate) owner_key: Option<&'a str>,
    /// The ids, distinct.
    pub(crate) keys: &'a [(RelationKey, Value)],
    /// The per-type loads.
    pub(crate) loads: Option<&'a MorphTypeLoads>,
    /// Loads that apply to every type (nested paths of the eager load).
    pub(crate) children: &'a EagerTree,
    /// Conditions that apply to every type.
    pub(crate) conditions: &'a [Filter],
    /// Access filters, asked for each type's condition.
    pub(crate) access: &'a [AccessConstraint],
    /// Soft-deleted records.
    pub(crate) trashed: Trashed,
}

/// What a `morph_to` existence check asks of one of its models.
pub struct MorphExists<'a> {
    /// The model the `morph_to` is declared on.
    pub(crate) parent: &'static ModelMeta,
    /// The id column on the parent.
    pub(crate) id_column: &'a str,
    /// The column the ids point at; the model's key when `None`.
    pub(crate) owner_key: Option<&'a str>,
    /// How deep the outer query is nested.
    pub(crate) depth: u32,
    /// What the subquery selects.
    pub(crate) select: ExistsSelect,
    /// A condition on the model's records.
    pub(crate) condition: Option<Filter>,
    /// Soft-deleted records.
    pub(crate) trashed: Trashed,
}

/// One model a `morph_to` can load, with its type erased.
pub trait MorphTarget: Send + Sync + 'static {
    /// The model's type id.
    fn type_id(&self) -> TypeId;

    /// The name the model's records are stored under in a type column.
    fn morph_name(&self) -> Cow<'static, str>;

    /// A stored id as the model's key stores it (an id column of text
    /// holding `"5"` for an integer key), so it binds and matches as one.
    fn normalize_key(&self, value: Value, owner_key: Option<&str>) -> Value;

    /// The records whose owner key is one of `request.keys`, by key.
    fn load<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        request: MorphLoad<'a>,
    ) -> BoxFuture<'a, Result<Vec<(RelationKey, AnyRecord)>>>;

    /// Loads `paths` (eager loads, or aggregates when `aggregate` is set)
    /// into `records` (all of this model).
    fn load_into<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        records: Vec<AnyRecord>,
        loads: &'a MorphTypeLoads,
        aggregate: Option<&'a crate::relations::aggregates::AggregateSpec>,
    ) -> BoxFuture<'a, Result<Vec<AnyRecord>>>;

    /// Loads what `rest` misses into `records` (all of this model).
    fn load_missing_into<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        records: Vec<AnyRecord>,
        rest: &'a EagerTree,
    ) -> BoxFuture<'a, Result<Vec<AnyRecord>>>;

    /// The existence subquery of this model's records.
    fn exists_query(&self, request: MorphExists<'_>) -> Result<QueryIr>;
}

/// `records` as records of `M` (an error names a record of another model).
fn typed_records<M: Model>(records: Vec<AnyRecord>) -> Result<Vec<Record<M>>> {
    records
        .into_iter()
        .map(|record| {
            record.downcast::<M>().map_err(|other| {
                OrmError::invalid(format!(
                    "a {} among the {} records of a morph load",
                    other.model_name(),
                    M::meta().name()
                ))
            })
        })
        .collect()
}

/// The [`MorphTarget`] of model `M`.
pub(crate) struct MorphTargetOf<M: Model>(PhantomData<fn() -> M>);

impl<M: Model> MorphTargetOf<M> {
    pub(crate) fn new() -> Self {
        MorphTargetOf(PhantomData)
    }
}

impl<M: Model> MorphTarget for MorphTargetOf<M> {
    fn type_id(&self) -> TypeId {
        TypeId::of::<M>()
    }

    fn morph_name(&self) -> Cow<'static, str> {
        M::morph_name()
    }

    fn normalize_key(&self, value: Value, owner_key: Option<&str>) -> Value {
        match owner_key {
            Some(column) if column != M::meta().key_column() => value,
            _ => match <M::Key as crate::Field>::from_value(value.clone()) {
                Ok(key) => crate::Field::to_value(&key),
                Err(_) => value,
            },
        }
    }

    fn load<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        request: MorphLoad<'a>,
    ) -> BoxFuture<'a, Result<Vec<(RelationKey, AnyRecord)>>> {
        Box::pin(async move {
            let meta = M::meta();
            let keys = distinct_keys(
                request
                    .keys
                    .iter()
                    .map(|(_, value)| self.normalize_key(value.clone(), request.owner_key)),
            );
            let owner_key = request.owner_key.unwrap_or(meta.key_column());
            let mut query = request.trashed.apply(Builder::<M>::new());
            if let Some(loads) = request.loads {
                query = loads.apply(query)?;
            }
            for condition in request.conditions {
                query = query.where_(condition.clone());
            }
            for access in request.access {
                query = access.apply(query)?;
            }
            query.eager.tree.merge(request.children);
            let query = query.where_(Filter::from(in_keys_on(
                target,
                qualified(meta.table(), owner_key),
                &keys,
            )));
            let records = query.fetch(target).await?;
            let mut found = Vec::with_capacity(records.len());
            for record in records {
                let value = column_value(&record.model, owner_key, "morph_to")?;
                if let Some(key) = key_of(&value) {
                    found.push((key, AnyRecord::new(record)));
                }
            }
            Ok(found)
        })
    }

    fn load_into<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        records: Vec<AnyRecord>,
        loads: &'a MorphTypeLoads,
        aggregate: Option<&'a crate::relations::aggregates::AggregateSpec>,
    ) -> BoxFuture<'a, Result<Vec<AnyRecord>>> {
        Box::pin(async move {
            let mut typed = typed_records::<M>(records)?;
            match aggregate {
                None => {
                    crate::relations::eager::load_tree(target, &mut typed, &loads.with).await?;
                    if !loads.counts.is_empty() {
                        let count = crate::relations::aggregates::AggregateSpec::count();
                        crate::relations::aggregates::load_aggregate_on(
                            target,
                            &mut typed,
                            &loads.counts,
                            &count,
                        )
                        .await?;
                    }
                }
                Some(spec) => {
                    // The relations listed for the model, with `on` (first
                    // steps) or `count_on`.
                    let mut paths: Vec<String> = loads
                        .with
                        .paths()
                        .into_iter()
                        .filter(|path| !path.contains('.'))
                        .collect();
                    for counted in &loads.counts {
                        if !paths.contains(counted) {
                            paths.push(counted.clone());
                        }
                    }
                    crate::relations::aggregates::load_aggregate_on(
                        target, &mut typed, &paths, spec,
                    )
                    .await?;
                }
            }
            Ok(typed.into_iter().map(AnyRecord::new).collect())
        })
    }

    fn load_missing_into<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        records: Vec<AnyRecord>,
        rest: &'a EagerTree,
    ) -> BoxFuture<'a, Result<Vec<AnyRecord>>> {
        Box::pin(async move {
            let mut typed = typed_records::<M>(records)?;
            crate::relations::lazy::load_missing_tree(target, &mut typed, rest).await?;
            Ok(typed.into_iter().map(AnyRecord::new).collect())
        })
    }

    fn exists_query(&self, request: MorphExists<'_>) -> Result<QueryIr> {
        let meta = M::meta();
        let owner_key = request.owner_key.unwrap_or(meta.key_column()).to_owned();
        let aliased = (meta.table() == request.parent.table()).then_some(meta.table());
        let definition = request.trashed.apply(Builder::<M>::new());
        let refine = match request.condition {
            Some(filter) => Refine::Filter(filter),
            None => Refine::None,
        };
        let outer = qualified(request.parent.table(), request.id_column);
        build_subquery::<M>(
            request.parent,
            &definition,
            ExistsRequest {
                depth: request.depth,
                select: request.select,
                refine,
            },
            "morph_to",
            aliased,
            |query, _| query,
            |alias| {
                vec![columns_equal(
                    outer,
                    qualified(alias.unwrap_or(meta.table()), &owner_key),
                )]
            },
        )
    }
}

/// The models a `morph_to` lists itself: tuples of models,
/// `.targets::<(Deal, Contact)>()`.
pub trait MorphTargets {
    #[doc(hidden)]
    fn morph_targets() -> Vec<Arc<dyn MorphTarget>>;
}

macro_rules! morph_targets {
    ($($model:ident),+) => {
        impl<$($model: Model),+> MorphTargets for ($($model,)+) {
            fn morph_targets() -> Vec<Arc<dyn MorphTarget>> {
                vec![$(Arc::new(MorphTargetOf::<$model>::new()) as Arc<dyn MorphTarget>),+]
            }
        }
    };
}

morph_targets!(A);
morph_targets!(A, B);
morph_targets!(A, B, C);
morph_targets!(A, B, C, D);
morph_targets!(A, B, C, D, E);
morph_targets!(A, B, C, D, E, F);
morph_targets!(A, B, C, D, E, F, G);
morph_targets!(A, B, C, D, E, F, G, H);

/// A polymorphic belongs-to: the parent holds the type and the key of one
/// record of any model (`commentable_type` and `commentable_id`). Loaded,
/// it holds an `Option<AnyRecord>`.
///
/// The models it can load come from its own list
/// ([`target`](MorphTo::target), [`targets`](MorphTo::targets)), else from
/// the process-wide [`MorphMap`].
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, note: Record<Note>) -> Result<()> {
/// // #[relations] impl Note {
/// //     fn notable(&self) -> MorphTo<Note> { self.morph_to("notable").targets::<(Deal, Company)>() }
/// // }
/// if let Some(notable) = note.notable().get(&db).await? {
///     if let Some(deal) = notable.downcast_ref::<Deal>() {
///         println!("a note on deal {}", deal.title);
///     }
/// }
/// # Ok(()) }
/// ```
pub struct MorphTo<P: Model> {
    pub(crate) name: Cow<'static, str>,
    pub(crate) parent: Option<P>,
    pub(crate) type_column: Cow<'static, str>,
    pub(crate) id_column: Cow<'static, str>,
    pub(crate) owner_key: Option<Cow<'static, str>>,
    pub(crate) targets: Vec<Arc<dyn MorphTarget>>,
    pub(crate) trashed: Trashed,
    pub(crate) with: MorphWith,
}

impl<P: Model> Clone for MorphTo<P> {
    fn clone(&self) -> Self {
        MorphTo {
            name: self.name.clone(),
            parent: self.parent.clone(),
            type_column: self.type_column.clone(),
            id_column: self.id_column.clone(),
            owner_key: self.owner_key.clone(),
            targets: self.targets.clone(),
            trashed: self.trashed,
            with: self.with.clone(),
        }
    }
}

impl<P: Model> fmt::Debug for MorphTo<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MorphTo")
            .field("parent", &P::meta().name())
            .field("name", &self.name)
            .field("type_column", &self.type_column)
            .field("id_column", &self.id_column)
            .finish_non_exhaustive()
    }
}

impl<P: Model> MorphTo<P> {
    /// The `morph_to` named `name` of `parent`: columns `{name}_type` and
    /// `{name}_id`.
    pub(crate) fn new(parent: Option<P>, name: &str) -> Self {
        MorphTo {
            name: Cow::Owned(name.to_owned()),
            parent,
            type_column: Cow::Owned(format!("{name}_type")),
            id_column: Cow::Owned(format!("{name}_id")),
            owner_key: None,
            targets: Vec::new(),
            trashed: Trashed::Without,
            with: MorphWith::new(),
        }
    }

    /// Names the relation (what it is loaded under).
    pub fn named(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.name = name.into();
        self
    }

    /// The type column, when it is not `{name}_type`.
    pub fn type_column(mut self, column: impl Into<Cow<'static, str>>) -> Self {
        self.type_column = column.into();
        self
    }

    /// The id column, when it is not `{name}_id`.
    pub fn id_column(mut self, column: impl Into<Cow<'static, str>>) -> Self {
        self.id_column = column.into();
        self
    }

    /// The column of the related models the ids point at, when it is not
    /// their key.
    pub fn owner_key(mut self, column: impl Into<Cow<'static, str>>) -> Self {
        self.owner_key = Some(column.into());
        self
    }

    /// Lists model `M` among the models the relation loads.
    pub fn target<M: Model>(mut self) -> Self {
        self.targets
            .retain(|target| target.type_id() != TypeId::of::<M>());
        self.targets.push(Arc::new(MorphTargetOf::<M>::new()));
        self
    }

    /// Lists the models of tuple `T` (`(Deal, Contact)`).
    pub fn targets<T: MorphTargets>(mut self) -> Self {
        for target in T::morph_targets() {
            self.targets
                .retain(|existing| existing.type_id() != target.type_id());
            self.targets.push(target);
        }
        self
    }

    /// Loads soft-deleted records too, of every model.
    pub fn with_trashed(mut self) -> Self {
        self.trashed = Trashed::With;
        self
    }

    /// Leaves soft-deleted records out (the default).
    pub fn without_trashed(mut self) -> Self {
        self.trashed = Trashed::Without;
        self
    }

    /// Loads only soft-deleted records (of models that soft-delete).
    pub fn only_trashed(mut self) -> Self {
        self.trashed = Trashed::Only;
        self
    }

    /// Per-model loads of every load of this relation.
    pub fn morph_with(mut self, with: MorphWith) -> Self {
        self.with.merge(&with);
        self
    }

    /// Constrains the query of model `M`'s records.
    pub fn constrain<M: Model>(
        mut self,
        constraint: impl Fn(Builder<M>) -> Builder<M> + Send + Sync + 'static,
    ) -> Self {
        self.with = std::mem::take(&mut self.with).constrain(constraint);
        self
    }

    /// The relation's name.
    pub fn relation_name(&self) -> &str {
        &self.name
    }

    /// The type column.
    pub fn morph_type(&self) -> &str {
        &self.type_column
    }

    /// The id column.
    pub fn foreign_key_name(&self) -> &str {
        &self.id_column
    }

    /// The id column qualified by the parent's table.
    pub fn qualified_foreign_key_name(&self) -> String {
        format!("{}.{}", P::meta().table(), self.id_column)
    }

    /// The column the ids point at, when it is not the related models'
    /// key.
    pub fn owner_key_name(&self) -> Option<&str> {
        self.owner_key.as_deref()
    }

    /// The model registered under `name`: the relation's own list first,
    /// then the morph map.
    pub(crate) fn resolve(&self, name: &str) -> Result<Arc<dyn MorphTarget>> {
        if let Some(target) = self
            .targets
            .iter()
            .find(|target| target.morph_name() == name)
        {
            return Ok(Arc::clone(target));
        }
        match MorphMap::current().entry(name) {
            Some(entry) => Ok(Arc::clone(&entry.target)),
            None => Err(OrmError::UnknownMorph {
                name: name.to_owned(),
            }),
        }
    }

    /// Every model the relation can load: its own list, else the morph
    /// map's.
    pub(crate) fn all_targets(&self) -> Vec<Arc<dyn MorphTarget>> {
        if !self.targets.is_empty() {
            return self.targets.clone();
        }
        MorphMap::current()
            .entries()
            .iter()
            .map(|entry| Arc::clone(&entry.target))
            .collect()
    }

    /// The related record of the parent, if any.
    pub async fn get<E: Executor + ?Sized>(self, exec: &E) -> Result<Option<AnyRecord>> {
        let parent = match &self.parent {
            Some(parent) => parent.clone(),
            None => {
                return Err(OrmError::invalid(format!(
                    "the relation `{}` of {} was not made from a record",
                    self.name,
                    P::meta().name()
                )));
            }
        };
        let target = exec.target(Access::Read).await?;
        let mut records = [Record::stored(parent)];
        let node = EagerNode {
            name: self.name.to_string(),
            ..EagerNode::default()
        };
        self.load_into(&target, &mut records, &node).await?;
        let [record] = records;
        Ok(record
            .relations
            .get::<Option<AnyRecord>>(&self.name)
            .cloned()
            .flatten())
    }

    /// Whether `record` is the parent's related record (same type name and
    /// key; no query).
    pub fn is<M: Model>(&self, record: &Record<M>) -> bool {
        let Some(parent) = &self.parent else {
            return false;
        };
        let (Ok(stored_type), Ok(stored_id)) = (
            column_value(parent, &self.type_column, &self.name),
            column_value(parent, &self.id_column, &self.name),
        ) else {
            return false;
        };
        let owner = match &self.owner_key {
            Some(column) => match record.model.value(column) {
                Ok(Some(value)) => value,
                _ => return false,
            },
            None => crate::Field::to_value(&record.key()),
        };
        let type_matches = match &stored_type {
            Value::String(Some(stored)) => *stored == M::morph_name(),
            _ => false,
        };
        type_matches
            && key_of(&stored_id).is_some()
            && key_of(&stored_id) == key_of(&owner)
            && M::meta().connection_name() == P::meta().connection_name()
    }

    /// Whether `record` is not the parent's related record.
    pub fn is_not<M: Model>(&self, record: &Record<M>) -> bool {
        !self.is(record)
    }

    /// Loads the relation into `parents`, one query per stored type.
    async fn load_into(
        &self,
        target: &Target<'_>,
        parents: &mut [Record<P>],
        node: &EagerNode,
    ) -> Result<()> {
        let mut relation = self.clone();
        let mut conditions: Vec<Filter> = Vec::new();
        let mut access: Vec<AccessConstraint> = Vec::new();
        for constraint in &node.constraints {
            if let Some(filter) = constraint.filter() {
                conditions.push(filter.clone());
                continue;
            }
            if let Some(constraint) = constraint.access() {
                access.push(constraint.clone());
                continue;
            }
            match constraint
                .closure
                .downcast_ref::<ConstraintFn<MorphTo<P>>>()
            {
                Some(apply) => relation = (apply.0)(relation),
                None => {
                    return Err(OrmError::invalid(format!(
                        "the constraint on morph_to relation `{}` of {} takes `{}`: constrain a MorphTo<{}>, or each model with MorphWith::constrain",
                        node.name,
                        P::meta().name(),
                        constraint.takes,
                        P::meta().name()
                    )));
                }
            }
        }
        let mut with = relation.with.clone();
        if let Some(per_type) = &node.morph {
            with.merge(per_type);
        }
        // The stored type and id of each parent, grouped by type in the
        // order the types first appear.
        let mut groups: Vec<(String, Vec<(usize, Value)>)> = Vec::new();
        for (index, parent) in parents.iter().enumerate() {
            let stored_type = column_value(&parent.model, &relation.type_column, &relation.name)?;
            let id = column_value(&parent.model, &relation.id_column, &relation.name)?;
            let name = match stored_type {
                Value::String(Some(name)) if !name.is_empty() => name,
                _ => continue,
            };
            if is_null(&id) {
                continue;
            }
            match groups.iter_mut().find(|(existing, _)| *existing == name) {
                Some((_, members)) => members.push((index, id)),
                None => groups.push((name, vec![(index, id)])),
            }
        }
        let mut found: Vec<Option<AnyRecord>> = vec![None; parents.len()];
        for (name, members) in groups {
            let morph_target = relation.resolve(&name)?;
            let keys = distinct_keys(members.iter().map(|(_, id)| id.clone()));
            let records = morph_target
                .load(
                    target,
                    MorphLoad {
                        owner_key: relation.owner_key.as_deref(),
                        keys: &keys,
                        loads: with.for_type(morph_target.type_id()),
                        children: &node.children,
                        conditions: &conditions,
                        access: &access,
                        trashed: relation.trashed,
                    },
                )
                .await?;
            for (index, id) in members {
                let key = key_of(&morph_target.normalize_key(id, relation.owner_key.as_deref()));
                if let Some((_, record)) = records
                    .iter()
                    .find(|(found_key, _)| Some(found_key) == key.as_ref())
                {
                    found[index] = Some(record.clone());
                }
            }
        }
        for (parent, value) in parents.iter_mut().zip(found) {
            parent.relations.set_relation(node.name.clone(), value);
        }
        Ok(())
    }

    /// The condition of a polymorphic existence check: per type, the type
    /// column holds the type's name and its records exist (or not); for
    /// "none" over every type, a NULL type too.
    pub(crate) fn has_morph_where(
        &self,
        depth: u32,
        types: Option<&[String]>,
        op: Operator,
        count: u64,
        refine: &mut dyn FnMut(&str) -> Option<Filter>,
    ) -> Result<Where> {
        let targets = match types {
            Some(names) => names
                .iter()
                .map(|name| self.resolve(name))
                .collect::<Result<Vec<_>>>()?,
            None => self.all_targets(),
        };
        let count_i = i64::try_from(count).unwrap_or(i64::MAX);
        let check_null = types.is_none()
            && match op {
                Operator::Lt => count >= 1,
                Operator::Lte => true,
                Operator::Eq => count == 0,
                Operator::Ne => count >= 1,
                _ => false,
            };
        if targets.is_empty() {
            // No model to check: compare a constant, as a count of zero.
            return Ok(Where::Compare {
                left: Expr::Value(Value::BigInt(Some(0))),
                op,
                right: Expr::Value(Value::BigInt(Some(count_i))),
            });
        }
        let exists = count == 1 && matches!(op, Operator::Gte | Operator::Lt);
        let parent = P::meta();
        let mut alternatives = Vec::with_capacity(targets.len() + 1);
        for target in targets {
            let name = target.morph_name();
            let query = target.exists_query(MorphExists {
                parent,
                id_column: &self.id_column,
                owner_key: self.owner_key.as_deref(),
                depth,
                select: if exists {
                    ExistsSelect::One
                } else {
                    ExistsSelect::Count
                },
                condition: refine(&name),
                trashed: self.trashed,
            })?;
            let check = if exists {
                Where::Exists {
                    query: Box::new(query),
                    not: op == Operator::Lt,
                }
            } else {
                Where::Compare {
                    left: Expr::Subquery(Box::new(query)),
                    op: op.clone(),
                    right: Expr::Value(Value::BigInt(Some(count_i))),
                }
            };
            alternatives.push(WhereClause::or(Where::Group(vec![
                WhereClause::and(Where::Compare {
                    left: Expr::Column(qualified(parent.table(), &self.type_column)),
                    op: Operator::Eq,
                    right: Expr::Value(Value::from(name.into_owned())),
                }),
                WhereClause::and(check),
            ])));
        }
        if check_null {
            alternatives.push(WhereClause::or(Where::Null {
                expr: Expr::Column(qualified(parent.table(), &self.type_column)),
                not: false,
            }));
        }
        Ok(Where::Group(alternatives))
    }
}

/// The polymorphic operations of a `morph_to`, for relation names.
pub trait MorphToOps<P: Model>: Send + Sync {
    /// The condition of a polymorphic existence check over `types` (every
    /// known type when `None`), with a condition per type from `refine`.
    fn has_morph(
        &self,
        depth: u32,
        types: Option<&[String]>,
        op: Operator,
        count: u64,
        refine: &mut dyn FnMut(&str) -> Option<Filter>,
    ) -> Result<Where>;

    /// The model registered under morph name `name`.
    #[doc(hidden)]
    fn target_for(&self, name: &str) -> Result<Arc<dyn MorphTarget>>;
}

impl<P: Model> MorphToOps<P> for MorphTo<P> {
    fn has_morph(
        &self,
        depth: u32,
        types: Option<&[String]>,
        op: Operator,
        count: u64,
        refine: &mut dyn FnMut(&str) -> Option<Filter>,
    ) -> Result<Where> {
        self.has_morph_where(depth, types, op, count, refine)
    }

    fn target_for(&self, name: &str) -> Result<Arc<dyn MorphTarget>> {
        self.resolve(name)
    }
}

impl<P: Model> private::Erase<P> for MorphTo<P> {
    fn erase(self) -> Arc<dyn ErasedRelation<P>> {
        let mut unbound = self;
        unbound.parent = None;
        Arc::new(unbound)
    }

    fn rename(&mut self, name: Cow<'static, str>) {
        self.name = name;
    }
}

impl<P: Model> Relation<P> for MorphTo<P> {
    type Value = Option<AnyRecord>;

    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> RelationKind {
        RelationKind::MorphTo
    }

    fn meta(&self) -> RelationMeta {
        let mut meta = RelationMeta::new::<P>(&self.name, RelationKind::MorphTo);
        meta.foreign_key = Some(self.id_column.to_string());
        meta.owner_key = self.owner_key.as_ref().map(ToString::to_string);
        meta.morph_type = Some(self.type_column.to_string());
        meta
    }
}

impl<P: Model> ErasedRelation<P> for MorphTo<P> {
    fn name(&self) -> &str {
        &self.name
    }

    fn meta(&self) -> RelationMeta {
        Relation::meta(self)
    }

    fn related_type(&self) -> Option<TypeId> {
        None
    }

    fn eager_load<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        node: &'a EagerNode,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.load_into(target, parents, node).await })
    }

    fn exists_query(&self, _request: ExistsRequest) -> Result<QueryIr> {
        Err(OrmError::invalid(format!(
            "`{}` of {} is a morph_to relation whose model varies: count or aggregate each type through its own relations",
            self.name,
            P::meta().name()
        )))
    }

    fn touch<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parent: &'a Record<P>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let stored_type = column_value(&parent.model, &self.type_column, &self.name)?;
            let id = column_value(&parent.model, &self.id_column, &self.name)?;
            let (Value::String(Some(name)), false) = (&stored_type, is_null(&id)) else {
                return Ok(());
            };
            let morph_target = self.resolve(name)?;
            let keys = distinct_keys([id]);
            let records = morph_target
                .load(
                    target,
                    MorphLoad {
                        owner_key: self.owner_key.as_deref(),
                        keys: &keys,
                        loads: None,
                        children: &EagerTree::default(),
                        conditions: &[],
                        access: &[],
                        trashed: Trashed::With,
                    },
                )
                .await?;
            for (_, mut record) in records {
                crate::relations::touch::touch_any(target, &mut record).await?;
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
        Box::pin(async move {
            // Gather the loaded records by model, load what each misses,
            // and put them back in place.
            let mut taken: Vec<Option<AnyRecord>> = parents
                .iter_mut()
                .map(|parent| {
                    parent
                        .relations
                        .take::<Option<AnyRecord>>(&self.name)
                        .flatten()
                })
                .collect();
            let mut model_types: Vec<TypeId> = Vec::new();
            let mut targets: Vec<Arc<dyn MorphTarget>> = Vec::new();
            for record in taken.iter().flatten() {
                let name = record.morph_name();
                let morph_target = self.resolve(&name)?;
                if !model_types.contains(&morph_target.type_id()) {
                    model_types.push(morph_target.type_id());
                    targets.push(morph_target);
                }
            }
            for morph_target in targets {
                let indexes: Vec<usize> = taken
                    .iter()
                    .enumerate()
                    .filter(|(_, record)| {
                        record
                            .as_ref()
                            .is_some_and(|record| record.morph_name() == morph_target.morph_name())
                    })
                    .map(|(index, _)| index)
                    .collect();
                let records: Vec<AnyRecord> = indexes
                    .iter()
                    .filter_map(|index| taken[*index].take())
                    .collect();
                let loaded = crate::relations::lazy::load_missing_any(
                    target,
                    morph_target.as_ref(),
                    records,
                    rest,
                )
                .await?;
                for (index, record) in indexes.into_iter().zip(loaded) {
                    taken[index] = Some(record);
                }
            }
            for (parent, record) in parents.iter_mut().zip(taken) {
                parent.relations.set_relation(self.name.clone(), record);
            }
            Ok(())
        })
    }

    fn morph_to(&self) -> Option<&dyn MorphToOps<P>> {
        Some(self)
    }
}

/// The models a polymorphic existence check covers: listed by name,
/// listed by model, or every known model.
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::MorphTypes;
///
/// let deals_or_companies = MorphTypes::of::<Deal>().and::<Company>();
/// let by_name: MorphTypes = ["deal", "company"].into();
/// let notes = Note::query().has_morph("notable", deals_or_companies);
/// # let _ = (by_name, notes, MorphTypes::all());
/// ```
#[derive(Clone, Debug, Default)]
pub struct MorphTypes {
    names: Option<Vec<String>>,
}

impl MorphTypes {
    /// Every model the relation knows (its list, else the morph map).
    pub fn all() -> Self {
        MorphTypes { names: None }
    }

    /// The models registered under `names`.
    pub fn named(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        MorphTypes {
            names: Some(names.into_iter().map(Into::into).collect()),
        }
    }

    /// Model `M` alone.
    pub fn of<M: Model>() -> Self {
        MorphTypes::named([M::morph_name().into_owned()])
    }

    /// These models and model `M`.
    pub fn and<M: Model>(mut self) -> Self {
        self.names
            .get_or_insert_with(Vec::new)
            .push(M::morph_name().into_owned());
        self
    }
}

impl From<&[&str]> for MorphTypes {
    fn from(names: &[&str]) -> Self {
        MorphTypes::named(names.iter().copied())
    }
}

impl<const N: usize> From<[&str; N]> for MorphTypes {
    fn from(names: [&str; N]) -> Self {
        MorphTypes::named(names)
    }
}

impl From<Vec<String>> for MorphTypes {
    fn from(names: Vec<String>) -> Self {
        MorphTypes { names: Some(names) }
    }
}

/// What a `morph_to` points at, for [`where_morphed_to`](Builder::where_morphed_to):
/// a record (`&deal`), several records of one model (`&deals`), any record
/// of a model ([`MorphRef::type_of`]), or nothing ([`MorphRef::null`]).
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::MorphRef;
///
/// let sql = Note::query()
///     .where_morphed_to("notable", MorphRef::type_of::<Deal>())
///     .to_sql(Dialect::Sqlite)
///     .unwrap();
/// assert!(sql.sql.ends_with(r#"WHERE "notes"."notable_type" = ?"#));
/// let loose = Note::query().where_morphed_to("notable", MorphRef::null());
/// # let _ = loose;
/// ```
#[derive(Clone, Debug)]
pub struct MorphRef {
    kind: MorphRefKind,
}

#[derive(Clone, Debug)]
enum MorphRefKind {
    Records(Vec<AnyRecord>),
    Type(String),
    Null,
}

impl MorphRef {
    /// Any record of model `M`.
    pub fn type_of<M: Model>() -> Self {
        MorphRef {
            kind: MorphRefKind::Type(M::morph_name().into_owned()),
        }
    }

    /// Any record of the model registered under `name`.
    pub fn named(name: impl Into<String>) -> Self {
        MorphRef {
            kind: MorphRefKind::Type(name.into()),
        }
    }

    /// No record: the type column is NULL.
    pub fn null() -> Self {
        MorphRef {
            kind: MorphRefKind::Null,
        }
    }
}

impl<M: Model> From<&Record<M>> for MorphRef {
    fn from(record: &Record<M>) -> Self {
        MorphRef {
            kind: MorphRefKind::Records(vec![AnyRecord::new(record.without_relations())]),
        }
    }
}

impl<M: Model> From<&Collection<M>> for MorphRef {
    fn from(records: &Collection<M>) -> Self {
        MorphRef {
            kind: MorphRefKind::Records(
                records
                    .iter()
                    .map(|record| AnyRecord::new(record.without_relations()))
                    .collect(),
            ),
        }
    }
}

impl From<&AnyRecord> for MorphRef {
    fn from(record: &AnyRecord) -> Self {
        MorphRef {
            kind: MorphRefKind::Records(vec![record.clone()]),
        }
    }
}

impl<M: Model> Builder<M> {
    /// Adds a polymorphic existence check of `morph_to` relation `path`.
    #[allow(clippy::too_many_arguments)]
    fn push_has_morph(
        mut self,
        boolean: Boolean,
        path: &str,
        types: MorphTypes,
        op: Operator,
        count: u64,
        refine: &mut dyn FnMut(&str) -> Option<Filter>,
    ) -> Self {
        let relation = match find::<M>(path) {
            Ok(relation) => relation,
            Err(error) => return self.with_error(error.to_string()),
        };
        let Some(morph) = relation.inner.morph_to() else {
            return self.with_error(format!(
                "`{path}` of {} is a {}, not a morph_to relation",
                M::meta().name(),
                relation.kind()
            ));
        };
        match morph.has_morph(self.eager.depth, types.names.as_deref(), op, count, refine) {
            Ok(filter) => {
                self.push_where(boolean, false, filter);
                self
            }
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// Only records whose `morph_to` relation `path` points at an existing
    /// record of one of `types` (`["deal", "company"]`,
    /// `MorphTypes::of::<Deal>()`, or `MorphTypes::all()`).
    pub fn has_morph(self, path: impl RelationPath<M>, types: impl Into<MorphTypes>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::And,
            &text,
            types.into(),
            Operator::Gte,
            1,
            &mut |_| None,
        )
    }

    /// `OR` records whose `morph_to` points at an existing record of one
    /// of `types`.
    pub fn or_has_morph(self, path: impl RelationPath<M>, types: impl Into<MorphTypes>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::Or,
            &text,
            types.into(),
            Operator::Gte,
            1,
            &mut |_| None,
        )
    }

    /// Only records whose `morph_to` relation `path` is of one of `types`
    /// but points at no existing record (a note about a deleted deal).
    /// Records of other types are left out; over every type
    /// (`MorphTypes::all()`), records with a NULL type are included.
    pub fn doesnt_have_morph(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::And,
            &text,
            types.into(),
            Operator::Lt,
            1,
            &mut |_| None,
        )
    }

    /// `OR` records whose `morph_to` is of one of `types` but points at no
    /// existing record.
    pub fn or_doesnt_have_morph(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::Or,
            &text,
            types.into(),
            Operator::Lt,
            1,
            &mut |_| None,
        )
    }

    /// Only records whose `morph_to` points at a record of one of `types`
    /// matching `constraint`, which gets the conditions to fill and the
    /// type's name.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let notes = Note::query().where_has_morph("notable", ["deal", "company"], |q, ty| match ty {
    ///     "deal" => q.where_(Deal::STAGE.eq(Stage::Won)),
    ///     _ => q.where_(Company::ACTIVE.eq(true)),
    /// });
    /// # let _ = notes;
    /// ```
    pub fn where_has_morph(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        mut constraint: impl FnMut(WhereGroup, &str) -> WhereGroup,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::And,
            &text,
            types.into(),
            Operator::Gte,
            1,
            &mut |ty| group_filter(constraint(WhereGroup::new(), ty)),
        )
    }

    /// `OR` records whose `morph_to` points at a record matching
    /// `constraint`.
    pub fn or_where_has_morph(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        mut constraint: impl FnMut(WhereGroup, &str) -> WhereGroup,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::Or,
            &text,
            types.into(),
            Operator::Gte,
            1,
            &mut |ty| group_filter(constraint(WhereGroup::new(), ty)),
        )
    }

    /// Only records whose `morph_to` is of one of `types` but points at no
    /// record matching `constraint` (over every type, NULL types too).
    pub fn where_doesnt_have_morph(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        mut constraint: impl FnMut(WhereGroup, &str) -> WhereGroup,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::And,
            &text,
            types.into(),
            Operator::Lt,
            1,
            &mut |ty| group_filter(constraint(WhereGroup::new(), ty)),
        )
    }

    /// `OR` records whose `morph_to` is of one of `types` but points at no
    /// record matching `constraint`.
    pub fn or_where_doesnt_have_morph(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        mut constraint: impl FnMut(WhereGroup, &str) -> WhereGroup,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has_morph(
            Boolean::Or,
            &text,
            types.into(),
            Operator::Lt,
            1,
            &mut |ty| group_filter(constraint(WhereGroup::new(), ty)),
        )
    }

    /// Only records whose `morph_to` points at a record of one of `types`
    /// matching `filter`.
    pub fn where_morph_relation(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        filter: impl Into<Filter>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has_morph(
            Boolean::And,
            &text,
            types.into(),
            Operator::Gte,
            1,
            &mut |_| Some(filter.clone()),
        )
    }

    /// `OR` records whose `morph_to` points at a record matching `filter`.
    pub fn or_where_morph_relation(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        filter: impl Into<Filter>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has_morph(
            Boolean::Or,
            &text,
            types.into(),
            Operator::Gte,
            1,
            &mut |_| Some(filter.clone()),
        )
    }

    /// Only records whose `morph_to` is of one of `types` but points at no
    /// record matching `filter`.
    pub fn where_morph_doesnt_have_relation(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        filter: impl Into<Filter>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has_morph(
            Boolean::And,
            &text,
            types.into(),
            Operator::Lt,
            1,
            &mut |_| Some(filter.clone()),
        )
    }

    /// `OR` records whose `morph_to` is of one of `types` but points at no
    /// record matching `filter`.
    pub fn or_where_morph_doesnt_have_relation(
        self,
        path: impl RelationPath<M>,
        types: impl Into<MorphTypes>,
        filter: impl Into<Filter>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has_morph(
            Boolean::Or,
            &text,
            types.into(),
            Operator::Lt,
            1,
            &mut |_| Some(filter.clone()),
        )
    }

    /// Only records whose `morph_to` relation `path` points at `target`: a
    /// record, the records of a collection, any record of a model
    /// ([`MorphRef::type_of`]), or none ([`MorphRef::null`]). No query of
    /// the related table: the type and id columns are compared.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::relations::MorphRef;
    ///
    /// let on_deals = Note::query().where_morphed_to("notable", MorphRef::type_of::<Deal>());
    /// # let _ = on_deals;
    /// ```
    pub fn where_morphed_to(self, path: impl RelationPath<M>, target: impl Into<MorphRef>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_morphed_to(Boolean::And, &text, target.into(), false)
    }

    /// Only records whose `morph_to` does not point at `target`.
    pub fn where_not_morphed_to(
        self,
        path: impl RelationPath<M>,
        target: impl Into<MorphRef>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_morphed_to(Boolean::And, &text, target.into(), true)
    }

    /// `OR` records whose `morph_to` points at `target`.
    pub fn or_where_morphed_to(
        self,
        path: impl RelationPath<M>,
        target: impl Into<MorphRef>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_morphed_to(Boolean::Or, &text, target.into(), false)
    }

    /// `OR` records whose `morph_to` does not point at `target`.
    pub fn or_where_not_morphed_to(
        self,
        path: impl RelationPath<M>,
        target: impl Into<MorphRef>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_morphed_to(Boolean::Or, &text, target.into(), true)
    }

    fn push_morphed_to(
        mut self,
        boolean: Boolean,
        path: &str,
        target: MorphRef,
        not: bool,
    ) -> Self {
        match morphed_to_filter::<M>(path, target, not) {
            Ok(filter) => {
                self.push_where(boolean, false, filter);
                self
            }
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// Eager loads `morph_to` relation `path` with per-model loads.
    pub fn with_morph(mut self, path: impl RelationPath<M>, with: MorphWith) -> Self {
        let text = path.relation_path().into_owned();
        match ParsedPath::parse(&text) {
            Ok(parsed) => {
                let node = self.eager.tree.entry(&parsed.segments);
                match &mut node.morph {
                    Some(existing) => existing.merge(&with),
                    None => node.morph = Some(with),
                }
                self
            }
            Err(error) => self.with_error(error.to_string()),
        }
    }
}

/// The conditions of a group, as one filter (`None` when it has none).
fn group_filter(group: WhereGroup) -> Option<Filter> {
    let clauses = group.into_clauses();
    (!clauses.is_empty()).then(|| Filter::from(Where::Group(clauses)))
}

/// The condition of `where_morphed_to`: per model, the type column holds
/// its name and the id column one of its records' keys.
fn morphed_to_filter<M: Model>(path: &str, target: MorphRef, not: bool) -> Result<Where> {
    let relation = find::<M>(path)?;
    let meta = relation.meta();
    if meta.kind != RelationKind::MorphTo {
        return Err(OrmError::invalid(format!(
            "`{path}` of {} is a {}, not a morph_to relation",
            M::meta().name(),
            meta.kind
        )));
    }
    let table = M::meta().table();
    let (Some(type_column), Some(id_column)) = (meta.morph_type, meta.foreign_key) else {
        return Err(OrmError::invalid(format!(
            "the morph_to relation `{path}` of {} has no columns",
            M::meta().name()
        )));
    };
    let type_op = if not {
        Operator::NullSafeEq
    } else {
        Operator::Eq
    };
    let condition = match target.kind {
        MorphRefKind::Null => Where::Null {
            expr: Expr::Column(qualified(table, &type_column)),
            not,
        },
        MorphRefKind::Type(name) => {
            let check = Where::Compare {
                left: Expr::Column(qualified(table, &type_column)),
                op: type_op,
                right: Expr::Value(Value::from(name)),
            };
            if not {
                Where::Group(vec![WhereClause {
                    boolean: Boolean::And,
                    negated: true,
                    filter: check,
                }])
            } else {
                check
            }
        }
        MorphRefKind::Records(records) => {
            if records.is_empty() {
                return Err(OrmError::invalid(format!(
                    "where_morphed_to(`{path}`) was given no records"
                )));
            }
            // By model, in the order the models first appear.
            let mut by_type: Vec<(String, Vec<Value>)> = Vec::new();
            for record in &records {
                let name = record.morph_name().into_owned();
                let key = match &meta.owner_key {
                    Some(column) => record.value(column)?.ok_or_else(|| {
                        OrmError::invalid(format!(
                            "{} has no column `{column}`, which `{path}` points at",
                            record.model_name()
                        ))
                    })?,
                    None => record.key(),
                };
                match by_type.iter_mut().find(|(existing, _)| *existing == name) {
                    Some((_, keys)) => keys.push(key),
                    None => by_type.push((name, vec![key])),
                }
            }
            let alternatives = by_type
                .into_iter()
                .map(|(name, keys)| {
                    let keys = distinct_keys(keys);
                    WhereClause::or(Where::Group(vec![
                        WhereClause::and(Where::Compare {
                            left: Expr::Column(qualified(table, &type_column)),
                            op: type_op.clone(),
                            right: Expr::Value(Value::from(name)),
                        }),
                        WhereClause::and(in_keys(qualified(table, &id_column), &keys)),
                    ]))
                })
                .collect();
            let any = Where::Group(alternatives);
            if not {
                Where::Group(vec![WhereClause {
                    boolean: Boolean::And,
                    negated: true,
                    filter: any,
                }])
            } else {
                any
            }
        }
    };
    Ok(condition)
}

/// The value a `morph_to` stores for `record`'s model: its morph name
/// (checked against a required morph map).
pub(crate) fn stored_type_of<M: Model>() -> Result<Value> {
    Ok(Value::from(type_name_of::<M>()?.into_owned()))
}
