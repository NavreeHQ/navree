//! `WHERE`, `HAVING` and join conditions, with the dialect rules for date
//! parts, LIKE, null-safe equality, regular expressions, JSON and full
//! text.

use std::borrow::Cow;

use chrono::{Datelike, Timelike};

use super::{Lowerer, normalize_operator};
use crate::error::{OrmError, Result};
use crate::ir::{
    Boolean, ColumnRef, DatePart, Expr, FullText, FullTextMode, InValues, JsonSegment, Operator,
    QueryIr, SelectItem, Where, WhereClause,
};
use crate::render::ast::{self, BinaryOp, LikeCase, SqlType};
use crate::sql::Dialect;
use crate::value::{Field, Value, to_json};

/// An `EXISTS` subquery selects `1`: its columns do not matter (unless a
/// union needs them to line up).
pub(crate) fn exists_select_list(query: &mut QueryIr) {
    if query.unions.is_empty() {
        query.columns = vec![SelectItem {
            expr: Expr::Raw {
                sql: "1".into(),
                values: Vec::new(),
            },
            alias: None,
        }];
    }
}

/// The text a JSON value has in a document, as PostgreSQL's `->>` gives
/// it: `3`, `true`, `{"a":1}`.
fn json_text_of(value: &Value) -> Value {
    match to_json(value) {
        serde_json::Value::String(text) => Value::from(text),
        serde_json::Value::Null => Value::String(None),
        other => Value::from(other.to_string()),
    }
}

impl Lowerer {
    /// A list of clauses with SQL's precedence: `a AND b OR c AND d` is
    /// `(a AND b) OR (c AND d)`. The first clause's boolean is ignored.
    pub(crate) fn clauses(&self, clauses: &[WhereClause]) -> Result<ast::Expr> {
        let mut groups: Vec<Vec<ast::Expr>> = vec![Vec::new()];
        for (i, clause) in clauses.iter().enumerate() {
            let mut condition = self.where_(&clause.filter)?;
            if clause.negated {
                condition = condition.not();
            }
            if i > 0 && clause.boolean == Boolean::Or {
                groups.push(Vec::new());
            }
            if let Some(group) = groups.last_mut() {
                group.push(condition);
            }
        }
        let junction = |mut items: Vec<ast::Expr>| {
            if items.len() == 1 {
                items.remove(0)
            } else {
                ast::Expr::And(items)
            }
        };
        Ok(if groups.len() == 1 {
            junction(groups.remove(0))
        } else {
            ast::Expr::Or(groups.into_iter().map(junction).collect())
        })
    }

    /// One condition.
    pub(crate) fn where_(&self, filter: &Where) -> Result<ast::Expr> {
        Ok(match filter {
            Where::Compare { left, op, right } => self.compare(left, op, right)?,
            Where::Null { expr, not } => {
                let expr = self.expr(expr)?;
                if *not {
                    expr.is_not_null()
                } else {
                    expr.is_null()
                }
            }
            Where::In { expr, values, not } => self.in_(expr, values, *not)?,
            Where::Between {
                expr,
                low,
                high,
                not,
            } => ast::Expr::Between {
                expr: Box::new(self.expr(expr)?),
                low: Box::new(self.expr(low)?),
                high: Box::new(self.expr(high)?),
                negated: *not,
            },
            Where::Like {
                expr,
                pattern,
                case_sensitive,
                not,
            } => self.like(expr, pattern, *case_sensitive, *not)?,
            Where::IgnoreCase { expr, text, mode } => self.ignore_case(expr, text, *mode)?,
            Where::DatePart {
                part,
                expr,
                op,
                value,
            } => self.date_part(*part, expr, op, value)?,
            Where::Exists { query, not } => {
                let mut query = (**query).clone();
                query.orders.clear();
                exists_select_list(&mut query);
                ast::Expr::Exists {
                    query: Box::new(self.select(&query)?),
                    negated: *not,
                }
            }
            Where::Group(clauses) => {
                if clauses.is_empty() {
                    return Ok(ast::Expr::And(Vec::new()));
                }
                self.clauses(clauses)?
            }
            Where::JsonContains { column, value, not } => {
                negate(self.json_contains(column, value)?, *not)
            }
            Where::JsonLength { column, op, length } => self.json_length(column, op, *length)?,
            Where::JsonContainedBy { column, value, not } => {
                negate(self.json_contained_by(column, value)?, *not)
            }
            Where::JsonOverlaps { column, value, not } => {
                negate(self.json_overlaps(column, value)?, *not)
            }
            Where::JsonContainsKey { column, not } => negate(self.json_contains_key(column)?, *not),
            Where::FullText {
                columns,
                query,
                options,
            } => self.full_text(columns, query, options)?,
            // Parenthesized beside other conditions and under `NOT`, so an
            // `OR` inside it cannot regroup them.
            Where::Raw { sql, values } => self.raw(sql.clone(), values)?,
            // Written by the query builder that built it, as a raw
            // fragment (until navree-access builds its filters as trees).
            Where::Sql(condition) => {
                let (sql, values) = crate::sql::condition_fragment(condition, self.dialect)?;
                self.raw(Cow::Owned(sql), &values)?
            }
            Where::Tree(condition) => (**condition).clone(),
            Where::Native(native) => {
                return Err(self.unsupported(format!("a {} filter", native.driver())));
            }
            Where::Invalid(message) => return Err(OrmError::invalid(message.clone())),
        })
    }

    fn compare(&self, left: &Expr, op: &Operator, right: &Expr) -> Result<ast::Expr> {
        let l = self.expr(left)?;
        let r = match (self.dialect, left, right) {
            // PostgreSQL's `->>` gives text, and a bound number or boolean
            // would not compare with text: compare with the value's JSON
            // text instead (`3`, `true`).
            (Dialect::Postgres, Expr::Column(column), Expr::Value(value))
                if !column.path.is_empty() && !matches!(value, Value::String(_)) =>
            {
                ast::Expr::Value(json_text_of(value))
            }
            _ => self.expr(right)?,
        };
        let custom = |op: &'static str| BinaryOp::Custom(Cow::Borrowed(op));
        Ok(match op {
            Operator::Eq => l.eq(r),
            Operator::Ne => l.ne(r),
            Operator::Lt => l.lt(r),
            Operator::Lte => l.lte(r),
            Operator::Gt => l.gt(r),
            Operator::Gte => l.gte(r),
            Operator::Like => self.like_expr(self.text(l), r, false, LikeCase::Database),
            Operator::NotLike => self.like_expr(self.text(l), r, true, LikeCase::Database),
            Operator::ILike => self.ilike(l, r, false),
            Operator::NotILike => self.ilike(l, r, true),
            Operator::NullSafeEq => l.binary(BinaryOp::NullSafeEq, r),
            Operator::NullSafeNe => l.binary(BinaryOp::NullSafeNotEq, r),
            Operator::BitAnd => l.binary(BinaryOp::BitAnd, r),
            Operator::BitOr => l.binary(BinaryOp::BitOr, r),
            Operator::BitXor => l.binary(BinaryOp::BitXor, r),
            Operator::ShiftLeft => l.binary(BinaryOp::ShiftLeft, r),
            Operator::ShiftRight => l.binary(BinaryOp::ShiftRight, r),
            Operator::Regexp | Operator::NotRegexp => {
                let not = matches!(op, Operator::NotRegexp);
                match self.dialect {
                    Dialect::Postgres => l.binary(custom(if not { "!~" } else { "~" }), r),
                    // `REGEXP` follows the column's collation (usually
                    // case-insensitive); the `c` flag matches case.
                    Dialect::MySql => negate(
                        ast::Expr::func("REGEXP_LIKE", vec![l, r, ast::Expr::string("c")]),
                        not,
                    ),
                    Dialect::Sqlite => {
                        l.binary(custom(if not { "NOT REGEXP" } else { "REGEXP" }), r)
                    }
                }
            }
            Operator::RegexpIgnoreCase | Operator::NotRegexpIgnoreCase => {
                let not = matches!(op, Operator::NotRegexpIgnoreCase);
                match self.dialect {
                    Dialect::Postgres => l.binary(custom(if not { "!~*" } else { "~*" }), r),
                    Dialect::MySql => negate(
                        ast::Expr::func("REGEXP_LIKE", vec![l, r, ast::Expr::string("i")]),
                        not,
                    ),
                    // The `regexp` function SQLite calls is the app's (sqlx
                    // registers one with `with_regexp`); `(?i)` turns case off.
                    Dialect::Sqlite => l.binary(
                        custom(if not { "NOT REGEXP" } else { "REGEXP" }),
                        ast::Expr::string("(?i)").binary(BinaryOp::Concat, r),
                    ),
                }
            }
            Operator::Postgres(op) => match self.dialect {
                Dialect::Postgres => l.binary(custom(op), r),
                _ => return Err(self.unsupported(format!("the operator `{op}`"))),
            },
            Operator::Raw(op) => {
                let allowed = condition_operators(self.dialect);
                let normalized = normalize_operator(op);
                match allowed.iter().find(|known| **known == normalized) {
                    Some(known) => l.binary(custom(known), r),
                    None => {
                        return Err(OrmError::invalid(format!(
                            "`{op}` is not an operator {} conditions take (they take: {})",
                            self.dialect.name(),
                            allowed.join(", ")
                        )));
                    }
                }
            }
            Operator::Invalid(op) => {
                return Err(OrmError::invalid(format!(
                    "`{op}` is not a known operator (use =, <>, <, <=, >, >=, like, not like, ilike, not ilike, <=>, &, |, ^, <<, >>, regexp, not regexp)"
                )));
            }
        })
    }

    /// An expression compared as text (PostgreSQL's `LIKE` needs text on
    /// the left; UUID and number columns are cast).
    fn text(&self, expr: ast::Expr) -> ast::Expr {
        match self.dialect {
            Dialect::Postgres => expr.cast(SqlType::Text),
            Dialect::MySql | Dialect::Sqlite => expr,
        }
    }

    fn like_expr(
        &self,
        expr: ast::Expr,
        pattern: ast::Expr,
        negated: bool,
        case: LikeCase,
    ) -> ast::Expr {
        ast::Expr::Like {
            expr: Box::new(expr),
            pattern: Box::new(pattern),
            escape: None,
            negated,
            case,
        }
    }

    /// Case-insensitive `LIKE`: `ILIKE` on PostgreSQL, `LOWER(..) LIKE
    /// LOWER(..)` elsewhere.
    fn ilike(&self, left: ast::Expr, right: ast::Expr, not: bool) -> ast::Expr {
        match self.dialect {
            Dialect::Postgres => self.like_expr(self.text(left), right, not, LikeCase::Ignore),
            Dialect::MySql | Dialect::Sqlite => self.like_expr(
                ast::Expr::func("LOWER", vec![left]),
                ast::Expr::func("LOWER", vec![right]),
                not,
                LikeCase::Database,
            ),
        }
    }

    fn like(
        &self,
        expr: &Expr,
        pattern: &Expr,
        case_sensitive: bool,
        not: bool,
    ) -> Result<ast::Expr> {
        let left = self.expr(expr)?;
        Ok(match (self.dialect, case_sensitive) {
            (Dialect::Postgres, false) => {
                let right = self.expr(pattern)?;
                self.ilike(left, right, not)
            }
            (Dialect::Postgres, true) | (Dialect::MySql | Dialect::Sqlite, false) => {
                let right = self.expr(pattern)?;
                self.like_expr(self.text(left), right, not, LikeCase::Database)
            }
            (Dialect::MySql, true) => {
                let right = self.expr(pattern)?;
                self.like_expr(left, right, not, LikeCase::Match)
            }
            (Dialect::Sqlite, true) => match pattern {
                // SQLite's LIKE ignores ASCII case; GLOB matches case, with
                // its own wildcards: a pattern given as text is converted
                // here, any other in SQL.
                Expr::Value(Value::String(Some(text))) => left.binary(
                    BinaryOp::Custom(Cow::Borrowed(if not { "NOT GLOB" } else { "GLOB" })),
                    ast::Expr::Value(Value::from(like_to_glob(text))),
                ),
                other => {
                    let right = self.expr(other)?;
                    self.like_expr(left, right, not, LikeCase::Match)
                }
            },
        })
    }

    fn in_(&self, expr: &Expr, values: &InValues, not: bool) -> Result<ast::Expr> {
        let left = self.expr(expr)?;
        Ok(match values {
            // `IN ()` is not SQL: the grammar writes an empty list as
            // `1 = 0` (`1 = 1` negated).
            InValues::List(list) => ast::Expr::InList {
                expr: Box::new(left),
                list: list
                    .iter()
                    .map(|value| self.expr(value))
                    .collect::<Result<Vec<_>>>()?,
                negated: not,
            },
            InValues::Subquery(query) => ast::Expr::InQuery {
                expr: Box::new(left),
                query: Box::new(self.select(query)?),
                negated: not,
            },
        })
    }

    fn date_part(
        &self,
        part: DatePart,
        expr: &Expr,
        op: &Operator,
        value: &Expr,
    ) -> Result<ast::Expr> {
        let column = self.expr(expr)?;
        let value = self.date_part_value(part, value)?;
        let function = |name: &'static str, column: ast::Expr| ast::Expr::func(name, vec![column]);
        let strftime = |format: &'static str, column: ast::Expr| {
            ast::Expr::func("strftime", vec![ast::Expr::string(format), column])
        };
        let extract = |field: &'static str, column: ast::Expr| {
            ast::Expr::template(format!("EXTRACT({field} FROM {{}})"), vec![column])
        };
        let left = match (self.dialect, part) {
            (Dialect::Postgres, DatePart::Date) => column.cast(SqlType::Date),
            (Dialect::Postgres, DatePart::Time) => column.cast(SqlType::Time),
            (Dialect::Postgres, DatePart::Day) => extract("DAY", column),
            (Dialect::Postgres, DatePart::Month) => extract("MONTH", column),
            (Dialect::Postgres, DatePart::Year) => extract("YEAR", column),
            (Dialect::MySql, DatePart::Date) => function("DATE", column),
            (Dialect::MySql, DatePart::Time) => function("TIME", column),
            (Dialect::MySql, DatePart::Day) => function("DAY", column),
            (Dialect::MySql, DatePart::Month) => function("MONTH", column),
            (Dialect::MySql, DatePart::Year) => function("YEAR", column),
            (Dialect::Sqlite, DatePart::Date) => strftime("%Y-%m-%d", column),
            (Dialect::Sqlite, DatePart::Time) => strftime("%H:%M:%S", column),
            (Dialect::Sqlite, DatePart::Day) => strftime("%d", column),
            (Dialect::Sqlite, DatePart::Month) => strftime("%m", column),
            (Dialect::Sqlite, DatePart::Year) => strftime("%Y", column),
        };
        let right = match (self.dialect, part) {
            (Dialect::Postgres, DatePart::Date) => value.cast(SqlType::Date),
            (Dialect::Postgres, DatePart::Time) => value.cast(SqlType::Time),
            _ => value,
        };
        compare_compiled(left, op, right)
    }

    /// The value a date part is compared with: numbers for day, month and
    /// year (zero-padded text on SQLite, whose `strftime` returns text),
    /// `YYYY-MM-DD` / `HH:MM:SS` text for dates and times on SQLite.
    fn date_part_value(&self, part: DatePart, value: &Expr) -> Result<ast::Expr> {
        let Expr::Value(value) = value else {
            return self.expr(value);
        };
        let normalized = match part {
            DatePart::Day | DatePart::Month | DatePart::Year => {
                let number = match value {
                    Value::ChronoDate(Some(date)) => i64::from(match part {
                        DatePart::Day => date.day() as i32,
                        DatePart::Month => date.month() as i32,
                        _ => date.year(),
                    }),
                    Value::ChronoDateTime(Some(at)) => i64::from(match part {
                        DatePart::Day => at.day() as i32,
                        DatePart::Month => at.month() as i32,
                        _ => at.year(),
                    }),
                    Value::ChronoDateTimeUtc(Some(at)) => i64::from(match part {
                        DatePart::Day => at.day() as i32,
                        DatePart::Month => at.month() as i32,
                        _ => at.year(),
                    }),
                    other => i64::from_value(other.clone()).map_err(|error| {
                        OrmError::invalid(format!("a {part:?} must be a number: {error}"))
                    })?,
                };
                match self.dialect {
                    Dialect::Sqlite if part == DatePart::Year => {
                        Value::from(format!("{number:04}"))
                    }
                    Dialect::Sqlite => Value::from(format!("{number:02}")),
                    Dialect::Postgres | Dialect::MySql => Value::from(number),
                }
            }
            DatePart::Date => match (self.dialect, value) {
                (Dialect::Sqlite, Value::ChronoDate(Some(date))) => {
                    Value::from(date.format("%Y-%m-%d").to_string())
                }
                (Dialect::Sqlite, Value::ChronoDateTime(Some(at))) => {
                    Value::from(at.format("%Y-%m-%d").to_string())
                }
                (Dialect::Sqlite, Value::ChronoDateTimeUtc(Some(at))) => {
                    Value::from(at.format("%Y-%m-%d").to_string())
                }
                _ => value.clone(),
            },
            DatePart::Time => match (self.dialect, value) {
                (Dialect::Sqlite, Value::ChronoTime(Some(time))) => Value::from(format!(
                    "{:02}:{:02}:{:02}",
                    time.hour(),
                    time.minute(),
                    time.second()
                )),
                _ => value.clone(),
            },
        };
        Ok(ast::Expr::Value(normalized))
    }

    fn json_contains(&self, column: &ColumnRef, value: &serde_json::Value) -> Result<ast::Expr> {
        Ok(match self.dialect {
            Dialect::Postgres => self.json_value(column).cast(SqlType::Json).binary(
                BinaryOp::Custom(Cow::Borrowed("@>")),
                ast::Expr::Value(Value::Json(Some(Box::new(value.clone())))),
            ),
            Dialect::MySql => {
                let mut args = vec![
                    self.plain_column(column),
                    ast::Expr::Value(Value::from(value.to_string())),
                ];
                if !column.path.is_empty() {
                    args.push(self.json_path(&column.path));
                }
                ast::Expr::func("json_contains", args)
            }
            Dialect::Sqlite => {
                let scalar = match value {
                    serde_json::Value::String(text) => Value::from(text.clone()),
                    serde_json::Value::Bool(flag) => Value::from(i64::from(*flag)),
                    serde_json::Value::Number(number) => match number.as_i64() {
                        Some(int) => Value::from(int),
                        None => Value::from(number.as_f64().unwrap_or_default()),
                    },
                    serde_json::Value::Null => Value::String(None),
                    _ => {
                        return Err(self
                            .unsupported("a JSON containment check for an object or array value"));
                    }
                };
                let elements = ast::Select::from_source(ast::Source::Function {
                    call: Box::new(self.sqlite_json_each(column)),
                    alias: None,
                })
                .column(ast::Expr::int(1))
                .filter(
                    ast::Expr::qualified(ast::ObjectName::single("json_each"), "value")
                        .binary(BinaryOp::NullSafeEq, ast::Expr::Value(scalar)),
                );
                ast::Expr::exists(elements)
            }
        })
    }

    /// The JSON at `column` is contained in `value` (`<@` on PostgreSQL,
    /// `json_contains(value, column)` on MySQL).
    fn json_contained_by(
        &self,
        column: &ColumnRef,
        value: &serde_json::Value,
    ) -> Result<ast::Expr> {
        Ok(match self.dialect {
            Dialect::Postgres => self.json_value(column).cast(SqlType::Json).binary(
                BinaryOp::Custom(Cow::Borrowed("<@")),
                ast::Expr::Value(Value::Json(Some(Box::new(value.clone())))),
            ),
            Dialect::MySql => ast::Expr::func(
                "json_contains",
                vec![
                    ast::Expr::Value(Value::from(value.to_string())).cast(SqlType::Json),
                    self.mysql_json(column),
                ],
            ),
            Dialect::Sqlite => return Err(self.unsupported("a JSON contained-by check")),
        })
    }

    /// The JSON array at `column` shares an element with `value`: on
    /// PostgreSQL the column contains one of the elements (`@>`, so any
    /// element type works), MySQL's `json_overlaps`, on SQLite the elements
    /// of both compared through `json_each`.
    fn json_overlaps(&self, column: &ColumnRef, value: &serde_json::Value) -> Result<ast::Expr> {
        let elements = match value {
            serde_json::Value::Array(_) => value.clone(),
            scalar => serde_json::Value::Array(vec![scalar.clone()]),
        };
        Ok(match self.dialect {
            Dialect::Postgres => {
                let candidates = ast::Select::from_source(ast::Source::function(
                    ast::Expr::func(
                        "jsonb_array_elements",
                        vec![ast::Expr::Value(Value::Json(Some(Box::new(elements))))],
                    ),
                    "navree_candidate",
                ))
                .column(ast::Expr::int(1))
                .filter(self.json_value(column).cast(SqlType::Json).binary(
                    BinaryOp::Custom(Cow::Borrowed("@>")),
                    ast::Expr::func(
                        "jsonb_build_array",
                        vec![ast::Expr::qualified(
                            ast::ObjectName::single("navree_candidate"),
                            "value",
                        )],
                    ),
                ));
                ast::Expr::exists(candidates)
            }
            Dialect::MySql => ast::Expr::func(
                "json_overlaps",
                vec![
                    self.mysql_json(column),
                    ast::Expr::Value(Value::from(elements.to_string())).cast(SqlType::Json),
                ],
            ),
            Dialect::Sqlite => {
                let pairs = ast::Select {
                    items: vec![ast::SelectItem::new(ast::Expr::int(1))],
                    from: vec![
                        ast::Source::function(self.sqlite_json_each(column), "navree_left"),
                        ast::Source::function(
                            ast::Expr::func(
                                "json_each",
                                vec![ast::Expr::Value(Value::from(elements.to_string()))],
                            ),
                            "navree_right",
                        ),
                    ],
                    ..ast::Select::default()
                }
                .filter(
                    ast::Expr::qualified(ast::ObjectName::single("navree_left"), "value").eq(
                        ast::Expr::qualified(ast::ObjectName::single("navree_right"), "value"),
                    ),
                );
                ast::Expr::exists(pairs)
            }
        })
    }

    /// The JSON column has the key (or array index) its path ends with.
    fn json_contains_key(&self, column: &ColumnRef) -> Result<ast::Expr> {
        let Some((last, parent_path)) = column.path.split_last() else {
            return Err(OrmError::invalid(format!(
                "a JSON key check needs a path to the key (`{}->key`), not the column alone",
                column.name
            )));
        };
        Ok(match self.dialect {
            Dialect::Postgres => {
                let parent = self
                    .json_value(&ColumnRef {
                        path: parent_path.to_vec(),
                        ..column.clone()
                    })
                    .cast(SqlType::Json);
                match last {
                    JsonSegment::Key(key) => ast::Expr::func(
                        "COALESCE",
                        vec![
                            parent.binary(
                                BinaryOp::Custom(Cow::Borrowed("?")),
                                ast::Expr::Value(Value::from(key.clone())),
                            ),
                            ast::Expr::bool(false),
                        ],
                    ),
                    JsonSegment::Index(index) => {
                        // The array is long enough to have the index (from
                        // the end when negative).
                        let needed = if *index < 0 { -index } else { index + 1 };
                        ast::Expr::case(
                            vec![(
                                ast::Expr::func("jsonb_typeof", vec![parent.clone()])
                                    .eq(ast::Expr::string("array")),
                                ast::Expr::func("jsonb_array_length", vec![parent])
                                    .gte(ast::Expr::Value(Value::from(needed))),
                            )],
                            Some(ast::Expr::bool(false)),
                        )
                    }
                }
            }
            Dialect::MySql => ast::Expr::func(
                "IFNULL",
                vec![
                    ast::Expr::func(
                        "json_contains_path",
                        vec![
                            self.plain_column(column),
                            ast::Expr::string("one"),
                            self.json_path(&column.path),
                        ],
                    ),
                    ast::Expr::int(0),
                ],
            ),
            Dialect::Sqlite => ast::Expr::func(
                "json_type",
                vec![self.plain_column(column), self.json_path(&column.path)],
            )
            .is_not_null(),
        })
    }

    /// MySQL: the JSON at `column` (its path extracted, still JSON).
    fn mysql_json(&self, column: &ColumnRef) -> ast::Expr {
        let base = self.plain_column(column);
        if column.path.is_empty() {
            base
        } else {
            ast::Expr::func("json_extract", vec![base, self.json_path(&column.path)])
        }
    }

    /// SQLite: the elements of the JSON at `column`, as `json_each` rows.
    fn sqlite_json_each(&self, column: &ColumnRef) -> ast::Expr {
        let mut args = vec![self.plain_column(column)];
        if !column.path.is_empty() {
            args.push(self.json_path(&column.path));
        }
        ast::Expr::func("json_each", args)
    }

    fn json_length(&self, column: &ColumnRef, op: &Operator, length: i64) -> Result<ast::Expr> {
        let left = match self.dialect {
            Dialect::Postgres => ast::Expr::func(
                "jsonb_array_length",
                vec![self.json_value(column).cast(SqlType::Json)],
            ),
            Dialect::MySql | Dialect::Sqlite => {
                let mut args = vec![self.plain_column(column)];
                if !column.path.is_empty() {
                    args.push(self.json_path(&column.path));
                }
                ast::Expr::func(
                    if self.dialect == Dialect::MySql {
                        "json_length"
                    } else {
                        "json_array_length"
                    },
                    args,
                )
            }
        };
        compare_compiled(left, op, ast::Expr::Value(Value::from(length)))
    }

    fn full_text(&self, columns: &[Expr], query: &str, options: &FullText) -> Result<ast::Expr> {
        let compiled = columns
            .iter()
            .map(|column| self.expr(column))
            .collect::<Result<Vec<_>>>()?;
        let search = ast::Expr::Value(Value::from(query.to_owned()));
        match self.dialect {
            Dialect::Postgres => {
                let language = options
                    .language
                    .as_deref()
                    .unwrap_or("english")
                    .to_ascii_lowercase();
                if !POSTGRES_LANGUAGES.contains(&language.as_str()) {
                    return Err(OrmError::invalid(format!(
                        "`{language}` is not a PostgreSQL text search language (use one of {})",
                        POSTGRES_LANGUAGES.join(", ")
                    )));
                }
                let function = match options.mode {
                    FullTextMode::Phrase => "phraseto_tsquery",
                    FullTextMode::Websearch | FullTextMode::Boolean => "websearch_to_tsquery",
                    FullTextMode::Raw => "to_tsquery",
                    _ => "plainto_tsquery",
                };
                // `tsvector` columns are matched as they are.
                let vectors = compiled
                    .into_iter()
                    .map(|column| {
                        if options.vectors {
                            column
                        } else {
                            ast::Expr::func(
                                "to_tsvector",
                                vec![ast::Expr::string(language.clone()), column],
                            )
                        }
                    })
                    .reduce(|all, next| all.binary(BinaryOp::Concat, next))
                    .ok_or_else(|| OrmError::invalid("a full-text condition names no column"))?;
                Ok(vectors.binary(
                    BinaryOp::Custom(Cow::Borrowed("@@")),
                    ast::Expr::func(function, vec![ast::Expr::string(language), search]),
                ))
            }
            Dialect::MySql => {
                if compiled.is_empty() {
                    return Err(OrmError::invalid("a full-text condition names no column"));
                }
                let mode = match options.mode {
                    FullTextMode::Boolean | FullTextMode::Websearch | FullTextMode::Raw => {
                        "IN BOOLEAN MODE"
                    }
                    _ if options.expanded => "IN NATURAL LANGUAGE MODE WITH QUERY EXPANSION",
                    _ => "IN NATURAL LANGUAGE MODE",
                };
                let markers = vec!["{}"; compiled.len()].join(", ");
                let mut args = compiled;
                args.push(search);
                Ok(ast::Expr::template(
                    format!("MATCH ({markers}) AGAINST ({{}} {mode})"),
                    args,
                ))
            }
            Dialect::Sqlite => Err(self
                .unsupported("a full-text condition (query an FTS5 table with where_raw instead)")),
        }
    }
}

/// `NOT (condition)` when `not`.
fn negate(condition: ast::Expr, not: bool) -> ast::Expr {
    if not { condition.not() } else { condition }
}

/// `left op right` for date parts and JSON lengths: comparisons only.
fn compare_compiled(left: ast::Expr, op: &Operator, right: ast::Expr) -> Result<ast::Expr> {
    Ok(match op {
        Operator::Eq => left.eq(right),
        Operator::Ne => left.ne(right),
        Operator::Lt => left.lt(right),
        Operator::Lte => left.lte(right),
        Operator::Gt => left.gt(right),
        Operator::Gte => left.gte(right),
        Operator::Invalid(op) => {
            return Err(OrmError::invalid(format!(
                "`{op}` is not a known operator for a date comparison (use =, <>, <, <=, >, >=)"
            )));
        }
        other => {
            return Err(OrmError::invalid(format!(
                "{other:?} cannot compare dates (use =, <>, <, <=, >, >=)"
            )));
        }
    })
}

/// The operators a hand-written condition ([`Operator::Raw`]) may use on
/// each database: the comparisons, and the ones that answer true or false
/// (JSON containment and key existence, array and range overlaps, text
/// search and pattern matches on PostgreSQL).
fn condition_operators(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres => &[
            "=",
            "<>",
            "!=",
            "<",
            "<=",
            ">",
            ">=",
            "@>",
            "<@",
            "?",
            "?|",
            "?&",
            "&&",
            "@?",
            "@@",
            "-|-",
            "&<",
            "&>",
            "<<",
            ">>",
            "~",
            "~*",
            "!~",
            "!~*",
            "^@",
            "LIKE",
            "NOT LIKE",
            "ILIKE",
            "NOT ILIKE",
            "SIMILAR TO",
            "NOT SIMILAR TO",
            "IS DISTINCT FROM",
            "IS NOT DISTINCT FROM",
        ],
        Dialect::MySql => &[
            "=",
            "<>",
            "!=",
            "<",
            "<=",
            ">",
            ">=",
            "<=>",
            "LIKE",
            "NOT LIKE",
            "REGEXP",
            "NOT REGEXP",
            "RLIKE",
            "NOT RLIKE",
            "SOUNDS LIKE",
        ],
        Dialect::Sqlite => &[
            "=",
            "==",
            "<>",
            "!=",
            "<",
            "<=",
            ">",
            ">=",
            "IS",
            "IS NOT",
            "LIKE",
            "NOT LIKE",
            "GLOB",
            "NOT GLOB",
            "MATCH",
            "REGEXP",
            "NOT REGEXP",
        ],
    }
}

/// PostgreSQL's built-in text search configurations: a full-text
/// language is written into the SQL, so only these are accepted.
const POSTGRES_LANGUAGES: [&str; 29] = [
    "simple",
    "arabic",
    "armenian",
    "basque",
    "catalan",
    "danish",
    "dutch",
    "english",
    "finnish",
    "french",
    "german",
    "greek",
    "hindi",
    "hungarian",
    "indonesian",
    "irish",
    "italian",
    "lithuanian",
    "nepali",
    "norwegian",
    "portuguese",
    "romanian",
    "russian",
    "serbian",
    "spanish",
    "swedish",
    "tamil",
    "turkish",
    "yiddish",
];

/// A LIKE pattern as a GLOB pattern: `%` → `*`, `_` → `?`, and GLOB's own
/// wildcards (`*`, `?`, `[`) matching themselves.
pub(crate) fn like_to_glob(pattern: &str) -> String {
    let mut glob = String::with_capacity(pattern.len() + 4);
    for c in pattern.chars() {
        match c {
            '%' => glob.push('*'),
            '_' => glob.push('?'),
            '*' => glob.push_str("[*]"),
            '?' => glob.push_str("[?]"),
            '[' => glob.push_str("[[]"),
            other => glob.push(other),
        }
    }
    glob
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_patterns_become_globs() {
        assert_eq!(like_to_glob("Acme%"), "Acme*");
        assert_eq!(like_to_glob("a_c"), "a?c");
        assert_eq!(like_to_glob("50*?["), "50[*][?][[]");
    }
}
