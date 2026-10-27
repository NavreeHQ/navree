//! `WHERE`, `HAVING` and join conditions, with the dialect rules for date
//! parts, LIKE, null-safe equality, regular expressions and JSON.

use chrono::{Datelike, Timelike};
use sea_query::{Alias, BinOper, Condition, ExprTrait, Func};

use super::{Compiler, Dialect, raw_sql};
use crate::error::OrmError;
use crate::ir::{
    Boolean, DatePart, Expr, FullText, FullTextMode, IgnoreCaseMode, InValues, JsonSegment,
    Operator, QueryIr, SelectItem, Where, WhereClause,
};
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

impl Compiler {
    /// A list of clauses with SQL's precedence: `a AND b OR c AND d` is
    /// `(a AND b) OR (c AND d)`. The first clause's boolean is ignored.
    pub(crate) fn clauses(&self, clauses: &[WhereClause]) -> Result<Condition, OrmError> {
        let mut groups: Vec<Condition> = vec![Condition::all()];
        for (i, clause) in clauses.iter().enumerate() {
            let mut condition = self.where_(&clause.filter)?;
            if clause.negated {
                condition = condition.not();
            }
            if i > 0 && clause.boolean == Boolean::Or {
                groups.push(Condition::all());
            }
            if let Some(group) = groups.pop() {
                groups.push(group.add(condition));
            }
        }
        Ok(if groups.len() == 1 {
            groups.pop().unwrap_or_else(Condition::all)
        } else {
            groups
                .into_iter()
                .fold(Condition::any(), |any, group| any.add(group))
        })
    }

    /// One condition.
    pub(crate) fn where_(&self, filter: &Where) -> Result<Condition, OrmError> {
        let expr = match filter {
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
            } => {
                let (expr, low, high) = (self.expr(expr)?, self.expr(low)?, self.expr(high)?);
                if *not {
                    expr.not_between(low, high)
                } else {
                    expr.between(low, high)
                }
            }
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
                let select = self.select(&query)?;
                if *not {
                    sea_query::Expr::not_exists(select)
                } else {
                    sea_query::Expr::exists(select)
                }
            }
            Where::Group(clauses) => {
                if clauses.is_empty() {
                    return Ok(Condition::all());
                }
                return self.clauses(clauses);
            }
            Where::JsonContains { column, value, not } => {
                let condition = self.json_contains(column, value)?;
                if *not {
                    return Ok(Condition::all().add(condition).not());
                }
                condition
            }
            Where::JsonLength { column, op, length } => self.json_length(column, op, *length)?,
            Where::JsonContainedBy { column, value, not } => {
                let condition = self.json_contained_by(column, value)?;
                if *not {
                    return Ok(Condition::all().add(condition).not());
                }
                condition
            }
            Where::JsonOverlaps { column, value, not } => {
                let condition = self.json_overlaps(column, value)?;
                if *not {
                    return Ok(Condition::all().add(condition).not());
                }
                condition
            }
            Where::JsonContainsKey { column, not } => {
                let condition = self.json_contains_key(column)?;
                if *not {
                    return Ok(Condition::all().add(condition).not());
                }
                condition
            }
            Where::FullText {
                columns,
                query,
                options,
            } => self.full_text(columns, query, options)?,
            Where::Raw { sql, values } => {
                // sea-query parenthesizes a custom fragment beside other
                // conditions, so an `OR` inside it cannot regroup them.
                let sql = raw_sql(sql, values.len(), self.dialect)?;
                sea_query::Expr::cust_with_values(sql, values.iter().cloned())
            }
            Where::Sql(condition) => return Ok(condition.clone()),
            Where::Invalid(message) => return Err(OrmError::invalid(message.clone())),
        };
        Ok(Condition::all().add(expr))
    }

    fn compare(
        &self,
        left: &Expr,
        op: &Operator,
        right: &Expr,
    ) -> Result<sea_query::Expr, OrmError> {
        let l = self.expr(left)?;
        let r = match (self.dialect, left, right) {
            // PostgreSQL's `->>` gives text, and a bound number or boolean
            // would not compare with text: compare with the value's JSON
            // text instead (`3`, `true`).
            (Dialect::Postgres, Expr::Column(column), Expr::Value(value))
                if !column.path.is_empty() && !matches!(value, Value::String(_)) =>
            {
                sea_query::Expr::val(json_text_of(value))
            }
            _ => self.expr(right)?,
        };
        Ok(match op {
            Operator::Eq => l.eq(r),
            Operator::Ne => l.ne(r),
            Operator::Lt => l.lt(r),
            Operator::Lte => l.lte(r),
            Operator::Gt => l.gt(r),
            Operator::Gte => l.gte(r),
            Operator::Like => self.text(l).binary(BinOper::Like, r),
            Operator::NotLike => self.text(l).binary(BinOper::NotLike, r),
            Operator::ILike => self.ilike(l, r, false),
            Operator::NotILike => self.ilike(l, r, true),
            Operator::NullSafeEq => match self.dialect {
                Dialect::Postgres => l.binary(BinOper::Custom("IS NOT DISTINCT FROM"), r),
                Dialect::MySql => l.binary(BinOper::Custom("<=>"), r),
                Dialect::Sqlite => l.binary(BinOper::Custom("IS"), r),
            },
            Operator::BitAnd => l.binary(BinOper::BitAnd, r),
            Operator::BitOr => l.binary(BinOper::BitOr, r),
            Operator::BitXor => match self.dialect {
                Dialect::Postgres => l.binary(BinOper::Custom("#"), r),
                Dialect::MySql => l.binary(BinOper::Custom("^"), r),
                // SQLite has no XOR: (a | b) - (a & b).
                Dialect::Sqlite => {
                    self.custom("(({} | {}) - ({} & {}))", vec![l.clone(), r.clone(), l, r])
                }
            },
            Operator::ShiftLeft => l.binary(BinOper::LShift, r),
            Operator::ShiftRight => l.binary(BinOper::RShift, r),
            Operator::Regexp | Operator::NotRegexp => {
                let not = matches!(op, Operator::NotRegexp);
                match self.dialect {
                    Dialect::Postgres => l.binary(BinOper::Custom(if not { "!~" } else { "~" }), r),
                    // `REGEXP` follows the column's collation (usually
                    // case-insensitive); the `c` flag matches case.
                    Dialect::MySql => self.custom(
                        if not {
                            "NOT REGEXP_LIKE({}, {}, 'c')"
                        } else {
                            "REGEXP_LIKE({}, {}, 'c')"
                        },
                        vec![l, r],
                    ),
                    Dialect::Sqlite => l.binary(
                        BinOper::Custom(if not { "NOT REGEXP" } else { "REGEXP" }),
                        r,
                    ),
                }
            }
            Operator::RegexpIgnoreCase | Operator::NotRegexpIgnoreCase => {
                let not = matches!(op, Operator::NotRegexpIgnoreCase);
                match self.dialect {
                    Dialect::Postgres => {
                        l.binary(BinOper::Custom(if not { "!~*" } else { "~*" }), r)
                    }
                    Dialect::MySql => self.custom(
                        if not {
                            "NOT REGEXP_LIKE({}, {}, 'i')"
                        } else {
                            "REGEXP_LIKE({}, {}, 'i')"
                        },
                        vec![l, r],
                    ),
                    // The `regexp` function SQLite calls is the app's (sqlx
                    // registers one with `with_regexp`); `(?i)` turns case off.
                    Dialect::Sqlite => self.custom(
                        if not {
                            "{} NOT REGEXP ('(?i)' || {})"
                        } else {
                            "{} REGEXP ('(?i)' || {})"
                        },
                        vec![l, r],
                    ),
                }
            }
            Operator::NullSafeNe => match self.dialect {
                Dialect::Postgres => l.binary(BinOper::Custom("IS DISTINCT FROM"), r),
                Dialect::MySql => self.custom("NOT ({} <=> {})", vec![l, r]),
                Dialect::Sqlite => l.binary(BinOper::Custom("IS NOT"), r),
            },
            Operator::Postgres(op) => match self.dialect {
                Dialect::Postgres => l.binary(BinOper::Custom(op), r),
                _ => return Err(self.unsupported(format!("the operator `{op}`"))),
            },
            Operator::Raw(op) => {
                let allowed = condition_operators(self.dialect);
                let normalized = normalize_operator(op);
                match allowed.iter().find(|known| **known == normalized) {
                    Some(known) => l.binary(BinOper::Custom(known), r),
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
    fn text(&self, expr: sea_query::Expr) -> sea_query::Expr {
        match self.dialect {
            Dialect::Postgres => self.custom("{}::text", vec![expr]),
            Dialect::MySql | Dialect::Sqlite => expr,
        }
    }

    fn ilike(&self, left: sea_query::Expr, right: sea_query::Expr, not: bool) -> sea_query::Expr {
        match self.dialect {
            Dialect::Postgres => self.text(left).binary(
                BinOper::Custom(if not { "NOT ILIKE" } else { "ILIKE" }),
                right,
            ),
            Dialect::MySql | Dialect::Sqlite => {
                let left: sea_query::Expr = Func::lower(left).into();
                let right: sea_query::Expr = Func::lower(right).into();
                left.binary(if not { BinOper::NotLike } else { BinOper::Like }, right)
            }
        }
    }

    fn like(
        &self,
        expr: &Expr,
        pattern: &Expr,
        case_sensitive: bool,
        not: bool,
    ) -> Result<sea_query::Expr, OrmError> {
        let left = self.expr(expr)?;
        Ok(match (self.dialect, case_sensitive) {
            (Dialect::Postgres, false) => {
                let right = self.expr(pattern)?;
                self.ilike(left, right, not)
            }
            (Dialect::Postgres, true) | (Dialect::MySql | Dialect::Sqlite, false) => {
                let right = self.expr(pattern)?;
                self.text(left)
                    .binary(if not { BinOper::NotLike } else { BinOper::Like }, right)
            }
            (Dialect::MySql, true) => {
                let right = self.expr(pattern)?;
                left.binary(
                    BinOper::Custom(if not {
                        "NOT LIKE BINARY"
                    } else {
                        "LIKE BINARY"
                    }),
                    right,
                )
            }
            (Dialect::Sqlite, true) => {
                // SQLite's LIKE ignores ASCII case; GLOB matches case, with
                // its own wildcards.
                let pattern = match pattern {
                    Expr::Value(Value::String(Some(text))) => {
                        Expr::Value(Value::from(like_to_glob(text)))
                    }
                    other => other.clone(),
                };
                let right = self.expr(&pattern)?;
                left.binary(
                    BinOper::Custom(if not { "NOT GLOB" } else { "GLOB" }),
                    right,
                )
            }
        })
    }

    /// Text compared ignoring case with navree-db's helpers, which fold
    /// non-ASCII letters on every database: a `GLOB` of each letter's case
    /// variants on SQLite (whose `LOWER` and `LIKE` fold ASCII only),
    /// lower-cased bytes on MySQL (whatever the column's collation),
    /// `LOWER(..) LIKE` or `=` on PostgreSQL. The text is literal.
    fn ignore_case(
        &self,
        expr: &Expr,
        text: &str,
        mode: IgnoreCaseMode,
    ) -> Result<sea_query::Expr, OrmError> {
        let expr = self.expr(expr)?;
        let backend = self.dialect.backend();
        Ok(match mode {
            IgnoreCaseMode::Contains => navree_db::contains_ignore_case(backend, expr, text),
            IgnoreCaseMode::Equals => navree_db::eq_ignore_case(backend, expr, text),
        })
    }

    fn in_(&self, expr: &Expr, values: &InValues, not: bool) -> Result<sea_query::Expr, OrmError> {
        let left = self.expr(expr)?;
        Ok(match values {
            InValues::List(list) if list.is_empty() => {
                // `IN ()` is not SQL: an empty IN matches nothing, NOT IN everything.
                sea_query::Expr::cust(if not { "1 = 1" } else { "1 = 0" })
            }
            InValues::List(list) => {
                let list = list
                    .iter()
                    .map(|value| self.expr(value))
                    .collect::<Result<Vec<_>, _>>()?;
                if not {
                    left.is_not_in(list)
                } else {
                    left.is_in(list)
                }
            }
            InValues::Subquery(query) => {
                let select = self.select(query)?;
                if not {
                    left.not_in_subquery(select)
                } else {
                    left.in_subquery(select)
                }
            }
        })
    }

    fn date_part(
        &self,
        part: DatePart,
        expr: &Expr,
        op: &Operator,
        value: &Expr,
    ) -> Result<sea_query::Expr, OrmError> {
        let column = self.expr(expr)?;
        let value = self.date_part_value(part, value)?;
        let function = |name: &str, column: sea_query::Expr| -> sea_query::Expr {
            Func::cust(Alias::new(name)).arg(column).into()
        };
        let strftime = |format: &str, column: sea_query::Expr| -> sea_query::Expr {
            Func::cust(Alias::new("strftime"))
                .arg(sea_query::Expr::cust(format!("'{format}'")))
                .arg(column)
                .into()
        };
        let left = match (self.dialect, part) {
            (Dialect::Postgres, DatePart::Date) => self.custom("CAST({} AS DATE)", vec![column]),
            (Dialect::Postgres, DatePart::Time) => self.custom("CAST({} AS TIME)", vec![column]),
            (Dialect::Postgres, DatePart::Day) => self.custom("EXTRACT(DAY FROM {})", vec![column]),
            (Dialect::Postgres, DatePart::Month) => {
                self.custom("EXTRACT(MONTH FROM {})", vec![column])
            }
            (Dialect::Postgres, DatePart::Year) => {
                self.custom("EXTRACT(YEAR FROM {})", vec![column])
            }
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
            (Dialect::Postgres, DatePart::Date) => self.custom("CAST({} AS DATE)", vec![value]),
            (Dialect::Postgres, DatePart::Time) => self.custom("CAST({} AS TIME)", vec![value]),
            _ => value,
        };
        self.compare_compiled(left, op, right)
    }

    /// `left op right` on already compiled sides (date parts).
    fn compare_compiled(
        &self,
        l: sea_query::Expr,
        op: &Operator,
        r: sea_query::Expr,
    ) -> Result<sea_query::Expr, OrmError> {
        Ok(match op {
            Operator::Eq => l.eq(r),
            Operator::Ne => l.ne(r),
            Operator::Lt => l.lt(r),
            Operator::Lte => l.lte(r),
            Operator::Gt => l.gt(r),
            Operator::Gte => l.gte(r),
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

    /// The value a date part is compared with: numbers for day, month and
    /// year (zero-padded text on SQLite, whose `strftime` returns text),
    /// `YYYY-MM-DD` / `HH:MM:SS` text for dates and times on SQLite.
    fn date_part_value(&self, part: DatePart, value: &Expr) -> Result<sea_query::Expr, OrmError> {
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
        Ok(sea_query::Expr::val(normalized))
    }

    fn json_contains(
        &self,
        column: &crate::ir::ColumnRef,
        value: &serde_json::Value,
    ) -> Result<sea_query::Expr, OrmError> {
        Ok(match self.dialect {
            Dialect::Postgres => {
                let target = self.json_value(column);
                self.custom(
                    "({})::jsonb @> {}",
                    vec![
                        target,
                        sea_query::Expr::val(Value::Json(Some(Box::new(value.clone())))),
                    ],
                )
            }
            Dialect::MySql => {
                let base = self.plain_column(column);
                let encoded = sea_query::Expr::val(Value::from(value.to_string()));
                if column.path.is_empty() {
                    self.custom("json_contains({}, {})", vec![base, encoded])
                } else {
                    self.custom(
                        &format!(
                            "json_contains({{}}, {{}}, {})",
                            self.json_path(&column.path)
                        ),
                        vec![base, encoded],
                    )
                }
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
                let base = self.plain_column(column);
                let source = if column.path.is_empty() {
                    self.custom("json_each({})", vec![base])
                } else {
                    self.custom(
                        &format!("json_each({{}}, {})", self.json_path(&column.path)),
                        vec![base],
                    )
                };
                self.custom(
                    "EXISTS (SELECT 1 FROM {} WHERE \"json_each\".\"value\" IS {})",
                    vec![source, sea_query::Expr::val(scalar)],
                )
            }
        })
    }

    /// The JSON at `column` is contained in `value` (`<@` on PostgreSQL,
    /// `json_contains(value, column)` on MySQL).
    fn json_contained_by(
        &self,
        column: &crate::ir::ColumnRef,
        value: &serde_json::Value,
    ) -> Result<sea_query::Expr, OrmError> {
        Ok(match self.dialect {
            Dialect::Postgres => self.custom(
                "({})::jsonb <@ {}",
                vec![
                    self.json_value(column),
                    sea_query::Expr::val(Value::Json(Some(Box::new(value.clone())))),
                ],
            ),
            Dialect::MySql => {
                let target = self.mysql_json(column);
                self.custom(
                    "json_contains(CAST({} AS JSON), {})",
                    vec![sea_query::Expr::val(Value::from(value.to_string())), target],
                )
            }
            Dialect::Sqlite => {
                return Err(self.unsupported("a JSON contained-by check"));
            }
        })
    }

    /// The JSON array at `column` shares an element with `value`: on
    /// PostgreSQL the column contains one of the elements (`@>`, so any
    /// element type works), MySQL's `json_overlaps`, on SQLite the elements
    /// of both compared through `json_each`.
    fn json_overlaps(
        &self,
        column: &crate::ir::ColumnRef,
        value: &serde_json::Value,
    ) -> Result<sea_query::Expr, OrmError> {
        let elements = match value {
            serde_json::Value::Array(_) => value.clone(),
            scalar => serde_json::Value::Array(vec![scalar.clone()]),
        };
        Ok(match self.dialect {
            Dialect::Postgres => self.custom(
                "EXISTS (SELECT 1 FROM jsonb_array_elements({}) AS \"navree_candidate\" WHERE ({})::jsonb @> jsonb_build_array(\"navree_candidate\".\"value\"))",
                vec![
                    sea_query::Expr::val(Value::Json(Some(Box::new(elements)))),
                    self.json_value(column),
                ],
            ),
            Dialect::MySql => {
                let target = self.mysql_json(column);
                self.custom(
                    "json_overlaps({}, CAST({} AS JSON))",
                    vec![target, sea_query::Expr::val(Value::from(elements.to_string()))],
                )
            }
            Dialect::Sqlite => {
                let source = self.sqlite_json_each(column);
                self.custom(
                    "EXISTS (SELECT 1 FROM {} AS \"navree_left\", json_each({}) AS \"navree_right\" WHERE \"navree_left\".\"value\" = \"navree_right\".\"value\")",
                    vec![source, sea_query::Expr::val(Value::from(elements.to_string()))],
                )
            }
        })
    }

    /// The JSON column has the key (or array index) its path ends with.
    fn json_contains_key(
        &self,
        column: &crate::ir::ColumnRef,
    ) -> Result<sea_query::Expr, OrmError> {
        let Some((last, parent_path)) = column.path.split_last() else {
            return Err(OrmError::invalid(format!(
                "a JSON key check needs a path to the key (`{}->key`), not the column alone",
                column.name
            )));
        };
        Ok(match self.dialect {
            Dialect::Postgres => {
                let parent = self.json_value(&crate::ir::ColumnRef {
                    path: parent_path.to_vec(),
                    ..column.clone()
                });
                match last {
                    JsonSegment::Key(key) => self.custom(
                        "COALESCE(({})::jsonb ? {}, false)",
                        vec![parent, sea_query::Expr::val(Value::from(key.clone()))],
                    ),
                    JsonSegment::Index(index) => {
                        // The array is long enough to have the index (from
                        // the end when negative).
                        let needed = if *index < 0 { -index } else { index + 1 };
                        self.custom(
                            "CASE WHEN jsonb_typeof(({})::jsonb) = 'array' THEN jsonb_array_length(({})::jsonb) >= {} ELSE false END",
                            vec![parent.clone(), parent, sea_query::Expr::val(needed)],
                        )
                    }
                }
            }
            Dialect::MySql => {
                let base = self.plain_column(column);
                self.custom(
                    &format!(
                        "IFNULL(json_contains_path({{}}, 'one', {}), 0)",
                        self.json_path(&column.path)
                    ),
                    vec![base],
                )
            }
            Dialect::Sqlite => {
                let base = self.plain_column(column);
                self.custom(
                    &format!(
                        "json_type({{}}, {}) IS NOT NULL",
                        self.json_path(&column.path)
                    ),
                    vec![base],
                )
            }
        })
    }

    /// MySQL: the JSON at `column` (its path extracted, still JSON).
    fn mysql_json(&self, column: &crate::ir::ColumnRef) -> sea_query::Expr {
        let base = self.plain_column(column);
        if column.path.is_empty() {
            base
        } else {
            self.custom(
                &format!("json_extract({{}}, {})", self.json_path(&column.path)),
                vec![base],
            )
        }
    }

    /// SQLite: the elements of the JSON at `column`, as `json_each` rows.
    fn sqlite_json_each(&self, column: &crate::ir::ColumnRef) -> sea_query::Expr {
        let base = self.plain_column(column);
        if column.path.is_empty() {
            self.custom("json_each({})", vec![base])
        } else {
            self.custom(
                &format!("json_each({{}}, {})", self.json_path(&column.path)),
                vec![base],
            )
        }
    }

    fn json_length(
        &self,
        column: &crate::ir::ColumnRef,
        op: &Operator,
        length: i64,
    ) -> Result<sea_query::Expr, OrmError> {
        let left = match self.dialect {
            Dialect::Postgres => {
                let target = self.json_value(column);
                self.custom("jsonb_array_length(({})::jsonb)", vec![target])
            }
            Dialect::MySql => {
                let base = self.plain_column(column);
                if column.path.is_empty() {
                    self.custom("json_length({})", vec![base])
                } else {
                    self.custom(
                        &format!("json_length({{}}, {})", self.json_path(&column.path)),
                        vec![base],
                    )
                }
            }
            Dialect::Sqlite => {
                let base = self.plain_column(column);
                if column.path.is_empty() {
                    self.custom("json_array_length({})", vec![base])
                } else {
                    self.custom(
                        &format!("json_array_length({{}}, {})", self.json_path(&column.path)),
                        vec![base],
                    )
                }
            }
        };
        self.compare_compiled(left, op, sea_query::Expr::val(length))
    }
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

/// An operator as the lists above spell it: upper case, single spaces.
fn normalize_operator(op: &str) -> String {
    op.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
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

impl Compiler {
    fn full_text(
        &self,
        columns: &[Expr],
        query: &str,
        options: &FullText,
    ) -> Result<sea_query::Expr, OrmError> {
        let compiled = columns
            .iter()
            .map(|column| self.expr(column))
            .collect::<Result<Vec<_>, _>>()?;
        let search = sea_query::Expr::val(Value::from(query.to_owned()));
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
                let vector = if options.vectors {
                    "{}".to_owned()
                } else {
                    format!("to_tsvector('{language}', {{}})")
                };
                let vectors = vec![vector; compiled.len()].join(" || ");
                let mut exprs = compiled;
                exprs.push(search);
                Ok(self.custom(
                    &format!("({vectors}) @@ {function}('{language}', {{}})"),
                    exprs,
                ))
            }
            Dialect::MySql => {
                let mode = match options.mode {
                    FullTextMode::Boolean | FullTextMode::Websearch | FullTextMode::Raw => {
                        "IN BOOLEAN MODE"
                    }
                    _ if options.expanded => "IN NATURAL LANGUAGE MODE WITH QUERY EXPANSION",
                    _ => "IN NATURAL LANGUAGE MODE",
                };
                let markers = vec!["{}"; compiled.len()].join(", ");
                let mut exprs = compiled;
                exprs.push(search);
                Ok(self.custom(&format!("MATCH ({markers}) AGAINST ({{}} {mode})"), exprs))
            }
            Dialect::Sqlite => Err(self
                .unsupported("a full-text condition (query an FTS5 table with where_raw instead)")),
        }
    }
}

/// A LIKE pattern as a GLOB pattern: `%` → `*`, `_` → `?`, and GLOB's own
/// wildcards (`*`, `?`, `[`) matching themselves.
fn like_to_glob(pattern: &str) -> String {
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
