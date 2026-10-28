//! SQL for the `postgres` engine (also SQLite) and the `pgvector` store:
//! statements with numbered or positional placeholders, and filters on a
//! JSON column of document fields.

use sea_orm::{DbBackend, Statement, Value as SqlValue};
use serde_json::Value;

use crate::engine::SortBy;
use crate::filter::{CmpOp, Filter};

/// A statement being built: SQL text and its bound values.
pub(crate) struct Sql {
    backend: DbBackend,
    text: String,
    values: Vec<SqlValue>,
}

impl Sql {
    pub(crate) fn new(backend: DbBackend) -> Self {
        Self {
            backend,
            text: String::new(),
            values: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, sql: &str) -> &mut Self {
        self.text.push_str(sql);
        self
    }

    /// Appends a placeholder bound to `value`.
    pub(crate) fn bind(&mut self, value: impl Into<SqlValue>) -> &mut Self {
        self.values.push(value.into());
        match self.backend {
            DbBackend::Postgres => {
                let n = self.values.len();
                self.text.push_str(&format!("${n}"));
            }
            _ => self.text.push('?'),
        }
        self
    }

    pub(crate) fn is_postgres(&self) -> bool {
        self.backend == DbBackend::Postgres
    }

    pub(crate) fn statement(self) -> Statement {
        Statement::from_sql_and_values(self.backend, self.text, self.values)
    }

    #[cfg(any(test, feature = "fuzzing"))]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// How many values are bound so far.
    #[cfg(feature = "fuzzing")]
    pub(crate) fn bound(&self) -> usize {
        self.values.len()
    }

    /// Appends `filter` (normalized) on the JSON document fields in
    /// `column` (`jsonb` on PostgreSQL, JSON text on SQLite).
    pub(crate) fn filter(&mut self, filter: &Filter, column: &str) -> &mut Self {
        match filter {
            Filter::Const(true) => {
                self.push("1 = 1");
            }
            Filter::Const(false) => {
                self.push("1 = 0");
            }
            Filter::All(items) | Filter::Any(items) => {
                let joiner = if matches!(filter, Filter::All(_)) {
                    " AND "
                } else {
                    " OR "
                };
                self.push("(");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.push(joiner);
                    }
                    self.filter(item, column);
                }
                self.push(")");
            }
            Filter::Not(inner) => {
                self.push("NOT ");
                self.filter(inner, column);
            }
            Filter::Null(field) => {
                let expr = self.field(column, field);
                self.push(&self.null_test(&expr, true));
            }
            Filter::NotNull(field) => {
                let expr = self.field(column, field);
                self.push(&self.null_test(&expr, false));
            }
            Filter::In { field, values } | Filter::NotIn { field, values } => {
                let negated = matches!(filter, Filter::NotIn { .. });
                let expr = self.field(column, field);
                self.push("(");
                if negated {
                    let null = self.null_test(&expr, true);
                    self.push(&null).push(" OR NOT ");
                }
                self.push("(");
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        self.push(" OR ");
                    }
                    self.equals(column, field, &expr, value);
                }
                self.push("))");
            }
            Filter::Cmp { field, op, value } => {
                let expr = self.field(column, field);
                match op {
                    CmpOp::Eq => {
                        self.equals(column, field, &expr, value);
                    }
                    CmpOp::Ne => {
                        let null = self.null_test(&expr, true);
                        self.push("(").push(&null).push(" OR NOT ");
                        self.equals(column, field, &expr, value);
                        self.push(")");
                    }
                    CmpOp::Lt | CmpOp::Lte | CmpOp::Gt | CmpOp::Gte => {
                        let operator = match op {
                            CmpOp::Lt => " < ",
                            CmpOp::Lte => " <= ",
                            CmpOp::Gt => " > ",
                            _ => " >= ",
                        };
                        self.push("(");
                        self.type_test(column, field, &expr, value);
                        self.push(" AND ").push(&expr).push(operator);
                        self.operand(value);
                        self.push(")");
                    }
                }
            }
        }
        self
    }

    /// `ORDER BY` terms for `sort` (documents without a value last).
    pub(crate) fn order(&mut self, sort: &[SortBy], column: &str) -> &mut Self {
        for key in sort {
            let expr = self.field(column, &key.field);
            let null = self.null_test(&expr, true);
            let direction = if key.descending { "DESC" } else { "ASC" };
            self.push(&format!("({null}), {expr} {direction}, "));
        }
        self
    }

    /// The expression for field `field` of the JSON in `column`. Field
    /// names are `[A-Za-z0-9_]` (checked by `IndexSettings::validate`).
    fn field(&self, column: &str, field: &str) -> String {
        debug_assert!(crate::settings::valid_name(field));
        if self.is_postgres() {
            format!("{column}->'{field}'")
        } else {
            format!("json_extract({column}, '$.\"{field}\"')")
        }
    }

    fn json_type(&self, column: &str, field: &str) -> String {
        if self.is_postgres() {
            format!("jsonb_typeof({column}->'{field}')")
        } else {
            format!("json_type({column}, '$.\"{field}\"')")
        }
    }

    fn null_test(&self, expr: &str, null: bool) -> String {
        match (self.is_postgres(), null) {
            (true, true) => format!("({expr} IS NULL OR {expr} = 'null'::jsonb)"),
            (true, false) => format!("({expr} IS NOT NULL AND {expr} <> 'null'::jsonb)"),
            (false, true) => format!("{expr} IS NULL"),
            (false, false) => format!("{expr} IS NOT NULL"),
        }
    }

    /// `expr = value`, strict about JSON types.
    fn equals(&mut self, column: &str, field: &str, expr: &str, value: &Value) {
        if self.is_postgres() {
            self.push(expr).push(" = ");
            self.operand(value);
            return;
        }
        match value {
            Value::Bool(b) => {
                let ty = self.json_type(column, field);
                self.push(&format!("{ty} = '{b}'"));
            }
            _ => {
                self.push("(");
                self.type_test(column, field, expr, value);
                self.push(" AND ").push(expr).push(" = ");
                self.operand(value);
                self.push(")");
            }
        }
    }

    /// The field holds a JSON value of `value`'s type.
    fn type_test(&mut self, column: &str, field: &str, _expr: &str, value: &Value) {
        let ty = self.json_type(column, field);
        let test = match (self.is_postgres(), value) {
            (true, Value::Number(_)) => format!("{ty} = 'number'"),
            (true, Value::String(_)) => format!("{ty} = 'string'"),
            (true, Value::Bool(_)) => format!("{ty} = 'boolean'"),
            (false, Value::Number(_)) => format!("{ty} IN ('integer', 'real')"),
            (false, Value::String(_)) => format!("{ty} = 'text'"),
            (false, Value::Bool(_)) => format!("{ty} IN ('true', 'false')"),
            _ => "1 = 0".to_owned(),
        };
        self.push(&test);
    }

    /// The bound operand: `$n::jsonb` on PostgreSQL, a typed value on SQLite.
    fn operand(&mut self, value: &Value) {
        if self.is_postgres() {
            self.bind(value.to_string()).push("::jsonb");
            return;
        }
        match value {
            Value::Bool(b) => self.bind(i64::from(*b)),
            Value::Number(n) => match n.as_i64() {
                Some(i) => self.bind(i),
                None => self.bind(n.as_f64().unwrap_or_default()),
            },
            Value::String(s) => self.bind(s.clone()),
            other => self.bind(other.to_string()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render(backend: DbBackend, filter: Filter) -> (String, usize) {
        let mut sql = Sql::new(backend);
        sql.filter(&filter.normalized(), "d.fields");
        let n = sql.values.len();
        (sql.text().to_owned(), n)
    }

    #[test]
    fn postgres_filters_compare_jsonb() {
        let (text, n) = render(
            DbBackend::Postgres,
            Filter::All(vec![
                Filter::eq("stage", "won"),
                Filter::range("amount", 100..),
                Filter::ne("owner", "u1"),
            ]),
        );
        assert_eq!(
            text,
            "(d.fields->'stage' = $1::jsonb AND (jsonb_typeof(d.fields->'amount') = 'number' AND d.fields->'amount' >= $2::jsonb) \
             AND ((d.fields->'owner' IS NULL OR d.fields->'owner' = 'null'::jsonb) OR NOT d.fields->'owner' = $3::jsonb))"
        );
        assert_eq!(n, 3);
    }

    #[test]
    fn sqlite_filters_check_json_types() {
        let (text, n) = render(
            DbBackend::Sqlite,
            Filter::Any(vec![Filter::eq("flag", true), Filter::null("closed_at")]),
        );
        assert_eq!(
            text,
            "(json_type(d.fields, '$.\"flag\"') = 'true' OR json_extract(d.fields, '$.\"closed_at\"') IS NULL)"
        );
        assert_eq!(n, 0);
        let (text, _) = render(
            DbBackend::Sqlite,
            Filter::is_in("s", [json!("a"), json!(2)]),
        );
        assert_eq!(
            text,
            "(((json_type(d.fields, '$.\"s\"') = 'text' AND json_extract(d.fields, '$.\"s\"') = ?) OR \
             (json_type(d.fields, '$.\"s\"') IN ('integer', 'real') AND json_extract(d.fields, '$.\"s\"') = ?)))"
        );
    }

    #[test]
    fn order_puts_missing_values_last() {
        let mut sql = Sql::new(DbBackend::Sqlite);
        sql.order(
            &[SortBy {
                field: "amount".into(),
                descending: true,
            }],
            "d.fields",
        );
        assert_eq!(
            sql.text(),
            "(json_extract(d.fields, '$.\"amount\"') IS NULL), json_extract(d.fields, '$.\"amount\"') DESC, "
        );
    }
}
