//! Model factories: records for tests and seeders, built from a definition
//! with fake data, states, sequences and relationships.
//!
//! A factory implements [`ModelFactory`] for one model; `#[model(factory =
//! DealFactory)]` links the two, so `Deal::factory()` starts a [`Factory`]
//! builder:
//!
//! ```no_run
//! # use navree_orm::__doc_factories::*;
//! # async fn demo(db: Db) -> Result<()> {
//! // Three won deals of one new company, each with two activities.
//! let deals = Deal::factory()
//!     .count(3)
//!     .state(DealFactory::won)
//!     .for_(Company::factory().set(Company::NAME, "Agra"), Deal::COMPANY_ID)
//!     .has(Activity::factory().count(2), Activity::DEAL_ID)
//!     .create(&db)
//!     .await?;
//!
//! // Unsaved records, no database involved.
//! let draft = Deal::factory().set(Deal::TITLE, "Draft").make_one()?;
//! assert!(!draft.exists());
//! # Ok(()) }
//! ```
//!
//! - **Definitions** build one model from a [`FactoryContext`]: its index in
//!   the batch, fake data ([`Faker`], seeded with [`Factory::seed`] for
//!   reproducible runs), the current time and the parents it belongs to
//!   ([`FactoryContext::belongs_to`]).
//! - **States** change the model after the definition, in order:
//!   closures and named functions ([`Factory::state`]), single columns
//!   ([`Factory::set`]), [`Sequence`]s, soft-deleted records
//!   ([`Factory::trashed`]).
//! - **Relationships**: parents shared by the batch ([`Factory::for_`]),
//!   children of each record ([`Factory::has`]), many-to-many attachments
//!   ([`Factory::has_attached`]), stored records reused instead of new
//!   parents ([`Factory::recycle`]), by typed columns or by the names a
//!   factory declares in [`ModelFactory::relationships`].
//! - **Results**: unsaved records ([`Factory::make`]), generated values only
//!   ([`Factory::raw`]), stored records with their relationships
//!   ([`Factory::create`], with model events or quietly), or one bulk
//!   `INSERT` ([`Factory::insert`]); hooks run after making and after
//!   creating.
//!
//! Records are created through the executor given (a `Db`, a `Uow`, ...),
//! so they join the unit of work, record change events and fill the
//! current workspace like any other save.

mod bulk;
mod context;
mod faker;
pub(crate) mod quiet;
mod relationships;
mod sequence;
mod sources;

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use navree_db::Db;
use navree_kernel::BoxFuture;

pub use context::{Attributes, FactoryContext};
pub use faker::{FakeNumber, Faker};
pub use relationships::{BelongsTo, HasMany, Pivot, RelationshipInfo, Relationships};
pub use sequence::{Sequence, SequenceIndex};
pub use sources::{IntoRecords, LazyFactory, ParentHandle, ParentSource};

use crate::collection::Collection;
use crate::column::{Assignment, Column};
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target, truncate_to_micros};
use crate::model::{Model, PrimaryKey};
use crate::persist;
use crate::record::Record;
use crate::value::{Value, ValueFor};

use self::context::PendingParent;
use self::relationships::{AttachRelated, Attachable, ChildLink, HasChildren};
use self::sources::{Recycle, ResolveParent};

/// A factory of one model: its default state, plus optional defaults for
/// every builder ([`configure`](ModelFactory::configure)) and the
/// relationships builders can name ([`relationships`](ModelFactory::relationships)).
///
/// ```
/// use chrono::{DateTime, Utc};
/// use navree_orm::factory::{Factory, FactoryContext, ModelFactory};
/// use navree_orm::prelude::*;
///
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// #[model(timestamps, factory = TaskFactory)]
/// pub struct Task {
///     id: i64,
///     title: String,
///     done: bool,
///     created_at: DateTime<Utc>,
///     updated_at: DateTime<Utc>,
/// }
///
/// #[derive(Default)]
/// pub struct TaskFactory;
///
/// impl ModelFactory for TaskFactory {
///     type Model = Task;
///
///     fn definition(&self, ctx: &mut FactoryContext<'_, Task>) -> Task {
///         Task { title: ctx.fake().sentence(), ..Task::default() }
///     }
/// }
///
/// impl TaskFactory {
///     /// A named state: `Task::factory().state(TaskFactory::done)`.
///     pub fn done(task: &mut Task) {
///         task.done = true;
///     }
/// }
///
/// let tasks = Task::factory().count(2).state(TaskFactory::done).make()?;
/// assert!(tasks.iter().all(|task| task.done && !task.title.is_empty()));
/// # Ok::<(), navree_orm::OrmError>(())
/// ```
pub trait ModelFactory: Send + Sync + Sized + 'static {
    /// The model this factory builds.
    type Model: Model;

    /// The model's default state: one new model, usually with fake data
    /// from `ctx.fake()`. Keys and timestamps left empty are filled when the
    /// record is saved.
    fn definition(&self, ctx: &mut FactoryContext<'_, Self::Model>) -> Self::Model;

    /// Defaults for every builder of this factory (`Model::factory()`,
    /// [`Factory::of`]): hooks, states. Returns the builder unchanged by
    /// default.
    fn configure(factory: Factory<Self>) -> Factory<Self> {
        factory
    }

    /// The relationships of the model that builders may name in
    /// [`Factory::has`], [`Factory::for_`] and [`Factory::has_attached`]
    /// (`.has(Activity::factory(), "activities")`). None by default; typed
    /// columns work without declaring anything.
    fn relationships(relationships: &mut Relationships<Self::Model>) {
        let _ = relationships;
    }
}

/// A model with a factory, linked by `#[model(factory = DealFactory)]`,
/// which also gives the model an inherent `factory()`.
pub trait HasFactory: Model {
    /// The model's factory.
    type Factory: ModelFactory<Model = Self> + Default;

    /// A builder of the model's factory.
    fn factory() -> Factory<Self::Factory> {
        Factory::new()
    }
}

/// Defaults for the factories of an app: register a value with
/// `override_value(Arc::new(FactoryOptions { .. }))` (or a provider), and
/// factories creating records through an executor of that app read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FactoryOptions {
    /// Whether `create` makes the parents that definitions declare with
    /// [`FactoryContext::belongs_to`] (default `true`;
    /// [`Factory::without_parents`] turns it off for one builder).
    pub expand_relationships: bool,
    /// The seed of every factory that has none of its own, for runs that
    /// generate the same data every time (default `None`: random).
    pub seed: Option<u64>,
}

impl FactoryOptions {
    /// The defaults: parents are created, seeds are random.
    pub const fn new() -> Self {
        FactoryOptions {
            expand_relationships: true,
            seed: None,
        }
    }

    /// Whether definitions' parents are created.
    pub const fn expand_relationships(mut self, expand: bool) -> Self {
        self.expand_relationships = expand;
        self
    }

    /// The default seed.
    pub const fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

impl Default for FactoryOptions {
    fn default() -> Self {
        FactoryOptions::new()
    }
}

/// A state: changes a model after its definition.
type StateFn<M> =
    Arc<dyn Fn(&mut M, &mut FactoryContext<'_, M>) -> Result<()> + Send + Sync + 'static>;

/// A hook after a record is made.
type AfterMaking<M> = Arc<dyn Fn(&mut Record<M>) + Send + Sync + 'static>;

/// A hook after a record is created.
type AfterCreating<M> = Arc<
    dyn for<'a> Fn(&'a mut Record<M>, &'a Target<'a>) -> BoxFuture<'a, Result<()>>
        + Send
        + Sync
        + 'static,
>;

/// A parent of the whole batch (`for_`): where it comes from and how
/// records point at it.
struct ParentLink {
    source: Arc<dyn ResolveParent>,
    owner_key: Option<Cow<'static, str>>,
    foreign_key: Cow<'static, str>,
    morph: Option<(Cow<'static, str>, Cow<'static, str>)>,
}

impl Clone for ParentLink {
    fn clone(&self) -> Self {
        ParentLink {
            source: self.source.clone(),
            owner_key: self.owner_key.clone(),
            foreign_key: self.foreign_key.clone(),
            morph: self.morph.clone(),
        }
    }
}

impl ParentLink {
    /// The column values pointing a record at the parent whose key (or
    /// owner key) is `value`.
    fn assignments(&self, value: Value) -> Vec<(Cow<'static, str>, Value)> {
        let mut values = vec![(self.foreign_key.clone(), value)];
        if let Some((column, morph_type)) = &self.morph {
            values.push((column.clone(), Value::from(morph_type.to_string())));
        }
        values
    }
}

/// Builds records of `F::Model` with factory `F`: start one with
/// `Model::factory()` (or [`Factory::new`] / [`Factory::of`]), chain
/// counts, states, sequences and relationships, then
/// [`make`](Factory::make), [`create`](Factory::create),
/// [`raw`](Factory::raw) or [`insert`](Factory::insert).
///
/// Builders are values: every method returns a new builder, and a builder
/// can be cloned and reused.
pub struct Factory<F: ModelFactory> {
    factory: Arc<F>,
    count: Option<usize>,
    states: Vec<StateFn<F::Model>>,
    parents: Vec<ParentLink>,
    children: Vec<Arc<dyn ChildLink<F::Model>>>,
    pub(crate) recycle: Recycle,
    after_making: Vec<AfterMaking<F::Model>>,
    after_creating: Vec<AfterCreating<F::Model>>,
    expand_parents: Option<bool>,
    excluded_parents: Vec<Cow<'static, str>>,
    connection: Option<Cow<'static, str>>,
    seed: Option<u64>,
    errors: Vec<String>,
}

impl<F: ModelFactory + Default> Factory<F> {
    /// A builder of `F::default()`, configured by [`ModelFactory::configure`]
    /// (what `Model::factory()` returns).
    pub fn new() -> Self {
        Factory::of(F::default())
    }
}

impl<F: ModelFactory + Default> Default for Factory<F> {
    fn default() -> Self {
        Factory::new()
    }
}

impl<F: ModelFactory> Factory<F> {
    /// A builder of `factory` (a factory with fields of its own),
    /// configured by [`ModelFactory::configure`].
    pub fn of(factory: F) -> Self {
        F::configure(Factory {
            factory: Arc::new(factory),
            count: None,
            states: Vec::new(),
            parents: Vec::new(),
            children: Vec::new(),
            recycle: Recycle::default(),
            after_making: Vec::new(),
            after_creating: Vec::new(),
            expand_parents: None,
            excluded_parents: Vec::new(),
            connection: None,
            seed: None,
            errors: Vec::new(),
        })
    }

    /// The factory.
    pub fn definition(&self) -> &F {
        &self.factory
    }

    // ---------------------------------------------------------------------
    // How many, and how
    // ---------------------------------------------------------------------

    /// Builds `count` records (one by default).
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// Changes each model after its definition (and the states before
    /// this one): a closure or a named function of the factory.
    ///
    /// ```
    /// # use navree_orm::__doc_factories::*;
    /// let won = Deal::factory().state(DealFactory::won).state(|d| d.amount_cents = 50_000).make_one()?;
    /// assert_eq!((won.stage, won.amount_cents), (Stage::Won, 50_000));
    /// # Ok::<(), navree_orm::OrmError>(())
    /// ```
    pub fn state(self, state: impl Fn(&mut F::Model) + Send + Sync + 'static) -> Self {
        self.push_state(move |model, _| {
            state(model);
            Ok(())
        })
    }

    /// Like [`state`](Factory::state), with the [`FactoryContext`]: the
    /// record's index, fake data, the time, parents.
    pub fn state_with(
        self,
        state: impl Fn(&mut F::Model, &mut FactoryContext<'_, F::Model>) + Send + Sync + 'static,
    ) -> Self {
        self.push_state(move |model, ctx| {
            state(model, ctx);
            Ok(())
        })
    }

    /// A state applied before every state added so far (right after the
    /// definition).
    pub fn prepend_state(mut self, state: impl Fn(&mut F::Model) + Send + Sync + 'static) -> Self {
        self.states.insert(
            0,
            Arc::new(move |model, _| {
                state(model);
                Ok(())
            }),
        );
        self
    }

    /// Sets `column` to `value` on every record.
    pub fn set<T>(self, column: Column<F::Model, T>, value: impl ValueFor<T>) -> Self {
        let assignment = column.set(value);
        self.push_state(move |model, _| apply_assignment::<F::Model>(model, &assignment))
    }

    /// Records take the states of `sequence` in turn: the first record the
    /// first state, and so on, starting over after the last. Fixed states
    /// can be given as an array of functions (or closures without
    /// captures).
    ///
    /// ```
    /// # use navree_orm::__doc_factories::*;
    /// use navree_orm::factory::Sequence;
    ///
    /// let deals = Deal::factory()
    ///     .count(3)
    ///     .sequence([DealFactory::won, DealFactory::lost])
    ///     .sequence(Sequence::column(Deal::TITLE, ["A", "B", "C"]))
    ///     .make()?;
    /// let stages: Vec<Stage> = deals.iter().map(|d| d.stage).collect();
    /// assert_eq!(stages, [Stage::Won, Stage::Lost, Stage::Won]);
    /// assert_eq!(deals[2].title, "C");
    /// # Ok::<(), navree_orm::OrmError>(())
    /// ```
    pub fn sequence(self, sequence: impl Into<Sequence<F::Model>>) -> Self {
        let sequence = sequence.into();
        self.push_state(move |model, ctx| sequence.apply(model, ctx.index()))
    }

    /// A [`sequence`](Factory::sequence) that also sets the count to its
    /// length: one record per state.
    pub fn for_each_sequence(self, sequence: impl Into<Sequence<F::Model>>) -> Self {
        let sequence = sequence.into();
        let count = sequence.len();
        self.sequence(sequence).count(count)
    }

    /// Records take every combination of one state from each of
    /// `sequences` in turn (see [`Sequence::cross_join`]); with no count
    /// set, one record per combination.
    pub fn cross_join_sequence(
        self,
        sequences: impl IntoIterator<Item = Sequence<F::Model>>,
    ) -> Self {
        let joined = Sequence::cross_join(sequences);
        let count = joined.len();
        let factory = self.sequence(joined);
        match factory.count {
            Some(_) => factory,
            None => factory.count(count),
        }
    }

    /// Soft-deleted records: the deleted-at column is set to a day before
    /// now. Fails when the model does not soft-delete.
    pub fn trashed(self) -> Self {
        self.trashed_with(None)
    }

    /// Soft-deleted records, deleted at `at`.
    pub fn trashed_at(self, at: DateTime<Utc>) -> Self {
        self.trashed_with(Some(at))
    }

    fn trashed_with(self, at: Option<DateTime<Utc>>) -> Self {
        let meta = F::Model::meta();
        let Some(column) = meta.deleted_at() else {
            let name = meta.name();
            return self.with_error(format!(
                "{name} does not soft-delete: trashed() needs #[model(soft_deletes)] on {name}"
            ));
        };
        self.push_state(move |model, ctx| {
            let at = at.unwrap_or_else(|| ctx.now() - Duration::days(1));
            model.touch_timestamp(column, at)
        })
    }

    /// Applies `then` when `condition` holds.
    pub fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if condition { then(self) } else { self }
    }

    /// The seed of the fake data, so every run builds the same records
    /// (unseeded factories use a random seed per call).
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Creates the records on the app's database connection `name` (a
    /// named `Db` provider of the executor's app) instead of the executor
    /// given to `create`.
    pub fn connection(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.connection = Some(name.into());
        self
    }

    // ---------------------------------------------------------------------
    // Hooks
    // ---------------------------------------------------------------------

    /// Runs `hook` on each record once it is made (by `make` and by
    /// `create`, before saving).
    pub fn after_making(
        mut self,
        hook: impl Fn(&mut Record<F::Model>) + Send + Sync + 'static,
    ) -> Self {
        self.after_making.push(Arc::new(hook));
        self
    }

    /// Runs `hook` on each record once it is created, with the executor it
    /// was created on (to create related records in the same transaction).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_factories::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let deals = Deal::factory()
    ///     .after_creating(|deal, exec| Box::pin(async move {
    ///         Activity::factory().set(Activity::DEAL_ID, deal.id).create(exec).await?;
    ///         Ok(())
    ///     }))
    ///     .create(&db)
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub fn after_creating(
        mut self,
        hook: impl for<'a> Fn(&'a mut Record<F::Model>, &'a Target<'a>) -> BoxFuture<'a, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.after_creating.push(Arc::new(hook));
        self
    }

    /// Drops the after-making hooks (also those `configure` added).
    pub fn without_after_making(mut self) -> Self {
        self.after_making.clear();
        self
    }

    /// Drops the after-creating hooks (also those `configure` added).
    pub fn without_after_creating(mut self) -> Self {
        self.after_creating.clear();
        self
    }

    // ---------------------------------------------------------------------
    // Relationships
    // ---------------------------------------------------------------------

    /// Points every record at one parent from `parent` through `link`: a
    /// factory (one parent is created for the whole batch, or a recycled
    /// record is used), a [`LazyFactory`], or a stored record. `link` is the
    /// foreign key column of this model, a [`BelongsTo`], or the name of a
    /// relationship the factory declares.
    ///
    /// The parent's columns are set before the states, so a state can
    /// still change them.
    pub fn for_<S: ParentSource>(
        self,
        parent: S,
        link: impl Into<BelongsTo<F::Model, S::Parent>>,
    ) -> Self {
        let link = link.into();
        let link = match link.named {
            Some(name) => {
                match Relationships::<F::Model>::of::<F>().belongs_to_link::<S::Parent>(name) {
                    Ok(link) => link,
                    Err(error) => return self.with_error(problem(error)),
                }
            }
            None => link,
        };
        let mut factory = self;
        factory.parents.push(ParentLink {
            source: parent.into_parent().0,
            owner_key: link.owner_key.clone(),
            foreign_key: link.foreign_key.clone(),
            morph: link.morph.clone(),
        });
        factory
    }

    /// [`for_`](Factory::for_) with a stored record.
    pub fn for_record<P: Model>(
        self,
        parent: &Record<P>,
        link: impl Into<BelongsTo<F::Model, P>>,
    ) -> Self {
        self.for_(parent, link)
    }

    /// Creates `children` for each record once it is stored, pointing at it
    /// through `link`: the child's foreign key column, a [`HasMany`], or
    /// the name of a relationship the factory declares.
    pub fn has<CF: ModelFactory>(
        self,
        children: Factory<CF>,
        link: impl Into<HasMany<F::Model, CF::Model>>,
    ) -> Self {
        let link = link.into();
        let link = match link.named {
            Some(name) => {
                match Relationships::<F::Model>::of::<F>().has_many_link::<CF::Model>(name) {
                    Ok(link) => link,
                    Err(error) => return self.with_error(problem(error)),
                }
            }
            None => link,
        };
        let mut factory = self;
        factory.children.push(Arc::new(HasChildren {
            factory: children,
            link,
        }));
        factory
    }

    /// Attaches records of `R` to each record once it is stored, through
    /// the pivot table of `pivot` (a [`Pivot`] or the name of a
    /// relationship the factory declares), with `values` in every pivot
    /// row. `related` is a factory (its records are created for each
    /// record) or stored records (attached to every record).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_factories::*;
    /// # async fn demo(db: Db, vip: Record<Tag>) -> Result<()> {
    /// let deals = Deal::factory()
    ///     .has_attached(Tag::factory().count(2), [("source", Value::from("import"))], "tags")
    ///     .has_attached(&vip, [], "tags")
    ///     .create(&db)
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub fn has_attached<R: Model>(
        self,
        related: impl Into<Attach<R>>,
        values: impl IntoIterator<Item = (&'static str, Value)>,
        pivot: impl Into<Pivot<F::Model, R>>,
    ) -> Self {
        let pivot = pivot.into();
        let pivot = match pivot.named {
            Some(name) => match Relationships::<F::Model>::of::<F>().pivot_link::<R>(name) {
                Ok(pivot) => pivot,
                Err(error) => return self.with_error(problem(error)),
            },
            None => pivot,
        };
        let values = values
            .into_iter()
            .map(|(column, value)| (Cow::Borrowed(column), value))
            .collect();
        let mut factory = self;
        factory.children.push(Arc::new(AttachRelated {
            related: related.into().0,
            pivot,
            values,
        }));
        factory
    }

    /// Uses `records` wherever this builder (or a factory nested in it)
    /// needs a parent of their model, instead of creating one: a random
    /// one per record for the parents definitions declare, one per batch
    /// for [`for_`](Factory::for_). Call it once per model.
    pub fn recycle<P: Model>(mut self, records: impl IntoRecords<P>) -> Self {
        self.recycle.add(records.into_records());
        self
    }

    /// The parents definitions declare with
    /// [`FactoryContext::belongs_to`] are not created: their columns keep
    /// the empty value (recycled records are still used).
    pub fn without_parents(mut self) -> Self {
        self.expand_parents = Some(false);
        self
    }

    /// Like [`without_parents`](Factory::without_parents), for the parent
    /// declared on `column` only.
    pub fn without_parent<T>(mut self, column: Column<F::Model, T>) -> Self {
        use crate::column::ColumnName;
        self.excluded_parents
            .push(Cow::Owned(column.column_name().into_owned()));
        self
    }

    /// A [`LazyFactory`]: one record, created the first time something
    /// needs it, then shared.
    pub fn lazy(self) -> LazyFactory<F> {
        LazyFactory::new(self)
    }

    // ---------------------------------------------------------------------
    // Building without the database
    // ---------------------------------------------------------------------

    /// Builds the records without storing them: definition, states, the
    /// parents known without the database (stored or recycled records),
    /// then the after-making hooks.
    pub fn make(self) -> Result<Collection<F::Model>> {
        let now = truncate_to_micros(Utc::now());
        let mut faker = self.faker(FactoryOptions::default());
        let parent_values = self.known_parent_values(&mut faker)?;
        let count = self.count.unwrap_or(1);
        let mut records = Vec::with_capacity(count);
        for index in 0..count {
            let (mut model, pending) = self.build(index, count, now, &mut faker, &parent_values)?;
            for parent in &pending {
                if parent.is_open(&model)? {
                    let pick = faker.next_u64();
                    if let Some(value) = parent.source.known(None, &self.recycle, pick)? {
                        model.set_value(&parent.column, value)?;
                    }
                }
            }
            records.push(Record::new(model));
        }
        self.run_after_making(&mut records);
        Ok(Collection::from(records))
    }

    /// One record, without storing it.
    pub fn make_one(self) -> Result<Record<F::Model>> {
        self.count(1)
            .make()?
            .into_vec()
            .pop()
            .ok_or_else(|| OrmError::factory(F::Model::meta().name(), "the factory made no record"))
    }

    /// `count` records, without storing them.
    pub fn make_many(self, count: usize) -> Result<Collection<F::Model>> {
        self.count(count).make()
    }

    /// The values the definition and the states generate for each record,
    /// by column, without an empty key or empty timestamps (request bodies,
    /// comparisons). Parents are filled as [`make`](Factory::make) fills
    /// them.
    pub fn raw(self) -> Result<Vec<Attributes>> {
        let meta = F::Model::meta();
        let timestamps: Vec<&str> = [meta.created_at(), meta.updated_at(), meta.deleted_at()]
            .into_iter()
            .flatten()
            .collect();
        self.without_after_making()
            .make()?
            .iter()
            .map(|record| {
                let model = record.model();
                let values = model
                    .values()?
                    .into_iter()
                    .filter(|(column, _)| {
                        if *column == meta.key_column() {
                            return !model.key().is_empty();
                        }
                        !(timestamps.contains(column) && model.timestamp_is_empty(column))
                    })
                    .collect::<Vec<_>>();
                Ok(Attributes::from(values))
            })
            .collect()
    }

    /// The values of one record (see [`raw`](Factory::raw)).
    pub fn raw_one(self) -> Result<Attributes> {
        self.count(1)
            .raw()?
            .pop()
            .ok_or_else(|| OrmError::factory(F::Model::meta().name(), "the factory made no record"))
    }

    // ---------------------------------------------------------------------
    // Creating
    // ---------------------------------------------------------------------

    /// Creates the records through `exec`: parents first (shared ones for
    /// `for_`, one per record for definitions), then each record is saved
    /// (model events, change events, generated keys and timestamps) with
    /// its children and attachments, then the after-creating hooks run.
    pub async fn create<E: Executor + ?Sized>(self, exec: &E) -> Result<Collection<F::Model>> {
        let target = exec.target(Access::Write).await?;
        match self.connection.clone() {
            Some(name) => {
                let db = named_connection(&target, &name).await?;
                let named = db.target(Access::Write).await?;
                self.create_on(&named).await
            }
            None => self.create_on(&target).await,
        }
    }

    /// One record, created.
    pub async fn create_one<E: Executor + ?Sized>(self, exec: &E) -> Result<Record<F::Model>> {
        self.count(1)
            .create(exec)
            .await?
            .into_vec()
            .pop()
            .ok_or_else(|| {
                OrmError::factory(F::Model::meta().name(), "the factory created no record")
            })
    }

    /// `count` records, created.
    pub async fn create_many<E: Executor + ?Sized>(
        self,
        exec: &E,
        count: usize,
    ) -> Result<Collection<F::Model>> {
        self.count(count).create(exec).await
    }

    /// [`create`](Factory::create) without model events: the app's model
    /// hooks (observers) do not run; the model's own behaviour (generated
    /// keys, timestamps, workspace) and the change events of the audit log
    /// still do.
    pub async fn create_quietly<E: Executor + ?Sized>(
        self,
        exec: &E,
    ) -> Result<Collection<F::Model>> {
        self::quiet::scope(self.create(exec)).await
    }

    /// One record, created quietly.
    pub async fn create_one_quietly<E: Executor + ?Sized>(
        self,
        exec: &E,
    ) -> Result<Record<F::Model>> {
        self::quiet::scope(self.create_one(exec)).await
    }

    /// `count` records, created quietly.
    pub async fn create_many_quietly<E: Executor + ?Sized>(
        self,
        exec: &E,
        count: usize,
    ) -> Result<Collection<F::Model>> {
        self::quiet::scope(self.create_many(exec, count)).await
    }

    /// Inserts the records in one `INSERT` statement: no model events, no
    /// change events, no relationships (stored and recycled parents still
    /// fill their columns; nothing else is created). Keys and timestamps
    /// are generated as a save would. Returns how many rows were inserted.
    pub async fn insert<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        let target = exec.target(Access::Write).await?;
        let ctx = target.context();
        let now = ctx.now().await;
        let options = factory_options(&ctx).await;
        let mut faker = self.faker(options);
        let parent_values = self.known_parent_values(&mut faker)?;
        let count = self.count.unwrap_or(1);
        let mut models = Vec::with_capacity(count);
        for index in 0..count {
            let (mut model, pending) = self.build(index, count, now, &mut faker, &parent_values)?;
            for parent in &pending {
                if parent.is_open(&model)? {
                    let pick = faker.next_u64();
                    if let Some(value) = parent.source.known(None, &self.recycle, pick)? {
                        model.set_value(&parent.column, value)?;
                    }
                }
            }
            models.push(Record::new(model));
        }
        self.run_after_making(&mut models);
        bulk::insert_models(
            &target,
            models.into_iter().map(Record::into_inner).collect(),
            now,
        )
        .await
    }

    /// [`create`](Factory::create) on a resolved target.
    pub(crate) async fn create_on(self, target: &Target<'_>) -> Result<Collection<F::Model>> {
        self.check()?;
        let ctx = target.context();
        let now = ctx.now().await;
        let options = factory_options(&ctx).await;
        let expand = self.expand_parents.unwrap_or(options.expand_relationships);
        let mut faker = self.faker(options);
        let count = self.count.unwrap_or(1);

        // The batch's parents, once.
        let mut parent_values = Vec::new();
        for link in &self.parents {
            let pick = faker.next_u64();
            let value = link
                .source
                .resolve(link.owner_key.as_deref(), target, &self.recycle, pick, true)
                .await?
                .ok_or_else(|| {
                    OrmError::factory(
                        F::Model::meta().name(),
                        format!(
                            "no {} record to point `{}` at",
                            link.source.model_name(),
                            link.foreign_key
                        ),
                    )
                })?;
            parent_values.extend(link.assignments(value));
        }

        // Make every record, with the parents its definition declares.
        let mut records = Vec::with_capacity(count);
        for index in 0..count {
            let (mut model, pending) = self.build(index, count, now, &mut faker, &parent_values)?;
            for parent in &pending {
                if !parent.is_open(&model)? {
                    continue;
                }
                let excluded = self.excluded_parents.contains(&parent.column);
                let pick = faker.next_u64();
                let value = parent
                    .source
                    .resolve(None, target, &self.recycle, pick, expand && !excluded)
                    .await?;
                if let Some(value) = value {
                    model.set_value(&parent.column, value)?;
                }
            }
            records.push(Record::new(model));
        }
        self.run_after_making(&mut records);

        // Store them with their children, then the after-creating hooks.
        for record in &mut records {
            if !persist::save(target, record).await? {
                return Err(OrmError::factory(
                    F::Model::meta().name(),
                    "a model hook cancelled saving a factory record",
                ));
            }
            for child in &self.children {
                child.create_for(record, target, &self.recycle).await?;
            }
        }
        for record in &mut records {
            for hook in &self.after_creating {
                hook(record, target).await?;
            }
        }
        Ok(Collection::from(records))
    }

    // ---------------------------------------------------------------------
    // Internals
    // ---------------------------------------------------------------------

    fn push_state(
        mut self,
        state: impl Fn(&mut F::Model, &mut FactoryContext<'_, F::Model>) -> Result<()>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.states.push(Arc::new(state));
        self
    }

    /// A state setting columns to fixed values (the foreign keys of `has`).
    pub(crate) fn state_values(self, values: Vec<(Cow<'static, str>, Value)>) -> Self {
        self.push_state(move |model, _| {
            for (column, value) in &values {
                model.set_value(column, value.clone())?;
            }
            Ok(())
        })
    }

    /// Records a problem found while building; the factory fails with it
    /// when it makes or creates records.
    fn with_error(mut self, message: String) -> Self {
        self.errors.push(message);
        self
    }

    fn check(&self) -> Result<()> {
        match self.errors.first() {
            Some(message) => Err(OrmError::factory(F::Model::meta().name(), message.clone())),
            None => Ok(()),
        }
    }

    fn faker(&self, options: FactoryOptions) -> Faker {
        match self.seed.or(options.seed) {
            Some(seed) => Faker::seeded(seed),
            None => Faker::new(),
        }
    }

    /// The values of the batch's parents known without the database, for
    /// `make` and `insert` (a parent from a factory that is not recycled
    /// sets nothing).
    fn known_parent_values(&self, faker: &mut Faker) -> Result<Vec<(Cow<'static, str>, Value)>> {
        self.check()?;
        let mut values = Vec::new();
        for link in &self.parents {
            let pick = faker.next_u64();
            if let Some(value) =
                link.source
                    .known(link.owner_key.as_deref(), &self.recycle, pick)?
            {
                values.extend(link.assignments(value));
            }
        }
        Ok(values)
    }

    /// One model: the definition, the batch's parents, then the states.
    fn build(
        &self,
        index: usize,
        count: usize,
        now: DateTime<Utc>,
        faker: &mut Faker,
        parent_values: &[(Cow<'static, str>, Value)],
    ) -> Result<(F::Model, Vec<PendingParent>)> {
        let mut pending = Vec::new();
        let mut ctx = FactoryContext::new(index, count, now, faker, &mut pending);
        let mut model = self.factory.definition(&mut ctx);
        for (column, value) in parent_values {
            model.set_value(column, value.clone())?;
        }
        for state in &self.states {
            state(&mut model, &mut ctx)?;
        }
        Ok((model, pending))
    }

    fn run_after_making(&self, records: &mut [Record<F::Model>]) {
        for record in records {
            for hook in &self.after_making {
                hook(record);
            }
        }
    }
}

/// The message of a problem found while building (a factory error's own
/// message, so it is not wrapped twice).
fn problem(error: OrmError) -> String {
    match error {
        OrmError::Factory { message, .. } => message,
        other => other.to_string(),
    }
}

/// Sets `assignment` on `model`.
fn apply_assignment<M: Model>(model: &mut M, assignment: &Assignment) -> Result<()> {
    match assignment.assigned_value() {
        Some(value) => model.set_value(assignment.column(), value.clone()),
        None => {
            let message = assignment
                .clone()
                .into_set()
                .err()
                .unwrap_or_else(|| format!("`{}` must be set to a value", assignment.column()));
            Err(OrmError::factory(M::meta().name(), message))
        }
    }
}

/// The app's [`FactoryOptions`], or the defaults.
async fn factory_options(ctx: &crate::exec::ExecContext) -> FactoryOptions {
    match ctx.app() {
        Some(app) => app
            .try_get::<FactoryOptions>()
            .await
            .map(|options| *options)
            .unwrap_or_default(),
        None => FactoryOptions::default(),
    }
}

/// The app's database connection `name` (a named `Db` provider).
async fn named_connection(target: &Target<'_>, name: &str) -> Result<Arc<Db>> {
    let ctx = target.context();
    let app = ctx.app().ok_or_else(|| {
        OrmError::invalid(format!(
            "the factory creates on connection `{name}`, but the executor belongs to no app to find it in"
        ))
    })?;
    app.get_named::<Db>(name).await.map_err(|error| {
        OrmError::invalid(format!(
            "no database connection named `{name}` in the app: {error}"
        ))
    })
}

/// What a builder attaches with [`Factory::has_attached`]: records of `R`
/// a factory creates for each record, or stored records.
pub struct Attach<R: Model>(Attachable<R>);

impl<RF: ModelFactory> From<Factory<RF>> for Attach<RF::Model> {
    fn from(factory: Factory<RF>) -> Self {
        Attach(Attachable::Factory(Arc::new(factory)))
    }
}

impl<R: Model> From<&Record<R>> for Attach<R> {
    fn from(record: &Record<R>) -> Self {
        Attach(Attachable::Records(vec![record.clone()]))
    }
}

impl<R: Model> From<Record<R>> for Attach<R> {
    fn from(record: Record<R>) -> Self {
        Attach(Attachable::Records(vec![record]))
    }
}

impl<R: Model> From<&Collection<R>> for Attach<R> {
    fn from(records: &Collection<R>) -> Self {
        Attach(Attachable::Records(records.to_vec()))
    }
}

impl<R: Model> From<Collection<R>> for Attach<R> {
    fn from(records: Collection<R>) -> Self {
        Attach(Attachable::Records(records.into_vec()))
    }
}

impl<R: Model> From<Vec<Record<R>>> for Attach<R> {
    fn from(records: Vec<Record<R>>) -> Self {
        Attach(Attachable::Records(records))
    }
}

impl<R: Model> fmt::Debug for Attach<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match &self.0 {
            Attachable::Factory(_) => "factory",
            Attachable::Records(_) => "records",
        };
        f.debug_tuple("Attach").field(&what).finish()
    }
}

impl<F: ModelFactory> Clone for Factory<F> {
    fn clone(&self) -> Self {
        Factory {
            factory: self.factory.clone(),
            count: self.count,
            states: self.states.clone(),
            parents: self.parents.clone(),
            children: self.children.clone(),
            recycle: self.recycle.clone(),
            after_making: self.after_making.clone(),
            after_creating: self.after_creating.clone(),
            expand_parents: self.expand_parents,
            excluded_parents: self.excluded_parents.clone(),
            connection: self.connection.clone(),
            seed: self.seed,
            errors: self.errors.clone(),
        }
    }
}

impl<F: ModelFactory> fmt::Debug for Factory<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Factory")
            .field("model", &F::Model::meta().name())
            .field("count", &self.count)
            .field("states", &self.states.len())
            .field("parents", &self.parents.len())
            .field("children", &self.children.len())
            .field("recycle", &self.recycle)
            .field("seed", &self.seed)
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}
