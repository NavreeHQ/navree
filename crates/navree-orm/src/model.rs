//! [`Model`]: what `#[derive(Model)]` implements, the metadata it writes
//! ([`ModelMeta`], [`ColumnMeta`]), keys ([`PrimaryKey`], [`KeyKind`]) and
//! timestamps ([`Timestamp`]).

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
use crate::factory::RelationshipInfo;
use crate::hooks::ModelHook;
use crate::query::Builder;
use crate::record::Record;
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
    route_key: Option<&'static str>,
    workspace: Option<&'static str>,
    factory: Option<&'static str>,
    relationships: Option<fn() -> Vec<RelationshipInfo>>,
    subject: Option<&'static str>,
    policy: Option<&'static str>,
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
            route_key: None,
            workspace: None,
            factory: None,
            relationships: None,
            subject: None,
            policy: None,
        }
    }

    /// The model's factory, as written in `#[model(factory = ...)]`, and
    /// the relationships it declares (`model:show` lists both).
    pub const fn factory(
        self,
        name: &'static str,
        relationships: fn() -> Vec<RelationshipInfo>,
    ) -> Self {
        ModelMeta {
            factory: Some(name),
            relationships: Some(relationships),
            ..self
        }
    }

    /// The navree-access subject the model's records are
    /// (`#[model(subject)]`).
    pub const fn subject(self, subject: &'static str) -> Self {
        ModelMeta {
            subject: Some(subject),
            ..self
        }
    }

    /// The policy attached to the model, as written in
    /// `#[model(policy = ...)]`.
    pub const fn policy(self, policy: &'static str) -> Self {
        ModelMeta {
            policy: Some(policy),
            ..self
        }
    }

    /// The model's factory, as written in `#[model(factory = ...)]`.
    pub const fn factory_name(&self) -> Option<&'static str> {
        self.factory
    }

    /// The relationships the model's factory declares
    /// ([`ModelFactory::relationships`](crate::factory::ModelFactory::relationships)),
    /// in declaration order; none without a factory.
    pub fn factory_relationships(&self) -> Vec<RelationshipInfo> {
        self.relationships.map(|f| f()).unwrap_or_default()
    }

    /// The navree-access subject of the model's records, if it is one.
    pub const fn subject_name(&self) -> Option<&'static str> {
        self.subject
    }

    /// The policy attached to the model, as written.
    pub const fn policy_name(&self) -> Option<&'static str> {
        self.policy
    }

    /// The column route model binding looks records up by, when it is not
    /// the key (`#[model(route_key = "slug")]`).
    pub const fn route_key(self, column: &'static str) -> Self {
        ModelMeta {
            route_key: Some(column),
            ..self
        }
    }

    /// The column holding a workspace-scoped model's workspace
    /// (`#[model(workspace)]`): queries see the current workspace's records
    /// only, and new records get it.
    pub const fn workspace(self, column: &'static str) -> Self {
        ModelMeta {
            workspace: Some(column),
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

    /// The column route model binding looks records up by: the declared
    /// route key, else the key column.
    pub const fn route_key_column(&self) -> &'static str {
        match self.route_key {
            Some(column) => column,
            None => self.key,
        }
    }

    /// The workspace column of a workspace-scoped model.
    pub const fn workspace_column(&self) -> Option<&'static str> {
        self.workspace
    }

    /// Whether the model is scoped to workspaces.
    pub const fn is_workspace_scoped(&self) -> bool {
        self.workspace.is_some()
    }
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

    /// The model's table.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// assert_eq!(Company::table_name(), "companies");
    /// ```
    fn table_name() -> &'static str {
        Self::meta().table()
    }

    /// The column route model binding looks records up by: the model's
    /// `#[model(route_key = "...")]`, else its key column.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// assert_eq!(Deal::route_key_name(), "id");
    /// ```
    fn route_key_name() -> &'static str {
        Self::meta().route_key_column()
    }

    /// A query of the model.
    fn query() -> Builder<Self> {
        Builder::new()
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

    /// The relationships factory `F` declares, for [`ModelMeta::factory`]
    /// (`#[model(factory = F)]`).
    ///
    /// [`ModelMeta::factory`]: crate::ModelMeta::factory
    pub fn factory_relationships<F: crate::factory::ModelFactory>()
    -> Vec<crate::factory::RelationshipInfo> {
        crate::factory::Relationships::<F::Model>::of::<F>().infos()
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
