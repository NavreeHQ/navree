//! The ORM's IR ([`crate::ir`]) as [syntax trees](super::ast), rule for rule
//! as the compiler in [`crate::sql`] builds statements today: the same
//! dialect rules (date parts, LIKE variants, JSON paths and containment,
//! full text, upserts, group limits, time buckets, vector distances, the
//! MySQL timeout hint), the same errors, the same results. The `render_*`
//! functions lower and render in one step, with the signatures of
//! `compile_*`.
//!
//! [`Where::Sql`] (a condition built by the query builder this replaces)
//! answers `Unsupported` here; it is ported away separately.
//!
//! ```
//! use navree_orm::Dialect;
//! use navree_orm::ir::QueryIr;
//! use navree_orm::render::render_select;
//!
//! let mut query = QueryIr::new("deals");
//! query.limit = Some(10);
//! query.offset = Some(20);
//! assert_eq!(
//!     render_select(&query, Dialect::Sqlite).unwrap().sql,
//!     r#"SELECT * FROM "deals" LIMIT ? OFFSET ?"#
//! );
//! ```

use std::borrow::Cow;

use super::ast;
use super::grammar::{is_identifier, json_path_text};
use crate::error::{OrmError, Result};
use crate::ir::{
    AggregateFunction, ArithmeticOp, ColumnRef, Direction, Expr, JsonSegment, Nulls, Order,
    VectorMetric,
};
use crate::sql::{Dialect, Statement};
use crate::value::Value;

mod filter;
mod select;
mod text;
mod timescale;
mod write;

pub use select::{aggregate, exists, select};
pub use write::{delete, insert, truncate, update, update_from};

/// The SQL of a read query, as [`compile_select`](crate::sql::compile_select)
/// builds it.
pub fn render_select(query: &crate::ir::QueryIr, dialect: Dialect) -> Result<Statement> {
    super::render_query(&select(query, dialect)?, dialect)
}

/// The SQL of an aggregate over a query's rows, in a column named
/// `aggregate`, as [`compile_aggregate`](crate::sql::compile_aggregate)
/// builds it.
pub fn render_aggregate(
    query: &crate::ir::QueryIr,
    aggregate: &crate::ir::Aggregate,
    dialect: Dialect,
) -> Result<Statement> {
    super::render_query(&select::aggregate(query, aggregate, dialect)?, dialect)
}

/// `SELECT EXISTS(query) AS "exists"`, as
/// [`compile_exists`](crate::sql::compile_exists) builds it.
pub fn render_exists(query: &crate::ir::QueryIr, dialect: Dialect) -> Result<Statement> {
    super::render_query(&select::exists(query, dialect)?, dialect)
}

/// The SQL of an insert, as [`compile_insert`](crate::sql::compile_insert)
/// builds it (`RETURNING` on PostgreSQL and SQLite only; MySQL reads
/// `last_insert_id`).
///
/// ```
/// use navree_orm::Value;
/// use navree_orm::ir::{Expr, InsertIr, OnConflict};
/// use navree_orm::render::render_insert;
/// use navree_orm::sql::Dialect;
///
/// let insert = InsertIr::new("deals", vec!["title".into()], vec![vec![Expr::Value(Value::from("Renewal"))]])
///     .returning(vec!["id".into()]);
/// let sql = render_insert(&insert, Dialect::Postgres).unwrap();
/// assert_eq!(sql.sql, r#"INSERT INTO "deals" ("title") VALUES ($1) RETURNING "id""#);
///
/// let ignore = insert.on_conflict(OnConflict::Ignore { target: vec![] });
/// assert_eq!(
///     render_insert(&ignore, Dialect::MySql).unwrap().sql,
///     "INSERT IGNORE INTO `deals` (`title`) VALUES (?)"
/// );
/// ```
pub fn render_insert(insert: &crate::ir::InsertIr, dialect: Dialect) -> Result<Statement> {
    super::render(
        &ast::Statement::from(write::insert(insert, dialect)?),
        dialect,
    )
}

/// The SQL of an update, as [`compile_update`](crate::sql::compile_update)
/// builds it (JSON path assignments folded per column).
pub fn render_update(update: &crate::ir::UpdateIr, dialect: Dialect) -> Result<Statement> {
    super::render(
        &ast::Statement::from(write::update(update, dialect)?),
        dialect,
    )
}

/// `UPDATE table SET .. FROM joined .. WHERE ..` (MySQL: `UPDATE table,
/// joined SET ..`), as [`compile_update_from`](crate::sql::compile_update_from)
/// builds it.
pub fn render_update_from(
    table: &crate::ir::TableRef,
    sets: &[(Cow<'static, str>, Expr)],
    joins: &[crate::ir::Join],
    wheres: &[crate::ir::WhereClause],
    dialect: Dialect,
) -> Result<Statement> {
    let update = write::update_from(table, sets, joins, wheres, dialect)?;
    super::render(&ast::Statement::from(update), dialect)
}

/// The SQL of a delete, as [`compile_delete`](crate::sql::compile_delete)
/// builds it.
pub fn render_delete(delete: &crate::ir::DeleteIr, dialect: Dialect) -> Result<Statement> {
    super::render(
        &ast::Statement::from(write::delete(delete, dialect)?),
        dialect,
    )
}

/// The statements that empty `table` and reset its identity, as
/// [`compile_truncate`](crate::sql::compile_truncate) builds them.
pub fn render_truncate(table: &str, cascade: bool, dialect: Dialect) -> Vec<Statement> {
    super::grammar::for_dialect(dialect).truncate(&write::truncate(table, cascade))
}

/// Lowers IR nodes for one dialect.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lowerer {
    pub(crate) dialect: Dialect,
}

impl Lowerer {
    pub(crate) fn new(dialect: Dialect) -> Self {
        Lowerer { dialect }
    }

    pub(crate) fn unsupported(&self, feature: impl Into<Cow<'static, str>>) -> OrmError {
        super::unsupported(self.dialect, feature)
    }

    /// A column, with its JSON path read as text.
    pub(crate) fn column(&self, column: &ColumnRef) -> ast::Expr {
        let base = self.plain_column(column);
        if column.path.is_empty() {
            base
        } else {
            base.json_text(column.path.clone())
        }
    }

    /// A column without its JSON path.
    pub(crate) fn plain_column(&self, column: &ColumnRef) -> ast::Expr {
        ast::Expr::Column(ast::ColumnName {
            table: column.table.clone().map(ast::ObjectName::parse),
            name: ast::Ident(column.name.clone()),
        })
    }

    /// The JSON at the column's path, kept as JSON (PostgreSQL's `->`).
    pub(crate) fn json_value(&self, column: &ColumnRef) -> ast::Expr {
        let base = self.plain_column(column);
        if column.path.is_empty() {
            base
        } else {
            base.json(column.path.clone())
        }
    }

    /// A JSON path as the string literal MySQL and SQLite take:
    /// `'$."a"[0]'`.
    pub(crate) fn json_path(&self, path: &[JsonSegment]) -> ast::Expr {
        ast::Expr::string(json_path_text(path, self.dialect))
    }

    /// An expression.
    pub(crate) fn expr(&self, expr: &Expr) -> Result<ast::Expr> {
        Ok(match expr {
            Expr::Column(column) => self.column(column),
            Expr::Value(value) => ast::Expr::Value(value.clone()),
            Expr::Raw { sql, values } => self.raw(sql.clone(), values)?,
            Expr::Function { name, args } => {
                if !is_identifier(name) {
                    return Err(OrmError::invalid(format!(
                        "`{name}` is not a valid SQL function name"
                    )));
                }
                let args = args
                    .iter()
                    .map(|arg| self.expr(arg))
                    .collect::<Result<Vec<_>>>()?;
                ast::Expr::func(name.clone(), args)
            }
            Expr::Aggregate {
                function,
                arg,
                distinct,
            } => {
                let arg = match arg {
                    Some(arg) => self.expr(arg)?,
                    None => ast::Expr::Wildcard(None),
                };
                aggregate_call(*function, arg, *distinct)
            }
            Expr::Arithmetic { left, op, right } => {
                let left = self.expr(left)?;
                let right = self.expr(right)?;
                left.binary(
                    match op {
                        ArithmeticOp::Add => ast::BinaryOp::Add,
                        ArithmeticOp::Sub => ast::BinaryOp::Sub,
                        ArithmeticOp::Mul => ast::BinaryOp::Mul,
                        ArithmeticOp::Div => ast::BinaryOp::Div,
                    },
                    right,
                )
            }
            Expr::Subquery(query) => ast::Expr::Subquery(Box::new(self.select(query)?)),
            Expr::TimeBucket {
                width,
                expr,
                origin,
            } => {
                let expr = self.expr(expr)?;
                let origin = origin
                    .as_deref()
                    .map(|origin| self.expr(origin))
                    .transpose()?;
                self.time_bucket(width, expr, origin)?
            }
            Expr::First { value, time } => self.first_last("first", value, time)?,
            Expr::Last { value, time } => self.first_last("last", value, time)?,
            Expr::All { table } => ast::Expr::Wildcard(table.clone().map(ast::ObjectName::parse)),
            Expr::Exists(query) => {
                let mut query = (**query).clone();
                query.orders.clear();
                filter::exists_select_list(&mut query);
                ast::Expr::Exists {
                    query: Box::new(self.select(&query)?),
                    negated: false,
                }
            }
            Expr::RowNumber { partition, order } => {
                let partition = partition
                    .iter()
                    .map(|expr| self.expr(expr))
                    .collect::<Result<Vec<_>>>()?;
                ast::Expr::row_number().over(partition, self.orders(order, false)?)
            }
            Expr::VectorDistance {
                expr,
                vector,
                metric,
            } => self.vector_distance(expr, vector, *metric)?,
            Expr::Binary { left, op, right } => {
                let allowed = value_operators(self.dialect);
                let normalized = normalize_operator(op);
                let Some(known) = allowed.iter().find(|known| **known == normalized) else {
                    return Err(OrmError::invalid(format!(
                        "`{op}` is not an operator {} expressions take (they take: {})",
                        self.dialect.name(),
                        allowed.join(", ")
                    )));
                };
                let (left, right) = (self.expr(left)?, self.expr(right)?);
                left.binary(ast::BinaryOp::Custom(Cow::Borrowed(known)), right)
            }
            Expr::Excluded { column } => ast::Expr::Excluded(ast::Ident(column.clone())),
        })
    }

    /// A raw fragment, its placeholders checked against its values now
    /// (where the compiler checks them).
    pub(crate) fn raw(&self, sql: Cow<'static, str>, values: &[Value]) -> Result<ast::Expr> {
        super::writer::check_raw(&sql, values.len(), self.dialect)?;
        Ok(ast::Expr::Raw {
            sql,
            values: values.to_vec(),
        })
    }

    /// `ORDER BY` items. With `unqualified`, plain columns lose their
    /// table: the orders of a union name its result columns.
    pub(crate) fn orders(&self, orders: &[Order], unqualified: bool) -> Result<Vec<ast::OrderBy>> {
        let mut items = Vec::with_capacity(orders.len());
        for order in orders {
            items.push(match order {
                Order::By {
                    expr,
                    direction,
                    nulls,
                } => ast::OrderBy {
                    expr: if unqualified {
                        self.expr(&unqualify(expr))?
                    } else {
                        self.expr(expr)?
                    },
                    descending: *direction == Direction::Desc,
                    nulls: nulls.map(|nulls| match nulls {
                        Nulls::First => ast::NullsOrder::First,
                        Nulls::Last => ast::NullsOrder::Last,
                    }),
                },
                Order::Random => ast::OrderBy {
                    expr: ast::Expr::Random,
                    descending: false,
                    nulls: None,
                },
                Order::Raw { sql, values } => {
                    let (body, descending, nulls) = split_direction(sql);
                    ast::OrderBy {
                        expr: self.raw(Cow::Owned(body.to_owned()), values)?,
                        descending,
                        nulls,
                    }
                }
            });
        }
        Ok(items)
    }

    /// The distance between the vector `expr` and `vector`: pgvector's
    /// operators on PostgreSQL, MariaDB's `VEC_DISTANCE_*` on MySQL.
    fn vector_distance(
        &self,
        expr: &Expr,
        vector: &[f32],
        metric: VectorMetric,
    ) -> Result<ast::Expr> {
        if vector.is_empty() || vector.iter().any(|x| !x.is_finite()) {
            return Err(OrmError::invalid(
                "a vector to compare with must hold at least one number, and only finite ones",
            ));
        }
        let text = format!(
            "[{}]",
            vector
                .iter()
                .map(f32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let column = self.expr(expr)?;
        let literal = ast::Expr::Value(Value::from(text));
        Ok(match (self.dialect, metric) {
            (Dialect::Postgres, metric) => {
                let op = match metric {
                    VectorMetric::Euclidean => "<->",
                    VectorMetric::InnerProduct => "<#>",
                    _ => "<=>",
                };
                column.binary(
                    ast::BinaryOp::Custom(Cow::Borrowed(op)),
                    literal.cast(ast::SqlType::Custom(Cow::Borrowed("vector"))),
                )
            }
            (Dialect::MySql, VectorMetric::Cosine | VectorMetric::Euclidean) => {
                let function = if metric == VectorMetric::Euclidean {
                    "VEC_DISTANCE_EUCLIDEAN"
                } else {
                    "VEC_DISTANCE_COSINE"
                };
                ast::Expr::func(
                    function,
                    vec![column, ast::Expr::func("VEC_FromText", vec![literal])],
                )
            }
            (Dialect::MySql, _) => {
                return Err(self.unsupported("the inner-product vector distance"));
            }
            (Dialect::Sqlite, _) => return Err(self.unsupported("a vector distance")),
        })
    }
}

/// `FUNCTION([DISTINCT] arg)` of an aggregate.
pub(crate) fn aggregate_call(
    function: AggregateFunction,
    arg: ast::Expr,
    distinct: bool,
) -> ast::Expr {
    ast::Expr::Function(ast::Function {
        name: Cow::Borrowed(function.sql_name()),
        args: vec![arg],
        distinct,
    })
}

/// A column reference without its table (outside a subquery, the columns
/// of a derived table are unqualified).
pub(crate) fn unqualify(expr: &Expr) -> Expr {
    match expr {
        Expr::Column(column) => Expr::Column(ColumnRef {
            table: None,
            ..column.clone()
        }),
        other => other.clone(),
    }
}

/// Splits the trailing direction (and NULLs placement) off a raw order:
/// `"field(stage, ?) DESC NULLS LAST"` is `field(stage, ?)`, descending,
/// NULLs last. Without a direction the order is ascending, as in SQL.
pub(crate) fn split_direction(sql: &str) -> (&str, bool, Option<ast::NullsOrder>) {
    let mut body = sql.trim_end();
    let mut nulls = None;
    for (suffix, placement) in [
        (" nulls first", ast::NullsOrder::First),
        (" nulls last", ast::NullsOrder::Last),
    ] {
        if ends_with_ignore_case(body, suffix) {
            body = body[..body.len() - suffix.len()].trim_end();
            nulls = Some(placement);
            break;
        }
    }
    let mut descending = false;
    for (suffix, desc) in [(" desc", true), (" asc", false)] {
        if ends_with_ignore_case(body, suffix) {
            body = body[..body.len() - suffix.len()].trim_end();
            descending = desc;
            break;
        }
    }
    (body, descending, nulls)
}

fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    text.len() >= suffix.len()
        && text.is_char_boundary(text.len() - suffix.len())
        && text[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// The operators a hand-written expression ([`Expr::Binary`]) may use on
/// each database: arithmetic, bit operations, concatenation, and the JSON,
/// array and vector operators that compute values.
fn value_operators(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres => &[
            "+", "-", "*", "/", "%", "^", "||", "#-", "->", "->>", "#>", "#>>", "<->", "<=>",
            "<#>", "<+>", "&", "|", "#", "<<", ">>",
        ],
        Dialect::MySql => &[
            "+", "-", "*", "/", "%", "DIV", "MOD", "&", "|", "^", "<<", ">>", "->", "->>",
        ],
        Dialect::Sqlite => &[
            "+", "-", "*", "/", "%", "||", "&", "|", "<<", ">>", "->", "->>",
        ],
    }
}

/// An operator as the allow-lists spell it: upper case, single spaces.
pub(crate) fn normalize_operator(op: &str) -> String {
    op.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_orders_split_their_direction() {
        assert_eq!(
            split_direction("field(stage, ?) DESC NULLS LAST"),
            ("field(stage, ?)", true, Some(ast::NullsOrder::Last))
        );
        assert_eq!(split_direction("a asc"), ("a", false, None));
        assert_eq!(split_direction("score"), ("score", false, None));
    }
}
