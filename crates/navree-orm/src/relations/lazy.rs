//! Loading relations after the query: onto a record or a collection
//! (`load`, `load_missing`, `load_count`, `load_morph`, ...), one record at
//! a time (`relation_or_load`), the lazy-loading rules ([`LazyLoading`]:
//! prevent it, or load for the whole result at once), and what records do
//! with loaded relations (`push`, `associate`, `dissociate`, pivots).

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};

use navree_kernel::BoxFuture;

use crate::collection::Collection;
use crate::column::IntoExpr;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::ir::AggregateFunction;
use crate::model::Model;
use crate::record::Record;
use crate::relations::Relation;
use crate::relations::aggregates::{AggregateSpec, load_aggregate_on};
use crate::relations::any::AnyRecord;
use crate::relations::belongs_to::BelongsTo;
use crate::relations::core::column_value;
use crate::relations::eager::{Constraint, EagerNode, EagerTree, load_tree};
use crate::relations::morph::{MorphTarget, MorphTo, MorphWith};
use crate::relations::path::{ConstrainPath, ParsedPath, RelationPath};
use crate::relations::pivot::Pivot;
use crate::relations::registry::{self, DynRelation};
use crate::relations::store::{RelationValue, Relations};
use crate::value::{Field, Value};

/// What a record knows about lazy loading: whether it came from a result
/// of several records while lazy loading is prevented, and the group of
/// records it loads relations with (automatic eager loading).
pub(crate) struct LoadState<M: Model> {
    pub(crate) prevent: bool,
    pub(crate) autoload: Option<(Arc<AutoloadGroup<M>>, usize)>,
}

impl<M: Model> Default for LoadState<M> {
    fn default() -> Self {
        LoadState {
            prevent: false,
            autoload: None,
        }
    }
}

impl<M: Model> Clone for LoadState<M> {
    fn clone(&self) -> Self {
        LoadState {
            prevent: self.prevent,
            autoload: self.autoload.clone(),
        }
    }
}

/// Records that load each relation together: the first lazy load of a
/// relation on one of them loads it for all (one query), and the others
/// find it here.
pub(crate) struct AutoloadGroup<M: Model> {
    records: tokio::sync::Mutex<Vec<Record<M>>>,
}

/// Gives `records` one automatic-loading group.
pub(crate) fn group_records<'a, M: Model>(records: impl Iterator<Item = &'a mut Record<M>>) {
    let mut records: Vec<&mut Record<M>> = records.collect();
    if records.is_empty() {
        return;
    }
    let snapshots: Vec<Record<M>> = records
        .iter()
        .map(|record| record.without_relations())
        .collect();
    let group = Arc::new(AutoloadGroup {
        records: tokio::sync::Mutex::new(snapshots),
    });
    for (index, record) in records.iter_mut().enumerate() {
        record.loading.autoload = Some((Arc::clone(&group), index));
    }
}

impl<M: Model> AutoloadGroup<M> {
    /// Relation `name` of the group's record `index`, loading it for every
    /// record of the group the first time.
    async fn load(
        &self,
        target: &Target<'_>,
        relation: &DynRelation<M>,
        name: &str,
        index: usize,
        into: &mut Relations,
    ) -> Result<()> {
        let mut records = self.records.lock().await;
        if !records[index].relations.is_loaded(name) {
            let node = EagerNode {
                name: name.to_owned(),
                autoload: true,
                ..EagerNode::default()
            };
            relation
                .inner
                .eager_load(target, &mut records, &node)
                .await?;
        }
        into.copy_from(&records[index].relations, name);
        Ok(())
    }
}

/// A lazy load that the lazy-loading rules flagged: relation `relation` of
/// a `model` record that came from a result of several records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LazyLoadingViolation {
    model: &'static str,
    relation: String,
}

impl LazyLoadingViolation {
    /// The model.
    pub fn model(&self) -> &'static str {
        self.model
    }

    /// The relation.
    pub fn relation(&self) -> &str {
        &self.relation
    }
}

impl fmt::Display for LazyLoadingViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "relation `{}` of {} was loaded lazily for a record of a multi-record result",
            self.relation, self.model
        )
    }
}

/// Handles a lazy-loading violation: `Ok` lets the load go ahead.
type ViolationHandler = Arc<dyn Fn(&LazyLoadingViolation) -> Result<()> + Send + Sync>;

/// The lazy-loading rules: loading a relation one record at a time is how
/// N+1 query problems start.
///
/// - [`prevented`](LazyLoading::prevented): a lazy load
///   ([`Record::relation_or_load`]) on a record that came from a result of
///   several records is an error, [`OrmError::LazyLoading`] (a record
///   fetched alone may load lazily); [`on_violation`](LazyLoading::on_violation)
///   handles it another way (log it and go ahead).
/// - [`automatic`](LazyLoading::automatic): a lazy load on one record of a
///   result loads the relation for every record of the result, in one
///   query (`with_relationship_autoloading` does this for one collection).
///
/// The rules apply process-wide once [`install`](LazyLoading::install)ed
/// (at boot: the `[orm]` options `prevent_lazy_loading` and
/// `auto_eager_load`), or to one future with [`scope`](LazyLoading::scope).
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::LazyLoading;
///
/// LazyLoading::new()
///     .prevented()
///     .on_violation(|violation| {
///         eprintln!("N+1: {violation}");
///         Ok(())
///     })
///     .install();
/// ```
#[derive(Clone, Default)]
pub struct LazyLoading {
    prevent: bool,
    automatic: bool,
    handler: Option<ViolationHandler>,
}

fn installed() -> &'static RwLock<LazyLoading> {
    static INSTALLED: OnceLock<RwLock<LazyLoading>> = OnceLock::new();
    INSTALLED.get_or_init(|| RwLock::new(LazyLoading::default()))
}

tokio::task_local! {
    static SCOPED: LazyLoading;
}

impl LazyLoading {
    /// Lazy loading allowed, nothing automatic.
    pub fn new() -> Self {
        LazyLoading::default()
    }

    /// Lazy loads of records from multi-record results are violations.
    pub fn prevented(mut self) -> Self {
        self.prevent = true;
        self
    }

    /// A lazy load on one record of a result loads the relation for the
    /// whole result.
    pub fn automatic(mut self) -> Self {
        self.automatic = true;
        self
    }

    /// Handles violations with `handler` (its `Ok` lets the load go ahead)
    /// instead of failing with [`OrmError::LazyLoading`].
    pub fn on_violation(
        mut self,
        handler: impl Fn(&LazyLoadingViolation) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Whether lazy loads of records from multi-record results are
    /// violations.
    pub fn is_prevented(&self) -> bool {
        self.prevent
    }

    /// Whether lazy loads load for the whole result.
    pub fn is_automatic(&self) -> bool {
        self.automatic
    }

    /// Makes these the process-wide rules.
    pub fn install(self) {
        *installed()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = self;
    }

    /// Runs `future` under these rules instead of the process-wide ones.
    pub async fn scope<F: Future>(self, future: F) -> F::Output {
        SCOPED.scope(self, future).await
    }

    /// The rules in effect: the scoped ones, else the installed ones.
    pub fn current() -> LazyLoading {
        SCOPED.try_with(Clone::clone).unwrap_or_else(|_| {
            installed()
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }

    /// A violation: the handler's answer, or the error.
    fn violation(&self, model: &'static str, relation: &str) -> Result<()> {
        let violation = LazyLoadingViolation {
            model,
            relation: relation.to_owned(),
        };
        match &self.handler {
            Some(handler) => handler(&violation),
            None => Err(OrmError::LazyLoading {
                model: Cow::Borrowed(model),
                relation: relation.to_owned(),
            }),
        }
    }
}

impl fmt::Debug for LazyLoading {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyLoading")
            .field("prevent", &self.prevent)
            .field("automatic", &self.automatic)
            .field("handler", &self.handler.is_some())
            .finish()
    }
}

/// Marks the records of one query's result for the lazy-loading rules.
pub(crate) fn mark_result<M: Model>(records: &mut [Record<M>]) {
    if records.len() < 2 {
        return;
    }
    let rules = LazyLoading::current();
    if rules.prevent {
        for record in records.iter_mut() {
            record.loading.prevent = true;
        }
    }
    if rules.automatic {
        group_records(records.iter_mut());
    }
}

/// Saves `record`, then the records of its loaded relations
/// (recursively); `false` when a hook cancelled a save.
pub(crate) fn push_record<'a, 'b, M: Model>(
    record: &'a mut Record<M>,
    target: &'a Target<'b>,
) -> BoxFuture<'a, Result<bool>> {
    Box::pin(async move {
        if !crate::persist::save(target, record).await? {
            return Ok(false);
        }
        record.relations.push_all(target).await
    })
}

/// The eager-load tree of `paths`.
fn tree_of<M: Model>(paths: impl IntoIterator<Item = impl RelationPath<M>>) -> Result<EagerTree> {
    let mut tree = EagerTree::default();
    for path in paths {
        let text = path.relation_path().into_owned();
        let parsed = ParsedPath::parse(&text)?;
        let node = tree.entry(&parsed.segments);
        if parsed.columns.is_some() {
            node.columns = parsed.columns;
        }
    }
    Ok(tree)
}

/// Loads the relations of `tree` that `records` miss, and what is missing
/// under the loaded ones.
pub(crate) fn load_missing_tree<'a, 'b, M: Model>(
    target: &'a Target<'b>,
    records: &'a mut [Record<M>],
    tree: &'a EagerTree,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        if records.is_empty() {
            return Ok(());
        }
        for node in &tree.nodes {
            let relation = registry::find::<M>(&node.name)?;
            let missing: Vec<usize> = records
                .iter()
                .enumerate()
                .filter(|(_, record)| !record.relations.is_loaded(&node.name))
                .map(|(index, _)| index)
                .collect();
            if !missing.is_empty() {
                let mut snapshots: Vec<Record<M>> = missing
                    .iter()
                    .map(|index| records[*index].without_relations())
                    .collect();
                let shallow = EagerNode {
                    children: EagerTree::default(),
                    ..node.clone()
                };
                relation
                    .inner
                    .eager_load(target, &mut snapshots, &shallow)
                    .await?;
                for (index, snapshot) in missing.into_iter().zip(&snapshots) {
                    records[index]
                        .relations
                        .copy_from(&snapshot.relations, &node.name);
                }
            }
            if !node.children.is_empty() {
                relation
                    .inner
                    .load_nested_missing(target, records, &node.children)
                    .await?;
            }
        }
        Ok(())
    })
}

/// For the parents with `name` loaded as `Option<Record<R>>`: loads what
/// `rest` misses into those records.
pub(crate) async fn load_nested_one<P: Model, R: Model>(
    target: &Target<'_>,
    parents: &mut [Record<P>],
    name: &str,
    rest: &EagerTree,
) -> Result<()> {
    let mut owners = Vec::new();
    let mut related = Vec::new();
    for (index, parent) in parents.iter_mut().enumerate() {
        if matches!(
            parent.relations.get::<Option<Record<R>>>(name),
            Some(Some(_))
        ) && let Some(Some(record)) = parent.relations.take::<Option<Record<R>>>(name)
        {
            owners.push(index);
            related.push(record);
        }
    }
    let result = load_missing_tree(target, &mut related, rest).await;
    for (index, record) in owners.into_iter().zip(related) {
        parents[index]
            .relations
            .set_relation(name.to_owned(), Some(record));
    }
    result
}

/// For the parents with `name` loaded as a `Collection<R>`: loads what
/// `rest` misses into those records.
pub(crate) async fn load_nested_many<P: Model, R: Model>(
    target: &Target<'_>,
    parents: &mut [Record<P>],
    name: &str,
    rest: &EagerTree,
) -> Result<()> {
    let mut owners = Vec::new();
    let mut related: Vec<Record<R>> = Vec::new();
    for (index, parent) in parents.iter_mut().enumerate() {
        if let Some(collection) = parent.relations.take::<Collection<R>>(name) {
            let records = collection.into_vec();
            owners.push((index, records.len()));
            related.extend(records);
        }
    }
    let result = load_missing_tree(target, &mut related, rest).await;
    let mut records = related.into_iter();
    for (index, count) in owners {
        let collection: Collection<R> = records.by_ref().take(count).collect();
        parents[index]
            .relations
            .set_relation(name.to_owned(), collection);
    }
    result
}

/// Loads what `rest` misses into `records`, all of `morph_target`'s
/// model.
pub(crate) async fn load_missing_any(
    target: &Target<'_>,
    morph_target: &dyn MorphTarget,
    records: Vec<AnyRecord>,
    rest: &EagerTree,
) -> Result<Vec<AnyRecord>> {
    morph_target.load_missing_into(target, records, rest).await
}

/// Loads `with`'s per-model loads (or, with `aggregate`, those aggregates
/// of the relations it lists) into the records `morph_to` relation `name`
/// holds on `records`, loading the relation first where it is missing.
async fn load_morph_on<M: Model>(
    target: &Target<'_>,
    records: &mut [Record<M>],
    name: &str,
    with: &MorphWith,
    aggregate: Option<&AggregateSpec>,
) -> Result<()> {
    let relation = registry::find::<M>(name)?;
    let Some(morph) = relation.inner.morph_to() else {
        return Err(OrmError::invalid(format!(
            "`{name}` of {} is a {}, not a morph_to relation",
            M::meta().name(),
            relation.kind()
        )));
    };
    let tree = tree_of::<M>([name])?;
    load_missing_tree(target, records, &tree).await?;
    let mut taken: Vec<Option<AnyRecord>> = records
        .iter_mut()
        .map(|record| record.relations.take::<Option<AnyRecord>>(name).flatten())
        .collect();
    let mut names: Vec<String> = Vec::new();
    for record in taken.iter().flatten() {
        let morph_name = record.morph_name().into_owned();
        if !names.contains(&morph_name) {
            names.push(morph_name);
        }
    }
    let mut result = Ok(());
    for morph_name in names {
        let morph_target = match morph.target_for(&morph_name) {
            Ok(morph_target) => morph_target,
            Err(error) => {
                result = Err(error);
                break;
            }
        };
        let Some(loads) = with.for_type(morph_target.type_id()) else {
            continue;
        };
        let indexes: Vec<usize> = taken
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record
                    .as_ref()
                    .is_some_and(|record| record.morph_name() == morph_name)
            })
            .map(|(index, _)| index)
            .collect();
        let of_type: Vec<AnyRecord> = indexes
            .iter()
            .filter_map(|index| taken[*index].take())
            .collect();
        match morph_target
            .load_into(target, of_type, loads, aggregate)
            .await
        {
            Ok(loaded) => {
                for (index, record) in indexes.into_iter().zip(loaded) {
                    taken[index] = Some(record);
                }
            }
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    for (record, value) in records.iter_mut().zip(taken) {
        record.relations.set_relation(name.to_owned(), value);
    }
    result
}

impl<M: Model> Record<M> {
    /// Relation `name`, when it is loaded as a `T` (`Option<Record<R>>` for
    /// a to-one relation, `Collection<R>` for a to-many one,
    /// `Option<AnyRecord>` for a `morph_to`).
    pub fn relation<T: 'static>(&self, name: &str) -> Option<&T> {
        self.relations.get::<T>(name)
    }

    /// Relation `name` for changes, when it is loaded as a `T`.
    pub fn relation_mut<T: 'static>(&mut self, name: &str) -> Option<&mut T> {
        self.relations.get_mut::<T>(name)
    }

    /// Sets relation `name` (as if it was loaded).
    pub fn set_relation<V: RelationValue>(&mut self, name: impl Into<Cow<'static, str>>, value: V) {
        self.relations.set_relation(name, value);
    }

    /// Whether relation `name` is loaded.
    pub fn relation_loaded(&self, name: &str) -> bool {
        self.relations.is_loaded(name)
    }

    /// Forgets relation `name`.
    pub fn unset_relation(&mut self, name: &str) {
        self.relations.unset(name);
    }

    /// Replaces the loaded relations.
    pub fn set_relations(&mut self, relations: Relations) {
        self.relations = relations;
    }

    /// Forgets every loaded relation.
    pub fn unset_relations(&mut self) {
        self.relations.clear();
    }

    /// A copy of the record without its loaded relations.
    pub fn without_relations(&self) -> Record<M> {
        Record {
            model: self.model.clone(),
            original: self.original.clone(),
            exists: self.exists,
            recently_created: self.recently_created,
            changes: self.changes.clone(),
            previous: self.previous.clone(),
            extras: self.extras.clone(),
            relations: Relations::new(),
            loading: LoadState::default(),
        }
    }

    /// A copy of the record without loaded relation `name`.
    pub fn without_relation(&self, name: &str) -> Record<M> {
        let mut copy = self.clone();
        copy.relations.unset(name);
        copy
    }

    /// The loaded value of `relation` (a relation method, `Deal::company`),
    /// typed by the relation: no query.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let deals = Deal::query().with("company").get(&db).await?;
    /// let company: Option<&Record<Company>> = deals[0].loaded(Deal::company).and_then(Option::as_ref);
    /// # Ok(()) }
    /// ```
    pub fn loaded<R: Relation<M>>(&self, relation: impl Fn(&M) -> R) -> Option<&R::Value> {
        let name = relation(&self.model).name().to_owned();
        self.relations.get::<R::Value>(&name)
    }

    /// Relation `name` as a `T`, loaded now if it is not loaded yet, under
    /// the [`LazyLoading`] rules: with automatic loading, the load covers
    /// the record's whole result; when lazy loading is prevented and the
    /// record came from a result of several records, it is a violation.
    pub async fn relation_or_load<T: 'static, E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
    ) -> Result<&T> {
        if !self.relations.is_loaded(name) {
            let relation = registry::find::<M>(name)?;
            let target = exec.target(Access::Read).await?;
            match self.loading.autoload.clone() {
                Some((group, index)) => {
                    group
                        .load(&target, &relation, name, index, &mut self.relations)
                        .await?;
                }
                None => {
                    if self.loading.prevent {
                        LazyLoading::current().violation(M::meta().name(), name)?;
                    }
                    let node = EagerNode {
                        name: name.to_owned(),
                        ..EagerNode::default()
                    };
                    relation
                        .inner
                        .eager_load(&target, std::slice::from_mut(self), &node)
                        .await?;
                }
            }
        }
        self.relations.get::<T>(name).ok_or_else(|| {
            OrmError::invalid(format!(
                "relation `{name}` of {} is loaded, but not as a {}",
                M::meta().name(),
                std::any::type_name::<T>()
            ))
        })
    }

    /// The value of `relation` (a relation method), loaded now if needed
    /// (see [`relation_or_load`](Record::relation_or_load)).
    pub async fn related<R: Relation<M>, E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        relation: impl Fn(&M) -> R,
    ) -> Result<&R::Value> {
        let name = relation(&self.model).name().to_owned();
        self.relation_or_load::<R::Value, E>(exec, &name).await
    }

    /// Eager loads relation `path` onto the record (again, if it is
    /// loaded).
    pub async fn load<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
    ) -> Result<()> {
        let tree = tree_of::<M>([path])?;
        let target = exec.target(Access::Read).await?;
        load_tree(&target, std::slice::from_mut(self), &tree).await
    }

    /// Eager loads every relation path of `paths` onto the record.
    pub async fn load_all<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let tree = tree_of::<M>(paths)?;
        let target = exec.target(Access::Read).await?;
        load_tree(&target, std::slice::from_mut(self), &tree).await
    }

    /// Eager loads relation `path`, constrained by `constraint` (see
    /// [`Builder::with_constrained`]).
    pub async fn load_constrained<Q: 'static, E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl ConstrainPath<M, Q>,
        constraint: impl Fn(Q) -> Q + Send + Sync + 'static,
    ) -> Result<()> {
        let text = path.relation_path().into_owned();
        let parsed = ParsedPath::parse(&text)?;
        let mut tree = EagerTree::default();
        let node = tree.entry(&parsed.segments);
        node.columns = parsed.columns;
        node.constraints.push(Constraint::new(constraint));
        let target = exec.target(Access::Read).await?;
        load_tree(&target, std::slice::from_mut(self), &tree).await
    }

    /// Loads the relations of `paths` the record does not have loaded (and,
    /// down a nested path, the ones its loaded records miss).
    pub async fn load_missing<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let tree = tree_of::<M>(paths)?;
        let target = exec.target(Access::Read).await?;
        load_missing_tree(&target, std::slice::from_mut(self), &tree).await
    }

    async fn load_aggregate_paths<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: Vec<String>,
        spec: AggregateSpec,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        load_aggregate_on(&target, std::slice::from_mut(self), &paths, &spec).await
    }

    /// Loads `{relation}_count` of each relation of `paths` into the
    /// record's extras.
    pub async fn load_count<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let paths = paths
            .into_iter()
            .map(|path| path.relation_path().into_owned())
            .collect();
        self.load_aggregate_paths(exec, paths, AggregateSpec::count())
            .await
    }

    /// Loads `{relation}_exists` of each relation of `paths`.
    pub async fn load_exists<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let paths = paths
            .into_iter()
            .map(|path| path.relation_path().into_owned())
            .collect();
        self.load_aggregate_paths(exec, paths, AggregateSpec::exists())
            .await
    }

    /// Loads `function` of related `column` of relation `path`
    /// (`{relation}_{function}_{column}`).
    pub async fn load_aggregate<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
        function: AggregateFunction,
    ) -> Result<()> {
        let spec = AggregateSpec::function(function, column)?;
        let paths = vec![path.relation_path().into_owned()];
        self.load_aggregate_paths(exec, paths, spec).await
    }

    /// Loads `{relation}_sum_{column}`.
    pub async fn load_sum<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Sum)
            .await
    }

    /// Loads `{relation}_avg_{column}`.
    pub async fn load_avg<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Avg)
            .await
    }

    /// Loads `{relation}_min_{column}`.
    pub async fn load_min<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Min)
            .await
    }

    /// Loads `{relation}_max_{column}`.
    pub async fn load_max<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Max)
            .await
    }

    /// Loads `morph_to` relation `name` (if missing), then `with`'s
    /// per-model loads into the record it holds.
    pub async fn load_morph<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        load_morph_on(&target, std::slice::from_mut(self), name, &with, None).await
    }

    /// Loads, onto the record `morph_to` relation `name` holds, the counts
    /// of the relations `with` lists for its model (with `on` or
    /// `count_on`): `{relation}_count`.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::relations::MorphWith;
    ///
    /// # async fn demo(db: Db, mut note: Record<Note>) -> Result<()> {
    /// note.load_morph_count(&db, "notable", MorphWith::new().count_on::<Deal>(["activities"]))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn load_morph_count<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        let spec = AggregateSpec::count();
        load_morph_on(
            &target,
            std::slice::from_mut(self),
            name,
            &with,
            Some(&spec),
        )
        .await
    }

    /// Loads `function` of `column` of the relations `with` lists for its
    /// model (with `on` or `count_on`), onto the record `morph_to`
    /// relation `name` holds: `{relation}_{function}_{column}`.
    pub async fn load_morph_aggregate<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
        column: impl IntoExpr,
        function: AggregateFunction,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        let spec = AggregateSpec::function(function, column)?;
        load_morph_on(
            &target,
            std::slice::from_mut(self),
            name,
            &with,
            Some(&spec),
        )
        .await
    }

    /// [`load_morph_aggregate`](Record::load_morph_aggregate) with `SUM`.
    pub async fn load_morph_sum<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_morph_aggregate(exec, name, with, column, AggregateFunction::Sum)
            .await
    }

    /// [`load_morph_aggregate`](Record::load_morph_aggregate) with `AVG`.
    pub async fn load_morph_avg<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_morph_aggregate(exec, name, with, column, AggregateFunction::Avg)
            .await
    }

    /// [`load_morph_aggregate`](Record::load_morph_aggregate) with `MIN`.
    pub async fn load_morph_min<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_morph_aggregate(exec, name, with, column, AggregateFunction::Min)
            .await
    }

    /// [`load_morph_aggregate`](Record::load_morph_aggregate) with `MAX`.
    pub async fn load_morph_max<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_morph_aggregate(exec, name, with, column, AggregateFunction::Max)
            .await
    }

    /// Saves the record, then every record of its loaded relations, and
    /// theirs (`false` when a hook cancelled a save; the records before it
    /// stay saved).
    pub async fn push<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Write).await?;
        push_record(self, &target).await
    }

    /// [`push`](Record::push) without lifecycle hooks.
    pub async fn push_quietly<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        crate::hooks::quietly(self.push(exec)).await
    }

    /// Points `relation` (a `belongs_to` or `morph_to` relation method,
    /// `Deal::company`) at `related`: sets the foreign key (and the type of
    /// a `morph_to`) and the loaded relation. Saving is separate.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, mut deal: Record<Deal>, acme: Record<Company>) -> Result<()> {
    /// deal.associate(Deal::company, &acme)?;
    /// assert_eq!(deal.company_id, Some(acme.id));
    /// deal.save(&uow).await?;
    /// # Ok(()) }
    /// ```
    pub fn associate<R, X>(&mut self, relation: impl Fn(&M) -> R, related: &X) -> Result<&mut Self>
    where
        R: Associate<M, X>,
    {
        relation(&self.model).associate_with(self, related)?;
        Ok(self)
    }

    /// Clears `relation` (a `belongs_to` or `morph_to` relation method):
    /// the foreign key (and type) NULL, the loaded relation `None`.
    pub fn dissociate<R>(&mut self, relation: impl Fn(&M) -> R) -> Result<&mut Self>
    where
        R: Dissociate<M>,
    {
        relation(&self.model).dissociate_from(self)?;
        Ok(self)
    }

    /// Gives the record an automatic-loading group of its own: relations
    /// loaded lazily on its loaded records load for all of them at once.
    pub fn with_relationship_autoloading(mut self) -> Self {
        group_records(std::iter::once(&mut self));
        self
    }

    /// The pivot row a many-to-many relation loaded the record with (under
    /// `pivot`).
    pub fn pivot(&self) -> Option<&Pivot> {
        self.relations.get::<Pivot>("pivot")
    }

    /// The typed pivot record (`.using::<P>()`) the record was loaded with
    /// (under `pivot`).
    pub fn pivot_as<P: Model>(&self) -> Option<&Record<P>> {
        self.relations.get::<Record<P>>("pivot")
    }

    /// Column `column` of the pivot row the record was loaded with (under
    /// `pivot`) read as `T`: from a [`Pivot`], or a typed pivot record's
    /// columns. `None` when there is no pivot or no such column.
    pub fn pivot_value<T: Field>(&self, column: &str) -> Result<Option<T>> {
        match self.relations.column("pivot", column) {
            Some(value) => T::from_value(value).map(Some).map_err(|error| {
                crate::row::decode_error(
                    format!("the pivot of {}", M::meta().name()),
                    column,
                    T::type_name(),
                    &error,
                )
            }),
            None => Ok(None),
        }
    }
}

/// A relation that can point a record at another (`associate`).
pub trait Associate<M: Model, X> {
    /// Points `child`'s relation at `related`.
    #[doc(hidden)]
    fn associate_with(&self, child: &mut Record<M>, related: &X) -> Result<()>;
}

/// A relation that can be cleared (`dissociate`).
pub trait Dissociate<M: Model> {
    /// Clears `child`'s relation.
    #[doc(hidden)]
    fn dissociate_from(&self, child: &mut Record<M>) -> Result<()>;
}

impl<M: Model, R: Model> Associate<M, Record<R>> for BelongsTo<M, R> {
    fn associate_with(&self, child: &mut Record<M>, related: &Record<R>) -> Result<()> {
        let owner = column_value(&related.model, &self.owner_key, &self.core.name)?;
        child.model.set_value(&self.foreign_key, owner)?;
        child
            .relations
            .set_relation(self.core.name.to_string(), Some(related.clone()));
        Ok(())
    }
}

impl<M: Model, R: Model> Dissociate<M> for BelongsTo<M, R> {
    fn dissociate_from(&self, child: &mut Record<M>) -> Result<()> {
        child
            .model
            .set_value(&self.foreign_key, Value::String(None))?;
        child
            .relations
            .set_relation(self.core.name.to_string(), None::<Record<R>>);
        Ok(())
    }
}

impl<M: Model, X: Model> Associate<M, Record<X>> for MorphTo<M> {
    fn associate_with(&self, child: &mut Record<M>, related: &Record<X>) -> Result<()> {
        let id = match &self.owner_key {
            Some(column) => column_value(&related.model, column, &self.name)?,
            None => related.key().to_value(),
        };
        child.model.set_value(&self.id_column, id)?;
        child.model.set_value(
            &self.type_column,
            crate::relations::morph::stored_type_of::<X>()?,
        )?;
        child
            .relations
            .set_relation(self.name.to_string(), Some(AnyRecord::new(related.clone())));
        Ok(())
    }
}

impl<M: Model> Dissociate<M> for MorphTo<M> {
    fn dissociate_from(&self, child: &mut Record<M>) -> Result<()> {
        child
            .model
            .set_value(&self.id_column, Value::String(None))?;
        child
            .model
            .set_value(&self.type_column, Value::String(None))?;
        child
            .relations
            .set_relation(self.name.to_string(), None::<AnyRecord>);
        Ok(())
    }
}

impl<M: Model> Collection<M> {
    async fn load_tree_into<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        tree: &EagerTree,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        load_tree(&target, self, tree).await
    }

    /// Eager loads relation `path` onto every record (one query per
    /// relation and level).
    pub async fn load<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
    ) -> Result<()> {
        let tree = tree_of::<M>([path])?;
        self.load_tree_into(exec, &tree).await
    }

    /// Eager loads every relation path of `paths` onto every record.
    pub async fn load_all<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let tree = tree_of::<M>(paths)?;
        self.load_tree_into(exec, &tree).await
    }

    /// Eager loads relation `path` onto every record, constrained by
    /// `constraint`.
    pub async fn load_constrained<Q: 'static, E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl ConstrainPath<M, Q>,
        constraint: impl Fn(Q) -> Q + Send + Sync + 'static,
    ) -> Result<()> {
        let text = path.relation_path().into_owned();
        let parsed = ParsedPath::parse(&text)?;
        let mut tree = EagerTree::default();
        let node = tree.entry(&parsed.segments);
        node.columns = parsed.columns;
        node.constraints.push(Constraint::new(constraint));
        self.load_tree_into(exec, &tree).await
    }

    /// Loads the relations of `paths` the records miss (and, down nested
    /// paths, what their loaded records miss).
    pub async fn load_missing<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let tree = tree_of::<M>(paths)?;
        let target = exec.target(Access::Read).await?;
        load_missing_tree(&target, self, &tree).await
    }

    async fn load_aggregate_paths<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: Vec<String>,
        spec: AggregateSpec,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        load_aggregate_on(&target, self, &paths, &spec).await
    }

    /// Loads `{relation}_count` of each relation of `paths` onto every
    /// record (one query).
    pub async fn load_count<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let paths = paths
            .into_iter()
            .map(|path| path.relation_path().into_owned())
            .collect();
        self.load_aggregate_paths(exec, paths, AggregateSpec::count())
            .await
    }

    /// Loads `{relation}_exists` of each relation of `paths`.
    pub async fn load_exists<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        paths: impl IntoIterator<Item = impl RelationPath<M>>,
    ) -> Result<()> {
        let paths = paths
            .into_iter()
            .map(|path| path.relation_path().into_owned())
            .collect();
        self.load_aggregate_paths(exec, paths, AggregateSpec::exists())
            .await
    }

    /// Loads `function` of related `column` of relation `path`.
    pub async fn load_aggregate<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
        function: AggregateFunction,
    ) -> Result<()> {
        let spec = AggregateSpec::function(function, column)?;
        let paths = vec![path.relation_path().into_owned()];
        self.load_aggregate_paths(exec, paths, spec).await
    }

    /// Loads `{relation}_sum_{column}`.
    pub async fn load_sum<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Sum)
            .await
    }

    /// Loads `{relation}_avg_{column}`.
    pub async fn load_avg<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Avg)
            .await
    }

    /// Loads `{relation}_min_{column}`.
    pub async fn load_min<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Min)
            .await
    }

    /// Loads `{relation}_max_{column}`.
    pub async fn load_max<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
    ) -> Result<()> {
        self.load_aggregate(exec, path, column, AggregateFunction::Max)
            .await
    }

    /// Loads `morph_to` relation `name` where missing, then `with`'s
    /// per-model loads into the records it holds (one query per model and
    /// relation).
    pub async fn load_morph<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        load_morph_on(&target, self, name, &with, None).await
    }

    /// Loads the counts of the relations `with` lists (with `on` or
    /// `count_on`) onto the records `morph_to` relation `name` holds.
    pub async fn load_morph_count<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        name: &str,
        with: MorphWith,
    ) -> Result<()> {
        let target = exec.target(Access::Read).await?;
        let spec = AggregateSpec::count();
        load_morph_on(&target, self, name, &with, Some(&spec)).await
    }

    /// Gives the records one automatic-loading group: a relation loaded
    /// lazily on one of them loads for all of them (one query).
    pub fn with_relationship_autoloading(mut self) -> Self {
        group_records(self.iter_mut());
        self
    }
}

/// Loads the relations of `paths` that the records of model `M` among
/// `records` miss (a `morph_to`'s mixed results); records of other models
/// are left alone.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, mut notables: Vec<navree_orm::relations::AnyRecord>) -> Result<()> {
/// navree_orm::relations::load_missing_chain::<Deal, _>(&db, &mut notables, ["company"]).await?;
/// # Ok(()) }
/// ```
pub async fn load_missing_chain<M: Model, E: Executor + ?Sized>(
    exec: &E,
    records: &mut [AnyRecord],
    paths: impl IntoIterator<Item = impl RelationPath<M>>,
) -> Result<()> {
    let tree = tree_of::<M>(paths)?;
    let indexes: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.is::<M>())
        .map(|(index, _)| index)
        .collect();
    if indexes.is_empty() {
        return Ok(());
    }
    let mut typed: Vec<Record<M>> = Vec::with_capacity(indexes.len());
    for index in &indexes {
        let placeholder = AnyRecord::new(Record::<M>::stored_placeholder(&records[*index])?);
        let record = std::mem::replace(&mut records[*index], placeholder);
        match record.downcast::<M>() {
            Ok(record) => typed.push(record),
            Err(_) => unreachable!("the model type was checked"),
        }
    }
    let target = exec.target(Access::Read).await?;
    let result = load_missing_tree(&target, &mut typed, &tree).await;
    for (index, record) in indexes.into_iter().zip(typed) {
        records[index] = AnyRecord::new(record);
    }
    result
}

impl<M: Model> Record<M> {
    /// A stand-in for `record` while its model's record is taken out of
    /// it: the same record, cloned without relations.
    fn stored_placeholder(record: &AnyRecord) -> Result<Record<M>> {
        record
            .downcast_ref::<M>()
            .map(Record::without_relations)
            .ok_or_else(|| OrmError::invalid(format!("not a {}", M::meta().name())))
    }
}

/// Loads again the relations `record` has loaded that are relations of
/// its model (what `refresh` does after reloading the columns); pivots and
/// values set by hand are left as they are.
pub(crate) async fn reload_relations<M: Model>(
    target: &Target<'_>,
    record: &mut Record<M>,
) -> Result<()> {
    let names: Vec<String> = record
        .relations
        .names()
        .filter(|name| M::relation(name).is_some())
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        return Ok(());
    }
    let mut tree = EagerTree::default();
    for name in &names {
        tree.entry(std::slice::from_ref(name));
    }
    load_tree(target, std::slice::from_mut(record), &tree).await
}
