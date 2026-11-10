//! [`ColumnType`], [`ColumnDefinition`] and [`ColumnBuilder`], the fluent
//! handle every column method of a [`Blueprint`] returns.

use crate::blueprint::Blueprint;
use crate::index::{ForeignKeyBuilder, IndexKind};
use crate::naming::table_for_foreign_key;
use crate::value::{DefaultValue, Expression};

/// The type of a column.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ColumnType {
    /// Fixed-length text.
    Char {
        /// The length in characters.
        length: u32,
    },
    /// Variable-length text.
    String {
        /// The maximum length in characters.
        length: u32,
    },
    /// Short text (MySQL `TINYTEXT`, up to 255 bytes).
    TinyText,
    /// Text (MySQL `TEXT`, up to 64 KiB).
    Text,
    /// Medium text (MySQL `MEDIUMTEXT`, up to 16 MiB).
    MediumText,
    /// Long text (MySQL `LONGTEXT`, up to 4 GiB).
    LongText,
    /// A 1-byte integer.
    TinyInteger,
    /// A 2-byte integer.
    SmallInteger,
    /// A 3-byte integer (MySQL; 4 bytes elsewhere).
    MediumInteger,
    /// A 4-byte integer.
    Integer,
    /// An 8-byte integer.
    BigInteger,
    /// A floating-point number with `precision` bits (53: double).
    Float {
        /// The precision in bits.
        precision: u32,
    },
    /// A double-precision floating-point number.
    Double,
    /// An exact decimal number.
    Decimal {
        /// The total number of digits.
        precision: u32,
        /// The digits after the decimal point.
        scale: u32,
    },
    /// A boolean.
    Boolean,
    /// One of a fixed set of strings.
    Enum {
        /// The allowed values.
        values: Vec<String>,
    },
    /// Any subset of a fixed set of strings (MySQL only).
    Set {
        /// The allowed values.
        values: Vec<String>,
    },
    /// A JSON document.
    Json,
    /// A binary JSON document (PostgreSQL `JSONB`; `JSON` elsewhere).
    Jsonb,
    /// A date.
    Date,
    /// A date and time without time zone.
    DateTime {
        /// Fractional-second digits (`None`: the database default).
        precision: Option<u8>,
    },
    /// A date and time with time zone (PostgreSQL); a date and time
    /// elsewhere.
    DateTimeTz {
        /// Fractional-second digits.
        precision: Option<u8>,
    },
    /// A time of day.
    Time {
        /// Fractional-second digits.
        precision: Option<u8>,
    },
    /// A time of day with time zone (PostgreSQL).
    TimeTz {
        /// Fractional-second digits.
        precision: Option<u8>,
    },
    /// A timestamp without time zone.
    Timestamp {
        /// Fractional-second digits.
        precision: Option<u8>,
    },
    /// A timestamp with time zone (PostgreSQL `TIMESTAMPTZ`).
    TimestampTz {
        /// Fractional-second digits.
        precision: Option<u8>,
    },
    /// A year.
    Year,
    /// Binary data.
    Binary {
        /// The length in bytes, when bounded.
        length: Option<u32>,
        /// Fixed length (`BINARY(n)`) rather than variable (`VARBINARY(n)`).
        fixed: bool,
    },
    /// A UUID.
    Uuid,
    /// An IPv4 or IPv6 address.
    IpAddress,
    /// A MAC address.
    MacAddress,
    /// A geometry (PostGIS on PostgreSQL).
    Geometry {
        /// `point`, `linestring`, `polygon`, ...
        subtype: Option<String>,
        /// The spatial reference id.
        srid: Option<u32>,
    },
    /// A geography (PostGIS on PostgreSQL).
    Geography {
        /// `point`, `linestring`, `polygon`, ...
        subtype: Option<String>,
        /// The spatial reference id (4326 by default).
        srid: u32,
    },
    /// An embedding vector (pgvector on PostgreSQL, MySQL 9).
    Vector {
        /// The number of dimensions.
        dimensions: Option<u32>,
    },
    /// A PostgreSQL full-text search vector.
    TsVector,
    /// A type written as raw SQL.
    Raw {
        /// The type definition.
        definition: String,
    },
}

impl ColumnType {
    /// The blueprint method that makes this type, for messages.
    pub fn name(&self) -> &'static str {
        match self {
            ColumnType::Char { .. } => "char",
            ColumnType::String { .. } => "string",
            ColumnType::TinyText => "tiny_text",
            ColumnType::Text => "text",
            ColumnType::MediumText => "medium_text",
            ColumnType::LongText => "long_text",
            ColumnType::TinyInteger => "tiny_integer",
            ColumnType::SmallInteger => "small_integer",
            ColumnType::MediumInteger => "medium_integer",
            ColumnType::Integer => "integer",
            ColumnType::BigInteger => "big_integer",
            ColumnType::Float { .. } => "float",
            ColumnType::Double => "double",
            ColumnType::Decimal { .. } => "decimal",
            ColumnType::Boolean => "boolean",
            ColumnType::Enum { .. } => "enumeration",
            ColumnType::Set { .. } => "set",
            ColumnType::Json => "json",
            ColumnType::Jsonb => "jsonb",
            ColumnType::Date => "date",
            ColumnType::DateTime { .. } => "date_time",
            ColumnType::DateTimeTz { .. } => "date_time_tz",
            ColumnType::Time { .. } => "time",
            ColumnType::TimeTz { .. } => "time_tz",
            ColumnType::Timestamp { .. } => "timestamp",
            ColumnType::TimestampTz { .. } => "timestamp_tz",
            ColumnType::Year => "year",
            ColumnType::Binary { .. } => "binary",
            ColumnType::Uuid => "uuid",
            ColumnType::IpAddress => "ip_address",
            ColumnType::MacAddress => "mac_address",
            ColumnType::Geometry { .. } => "geometry",
            ColumnType::Geography { .. } => "geography",
            ColumnType::Vector { .. } => "vector",
            ColumnType::TsVector => "tsvector",
            ColumnType::Raw { .. } => "raw_column",
        }
    }

    /// Whether this is one of the integer types (which may auto-increment).
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            ColumnType::TinyInteger
                | ColumnType::SmallInteger
                | ColumnType::MediumInteger
                | ColumnType::Integer
                | ColumnType::BigInteger
        )
    }

    /// Whether this is a date/time type (which may default to the current
    /// time with `use_current`).
    pub fn is_temporal(&self) -> bool {
        matches!(
            self,
            ColumnType::Date
                | ColumnType::DateTime { .. }
                | ColumnType::DateTimeTz { .. }
                | ColumnType::Time { .. }
                | ColumnType::TimeTz { .. }
                | ColumnType::Timestamp { .. }
                | ColumnType::TimestampTz { .. }
                | ColumnType::Year
        )
    }

    /// The fractional-second precision of a time type.
    pub fn precision(&self) -> Option<u8> {
        match self {
            ColumnType::DateTime { precision }
            | ColumnType::DateTimeTz { precision }
            | ColumnType::Time { precision }
            | ColumnType::TimeTz { precision }
            | ColumnType::Timestamp { precision }
            | ColumnType::TimestampTz { precision } => *precision,
            _ => None,
        }
    }
}

/// An index declared on the column itself (`.unique()`, `.index()`, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FluentIndex {
    /// Add the index, with this name or the conventional one.
    Add(Option<String>),
    /// Drop the index with the conventional name (with
    /// [`change`](ColumnBuilder::change)).
    Drop,
}

/// A PostgreSQL identity column (`GENERATED ... AS IDENTITY`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Identity {
    /// `GENERATED ALWAYS` (values cannot be supplied) rather than `BY
    /// DEFAULT`.
    pub always: bool,
    /// Sequence options, e.g. `START WITH 1000 INCREMENT BY 10`.
    pub options: Option<String>,
}

/// A column of a [`Blueprint`]: its name, type and modifiers.
///
/// Built through [`ColumnBuilder`]; the fields are public so drivers and
/// tools can read a blueprint.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ColumnDefinition {
    /// The column name.
    pub name: String,
    /// The type.
    pub kind: ColumnType,
    /// `Some(true)`: nullable; `Some(false)`: `NOT NULL` explicitly;
    /// `None`: not said (`NOT NULL`, except generated columns).
    pub nullable: Option<bool>,
    /// The default value.
    pub default: Option<DefaultValue>,
    /// Unsigned (MySQL).
    pub unsigned: bool,
    /// Auto-incrementing (identity on PostgreSQL).
    pub auto_increment: bool,
    /// Defaults to the current date/time.
    pub use_current: bool,
    /// Set to the current time on every update (MySQL `ON UPDATE`).
    pub use_current_on_update: bool,
    /// A raw `ON UPDATE` expression (MySQL).
    pub on_update: Option<Expression>,
    /// A comment.
    pub comment: Option<String>,
    /// The collation.
    pub collation: Option<String>,
    /// The character set (MySQL).
    pub charset: Option<String>,
    /// Place after this column (MySQL).
    pub after: Option<String>,
    /// Place first (MySQL).
    pub first: bool,
    /// A virtual generated column's expression.
    pub virtual_as: Option<Expression>,
    /// A stored generated column's expression.
    pub stored_as: Option<Expression>,
    /// An identity column (PostgreSQL).
    pub identity: Option<Identity>,
    /// The first value of an auto-incrementing column.
    pub starting_value: Option<u64>,
    /// Hidden from `SELECT *` (MySQL).
    pub invisible: bool,
    /// Alter an existing column instead of adding one.
    pub change: bool,
    /// The conversion expression of a type change (PostgreSQL `USING`).
    pub using: Option<Expression>,
    /// A primary key on this column.
    pub primary: Option<FluentIndex>,
    /// A unique index on this column.
    pub unique: Option<FluentIndex>,
    /// A plain index on this column.
    pub index: Option<FluentIndex>,
    /// A full-text index on this column.
    pub fulltext: Option<FluentIndex>,
    /// A spatial index on this column.
    pub spatial_index: Option<FluentIndex>,
    /// Modifiers that did not apply, reported when the blueprint compiles.
    pub(crate) problems: Vec<String>,
}

impl ColumnDefinition {
    /// A `NOT NULL` column of `kind` with no modifiers.
    pub fn new(name: impl Into<String>, kind: ColumnType) -> ColumnDefinition {
        ColumnDefinition {
            name: name.into(),
            kind,
            nullable: None,
            default: None,
            unsigned: false,
            auto_increment: false,
            use_current: false,
            use_current_on_update: false,
            on_update: None,
            comment: None,
            collation: None,
            charset: None,
            after: None,
            first: false,
            virtual_as: None,
            stored_as: None,
            identity: None,
            starting_value: None,
            invisible: false,
            change: false,
            using: None,
            primary: None,
            unique: None,
            index: None,
            fulltext: None,
            spatial_index: None,
            problems: Vec::new(),
        }
    }

    /// Whether the column accepts `NULL`.
    pub fn is_nullable(&self) -> bool {
        self.nullable == Some(true)
    }

    /// Whether this is a generated (computed) column.
    pub fn is_generated(&self) -> bool {
        self.virtual_as.is_some() || self.stored_as.is_some()
    }

    /// Whether the column is an auto-incrementing integer.
    pub fn is_auto_increment(&self) -> bool {
        self.auto_increment && self.kind.is_integer()
    }

    pub(crate) fn fluent(&mut self, kind: IndexKind) -> &mut Option<FluentIndex> {
        match kind {
            IndexKind::Primary => &mut self.primary,
            IndexKind::Unique => &mut self.unique,
            IndexKind::Index => &mut self.index,
            IndexKind::Fulltext => &mut self.fulltext,
            IndexKind::Spatial => &mut self.spatial_index,
        }
    }
}

/// The fluent handle every column method of a [`Blueprint`] returns: chain
/// modifiers, declare indexes on the column, or constrain it with a foreign
/// key.
///
/// ```
/// use navree_schema::{Blueprint, raw};
///
/// let mut t = Blueprint::new("deals");
/// t.string("title").length(120);
/// t.string("stage").default("lead").index();
/// t.timestamp_tz("closed_at").nullable().comment("set when won or lost");
/// t.foreign_uuid("company_id").constrained().cascade_on_delete();
/// t.decimal("amount", 12, 2).unsigned().default(0);
/// t.timestamp_tz("seen_at").default(raw("CURRENT_TIMESTAMP"));
/// assert_eq!(t.columns().len(), 6);
/// ```
pub struct ColumnBuilder<'a> {
    blueprint: &'a mut Blueprint,
    index: usize,
}

impl<'a> ColumnBuilder<'a> {
    pub(crate) fn new(blueprint: &'a mut Blueprint, index: usize) -> ColumnBuilder<'a> {
        ColumnBuilder { blueprint, index }
    }

    fn column(&mut self) -> &mut ColumnDefinition {
        &mut self.blueprint.columns[self.index]
    }

    fn problem(&mut self, modifier: &str) {
        let column = self.column();
        let message = format!(
            "`{modifier}` does not apply to {} column `{}`",
            column.kind.name(),
            column.name
        );
        column.problems.push(message);
    }

    /// The column definition as built so far.
    pub fn definition(&self) -> &ColumnDefinition {
        &self.blueprint.columns[self.index]
    }

    /// Allows `NULL`.
    pub fn nullable(mut self) -> Self {
        self.column().nullable = Some(true);
        self
    }

    /// Requires a value (`NOT NULL`), stated explicitly (the default for
    /// every column except generated ones).
    pub fn not_null(mut self) -> Self {
        self.column().nullable = Some(false);
        self
    }

    /// The default value: a literal (`0`, `"lead"`, `true`) or
    /// [`raw`](crate::raw) SQL.
    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        self.column().default = Some(value.into());
        self
    }

    /// Defaults to the current date or time (`CURRENT_TIMESTAMP`,
    /// `CURRENT_DATE`, ...).
    pub fn use_current(mut self) -> Self {
        if !self.column().kind.is_temporal() {
            self.problem("use_current");
        }
        self.column().use_current = true;
        self
    }

    /// Sets the current time on every update (MySQL `ON UPDATE
    /// CURRENT_TIMESTAMP`; other databases have no such clause and ignore
    /// it).
    pub fn use_current_on_update(mut self) -> Self {
        if !matches!(
            self.column().kind,
            ColumnType::DateTime { .. }
                | ColumnType::DateTimeTz { .. }
                | ColumnType::Timestamp { .. }
                | ColumnType::TimestampTz { .. }
        ) {
            self.problem("use_current_on_update");
        }
        self.column().use_current_on_update = true;
        self
    }

    /// A raw `ON UPDATE` expression (MySQL).
    pub fn on_update(mut self, expression: impl Into<Expression>) -> Self {
        self.column().on_update = Some(expression.into());
        self
    }

    /// Unsigned (MySQL; other databases have no unsigned integers).
    pub fn unsigned(mut self) -> Self {
        let kind = &self.column().kind;
        if !(kind.is_integer()
            || matches!(
                kind,
                ColumnType::Decimal { .. } | ColumnType::Float { .. } | ColumnType::Double
            ))
        {
            self.problem("unsigned");
        }
        self.column().unsigned = true;
        self
    }

    /// Auto-increments (an identity column on PostgreSQL).
    pub fn auto_increment(mut self) -> Self {
        if !self.column().kind.is_integer() {
            self.problem("auto_increment");
        }
        self.column().auto_increment = true;
        self
    }

    /// A comment on the column (PostgreSQL and MySQL; SQLite ignores it).
    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.column().comment = Some(comment.into());
        self
    }

    /// The collation, e.g. `C`, `und-x-icu` or `utf8mb4_bin`.
    pub fn collation(mut self, collation: impl Into<String>) -> Self {
        self.column().collation = Some(collation.into());
        self
    }

    /// The character set (MySQL), e.g. `utf8mb4`.
    pub fn charset(mut self, charset: impl Into<String>) -> Self {
        self.column().charset = Some(charset.into());
        self
    }

    /// Places the column after `column` (MySQL; ignored elsewhere).
    pub fn after(mut self, column: impl Into<String>) -> Self {
        self.column().after = Some(column.into());
        self
    }

    /// Places the column first (MySQL; ignored elsewhere).
    pub fn first(mut self) -> Self {
        self.column().first = true;
        self
    }

    /// A virtual generated column: computed from `expression` when read.
    pub fn virtual_as(mut self, expression: impl Into<Expression>) -> Self {
        self.column().virtual_as = Some(expression.into());
        self
    }

    /// A stored generated column: computed from `expression` on write.
    pub fn stored_as(mut self, expression: impl Into<Expression>) -> Self {
        self.column().stored_as = Some(expression.into());
        self
    }

    /// A PostgreSQL identity column, `GENERATED BY DEFAULT AS IDENTITY`
    /// (other databases: auto-increment).
    pub fn generated_as(mut self) -> Self {
        if !self.column().kind.is_integer() {
            self.problem("generated_as");
        }
        self.column().identity.get_or_insert_with(Identity::default);
        self
    }

    /// An identity column with sequence options:
    /// `generated_as_with("START WITH 1000 INCREMENT BY 10")`.
    pub fn generated_as_with(mut self, options: impl Into<String>) -> Self {
        if !self.column().kind.is_integer() {
            self.problem("generated_as_with");
        }
        self.column()
            .identity
            .get_or_insert_with(Identity::default)
            .options = Some(options.into());
        self
    }

    /// `GENERATED ALWAYS AS IDENTITY`: the database refuses explicit values.
    pub fn always(mut self) -> Self {
        self.column()
            .identity
            .get_or_insert_with(Identity::default)
            .always = true;
        self
    }

    /// The first value of an auto-incrementing column.
    pub fn starting_value(mut self, value: u64) -> Self {
        self.column().starting_value = Some(value);
        self
    }

    /// Same as [`starting_value`](Self::starting_value).
    pub fn from(self, value: u64) -> Self {
        self.starting_value(value)
    }

    /// Hides the column from `SELECT *` (MySQL 8.0.23+).
    pub fn invisible(mut self) -> Self {
        self.column().invisible = true;
        self
    }

    /// Alters the existing column to this definition instead of adding it
    /// (inside [`Schema::table`](crate::Schema::table)). The definition is
    /// complete: modifiers left out (a default, `nullable`) are removed.
    pub fn change(mut self) -> Self {
        self.column().change = true;
        self
    }

    /// The expression converting existing values when the type changes
    /// (PostgreSQL `USING`): `.using("amount::numeric(12, 2)")`.
    pub fn using(mut self, expression: impl Into<Expression>) -> Self {
        self.column().using = Some(expression.into());
        self
    }

    /// The maximum length of a `string`, `char` or `binary` column.
    pub fn length(mut self, length: u32) -> Self {
        match &mut self.column().kind {
            ColumnType::String { length: l } | ColumnType::Char { length: l } => *l = length,
            ColumnType::Binary { length: l, .. } => *l = Some(length),
            _ => self.problem("length"),
        }
        self
    }

    /// Fixed-length binary (`BINARY(n)` on MySQL).
    pub fn fixed(mut self) -> Self {
        match &mut self.column().kind {
            ColumnType::Binary { fixed, .. } => *fixed = true,
            _ => self.problem("fixed"),
        }
        self
    }

    /// The fractional-second digits of a time column (0 to 6), or the
    /// bits of a `float`.
    pub fn precision(mut self, digits: u8) -> Self {
        match &mut self.column().kind {
            ColumnType::DateTime { precision }
            | ColumnType::DateTimeTz { precision }
            | ColumnType::Time { precision }
            | ColumnType::TimeTz { precision }
            | ColumnType::Timestamp { precision }
            | ColumnType::TimestampTz { precision } => *precision = Some(digits),
            ColumnType::Float { precision } => *precision = u32::from(digits),
            _ => self.problem("precision"),
        }
        self
    }

    /// Leaves the fractional-second precision to the database's default.
    pub fn default_precision(mut self) -> Self {
        match &mut self.column().kind {
            ColumnType::DateTime { precision }
            | ColumnType::DateTimeTz { precision }
            | ColumnType::Time { precision }
            | ColumnType::TimeTz { precision }
            | ColumnType::Timestamp { precision }
            | ColumnType::TimestampTz { precision } => *precision = None,
            _ => self.problem("default_precision"),
        }
        self
    }

    /// The geometry subtype: `point`, `linestring`, `polygon`,
    /// `multipoint`, `multilinestring`, `multipolygon`,
    /// `geometrycollection`.
    pub fn subtype(mut self, subtype: impl Into<String>) -> Self {
        match &mut self.column().kind {
            ColumnType::Geometry { subtype: s, .. } | ColumnType::Geography { subtype: s, .. } => {
                *s = Some(subtype.into())
            }
            _ => self.problem("subtype"),
        }
        self
    }

    /// The spatial reference id of a geometry or geography column.
    pub fn srid(mut self, srid: u32) -> Self {
        match &mut self.column().kind {
            ColumnType::Geometry { srid: s, .. } => *s = Some(srid),
            ColumnType::Geography { srid: s, .. } => *s = srid,
            _ => self.problem("srid"),
        }
        self
    }

    fn fluent_index(mut self, kind: IndexKind, value: FluentIndex) -> Self {
        *self.column().fluent(kind) = Some(value);
        self
    }

    /// Makes this column the primary key.
    pub fn primary(self) -> Self {
        self.fluent_index(IndexKind::Primary, FluentIndex::Add(None))
    }

    /// Adds a unique index on this column (`{table}_{column}_unique`).
    pub fn unique(self) -> Self {
        self.fluent_index(IndexKind::Unique, FluentIndex::Add(None))
    }

    /// Adds a unique index named `name` on this column.
    pub fn unique_named(self, name: impl Into<String>) -> Self {
        self.fluent_index(IndexKind::Unique, FluentIndex::Add(Some(name.into())))
    }

    /// Adds an index on this column (`{table}_{column}_index`).
    pub fn index(self) -> Self {
        self.fluent_index(IndexKind::Index, FluentIndex::Add(None))
    }

    /// Adds an index named `name` on this column.
    pub fn index_named(self, name: impl Into<String>) -> Self {
        self.fluent_index(IndexKind::Index, FluentIndex::Add(Some(name.into())))
    }

    /// Adds a full-text index on this column (PostgreSQL and MySQL).
    pub fn fulltext(self) -> Self {
        self.fluent_index(IndexKind::Fulltext, FluentIndex::Add(None))
    }

    /// Adds a spatial index on this column (PostgreSQL and MySQL).
    pub fn spatial_index(self) -> Self {
        self.fluent_index(IndexKind::Spatial, FluentIndex::Add(None))
    }

    /// With [`change`](Self::change): drops the column's conventional
    /// primary key.
    pub fn without_primary(self) -> Self {
        self.fluent_index(IndexKind::Primary, FluentIndex::Drop)
    }

    /// With [`change`](Self::change): drops the column's conventional
    /// unique index.
    pub fn without_unique(self) -> Self {
        self.fluent_index(IndexKind::Unique, FluentIndex::Drop)
    }

    /// With [`change`](Self::change): drops the column's conventional
    /// index.
    pub fn without_index(self) -> Self {
        self.fluent_index(IndexKind::Index, FluentIndex::Drop)
    }

    /// Adds a foreign key on this column to the conventional table and
    /// column: `company_id` references `companies.id`.
    pub fn constrained(self) -> ForeignKeyBuilder<'a> {
        let name = self.blueprint.columns[self.index].name.clone();
        let table = table_for_foreign_key(&name, "id");
        self.constrained_on(table, "id")
    }

    /// Adds a foreign key on this column referencing `table.column`.
    pub fn constrained_on(
        self,
        table: impl Into<String>,
        column: impl Into<String>,
    ) -> ForeignKeyBuilder<'a> {
        self.references(column).on(table)
    }

    /// Adds a foreign key on this column referencing `column`; name the
    /// table with [`on`](ForeignKeyBuilder::on).
    pub fn references(self, column: impl Into<String>) -> ForeignKeyBuilder<'a> {
        let ColumnBuilder { blueprint, index } = self;
        let name = blueprint.columns[index].name.clone();
        blueprint.foreign(name).references(column.into())
    }
}
