//! SQL list filters: an [`Ability`]'s rules compiled into one `CASE` chain.
//!
//! The chain lists the relevant conditional rules from last to first, so the
//! first `WHEN` that holds is the rule [`Ability::can`] would pick:
//!
//! ```sql
//! CASE WHEN ("locked" = TRUE) THEN FALSE   -- cannot delete Post where locked
//!      WHEN ("author_id" = 7) THEN TRUE    -- can delete Post where author_id = 7
//!      ELSE FALSE END                      -- no rule: deny
//! ```
//!
//! Each condition compiles to SQL that is `TRUE` exactly when the record
//! check holds and `FALSE` or `NULL` otherwise (`CASE` treats `NULL` as not
//! matched), and the chain itself always yields `TRUE` or `FALSE`, so the
//! filter is safe to negate. Null handling follows JavaScript: `$eq: null` is
//! `IS NULL`, `$ne` is null-safe (`IS DISTINCT FROM`), and `$in` / `$nin`
//! treat a `null` member as matching null columns.
//!
//! The compiled filter is an [`SqlFilter`]: a small, backend-neutral
//! condition with its values bound. [`SqlFilter::to_statement`] writes it as
//! a portable fragment for one backend (`?` placeholders, identifiers quoted
//! for it), the shape `navree_db::record::Filter::raw` and
//! `navree_orm::Filter::from(Statement)` take; navree-orm lowers it into its
//! own render tree for model queries.
//!
//! Parity with [`Ability::can`] assumes each column holds the JSON type its
//! conditions use (see [`SqlColumns::accepts`]), and that strings compare in
//! binary (code point) order: SQLite's default collation, `COLLATE "C"` on
//! PostgreSQL, a binary collation on MySQL.

use navree_db::{Backend, Kind, Statement, Value as SqlValue};
use serde_json::Value;

use super::ability::Ability;
use super::condition::{Cond, Conditions};

/// A column a rule field maps to ([`SqlColumns::column`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlColumn {
    /// A column, qualified with its table (schema-qualified names, such as
    /// `audit.events`, are quoted part by part) or not.
    Column {
        /// The table, if the column is qualified.
        table: Option<String>,
        /// The column.
        name: String,
    },
    /// SQL `NULL`: the field is not a column, so a record check sees `null`.
    Null,
}

impl SqlColumn {
    /// An unqualified column.
    pub fn named(name: impl Into<String>) -> SqlColumn {
        SqlColumn::Column {
            table: None,
            name: name.into(),
        }
    }

    /// Column `name` of `table`.
    pub fn qualified(table: impl Into<String>, name: impl Into<String>) -> SqlColumn {
        SqlColumn::Column {
            table: Some(table.into()),
            name: name.into(),
        }
    }

    /// The column as `backend` writes it.
    fn write(&self, backend: Backend, sql: &mut String) {
        match self {
            SqlColumn::Column { table, name } => {
                if let Some(table) = table {
                    sql.push_str(&backend.quote_identifier(table));
                    sql.push('.');
                }
                sql.push_str(&backend.quote_part(name));
            }
            SqlColumn::Null => sql.push_str("NULL"),
        }
    }
}

/// Maps rule fields and condition operands onto SQL for
/// [`Ability::sql_filter_with`].
///
/// The defaults use an unqualified column named like the field and convert
/// operands with [`sql_value`]. Implement it to qualify columns with a table,
/// to map wire names to column names, or to bind typed values (a UUID or
/// timestamp column whose JSON form is a string); [`TypedColumns`] does all
/// three from a list of column types.
pub trait SqlColumns {
    /// The column for wire field `field`.
    fn column(&self, field: &str) -> SqlColumn {
        SqlColumn::named(field)
    }

    /// The SQL value for the non-null JSON operand `value` on `field`.
    fn value(&self, field: &str, value: &Value) -> SqlValue {
        let _ = field;
        sql_value(value)
    }

    /// Whether the non-null JSON operand `value` has the JSON type of
    /// `field`'s column (a number for a numeric column, a boolean for a
    /// boolean column, ...). The default accepts everything.
    ///
    /// A record check never matches across types (`false` is not `<= 0`),
    /// but SQL may coerce (SQLite stores booleans as integers) or fail
    /// (PostgreSQL), so a rejected operand compiles as the record check
    /// evaluates it: `$eq` and the range operators are `FALSE`, `$ne` is
    /// `TRUE`, and `$in` / `$nin` ignore it.
    fn accepts(&self, field: &str, value: &Value) -> bool {
        let _ = (field, value);
        true
    }

    /// The kind of `field`'s column, when it is known; `None` by default.
    ///
    /// For a numeric column it fixes the type of every numeric operand:
    /// a float column binds `F64`, an integer column `I64`, with a
    /// fractional operand written as the integer bound it amounts to
    /// (`n > 3.5` is `n > 3`, `n = 3.5` is `FALSE`). A statement then binds
    /// one type per placeholder whatever the JSON form of the rule's
    /// operands (`3` or `3.5`), so it is one prepared statement per
    /// connection (navree-db keys prepared statements by their values'
    /// types, so mixed types would be correct too, as more statements).
    fn kind(&self, field: &str) -> Option<Kind> {
        let _ = field;
        None
    }
}

/// [`SqlColumns`] with the default mapping.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultColumns;

impl SqlColumns for DefaultColumns {}

/// [`SqlColumns`] for a table whose columns' types are known: rule fields
/// are column names, qualified with the table, operands are bound with the
/// column's type (UUIDs, instants, date-times and dates parsed from their
/// JSON strings), and operands of another JSON type than the column's are
/// rejected ([`SqlColumns::accepts`]). A field that is not one of the
/// columns is SQL `NULL`, as a record check sees `null` for it.
///
/// ```
/// use navree_access::engine::{READ, Rules, TypedColumns, field};
/// use navree_db::{Backend, Kind};
///
/// let columns = TypedColumns::new("posts")
///     .with("id", Kind::Uuid)
///     .with("author_id", Kind::Uuid)
///     .with("published", Kind::Bool);
/// let mut rules = Rules::new();
/// rules.can(READ, "Post").when(field("published").eq(true));
/// let filter = rules.build().sql_filter_with(READ, "Post", &columns);
/// assert_eq!(
///     filter.to_statement(Backend::Postgres).to_string(),
///     r#"CASE WHEN ("posts"."published" = TRUE) THEN TRUE ELSE FALSE END"#
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedColumns {
    table: Option<String>,
    columns: Vec<(String, Kind)>,
}

impl TypedColumns {
    /// The columns of `table` (add them with [`with`](TypedColumns::with)).
    pub fn new(table: impl Into<String>) -> TypedColumns {
        TypedColumns {
            table: Some(table.into()),
            columns: Vec::new(),
        }
    }

    /// Columns written without their table.
    pub fn unqualified() -> TypedColumns {
        TypedColumns {
            table: None,
            columns: Vec::new(),
        }
    }

    /// Adds column `name`, whose values are of `kind`.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, kind: Kind) -> TypedColumns {
        self.columns.push((name.into(), kind));
        self
    }
}

impl SqlColumns for TypedColumns {
    fn kind(&self, field: &str) -> Option<Kind> {
        self.columns
            .iter()
            .find(|(column, _)| column == field)
            .map(|(_, kind)| *kind)
    }

    fn column(&self, field: &str) -> SqlColumn {
        match (self.kind(field), &self.table) {
            (None, _) => SqlColumn::Null,
            (Some(_), Some(table)) => SqlColumn::qualified(table.clone(), field),
            (Some(_), None) => SqlColumn::named(field),
        }
    }

    fn value(&self, field: &str, value: &Value) -> SqlValue {
        self.kind(field)
            .and_then(|kind| typed_value(kind, value))
            .unwrap_or_else(|| sql_value(value))
    }

    fn accepts(&self, field: &str, value: &Value) -> bool {
        // Not a column: SQL `NULL`, which matches nothing either way.
        self.kind(field)
            .is_none_or(|kind| accepts_kind(kind, value))
    }
}

/// The value of JSON operand `value` for a column of `kind`, when its JSON
/// form names one: a UUID, an RFC 3339 instant or date-time, a date.
/// `None` leaves it to [`sql_value`].
pub fn typed_value(kind: Kind, value: &Value) -> Option<SqlValue> {
    let text = value.as_str()?;
    match kind {
        Kind::Uuid => uuid::Uuid::parse_str(text).ok().map(SqlValue::from),
        Kind::Timestamp | Kind::TimestampTz => chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|at| SqlValue::from(at.with_timezone(&chrono::Utc))),
        Kind::DateTime => chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|at| SqlValue::from(at.with_timezone(&chrono::Utc).naive_utc())),
        Kind::Date => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .map(SqlValue::from),
        _ => None,
    }
}

/// Whether a non-null JSON operand has the JSON type of a column of `kind`:
/// booleans for booleans, numbers for integers and floats, strings for
/// text, UUIDs and times. Decimals (numbers or strings), JSON, bytes and
/// the rest compare as SQL compares them.
pub fn accepts_kind(kind: Kind, value: &Value) -> bool {
    match kind {
        Kind::Bool => value.is_boolean(),
        Kind::I8
        | Kind::I16
        | Kind::I32
        | Kind::I64
        | Kind::U8
        | Kind::U16
        | Kind::U32
        | Kind::U64
        | Kind::F32
        | Kind::F64 => value.is_number(),
        Kind::String
        | Kind::Uuid
        | Kind::Date
        | Kind::Time
        | Kind::DateTime
        | Kind::Timestamp
        | Kind::TimestampTz
        | Kind::Enum => value.is_string(),
        _ => true,
    }
}

/// The default SQL value for a JSON scalar: booleans as `Bool`, integers as
/// `I64`, other numbers as `F64` and strings as `String`. (`null`, arrays
/// and objects never reach SQL as operands; they map to a null string and
/// the JSON text respectively.)
pub fn sql_value(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null(Kind::String),
        Value::Bool(b) => SqlValue::from(*b),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => SqlValue::from(i),
            (None, Some(f)) => SqlValue::from(f),
            (None, None) => SqlValue::from(n.to_string()),
        },
        Value::String(s) => SqlValue::from(s.as_str()),
        other => SqlValue::from(other.to_string()),
    }
}

/// A stored value as a condition operand spells it, so a record check sees
/// what the list filter compares: UUIDs as hyphenated lower-case strings,
/// instants and date-times as RFC 3339 UTC strings with microseconds
/// (`2026-10-25T12:00:00.250000Z`, the form
/// [`IntoCondValue`](super::IntoCondValue) gives operands), dates as
/// `YYYY-MM-DD`, times as `HH:MM:SS[.f]`, decimals as numbers, bytes as
/// `null` (they have no condition form), anything else as its JSON.
pub fn value_json(value: &SqlValue) -> Value {
    use chrono::{SecondsFormat, Utc};
    let micros =
        |at: chrono::DateTime<Utc>| Value::String(at.to_rfc3339_opts(SecondsFormat::Micros, true));
    match value {
        SqlValue::Null(_) => Value::Null,
        SqlValue::Uuid(uuid) => Value::String(uuid.hyphenated().to_string()),
        SqlValue::Timestamp(at) => micros(*at),
        SqlValue::TimestampTz(at) => micros(at.with_timezone(&Utc)),
        SqlValue::DateTime(at) => micros(at.and_utc()),
        SqlValue::Date(date) => Value::String(date.format("%Y-%m-%d").to_string()),
        SqlValue::Time(time) => Value::String(time.format("%H:%M:%S%.f").to_string()),
        SqlValue::Bytes(_) => Value::Null,
        SqlValue::Decimal(decimal) => decimal
            .to_string()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        other => other.to_json(),
    }
}

/// A comparison of [`SqlFilter::Compare`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlOp {
    /// `=`
    Eq,
    /// Null-safe inequality: `IS DISTINCT FROM` on PostgreSQL, `IS NOT` on
    /// SQLite, `NOT (a <=> b)` on MySQL.
    DistinctFrom,
    /// `<`
    Lt,
    /// `<=`
    Lte,
    /// `>`
    Gt,
    /// `>=`
    Gte,
}

/// A compiled list filter: a backend-neutral boolean condition whose values
/// are bound, built by [`Ability::sql_filter`] /
/// [`Ability::sql_filter_with`]. [`SqlFilter::to_statement`] writes it for
/// a backend.
#[derive(Clone, Debug, PartialEq)]
pub enum SqlFilter {
    /// `TRUE` or `FALSE`.
    Bool(bool),
    /// `CASE WHEN c THEN r .. ELSE otherwise END`, the first `WHEN` that
    /// holds deciding.
    Case {
        /// The conditions with their results, in order.
        whens: Vec<(SqlFilter, bool)>,
        /// The result when no condition holds.
        otherwise: bool,
    },
    /// Every condition holds.
    And(Vec<SqlFilter>),
    /// One condition holds.
    Or(Vec<SqlFilter>),
    /// `column IS NULL`.
    IsNull(SqlColumn),
    /// `column IS NOT NULL`.
    IsNotNull(SqlColumn),
    /// `column <op> value`.
    Compare(SqlColumn, SqlOp, SqlValue),
    /// `column IN (values)` (never empty).
    In(SqlColumn, Vec<SqlValue>),
    /// `column NOT IN (values)` (never empty).
    NotIn(SqlColumn, Vec<SqlValue>),
}

impl SqlFilter {
    /// The filter as a portable fragment for `backend`: SQL with `?`
    /// placeholders and its values, identifiers quoted for `backend`, the
    /// null-safe inequality spelled as `backend` spells it. Embed it with
    /// `navree_db::record::Filter::raw` or `navree_orm::Filter::from`, or
    /// run it inside a statement of your own.
    ///
    /// ```
    /// use navree_access::engine::{DELETE, Rules, field};
    /// use navree_db::Backend;
    ///
    /// let mut rules = Rules::new();
    /// rules.can(DELETE, "Post").when(field("author_id").eq(7));
    /// rules.cannot(DELETE, "Post").when(field("status").ne("draft"));
    /// let filter = rules.build().sql_filter(DELETE, "Post");
    ///
    /// let statement = filter.to_statement(Backend::MySql);
    /// assert_eq!(
    ///     statement.sql(),
    ///     "CASE WHEN (NOT (`status` <=> ?)) THEN FALSE WHEN (`author_id` = ?) THEN TRUE ELSE FALSE END"
    /// );
    /// assert_eq!(statement.values(), ["draft".into(), 7_i64.into()]);
    /// ```
    pub fn to_statement(&self, backend: Backend) -> Statement {
        let mut sql = String::new();
        let mut values = Vec::new();
        self.write(backend, &mut sql, &mut values);
        Statement::portable(sql, values)
    }

    fn write(&self, backend: Backend, sql: &mut String, values: &mut Vec<SqlValue>) {
        match self {
            SqlFilter::Bool(true) => sql.push_str("TRUE"),
            SqlFilter::Bool(false) => sql.push_str("FALSE"),
            SqlFilter::Case { whens, otherwise } => {
                sql.push_str("CASE");
                for (condition, result) in whens {
                    sql.push_str(" WHEN (");
                    condition.write(backend, sql, values);
                    sql.push_str(if *result {
                        ") THEN TRUE"
                    } else {
                        ") THEN FALSE"
                    });
                }
                sql.push_str(if *otherwise {
                    " ELSE TRUE END"
                } else {
                    " ELSE FALSE END"
                });
            }
            SqlFilter::And(parts) | SqlFilter::Or(parts) => {
                let joiner = if matches!(self, SqlFilter::And(_)) {
                    " AND "
                } else {
                    " OR "
                };
                for (at, part) in parts.iter().enumerate() {
                    if at > 0 {
                        sql.push_str(joiner);
                    }
                    let nested = matches!(
                        part,
                        SqlFilter::And(_) | SqlFilter::Or(_) | SqlFilter::Case { .. }
                    );
                    if nested {
                        sql.push('(');
                    }
                    part.write(backend, sql, values);
                    if nested {
                        sql.push(')');
                    }
                }
            }
            SqlFilter::IsNull(column) => {
                column.write(backend, sql);
                sql.push_str(" IS NULL");
            }
            SqlFilter::IsNotNull(column) => {
                column.write(backend, sql);
                sql.push_str(" IS NOT NULL");
            }
            SqlFilter::Compare(column, SqlOp::DistinctFrom, value) => {
                if backend == Backend::MySql {
                    sql.push_str("NOT (");
                    column.write(backend, sql);
                    sql.push_str(" <=> ?)");
                } else {
                    column.write(backend, sql);
                    sql.push_str(if backend == Backend::Postgres {
                        " IS DISTINCT FROM ?"
                    } else {
                        " IS NOT ?"
                    });
                }
                values.push(value.clone());
            }
            SqlFilter::Compare(column, op, value) => {
                column.write(backend, sql);
                sql.push_str(match op {
                    SqlOp::Eq => " = ?",
                    SqlOp::Lt => " < ?",
                    SqlOp::Lte => " <= ?",
                    SqlOp::Gt => " > ?",
                    SqlOp::Gte => " >= ?",
                    SqlOp::DistinctFrom => unreachable!("written above"),
                });
                values.push(value.clone());
            }
            SqlFilter::In(column, list) | SqlFilter::NotIn(column, list) => {
                column.write(backend, sql);
                sql.push_str(if matches!(self, SqlFilter::In(..)) {
                    " IN ("
                } else {
                    " NOT IN ("
                });
                sql.push_str(&vec!["?"; list.len()].join(", "));
                sql.push(')');
                values.extend(list.iter().cloned());
            }
        }
    }
}

impl Ability {
    /// A boolean SQL condition that is true exactly for the rows of
    /// `subject_type` on which `action` is allowed, i.e. for which
    /// [`Ability::can`] with the row as record returns true; `FALSE` when no
    /// rule applies. Uses the default column mapping ([`DefaultColumns`]).
    pub fn sql_filter(&self, action: &str, subject_type: &str) -> SqlFilter {
        self.sql_filter_with(action, subject_type, &DefaultColumns)
    }

    /// [`Ability::sql_filter`] with a custom column and value mapping.
    pub fn sql_filter_with<C>(&self, action: &str, subject_type: &str, columns: &C) -> SqlFilter
    where
        C: SqlColumns + ?Sized,
    {
        // Same rule selection as `Ability::can` for a whole record: field-only
        // `cannot` rules do not hide records.
        let relevant = self.rules().iter().rev().filter(|rule| {
            rule.matches_action(action)
                && rule.matches_subject(subject_type)
                && rule.matches_field(None)
        });

        let mut whens: Vec<(&Conditions, bool)> = Vec::new();
        let mut otherwise = false;
        for rule in relevant {
            match &rule.conditions {
                Some(conditions) if !conditions.is_empty() => {
                    whens.push((conditions, !rule.inverted));
                }
                // Matches every record: decides for all rows not caught above.
                _ => {
                    otherwise = !rule.inverted;
                    break;
                }
            }
        }
        // A trailing WHEN with the ELSE result changes nothing.
        while whens.last().is_some_and(|(_, allow)| *allow == otherwise) {
            whens.pop();
        }
        if whens.is_empty() {
            return SqlFilter::Bool(otherwise);
        }
        SqlFilter::Case {
            whens: whens
                .into_iter()
                .map(|(conditions, allow)| (conditions_sql(conditions, columns), allow))
                .collect(),
            otherwise,
        }
    }
}

/// Every condition of a rule, joined with `AND` (a single one as itself).
fn conditions_sql<C>(conditions: &Conditions, columns: &C) -> SqlFilter
where
    C: SqlColumns + ?Sized,
{
    let mut all: Vec<SqlFilter> = conditions
        .iter()
        .flat_map(|(field, conds)| conds.iter().map(move |cond| (field, cond)))
        .map(|(field, cond)| cond_sql(field, cond, columns))
        .collect();
    if all.len() == 1 {
        all.remove(0)
    } else {
        SqlFilter::And(all)
    }
}

fn cond_sql<C>(field: &str, cond: &Cond, columns: &C) -> SqlFilter
where
    C: SqlColumns + ?Sized,
{
    let column = columns.column(field);
    let operand = |v: &Value| operand(field, v, columns);
    // A non-null operand of another type than the column never compares
    // equal or ordered (see `SqlColumns::accepts`).
    let foreign = |v: &Value| !v.is_null() && !columns.accepts(field, v);
    let compare = |op: SqlOp, value: SqlValue| SqlFilter::Compare(column.clone(), op, value);
    // A range test against a number beyond every integer the column holds:
    // every non-null row, or none.
    let every_row = || SqlFilter::IsNotNull(column.clone());
    match cond {
        Cond::Eq(Value::Null) => SqlFilter::IsNull(column),
        Cond::Eq(v) if foreign(v) => SqlFilter::Bool(false),
        Cond::Eq(v) => match operand(v) {
            Operand::Value(value) => compare(SqlOp::Eq, value),
            Operand::Between(_) | Operand::Above | Operand::Below | Operand::Never => {
                SqlFilter::Bool(false)
            }
        },
        Cond::Ne(Value::Null) => SqlFilter::IsNotNull(column),
        Cond::Ne(v) if foreign(v) => SqlFilter::Bool(true),
        Cond::Ne(v) => match operand(v) {
            Operand::Value(value) => compare(SqlOp::DistinctFrom, value),
            Operand::Between(_) | Operand::Above | Operand::Below | Operand::Never => {
                SqlFilter::Bool(true)
            }
        },
        Cond::Lt(v) | Cond::Lte(v) | Cond::Gt(v) | Cond::Gte(v) if foreign(v) => {
            SqlFilter::Bool(false)
        }
        Cond::Lt(v) | Cond::Lte(v) => match (operand(v), cond) {
            (Operand::Value(value), Cond::Lt(_)) => compare(SqlOp::Lt, value),
            (Operand::Value(value), _) => compare(SqlOp::Lte, value),
            // `n < 3.5` and `n <= 3.5` are `n <= 3` for an integer `n`.
            (Operand::Between(floor), _) => compare(SqlOp::Lte, SqlValue::I64(floor)),
            (Operand::Above, _) => every_row(),
            (Operand::Below | Operand::Never, _) => SqlFilter::Bool(false),
        },
        Cond::Gt(v) | Cond::Gte(v) => match (operand(v), cond) {
            (Operand::Value(value), Cond::Gt(_)) => compare(SqlOp::Gt, value),
            (Operand::Value(value), _) => compare(SqlOp::Gte, value),
            // `n > 3.5` and `n >= 3.5` are `n > 3` for an integer `n`.
            (Operand::Between(floor), _) => compare(SqlOp::Gt, SqlValue::I64(floor)),
            (Operand::Above | Operand::Never, _) => SqlFilter::Bool(false),
            (Operand::Below, _) => every_row(),
        },
        Cond::In(items) => {
            let (has_null, listed) = split_null(items, &foreign);
            let listed = bound(listed, operand);
            let listed = (!listed.is_empty()).then(|| SqlFilter::In(column.clone(), listed));
            match (listed, has_null) {
                (Some(listed), true) => SqlFilter::Or(vec![listed, SqlFilter::IsNull(column)]),
                (Some(listed), false) => listed,
                (None, true) => SqlFilter::IsNull(column),
                (None, false) => SqlFilter::Bool(false),
            }
        }
        Cond::Nin(items) => {
            let (has_null, listed) = split_null(items, &foreign);
            let listed = bound(listed, operand);
            let unlisted = (!listed.is_empty()).then(|| SqlFilter::NotIn(column.clone(), listed));
            match (unlisted, has_null) {
                (Some(unlisted), true) => {
                    SqlFilter::And(vec![SqlFilter::IsNotNull(column), unlisted])
                }
                (Some(unlisted), false) => SqlFilter::Or(vec![SqlFilter::IsNull(column), unlisted]),
                (None, true) => SqlFilter::IsNotNull(column),
                (None, false) => SqlFilter::Bool(true),
            }
        }
    }
}

/// A non-null operand the column accepts, as SQL compares it.
enum Operand {
    /// The value to bind.
    Value(SqlValue),
    /// A fractional number on an integer column, between `floor` and
    /// `floor + 1`: no row equals it.
    Between(i64),
    /// A number above every integer the column can hold.
    Above,
    /// A number below every integer the column can hold.
    Below,
    /// An operand for a field that is no column (SQL `NULL`, `null` to a
    /// record check): never equal, never ordered. Nothing is bound, so the
    /// operand's type cannot vary between runs of one statement.
    Never,
}

/// `value` as `field`'s column compares it (see [`SqlColumns::kind`]):
/// numbers on an integer column as `I64` (or where they fall between
/// integers), on a float column as `F64`; anything else through
/// [`SqlColumns::value`].
fn operand<C>(field: &str, value: &Value, columns: &C) -> Operand
where
    C: SqlColumns + ?Sized,
{
    // 2^63, the first float above `i64::MAX`.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if columns.column(field) == SqlColumn::Null {
        return Operand::Never;
    }
    match (columns.kind(field), value) {
        (Some(kind), Value::Number(n)) if is_integer(kind) => {
            if let Some(i) = n.as_i64() {
                return Operand::Value(SqlValue::I64(i));
            }
            if n.as_u64().is_some() {
                return Operand::Above;
            }
            let f = n.as_f64().unwrap_or_default();
            if f >= LIMIT {
                Operand::Above
            } else if f < -LIMIT {
                Operand::Below
            } else if f.fract() == 0.0 {
                // In range and whole: exact.
                #[allow(clippy::cast_possible_truncation)]
                Operand::Value(SqlValue::I64(f as i64))
            } else {
                #[allow(clippy::cast_possible_truncation)]
                Operand::Between(f.floor() as i64)
            }
        }
        (Some(Kind::F32 | Kind::F64), Value::Number(n)) => {
            Operand::Value(SqlValue::F64(n.as_f64().unwrap_or_default()))
        }
        _ => Operand::Value(columns.value(field, value)),
    }
}

fn is_integer(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::I8 | Kind::I16 | Kind::I32 | Kind::I64 | Kind::U8 | Kind::U16 | Kind::U32 | Kind::U64
    )
}

/// The values of `items` a column can equal.
fn bound(items: Vec<&Value>, operand: impl Fn(&Value) -> Operand) -> Vec<SqlValue> {
    items
        .into_iter()
        .filter_map(|item| match operand(item) {
            Operand::Value(value) => Some(value),
            Operand::Between(_) | Operand::Above | Operand::Below | Operand::Never => None,
        })
        .collect()
}

/// Whether `items` contains `null`, and its non-null members the column
/// accepts.
fn split_null<'a>(items: &'a [Value], foreign: &impl Fn(&Value) -> bool) -> (bool, Vec<&'a Value>) {
    let has_null = items.iter().any(Value::is_null);
    (
        has_null,
        items
            .iter()
            .filter(|v| !v.is_null() && !foreign(v))
            .collect(),
    )
}
