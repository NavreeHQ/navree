//! Grouped queries as `$group` stages: the group keys under `_id`, one
//! accumulator per aggregate (TimescaleDB's `first` and `last` as `$top`
//! and `$bottom`), the selected columns, `HAVING` and the order over the
//! groups.

use bson::{Bson, Document, doc};
use navree_orm::OrmError;
use navree_orm::ir::{
    Boolean, Direction, Expr, InValues, Nulls, Operator, Order, QueryIr, SelectItem, Where,
    WhereClause,
};

use super::Pending;
use super::expr::{self, Accumulator, literal};
use super::filter;
use super::like::like_regex;
use super::read::{UNNAMED, default_name, page};
use super::scope::{Cx, Place, Scope, output_name};
use crate::convert::to_bson;
use crate::error::{invalid, unsupported};

/// `value LIKE pattern` over the groups, as `$regexMatch`.
fn like(value: Bson, pattern: &Expr, insensitive: bool, negated: bool) -> Result<Bson, OrmError> {
    let Expr::Value(navree_orm::Value::String(Some(pattern))) = pattern else {
        return Err(invalid("a LIKE in HAVING takes a text pattern"));
    };
    let options = if insensitive { "is" } else { "s" };
    let matched = Bson::Document(doc! {
        "$regexMatch": { "input": { "$toString": value }, "regex": like_regex(pattern), "options": options }
    });
    Ok(if negated {
        Bson::Document(doc! { "$not": [matched] })
    } else {
        matched
    })
}

/// Whether `expr` holds an aggregate.
pub(crate) fn has_aggregate(expr: &Expr) -> bool {
    match expr {
        Expr::Aggregate { .. } | Expr::First { .. } | Expr::Last { .. } => true,
        Expr::Function { args, .. } => args.iter().any(has_aggregate),
        Expr::Arithmetic { left, right, .. } => has_aggregate(left) || has_aggregate(right),
        _ => false,
    }
}

/// Expressions over the grouped documents: group keys are `$_id.g<n>`,
/// aggregates their accumulator's field, output columns their name.
struct Post<'g> {
    /// The group expressions and their field under `_id`.
    groups: &'g [(Expr, String)],
    /// `(key, field, accumulator)`, one per distinct aggregate.
    accumulators: Vec<(String, String, Accumulator)>,
    /// The output columns (visible to `HAVING` and the order by name).
    aliases: Vec<String>,
}

impl Post<'_> {
    /// The field under `_id` of a group expression equal to `expr`.
    fn group_field(&self, expr: &Expr) -> Option<&str> {
        let key = format!("{expr:?}");
        if let Some((_, field)) = self.groups.iter().find(|(group, _)| format!("{group:?}") == key) {
            return Some(field);
        }
        // A column matches a grouped column of the same name and path,
        // qualified or not.
        let Expr::Column(column) = expr else {
            return None;
        };
        self.groups
            .iter()
            .find(|(group, _)| {
                matches!(group, Expr::Column(grouped)
                    if grouped.name == column.name
                        && grouped.path == column.path
                        && (grouped.table.is_none() || column.table.is_none() || grouped.table == column.table))
            })
            .map(|(_, field)| field.as_str())
    }

    fn value(
        &mut self,
        cx: &mut Cx,
        scope: &Scope<'_>,
        pending: &mut Pending,
        expr: &Expr,
    ) -> Result<Bson, OrmError> {
        if let Some(field) = self.group_field(expr) {
            return Ok(Bson::String(format!("$_id.{field}")));
        }
        Ok(match expr {
            Expr::Column(column)
                if column.table.is_none()
                    && column.path.is_empty()
                    && self.aliases.iter().any(|alias| *alias == column.name) =>
            {
                Bson::String(format!("${}", column.name))
            }
            Expr::Column(column) => {
                return Err(invalid(format!(
                    "`{column}` is selected but neither grouped nor aggregated: group by it, or aggregate it"
                )));
            }
            Expr::Value(value) => literal(to_bson(value)?),
            Expr::Aggregate {
                function,
                arg,
                distinct,
            } => {
                let key = format!("{expr:?}");
                if let Some((_, field, accumulator)) =
                    self.accumulators.iter().find(|(known, _, _)| *known == key)
                {
                    return Ok(accumulator.output(field));
                }
                let argument = arg
                    .as_deref()
                    .map(|arg| expr::value(cx, scope, pending, arg))
                    .transpose()?;
                let accumulator = expr::accumulator(*function, argument, *distinct)?;
                let field = cx.field("a");
                let output = accumulator.output(&field);
                self.accumulators.push((key, field, accumulator));
                output
            }
            Expr::First { value, time } | Expr::Last { value, time } => {
                let key = format!("{expr:?}");
                if let Some((_, field, accumulator)) =
                    self.accumulators.iter().find(|(known, _, _)| *known == key)
                {
                    return Ok(accumulator.output(field));
                }
                let Expr::Column(time) = time.as_ref() else {
                    return Err(unsupported(
                        "first() and last() by a computed time (MongoDB sorts groups by a field)",
                    ));
                };
                let Place::Field(time) = scope.place(cx, time)? else {
                    return Err(unsupported(
                        "first() and last() by a column of an enclosing query",
                    ));
                };
                let output = expr::value(cx, scope, pending, value)?;
                let accumulator =
                    expr::first_last(matches!(expr, Expr::First { .. }), output, &time);
                let field = cx.field("a");
                let result = accumulator.output(&field);
                self.accumulators.push((key, field, accumulator));
                result
            }
            Expr::Function { name, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.value(cx, scope, pending, arg))
                    .collect::<Result<Vec<_>, _>>()?;
                expr::function(name, args)?
            }
            Expr::Arithmetic { left, op, right } => {
                let operator = match op {
                    navree_orm::ir::ArithmeticOp::Add => "$add",
                    navree_orm::ir::ArithmeticOp::Sub => "$subtract",
                    navree_orm::ir::ArithmeticOp::Mul => "$multiply",
                    navree_orm::ir::ArithmeticOp::Div => "$divide",
                    other => return Err(unsupported(format!("the arithmetic operator {other:?}"))),
                };
                let left = self.value(cx, scope, pending, left)?;
                let right = self.value(cx, scope, pending, right)?;
                Bson::Document(doc! { operator: [left, right] })
            }
            Expr::TimeBucket { .. } => {
                return Err(invalid(
                    "a time bucket is selected but not grouped: group by its alias (`group_by([\"bucket\"])`)",
                ));
            }
            Expr::Raw { .. } => {
                return Err(unsupported(
                    "raw SQL expressions (write them with typed columns and functions)",
                ));
            }
            Expr::Subquery(_) => {
                return Err(unsupported(
                    "a subquery in a grouped select or HAVING (group in a derived table with `from_sub`, then select the subquery over it)",
                ));
            }
            Expr::All { .. } => {
                return Err(invalid(
                    "`*` in a grouped query: select the group keys and aggregates",
                ));
            }
            other => {
                return Err(unsupported(format!(
                    "{other:?} in a grouped query (select group keys, aggregates, and expressions of them)"
                )));
            }
        })
    }

    /// `HAVING` clauses, with SQL's precedence.
    fn clauses(
        &mut self,
        cx: &mut Cx,
        scope: &Scope<'_>,
        pending: &mut Pending,
        clauses: &[WhereClause],
    ) -> Result<Document, OrmError> {
        let mut groups: Vec<Vec<Document>> = vec![Vec::new()];
        for (i, clause) in clauses.iter().enumerate() {
            let mut condition = self.condition(cx, scope, pending, &clause.filter)?;
            if clause.negated {
                condition = filter::not(condition);
            }
            if i > 0 && clause.boolean == Boolean::Or {
                groups.push(Vec::new());
            }
            if let Some(group) = groups.last_mut() {
                group.push(condition);
            }
        }
        let mut groups: Vec<Document> = groups.into_iter().map(filter::and).collect();
        Ok(if groups.len() == 1 {
            groups.remove(0)
        } else {
            filter::or(groups)
        })
    }

    fn condition(
        &mut self,
        cx: &mut Cx,
        scope: &Scope<'_>,
        pending: &mut Pending,
        condition: &Where,
    ) -> Result<Document, OrmError> {
        let expr = |value: Bson| doc! { "$expr": value };
        Ok(match condition {
            Where::Compare { left, op, right } => {
                let comparison = match op {
                    Operator::Eq | Operator::NullSafeEq => Some("$eq"),
                    Operator::Ne => Some("$ne"),
                    Operator::Lt => Some("$lt"),
                    Operator::Lte => Some("$lte"),
                    Operator::Gt => Some("$gt"),
                    Operator::Gte => Some("$gte"),
                    _ => None,
                };
                let l = self.value(cx, scope, pending, left)?;
                match (comparison, op) {
                    (Some(_), Operator::NullSafeEq) => {
                        let r = self.value(cx, scope, pending, right)?;
                        expr(filter::null_safe_equal(l, r))
                    }
                    (Some(operator), _) => {
                        let r = self.value(cx, scope, pending, right)?;
                        expr(filter::compared(operator, l, r))
                    }
                    (None, Operator::Like | Operator::NotLike | Operator::ILike | Operator::NotILike) => {
                        expr(like(
                            l,
                            right,
                            matches!(op, Operator::ILike | Operator::NotILike),
                            matches!(op, Operator::NotLike | Operator::NotILike),
                        )?)
                    }
                    (None, Operator::Invalid(op)) => {
                        return Err(invalid(format!("`{op}` is not a known operator")));
                    }
                    (None, other) => {
                        return Err(unsupported(format!(
                            "the operator `{}` in HAVING (compare with =, <>, <, <=, >, >= or LIKE)",
                            filter::operator_text(other)
                        )));
                    }
                }
            }
            Where::Like {
                expr: value,
                pattern,
                case_sensitive,
                not,
            } => {
                let value = self.value(cx, scope, pending, value)?;
                expr(like(value, pattern, !*case_sensitive, *not)?)
            }
            Where::Null { expr: value, not } => {
                let value = self.value(cx, scope, pending, value)?;
                let operator = if *not { "$ne" } else { "$eq" };
                expr(Bson::Document(doc! {
                    operator: [{ "$ifNull": [value, Bson::Null] }, Bson::Null]
                }))
            }
            Where::In {
                expr: value,
                values: InValues::List(list),
                not,
            } => {
                let value = self.value(cx, scope, pending, value)?;
                let list = list
                    .iter()
                    .map(|item| self.value(cx, scope, pending, item))
                    .collect::<Result<Vec<_>, _>>()?;
                let found = Bson::Document(doc! { "$in": [value, list] });
                expr(if *not {
                    Bson::Document(doc! { "$not": [found] })
                } else {
                    found
                })
            }
            Where::Between {
                expr: value,
                low,
                high,
                not,
            } => {
                let value = self.value(cx, scope, pending, value)?;
                let low = self.value(cx, scope, pending, low)?;
                let high = self.value(cx, scope, pending, high)?;
                let within = Bson::Document(doc! {
                    "$and": [
                        filter::compared("$gte", value.clone(), low),
                        filter::compared("$lte", value, high),
                    ]
                });
                expr(if *not {
                    Bson::Document(doc! { "$not": [within] })
                } else {
                    within
                })
            }
            Where::Group(inner) => self.clauses(cx, scope, pending, inner)?,
            Where::Raw { .. } | Where::Sql(_) => {
                return Err(unsupported(
                    "raw SQL in HAVING (`having_raw`; compare aggregates and group keys)",
                ));
            }
            Where::Invalid(message) => return Err(invalid(message.clone())),
            other => {
                return Err(unsupported(format!(
                    "{} in HAVING (HAVING compares group keys and aggregates; filter the documents with `where`)",
                    filter::kind(other)
                )));
            }
        })
    }

    /// The order over the groups: `($set before the sort, $sort)`.
    fn order(
        &mut self,
        cx: &mut Cx,
        scope: &Scope<'_>,
        pending: &mut Pending,
        orders: &[Order],
    ) -> Result<(Document, Document), OrmError> {
        let mut set = Document::new();
        let mut sort = Document::new();
        for order in orders {
            match order {
                Order::By {
                    expr,
                    direction,
                    nulls,
                } => {
                    let value = self.value(cx, scope, pending, expr)?;
                    let key = match &value {
                        Bson::String(path) if path.starts_with('$') && !path.starts_with("$$") => {
                            path[1..].to_owned()
                        }
                        _ => {
                            let temp = cx.field("order");
                            set.insert(temp.clone(), value.clone());
                            temp
                        }
                    };
                    if let Some(nulls) = nulls {
                        let natural = matches!(
                            (direction, nulls),
                            (Direction::Asc, Nulls::First) | (Direction::Desc, Nulls::Last)
                        );
                        if !natural {
                            let flag = cx.field("nulls");
                            set.insert(
                                flag.clone(),
                                doc! { "$cond": [{ "$eq": [{ "$ifNull": [value, Bson::Null] }, Bson::Null] }, 1, 0] },
                            );
                            sort.insert(flag, if *nulls == Nulls::Last { 1 } else { -1 });
                        }
                    }
                    sort.insert(key, if *direction == Direction::Asc { 1 } else { -1 });
                }
                Order::Random => {
                    let temp = cx.field("random");
                    set.insert(temp.clone(), doc! { "$rand": {} });
                    sort.insert(temp, 1);
                }
                Order::Raw { .. } => {
                    return Err(unsupported(
                        "raw SQL orders (`order_by_raw`; order by columns or aggregates)",
                    ));
                }
                other => return Err(unsupported(format!("the order {other:?}"))),
            }
        }
        Ok((set, sort))
    }
}

/// The stages of a grouped query, after its `$match`: `$group`, the
/// outputs, `HAVING`, the order and the page, the projection. Returns the
/// output columns.
pub(crate) fn stages(
    cx: &mut Cx,
    scope: &Scope<'_>,
    query: &QueryIr,
    stages: &mut Vec<Document>,
    ordered_here: bool,
) -> Result<Option<Vec<String>>, OrmError> {
    let mut pending = Pending::default();
    let mut groups = Vec::with_capacity(query.groups.len());
    let mut id = Document::new();
    for (i, group) in query.groups.iter().enumerate() {
        // `group_by(["bucket"])` names a selected expression by its alias.
        let group = match group {
            Expr::Column(column) if column.table.is_none() && column.path.is_empty() => query
                .columns
                .iter()
                .find(|item| {
                    item.alias.as_deref() == Some(column.name.as_ref())
                        && !matches!(&item.expr, Expr::Column(own) if own.name == column.name)
                })
                .map_or_else(|| group.clone(), |item| item.expr.clone()),
            other => other.clone(),
        };
        let field = format!("g{i}");
        id.insert(field.clone(), expr::value(cx, scope, &mut pending, &group)?);
        groups.push((group, field));
    }
    let items: Vec<SelectItem> = if query.columns.is_empty() {
        query
            .groups
            .iter()
            .cloned()
            .map(|expr| SelectItem { expr, alias: None })
            .collect()
    } else {
        query.columns.clone()
    };

    let mut post = Post {
        groups: &groups,
        accumulators: Vec::new(),
        aliases: Vec::new(),
    };
    let mut outputs: Vec<(String, Bson)> = Vec::with_capacity(items.len());
    for item in &items {
        if matches!(item.expr, Expr::All { .. }) {
            return Err(invalid(
                "`*` in a grouped query: select the group keys and aggregates",
            ));
        }
        let value = post.value(cx, scope, &mut pending, &item.expr)?;
        let name = match &item.alias {
            Some(alias) => output_name(alias)?.to_owned(),
            None => default_name(&item.expr)
                .ok_or_else(|| invalid(UNNAMED))?
                .into_owned(),
        };
        if outputs.iter().any(|(output, _)| *output == name) {
            return Err(invalid(format!(
                "the query selects two columns named `{name}`: give one an alias"
            )));
        }
        outputs.push((name, value));
    }
    post.aliases = outputs.iter().map(|(name, _)| name.clone()).collect();
    let having = post.clauses(cx, scope, &mut pending, &query.havings)?;
    let (sort_set, sort) = if ordered_here {
        post.order(cx, scope, &mut pending, &query.orders)?
    } else {
        (Document::new(), Document::new())
    };

    pending.flush(stages);
    let mut group = doc! {
        "_id": if groups.is_empty() { Bson::Null } else { Bson::Document(id) }
    };
    for (_, field, accumulator) in &post.accumulators {
        group.insert(field.clone(), accumulator.op.clone());
    }
    stages.push(doc! { "$group": group });
    let set: Document = outputs
        .iter()
        .filter(|(name, _)| name != "_id")
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if !set.is_empty() {
        stages.push(doc! { "$set": set });
    }
    if !having.is_empty() {
        stages.push(doc! { "$match": having });
    }
    if !sort_set.is_empty() {
        stages.push(doc! { "$set": sort_set });
    }
    if !sort.is_empty() {
        stages.push(doc! { "$sort": sort });
    }
    if ordered_here {
        page(query, stages);
    }
    let mut project = Document::new();
    for (name, value) in &outputs {
        if name == "_id" {
            project.insert("_id", value.clone());
        } else {
            project.insert(name.clone(), 1);
        }
    }
    if !project.contains_key("_id") {
        project.insert("_id", 0);
    }
    stages.push(doc! { "$project": project });
    Ok(Some(outputs.into_iter().map(|(name, _)| name).collect()))
}
