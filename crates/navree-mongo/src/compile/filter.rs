//! Conditions ([`Where`]) as MongoDB filter documents: the query language
//! (`{"stage": "won"}`, `{"amount": {"$gte": 100}}`) when a condition is
//! about a field and a value, `$expr` when it compares computed values,
//! other fields or a subquery's columns.

use bson::{Bson, Document, doc};
use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use navree_orm::ir::{
    Boolean, ColumnRef, DatePart, Expr, FullText, FullTextMode, IgnoreCaseMode, InValues, Operator,
    Where, WhereClause,
};
use navree_orm::{OrmError, Value};

use super::expr::{self, literal};
use super::like::like_regex;
use super::scope::{Cx, Place, Scope};
use super::{Pending, subquery};
use crate::convert::{json_to_bson, to_bson};
use crate::error::{invalid, unsupported};

/// The filter that matches nothing.
pub(crate) fn nothing() -> Document {
    doc! { "$expr": false }
}

/// A list of clauses with SQL's precedence: `a AND b OR c AND d` is
/// `(a AND b) OR (c AND d)`. An empty list matches every document.
pub(crate) fn clauses(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    clauses: &[WhereClause],
) -> Result<Document, OrmError> {
    let mut groups: Vec<Vec<Document>> = vec![Vec::new()];
    for (i, clause) in clauses.iter().enumerate() {
        let mut filter = condition(cx, scope, pending, &clause.filter)?;
        if clause.negated {
            filter = not(filter);
        }
        if i > 0 && clause.boolean == Boolean::Or {
            groups.push(Vec::new());
        }
        if let Some(group) = groups.last_mut() {
            group.push(filter);
        }
    }
    let mut groups: Vec<Document> = groups.into_iter().map(and).collect();
    Ok(if groups.len() == 1 {
        groups.remove(0)
    } else {
        or(groups)
    })
}

/// Every one of `filters`: one document when their top-level keys do not
/// collide, `$and` otherwise.
pub(crate) fn and(filters: Vec<Document>) -> Document {
    let filters: Vec<Document> = filters
        .into_iter()
        .filter(|filter| !filter.is_empty())
        .collect();
    if filters.len() == 1 {
        return filters.into_iter().next().unwrap_or_default();
    }
    let mut merged = Document::new();
    let mut collide = false;
    for filter in &filters {
        for key in filter.keys() {
            if merged.contains_key(key) {
                collide = true;
            }
        }
        if collide {
            break;
        }
        for (key, value) in filter {
            merged.insert(key.clone(), value.clone());
        }
    }
    if collide {
        doc! { "$and": filters }
    } else {
        merged
    }
}

/// Any of `filters` (`$or`).
pub(crate) fn or(filters: Vec<Document>) -> Document {
    if filters.iter().any(Document::is_empty) {
        return Document::new();
    }
    doc! { "$or": filters }
}

/// Not `filter` (`$nor`). Like every negation on MongoDB, it matches the
/// documents where the field is missing or null too.
pub(crate) fn not(filter: Document) -> Document {
    if filter.is_empty() {
        return nothing();
    }
    doc! { "$nor": [filter] }
}

/// `$expr` around an aggregation expression.
fn expression(expr: Bson) -> Document {
    doc! { "$expr": expr }
}

/// `{"$op": [left, right]}` between aggregation values, with SQL's rule
/// that no comparison but `<>` holds for NULL, a missing field read as
/// null.
///
/// Aggregation orders null below every value, so `null < 5`, `5 > null`
/// and `null = null` would hold. The comparison therefore first checks
/// that the side a null would wrongly satisfy (the left of `<` and `<=`,
/// the right of `>` and `>=`, either of `=`) is a value, unless it is a
/// constant. `<>` keeps MongoDB's rule, like every negation here: null
/// differs from every value.
pub(crate) fn compared(operator: &str, left: Bson, right: Bson) -> Bson {
    let (left, right) = (missing_as_null(left), missing_as_null(right));
    let checked = match operator {
        "$lt" | "$lte" => Some(&left),
        "$gt" | "$gte" => Some(&right),
        // Equal to a constant is a value already.
        "$eq" if can_be_null(&right) => Some(&left),
        _ => None,
    }
    .filter(|side| can_be_null(side))
    .map(|side| Bson::Document(doc! { "$gt": [side.clone(), Bson::Null] }));
    let comparison = Bson::Document(doc! { operator: [left, right] });
    match checked {
        Some(check) => Bson::Document(doc! { "$and": [check, comparison] }),
        None => comparison,
    }
}

/// `left <=> right`: equal, or both null (a missing field is null).
pub(crate) fn null_safe_equal(left: Bson, right: Bson) -> Bson {
    Bson::Document(doc! { "$eq": [missing_as_null(left), missing_as_null(right)] })
}

/// A field path or variable as `{"$ifNull": [path, null]}`: in
/// aggregation, a missing field is neither null nor a value.
fn missing_as_null(value: Bson) -> Bson {
    match value {
        Bson::String(path) if path.starts_with('$') => {
            Bson::Document(doc! { "$ifNull": [path, Bson::Null] })
        }
        other => other,
    }
}

/// Whether an aggregation value can be null: anything but a constant
/// other than null.
fn can_be_null(value: &Bson) -> bool {
    match value {
        Bson::Null | Bson::Undefined => true,
        Bson::String(text) => text.starts_with('$'),
        Bson::Document(document) => match document.get("$literal") {
            Some(constant) if document.len() == 1 => {
                matches!(constant, Bson::Null | Bson::Undefined)
            }
            _ => true,
        },
        _ => false,
    }
}

/// One condition.
pub(crate) fn condition(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    filter: &Where,
) -> Result<Document, OrmError> {
    Ok(match filter {
        Where::Compare { left, op, right } => compare(cx, scope, pending, left, op, right)?,
        Where::Null { expr, not } => {
            if let Some(path) = field(cx, scope, expr)? {
                if *not {
                    doc! { path: { "$ne": Bson::Null } }
                } else {
                    doc! { path: Bson::Null }
                }
            } else {
                let value = expr::value(cx, scope, pending, expr)?;
                let operator = if *not { "$ne" } else { "$eq" };
                expression(Bson::Document(doc! {
                    operator: [{ "$ifNull": [value, Bson::Null] }, Bson::Null]
                }))
            }
        }
        Where::In { expr, values, not } => in_(cx, scope, pending, expr, values, *not)?,
        Where::Between {
            expr,
            low,
            high,
            not,
        } => between(cx, scope, pending, expr, low, high, *not)?,
        Where::Like {
            expr,
            pattern,
            case_sensitive,
            not,
        } => {
            let Expr::Value(pattern) = pattern else {
                return Err(unsupported(
                    "a LIKE pattern computed from columns (MongoDB matches literal patterns)",
                ));
            };
            let pattern = text(pattern, "a LIKE pattern")?;
            regex_condition(
                cx,
                scope,
                pending,
                expr,
                &like_regex(&pattern),
                !*case_sensitive,
                *not,
            )?
        }
        Where::IgnoreCase { expr, text, mode } => regex_condition(
            cx,
            scope,
            pending,
            expr,
            &ignore_case_regex(text, *mode)?,
            true,
            false,
        )?,
        Where::DatePart {
            part,
            expr,
            op,
            value,
        } => date_part(cx, scope, pending, *part, expr, op, value)?,
        Where::Exists { query, not } => subquery::exists(cx, scope, pending, query, *not)?,
        Where::Group(inner) => clauses(cx, scope, pending, inner)?,
        Where::JsonContains { column, value, not } => {
            let path = field_path_of(cx, scope, column, "a JSON containment check")?;
            let filter = contains(&path, value)?;
            if *not {
                super::filter::not(filter)
            } else {
                filter
            }
        }
        Where::JsonLength { column, op, length } => {
            let path = field_path_of(cx, scope, column, "a JSON length check")?;
            json_length(&path, op, *length)?
        }
        Where::FullText {
            columns: _,
            query,
            options,
        } => full_text(query, options)?,
        Where::Raw { .. } => {
            return Err(unsupported(
                "raw SQL conditions (`where_raw`; write the condition with typed columns, or as a filter document with `Filter::document(doc! { .. })`)",
            ));
        }
        Where::Tree(_) => {
            return Err(unsupported(
                "SQL conditions (render-tree conditions and access-control filters; write them with typed columns or as a filter document)",
            ));
        }
        Where::Native(native) => match native.downcast_ref::<Document>() {
            Some(document) if native.driver() == crate::error::DRIVER => document.clone(),
            _ => {
                return Err(unsupported(format!(
                    "a {} filter (a native filter of another store)",
                    native.driver()
                )));
            }
        },
        Where::Invalid(message) => return Err(invalid(message.clone())),
        other => return Err(unsupported(format!("the condition {other:?}"))),
    })
}

/// The regular expression of a [`Where::IgnoreCase`] condition: `text`
/// literally (its LIKE wildcards escaped), anywhere in the value or as the
/// whole value. Matched with the `i` option, which folds every letter with
/// case forms (MongoDB's regular expressions are Unicode) and keeps
/// accents.
fn ignore_case_regex(text: &str, mode: IgnoreCaseMode) -> Result<String, OrmError> {
    let mut literal = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            literal.push('\\');
        }
        literal.push(c);
    }
    Ok(like_regex(&match mode {
        IgnoreCaseMode::Contains => format!("%{literal}%"),
        IgnoreCaseMode::Equals => literal,
        other => return Err(unsupported(format!("the text comparison {other:?}"))),
    }))
}

/// The field path of `expr` when it is a column of the document (not a
/// computed value, not a variable).
fn field(cx: &mut Cx, scope: &Scope<'_>, expr: &Expr) -> Result<Option<String>, OrmError> {
    match expr {
        Expr::Column(column) => match scope.place(cx, column)? {
            Place::Field(path) => Ok(Some(path)),
            Place::Variable(_) => Ok(None),
        },
        _ => Ok(None),
    }
}

/// The field path of a JSON condition's column.
fn field_path_of(
    cx: &mut Cx,
    scope: &Scope<'_>,
    column: &ColumnRef,
    what: &str,
) -> Result<String, OrmError> {
    match scope.place(cx, column)? {
        Place::Field(path) => Ok(path),
        Place::Variable(_) => Err(unsupported(format!(
            "{what} on a column of an enclosing query"
        ))),
    }
}

/// Text from a value (a pattern, a key).
fn text(value: &Value, what: &str) -> Result<String, OrmError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        other => Err(invalid(format!(
            "{what} must be text, not {}",
            navree_orm::to_json(other)
        ))),
    }
}

/// `{path: value}`, with `$eq` when the value is a document, an array or
/// anything MongoDB would read as an operator or a pattern.
fn equals(path: &str, value: Bson) -> Document {
    match value {
        Bson::Document(_)
        | Bson::Array(_)
        | Bson::RegularExpression(_)
        | Bson::JavaScriptCode(_)
        | Bson::JavaScriptCodeWithScope(_) => doc! { path: { "$eq": value } },
        other => doc! { path: other },
    }
}

/// The comparison operator of `op`, for `{field: {op: value}}` and
/// `{"$op": [a, b]}` alike.
fn comparison(op: &Operator) -> Option<&'static str> {
    Some(match op {
        Operator::Eq | Operator::NullSafeEq => "$eq",
        Operator::Ne => "$ne",
        Operator::Lt => "$lt",
        Operator::Lte => "$lte",
        Operator::Gt => "$gt",
        Operator::Gte => "$gte",
        _ => return None,
    })
}

/// How `op` is written (`<=`, `LIKE`, `@>`), for messages.
pub(crate) fn operator_text(op: &Operator) -> String {
    match op {
        Operator::Eq => "=",
        Operator::Ne => "<>",
        Operator::Lt => "<",
        Operator::Lte => "<=",
        Operator::Gt => ">",
        Operator::Gte => ">=",
        Operator::Like => "LIKE",
        Operator::NotLike => "NOT LIKE",
        Operator::ILike => "ILIKE",
        Operator::NotILike => "NOT ILIKE",
        Operator::NullSafeEq => "<=>",
        Operator::BitAnd => "&",
        Operator::BitOr => "|",
        Operator::BitXor => "^",
        Operator::ShiftLeft => "<<",
        Operator::ShiftRight => ">>",
        Operator::Regexp => "REGEXP",
        Operator::NotRegexp => "NOT REGEXP",
        Operator::Postgres(op) => op,
        Operator::Invalid(op) => return op.clone(),
        other => return format!("{other:?}"),
    }
    .to_owned()
}

/// What kind of condition `filter` is, for messages.
pub(crate) fn kind(filter: &Where) -> &'static str {
    match filter {
        Where::Compare { .. } => "a comparison",
        Where::Null { .. } => "IS NULL",
        Where::In {
            values: InValues::Subquery(_),
            ..
        } => "IN (subquery)",
        Where::In { .. } => "IN",
        Where::Between { .. } => "BETWEEN",
        Where::Like { .. } => "LIKE",
        Where::IgnoreCase { .. } => "a text match ignoring case",
        Where::DatePart { .. } => "a date-part condition (`where_date`, `where_month`, ...)",
        Where::Exists { .. } => "EXISTS (`where_exists`, `where_has`)",
        Where::Group(_) => "a group of conditions",
        Where::JsonContains { .. } => "a JSON containment check",
        Where::JsonLength { .. } => "a JSON length check",
        Where::FullText { .. } => "a full-text condition",
        Where::Raw { .. } | Where::Tree(_) => "raw SQL",
        Where::Native(_) => "a native filter",
        Where::Invalid(_) => "an invalid condition",
        _ => "this condition",
    }
}

/// The message for an operator nothing knows.
fn unknown_operator(op: &str) -> OrmError {
    invalid(format!(
        "`{op}` is not a known operator (use =, <>, <, <=, >, >=, like, not like, ilike, not ilike, <=>, &, |, ^, regexp, not regexp)"
    ))
}

fn compare(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    left: &Expr,
    op: &Operator,
    right: &Expr,
) -> Result<Document, OrmError> {
    if let (Some(path), Expr::Value(value)) = (field(cx, scope, left)?, right) {
        return field_compare(&path, op, value);
    }
    match op {
        Operator::Like | Operator::NotLike | Operator::ILike | Operator::NotILike => {
            let Expr::Value(pattern) = right else {
                return Err(unsupported(
                    "a LIKE pattern computed from columns (MongoDB matches literal patterns)",
                ));
            };
            let pattern = like_regex(&text(pattern, "a LIKE pattern")?);
            let insensitive = matches!(op, Operator::ILike | Operator::NotILike);
            let negated = matches!(op, Operator::NotLike | Operator::NotILike);
            return regex_condition(cx, scope, pending, left, &pattern, insensitive, negated);
        }
        Operator::Regexp | Operator::NotRegexp => {
            let Expr::Value(pattern) = right else {
                return Err(unsupported("a regular expression computed from columns"));
            };
            let pattern = text(pattern, "a regular expression")?;
            let negated = *op == Operator::NotRegexp;
            return regex_condition(cx, scope, pending, left, &pattern, false, negated);
        }
        _ => {}
    }
    let l = expr::value(cx, scope, pending, left)?;
    let r = expr::value(cx, scope, pending, right)?;
    if *op == Operator::NullSafeEq {
        return Ok(expression(null_safe_equal(l, r)));
    }
    if let Some(operator) = comparison(op) {
        return Ok(expression(compared(operator, l, r)));
    }
    match op {
        Operator::BitAnd => Ok(expression(Bson::Document(doc! {
            "$ne": [{ "$bitAnd": [l, r] }, 0]
        }))),
        Operator::BitOr => Ok(expression(Bson::Document(doc! {
            "$ne": [{ "$bitOr": [l, r] }, 0]
        }))),
        Operator::BitXor => Ok(expression(Bson::Document(doc! {
            "$ne": [{ "$bitXor": [l, r] }, 0]
        }))),
        Operator::ShiftLeft | Operator::ShiftRight => Err(unsupported(
            "bit shifts (MongoDB has no shift operators: multiply or divide by a power of two)",
        )),
        Operator::Postgres(op) => Err(unsupported(format!(
            "the PostgreSQL operator `{op}` between computed values"
        ))),
        Operator::Invalid(op) => Err(unknown_operator(op)),
        other => Err(unsupported(format!(
            "the operator `{}`",
            operator_text(other)
        ))),
    }
}

/// `field op value` in the query language.
fn field_compare(path: &str, op: &Operator, value: &Value) -> Result<Document, OrmError> {
    let null = navree_orm::is_null(value);
    if let Some(operator) = comparison(op) {
        return Ok(match (operator, null) {
            ("$eq", true) => doc! { path: Bson::Null },
            ("$eq", false) => equals(path, to_bson(value)?),
            ("$ne", _) => doc! { path: { "$ne": to_bson(value)? } },
            // Nothing is less or greater than NULL.
            (_, true) => nothing(),
            (operator, false) => doc! { path: { operator: to_bson(value)? } },
        });
    }
    match op {
        Operator::Like | Operator::NotLike | Operator::ILike | Operator::NotILike => {
            let pattern = like_regex(&text(value, "a LIKE pattern")?);
            let insensitive = matches!(op, Operator::ILike | Operator::NotILike);
            let negated = matches!(op, Operator::NotLike | Operator::NotILike);
            Ok(regex_field(path, &pattern, insensitive, negated))
        }
        Operator::Regexp | Operator::NotRegexp => Ok(regex_field(
            path,
            &text(value, "a regular expression")?,
            false,
            *op == Operator::NotRegexp,
        )),
        Operator::BitAnd => Ok(doc! { path: { "$bitsAnySet": to_bson(value)? } }),
        Operator::BitOr | Operator::BitXor => {
            let operator = if *op == Operator::BitOr {
                "$bitOr"
            } else {
                "$bitXor"
            };
            Ok(expression(Bson::Document(doc! {
                "$ne": [{ operator: [format!("${path}"), literal(to_bson(value)?)] }, 0]
            })))
        }
        Operator::ShiftLeft | Operator::ShiftRight => Err(unsupported(
            "bit shifts (MongoDB has no shift operators: multiply or divide by a power of two)",
        )),
        Operator::Postgres(op) => postgres(path, op, value),
        Operator::Invalid(op) => Err(unknown_operator(op)),
        other => Err(unsupported(format!(
            "the operator `{}`",
            operator_text(other)
        ))),
    }
}

/// PostgreSQL's JSON and regular-expression operators on a field.
fn postgres(path: &str, op: &str, value: &Value) -> Result<Document, OrmError> {
    let keys = |value: &Value| -> Result<Vec<String>, OrmError> {
        let json = navree_orm::to_json(value);
        match json {
            serde_json::Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::String(key) => Ok(key),
                    other => Err(invalid(format!("`{op}` takes keys (text), not {other}"))),
                })
                .collect(),
            serde_json::Value::String(text) => match serde_json::from_str(&text) {
                Ok(serde_json::Value::Array(items)) => items
                    .into_iter()
                    .map(|item| match item {
                        serde_json::Value::String(key) => Ok(key),
                        other => Err(invalid(format!("`{op}` takes keys (text), not {other}"))),
                    })
                    .collect(),
                _ => Ok(vec![text]),
            },
            other => Err(invalid(format!("`{op}` takes keys (text), not {other}"))),
        }
    };
    let exists = |key: &str| -> Result<Document, OrmError> {
        if key.is_empty() || key.starts_with('$') || key.contains('.') {
            return Err(invalid(format!(
                "`{key}` is not a key MongoDB can look for"
            )));
        }
        Ok(doc! { format!("{path}.{key}"): { "$exists": true } })
    };
    match op {
        "?" => exists(&text(value, "the key `?` looks for")?),
        "?|" => Ok(or(keys(value)?
            .iter()
            .map(|key| exists(key))
            .collect::<Result<Vec<_>, _>>()?)),
        "?&" => Ok(and(keys(value)?
            .iter()
            .map(|key| exists(key))
            .collect::<Result<Vec<_>, _>>()?)),
        "@>" => {
            let json = match navree_orm::to_json(value) {
                serde_json::Value::String(text) => serde_json::from_str(&text)
                    .map_err(|_| invalid(format!("`@>` takes JSON, and `{text}` is not JSON")))?,
                other => other,
            };
            contains(path, &json)
        }
        "&&" => match to_bson(value)? {
            Bson::Array(items) => Ok(doc! { path: { "$in": items } }),
            _ => Err(invalid("`&&` takes an array")),
        },
        "~" | "~*" | "!~" | "!~*" => Ok(regex_field(
            path,
            &text(value, "a regular expression")?,
            op.contains('*'),
            op.starts_with('!'),
        )),
        other => Err(unsupported(format!("the PostgreSQL operator `{other}`"))),
    }
}

/// `{path: {"$regex": pattern, "$options": ..}}` (`$not` around it when
/// negated). `s` lets `.` match newlines, `i` ignores case.
fn regex_field(path: &str, pattern: &str, insensitive: bool, negated: bool) -> Document {
    let options = if insensitive { "is" } else { "s" };
    let regex = doc! { "$regex": pattern, "$options": options };
    if negated {
        doc! { path: { "$not": regex } }
    } else {
        doc! { path: regex }
    }
}

/// A regular-expression match of `expr`: on a field in the query language,
/// on a computed value with `$regexMatch`.
fn regex_condition(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    expr: &Expr,
    pattern: &str,
    insensitive: bool,
    negated: bool,
) -> Result<Document, OrmError> {
    if let Some(path) = field(cx, scope, expr)? {
        return Ok(regex_field(&path, pattern, insensitive, negated));
    }
    let input = expr::value(cx, scope, pending, expr)?;
    let options = if insensitive { "is" } else { "s" };
    let matched = Bson::Document(doc! {
        "$regexMatch": { "input": { "$toString": input }, "regex": pattern, "options": options }
    });
    Ok(expression(if negated {
        Bson::Document(doc! { "$not": [matched] })
    } else {
        matched
    }))
}

fn in_(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    expr: &Expr,
    values: &InValues,
    not: bool,
) -> Result<Document, OrmError> {
    match values {
        InValues::List(list) => {
            let all_values = list.iter().all(|item| matches!(item, Expr::Value(_)));
            if let (Some(path), true) = (field(cx, scope, expr)?, all_values) {
                let items = list
                    .iter()
                    .map(|item| match item {
                        Expr::Value(value) => to_bson(value),
                        _ => Err(invalid("an IN list holds values")),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let operator = if not { "$nin" } else { "$in" };
                return Ok(doc! { path: { operator: items } });
            }
            let value = expr::value(cx, scope, pending, expr)?;
            let items = list
                .iter()
                .map(|item| expr::value(cx, scope, pending, item))
                .collect::<Result<Vec<_>, _>>()?;
            let found = Bson::Document(doc! { "$in": [value, items] });
            Ok(expression(if not {
                Bson::Document(doc! { "$not": [found] })
            } else {
                found
            }))
        }
        InValues::Subquery(query) => subquery::in_(cx, scope, pending, expr, query, not),
        other => Err(unsupported(format!("the IN operand {other:?}"))),
    }
}

#[allow(clippy::too_many_arguments)]
fn between(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    expr: &Expr,
    low: &Expr,
    high: &Expr,
    not: bool,
) -> Result<Document, OrmError> {
    if let (Some(path), Expr::Value(low), Expr::Value(high)) = (field(cx, scope, expr)?, low, high)
    {
        let range = doc! { "$gte": to_bson(low)?, "$lte": to_bson(high)? };
        return Ok(if not {
            doc! { path: { "$not": range } }
        } else {
            doc! { path: range }
        });
    }
    let value = expr::value(cx, scope, pending, expr)?;
    let low = expr::value(cx, scope, pending, low)?;
    let high = expr::value(cx, scope, pending, high)?;
    let within = Bson::Document(doc! {
        "$and": [compared("$gte", value.clone(), low), compared("$lte", value, high)]
    });
    Ok(expression(if not {
        Bson::Document(doc! { "$not": [within] })
    } else {
        within
    }))
}

/// A comparison of part of a date: the date (`YYYY-MM-DD`) and time
/// (`HH:MM:SS`) as text, the day of the month, the month and the year as
/// numbers, all in UTC.
#[allow(clippy::too_many_arguments)]
fn date_part(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    part: DatePart,
    expr: &Expr,
    op: &Operator,
    value: &Expr,
) -> Result<Document, OrmError> {
    let Some(operator) = comparison(op) else {
        return Err(invalid(format!(
            "a date-part condition compares with =, <>, <, <=, > or >=, not `{}`",
            operator_text(op)
        )));
    };
    let Expr::Value(value) = value else {
        return Err(unsupported(
            "a date-part condition against a computed value",
        ));
    };
    let date = expr::value(cx, scope, pending, expr)?;
    let (left, right) = match part {
        DatePart::Date => (
            Bson::Document(doc! { "$dateToString": { "format": "%Y-%m-%d", "date": date } }),
            Bson::String(date_text(value)?),
        ),
        DatePart::Time => (
            Bson::Document(doc! { "$dateToString": { "format": "%H:%M:%S", "date": date } }),
            Bson::String(time_text(value)?),
        ),
        DatePart::Day => (
            Bson::Document(doc! { "$dayOfMonth": date }),
            Bson::Int64(number(value, part)?),
        ),
        DatePart::Month => (
            Bson::Document(doc! { "$month": date }),
            Bson::Int64(number(value, part)?),
        ),
        DatePart::Year => (
            Bson::Document(doc! { "$year": date }),
            Bson::Int64(number(value, part)?),
        ),
        other => return Err(unsupported(format!("the date part {other:?}"))),
    };
    Ok(expression(compared(operator, left, right)))
}

/// A date value as `YYYY-MM-DD`.
fn date_text(value: &Value) -> Result<String, OrmError> {
    let date = match value {
        Value::Date(date) => *date,
        Value::DateTime(at) => at.date(),
        Value::Timestamp(at) => at.date_naive(),
        Value::TimestampTz(at) => at.date_naive(),
        Value::String(text) => NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
            .map_err(|_| invalid(format!("`{text}` is not a date (YYYY-MM-DD)")))?,
        other => {
            return Err(invalid(format!(
                "a date condition takes a date, not {}",
                navree_orm::to_json(other)
            )));
        }
    };
    Ok(date.format("%Y-%m-%d").to_string())
}

/// A time value as `HH:MM:SS`.
fn time_text(value: &Value) -> Result<String, OrmError> {
    let time = match value {
        Value::Time(time) => *time,
        Value::DateTime(at) => at.time(),
        Value::Timestamp(at) => at.time(),
        Value::String(text) => NaiveTime::parse_from_str(text.trim(), "%H:%M:%S")
            .or_else(|_| NaiveTime::parse_from_str(text.trim(), "%H:%M"))
            .map_err(|_| invalid(format!("`{text}` is not a time (HH:MM:SS)")))?,
        other => {
            return Err(invalid(format!(
                "a time condition takes a time, not {}",
                navree_orm::to_json(other)
            )));
        }
    };
    Ok(format!(
        "{:02}:{:02}:{:02}",
        time.hour(),
        time.minute(),
        time.second()
    ))
}

/// A day, month or year number.
fn number(value: &Value, part: DatePart) -> Result<i64, OrmError> {
    let (name, from_date): (&str, fn(NaiveDate) -> i64) = match part {
        DatePart::Day => ("day", |date| i64::from(date.day())),
        DatePart::Month => ("month", |date| i64::from(date.month())),
        _ => ("year", |date| i64::from(date.year())),
    };
    Ok(match value {
        Value::Date(date) => from_date(*date),
        Value::Timestamp(at) => from_date(at.date_naive()),
        Value::DateTime(at) => from_date(at.date()),
        other => <i64 as navree_orm::Field>::from_value(other.clone())
            .map_err(|error| invalid(format!("a {name} condition takes a number: {error}")))?,
    })
}

/// The JSON at `path` contains `value`: an element of an array (or the
/// value itself), every element of an array, an object's keys and values.
fn contains(path: &str, value: &serde_json::Value) -> Result<Document, OrmError> {
    Ok(match value {
        serde_json::Value::Array(items) if items.is_empty() => {
            doc! { path: { "$type": "array" } }
        }
        serde_json::Value::Array(items) => {
            if items
                .iter()
                .all(|item| !item.is_object() && !item.is_array())
            {
                let items = items
                    .iter()
                    .map(json_to_bson)
                    .collect::<Result<Vec<_>, _>>()?;
                doc! { path: { "$all": items } }
            } else {
                and(items
                    .iter()
                    .map(|item| contains(path, item))
                    .collect::<Result<Vec<_>, _>>()?)
            }
        }
        serde_json::Value::Object(map) if map.is_empty() => {
            doc! { path: { "$type": "object" } }
        }
        serde_json::Value::Object(map) => {
            let mut fields = Vec::with_capacity(map.len());
            // Keys in a stable order, whatever order the map keeps.
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (key, inner) in entries {
                if key.is_empty() || key.starts_with('$') || key.contains('.') {
                    return Err(invalid(format!(
                        "`{key}` is not a key MongoDB can look for"
                    )));
                }
                fields.push(contains(&format!("{path}.{key}"), inner)?);
            }
            and(fields)
        }
        scalar => equals(path, json_to_bson(scalar)?),
    })
}

/// The length of the array at `path`, compared.
fn json_length(path: &str, op: &Operator, length: i64) -> Result<Document, OrmError> {
    if matches!(op, Operator::Eq) {
        return Ok(doc! { path: { "$size": length } });
    }
    let Some(operator) = comparison(op) else {
        return Err(invalid(format!(
            "a JSON length compares with =, <>, <, <=, > or >=, not `{}`",
            operator_text(op)
        )));
    };
    let field = format!("${path}");
    let size = Bson::Document(doc! {
        "$cond": [{ "$isArray": field.clone() }, { "$size": field }, Bson::Null]
    });
    Ok(expression(compared(operator, size, Bson::Int64(length))))
}

/// The languages MongoDB's text indexes stem.
const TEXT_LANGUAGES: [&str; 16] = [
    "danish",
    "dutch",
    "english",
    "finnish",
    "french",
    "german",
    "hungarian",
    "italian",
    "norwegian",
    "portuguese",
    "romanian",
    "russian",
    "spanish",
    "swedish",
    "turkish",
    "none",
];

/// A full-text condition: `$text` over the collection's text index (which
/// decides the fields searched). Natural mode searches the words, phrase
/// mode the phrase, websearch and boolean modes pass MongoDB's own syntax
/// through (`"a phrase"`, `-excluded`).
fn full_text(query: &str, options: &FullText) -> Result<Document, OrmError> {
    let search = match options.mode {
        FullTextMode::Phrase => format!("\"{}\"", query.replace('"', " ").trim()),
        FullTextMode::Websearch | FullTextMode::Boolean => query.to_owned(),
        // Words: quotes and leading minus signs are not operators here.
        _ => query
            .split_whitespace()
            .map(|word| word.trim_start_matches('-').replace('"', ""))
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    };
    if options.expanded {
        return Err(unsupported(
            "query expansion in full-text conditions (a MySQL mode)",
        ));
    }
    let mut text = doc! { "$search": search };
    if let Some(language) = &options.language {
        let language = language.to_ascii_lowercase();
        if !TEXT_LANGUAGES.contains(&language.as_str()) {
            return Err(invalid(format!(
                "MongoDB's text search has no language `{language}` (it has {})",
                TEXT_LANGUAGES.join(", ")
            )));
        }
        text.insert("$language", language);
    }
    Ok(doc! { "$text": text })
}

#[cfg(test)]
mod tests {
    use navree_orm::ir::TableRef;

    use super::*;

    fn compile(filter: Where) -> Result<Document, OrmError> {
        let mut cx = Cx::default();
        let scope = Scope::of(&TableRef::new("deals"));
        let mut pending = Pending::default();
        let result = condition(&mut cx, &scope, &mut pending, &filter);
        assert!(pending.stages.is_empty(), "no lookups for {filter:?}");
        result
    }

    fn column(name: &'static str) -> Expr {
        Expr::Column(ColumnRef::parse(name))
    }

    fn cmp(name: &'static str, op: &str, value: impl Into<Value>) -> Where {
        Where::Compare {
            left: column(name),
            op: Operator::from(op),
            right: Expr::Value(value.into()),
        }
    }

    #[test]
    fn comparisons_use_the_query_language() {
        assert_eq!(
            compile(cmp("stage", "=", "won")).unwrap(),
            doc! { "stage": "won" }
        );
        assert_eq!(
            compile(cmp("deals.amount", ">=", 100_i64)).unwrap(),
            doc! { "amount": { "$gte": 100_i64 } }
        );
        assert_eq!(
            compile(cmp("stage", "<>", "won")).unwrap(),
            doc! { "stage": { "$ne": "won" } }
        );
        assert_eq!(
            compile(cmp("closed_at", "=", Value::Null(navree_orm::Kind::String))).unwrap(),
            doc! { "closed_at": Bson::Null }
        );
        assert_eq!(
            compile(cmp("amount", ">", Value::Null(navree_orm::Kind::I64))).unwrap(),
            nothing()
        );
        assert_eq!(
            compile(cmp("meta->tags->0", "=", "vip")).unwrap(),
            doc! { "meta.tags.0": "vip" }
        );
        assert_eq!(
            compile(cmp("meta", "=", serde_json::json!({ "$gt": 1 }))).unwrap(),
            doc! { "meta": { "$eq": { "$gt": 1 } } },
            "documents are compared, never read as operators"
        );
        assert_eq!(
            compile(cmp("flags", "&", 4_i64)).unwrap(),
            doc! { "flags": { "$bitsAnySet": 4_i64 } }
        );
        assert!(compile(cmp("flags", "<<", 1_i64)).is_err());
        let error = compile(cmp("stage", "=>", "x")).unwrap_err();
        assert!(
            error.to_string().contains("not a known operator"),
            "{error}"
        );
    }

    #[test]
    fn columns_compared_with_columns_use_expr() {
        let compare = |op: &str| Where::Compare {
            left: column("amount"),
            op: Operator::from(op),
            right: column("budget"),
        };
        let amount = doc! { "$ifNull": ["$amount", Bson::Null] };
        let budget = doc! { "$ifNull": ["$budget", Bson::Null] };
        // Aggregation holds `null = null` and orders null below every
        // value, so a comparison first checks the side a null would
        // satisfy, as SQL never holds for NULL.
        assert_eq!(
            compile(compare("=")).unwrap(),
            doc! { "$expr": { "$and": [
                { "$gt": [amount.clone(), Bson::Null] },
                { "$eq": [amount.clone(), budget.clone()] },
            ] } }
        );
        assert_eq!(
            compile(compare("<=>")).unwrap(),
            doc! { "$expr": { "$eq": [amount.clone(), budget.clone()] } },
            "null-safe equality holds for two nulls"
        );
        assert_eq!(
            compile(compare("<>")).unwrap(),
            doc! { "$expr": { "$ne": [amount.clone(), budget.clone()] } }
        );
        assert_eq!(
            compile(compare(">")).unwrap(),
            doc! { "$expr": { "$and": [
                { "$gt": [budget.clone(), Bson::Null] },
                { "$gt": [amount.clone(), budget.clone()] },
            ] } }
        );
        assert_eq!(
            compile(compare("<=")).unwrap(),
            doc! { "$expr": { "$and": [
                { "$gt": [amount.clone(), Bson::Null] },
                { "$lte": [amount, budget] },
            ] } }
        );
    }

    #[test]
    fn computed_values_below_a_bound_are_not_null() {
        let month = Where::DatePart {
            part: DatePart::Month,
            expr: column("closes_on"),
            op: Operator::Lt,
            value: Expr::Value(6_i64.into()),
        };
        assert_eq!(
            compile(month).unwrap(),
            doc! { "$expr": { "$and": [
                { "$gt": [{ "$month": "$closes_on" }, Bson::Null] },
                { "$lt": [{ "$month": "$closes_on" }, 6_i64] },
            ] } }
        );
        let shorter = Where::JsonLength {
            column: ColumnRef::parse("tags"),
            op: Operator::Lt,
            length: 2,
        };
        let size = doc! { "$cond": [{ "$isArray": "$tags" }, { "$size": "$tags" }, Bson::Null] };
        assert_eq!(
            compile(shorter).unwrap(),
            doc! { "$expr": { "$and": [{ "$gt": [size.clone(), Bson::Null] }, { "$lt": [size, 2_i64] }] } }
        );
        let range = Where::Between {
            expr: Expr::Function {
                name: "length".into(),
                args: vec![column("title")],
            },
            low: Expr::Value(2_i64.into()),
            high: column("max_length"),
            not: false,
        };
        let length = doc! { "$strLenCP": "$title" };
        assert_eq!(
            compile(range).unwrap(),
            doc! { "$expr": { "$and": [
                { "$gte": [length.clone(), 2_i64] },
                { "$and": [
                    { "$gt": [length.clone(), Bson::Null] },
                    { "$lte": [length, { "$ifNull": ["$max_length", Bson::Null] }] },
                ] },
            ] } }
        );
    }

    #[test]
    fn like_becomes_an_escaped_regex() {
        let like = |pattern: &str, case_sensitive, not| Where::Like {
            expr: column("title"),
            pattern: Expr::Value(Value::from(pattern)),
            case_sensitive,
            not,
        };
        assert_eq!(
            compile(like("acme%", false, false)).unwrap(),
            doc! { "title": { "$regex": "^acme", "$options": "is" } }
        );
        assert_eq!(
            compile(like("%1+1%", true, true)).unwrap(),
            doc! { "title": { "$not": { "$regex": "1\\+1", "$options": "s" } } }
        );
        assert_eq!(
            compile(cmp("title", "ilike", "%co_")).unwrap(),
            doc! { "title": { "$regex": "co.\\z", "$options": "is" } }
        );
    }

    #[test]
    fn text_ignoring_case_becomes_a_literal_case_insensitive_regex() {
        let ignore_case = |text: &str, mode| Where::IgnoreCase {
            expr: column("title"),
            text: text.to_owned(),
            mode,
        };
        assert_eq!(
            compile(ignore_case("Équipe", IgnoreCaseMode::Contains)).unwrap(),
            doc! { "title": { "$regex": "Équipe", "$options": "is" } }
        );
        assert_eq!(
            compile(ignore_case("50%_off\\", IgnoreCaseMode::Contains)).unwrap(),
            doc! { "title": { "$regex": "50%_off\\\\", "$options": "is" } }
        );
        assert_eq!(
            compile(ignore_case("a.c+", IgnoreCaseMode::Equals)).unwrap(),
            doc! { "title": { "$regex": "^a\\.c\\+\\z", "$options": "is" } }
        );
    }

    #[test]
    fn nulls_lists_and_ranges() {
        assert_eq!(
            compile(Where::Null {
                expr: column("deleted_at"),
                not: false
            })
            .unwrap(),
            doc! { "deleted_at": Bson::Null }
        );
        assert_eq!(
            compile(Where::Null {
                expr: column("deleted_at"),
                not: true
            })
            .unwrap(),
            doc! { "deleted_at": { "$ne": Bson::Null } }
        );
        let list = |not| Where::In {
            expr: column("stage"),
            values: InValues::List(vec![Expr::Value("won".into()), Expr::Value("lost".into())]),
            not,
        };
        assert_eq!(
            compile(list(false)).unwrap(),
            doc! { "stage": { "$in": ["won", "lost"] } }
        );
        assert_eq!(
            compile(list(true)).unwrap(),
            doc! { "stage": { "$nin": ["won", "lost"] } }
        );
        let empty = Where::In {
            expr: column("stage"),
            values: InValues::List(Vec::new()),
            not: false,
        };
        assert_eq!(compile(empty).unwrap(), doc! { "stage": { "$in": [] } });
        let range = |not| Where::Between {
            expr: column("amount"),
            low: Expr::Value(10_i64.into()),
            high: Expr::Value(20_i64.into()),
            not,
        };
        assert_eq!(
            compile(range(false)).unwrap(),
            doc! { "amount": { "$gte": 10_i64, "$lte": 20_i64 } }
        );
        assert_eq!(
            compile(range(true)).unwrap(),
            doc! { "amount": { "$not": { "$gte": 10_i64, "$lte": 20_i64 } } }
        );
    }

    #[test]
    fn groups_follow_sql_precedence_and_negation() {
        let clauses_of = |clauses: Vec<WhereClause>| {
            let mut cx = Cx::default();
            let scope = Scope::of(&TableRef::new("deals"));
            let mut pending = Pending::default();
            super::clauses(&mut cx, &scope, &mut pending, &clauses).unwrap()
        };
        // a AND b OR c
        let filter = clauses_of(vec![
            WhereClause::and(cmp("a", "=", 1_i64)),
            WhereClause::and(cmp("b", "=", 2_i64)),
            WhereClause::or(cmp("c", "=", 3_i64)),
        ]);
        assert_eq!(
            filter,
            doc! { "$or": [{ "a": 1_i64, "b": 2_i64 }, { "c": 3_i64 }] }
        );
        // Colliding keys stay apart.
        let filter = clauses_of(vec![
            WhereClause::and(cmp("a", ">", 1_i64)),
            WhereClause::and(cmp("a", "<", 5_i64)),
        ]);
        assert_eq!(
            filter,
            doc! { "$and": [{ "a": { "$gt": 1_i64 } }, { "a": { "$lt": 5_i64 } }] }
        );
        let negated = clauses_of(vec![WhereClause {
            boolean: Boolean::And,
            negated: true,
            filter: cmp("a", "=", 1_i64),
        }]);
        assert_eq!(negated, doc! { "$nor": [{ "a": 1_i64 }] });
        assert_eq!(clauses_of(Vec::new()), Document::new());
    }

    #[test]
    fn date_parts_use_expr_in_utc() {
        let part = |part, value: Value| Where::DatePart {
            part,
            expr: column("created_at"),
            op: Operator::Eq,
            value: Expr::Value(value),
        };
        assert_eq!(
            compile(part(DatePart::Date, Value::from("2026-10-27"))).unwrap(),
            doc! { "$expr": { "$eq": [{ "$dateToString": { "format": "%Y-%m-%d", "date": "$created_at" } }, "2026-10-27"] } }
        );
        assert_eq!(
            compile(part(DatePart::Month, Value::from(10_i32))).unwrap(),
            doc! { "$expr": { "$eq": [{ "$month": "$created_at" }, 10_i64] } }
        );
        assert_eq!(
            compile(part(DatePart::Time, Value::from("08:30"))).unwrap(),
            doc! { "$expr": { "$eq": [{ "$dateToString": { "format": "%H:%M:%S", "date": "$created_at" } }, "08:30:00"] } }
        );
        assert!(compile(part(DatePart::Date, Value::from("tomorrow"))).is_err());
    }

    #[test]
    fn json_paths_contain_and_length() {
        let contains = |value: serde_json::Value, not| Where::JsonContains {
            column: ColumnRef::parse("meta->tags"),
            value,
            not,
        };
        assert_eq!(
            compile(contains(serde_json::json!("vip"), false)).unwrap(),
            doc! { "meta.tags": "vip" }
        );
        assert_eq!(
            compile(contains(serde_json::json!(["a", "b"]), false)).unwrap(),
            doc! { "meta.tags": { "$all": ["a", "b"] } }
        );
        assert_eq!(
            compile(contains(
                serde_json::json!({ "level": 2, "by": "ada" }),
                true
            ))
            .unwrap(),
            doc! { "$nor": [{ "meta.tags.by": "ada", "meta.tags.level": 2 }] }
        );
        let length = |op: &str| Where::JsonLength {
            column: ColumnRef::parse("meta->tags"),
            op: Operator::from(op),
            length: 2,
        };
        assert_eq!(
            compile(length("=")).unwrap(),
            doc! { "meta.tags": { "$size": 2_i64 } }
        );
        assert_eq!(
            compile(length(">")).unwrap(),
            doc! { "$expr": { "$gt": [{ "$cond": [{ "$isArray": "$meta.tags" }, { "$size": "$meta.tags" }, Bson::Null] }, 2_i64] } }
        );
        assert_eq!(
            compile(cmp("meta", "?", "vip")).unwrap(),
            doc! { "meta.vip": { "$exists": true } }
        );
    }

    #[test]
    fn full_text_uses_text_indexes() {
        let text = |query: &str, options: FullText| Where::FullText {
            columns: vec![column("title")],
            query: query.into(),
            options,
        };
        assert_eq!(
            compile(text("renewal -draft \"net\"", FullText::default())).unwrap(),
            doc! { "$text": { "$search": "renewal draft net" } }
        );
        assert_eq!(
            compile(text("net 30", FullText::phrase().language("German"))).unwrap(),
            doc! { "$text": { "$search": "\"net 30\"", "$language": "german" } }
        );
        assert_eq!(
            compile(text("\"net 30\" -draft", FullText::websearch())).unwrap(),
            doc! { "$text": { "$search": "\"net 30\" -draft" } }
        );
        assert!(compile(text("x", FullText::default().language("klingon"))).is_err());
    }

    #[test]
    fn raw_sql_and_foreign_native_filters_are_refused() {
        let error = compile(Where::Raw {
            sql: "1 = 1".into(),
            values: Vec::new(),
        })
        .unwrap_err();
        assert!(matches!(error, OrmError::Unsupported { .. }), "{error:?}");
        assert!(error.to_string().contains("Filter::document"), "{error}");
        let native = navree_orm::ir::NativeFilter::new("mongodb", doc! { "$where": "x" });
        assert_eq!(
            compile(Where::Native(native)).unwrap(),
            doc! { "$where": "x" }
        );
        let other = navree_orm::ir::NativeFilter::new("memory", "x == 1".to_owned());
        assert!(compile(Where::Native(other)).is_err());
    }
}
