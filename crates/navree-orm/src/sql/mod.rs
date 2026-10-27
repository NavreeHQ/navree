//! SQL for PostgreSQL / TimescaleDB, MySQL and SQLite.
//!
//! The IR is compiled into sea-query statements and built with the
//! dialect's query builder, so identifiers are quoted and values bound the
//! way SeaORM binds them. Dialect differences live next to the clause they
//! affect: date parts, LIKE and JSON in [`filter`], `RETURNING` and
//! generated keys in [`write`], TimescaleDB's helpers in [`timescale`].

use std::fmt;

use sea_orm::DbBackend;
use sea_query::{
    Alias, Asterisk, ExprTrait, Func, MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder,
};

use crate::error::OrmError;
use crate::ir::{AggregateFunction, ColumnRef, Expr, JsonSegment, VectorMetric, Where};
use crate::value::Value;

mod filter;
mod select;
mod timescale;
mod write;

pub(crate) use select::timeout_millis;
pub use select::{compile_aggregate, compile_exists, compile_select};
pub use write::{
    compile_delete, compile_insert, compile_truncate, compile_update, compile_update_from,
};

/// An SQL dialect.
///
/// TimescaleDB is PostgreSQL with the extension: its helpers
/// (`time_bucket`, `first`, `last`) compile on [`Dialect::Postgres`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// PostgreSQL and TimescaleDB.
    Postgres,
    /// MySQL and MariaDB.
    MySql,
    /// SQLite.
    Sqlite,
}

impl Dialect {
    /// The dialect of a SeaORM backend.
    pub fn from_backend(backend: DbBackend) -> Result<Self, OrmError> {
        match backend {
            DbBackend::Postgres => Ok(Dialect::Postgres),
            DbBackend::MySql => Ok(Dialect::MySql),
            DbBackend::Sqlite => Ok(Dialect::Sqlite),
            other => Err(OrmError::unsupported("the ORM", format!("{other:?}"))),
        }
    }

    /// The SeaORM backend of this dialect.
    pub fn backend(self) -> DbBackend {
        match self {
            Dialect::Postgres => DbBackend::Postgres,
            Dialect::MySql => DbBackend::MySql,
            Dialect::Sqlite => DbBackend::Sqlite,
        }
    }

    /// The database's name in messages.
    pub fn name(self) -> &'static str {
        match self {
            Dialect::Postgres => "PostgreSQL",
            Dialect::MySql => "MySQL",
            Dialect::Sqlite => "SQLite",
        }
    }

    /// Whether `INSERT`, `UPDATE` and `DELETE` can return rows.
    pub fn supports_returning(self) -> bool {
        !matches!(self, Dialect::MySql)
    }

    /// The most values one statement can bind: 65,535 on PostgreSQL and
    /// MySQL, 32,766 on SQLite. Multi-row inserts are split to stay under
    /// it.
    pub fn max_bind_values(self) -> usize {
        match self {
            Dialect::Postgres | Dialect::MySql => 65_535,
            Dialect::Sqlite => 32_766,
        }
    }

    /// `name` quoted as an identifier (`"deals"`, `` `deals` ``), each part
    /// of a schema-qualified name on its own.
    ///
    /// ```
    /// use navree_orm::Dialect;
    ///
    /// assert_eq!(Dialect::Postgres.quote_identifier("audit.events"), r#""audit"."events""#);
    /// assert_eq!(Dialect::MySql.quote_identifier("odd`name"), "`odd``name`");
    /// ```
    pub fn quote_identifier(self, name: &str) -> String {
        quote_identifier(name, self)
    }
}

/// `name` quoted as an identifier for `dialect`.
pub(crate) fn quote_identifier(name: &str, dialect: Dialect) -> String {
    let quote = match dialect {
        Dialect::MySql => '`',
        Dialect::Postgres | Dialect::Sqlite => '"',
    };
    name.split('.')
        .map(|part| {
            let escaped = part.replace(quote, &format!("{quote}{quote}"));
            format!("{quote}{escaped}{quote}")
        })
        .collect::<Vec<_>>()
        .join(".")
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A compiled statement: SQL with placeholders and the values bound to
/// them.
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    /// The SQL (`$1` placeholders on PostgreSQL, `?` elsewhere).
    pub sql: String,
    /// The bound values, in placeholder order.
    pub values: Vec<Value>,
    /// The dialect it was compiled for.
    pub dialect: Dialect,
}

impl Statement {
    /// The statement as SeaORM runs it.
    pub fn to_sea_orm(&self) -> sea_orm::Statement {
        sea_orm::Statement::from_sql_and_values(
            self.dialect.backend(),
            self.sql.clone(),
            self.values.clone(),
        )
    }
}

impl fmt::Display for Statement {
    /// The SQL followed by the values, one statement per line:
    /// `SELECT ... WHERE "id" = $1 -- [BigInt(Some(1))]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sql)?;
        if !self.values.is_empty() {
            write!(f, " -- {:?}", self.values)?;
        }
        Ok(())
    }
}

/// Builds a sea-query statement for `dialect`.
pub(crate) fn build<S: sea_query::QueryStatementBuilder>(
    statement: &S,
    dialect: Dialect,
) -> Statement {
    let (sql, values) = match dialect {
        Dialect::Postgres => statement.build_any(&PostgresQueryBuilder),
        Dialect::MySql => statement.build_any(&MysqlQueryBuilder),
        Dialect::Sqlite => statement.build_any(&SqliteQueryBuilder),
    };
    Statement {
        sql,
        values: values.0,
        dialect,
    }
}

/// A filter alone, as `WHERE` would hold it (for tests and debugging).
pub(crate) fn compile_filter(filter: &Where, dialect: Dialect) -> Result<Statement, OrmError> {
    let compiler = Compiler::new(dialect);
    let condition = compiler.where_(filter)?;
    let mut select = sea_query::Query::select();
    select
        .expr(sea_query::Expr::cust("1"))
        .cond_where(condition);
    let statement = build(&select, dialect);
    let sql = statement
        .sql
        .strip_prefix("SELECT 1 WHERE ")
        .unwrap_or(&statement.sql)
        .to_owned();
    Ok(Statement { sql, ..statement })
}

/// Compiles IR nodes for one dialect.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Compiler {
    pub(crate) dialect: Dialect,
}

impl Compiler {
    pub(crate) fn new(dialect: Dialect) -> Self {
        Compiler { dialect }
    }

    pub(crate) fn unsupported(
        &self,
        feature: impl Into<std::borrow::Cow<'static, str>>,
    ) -> OrmError {
        OrmError::unsupported(feature, self.dialect.name())
    }

    /// A custom SQL fragment whose `{}` markers are replaced by `exprs`, in
    /// order. Literal `?` and `$` in the template must sit inside quotes.
    pub(crate) fn custom(&self, template: &str, exprs: Vec<sea_query::Expr>) -> sea_query::Expr {
        let mut sql = String::with_capacity(template.len() + 8);
        let mut index = 0;
        let mut rest = template;
        while let Some(at) = rest.find("{}") {
            sql.push_str(&rest[..at]);
            index += 1;
            match self.dialect {
                Dialect::Postgres => {
                    sql.push('$');
                    sql.push_str(&index.to_string());
                }
                Dialect::MySql | Dialect::Sqlite => sql.push('?'),
            }
            rest = &rest[at + 2..];
        }
        sql.push_str(rest);
        sea_query::Expr::cust_with_exprs(sql, exprs)
    }

    /// A column, with its JSON path extracted as text.
    pub(crate) fn column(&self, column: &ColumnRef) -> sea_query::Expr {
        let base = self.plain_column(column);
        if column.path.is_empty() {
            base
        } else {
            self.json_text(base, &column.path)
        }
    }

    /// A column without its JSON path.
    pub(crate) fn plain_column(&self, column: &ColumnRef) -> sea_query::Expr {
        match &column.table {
            Some(table) => sea_query::Expr::col((
                Alias::new(table.to_string()),
                Alias::new(column.name.to_string()),
            )),
            None => sea_query::Expr::col(Alias::new(column.name.to_string())),
        }
    }

    /// The JSON value at `path` of `base`, as text (for comparisons).
    fn json_text(&self, base: sea_query::Expr, path: &[JsonSegment]) -> sea_query::Expr {
        match self.dialect {
            Dialect::Postgres => {
                let mut template = String::from("({}");
                for (i, segment) in path.iter().enumerate() {
                    template.push_str(if i + 1 == path.len() { "->>" } else { "->" });
                    template.push_str(&postgres_path_segment(segment));
                }
                template.push(')');
                self.custom(&template, vec![base])
            }
            Dialect::MySql => Func::cust(Alias::new("json_unquote"))
                .arg(
                    Func::cust(Alias::new("json_extract"))
                        .arg(base)
                        .arg(sea_query::Expr::cust(self.json_path(path))),
                )
                .into(),
            Dialect::Sqlite => Func::cust(Alias::new("json_extract"))
                .arg(base)
                .arg(sea_query::Expr::cust(self.json_path(path)))
                .into(),
        }
    }

    /// The JSON value at `path` of `column`, kept as JSON (PostgreSQL
    /// `->`), for containment and length checks (whose templates
    /// parenthesize it).
    pub(crate) fn json_value(&self, column: &ColumnRef) -> sea_query::Expr {
        let base = self.plain_column(column);
        if column.path.is_empty() {
            return base;
        }
        let mut template = String::from("{}");
        for segment in &column.path {
            template.push_str("->");
            template.push_str(&postgres_path_segment(segment));
        }
        self.custom(&template, vec![base])
    }

    /// An expression.
    pub(crate) fn expr(&self, expr: &Expr) -> Result<sea_query::Expr, OrmError> {
        Ok(match expr {
            Expr::Column(column) => self.column(column),
            Expr::Value(value) => sea_query::Expr::val(value.clone()),
            Expr::Raw { sql, values } => {
                let sql = raw_sql(sql, values.len(), self.dialect)?;
                sea_query::Expr::cust_with_values(sql, values.iter().cloned())
            }
            Expr::Function { name, args } => {
                if !is_identifier(name) {
                    return Err(OrmError::invalid(format!(
                        "`{name}` is not a valid SQL function name"
                    )));
                }
                let args = args
                    .iter()
                    .map(|arg| self.expr(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Func::cust(Alias::new(name.to_string())).args(args).into()
            }
            Expr::Aggregate {
                function,
                arg,
                distinct,
            } => {
                let arg = match arg {
                    Some(arg) => self.expr(arg)?,
                    None => sea_query::Expr::col(Asterisk),
                };
                match (function, distinct) {
                    (AggregateFunction::Count, false) => Func::count(arg).into(),
                    (AggregateFunction::Count, true) => Func::count_distinct(arg).into(),
                    (AggregateFunction::Sum, false) => Func::sum(arg).into(),
                    (AggregateFunction::Avg, false) => Func::avg(arg).into(),
                    (AggregateFunction::Min, false) => Func::min(arg).into(),
                    (AggregateFunction::Max, false) => Func::max(arg).into(),
                    (function, true) => self.custom(
                        &format!("{}(DISTINCT {{}})", function.sql_name()),
                        vec![arg],
                    ),
                }
            }
            Expr::Arithmetic { left, op, right } => {
                let left = self.expr(left)?;
                let right = self.expr(right)?;
                match op {
                    crate::ir::ArithmeticOp::Add => left.add(right),
                    crate::ir::ArithmeticOp::Sub => left.sub(right),
                    crate::ir::ArithmeticOp::Mul => left.mul(right),
                    crate::ir::ArithmeticOp::Div => left.div(right),
                }
            }
            Expr::Subquery(query) => {
                let select = self.select(query)?;
                sea_query::Expr::SubQuery(None, Box::new(select.into()))
            }
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
            Expr::All { table } => match table {
                Some(table) => sea_query::Expr::col((Alias::new(table.to_string()), Asterisk)),
                None => sea_query::Expr::col(Asterisk),
            },
            Expr::VectorDistance {
                expr,
                vector,
                metric,
            } => self.vector_distance(expr, vector, *metric)?,
            Expr::Binary { left, op, right } => {
                let allowed = value_operators(self.dialect);
                let normalized = op.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase();
                let Some(known) = allowed.iter().find(|known| **known == normalized) else {
                    return Err(OrmError::invalid(format!(
                        "`{op}` is not an operator {} expressions take (they take: {})",
                        self.dialect.name(),
                        allowed.join(", ")
                    )));
                };
                let (left, right) = (self.expr(left)?, self.expr(right)?);
                self.custom(&format!("({{}} {known} {{}})"), vec![left, right])
            }
            Expr::Excluded { column } => match self.dialect {
                Dialect::Postgres | Dialect::Sqlite => sea_query::Expr::col((
                    Alias::new("excluded"),
                    Alias::new(column.to_string()),
                )),
                Dialect::MySql => Func::cust(Alias::new("VALUES"))
                    .arg(sea_query::Expr::col(Alias::new(column.to_string())))
                    .into(),
            },
        })
    }
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
        Dialect::Sqlite => &["+", "-", "*", "/", "%", "||", "&", "|", "<<", ">>", "->", "->>"],
    }
}

impl Compiler {
    /// The distance between the vector `expr` and `vector`.
    fn vector_distance(
        &self,
        expr: &Expr,
        vector: &[f32],
        metric: VectorMetric,
    ) -> Result<sea_query::Expr, OrmError> {
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
        let literal = sea_query::Expr::val(Value::from(text));
        Ok(match (self.dialect, metric) {
            (Dialect::Postgres, metric) => {
                let op = match metric {
                    VectorMetric::Euclidean => "<->",
                    VectorMetric::InnerProduct => "<#>",
                    _ => "<=>",
                };
                self.custom(&format!("({{}} {op} CAST({{}} AS vector))"), vec![column, literal])
            }
            (Dialect::MySql, VectorMetric::Cosine | VectorMetric::Euclidean) => {
                let function = if metric == VectorMetric::Euclidean {
                    "VEC_DISTANCE_EUCLIDEAN"
                } else {
                    "VEC_DISTANCE_COSINE"
                };
                self.custom(&format!("{function}({{}}, VEC_FromText({{}}))"), vec![column, literal])
            }
            (Dialect::MySql, _) => {
                return Err(self.unsupported("the inner-product vector distance"));
            }
            (Dialect::Sqlite, _) => return Err(self.unsupported("a vector distance")),
        })
    }
}

/// Whether `name` is a plain (optionally schema-qualified) SQL identifier.
pub(crate) fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// One PostgreSQL JSON path step: `'key'` or an index.
fn postgres_path_segment(segment: &JsonSegment) -> String {
    match segment {
        JsonSegment::Key(key) => format!("'{}'", key.replace('\'', "''")),
        JsonSegment::Index(index) => index.to_string(),
    }
}

impl Compiler {
    /// The SQL string literal of a JSON path for this dialect (MySQL and
    /// SQLite): `'$."key"[0]'`.
    pub(crate) fn json_path(&self, path: &[JsonSegment]) -> String {
        json_path_literal(path, self.dialect)
    }
}

/// The SQL string literal of a JSON path for MySQL and SQLite:
/// `'$."key"[0]'`. An index from the end (negative) is `[last]`,
/// `[last-1]`, ... on MySQL and `[#-1]`, `[#-2]`, ... on SQLite.
pub(crate) fn json_path_literal(path: &[JsonSegment], dialect: Dialect) -> String {
    let mut text = String::from("$");
    for segment in path {
        match segment {
            JsonSegment::Key(key) => {
                text.push_str(".\"");
                text.push_str(&key.replace('\\', "\\\\").replace('"', "\\\""));
                text.push('"');
            }
            JsonSegment::Index(index) if *index < 0 => {
                let back = index.unsigned_abs();
                match dialect {
                    Dialect::MySql if back == 1 => text.push_str("[last]"),
                    Dialect::MySql => text.push_str(&format!("[last-{}]", back - 1)),
                    _ => text.push_str(&format!("[#-{back}]")),
                }
            }
            JsonSegment::Index(index) => {
                text.push('[');
                text.push_str(&index.to_string());
                text.push(']');
            }
        }
    }
    format!("'{}'", text.replace('\'', "''"))
}

/// Rewrites a raw fragment's portable placeholders for `dialect`: `?` binds
/// the next value (`$n` on PostgreSQL), `??` is a literal `?`, and on
/// PostgreSQL a literal `$` is escaped. Quoted text (`'...'`, `"..."`,
/// `` `...` ``) is left alone. The number of placeholders must match the
/// number of values.
pub(crate) fn raw_sql(sql: &str, values: usize, dialect: Dialect) -> Result<String, OrmError> {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut placeholders = 0;
    let mut chars = sql.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            out.push(c);
            if c == open {
                if chars.peek() == Some(&open) {
                    out.push(chars.next().unwrap_or(open));
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => {
                quote = Some(c);
                out.push(c);
            }
            '?' if chars.peek() == Some(&'?') => {
                chars.next();
                // A literal `?`: doubled for builders whose placeholder it is.
                match dialect {
                    Dialect::Postgres => out.push('?'),
                    Dialect::MySql | Dialect::Sqlite => out.push_str("??"),
                }
            }
            '?' => {
                placeholders += 1;
                match dialect {
                    Dialect::Postgres => {
                        out.push('$');
                        out.push_str(&placeholders.to_string());
                    }
                    Dialect::MySql | Dialect::Sqlite => out.push('?'),
                }
            }
            '$' if dialect == Dialect::Postgres => out.push_str("$$"),
            other => out.push(other),
        }
    }
    if placeholders != values {
        return Err(OrmError::invalid(format!(
            "the raw SQL `{sql}` has {placeholders} placeholder(s) but {values} value(s)"
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_placeholders_are_portable() {
        assert_eq!(
            raw_sql("a = ? AND b = ?", 2, Dialect::Postgres).unwrap(),
            "a = $1 AND b = $2"
        );
        assert_eq!(
            raw_sql("a = ? AND b = ?", 2, Dialect::Sqlite).unwrap(),
            "a = ? AND b = ?"
        );
        assert_eq!(
            raw_sql("data ?? 'k' AND x = ?", 1, Dialect::Postgres).unwrap(),
            "data ? 'k' AND x = $1"
        );
        assert_eq!(
            raw_sql("data ?? 'k'", 0, Dialect::MySql).unwrap(),
            "data ?? 'k'"
        );
        assert_eq!(
            raw_sql("name = '?' AND x = ?", 1, Dialect::Postgres).unwrap(),
            "name = '?' AND x = $1"
        );
        assert_eq!(
            raw_sql("price > 5$", 0, Dialect::Postgres).unwrap(),
            "price > 5$$"
        );
        assert_eq!(
            raw_sql("'it''s ?' = ?", 1, Dialect::Sqlite).unwrap(),
            "'it''s ?' = ?"
        );
        let error = raw_sql("a = ?", 2, Dialect::Sqlite)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "invalid query: the raw SQL `a = ?` has 1 placeholder(s) but 2 value(s)"
        );
    }

    #[test]
    fn json_paths_are_quoted_and_escaped() {
        let path = [
            JsonSegment::Key("a".into()),
            JsonSegment::Index(2),
            JsonSegment::Key("it's \"x\"".into()),
        ];
        assert_eq!(
            json_path_literal(&path, Dialect::MySql),
            r#"'$."a"[2]."it''s \"x\""'"#
        );
        let from_end = [JsonSegment::Index(-1), JsonSegment::Index(-3)];
        assert_eq!(
            json_path_literal(&from_end, Dialect::MySql),
            "'$[last][last-2]'"
        );
        assert_eq!(
            json_path_literal(&from_end, Dialect::Sqlite),
            "'$[#-1][#-3]'"
        );
        assert_eq!(
            postgres_path_segment(&JsonSegment::Key("o'k".into())),
            "'o''k'"
        );
    }

    #[test]
    fn identifiers() {
        assert!(is_identifier("time_bucket"));
        assert!(is_identifier("public.my_fn"));
        assert!(!is_identifier("drop table x; --"));
        assert!(!is_identifier("1abc"));
        assert!(!is_identifier(""));
    }
}
