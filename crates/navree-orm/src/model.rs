//! [`Model`]: what `#[derive(Model)]` implements, the metadata it writes
//! ([`ModelMeta`], [`ColumnMeta`]), keys ([`PrimaryKey`], [`KeyKind`]) and
//! timestamps ([`Timestamp`]).

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::collection::Collection;
use crate::column::{Assignment, Filter};
use crate::error::Result;
use crate::exec::Executor;
use crate::hooks::ModelHook;
use crate::query::Builder;
use crate::record::Record;
use crate::relations::{DynRelation, RelationMeta};
use crate::row::Row;
use crate::scope::GlobalScope;
use crate::ulid::Ulid;
use crate::value::{Field, Value};

/// How a model's key gets its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyKind {
    /// The database assigns it (auto-increment, identity, serial): a record
    /// whose key is `0` inserts without it and reads the new key back.
    Increments,
    /// A UUID version 7 (time-ordered), generated when the key is nil.
    UuidV7,
    /// A random UUID (version 4), generated when the key is nil.
    UuidV4,
    /// A [`Ulid`], generated when the key is nil.
    Ulid,
    /// Set by the application; never generated.
    Manual,
}

/// The Rust type of a model's key.
///
/// Implemented for `i16`, `i32`, `i64`, `u32`, `u64`, `Uuid`, [`Ulid`] and
/// `String`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a model key",
    note = "use an integer, `Uuid`, `Ulid` or `String` field as the primary key"
)]
pub trait PrimaryKey: Field + Clone + PartialEq + fmt::Debug {
    /// Whether the key is unset (`0`, the nil UUID or ULID, an empty
    /// string): a new record with such a key gets a generated one.
    fn is_empty(&self) -> bool;

    /// A new key of `kind`, when this type can hold one.
    fn generate(kind: KeyKind) -> Option<Self>;

    /// The unset key (for [`Record::replicate`]).
    fn empty() -> Self;

    /// The key as text (change events, error messages).
    fn to_key_string(&self) -> String;
}

macro_rules! integer_key {
    ($($ty:ty),*) => {$(
        impl PrimaryKey for $ty {
            fn is_empty(&self) -> bool {
                *self == 0
            }

            fn generate(_kind: KeyKind) -> Option<Self> {
                None
            }

            fn empty() -> Self {
                0
            }

            fn to_key_string(&self) -> String {
                self.to_string()
            }
        }
    )*};
}

integer_key!(i16, i32, i64, u32, u64);

impl PrimaryKey for Uuid {
    fn is_empty(&self) -> bool {
        self.is_nil()
    }

    fn generate(kind: KeyKind) -> Option<Self> {
        match kind {
            KeyKind::UuidV7 => Some(Uuid::now_v7()),
            KeyKind::UuidV4 => Some(Uuid::new_v4()),
            KeyKind::Ulid => Some(Ulid::new().to_uuid()),
            KeyKind::Increments | KeyKind::Manual => None,
        }
    }

    fn empty() -> Self {
        Uuid::nil()
    }

    fn to_key_string(&self) -> String {
        self.to_string()
    }
}

impl PrimaryKey for Ulid {
    fn is_empty(&self) -> bool {
        self.is_nil()
    }

    fn generate(kind: KeyKind) -> Option<Self> {
        match kind {
            KeyKind::Ulid | KeyKind::UuidV7 | KeyKind::UuidV4 => Some(Ulid::new()),
            KeyKind::Increments | KeyKind::Manual => None,
        }
    }

    fn empty() -> Self {
        Ulid::nil()
    }

    fn to_key_string(&self) -> String {
        self.to_string()
    }
}

impl PrimaryKey for String {
    fn is_empty(&self) -> bool {
        str::is_empty(self)
    }

    fn generate(kind: KeyKind) -> Option<Self> {
        match kind {
            KeyKind::UuidV7 => Some(Uuid::now_v7().to_string()),
            KeyKind::UuidV4 => Some(Uuid::new_v4().to_string()),
            KeyKind::Ulid => Some(Ulid::new().to_string()),
            KeyKind::Increments | KeyKind::Manual => None,
        }
    }

    fn empty() -> Self {
        String::new()
    }

    fn to_key_string(&self) -> String {
        self.clone()
    }
}

/// The Rust type of a timestamp column (`created_at`, `updated_at`,
/// `deleted_at`).
///
/// Implemented for `DateTime<Utc>`, `DateTime<FixedOffset>`,
/// `NaiveDateTime` (UTC), `i64` (Unix seconds) and `Option` of each. A
/// timestamp is "empty" when it is `None` or the type's zero value (the
/// Unix epoch): a new record's empty timestamps are set when it is saved.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a timestamp column",
    note = "use `DateTime<Utc>`, `DateTime<FixedOffset>`, `NaiveDateTime`, `i64` or an `Option` of one"
)]
pub trait Timestamp: Field {
    /// The timestamp for `now`.
    fn from_now(now: DateTime<Utc>) -> Self;

    /// Whether the timestamp is unset.
    fn is_empty(&self) -> bool;

    /// The unset timestamp.
    fn empty() -> Self;
}

impl Timestamp for DateTime<Utc> {
    fn from_now(now: DateTime<Utc>) -> Self {
        now
    }

    fn is_empty(&self) -> bool {
        *self == DateTime::<Utc>::UNIX_EPOCH
    }

    fn empty() -> Self {
        DateTime::<Utc>::UNIX_EPOCH
    }
}

impl Timestamp for DateTime<FixedOffset> {
    fn from_now(now: DateTime<Utc>) -> Self {
        now.fixed_offset()
    }

    fn is_empty(&self) -> bool {
        *self == DateTime::<Utc>::UNIX_EPOCH
    }

    fn empty() -> Self {
        DateTime::<Utc>::UNIX_EPOCH.fixed_offset()
    }
}

impl Timestamp for NaiveDateTime {
    fn from_now(now: DateTime<Utc>) -> Self {
        now.naive_utc()
    }

    fn is_empty(&self) -> bool {
        *self == DateTime::<Utc>::UNIX_EPOCH.naive_utc()
    }

    fn empty() -> Self {
        DateTime::<Utc>::UNIX_EPOCH.naive_utc()
    }
}

impl Timestamp for i64 {
    fn from_now(now: DateTime<Utc>) -> Self {
        now.timestamp()
    }

    fn is_empty(&self) -> bool {
        *self == 0
    }

    fn empty() -> Self {
        0
    }
}

impl<T: Timestamp> Timestamp for Option<T> {
    fn from_now(now: DateTime<Utc>) -> Self {
        Some(T::from_now(now))
    }

    fn is_empty(&self) -> bool {
        self.as_ref().is_none_or(T::is_empty)
    }

    fn empty() -> Self {
        None
    }
}

/// One column of a model, as `#[derive(Model)]` describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnMeta {
    name: &'static str,
    field: &'static str,
    rust_type: &'static str,
    nullable: bool,
    cast: Option<&'static str>,
}

impl ColumnMeta {
    /// Column `name`, stored from struct field `field` of Rust type
    /// `rust_type`.
    pub const fn new(name: &'static str, field: &'static str, rust_type: &'static str) -> Self {
        ColumnMeta {
            name,
            field,
            rust_type,
            nullable: false,
            cast: None,
        }
    }

    /// The column holds NULLs (the field is an `Option`).
    pub const fn nullable(self) -> Self {
        ColumnMeta {
            nullable: true,
            ..self
        }
    }

    /// The column is converted by cast `cast`.
    pub const fn cast(self, cast: &'static str) -> Self {
        ColumnMeta {
            cast: Some(cast),
            ..self
        }
    }

    /// The column's name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The struct field's name.
    pub const fn field(&self) -> &'static str {
        self.field
    }

    /// The field's Rust type, as written.
    pub const fn rust_type(&self) -> &'static str {
        self.rust_type
    }

    /// Whether the column holds NULLs.
    pub const fn is_nullable(&self) -> bool {
        self.nullable
    }

    /// The cast converting the column, if any.
    pub const fn cast_name(&self) -> Option<&'static str> {
        self.cast
    }
}

/// What `#[derive(Model)]` knows about a model: its name, table, key,
/// columns, timestamps, soft-delete column, connection and page size.
///
/// Built with `const` methods, so the derive writes it as a `static`.
///
/// ```
/// use navree_orm::{ColumnMeta, KeyKind, ModelMeta};
///
/// static COLUMNS: [ColumnMeta; 2] = [
///     ColumnMeta::new("id", "id", "i64"),
///     ColumnMeta::new("title", "title", "String"),
/// ];
/// static META: ModelMeta = ModelMeta::new("Deal", "deals")
///     .primary_key("id", KeyKind::Increments)
///     .columns(&COLUMNS)
///     .per_page(25);
/// assert_eq!(META.table(), "deals");
/// assert_eq!(META.column("title").unwrap().rust_type(), "String");
/// assert!(!META.uses_timestamps());
/// ```
#[derive(Debug)]
pub struct ModelMeta {
    name: &'static str,
    table: &'static str,
    key: &'static str,
    key_kind: KeyKind,
    columns: &'static [ColumnMeta],
    created_at: Option<&'static str>,
    updated_at: Option<&'static str>,
    deleted_at: Option<&'static str>,
    connection: Option<&'static str>,
    per_page: u64,
    morph_name: Option<&'static str>,
    foreign_key: Option<&'static str>,
    touches: &'static [&'static str],
    with: &'static [&'static str],
    with_count: &'static [&'static str],
    pivot_keys: Option<(&'static str, &'static str)>,
}

impl ModelMeta {
    /// Model `name` stored in `table`, keyed by an auto-incrementing `id`,
    /// 15 records per page.
    pub const fn new(name: &'static str, table: &'static str) -> Self {
        ModelMeta {
            name,
            table,
            key: "id",
            key_kind: KeyKind::Increments,
            columns: &[],
            created_at: None,
            updated_at: None,
            deleted_at: None,
            connection: None,
            per_page: 15,
            morph_name: None,
            foreign_key: None,
            touches: &[],
            with: &[],
            with_count: &[],
            pivot_keys: None,
        }
    }

    /// The name polymorphic type columns store for the model
    /// (`#[model(morph_name = "...")]`; the derive writes the snake_case
    /// type name when none is given).
    pub const fn morph_name(self, name: &'static str) -> Self {
        ModelMeta {
            morph_name: Some(name),
            ..self
        }
    }

    /// The column other models' foreign keys to this model are named by
    /// default (`company_id`).
    pub const fn foreign_key(self, column: &'static str) -> Self {
        ModelMeta {
            foreign_key: Some(column),
            ..self
        }
    }

    /// The relations whose records are touched (their update timestamp
    /// set to now) when a record of this model is saved or deleted
    /// (`#[model(touches = ["company"])]`).
    pub const fn touches(self, relations: &'static [&'static str]) -> Self {
        ModelMeta {
            touches: relations,
            ..self
        }
    }

    /// The relations every query of the model eager loads
    /// (`#[model(with = ["company"])]`).
    pub const fn with(self, relations: &'static [&'static str]) -> Self {
        ModelMeta {
            with: relations,
            ..self
        }
    }

    /// The relations every query of the model counts
    /// (`#[model(with_count = ["notes"])]`).
    pub const fn with_count(self, relations: &'static [&'static str]) -> Self {
        ModelMeta {
            with_count: relations,
            ..self
        }
    }

    /// For a pivot model without an `id` column: the pivot columns
    /// pointing at the two related records. Its rows are identified by
    /// both (`#[model(pivot(foreign_key = "deal_id", related_key =
    /// "tag_id"))]`).
    pub const fn pivot_keys(self, foreign_key: &'static str, related_key: &'static str) -> Self {
        ModelMeta {
            pivot_keys: Some((foreign_key, related_key)),
            ..self
        }
    }

    /// The key column and how it gets its value.
    pub const fn primary_key(self, column: &'static str, kind: KeyKind) -> Self {
        ModelMeta {
            key: column,
            key_kind: kind,
            ..self
        }
    }

    /// The columns, in field order.
    pub const fn columns(self, columns: &'static [ColumnMeta]) -> Self {
        ModelMeta { columns, ..self }
    }

    /// The creation and update timestamp columns (either may be `None`).
    pub const fn timestamps(
        self,
        created_at: Option<&'static str>,
        updated_at: Option<&'static str>,
    ) -> Self {
        ModelMeta {
            created_at,
            updated_at,
            ..self
        }
    }

    /// The soft-delete column: deleting sets it, queries skip rows where it
    /// is set.
    pub const fn soft_deletes(self, column: &'static str) -> Self {
        ModelMeta {
            deleted_at: Some(column),
            ..self
        }
    }

    /// The named connection the model lives on.
    pub const fn connection(self, name: &'static str) -> Self {
        ModelMeta {
            connection: Some(name),
            ..self
        }
    }

    /// The default page size.
    pub const fn per_page(self, per_page: u64) -> Self {
        ModelMeta { per_page, ..self }
    }

    /// The model's name, e.g. `Deal`.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The table.
    pub const fn table(&self) -> &'static str {
        self.table
    }

    /// The key column.
    pub const fn key_column(&self) -> &'static str {
        self.key
    }

    /// How the key gets its value.
    pub const fn key_kind(&self) -> KeyKind {
        self.key_kind
    }

    /// Every column, in field order.
    pub const fn all_columns(&self) -> &'static [ColumnMeta] {
        self.columns
    }

    /// The column `name`, if the model has it.
    pub fn column(&self, name: &str) -> Option<&'static ColumnMeta> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// Whether the model has column `name`.
    pub fn has_column(&self, name: &str) -> bool {
        self.column(name).is_some()
    }

    /// The creation timestamp column.
    pub const fn created_at(&self) -> Option<&'static str> {
        self.created_at
    }

    /// The update timestamp column.
    pub const fn updated_at(&self) -> Option<&'static str> {
        self.updated_at
    }

    /// Whether the model keeps either timestamp.
    pub const fn uses_timestamps(&self) -> bool {
        self.created_at.is_some() || self.updated_at.is_some()
    }

    /// The soft-delete column.
    pub const fn deleted_at(&self) -> Option<&'static str> {
        self.deleted_at
    }

    /// Whether the model soft-deletes.
    pub const fn soft_deletes_enabled(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// The named connection, if the model declares one.
    pub const fn connection_name(&self) -> Option<&'static str> {
        self.connection
    }

    /// The default page size.
    pub const fn default_per_page(&self) -> u64 {
        self.per_page
    }

    /// The name polymorphic type columns store for the model: the declared
    /// one, else the snake_case model name (`DealStage` → `deal_stage`).
    pub fn morph_type(&self) -> Cow<'static, str> {
        match self.morph_name {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned(snake_case(self.name)),
        }
    }

    /// The default name of foreign keys to the model: the declared one,
    /// else the snake_case model name, `_`, and the key column
    /// (`Company` keyed by `id` → `company_id`).
    pub fn foreign_key_name(&self) -> Cow<'static, str> {
        match self.foreign_key {
            Some(column) => Cow::Borrowed(column),
            None => Cow::Owned(format!("{}_{}", snake_case(self.name), self.key)),
        }
    }

    /// The relations touched when a record is saved or deleted.
    pub const fn touched_relations(&self) -> &'static [&'static str] {
        self.touches
    }

    /// The relations every query eager loads.
    pub const fn default_with(&self) -> &'static [&'static str] {
        self.with
    }

    /// The relations every query counts.
    pub const fn default_with_count(&self) -> &'static [&'static str] {
        self.with_count
    }

    /// The two pivot columns identifying a row of a pivot model without
    /// an `id` column: `(foreign_key, related_key)`.
    pub const fn pivot_key_columns(&self) -> Option<(&'static str, &'static str)> {
        self.pivot_keys
    }
}

/// `DealStage` → `deal_stage`, `HTTPRequest` → `http_request`.
pub(crate) fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let previous = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1).copied();
            let boundary = match previous {
                Some(p) if p.is_lowercase() || p.is_ascii_digit() => true,
                Some(p) if p.is_uppercase() => next.is_some_and(char::is_lowercase),
                _ => false,
            };
            if boundary && !out.ends_with('_') {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// An ORM model: a plain struct with `#[derive(Model)]`.
///
/// The derive implements the required methods; the provided ones are the
/// model API, all taking the executor first:
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, uow: Uow) -> Result<()> {
/// let deal = Deal::create(&uow, Deal { title: "Renewal".into(), ..Default::default() }).await?;
/// let found = Deal::find(&db, deal.id).await?;
/// let open = Deal::query().where_(Deal::CLOSED_AT.is_null()).latest().get(&db).await?;
/// let removed = Deal::destroy(&uow, [deal.id]).await?;
/// # Ok(()) }
/// ```
///
/// Records come back as [`Record<Self>`](Record), which dereferences to
/// the model and tracks what changed.
pub trait Model: Clone + Send + Sync + Serialize + 'static {
    /// The key's Rust type.
    type Key: PrimaryKey;

    /// What the derive knows about the model.
    fn meta() -> &'static ModelMeta;

    /// The key.
    fn key(&self) -> Self::Key;

    /// Sets the key.
    fn set_key(&mut self, key: Self::Key);

    /// Reads a model from a row.
    fn from_row(row: &Row) -> Result<Self>;

    /// Every column and its stored value, in field order.
    fn values(&self) -> Result<Vec<(&'static str, Value)>>;

    /// The stored value of `column`; `None` when the model has no such
    /// column.
    fn value(&self, column: &str) -> Result<Option<Value>>;

    /// Sets `column` from a stored value.
    fn set_value(&mut self, column: &str, value: Value) -> Result<()>;

    /// Sets timestamp column `column` to `now`.
    fn touch_timestamp(&mut self, column: &str, now: DateTime<Utc>) -> Result<()>;

    /// Whether timestamp column `column` is unset.
    fn timestamp_is_empty(&self, column: &str) -> bool;

    /// Unsets timestamp column `column`.
    fn clear_timestamp(&mut self, column: &str);

    /// The stored value of timestamp column `column` at `now`.
    fn timestamp_value(column: &str, now: DateTime<Utc>) -> Option<Value>;

    /// The global scopes applied to every query of the model
    /// (`#[model(global_scopes(..))]`).
    fn global_scopes() -> Vec<Arc<dyn GlobalScope<Self>>> {
        Vec::new()
    }

    /// The model's own lifecycle hooks, run before the app's (see
    /// [`hooks`](crate::hooks)).
    fn hooks() -> Vec<Arc<dyn ModelHook<Self>>> {
        Vec::new()
    }

    /// The relation named `name`, for relation paths (`"company.owner"` in
    /// `with`, `has`, `load`, ...): the ones a `#[relations]` impl block
    /// declares, then the ones other modules define with
    /// [`RelationRegistry`](crate::relations::RelationRegistry).
    /// `#[derive(Model)]` writes this method.
    fn relation(name: &str) -> Option<DynRelation<Self>> {
        crate::relations::registry::defined::<Self>(name)
    }

    /// The names of the model's relations (declared, then registered).
    fn relation_names() -> Vec<Cow<'static, str>> {
        crate::relations::registry::defined_names::<Self>()
    }

    /// What relation `name` is: its kind, related model, tables and keys
    /// (for tooling such as `model:show` and API documentation).
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::relations::RelationKind;
    ///
    /// let company = Deal::relation_meta("company").unwrap();
    /// assert_eq!(company.kind, RelationKind::BelongsTo);
    /// assert_eq!(company.related, Some("Company"));
    /// assert_eq!(company.foreign_key.as_deref(), Some("company_id"));
    /// assert!(Deal::relation_meta("nope").is_none());
    /// ```
    fn relation_meta(name: &str) -> Option<RelationMeta> {
        Self::relation(name).map(|relation| relation.meta())
    }

    /// The name polymorphic type columns store for the model
    /// (`#[model(morph_name = "...")]`, else the snake_case type name).
    fn morph_name() -> Cow<'static, str> {
        Self::meta().morph_type()
    }

    /// The default name of foreign keys to the model (`company_id`): the
    /// snake_case type name, `_`, the key column.
    fn foreign_key_name() -> Cow<'static, str> {
        Self::meta().foreign_key_name()
    }

    /// The relations touched when a record is saved or deleted
    /// (`#[model(touches = [..])]`).
    fn touched_relations() -> &'static [&'static str] {
        Self::meta().touched_relations()
    }

    /// A query of the model, with the model's default eager loads and
    /// relation counts (`#[model(with = [..], with_count = [..])]`).
    fn query() -> Builder<Self> {
        Builder::new().with_model_defaults()
    }

    /// A query of the model with one condition.
    fn where_(filter: impl Into<Filter>) -> Builder<Self> {
        Builder::new().where_(filter)
    }

    /// Every record.
    fn all<E: Executor + ?Sized>(
        exec: &E,
    ) -> impl Future<Output = Result<Collection<Self>>> + Send {
        Self::query().get(exec)
    }

    /// The record with key `key`.
    fn find<E: Executor + ?Sized>(
        exec: &E,
        key: impl Into<Self::Key>,
    ) -> impl Future<Output = Result<Option<Record<Self>>>> + Send {
        let key: Self::Key = key.into();
        Self::query().find(exec, key)
    }

    /// The record with key `key`, or [`OrmError::ModelNotFound`](crate::OrmError::ModelNotFound) (a 404 over
    /// HTTP).
    fn find_or_fail<E: Executor + ?Sized>(
        exec: &E,
        key: impl Into<Self::Key>,
    ) -> impl Future<Output = Result<Record<Self>>> + Send {
        let key: Self::Key = key.into();
        Self::query().find_or_fail(exec, key)
    }

    /// The records with these keys (missing ones are left out).
    fn find_many<E: Executor + ?Sized>(
        exec: &E,
        keys: impl IntoIterator<Item = impl Into<Self::Key>>,
    ) -> impl Future<Output = Result<Collection<Self>>> + Send {
        let keys: Vec<Self::Key> = keys.into_iter().map(Into::into).collect();
        Self::query().find_many(exec, keys)
    }

    /// Inserts `model` and returns its record (with any generated key and
    /// timestamps).
    fn create<E: Executor + ?Sized>(
        exec: &E,
        model: Self,
    ) -> impl Future<Output = Result<Record<Self>>> + Send {
        async move {
            let mut record = Record::new(model);
            record.save(exec).await?;
            Ok(record)
        }
    }

    /// Deletes the records with these keys one by one (so each one's
    /// lifecycle runs); returns how many were deleted.
    fn destroy<E: Executor + ?Sized>(
        exec: &E,
        keys: impl IntoIterator<Item = impl Into<Self::Key>>,
    ) -> impl Future<Output = Result<u64>> + Send {
        let keys: Vec<Self::Key> = keys.into_iter().map(Into::into).collect();
        async move {
            if keys.is_empty() {
                return Ok(0);
            }
            let records = Self::query().where_key_in(keys).get(exec).await?;
            let mut deleted = 0;
            for mut record in records {
                if record.delete(exec).await? {
                    deleted += 1;
                }
            }
            Ok(deleted)
        }
    }

    /// The first record matching `attributes`, or a new one with them
    /// (and whatever `fill` sets) inserted. A concurrent insert of the same
    /// record (a unique violation) returns the record that won.
    fn first_or_create<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut Self) + Send,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        Self::query().first_or_create(exec, attributes, fill)
    }

    /// The first record matching `attributes`, or a new, unsaved one with
    /// them (and whatever `fill` sets).
    fn first_or_new<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut Self) + Send,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        Self::query().first_or_new(exec, attributes, fill)
    }

    /// Inserts a record with `attributes` (and whatever `fill` sets), or,
    /// when that violates a unique constraint, returns the record matching
    /// `attributes`.
    fn create_or_first<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut Self) + Send,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        Self::query().create_or_first(exec, attributes, fill)
    }

    /// Updates the first record matching `attributes` with `fill`, or
    /// creates one with both.
    fn update_or_create<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl Fn(&mut Self) + Send + Sync,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        Self::query().update_or_create(exec, attributes, fill)
    }
}

/// Functions `#[derive(Model)]` expands to. Not part of the API.
#[doc(hidden)]
pub mod derive_support {
    use std::borrow::Cow;

    use crate::cast::Cast;
    use crate::error::OrmError;
    use crate::model::Model;
    use crate::row::{Row, decode_error};
    use crate::value::{Field, FieldError, Value};

    fn cast_error<M: Model>(column: &str, message: impl Into<String>) -> OrmError {
        OrmError::Cast {
            model: Cow::Borrowed(M::meta().name()),
            column: column.to_owned(),
            message: message.into(),
        }
    }

    /// Reads field column `column` of model `M`.
    pub fn read<M: Model, T: Field>(row: &Row, column: &str) -> Result<T, OrmError> {
        row.field::<T>(column)
            .map_err(|error| field_error::<M>(row, column, T::type_name(), &error))
    }

    /// Reads field column `column`, or the field's default when the row
    /// does not have it (`#[model(default)]`).
    pub fn read_or_default<M: Model, T: Field + Default>(
        row: &Row,
        column: &str,
    ) -> Result<T, OrmError> {
        match row.field::<T>(column) {
            Ok(value) => Ok(value),
            Err(FieldError::Missing) => Ok(T::default()),
            Err(error) => Err(field_error::<M>(row, column, T::type_name(), &error)),
        }
    }

    fn field_error<M: Model>(
        row: &Row,
        column: &str,
        expected: Cow<'static, str>,
        error: &FieldError,
    ) -> OrmError {
        match error {
            FieldError::Missing => OrmError::Decode {
                model: Cow::Borrowed(M::meta().name()),
                column: column.to_owned(),
                expected,
                message: format!(
                    "the row has no such column (it has: {}); select it, or mark the field #[model(default)]",
                    row.columns().join(", ")
                ),
            },
            other => decode_error(M::meta().name(), column, expected, other),
        }
    }

    /// Reads cast column `column` through cast `C`.
    pub fn read_cast<M: Model, C: Cast<T>, T>(row: &Row, column: &str) -> Result<T, OrmError> {
        let stored = read::<M, C::Stored>(row, column)?;
        C::get(stored).map_err(|error| cast_error::<M>(column, error.to_string()))
    }

    /// Reads nullable cast column `column` through cast `C`.
    pub fn read_cast_option<M: Model, C: Cast<T>, T>(
        row: &Row,
        column: &str,
    ) -> Result<Option<T>, OrmError> {
        match read::<M, Option<C::Stored>>(row, column)? {
            Some(stored) => C::get(stored)
                .map(Some)
                .map_err(|error| cast_error::<M>(column, error.to_string())),
            None => Ok(None),
        }
    }

    /// Reads cast column `column`, or the default when the row lacks it.
    pub fn read_cast_or_default<M: Model, C: Cast<T>, T: Default>(
        row: &Row,
        column: &str,
    ) -> Result<T, OrmError> {
        if row.has_column(column) {
            read_cast::<M, C, T>(row, column)
        } else {
            Ok(T::default())
        }
    }

    /// Reads nullable cast column `column`, or `None` when the row lacks it.
    pub fn read_cast_option_or_default<M: Model, C: Cast<T>, T>(
        row: &Row,
        column: &str,
    ) -> Result<Option<T>, OrmError> {
        if row.has_column(column) {
            read_cast_option::<M, C, T>(row, column)
        } else {
            Ok(None)
        }
    }

    /// The stored value of a cast field.
    pub fn write_cast<M: Model, C: Cast<T>, T>(column: &str, value: &T) -> Result<Value, OrmError> {
        C::set(value)
            .map(|stored| stored.to_value())
            .map_err(|error| cast_error::<M>(column, error.to_string()))
    }

    /// The stored value of a nullable cast field.
    pub fn write_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &Option<T>,
    ) -> Result<Value, OrmError> {
        match value {
            Some(value) => write_cast::<M, C, T>(column, value),
            None => Ok(<C::Stored as Field>::null()),
        }
    }

    /// Converts a stored value into a field.
    pub fn convert<M: Model, T: Field>(column: &str, value: Value) -> Result<T, OrmError> {
        T::from_value(value)
            .map_err(|error| decode_error(M::meta().name(), column, T::type_name(), &error))
    }

    /// Converts a stored value into a cast field.
    pub fn convert_cast<M: Model, C: Cast<T>, T>(
        column: &str,
        value: Value,
    ) -> Result<T, OrmError> {
        let stored = convert::<M, C::Stored>(column, value)?;
        C::get(stored).map_err(|error| cast_error::<M>(column, error.to_string()))
    }

    /// Converts a stored value into a nullable cast field.
    pub fn convert_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        value: Value,
    ) -> Result<Option<T>, OrmError> {
        match convert::<M, Option<C::Stored>>(column, value)? {
            Some(stored) => C::get(stored)
                .map(Some)
                .map_err(|error| cast_error::<M>(column, error.to_string())),
            None => Ok(None),
        }
    }

    /// The error for a column the model does not have.
    pub fn unknown_column<M: Model>(column: &str) -> OrmError {
        OrmError::InvalidQuery(format!("{} has no column `{column}`", M::meta().name()))
    }

    /// The error for a column that is not one of the model's timestamps.
    pub fn not_a_timestamp<M: Model>(column: &str) -> OrmError {
        OrmError::InvalidQuery(format!(
            "`{column}` is not a timestamp column of {}",
            M::meta().name()
        ))
    }
}
