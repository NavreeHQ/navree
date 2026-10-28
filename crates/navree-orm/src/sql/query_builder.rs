//! Conditions built with sea-query ([`Where::Sql`](crate::ir::Where::Sql),
//! `Filter::from(sea_query::Condition)`) as raw fragments the renderer
//! writes into its statements.
//!
//! Transitional: the condition is written by sea-query's builder for the
//! dialect, and its placeholders made portable (`?`, with a literal `?` as
//! `??`), so it binds its values in the renderer's order. It goes with
//! `Where::Sql` once navree-access builds its filters without sea-query
//! (data-layer move, step 5).

use sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};

use super::Dialect;
use crate::error::{OrmError, Result};
use crate::value::Value;

/// `condition` as a raw fragment for `dialect`: SQL with portable `?`
/// placeholders, and its values in order. An empty condition holds for
/// every row (`1 = 1`).
pub(crate) fn condition_fragment(
    condition: &sea_query::Condition,
    dialect: Dialect,
) -> Result<(String, Vec<Value>)> {
    let mut select = sea_query::Query::select();
    select
        .expr(sea_query::Expr::cust("1"))
        .cond_where(condition.clone());
    let (sql, values) = match dialect {
        Dialect::Postgres => select.build_any(&PostgresQueryBuilder),
        Dialect::MySql => select.build_any(&MysqlQueryBuilder),
        Dialect::Sqlite => select.build_any(&SqliteQueryBuilder),
    };
    let values = values.0;
    let Some(sql) = sql.strip_prefix("SELECT 1 WHERE ") else {
        return if sql == "SELECT 1" && values.is_empty() {
            Ok(("1 = 1".to_owned(), values))
        } else {
            Err(OrmError::invalid(format!(
                "a sea-query condition wrote unexpected SQL: {sql}"
            )))
        };
    };
    let sql = match dialect {
        Dialect::Postgres => portable_postgres(sql, values.len())?,
        // `?` placeholders already; quoted text is left alone by the
        // renderer's scanner as by the driver's.
        Dialect::MySql | Dialect::Sqlite => sql.to_owned(),
    };
    Ok((sql, values))
}

/// PostgreSQL's `$1 .. $n` (in order) as portable `?`, and a literal `?`
/// (the JSON key operator) as `??`; quoted text and identifiers are left
/// alone.
fn portable_postgres(sql: &str, values: usize) -> Result<String> {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut quote: Option<char> = None;
    let mut next = 1;
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            out.push(c);
            if c == open {
                if chars.peek() == Some(&open) {
                    out.extend(chars.next());
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                out.push(c);
            }
            '$' if chars.peek().is_some_and(char::is_ascii_digit) => {
                let mut digits = String::new();
                while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                    digits.push(d);
                    chars.next();
                }
                if digits.parse::<usize>().ok() != Some(next) {
                    return Err(OrmError::invalid(format!(
                        "a sea-query condition binds its values out of order: {sql}"
                    )));
                }
                next += 1;
                out.push('?');
            }
            '?' => out.push_str("??"),
            other => out.push(other),
        }
    }
    if next - 1 != values {
        return Err(OrmError::invalid(format!(
            "a sea-query condition has {} placeholder(s) but {values} value(s): {sql}",
            next - 1
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use sea_query::{Alias, Condition, Expr, ExprTrait};

    use super::*;

    fn condition() -> Condition {
        Condition::any()
            .add(Expr::col((Alias::new("deals"), Alias::new("stage"))).eq("won"))
            .add(Expr::cust("meta ? 'vip?'"))
            .add(Expr::col(Alias::new("n")).gt(3))
    }

    #[test]
    fn conditions_become_portable_fragments() {
        let (sql, values) = condition_fragment(&condition(), Dialect::Postgres).unwrap();
        assert_eq!(sql, r#""deals"."stage" = ? OR (meta ?? 'vip?') OR "n" > ?"#);
        assert_eq!(values, [Value::from("won"), Value::from(3)]);
        let (sql, _) = condition_fragment(&condition(), Dialect::MySql).unwrap();
        assert!(sql.starts_with("`deals`.`stage` = ?"), "{sql}");
        let (sql, values) = condition_fragment(&Condition::all(), Dialect::Sqlite).unwrap();
        assert_eq!((sql.as_str(), values.len()), ("TRUE", 0));
    }
}
