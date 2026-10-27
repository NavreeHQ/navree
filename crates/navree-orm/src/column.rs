//! Typed columns ([`Column`]), conditions ([`Filter`]) and assignments
//! ([`Assignment`]).

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use crate::ir::{Boolean, ColumnRef, Expr, InValues, Operator, Order, QueryIr, Where, WhereClause};
use crate::row::Row;
use crate::value::{Field, FieldError, Value, ValueFor, is_null};

/// Encodes a field value as a column value (with the field's cast, if any).
pub type Encoder<T> = fn(&T) -> Result<Value, String>;

/// Reads a column value as a field (with the field's cast, if any).
pub type Decoder<T> = fn(&Row, &str) -> Result<T, FieldError>;

/// The model type of columns that belong to no model: [`col`] and column
/// names given as strings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Untyped;

/// A column of model `M` holding values of type `T`.
///
/// `#[derive(Model)]` generates one constant per column, typed like its
/// field (`Deal::AMOUNT_CENTS: Column<Deal, i64>`, `Deal::CLOSED_AT:
/// Column<Deal, Option<DateTime<Utc>>>`). Conditions built from a column
/// take values of its type (a nullable column takes plain values too:
/// `Deal::CLOSED_AT.gt(now)`) and encode them exactly as the field is
/// stored (casts included), so `Deal::STAGE.eq(Stage::Won)` compares with
/// `'won'` when the stage is stored as its name.
///
/// Typed columns are qualified by their table in SQL, so they stay
/// unambiguous in joins and subqueries. [`col`] builds an untyped column
/// from a name (`"title"`, `"deals.title"`, a JSON path `"meta->status"`).
///
/// ```
/// use navree_orm::{Column, Dialect, Filter, Untyped, Value, col};
///
/// struct Deal;
/// const AMOUNT: Column<Deal, i64> = Column::field("deals", "amount_cents");
///
/// let filter: Filter = AMOUNT.gte(10_000).and(col("stage").eq("won"));
/// let sql = filter.to_sql(Dialect::Postgres).unwrap();
/// assert_eq!(sql.sql, r#""deals"."amount_cents" >= $1 AND "stage" = $2"#);
/// assert_eq!(sql.values, [Value::BigInt(Some(10_000)), Value::from("won")]);
/// ```
pub struct Column<M, T> {
    table: Option<Cow<'static, str>>,
    name: Cow<'static, str>,
    encode: Encoder<T>,
    decode: Decoder<T>,
    _model: PhantomData<fn() -> M>,
}

impl<M, T> Clone for Column<M, T> {
    fn clone(&self) -> Self {
        Column {
            table: self.table.clone(),
            name: self.name.clone(),
            encode: self.encode,
            decode: self.decode,
            _model: PhantomData,
        }
    }
}

impl<M, T> fmt::Debug for Column<M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Column")
            .field(&self.column_ref().to_string())
            .finish()
    }
}

fn encode_field<T: Field>(value: &T) -> Result<Value, String> {
    Ok(value.to_value())
}

fn decode_field<T: Field>(row: &Row, column: &str) -> Result<T, FieldError> {
    row.field(column)
}

impl<M, T: Field> Column<M, T> {
    /// The column `name` of `table`, holding a [`Field`] type without a
    /// cast. (`#[derive(Model)]` writes these.)
    pub const fn field(table: &'static str, name: &'static str) -> Self {
        Column {
            table: Some(Cow::Borrowed(table)),
            name: Cow::Borrowed(name),
            encode: encode_field::<T>,
            decode: decode_field::<T>,
            _model: PhantomData,
        }
    }
}

impl<M, T> Column<M, T> {
    /// The column `name` of `table` with its own encoder and decoder (a
    /// cast's). (`#[derive(Model)]` writes these.)
    pub const fn with_codec(
        table: &'static str,
        name: &'static str,
        encode: Encoder<T>,
        decode: Decoder<T>,
    ) -> Self {
        Column {
            table: Some(Cow::Borrowed(table)),
            name: Cow::Borrowed(name),
            encode,
            decode,
            _model: PhantomData,
        }
    }

    /// The column's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The table qualifying the column, if any.
    pub fn table(&self) -> Option<&str> {
        self.table.as_deref()
    }

    /// The column as an IR reference.
    pub fn column_ref(&self) -> ColumnRef {
        let mut column = ColumnRef::parse(self.name.clone());
        if column.table.is_none() {
            column.table = self.table.clone();
        }
        column
    }

    /// The column as an expression.
    pub fn expr(&self) -> Expr {
        Expr::Column(self.column_ref())
    }

    /// The same column without its table (where an unqualified name is
    /// needed, e.g. an alias over a subquery).
    pub fn unqualified(&self) -> Self {
        Column {
            table: None,
            ..self.clone()
        }
    }

    /// Encodes `value` as this column stores it.
    pub fn encode(&self, value: &T) -> Result<Value, String> {
        (self.encode)(value)
    }

    /// Reads this column from `row`.
    pub fn decode(&self, row: &Row) -> Result<T, FieldError> {
        (self.decode)(row, ColumnRef::parse(self.name.clone()).name.as_ref())
    }

    /// Reads result column `column` of `row` as this column's type (for a
    /// value selected under another name).
    pub fn decode_as(&self, row: &Row, column: &str) -> Result<T, FieldError> {
        (self.decode)(row, column)
    }

    /// A path into this JSON column: `Deal::META.json("status")` is
    /// `deals.meta->status`.
    pub fn json(&self, path: &str) -> Column<Untyped, Value> {
        Column {
            table: self.table.clone(),
            name: Cow::Owned(format!("{}->{path}", self.name)),
            encode: encode_field::<Value>,
            decode: decode_field::<Value>,
            _model: PhantomData,
        }
    }

    fn value_operand(&self, value: &T) -> Result<Expr, String> {
        (self.encode)(value)
            .map(Expr::Value)
            .map_err(|error| format!("cannot use a value for column `{}`: {error}", self.name))
    }

    fn compare(&self, op: Operator, value: T) -> Filter {
        match self.value_operand(&value) {
            Ok(right) => Filter::compare(self.expr(), op, right),
            Err(error) => Filter(Where::Invalid(error)),
        }
    }

    /// `column = value` (`IS NULL` for a NULL value).
    pub fn eq(&self, value: impl ValueFor<T>) -> Filter {
        self.compare(Operator::Eq, value.into_value_for())
    }

    /// `column <> value` (`IS NOT NULL` for a NULL value).
    pub fn ne(&self, value: impl ValueFor<T>) -> Filter {
        self.compare(Operator::Ne, value.into_value_for())
    }

    /// `column > value`.
    pub fn gt(&self, value: impl ValueFor<T>) -> Filter {
        self.compare(Operator::Gt, value.into_value_for())
    }

    /// `column >= value`.
    pub fn gte(&self, value: impl ValueFor<T>) -> Filter {
        self.compare(Operator::Gte, value.into_value_for())
    }

    /// `column < value`.
    pub fn lt(&self, value: impl ValueFor<T>) -> Filter {
        self.compare(Operator::Lt, value.into_value_for())
    }

    /// `column <= value`.
    pub fn lte(&self, value: impl ValueFor<T>) -> Filter {
        self.compare(Operator::Lte, value.into_value_for())
    }

    /// `column <op> value` for any [`Operator`] (`">="`, `"<>"`, `"like"`,
    /// PostgreSQL's `"@>"`, ...).
    pub fn op(&self, op: impl Into<Operator>, value: impl ValueFor<T>) -> Filter {
        self.compare(op.into(), value.into_value_for())
    }

    /// Null-safe equality: true when both sides are NULL.
    pub fn null_safe_eq(&self, value: impl ValueFor<T>) -> Filter {
        match self.value_operand(&value.into_value_for()) {
            Ok(right) => Filter(Where::Compare {
                left: self.expr(),
                op: Operator::NullSafeEq,
                right,
            }),
            Err(error) => Filter(Where::Invalid(error)),
        }
    }

    fn list(&self, values: impl IntoIterator<Item = impl ValueFor<T>>, not: bool) -> Filter {
        let encoded: Result<Vec<Expr>, String> = values
            .into_iter()
            .map(|value| self.value_operand(&value.into_value_for()))
            .collect();
        match encoded {
            Ok(values) => Filter(Where::In {
                expr: self.expr(),
                values: InValues::List(values),
                not,
            }),
            Err(error) => Filter(Where::Invalid(error)),
        }
    }

    /// `column IN (values)`; an empty list matches no row.
    pub fn is_in(&self, values: impl IntoIterator<Item = impl ValueFor<T>>) -> Filter {
        self.list(values, false)
    }

    /// `column NOT IN (values)`; an empty list matches every row.
    pub fn not_in(&self, values: impl IntoIterator<Item = impl ValueFor<T>>) -> Filter {
        self.list(values, true)
    }

    /// `column IN (subquery)`.
    pub fn in_subquery(&self, query: impl IntoQuery) -> Filter {
        Filter(Where::In {
            expr: self.expr(),
            values: InValues::Subquery(Box::new(query.into_query())),
            not: false,
        })
    }

    /// `column NOT IN (subquery)`.
    pub fn not_in_subquery(&self, query: impl IntoQuery) -> Filter {
        Filter(Where::In {
            expr: self.expr(),
            values: InValues::Subquery(Box::new(query.into_query())),
            not: true,
        })
    }

    /// `column IS NULL`.
    pub fn is_null(&self) -> Filter {
        Filter(Where::Null {
            expr: self.expr(),
            not: false,
        })
    }

    /// `column IS NOT NULL`.
    pub fn is_not_null(&self) -> Filter {
        Filter(Where::Null {
            expr: self.expr(),
            not: true,
        })
    }

    fn range(&self, low: T, high: T, not: bool) -> Filter {
        match (self.value_operand(&low), self.value_operand(&high)) {
            (Ok(low), Ok(high)) => Filter(Where::Between {
                expr: self.expr(),
                low,
                high,
                not,
            }),
            (Err(error), _) | (_, Err(error)) => Filter(Where::Invalid(error)),
        }
    }

    /// `column BETWEEN low AND high` (both ends included).
    pub fn between(&self, low: impl ValueFor<T>, high: impl ValueFor<T>) -> Filter {
        self.range(low.into_value_for(), high.into_value_for(), false)
    }

    /// `column NOT BETWEEN low AND high`.
    pub fn not_between(&self, low: impl ValueFor<T>, high: impl ValueFor<T>) -> Filter {
        self.range(low.into_value_for(), high.into_value_for(), true)
    }

    /// `column = other` (another column).
    pub fn eq_column<N, U>(&self, other: &Column<N, U>) -> Filter {
        Filter::compare(self.expr(), Operator::Eq, other.expr())
    }

    /// `column <op> other` (another column).
    pub fn op_column<N, U>(&self, op: impl Into<Operator>, other: &Column<N, U>) -> Filter {
        Filter::compare(self.expr(), op.into(), other.expr())
    }

    /// The column in ascending order.
    pub fn asc(&self) -> Order {
        Order::asc(self.expr())
    }

    /// The column in descending order.
    pub fn desc(&self) -> Order {
        Order::desc(self.expr())
    }

    /// `SET column = value` (mass updates, `first_or_create` attributes).
    pub fn set(&self, value: impl ValueFor<T>) -> Assignment {
        match (self.encode)(&value.into_value_for()) {
            Ok(value) => Assignment::value(self.name.clone(), value),
            Err(error) => Assignment::invalid(
                self.name.clone(),
                format!("cannot use a value for column `{}`: {error}", self.name),
            ),
        }
    }

    /// `SET column = expr` (mass updates only).
    pub fn set_expr(&self, expr: impl Into<Expr>) -> Assignment {
        Assignment {
            column: self.name.clone(),
            value: AssignedValue::Expr(expr.into()),
        }
    }

    /// `SET column = NULL`.
    pub fn set_null(&self) -> Assignment
    where
        T: Field,
    {
        Assignment::value(self.name.clone(), T::null())
    }

    /// `count(column)`.
    pub fn count(&self) -> Expr {
        Expr::count(self.expr())
    }

    /// `sum(column)`.
    pub fn sum(&self) -> Expr {
        Expr::sum(self.expr())
    }

    /// `avg(column)`.
    pub fn avg(&self) -> Expr {
        Expr::avg(self.expr())
    }

    /// `min(column)`.
    pub fn min(&self) -> Expr {
        Expr::min(self.expr())
    }

    /// `max(column)`.
    pub fn max(&self) -> Expr {
        Expr::max(self.expr())
    }
}

impl<M> Column<M, String> {
    /// `column LIKE pattern`, ignoring case on every database (`%` and `_`
    /// are wildcards).
    pub fn like(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, false)
    }

    /// `column NOT LIKE pattern`, ignoring case on every database.
    pub fn not_like(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, true)
    }

    /// `column LIKE pattern`, matching case on every database.
    pub fn like_case_sensitive(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), true, false)
    }
}

impl<M> Column<M, Option<String>> {
    /// `column LIKE pattern`, ignoring case on every database (NULLs never
    /// match).
    pub fn like(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, false)
    }

    /// `column NOT LIKE pattern`, ignoring case on every database.
    pub fn not_like(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, true)
    }

    /// `column LIKE pattern`, matching case on every database.
    pub fn like_case_sensitive(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), true, false)
    }
}

impl<M> Column<M, Value> {
    /// `column LIKE pattern`, ignoring case on every database.
    pub fn like(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, false)
    }

    /// `column NOT LIKE pattern`, ignoring case on every database.
    pub fn not_like(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), false, true)
    }

    /// `column LIKE pattern`, matching case on every database.
    pub fn like_case_sensitive(&self, pattern: impl Into<String>) -> Filter {
        Filter::like(self.expr(), pattern.into(), true, false)
    }
}

/// An untyped column: `"title"`, `"deals.title"`, or a JSON path
/// `"meta->status"`. Its conditions take any value.
///
/// ```
/// use navree_orm::{Dialect, col};
///
/// let sql = col("meta->status").eq("open").to_sql(Dialect::Sqlite).unwrap();
/// assert_eq!(sql.sql, r#"json_extract("meta", '$."status"') = ?"#);
/// ```
pub fn col(name: impl Into<Cow<'static, str>>) -> Column<Untyped, Value> {
    let column = ColumnRef::parse(name);
    let name = match column.path.is_empty() {
        true => column.name.clone(),
        false => Cow::Owned(column.without_path().name.to_string() + &json_suffix(&column)),
    };
    Column {
        table: column.table,
        name,
        encode: encode_field::<Value>,
        decode: decode_field::<Value>,
        _model: PhantomData,
    }
}

/// An untyped column whose values are read as `T`: for plucks and single
/// values of tables without a model.
///
/// ```ignore
/// let titles: Vec<String> = table("deals").pluck(&db, col_as::<String>("title")).await?;
/// ```
pub fn col_as<T: Field>(name: impl Into<Cow<'static, str>>) -> Column<Untyped, T> {
    let untyped = col(name);
    Column {
        table: untyped.table,
        name: untyped.name,
        encode: encode_field::<T>,
        decode: decode_field::<T>,
        _model: PhantomData,
    }
}

fn json_suffix(column: &ColumnRef) -> String {
    let text = column.to_string();
    match text.find("->") {
        Some(at) => text[at..].to_owned(),
        None => String::new(),
    }
}

/// A condition for `where_`, `having` and join clauses.
///
/// Built from columns (`Deal::STAGE.eq(Stage::Won)`, `col("x").gt(5)`),
/// expressions (`Expr::count_all().gt(3)`), raw SQL ([`Filter::raw`]), or a
/// sea-query condition (navree-access SQL filters, navree-db's
/// `contains_ignore_case`), and combined with [`and`](Filter::and),
/// [`or`](Filter::or) and [`not`](Filter::not).
#[derive(Clone, Debug)]
pub struct Filter(pub(crate) Where);

impl Filter {
    /// `left op right`, turning a comparison with NULL into `IS [NOT] NULL`.
    pub(crate) fn compare(left: Expr, op: Operator, right: Expr) -> Filter {
        if let Expr::Value(value) = &right
            && is_null(value)
        {
            return match op {
                Operator::Eq => Filter(Where::Null {
                    expr: left,
                    not: false,
                }),
                Operator::Ne => Filter(Where::Null {
                    expr: left,
                    not: true,
                }),
                Operator::NullSafeEq => Filter(Where::Compare { left, op, right }),
                other => Filter(Where::Invalid(format!(
                    "a NULL value cannot be compared with `{other:?}`; use is_null() or is_not_null()"
                ))),
            };
        }
        Filter(Where::Compare { left, op, right })
    }

    pub(crate) fn like(expr: Expr, pattern: String, case_sensitive: bool, not: bool) -> Filter {
        Filter(Where::Like {
            expr,
            pattern: Expr::Value(Value::from(pattern)),
            case_sensitive,
            not,
        })
    }

    /// Raw SQL with `?` placeholders for `values` (`??` for a literal `?`).
    /// SQL databases only. Beside other conditions (and under `NOT`) the
    /// fragment is parenthesized, so an `OR` inside it stays its own.
    ///
    /// ```
    /// use navree_orm::{Dialect, Filter};
    ///
    /// let sql = Filter::raw("lower(title) = ?", ["renewal"]).to_sql(Dialect::Postgres).unwrap();
    /// assert_eq!(sql.sql, "lower(title) = $1");
    ///
    /// let either = Filter::raw("stage = ? OR stage = ?", ["won", "lost"]).not();
    /// assert_eq!(either.to_sql(Dialect::Postgres).unwrap().sql, "NOT (stage = $1 OR stage = $2)");
    /// ```
    pub fn raw(
        sql: impl Into<Cow<'static, str>>,
        values: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Filter {
        Filter(Where::Raw {
            sql: sql.into(),
            values: values.into_iter().map(Into::into).collect(),
        })
    }

    /// `EXISTS (query)`.
    pub fn exists(query: impl IntoQuery) -> Filter {
        Filter(Where::Exists {
            query: Box::new(query.into_query()),
            not: false,
        })
    }

    /// `NOT EXISTS (query)`.
    pub fn not_exists(query: impl IntoQuery) -> Filter {
        Filter(Where::Exists {
            query: Box::new(query.into_query()),
            not: true,
        })
    }

    /// Both conditions.
    pub fn and(self, other: impl Into<Filter>) -> Filter {
        Filter(Where::Group(vec![
            WhereClause::and(self.0),
            WhereClause::and(other.into().0),
        ]))
    }

    /// Either condition.
    pub fn or(self, other: impl Into<Filter>) -> Filter {
        Filter(Where::Group(vec![
            WhereClause::and(self.0),
            WhereClause::or(other.into().0),
        ]))
    }

    /// The opposite condition: `NOT (...)`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Filter {
        Filter(Where::Group(vec![WhereClause {
            boolean: Boolean::And,
            negated: true,
            filter: self.0,
        }]))
    }

    /// Every condition (`TRUE` when there are none).
    pub fn all(filters: impl IntoIterator<Item = impl Into<Filter>>) -> Filter {
        Filter(Where::Group(
            filters
                .into_iter()
                .map(|filter| WhereClause::and(filter.into().0))
                .collect(),
        ))
    }

    /// Any of the conditions (`FALSE` when there are none).
    pub fn any(filters: impl IntoIterator<Item = impl Into<Filter>>) -> Filter {
        let clauses: Vec<WhereClause> = filters
            .into_iter()
            .map(|filter| WhereClause::or(filter.into().0))
            .collect();
        if clauses.is_empty() {
            return Filter(Where::Raw {
                sql: Cow::Borrowed("1 = 0"),
                values: Vec::new(),
            });
        }
        Filter(Where::Group(clauses))
    }

    /// The condition as an IR node.
    pub fn into_where(self) -> Where {
        self.0
    }

    /// The condition as SQL for `dialect` (for tests and debugging).
    pub fn to_sql(&self, dialect: crate::Dialect) -> Result<crate::Statement, crate::OrmError> {
        crate::sql::compile_filter(&self.0, dialect)
    }
}

impl From<Where> for Filter {
    fn from(filter: Where) -> Self {
        Filter(filter)
    }
}

impl From<sea_query::Condition> for Filter {
    fn from(condition: sea_query::Condition) -> Self {
        Filter(Where::Sql(condition))
    }
}

impl From<sea_query::Expr> for Filter {
    fn from(expr: sea_query::Expr) -> Self {
        Filter(Where::Sql(sea_query::Condition::all().add(expr)))
    }
}

/// `SET column = value` in a mass update, or an attribute of
/// `first_or_create` and friends: [`Column::set`].
#[derive(Clone, Debug)]
pub struct Assignment {
    pub(crate) column: Cow<'static, str>,
    pub(crate) value: AssignedValue,
}

/// What an [`Assignment`] sets.
#[derive(Clone, Debug)]
pub(crate) enum AssignedValue {
    Value(Value),
    Expr(Expr),
    Invalid(String),
}

impl Assignment {
    /// Sets `column` to `value`.
    pub fn value(column: impl Into<Cow<'static, str>>, value: impl Into<Value>) -> Self {
        Assignment {
            column: column.into(),
            value: AssignedValue::Value(value.into()),
        }
    }

    pub(crate) fn invalid(column: Cow<'static, str>, message: String) -> Self {
        Assignment {
            column,
            value: AssignedValue::Invalid(message),
        }
    }

    /// The column set.
    pub fn column(&self) -> &str {
        &self.column
    }

    /// The value set, unless it is an expression.
    pub fn assigned_value(&self) -> Option<&Value> {
        match &self.value {
            AssignedValue::Value(value) => Some(value),
            AssignedValue::Expr(_) | AssignedValue::Invalid(_) => None,
        }
    }

    /// The assignment as `SET column = expr`.
    pub(crate) fn into_set(self) -> Result<(Cow<'static, str>, Expr), String> {
        match self.value {
            AssignedValue::Value(value) => Ok((self.column, Expr::Value(value))),
            AssignedValue::Expr(expr) => Ok((self.column, expr)),
            AssignedValue::Invalid(error) => Err(error),
        }
    }

    /// The assignment as the condition `column = value` (attribute lookups).
    pub(crate) fn to_filter(&self, table: Option<&'static str>) -> Filter {
        let mut column = ColumnRef::parse(self.column.clone());
        if column.table.is_none() {
            column.table = table.map(Cow::Borrowed);
        }
        match &self.value {
            AssignedValue::Value(value) => Filter::compare(
                Expr::Column(column),
                Operator::Eq,
                Expr::Value(value.clone()),
            ),
            AssignedValue::Expr(expr) => {
                Filter::compare(Expr::Column(column), Operator::Eq, expr.clone())
            }
            AssignedValue::Invalid(error) => Filter(Where::Invalid(error.clone())),
        }
    }
}

/// A column given by name or as a typed constant, for methods that only
/// need its name (`is_dirty_on`, `extra`, ...).
pub trait ColumnName {
    /// The column's name (without table or JSON path).
    fn column_name(&self) -> Cow<'_, str>;
}

impl<M, T> ColumnName for Column<M, T> {
    fn column_name(&self) -> Cow<'_, str> {
        Cow::Owned(ColumnRef::parse(self.name.clone()).name.into_owned())
    }
}

impl ColumnName for &str {
    fn column_name(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl ColumnName for String {
    fn column_name(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

/// A column of values of type `T`, typed ([`Column<M, T>`]) or named by a
/// string (any value), for the builder methods that take a column and
/// values: `where_in`, `where_between`, `where_date`, ...
pub trait ColumnOf<T> {
    /// The column.
    fn column_ref(&self) -> ColumnRef;

    /// Encodes a value as the column stores it.
    fn encode_value(&self, value: &T) -> Result<Value, String>;
}

impl<M, T> ColumnOf<T> for Column<M, T> {
    fn column_ref(&self) -> ColumnRef {
        Column::column_ref(self)
    }

    fn encode_value(&self, value: &T) -> Result<Value, String> {
        (self.encode)(value)
    }
}

impl ColumnOf<Value> for &str {
    fn column_ref(&self) -> ColumnRef {
        ColumnRef::parse(Cow::Owned((*self).to_owned()))
    }

    fn encode_value(&self, value: &Value) -> Result<Value, String> {
        Ok(value.clone())
    }
}

impl ColumnOf<Value> for String {
    fn column_ref(&self) -> ColumnRef {
        ColumnRef::parse(Cow::Owned(self.clone()))
    }

    fn encode_value(&self, value: &Value) -> Result<Value, String> {
        Ok(value.clone())
    }
}

/// Something that names a column or computes a value in SQL: a typed
/// [`Column`], a column name (`"title"`, `"deals.title"`,
/// `"meta->status"`), or an [`Expr`]. Select lists, orders and groups take
/// these.
pub trait IntoExpr {
    /// The expression.
    fn into_expr(self) -> Expr;
}

impl<M, T> IntoExpr for Column<M, T> {
    fn into_expr(self) -> Expr {
        self.expr()
    }
}

impl<M, T> IntoExpr for &Column<M, T> {
    fn into_expr(self) -> Expr {
        self.expr()
    }
}

impl IntoExpr for &str {
    fn into_expr(self) -> Expr {
        column_or_all(Cow::Owned(self.to_owned()))
    }
}

impl IntoExpr for String {
    fn into_expr(self) -> Expr {
        column_or_all(Cow::Owned(self))
    }
}

impl IntoExpr for Expr {
    fn into_expr(self) -> Expr {
        self
    }
}

impl IntoExpr for ColumnRef {
    fn into_expr(self) -> Expr {
        Expr::Column(self)
    }
}

/// `*` and `table.*` select every column; anything else names one.
fn column_or_all(name: Cow<'static, str>) -> Expr {
    let trimmed = name.trim();
    if trimmed == "*" {
        return Expr::All { table: None };
    }
    if let Some(table) = trimmed.strip_suffix(".*") {
        return Expr::All {
            table: Some(Cow::Owned(table.to_owned())),
        };
    }
    Expr::Column(ColumnRef::parse(name))
}

/// A query usable as a subquery: model builders, table builders and IR.
pub trait IntoQuery {
    /// The query, with any global scopes applied.
    fn into_query(self) -> QueryIr;
}

impl IntoQuery for QueryIr {
    fn into_query(self) -> QueryIr {
        self
    }
}

impl<M, T> From<Column<M, T>> for Expr {
    fn from(column: Column<M, T>) -> Self {
        column.expr()
    }
}

impl<M, T> From<&Column<M, T>> for Expr {
    fn from(column: &Column<M, T>) -> Self {
        column.expr()
    }
}

impl<M, T> From<&Column<M, T>> for Order {
    fn from(column: &Column<M, T>) -> Self {
        column.asc()
    }
}

impl<M, T> From<Column<M, T>> for Order {
    fn from(column: Column<M, T>) -> Self {
        column.asc()
    }
}

impl From<&str> for Order {
    fn from(column: &str) -> Self {
        Order::asc(column.into_expr())
    }
}
