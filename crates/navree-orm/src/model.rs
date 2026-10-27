//! [`Model`]: what `#[derive(Model)]` implements, the metadata it writes
//! ([`ModelMeta`], [`ColumnMeta`]), keys ([`PrimaryKey`], [`KeyKind`]) and
//! timestamps ([`Timestamp`]).

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use navree_kernel::BoxFuture;
use serde::Serialize;
use uuid::Uuid;

use crate::attributes::{Accessor, CastContext};
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
use crate::value::{Field, FieldKind, Value};

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

/// The OpenAPI schema of a column or appended value
/// (`#[model(schema = Type)]` gives `Type`'s `PartialSchema::schema`).
pub type PartialSchemaFn = fn() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>;

/// One column of a model, as `#[derive(Model)]` describes it.
#[derive(Clone, Copy, Debug)]
pub struct ColumnMeta {
    name: &'static str,
    field: &'static str,
    rust_type: &'static str,
    nullable: bool,
    cast: Option<&'static str>,
    serialized: Option<&'static str>,
    kind: FieldKind,
    serde_custom: bool,
    serde_optional: bool,
    mutator: bool,
    schema: Option<PartialSchemaFn>,
}

impl ColumnMeta {
    /// Column `name`, stored from struct field `field` of Rust type
    /// `rust_type`, serialized under the field's name.
    pub const fn new(name: &'static str, field: &'static str, rust_type: &'static str) -> Self {
        ColumnMeta {
            name,
            field,
            rust_type,
            nullable: false,
            cast: None,
            serialized: Some(field),
            kind: FieldKind::Any,
            serde_custom: false,
            serde_optional: false,
            mutator: false,
            schema: None,
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

    /// The key the field has in serialized records (serde's name for it);
    /// `None` when serde never writes it.
    pub const fn serialized_as(self, key: Option<&'static str>) -> Self {
        ColumnMeta {
            serialized: key,
            ..self
        }
    }

    /// What the field's values are in serialized records (from the field
    /// type's [`Field::KIND`] or its cast's
    /// [`Cast::KIND`](crate::cast::Cast::KIND)).
    pub const fn kind(self, kind: FieldKind) -> Self {
        ColumnMeta { kind, ..self }
    }

    /// Serde writes the field its own way (`serialize_with`, `with`,
    /// `flatten`): serialized records keep what serde writes.
    pub const fn serde_custom(self) -> Self {
        ColumnMeta {
            serde_custom: true,
            ..self
        }
    }

    /// Serde may leave the field out (`skip_serializing_if`).
    pub const fn serde_optional(self) -> Self {
        ColumnMeta {
            serde_optional: true,
            ..self
        }
    }

    /// The field has a mutator (`#[model(set_with = ..)]`).
    pub const fn mutator(self) -> Self {
        ColumnMeta {
            mutator: true,
            ..self
        }
    }

    /// The column's OpenAPI schema (`#[model(schema = Type)]`).
    pub const fn schema(self, schema: PartialSchemaFn) -> Self {
        ColumnMeta {
            schema: Some(schema),
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

    /// The key the field has in serialized records; `None` when serde
    /// never writes it (`#[serde(skip)]`).
    pub const fn serialized_name(&self) -> Option<&'static str> {
        self.serialized
    }

    /// What the field's values are in serialized records.
    pub const fn field_kind(&self) -> FieldKind {
        self.kind
    }

    /// Whether the field is a date-time that serialized records write in
    /// the model's date format (no serde customization).
    pub const fn is_date_time(&self) -> bool {
        matches!(self.kind, FieldKind::DateTime) && !self.serde_custom
    }

    /// Whether the column holds JSON (a `serde_json::Value` field or a JSON
    /// cast): fill keys may reach into it (`"meta->status"`).
    pub const fn is_json(&self) -> bool {
        matches!(self.kind, FieldKind::Json)
    }

    /// Whether serde writes the field its own way.
    pub const fn has_custom_serde(&self) -> bool {
        self.serde_custom
    }

    /// Whether serde may leave the field out of serialized records.
    pub const fn is_serde_optional(&self) -> bool {
        self.serde_optional
    }

    /// Whether the field has a mutator (`#[model(set_with = ..)]`).
    pub const fn has_mutator(&self) -> bool {
        self.mutator
    }

    /// The column's OpenAPI schema override, if any.
    pub const fn schema_override(&self) -> Option<PartialSchemaFn> {
        self.schema
    }
}

impl PartialEq for ColumnMeta {
    fn eq(&self, other: &Self) -> bool {
        let same_schema = match (self.schema, other.schema) {
            (Some(a), Some(b)) => std::ptr::fn_addr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.name == other.name
            && self.field == other.field
            && self.rust_type == other.rust_type
            && self.nullable == other.nullable
            && self.cast == other.cast
            && self.serialized == other.serialized
            && self.kind == other.kind
            && self.serde_custom == other.serde_custom
            && self.serde_optional == other.serde_optional
            && self.mutator == other.mutator
            && same_schema
    }
}

impl Eq for ColumnMeta {}

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
    pruning: Option<crate::prune::Pruning>,
    morph_name: Option<&'static str>,
    foreign_key: Option<&'static str>,
    touches: &'static [&'static str],
    with: &'static [&'static str],
    with_count: &'static [&'static str],
    pivot_keys: Option<(&'static str, &'static str)>,
    fillable: &'static [&'static str],
    guarded: Option<&'static [&'static str]>,
    unguarded: bool,
    hidden: &'static [&'static str],
    visible: &'static [&'static str],
    appends: &'static [&'static str],
    encrypter: Option<&'static str>,
    cast_context: bool,
    date_serializer: Option<DateSerializer>,
}

/// How a model writes its date-times in serialized records
/// (`#[model(serialize_dates_with = path)]`): the instant, with the offset
/// the field had (UTC for `DateTime<Utc>` and `NaiveDateTime` fields), to
/// any JSON value.
pub type DateSerializer = fn(&DateTime<FixedOffset>) -> serde_json::Value;

impl ModelMeta {
    /// Model `name` stored in `table`, keyed by an auto-incrementing `id`,
    /// 15 records per page, totally guarded (no column is fillable), with
    /// nothing hidden and nothing appended.
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
            pruning: None,
            morph_name: None,
            foreign_key: None,
            touches: &[],
            with: &[],
            with_count: &[],
            pivot_keys: None,
            fillable: &[],
            guarded: None,
            unguarded: false,
            hidden: &[],
            visible: &[],
            appends: &[],
            encrypter: None,
            cast_context: false,
            date_serializer: None,
        }
    }

    /// The columns `fill` may set (`#[model(fillable(..))]`); JSON paths
    /// into a JSON column are written `"meta->status"`.
    pub const fn fillable(self, columns: &'static [&'static str]) -> Self {
        ModelMeta {
            fillable: columns,
            ..self
        }
    }

    /// The columns `fill` may not set (`#[model(guarded(..))]`); every other
    /// column becomes fillable. An empty list guards nothing.
    pub const fn guarded(self, columns: &'static [&'static str]) -> Self {
        ModelMeta {
            guarded: Some(columns),
            ..self
        }
    }

    /// Mass assignment is never guarded for this model
    /// (`#[model(unguarded)]`).
    pub const fn unguarded(self) -> Self {
        ModelMeta {
            unguarded: true,
            ..self
        }
    }

    /// The names left out of serialized records (`#[model(hidden(..))]` and
    /// fields marked `#[model(hidden)]`).
    pub const fn hidden(self, names: &'static [&'static str]) -> Self {
        ModelMeta {
            hidden: names,
            ..self
        }
    }

    /// The only names serialized records keep (`#[model(visible(..))]`);
    /// empty keeps every name.
    pub const fn visible(self, names: &'static [&'static str]) -> Self {
        ModelMeta {
            visible: names,
            ..self
        }
    }

    /// The accessors appended to serialized records
    /// (`#[model(appends(..))]`).
    pub const fn appends(self, names: &'static [&'static str]) -> Self {
        ModelMeta {
            appends: names,
            ..self
        }
    }

    /// The named `Encrypter` the model's encrypted casts use
    /// (`#[model(encrypter = "...")]`).
    pub const fn encrypter(self, name: &'static str) -> Self {
        ModelMeta {
            encrypter: Some(name),
            ..self
        }
    }

    /// Whether a cast of the model needs a
    /// [`CastContext`](crate::attributes::CastContext) (resolved from the
    /// app before records are read or written).
    pub const fn cast_context(self, needed: bool) -> Self {
        ModelMeta {
            cast_context: needed,
            ..self
        }
    }

    /// How serialized records write the model's date-times
    /// (`#[model(serialize_dates_with = path)]`).
    pub const fn serialize_dates_with(self, serializer: DateSerializer) -> Self {
        ModelMeta {
            date_serializer: Some(serializer),
            ..self
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

    /// How `model:prune` prunes the model (`#[model(prunable)]`,
    /// `#[model(mass_prunable)]`).
    pub const fn pruned(self, pruning: crate::prune::Pruning) -> Self {
        ModelMeta {
            pruning: Some(pruning),
            ..self
        }
    }

    /// How the model is pruned, when it is prunable.
    pub const fn pruning(&self) -> Option<&crate::prune::Pruning> {
        self.pruning.as_ref()
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

    /// The two pivot columns of a pivot model (`#[model(pivot(..))]`):
    /// `(foreign_key, related_key)`. A pivot model without an `id` column
    /// is keyed by `related_key`, and its rows are identified by both.
    pub const fn pivot_key_columns(&self) -> Option<(&'static str, &'static str)> {
        self.pivot_keys
    }

    /// The fillable columns, as declared.
    pub const fn fillable_columns(&self) -> &'static [&'static str] {
        self.fillable
    }

    /// The guarded columns, as declared: `None` when the model declares no
    /// guarded list (then every column is guarded unless it is fillable).
    pub const fn guarded_columns(&self) -> Option<&'static [&'static str]> {
        self.guarded
    }

    /// Whether the model is never guarded (`#[model(unguarded)]`).
    pub const fn is_unguarded(&self) -> bool {
        self.unguarded
    }

    /// The names hidden in serialized records.
    pub const fn hidden_names(&self) -> &'static [&'static str] {
        self.hidden
    }

    /// The only names serialized records keep; empty keeps every name.
    pub const fn visible_names(&self) -> &'static [&'static str] {
        self.visible
    }

    /// The accessors appended to serialized records.
    pub const fn appended_names(&self) -> &'static [&'static str] {
        self.appends
    }

    /// The named `Encrypter` the model's encrypted casts use, if it names
    /// one.
    pub const fn encrypter_name(&self) -> Option<&'static str> {
        self.encrypter
    }

    /// Whether a cast of the model needs a cast context.
    pub const fn uses_cast_context(&self) -> bool {
        self.cast_context
    }

    /// How serialized records write the model's date-times, when the model
    /// overrides the default (RFC 3339, UTC, microseconds).
    pub const fn date_serializer(&self) -> Option<DateSerializer> {
        self.date_serializer
    }

    /// The column whose field is `field` (its first column, for a field
    /// stored across several columns).
    pub fn column_of_field(&self, field: &str) -> Option<&'static ColumnMeta> {
        self.columns.iter().find(|column| column.field == field)
    }

    /// The column called `name`, or the column of the field called `name`.
    pub fn column_named(&self, name: &str) -> Option<&'static ColumnMeta> {
        self.column(name).or_else(|| self.column_of_field(name))
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

    // ---------------------------------------------------------------------
    // Attributes (the derive writes these from the field types, casts and
    // `#[model(..)]` attributes)
    // ---------------------------------------------------------------------

    /// Sets `column` from untyped input, the way mass assignment does: the
    /// value is converted to the field's type (through its cast's
    /// [`from_json`](crate::cast::Cast::from_json)), then assigned through
    /// the field's mutator (`#[model(set_with = ..)]`) when it has one.
    /// Unknown columns are errors.
    ///
    /// Unlike [`set_value`](Model::set_value), which takes the *stored*
    /// form (what the database holds: a ciphertext, an enum's name), this
    /// takes the *plain* form a user sends (the secret, the enum's name).
    fn set_attribute(&mut self, column: &str, input: serde_json::Value) -> Result<()> {
        self.set_value(column, crate::value::json_to_value(input))
    }

    /// Sets several columns of field `field` (a field stored across several
    /// columns) from untyped input at once, so the field is checked whole
    /// rather than one column at a time.
    #[doc(hidden)]
    fn set_attribute_parts(
        &mut self,
        field: &str,
        inputs: Vec<(String, serde_json::Value)>,
    ) -> Result<()> {
        let _ = field;
        for (column, input) in inputs {
            self.set_attribute(&column, input)?;
        }
        Ok(())
    }

    /// The columns whose values differ between `self` and `original`
    /// (dirty tracking), compared as stored: floats within a small
    /// tolerance, decimals numerically, date-times by instant, JSON
    /// structurally, cast fields by their cast's
    /// [`same`](crate::cast::Cast::same).
    fn changed_columns(&self, original: &Self) -> Result<Vec<&'static str>> {
        let now = self.values()?;
        let then = original.values()?;
        Ok(now
            .into_iter()
            .zip(then)
            .filter(|((_, a), (_, b))| !crate::value::same_value(a, b))
            .map(|((column, _), _)| column)
            .collect())
    }

    /// The JSON value of JSON column `column` (a `serde_json::Value` field
    /// or a JSON cast), for edits by path; `None` when the model has no
    /// such JSON column.
    fn json_attribute(&self, column: &str) -> Result<Option<serde_json::Value>> {
        let _ = column;
        Ok(None)
    }

    /// The accessors of the model (`#[accessor]` methods in an
    /// `#[accessors]` impl block), which serialized records can append.
    fn accessors() -> &'static [Accessor<Self>] {
        &[]
    }

    /// The columns `fill` may set, as declared (`#[model(fillable(..))]`).
    fn fillable() -> &'static [&'static str] {
        Self::meta().fillable_columns()
    }

    /// The columns `fill` may not set, as declared (`#[model(guarded(..))]`);
    /// `None` when the model declares none (every column not fillable is
    /// guarded).
    fn guarded() -> Option<&'static [&'static str]> {
        Self::meta().guarded_columns()
    }

    /// Whether `fill` may set `column` (or the JSON path `column->path`).
    fn is_fillable(column: &str) -> bool {
        crate::attributes::fill::is_fillable(Self::meta(), column, false)
    }

    /// Whether `column` is guarded against mass assignment.
    fn is_guarded(column: &str) -> bool {
        crate::attributes::fill::is_guarded(Self::meta(), column, false)
    }

    /// Whether the model declares neither fillable nor guarded columns, so
    /// every `fill` is refused (`force_fill` still works).
    fn totally_guarded() -> bool {
        crate::attributes::fill::totally_guarded(Self::meta(), false)
    }

    /// Runs the casts' asynchronous preparation before the record is
    /// written (the hashed cast hashes plain values here).
    #[doc(hidden)]
    fn prepare_for_write<'a>(&'a mut self, context: &'a CastContext) -> BoxFuture<'a, Result<()>> {
        let _ = context;
        Box::pin(std::future::ready(Ok(())))
    }

    /// The values casts write into serialized records themselves
    /// ([`Cast::serialize`](crate::cast::Cast::serialize)), by serialized
    /// key.
    #[doc(hidden)]
    fn serialized_overrides(&self) -> Result<Vec<(&'static str, serde_json::Value)>> {
        Ok(Vec::new())
    }

    /// The cast columns of `row` whose stored values should be rewritten
    /// ([`Cast::is_stale`](crate::cast::Cast::is_stale)).
    #[doc(hidden)]
    fn stale_columns(row: &Row) -> Vec<&'static str> {
        let _ = row;
        Vec::new()
    }

    /// Adds (or subtracts) `amount`, given in the stored form, to cast
    /// column `column` in memory, through the cast's
    /// [`increment`](crate::cast::Cast::increment) and
    /// [`decrement`](crate::cast::Cast::decrement).
    #[doc(hidden)]
    fn increment_cast(&mut self, column: &str, amount: Value, decrement: bool) -> Result<()> {
        let _ = (amount, decrement);
        Err(derive_support::not_a_cast_column::<Self>(column))
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

    /// A query of the model with one condition (and the model's default
    /// eager loads and relation counts, as [`query`](Model::query)).
    fn where_(filter: impl Into<Filter>) -> Builder<Self> {
        Self::query().where_(filter)
    }

    /// The connection the model lives on (`#[model(connection = "..")]`);
    /// `None` runs it on the executor's. Given the default `Db`, the
    /// model's queries and records go to this connection.
    fn connection_name() -> Option<&'static str> {
        Self::meta().connection_name()
    }

    /// A query of the model on connection `name`, whatever the model's
    /// (see [`Builder::on_connection`]).
    fn on_connection(name: impl Into<std::borrow::Cow<'static, str>>) -> Builder<Self> {
        Builder::new().on_connection(name)
    }

    /// A query of the model reading from the writer, even where read
    /// replicas exist (see [`Builder::use_write`]).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, id: Uuid) -> Result<()> {
    /// let current = Deal::on_write_connection().find(&db, id).await?;
    /// # Ok(()) }
    /// ```
    fn on_write_connection() -> Builder<Self> {
        Builder::new().use_write()
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

    /// Inserts a record built from the model's defaults and filled with
    /// untyped `attributes` (guarded: only fillable columns are set; see
    /// [`Record::fill`]).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, body: navree_orm::attributes::Attributes) -> Result<()> {
    /// let tag = Tag::create_from(&uow, body).await?;
    /// # Ok(()) }
    /// ```
    fn create_from<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl Into<crate::attributes::Attributes>,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: Default,
    {
        let attributes = attributes.into();
        Self::query().create_from(exec, attributes)
    }

    /// Inserts a record built from the model's defaults with every key of
    /// `attributes` set, ignoring the model's guards (see
    /// [`Record::force_fill`]).
    fn force_create<E: Executor + ?Sized>(
        exec: &E,
        attributes: impl Into<crate::attributes::Attributes>,
    ) -> impl Future<Output = Result<Record<Self>>> + Send
    where
        Self: Default,
    {
        let attributes = attributes.into();
        Self::query().force_create(exec, attributes)
    }

    /// Inserts `models` as they are, in one statement without hooks or
    /// change events (see [`Builder::insert`]); returns how many rows were
    /// inserted.
    fn insert<E: Executor + ?Sized>(
        exec: &E,
        models: impl IntoIterator<Item = Self>,
    ) -> impl Future<Output = Result<u64>> + Send {
        let models: Vec<Self> = models.into_iter().collect();
        Self::query().insert(exec, models)
    }

    /// Inserts `models` as they are, skipping the ones that conflict with a
    /// unique index (see [`Builder::insert_or_ignore`]).
    fn insert_or_ignore<E: Executor + ?Sized>(
        exec: &E,
        models: impl IntoIterator<Item = Self>,
    ) -> impl Future<Output = Result<u64>> + Send {
        let models: Vec<Self> = models.into_iter().collect();
        Self::query().insert_or_ignore(exec, models)
    }

    /// Inserts `models` with generated keys and timestamps, in one
    /// statement without hooks (see [`Builder::fill_and_insert`]).
    fn fill_and_insert<E: Executor + ?Sized>(
        exec: &E,
        models: impl IntoIterator<Item = Self>,
    ) -> impl Future<Output = Result<u64>> + Send {
        let models: Vec<Self> = models.into_iter().collect();
        Self::query().fill_and_insert(exec, models)
    }

    /// Inserts `models`, updating the stored rows they conflict with on
    /// `unique_by` (see [`Builder::upsert`]).
    fn upsert<E, U>(
        exec: &E,
        models: impl IntoIterator<Item = Self>,
        unique_by: impl IntoIterator<Item = impl crate::column::ColumnName>,
        update: impl IntoIterator<Item = U>,
    ) -> impl Future<Output = Result<u64>> + Send
    where
        E: Executor + ?Sized,
        U: Into<crate::query::ConflictUpdate>,
    {
        let models: Vec<Self> = models.into_iter().collect();
        let unique_by: Vec<String> = unique_by
            .into_iter()
            .map(|column| column.column_name().into_owned())
            .collect();
        let update: Vec<crate::query::ConflictUpdate> =
            update.into_iter().map(Into::into).collect();
        Self::query().upsert(exec, models, unique_by, update)
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

    use serde::Serialize;

    use crate::attributes::CastContext;
    use crate::cast::{Cast, CastError, MultiColumnCast, StoredColumns};
    use crate::error::OrmError;
    use crate::model::Model;
    use crate::row::{Row, decode_error};
    use crate::value::{Field, FieldError, Value, same_value};

    fn cast_error<M: Model>(column: &str, message: impl Into<String>) -> OrmError {
        OrmError::Cast {
            model: Cow::Borrowed(M::meta().name()),
            column: column.to_owned(),
            message: message.into(),
        }
    }

    /// Reads field column `column` of model `M`; `default` is the field's
    /// default when its type has one (for a column a partial select left
    /// out, in lenient mode).
    pub fn read<M: Model, T: Field>(
        row: &Row,
        column: &str,
        default: Option<fn() -> T>,
    ) -> Result<T, OrmError> {
        match row.field::<T>(column) {
            Ok(value) => Ok(value),
            Err(FieldError::Missing) => missing::<M, T>(row, column, T::type_name(), default),
            Err(error) => Err(decode_error(
                M::meta().name(),
                column,
                T::type_name(),
                &error,
            )),
        }
    }

    /// A field whose column the row lacks: an error in strict mode (unless
    /// a handler tolerates it) and when the field has no default; else the
    /// default.
    fn missing<M: Model, T>(
        row: &Row,
        column: &str,
        expected: Cow<'static, str>,
        default: Option<fn() -> T>,
    ) -> Result<T, OrmError> {
        let error = |expected| OrmError::MissingAttribute {
            model: Cow::Borrowed(M::meta().name()),
            column: column.to_owned(),
            expected,
            selected: row.columns(),
        };
        let Some(default) = default else {
            return Err(error(expected));
        };
        crate::strict::current().check_with(
            crate::strict::Violation::MissingAttribute {
                model: M::meta().name(),
                column: column.to_owned(),
            },
            |_| error(expected),
        )?;
        Ok(default())
    }

    /// The default of a field's type, when it has one: `(&DefaultOf::<T>::new()).default_fn()`
    /// is `Some(T::default)` for a `T: Default` and `None` otherwise (the
    /// derive names concrete types, so the method resolves to one of
    /// [`ViaDefault`] and [`WithoutDefault`]).
    pub struct DefaultOf<T>(std::marker::PhantomData<fn() -> T>);

    impl<T> DefaultOf<T> {
        /// The probe for `T`.
        pub const fn new() -> Self {
            DefaultOf(std::marker::PhantomData)
        }
    }

    impl<T> Default for DefaultOf<T> {
        fn default() -> Self {
            DefaultOf::new()
        }
    }

    /// The default of a type that has one.
    pub trait ViaDefault {
        /// The field type.
        type Field;
        /// `Some(Field::default)`.
        fn default_fn(&self) -> Option<fn() -> Self::Field>;
    }

    impl<T: Default> ViaDefault for DefaultOf<T> {
        type Field = T;
        fn default_fn(&self) -> Option<fn() -> T> {
            Some(T::default)
        }
    }

    /// No default for a type that has none.
    pub trait WithoutDefault {
        /// The field type.
        type Field;
        /// `None`.
        fn default_fn(&self) -> Option<fn() -> Self::Field>;
    }

    impl<T> WithoutDefault for &DefaultOf<T> {
        type Field = T;
        fn default_fn(&self) -> Option<fn() -> T> {
            None
        }
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
            Err(error) => Err(decode_error(
                M::meta().name(),
                column,
                T::type_name(),
                &error,
            )),
        }
    }

    /// Reads cast column `column` through cast `C`; `default` as for
    /// [`read`].
    pub fn read_cast<M: Model, C: Cast<T>, T>(
        row: &Row,
        column: &str,
        default: Option<fn() -> T>,
    ) -> Result<T, OrmError> {
        match row.field::<C::Stored>(column) {
            Ok(stored) => get::<M, C, T>(column, stored),
            Err(FieldError::Missing) => {
                missing::<M, T>(row, column, C::Stored::type_name(), default)
            }
            Err(error) => Err(decode_error(
                M::meta().name(),
                column,
                C::Stored::type_name(),
                &error,
            )),
        }
    }

    /// `C::get_with` with the current cast context.
    fn get<M: Model, C: Cast<T>, T>(column: &str, stored: C::Stored) -> Result<T, OrmError> {
        CastContext::with_current(|context| C::get_with(stored, context))
            .map_err(|error| cast_error::<M>(column, error.to_string()))
    }

    /// `C::set_with` with the current cast context.
    fn set<M: Model, C: Cast<T>, T>(column: &str, value: &T) -> Result<C::Stored, OrmError> {
        CastContext::with_current(|context| C::set_with(value, context))
            .map_err(|error| cast_error::<M>(column, error.to_string()))
    }

    /// Reads nullable cast column `column` through cast `C`; `default` as
    /// for [`read`].
    pub fn read_cast_option<M: Model, C: Cast<T>, T>(
        row: &Row,
        column: &str,
        default: Option<fn() -> Option<T>>,
    ) -> Result<Option<T>, OrmError> {
        match row.field::<Option<C::Stored>>(column) {
            Ok(Some(stored)) => get::<M, C, T>(column, stored).map(Some),
            Ok(None) => Ok(None),
            Err(FieldError::Missing) => {
                missing::<M, Option<T>>(row, column, <Option<C::Stored>>::type_name(), default)
            }
            Err(error) => Err(decode_error(
                M::meta().name(),
                column,
                <Option<C::Stored>>::type_name(),
                &error,
            )),
        }
    }

    /// Reads cast column `column`, or the default when the row lacks it.
    pub fn read_cast_or_default<M: Model, C: Cast<T>, T: Default>(
        row: &Row,
        column: &str,
    ) -> Result<T, OrmError> {
        if row.has_column(column) {
            read_cast::<M, C, T>(row, column, Some(T::default))
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
            read_cast_option::<M, C, T>(row, column, Some(|| None))
        } else {
            Ok(None)
        }
    }

    /// The stored value of a cast field.
    pub fn write_cast<M: Model, C: Cast<T>, T>(column: &str, value: &T) -> Result<Value, OrmError> {
        set::<M, C, T>(column, value).map(|stored| stored.to_value())
    }

    /// The stored value of a nullable cast field.
    pub fn write_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &Option<T>,
    ) -> Result<Value, OrmError> {
        match value {
            Some(value) => write_cast::<M, C, T>(column, value),
            None => Ok(CastContext::with_current(C::null_with)),
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
        get::<M, C, T>(column, stored)
    }

    /// Converts a stored value into a nullable cast field.
    pub fn convert_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        value: Value,
    ) -> Result<Option<T>, OrmError> {
        match convert::<M, Option<C::Stored>>(column, value)? {
            Some(stored) => get::<M, C, T>(column, stored).map(Some),
            None => Ok(None),
        }
    }

    // ---------------------------------------------------------------------
    // Untyped input (mass assignment)
    // ---------------------------------------------------------------------

    fn input_error<M: Model>(
        column: &str,
        expected: Cow<'static, str>,
        message: String,
    ) -> OrmError {
        OrmError::Decode {
            model: Cow::Borrowed(M::meta().name()),
            column: column.to_owned(),
            expected,
            message,
        }
    }

    /// Converts untyped input into a field.
    pub fn input<M: Model, T: Field>(
        column: &str,
        input: serde_json::Value,
    ) -> Result<T, OrmError> {
        T::from_json(input).map_err(|error| {
            input_error::<M>(
                column,
                T::type_name(),
                match error {
                    FieldError::Null => "the input is null (make the field an Option)".to_owned(),
                    other => other.to_string(),
                },
            )
        })
    }

    /// Converts untyped input into a cast field.
    pub fn input_cast<M: Model, C: Cast<T>, T>(
        column: &str,
        input: serde_json::Value,
    ) -> Result<T, OrmError> {
        if input.is_null() {
            return Err(input_error::<M>(
                column,
                Cow::Borrowed(short_type_name::<T>()),
                "the input is null (make the field an Option)".to_owned(),
            ));
        }
        CastContext::with_current(|context| C::from_json(input, context))
            .map_err(|error| cast_error::<M>(column, error.to_string()))
    }

    /// Converts untyped input into a nullable cast field (`null` is `None`).
    pub fn input_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        input: serde_json::Value,
    ) -> Result<Option<T>, OrmError> {
        if input.is_null() {
            return Ok(None);
        }
        input_cast::<M, C, T>(column, input).map(Some)
    }

    /// `T`'s name without its module path, for messages.
    fn short_type_name<T>() -> &'static str {
        let full = std::any::type_name::<T>();
        let base = full.split('<').next().unwrap_or(full);
        base.rsplit("::").next().unwrap_or(base)
    }

    // ---------------------------------------------------------------------
    // Dirty tracking
    // ---------------------------------------------------------------------

    /// Whether two field values are stored as the same thing.
    pub fn same_field<T: Field>(a: &T, b: &T) -> bool {
        same_value(&a.to_value(), &b.to_value())
    }

    /// Whether two cast field values are stored as the same thing.
    pub fn same_cast<C: Cast<T>, T>(a: &T, b: &T) -> bool {
        CastContext::with_current(|context| C::same(a, b, context))
    }

    /// Whether two nullable cast field values are stored as the same thing.
    pub fn same_cast_option<C: Cast<T>, T>(a: &Option<T>, b: &Option<T>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => same_cast::<C, T>(a, b),
            (None, None) => true,
            _ => false,
        }
    }

    /// Whether cast column `column` of `row` holds a value to rewrite.
    pub fn stale_cast<C: Cast<T>, T>(row: &Row, column: &str) -> bool {
        CastContext::with_current(|context| C::is_stale(row, column, context))
    }

    // ---------------------------------------------------------------------
    // Preparation, serialization, increments
    // ---------------------------------------------------------------------

    /// Runs cast `C`'s preparation of a field before a write.
    pub async fn prepare_cast<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &mut T,
        context: &CastContext,
    ) -> Result<(), OrmError> {
        C::prepare(value, context)
            .await
            .map_err(|error| cast_error::<M>(column, error.to_string()))
    }

    /// Runs cast `C`'s preparation of a nullable field before a write.
    pub async fn prepare_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &mut Option<T>,
        context: &CastContext,
    ) -> Result<(), OrmError> {
        match value {
            Some(value) => prepare_cast::<M, C, T>(column, value, context).await,
            None => Ok(()),
        }
    }

    /// What cast `C` writes for a field in serialized records, if it
    /// decides it.
    pub fn serialize_cast<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &T,
    ) -> Result<Option<serde_json::Value>, OrmError> {
        C::serialize(value)
            .transpose()
            .map_err(|error| serialize_error::<M>(column, error.to_string()))
    }

    /// What cast `C` writes for a nullable field in serialized records, if
    /// it decides it (`None` stays `null`).
    pub fn serialize_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &Option<T>,
    ) -> Result<Option<serde_json::Value>, OrmError> {
        match value {
            Some(value) => serialize_cast::<M, C, T>(column, value),
            None => Ok(None),
        }
    }

    fn serialize_error<M: Model>(column: &str, message: String) -> OrmError {
        OrmError::Serialize {
            model: Cow::Borrowed(M::meta().name()),
            column: Some(column.to_owned()),
            message,
        }
    }

    /// A field as JSON (JSON columns edited by path).
    pub fn json_of<M: Model, T: Serialize>(
        column: &str,
        value: &T,
    ) -> Result<serde_json::Value, OrmError> {
        serde_json::to_value(value).map_err(|error| serialize_error::<M>(column, error.to_string()))
    }

    /// Adds or subtracts `amount` (stored form) to a cast field through
    /// `C::increment` / `C::decrement`.
    pub fn increment_cast<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &mut T,
        amount: Value,
        decrement: bool,
    ) -> Result<(), OrmError> {
        let amount = convert_cast::<M, C, T>(column, amount)?;
        let next = if decrement {
            C::decrement(value, &amount)
        } else {
            C::increment(value, &amount)
        };
        *value = next.map_err(|error| cast_error::<M>(column, error.to_string()))?;
        Ok(())
    }

    /// Like [`increment_cast`] for a nullable field: a `None` stays `None`
    /// (the database's `NULL + amount` is `NULL` too).
    pub fn increment_cast_option<M: Model, C: Cast<T>, T>(
        column: &str,
        value: &mut Option<T>,
        amount: Value,
        decrement: bool,
    ) -> Result<(), OrmError> {
        match value {
            Some(value) => increment_cast::<M, C, T>(column, value, amount, decrement),
            None => Ok(()),
        }
    }

    /// The error for a column that has no cast to increment through.
    pub fn not_a_cast_column<M: Model>(column: &str) -> OrmError {
        OrmError::InvalidQuery(format!(
            "`{column}` of {} is not a cast column: increment it with Record::increment",
            M::meta().name()
        ))
    }

    // ---------------------------------------------------------------------
    // Fields stored across several columns
    // ---------------------------------------------------------------------

    /// Builds a field from its stored columns through cast `C`.
    pub fn read_multi<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        stored: C::Stored,
    ) -> Result<T, OrmError> {
        CastContext::with_current(|context| C::get_with(stored, context))
            .map_err(|error| cast_error::<M>(field, error.to_string()))
    }

    /// The stored columns of a field through cast `C`.
    pub fn write_multi<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        value: &T,
    ) -> Result<C::Stored, OrmError> {
        CastContext::with_current(|context| C::set_with(value, context))
            .map_err(|error| cast_error::<M>(field, error.to_string()))
    }

    /// The stored columns of a nullable field (`None` is NULL in each).
    pub fn write_multi_option<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        value: &Option<T>,
    ) -> Result<Vec<Value>, OrmError> {
        match value {
            Some(value) => write_multi::<M, C, T>(field, value).map(|stored| stored.to_values()),
            None => Ok(C::Stored::nulls()),
        }
    }

    /// The stored columns of a field as values.
    pub fn multi_values<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        value: &T,
    ) -> Result<Vec<Value>, OrmError> {
        write_multi::<M, C, T>(field, value).map(|stored| stored.to_values())
    }

    fn multi_error<M: Model, C: MultiColumnCast<T>, T>(
        row: &Row,
        columns: &[&str],
        (index, error): (usize, FieldError),
    ) -> OrmError {
        let column = columns.get(index).copied().unwrap_or("?");
        let expected = C::Stored::type_name(index);
        match error {
            // The columns of a multi-column field have no default to read
            // instead.
            FieldError::Missing => OrmError::MissingAttribute {
                model: Cow::Borrowed(M::meta().name()),
                column: column.to_owned(),
                expected,
                selected: row.columns(),
            },
            other => decode_error(M::meta().name(), column, expected, &other),
        }
    }

    /// Reads a field stored across `columns` of `row`.
    pub fn read_multi_row<M: Model, C: MultiColumnCast<T>, T>(
        row: &Row,
        field: &str,
        columns: &[&str],
    ) -> Result<T, OrmError> {
        let stored = C::Stored::from_row(row, columns)
            .map_err(|error| multi_error::<M, C, T>(row, columns, error))?;
        read_multi::<M, C, T>(field, stored)
    }

    /// Reads a nullable field stored across `columns` of `row`: `None` when
    /// every column is NULL.
    pub fn read_multi_row_option<M: Model, C: MultiColumnCast<T>, T>(
        row: &Row,
        field: &str,
        columns: &[&str],
    ) -> Result<Option<T>, OrmError> {
        match C::Stored::from_row_option(row, columns)
            .map_err(|error| multi_error::<M, C, T>(row, columns, error))?
        {
            Some(stored) => read_multi::<M, C, T>(field, stored).map(Some),
            None => Ok(None),
        }
    }

    /// The field after replacing stored column `index` with `value`.
    pub fn replace_multi<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        current: &T,
        column: &str,
        index: usize,
        value: Value,
    ) -> Result<T, OrmError> {
        let mut values = write_multi::<M, C, T>(field, current)?.to_values();
        values[index] = value;
        let stored = C::Stored::from_values(values).map_err(|(index, error)| {
            decode_error(
                M::meta().name(),
                column,
                C::Stored::type_name(index),
                &error,
            )
        })?;
        read_multi::<M, C, T>(field, stored)
    }

    /// A nullable field after replacing stored column `index` with
    /// `value`: a `None` field takes NULLs in its other columns, and
    /// becomes `None` again when every column is NULL.
    pub fn replace_multi_option<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        current: &Option<T>,
        column: &str,
        index: usize,
        value: Value,
    ) -> Result<Option<T>, OrmError> {
        let mut values = write_multi_option::<M, C, T>(field, current)?;
        values[index] = value;
        if values.iter().all(crate::value::is_null) {
            return Ok(None);
        }
        let stored = C::Stored::from_values(values).map_err(|(index, error)| {
            decode_error(
                M::meta().name(),
                column,
                C::Stored::type_name(index),
                &error,
            )
        })?;
        read_multi::<M, C, T>(field, stored).map(Some)
    }

    /// A multi-column field from its stored column values.
    pub fn multi_from_values<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        columns: &[&str],
        values: Vec<Value>,
    ) -> Result<T, OrmError> {
        let stored = C::Stored::from_values(values).map_err(|(index, error)| {
            let column = columns.get(index).copied().unwrap_or(field);
            decode_error(
                M::meta().name(),
                column,
                C::Stored::type_name(index),
                &error,
            )
        })?;
        read_multi::<M, C, T>(field, stored)
    }

    /// A nullable multi-column field from its stored column values: `None`
    /// when every one is NULL.
    pub fn multi_from_values_option<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        columns: &[&str],
        values: Vec<Value>,
    ) -> Result<Option<T>, OrmError> {
        if values.iter().all(crate::value::is_null) {
            return Ok(None);
        }
        multi_from_values::<M, C, T>(field, columns, values).map(Some)
    }

    /// The stored columns of a multi-column field that differ between `a`
    /// and `b` (by index); every column when either cannot be encoded.
    pub fn changed_multi<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        a: &T,
        b: &T,
    ) -> Vec<usize> {
        match (
            multi_values::<M, C, T>(field, a),
            multi_values::<M, C, T>(field, b),
        ) {
            (Ok(a), Ok(b)) => (0..a.len())
                .filter(|&i| !same_value(&a[i], &b[i]))
                .collect(),
            _ => (0..C::Stored::COUNT).collect(),
        }
    }

    /// Like [`changed_multi`] for a nullable field.
    pub fn changed_multi_option<M: Model, C: MultiColumnCast<T>, T>(
        field: &str,
        a: &Option<T>,
        b: &Option<T>,
    ) -> Vec<usize> {
        match (
            write_multi_option::<M, C, T>(field, a),
            write_multi_option::<M, C, T>(field, b),
        ) {
            (Ok(a), Ok(b)) => (0..a.len())
                .filter(|&i| !same_value(&a[i], &b[i]))
                .collect(),
            _ => (0..C::Stored::COUNT).collect(),
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

    /// An accessor's value as JSON.
    pub fn accessor_json<T: Serialize>(value: &T) -> Result<serde_json::Value, CastError> {
        serde_json::to_value(value).map_err(|error| CastError::new(error.to_string()))
    }

    /// The JSON inside a stored value, if it is JSON.
    pub fn stored_json(value: Value) -> Option<serde_json::Value> {
        match value {
            Value::Json(Some(json)) => Some(*json),
            _ => None,
        }
    }

    /// A field's value, probed for being JSON (`json_attribute`): fields of
    /// type `serde_json::Value` (or an `Option` of it) answer with their
    /// JSON through [`JsonProbeJson`], every other field with `None`
    /// through [`JsonProbeAny`] (the derive calls
    /// `(&JsonProbe::<T>(&field)).json_attribute()`, which picks the first
    /// when it applies).
    pub struct JsonProbe<'a, T>(pub &'a T);

    /// [`JsonProbe`] of a JSON field.
    pub trait JsonProbeJson {
        /// The field's JSON.
        fn json_attribute(&self) -> Result<Option<serde_json::Value>, OrmError>;
    }

    impl JsonProbeJson for JsonProbe<'_, serde_json::Value> {
        fn json_attribute(&self) -> Result<Option<serde_json::Value>, OrmError> {
            Ok(Some(self.0.clone()))
        }
    }

    impl JsonProbeJson for JsonProbe<'_, Option<serde_json::Value>> {
        fn json_attribute(&self) -> Result<Option<serde_json::Value>, OrmError> {
            Ok(Some(self.0.clone().unwrap_or(serde_json::Value::Null)))
        }
    }

    /// [`JsonProbe`] of any other field.
    pub trait JsonProbeAny {
        /// `None`: the field is not JSON.
        fn json_attribute(&self) -> Result<Option<serde_json::Value>, OrmError>;
    }

    impl<T> JsonProbeAny for &JsonProbe<'_, T> {
        fn json_attribute(&self) -> Result<Option<serde_json::Value>, OrmError> {
            Ok(None)
        }
    }
}
