//! [`Blueprint`]: the description of a table being created or altered, and
//! its [`Command`]s.

use crate::column::{ColumnBuilder, ColumnDefinition, ColumnType, FluentIndex};
use crate::index::{
    Columns, ForeignKeyBuilder, ForeignKeyDefinition, IndexBuilder, IndexDefinition, IndexKind,
    IndexRef,
};
use crate::naming::index_name;
use crate::options::{MorphKey, SchemaOptions};
use crate::timescale::Hypertable;
use crate::value::Expression;

/// One change a blueprint makes, in the order it was declared.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// Add the column at this position of [`Blueprint::columns`] (or alter
    /// it, when the column is marked [`change`](ColumnBuilder::change)).
    /// Only in alter mode: a created table's columns are its body.
    Column(usize),
    /// Create an index (the primary key included).
    Index(IndexDefinition),
    /// Add a foreign key.
    Foreign(ForeignKeyDefinition),
    /// Drop columns.
    DropColumn(Vec<String>),
    /// Rename a column.
    RenameColumn {
        /// The current name.
        from: String,
        /// The new name.
        to: String,
    },
    /// Drop an index (or the primary key).
    DropIndex {
        /// The kind of index.
        kind: IndexKind,
        /// Its name (`None`: the primary key under its default name).
        name: Option<String>,
        /// Its columns, when it was named by columns.
        columns: Vec<String>,
    },
    /// Drop a foreign key.
    DropForeign {
        /// The constraint name.
        name: String,
        /// Its columns, when it was named by columns (SQLite drops foreign
        /// keys by columns).
        columns: Vec<String>,
    },
    /// Rename an index.
    RenameIndex {
        /// The current name.
        from: String,
        /// The new name.
        to: String,
    },
    /// Rename the table.
    Rename(String),
    /// Drop the table.
    Drop,
    /// Drop the table if it exists.
    DropIfExists,
    /// Set the table's comment.
    Comment(String),
}

/// The description of a table being created ([`Schema::create`]) or altered
/// ([`Schema::table`]): columns, indexes, foreign keys and table options.
///
/// Every column method returns a [`ColumnBuilder`] for modifiers; index
/// methods return an [`IndexBuilder`]; [`foreign`](Blueprint::foreign)
/// returns a [`ForeignKeyBuilder`].
///
/// ```
/// use navree_schema::Blueprint;
///
/// let mut t = Blueprint::new("deals");
/// t.id();
/// t.foreign_id("company_id").constrained().cascade_on_delete();
/// t.string("title");
/// t.decimal("amount", 12, 2);
/// t.timestamps();
/// t.soft_deletes();
/// let names: Vec<&str> = t.columns().iter().map(|c| c.name.as_str()).collect();
/// assert_eq!(
///     names,
///     ["id", "company_id", "title", "amount", "created_at", "updated_at", "deleted_at"]
/// );
/// ```
///
/// [`Schema::create`]: crate::Schema::create
/// [`Schema::table`]: crate::Schema::table
#[derive(Clone, Debug)]
pub struct Blueprint {
    pub(crate) table: String,
    pub(crate) columns: Vec<ColumnDefinition>,
    pub(crate) commands: Vec<Command>,
    pub(crate) creating: bool,
    pub(crate) if_not_exists: bool,
    pub(crate) engine: Option<String>,
    pub(crate) charset: Option<String>,
    pub(crate) collation: Option<String>,
    pub(crate) temporary: bool,
    pub(crate) after: Option<String>,
    pub(crate) hypertable: Option<Hypertable>,
    pub(crate) options: SchemaOptions,
}

impl Blueprint {
    /// A blueprint creating `table`, with the default
    /// [`SchemaOptions`].
    pub fn new(table: impl Into<String>) -> Blueprint {
        Blueprint::with_options(table, SchemaOptions::default())
    }

    /// A blueprint creating `table` with `options`.
    pub fn with_options(table: impl Into<String>, options: SchemaOptions) -> Blueprint {
        Blueprint {
            table: table.into(),
            columns: Vec::new(),
            commands: Vec::new(),
            creating: true,
            if_not_exists: false,
            engine: None,
            charset: None,
            collation: None,
            temporary: false,
            after: None,
            hypertable: None,
            options,
        }
    }

    /// A blueprint altering `table`.
    pub fn alter(table: impl Into<String>, options: SchemaOptions) -> Blueprint {
        Blueprint {
            creating: false,
            ..Blueprint::with_options(table, options)
        }
    }

    /// The table.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Whether the blueprint creates the table (rather than altering it).
    pub fn is_creating(&self) -> bool {
        self.creating
    }

    /// The columns, in declaration order.
    pub fn columns(&self) -> &[ColumnDefinition] {
        &self.columns
    }

    /// The commands, in declaration order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// The hypertable declared with [`hypertable`](Blueprint::hypertable).
    pub fn hypertable_definition(&self) -> Option<&Hypertable> {
        self.hypertable.as_ref()
    }

    /// The MySQL storage engine set with [`engine`](Blueprint::engine).
    pub fn storage_engine(&self) -> Option<&str> {
        self.engine.as_deref()
    }

    /// The options the blueprint uses for defaults.
    pub fn options(&self) -> &SchemaOptions {
        &self.options
    }

    // ----- table options ---------------------------------------------------

    /// The MySQL storage engine (`InnoDB`, `MyISAM`, ...); other databases
    /// ignore it.
    pub fn engine(&mut self, engine: impl Into<String>) {
        self.engine = Some(engine.into());
    }

    /// Uses InnoDB (MySQL).
    pub fn innodb(&mut self) {
        self.engine("InnoDB");
    }

    /// The table's default character set (MySQL).
    pub fn charset(&mut self, charset: impl Into<String>) {
        self.charset = Some(charset.into());
    }

    /// The table's default collation (MySQL).
    pub fn collation(&mut self, collation: impl Into<String>) {
        self.collation = Some(collation.into());
    }

    /// Creates a temporary table.
    pub fn temporary(&mut self) {
        self.temporary = true;
    }

    /// Creates the table only if it does not exist (`CREATE TABLE IF NOT
    /// EXISTS`).
    pub fn if_not_exists(&mut self) {
        self.if_not_exists = true;
    }

    /// A comment on the table (PostgreSQL and MySQL).
    pub fn comment(&mut self, comment: impl Into<String>) {
        self.commands.push(Command::Comment(comment.into()));
    }

    /// Makes the table a TimescaleDB hypertable partitioned by `hypertable`'s
    /// time column: `t.hypertable("occurred_at")`, or a configured
    /// [`Hypertable`]. On databases without TimescaleDB the table stays a
    /// regular table with an index on the time column, and the schema
    /// reports a notice.
    pub fn hypertable(&mut self, hypertable: impl Into<Hypertable>) {
        self.hypertable = Some(hypertable.into());
    }

    /// Declares the columns added by `f` after `column` (MySQL), each after
    /// the previous one.
    pub fn after(&mut self, column: impl Into<String>, f: impl FnOnce(&mut Blueprint)) {
        self.after = Some(column.into());
        f(self);
        self.after = None;
    }

    // ----- columns ---------------------------------------------------------

    /// Adds a column of `kind`; the typed methods (`string`, `integer`, ...)
    /// call this.
    pub fn add_column(&mut self, name: impl Into<String>, kind: ColumnType) -> ColumnBuilder<'_> {
        let mut column = ColumnDefinition::new(name, kind);
        if let Some(after) = self.after.take() {
            column.after = Some(after);
            self.after = Some(column.name.clone());
        }
        self.columns.push(column);
        let index = self.columns.len() - 1;
        if !self.creating {
            self.commands.push(Command::Column(index));
        }
        ColumnBuilder::new(self, index)
    }

    fn time_type(&self, kind: fn(Option<u8>) -> ColumnType) -> ColumnType {
        kind(self.options.default_time_precision)
    }

    /// An auto-incrementing unsigned big-integer primary key named `id`.
    pub fn id(&mut self) -> ColumnBuilder<'_> {
        self.big_increments("id")
    }

    /// An auto-incrementing unsigned integer primary key.
    pub fn increments(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Integer)
            .unsigned()
            .auto_increment()
    }

    /// Same as [`increments`](Self::increments).
    pub fn integer_increments(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.increments(name)
    }

    /// An auto-incrementing unsigned tiny-integer primary key.
    pub fn tiny_increments(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::TinyInteger)
            .unsigned()
            .auto_increment()
    }

    /// An auto-incrementing unsigned small-integer primary key.
    pub fn small_increments(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::SmallInteger)
            .unsigned()
            .auto_increment()
    }

    /// An auto-incrementing unsigned medium-integer primary key.
    pub fn medium_increments(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::MediumInteger)
            .unsigned()
            .auto_increment()
    }

    /// An auto-incrementing unsigned big-integer primary key.
    pub fn big_increments(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::BigInteger)
            .unsigned()
            .auto_increment()
    }

    /// A UUID primary key named `id` (Navree generates UUID v7 values).
    pub fn uuid_primary(&mut self) -> ColumnBuilder<'_> {
        self.uuid("id").primary()
    }

    /// A ULID primary key named `id` (`CHAR(26)`).
    pub fn ulid_primary(&mut self) -> ColumnBuilder<'_> {
        self.ulid("id").primary()
    }

    /// A fixed-length text column of `length` characters.
    pub fn char(&mut self, name: impl Into<String>, length: u32) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Char { length })
    }

    /// A variable-length text column (`VARCHAR(255)` by default; set the
    /// length with [`length`](ColumnBuilder::length)).
    pub fn string(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        let length = self.options.default_string_length;
        self.add_column(name, ColumnType::String { length })
    }

    /// A tiny text column (MySQL `TINYTEXT`).
    pub fn tiny_text(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::TinyText)
    }

    /// A text column.
    pub fn text(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Text)
    }

    /// A medium text column (MySQL `MEDIUMTEXT`).
    pub fn medium_text(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::MediumText)
    }

    /// A long text column (MySQL `LONGTEXT`).
    pub fn long_text(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::LongText)
    }

    /// A 4-byte integer column.
    pub fn integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Integer)
    }

    /// A 1-byte integer column.
    pub fn tiny_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::TinyInteger)
    }

    /// A 2-byte integer column.
    pub fn small_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::SmallInteger)
    }

    /// A 3-byte integer column (MySQL; 4 bytes elsewhere).
    pub fn medium_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::MediumInteger)
    }

    /// An 8-byte integer column.
    pub fn big_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::BigInteger)
    }

    /// An unsigned 4-byte integer column.
    pub fn unsigned_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.integer(name).unsigned()
    }

    /// An unsigned 1-byte integer column.
    pub fn unsigned_tiny_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.tiny_integer(name).unsigned()
    }

    /// An unsigned 2-byte integer column.
    pub fn unsigned_small_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.small_integer(name).unsigned()
    }

    /// An unsigned 3-byte integer column.
    pub fn unsigned_medium_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.medium_integer(name).unsigned()
    }

    /// An unsigned 8-byte integer column.
    pub fn unsigned_big_integer(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.big_integer(name).unsigned()
    }

    /// An unsigned big-integer column for a foreign key to an [`id`](Self::id)
    /// key; chain [`constrained`](ColumnBuilder::constrained) to add the
    /// constraint.
    pub fn foreign_id(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.unsigned_big_integer(name)
    }

    /// A UUID column for a foreign key to a [`uuid_primary`](Self::uuid_primary) key.
    pub fn foreign_uuid(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.uuid(name)
    }

    /// A ULID column for a foreign key to a [`ulid_primary`](Self::ulid_primary) key.
    pub fn foreign_ulid(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.ulid(name)
    }

    /// A floating-point column (53 bits of precision: a double; set fewer
    /// with [`precision`](ColumnBuilder::precision)).
    pub fn float(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Float { precision: 53 })
    }

    /// A double-precision floating-point column.
    pub fn double(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Double)
    }

    /// An exact decimal column with `precision` digits, `scale` of them
    /// after the point.
    pub fn decimal(
        &mut self,
        name: impl Into<String>,
        precision: u32,
        scale: u32,
    ) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Decimal { precision, scale })
    }

    /// A boolean column.
    pub fn boolean(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Boolean)
    }

    /// A column holding one of `values` (MySQL `ENUM`; a checked
    /// `VARCHAR` elsewhere).
    pub fn enumeration<V: Into<String>>(
        &mut self,
        name: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> ColumnBuilder<'_> {
        let values = values.into_iter().map(Into::into).collect();
        self.add_column(name, ColumnType::Enum { values })
    }

    /// A MySQL `SET` column holding any subset of `values`.
    pub fn set<V: Into<String>>(
        &mut self,
        name: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> ColumnBuilder<'_> {
        let values = values.into_iter().map(Into::into).collect();
        self.add_column(name, ColumnType::Set { values })
    }

    /// A JSON column.
    pub fn json(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Json)
    }

    /// A binary JSON column (`JSONB` on PostgreSQL).
    pub fn jsonb(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Jsonb)
    }

    /// A date column.
    pub fn date(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Date)
    }

    /// A date-time column without time zone.
    pub fn date_time(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        let kind = self.time_type(|precision| ColumnType::DateTime { precision });
        self.add_column(name, kind)
    }

    /// A date-time column with time zone (PostgreSQL).
    pub fn date_time_tz(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        let kind = self.time_type(|precision| ColumnType::DateTimeTz { precision });
        self.add_column(name, kind)
    }

    /// A time-of-day column.
    pub fn time(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        let kind = self.time_type(|precision| ColumnType::Time { precision });
        self.add_column(name, kind)
    }

    /// A time-of-day column with time zone (PostgreSQL).
    pub fn time_tz(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        let kind = self.time_type(|precision| ColumnType::TimeTz { precision });
        self.add_column(name, kind)
    }

    /// A timestamp column without time zone.
    pub fn timestamp(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        let kind = self.time_type(|precision| ColumnType::Timestamp { precision });
        self.add_column(name, kind)
    }

    /// A timestamp column with time zone (`TIMESTAMPTZ` on PostgreSQL), the
    /// right type for instants.
    pub fn timestamp_tz(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        let kind = self.time_type(|precision| ColumnType::TimestampTz { precision });
        self.add_column(name, kind)
    }

    /// Nullable `created_at` and `updated_at` timestamps.
    pub fn timestamps(&mut self) {
        self.timestamp("created_at").nullable();
        self.timestamp("updated_at").nullable();
    }

    /// Nullable `created_at` and `updated_at` timestamps with time zone.
    pub fn timestamps_tz(&mut self) {
        self.timestamp_tz("created_at").nullable();
        self.timestamp_tz("updated_at").nullable();
    }

    /// Same as [`timestamps`](Self::timestamps).
    pub fn nullable_timestamps(&mut self) {
        self.timestamps();
    }

    /// Nullable `created_at` and `updated_at` date-time columns.
    pub fn datetimes(&mut self) {
        self.date_time("created_at").nullable();
        self.date_time("updated_at").nullable();
    }

    /// A nullable `deleted_at` timestamp for soft deletes.
    pub fn soft_deletes(&mut self) -> ColumnBuilder<'_> {
        self.timestamp("deleted_at").nullable()
    }

    /// A nullable `deleted_at` timestamp with time zone for soft deletes.
    pub fn soft_deletes_tz(&mut self) -> ColumnBuilder<'_> {
        self.timestamp_tz("deleted_at").nullable()
    }

    /// A nullable `deleted_at` date-time for soft deletes.
    pub fn soft_deletes_datetime(&mut self) -> ColumnBuilder<'_> {
        self.date_time("deleted_at").nullable()
    }

    /// A year column.
    pub fn year(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Year)
    }

    /// A binary column (`BLOB`/`BYTEA`; bound it with
    /// [`length`](ColumnBuilder::length), fix it with
    /// [`fixed`](ColumnBuilder::fixed)).
    pub fn binary(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(
            name,
            ColumnType::Binary {
                length: None,
                fixed: false,
            },
        )
    }

    /// A UUID column.
    pub fn uuid(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::Uuid)
    }

    /// A ULID column (`CHAR(26)`).
    pub fn ulid(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.char(name, 26)
    }

    /// An IP address column (`INET` on PostgreSQL).
    pub fn ip_address(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::IpAddress)
    }

    /// A MAC address column (`MACADDR` on PostgreSQL).
    pub fn mac_address(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::MacAddress)
    }

    /// A geometry column (PostGIS on PostgreSQL); set
    /// [`subtype`](ColumnBuilder::subtype) and [`srid`](ColumnBuilder::srid).
    pub fn geometry(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(
            name,
            ColumnType::Geometry {
                subtype: None,
                srid: None,
            },
        )
    }

    /// A geography column (PostGIS on PostgreSQL), SRID 4326 by default.
    pub fn geography(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(
            name,
            ColumnType::Geography {
                subtype: None,
                srid: 4326,
            },
        )
    }

    /// An embedding vector column of `dimensions` (pgvector on PostgreSQL;
    /// MySQL 9).
    pub fn vector(&mut self, name: impl Into<String>, dimensions: u32) -> ColumnBuilder<'_> {
        self.add_column(
            name,
            ColumnType::Vector {
                dimensions: Some(dimensions),
            },
        )
    }

    /// A full-text search vector column (PostgreSQL).
    pub fn tsvector(&mut self, name: impl Into<String>) -> ColumnBuilder<'_> {
        self.add_column(name, ColumnType::TsVector)
    }

    /// A column whose type is written as raw SQL: `raw_column("rank", "SMALLINT CHECK (rank > 0)")`.
    pub fn raw_column(
        &mut self,
        name: impl Into<String>,
        definition: impl Into<String>,
    ) -> ColumnBuilder<'_> {
        self.add_column(
            name,
            ColumnType::Raw {
                definition: definition.into(),
            },
        )
    }

    /// A nullable `remember_token` string of 100 characters.
    pub fn remember_token(&mut self) -> ColumnBuilder<'_> {
        self.string("remember_token").length(100).nullable()
    }

    fn morph_columns(&mut self, name: &str, key: MorphKey, nullable: bool, index: Option<&str>) {
        let type_column = format!("{name}_type");
        let id_column = format!("{name}_id");
        let column = self.string(type_column.clone());
        if nullable {
            column.nullable();
        }
        let column = match key {
            MorphKey::Int => self.unsigned_big_integer(id_column.clone()),
            MorphKey::Uuid => self.uuid(id_column.clone()),
            MorphKey::Ulid => self.ulid(id_column.clone()),
        };
        if nullable {
            column.nullable();
        }
        let builder = self.index([type_column.as_str(), id_column.as_str()]);
        if let Some(index) = index {
            builder.name(index);
        }
    }

    /// `{name}_type` and `{name}_id` columns for a polymorphic relation, with
    /// an index on both. The id's type follows `schema.morph_key` (`int` by
    /// default).
    pub fn morphs(&mut self, name: &str) {
        let key = self.options.morph_key;
        self.morph_columns(name, key, false, None);
    }

    /// [`morphs`](Self::morphs) with an explicit index name.
    pub fn morphs_with_index(&mut self, name: &str, index: &str) {
        let key = self.options.morph_key;
        self.morph_columns(name, key, false, Some(index));
    }

    /// Nullable [`morphs`](Self::morphs).
    pub fn nullable_morphs(&mut self, name: &str) {
        let key = self.options.morph_key;
        self.morph_columns(name, key, true, None);
    }

    /// [`morphs`](Self::morphs) with an unsigned big-integer id.
    pub fn numeric_morphs(&mut self, name: &str) {
        self.morph_columns(name, MorphKey::Int, false, None);
    }

    /// Nullable [`numeric_morphs`](Self::numeric_morphs).
    pub fn nullable_numeric_morphs(&mut self, name: &str) {
        self.morph_columns(name, MorphKey::Int, true, None);
    }

    /// [`morphs`](Self::morphs) with a UUID id.
    pub fn uuid_morphs(&mut self, name: &str) {
        self.morph_columns(name, MorphKey::Uuid, false, None);
    }

    /// Nullable [`uuid_morphs`](Self::uuid_morphs).
    pub fn nullable_uuid_morphs(&mut self, name: &str) {
        self.morph_columns(name, MorphKey::Uuid, true, None);
    }

    /// [`morphs`](Self::morphs) with a ULID id.
    pub fn ulid_morphs(&mut self, name: &str) {
        self.morph_columns(name, MorphKey::Ulid, false, None);
    }

    /// Nullable [`ulid_morphs`](Self::ulid_morphs).
    pub fn nullable_ulid_morphs(&mut self, name: &str) {
        self.morph_columns(name, MorphKey::Ulid, true, None);
    }

    // ----- indexes and foreign keys ---------------------------------------

    pub(crate) fn conventional_name(&self, kind: IndexKind, columns: &[String]) -> String {
        index_name(&self.table, columns, kind.suffix())
    }

    pub(crate) fn foreign_name(&self, columns: &[String]) -> String {
        index_name(&self.table, columns, "foreign")
    }

    fn index_command(&mut self, kind: IndexKind, columns: Vec<String>) -> IndexBuilder<'_> {
        let name = self.conventional_name(kind, &columns);
        self.commands
            .push(Command::Index(IndexDefinition::new(kind, name, columns)));
        match self.commands.last_mut() {
            Some(Command::Index(index)) => IndexBuilder::new(index),
            _ => unreachable!("an index command was just pushed"),
        }
    }

    /// Sets the primary key: `t.primary(["workspace_id", "user_id"])`.
    pub fn primary(&mut self, columns: impl Columns) -> IndexBuilder<'_> {
        self.index_command(IndexKind::Primary, columns.into_columns())
    }

    /// Adds a unique index (`{table}_{columns}_unique`).
    pub fn unique(&mut self, columns: impl Columns) -> IndexBuilder<'_> {
        self.index_command(IndexKind::Unique, columns.into_columns())
    }

    /// Adds an index (`{table}_{columns}_index`).
    pub fn index(&mut self, columns: impl Columns) -> IndexBuilder<'_> {
        self.index_command(IndexKind::Index, columns.into_columns())
    }

    /// Adds a full-text index (PostgreSQL `GIN` over `to_tsvector`, MySQL
    /// `FULLTEXT`).
    pub fn fulltext(&mut self, columns: impl Columns) -> IndexBuilder<'_> {
        self.index_command(IndexKind::Fulltext, columns.into_columns())
    }

    /// Adds a spatial index (PostgreSQL `GIST`, MySQL `SPATIAL`).
    pub fn spatial_index(&mut self, columns: impl Columns) -> IndexBuilder<'_> {
        self.index_command(IndexKind::Spatial, columns.into_columns())
    }

    /// Adds an index on an expression: `t.raw_index("lower(email)", "users_email_lower_index")`.
    pub fn raw_index(
        &mut self,
        expression: impl Into<Expression>,
        name: impl Into<String>,
    ) -> IndexBuilder<'_> {
        let mut index = IndexDefinition::new(IndexKind::Index, name, Vec::new());
        index.expression = Some(expression.into());
        self.commands.push(Command::Index(index));
        match self.commands.last_mut() {
            Some(Command::Index(index)) => IndexBuilder::new(index),
            _ => unreachable!("an index command was just pushed"),
        }
    }

    /// Adds a foreign key on `columns`; chain
    /// [`references`](ForeignKeyBuilder::references) and
    /// [`on`](ForeignKeyBuilder::on).
    pub fn foreign(&mut self, columns: impl Columns) -> ForeignKeyBuilder<'_> {
        let columns = columns.into_columns();
        let name = self.foreign_name(&columns);
        self.commands.push(Command::Foreign(ForeignKeyDefinition::new(
            name, columns,
        )));
        match self.commands.last_mut() {
            Some(Command::Foreign(foreign)) => ForeignKeyBuilder::new(foreign),
            _ => unreachable!("a foreign key command was just pushed"),
        }
    }

    // ----- drops and renames ----------------------------------------------

    /// Drops the table.
    pub fn drop(&mut self) {
        self.commands.push(Command::Drop);
    }

    /// Drops the table if it exists.
    pub fn drop_if_exists(&mut self) {
        self.commands.push(Command::DropIfExists);
    }

    /// Renames the table.
    pub fn rename(&mut self, to: impl Into<String>) {
        self.commands.push(Command::Rename(to.into()));
    }

    /// Drops columns: `t.drop_column("legacy")`, `t.drop_column(["a", "b"])`.
    pub fn drop_column(&mut self, columns: impl Columns) {
        self.commands
            .push(Command::DropColumn(columns.into_columns()));
    }

    /// Renames a column.
    pub fn rename_column(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.commands.push(Command::RenameColumn {
            from: from.into(),
            to: to.into(),
        });
    }

    fn drop_index_command(&mut self, kind: IndexKind, index: IndexRef) {
        let (name, columns) = match index {
            IndexRef::Name(name) => (name, Vec::new()),
            IndexRef::Columns(columns) => (self.conventional_name(kind, &columns), columns),
        };
        self.commands.push(Command::DropIndex {
            kind,
            name: Some(name),
            columns,
        });
    }

    /// Drops the primary key (under the database's default name).
    pub fn drop_primary(&mut self) {
        self.commands.push(Command::DropIndex {
            kind: IndexKind::Primary,
            name: None,
            columns: Vec::new(),
        });
    }

    /// Drops the primary key named `name`.
    pub fn drop_primary_named(&mut self, name: impl Into<String>) {
        self.commands.push(Command::DropIndex {
            kind: IndexKind::Primary,
            name: Some(name.into()),
            columns: Vec::new(),
        });
    }

    /// Drops a unique index, by name or by columns.
    pub fn drop_unique(&mut self, index: impl Into<IndexRef>) {
        self.drop_index_command(IndexKind::Unique, index.into());
    }

    /// Drops an index, by name or by columns.
    pub fn drop_index(&mut self, index: impl Into<IndexRef>) {
        self.drop_index_command(IndexKind::Index, index.into());
    }

    /// Drops a full-text index, by name or by columns.
    pub fn drop_fulltext(&mut self, index: impl Into<IndexRef>) {
        self.drop_index_command(IndexKind::Fulltext, index.into());
    }

    /// Drops a spatial index, by name or by columns.
    pub fn drop_spatial_index(&mut self, index: impl Into<IndexRef>) {
        self.drop_index_command(IndexKind::Spatial, index.into());
    }

    /// Drops a foreign key, by name or by columns.
    pub fn drop_foreign(&mut self, foreign: impl Into<IndexRef>) {
        let (name, columns) = match foreign.into() {
            IndexRef::Name(name) => (name, Vec::new()),
            IndexRef::Columns(columns) => (self.foreign_name(&columns), columns),
        };
        self.commands.push(Command::DropForeign { name, columns });
    }

    /// Drops the foreign key on `column`, then the column.
    pub fn drop_constrained_foreign_id(&mut self, column: &str) {
        self.drop_foreign([column]);
        self.drop_column(column);
    }

    /// Renames an index.
    pub fn rename_index(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.commands.push(Command::RenameIndex {
            from: from.into(),
            to: to.into(),
        });
    }

    /// Drops `created_at` and `updated_at`.
    pub fn drop_timestamps(&mut self) {
        self.drop_column(["created_at", "updated_at"]);
    }

    /// Same as [`drop_timestamps`](Self::drop_timestamps).
    pub fn drop_timestamps_tz(&mut self) {
        self.drop_timestamps();
    }

    /// Drops `deleted_at`.
    pub fn drop_soft_deletes(&mut self) {
        self.drop_column("deleted_at");
    }

    /// Same as [`drop_soft_deletes`](Self::drop_soft_deletes).
    pub fn drop_soft_deletes_tz(&mut self) {
        self.drop_soft_deletes();
    }

    /// Drops `remember_token`.
    pub fn drop_remember_token(&mut self) {
        self.drop_column("remember_token");
    }

    /// Drops the index and columns of [`morphs`](Self::morphs).
    pub fn drop_morphs(&mut self, name: &str) {
        let type_column = format!("{name}_type");
        let id_column = format!("{name}_id");
        self.drop_index([type_column.as_str(), id_column.as_str()]);
        self.drop_column([type_column, id_column]);
    }

    // ----- compilation support --------------------------------------------

    /// The commands to compile: the declared ones, then the indexes declared
    /// on columns (`.unique()`, `.index()`, ...).
    pub(crate) fn resolved_commands(&self) -> Vec<Command> {
        let mut commands = self.commands.clone();
        for column in &self.columns {
            for kind in [
                IndexKind::Primary,
                IndexKind::Unique,
                IndexKind::Index,
                IndexKind::Fulltext,
                IndexKind::Spatial,
            ] {
                let fluent = match kind {
                    IndexKind::Primary => &column.primary,
                    IndexKind::Unique => &column.unique,
                    IndexKind::Index => &column.index,
                    IndexKind::Fulltext => &column.fulltext,
                    IndexKind::Spatial => &column.spatial_index,
                };
                let columns = vec![column.name.clone()];
                match fluent {
                    Some(FluentIndex::Add(name)) => {
                        let name = name
                            .clone()
                            .unwrap_or_else(|| self.conventional_name(kind, &columns));
                        commands.push(Command::Index(IndexDefinition::new(kind, name, columns)));
                    }
                    Some(FluentIndex::Drop) if column.change => {
                        let name = (kind != IndexKind::Primary)
                            .then(|| self.conventional_name(kind, &columns));
                        commands.push(Command::DropIndex {
                            kind,
                            name,
                            columns,
                        });
                    }
                    _ => {}
                }
            }
        }
        commands
    }
}
