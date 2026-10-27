//! Read queries ([`QueryIr`]) as aggregation pipelines, shown and run as a
//! `find` when they are one.

use std::borrow::Cow;

use bson::{Bson, Document, doc};
use navree_orm::OrmError;
use navree_orm::ir::{
    Aggregate as AggregateIr, AggregateFunction, Boolean, ColumnRef, Direction, Distinct, Expr,
    Join, JoinKind, Nulls, Order, QueryIr, SelectItem, TableRef, Where, WhereClause,
};

use super::expr::{self, Accumulator};
use super::filter::{self, nothing};
use super::group;
use super::scope::{Cx, Place, Scope, output_name};
use super::Pending;
use crate::command::{Aggregate, Count, Find, MongoCommand};
use crate::error::{invalid, unsupported};

/// A read planned as a pipeline.
#[derive(Debug)]
pub(crate) struct Plan {
    /// The collection the pipeline starts from.
    pub(crate) collection: String,
    /// The stages.
    pub(crate) stages: Vec<Document>,
    /// The output columns in order, when the query names them (`None`:
    /// every field of the documents).
    pub(crate) columns: Option<Vec<String>>,
}

impl Plan {
    /// The plan as a command: a `find` when its stages are a filter, a
    /// sort, a skip, a limit and a projection, an aggregation otherwise.
    pub(crate) fn command(self) -> MongoCommand {
        match as_find(&self.collection, &self.stages) {
            Some(find) => MongoCommand::Find(find),
            None => MongoCommand::Aggregate(Aggregate {
                collection: self.collection,
                pipeline: self.stages,
            }),
        }
    }
}

/// What MongoDB says about row locks.
const LOCKS: &str = "row locks (`lock_for_update`, `shared_lock`): MongoDB has none; run the read and the write in a `MongoUow`, where two transactions writing the same document conflict and one is retried";

/// Plans `query` in `scope` (a fresh scope for a top-level query, a child
/// scope for a subquery).
pub(crate) fn plan(cx: &mut Cx, scope: &mut Scope<'_>, query: &QueryIr) -> Result<Plan, OrmError> {
    query.check()?;
    if query.lock.is_some() {
        return Err(unsupported(LOCKS));
    }
    let mut stages = Vec::new();
    let collection = match &query.table.subquery {
        Some(inner) => {
            let mut derived = scope.sibling(&inner.table);
            let inner_plan = plan(cx, &mut derived, inner)?;
            scope.absorb(&derived);
            stages.extend(inner_plan.stages);
            inner_plan.collection
        }
        None => query.table.name.to_string(),
    };

    // A full-text condition must be the pipeline's first stage.
    let (text, wheres) = split_text(&query.wheres)?;
    if let Some(text) = text {
        if !stages.is_empty() || !query.joins.is_empty() {
            return Err(unsupported(
                "a full-text condition on a derived table or a join: MongoDB runs `$text` first, on the collection itself",
            ));
        }
        stages.push(doc! { "$match": text });
    }

    for join in &query.joins {
        join_stages(cx, scope, join, &mut stages)?;
    }

    let mut pending = Pending::default();
    let filter = filter::clauses(cx, scope, &mut pending, &wheres)?;
    let temps = pending.flush(&mut stages);
    if !filter.is_empty() {
        stages.push(doc! { "$match": filter });
    }
    if !temps.is_empty() {
        stages.push(doc! { "$unset": temps });
    }

    let grouped = !query.groups.is_empty()
        || !query.havings.is_empty()
        || query
            .columns
            .iter()
            .any(|item| group::has_aggregate(&item.expr));
    let ordered_here = query.unions.is_empty();
    let columns = if grouped {
        group::stages(cx, scope, query, &mut stages, ordered_here)?
    } else {
        plain(cx, scope, query, &mut stages, ordered_here)?
    };

    if !query.unions.is_empty() {
        let mut distinct = false;
        for union in &query.unions {
            let mut other_scope = Scope::of(&union.query.table);
            let other = plan(cx, &mut other_scope, &union.query)?;
            let mut with = doc! { "coll": other.collection };
            if !other.stages.is_empty() {
                with.insert("pipeline", other.stages);
            }
            stages.push(doc! { "$unionWith": with });
            distinct |= !union.all;
        }
        if distinct {
            stages.push(doc! { "$group": { "_id": "$$ROOT" } });
            stages.push(doc! { "$replaceWith": "$_id" });
        }
        let outputs = Scope::of(&TableRef::new("union"));
        let temps = order(cx, &outputs, &query.orders, None, &mut stages)?;
        page(query, &mut stages);
        if !temps.is_empty() {
            stages.push(doc! { "$unset": temps });
        }
    }

    Ok(Plan {
        collection,
        stages,
        columns,
    })
}

/// Takes a top-level full-text condition (joined with AND) out of
/// `wheres`; anywhere else it is an error, as MongoDB runs `$text` once,
/// on its own.
fn split_text(wheres: &[WhereClause]) -> Result<(Option<Document>, Vec<WhereClause>), OrmError> {
    let has_or = wheres
        .iter()
        .skip(1)
        .any(|clause| clause.boolean == Boolean::Or);
    let mut text = None;
    let mut rest = Vec::with_capacity(wheres.len());
    for clause in wheres {
        match &clause.filter {
            Where::FullText { .. } if !has_or && !clause.negated && text.is_none() => {
                let mut cx = Cx::default();
                let scope = Scope::of(&TableRef::new("text"));
                text = Some(filter::condition(
                    &mut cx,
                    &scope,
                    &mut Pending::default(),
                    &clause.filter,
                )?);
            }
            other if mentions_text(other) => {
                return Err(unsupported(
                    "a full-text condition inside OR, NOT or a group, or twice: MongoDB runs one `$text`, joined with AND at the top of the filter",
                ));
            }
            _ => rest.push(clause.clone()),
        }
    }
    Ok((text, rest))
}

fn mentions_text(filter: &Where) -> bool {
    match filter {
        Where::FullText { .. } => true,
        Where::Group(clauses) => clauses.iter().any(|clause| mentions_text(&clause.filter)),
        _ => false,
    }
}

/// A join as a `$lookup` of the joined collection under the join's
/// qualifier, unwound: inner joins keep the documents that matched, left
/// joins every document.
fn join_stages(
    cx: &mut Cx,
    scope: &mut Scope<'_>,
    join: &Join,
    stages: &mut Vec<Document>,
) -> Result<(), OrmError> {
    let qualifier = join.table.qualifier().to_owned();
    super::scope::output_name(&qualifier)?;
    let keep_unmatched = match join.kind {
        JoinKind::Inner | JoinKind::Cross => false,
        JoinKind::Left => true,
        JoinKind::Right => {
            return Err(unsupported(
                "RIGHT JOIN: swap the collections and LEFT JOIN, MongoDB joins from the collection queried",
            ));
        }
        other => return Err(unsupported(format!("the join {other:?}"))),
    };
    let lookup = {
        // An equality between a joined field and one of ours is a plain
        // `localField`/`foreignField` lookup.
        let simple = match (&join.table.subquery, join.on.as_slice()) {
            (None, [clause]) if !clause.negated => match &clause.filter {
                Where::Compare {
                    left: Expr::Column(left),
                    op: navree_orm::ir::Operator::Eq,
                    right: Expr::Column(right),
                } => equality(cx, scope, &join.table, left, right)?,
                _ => None,
            },
            _ => None,
        };
        match simple {
            Some((local, foreign)) => doc! {
                "from": join.table.name.to_string(),
                "localField": local,
                "foreignField": foreign,
                "as": qualifier.clone(),
            },
            None => {
                let joined = scope.child(&join.table);
                let (collection, mut pipeline) = match &join.table.subquery {
                    Some(inner) => {
                        let mut inner_scope = Scope::of(&inner.table);
                        let inner_plan = plan(cx, &mut inner_scope, inner)?;
                        (inner_plan.collection, inner_plan.stages)
                    }
                    None => (join.table.name.to_string(), Vec::new()),
                };
                let mut pending = Pending::default();
                let on = filter::clauses(cx, &joined, &mut pending, &join.on)?;
                let temps = pending.flush(&mut pipeline);
                if !on.is_empty() {
                    pipeline.push(doc! { "$match": on });
                }
                if !temps.is_empty() {
                    pipeline.push(doc! { "$unset": temps });
                }
                let lets = joined.lets();
                let mut lookup = doc! { "from": collection };
                if !lets.is_empty() {
                    lookup.insert("let", lets.into_iter().collect::<Document>());
                }
                lookup.insert("pipeline", pipeline);
                lookup.insert("as", qualifier.clone());
                lookup
            }
        }
    };
    stages.push(doc! { "$lookup": lookup });
    stages.push(if keep_unmatched {
        doc! { "$unwind": { "path": format!("${qualifier}"), "preserveNullAndEmptyArrays": true } }
    } else {
        doc! { "$unwind": format!("${qualifier}") }
    });
    scope.join(&qualifier, &qualifier);
    Ok(())
}

/// `(localField, foreignField)` of an ON equality between a field of the
/// joined table and a field we have, if it is one.
fn equality(
    cx: &mut Cx,
    scope: &Scope<'_>,
    joined: &TableRef,
    left: &ColumnRef,
    right: &ColumnRef,
) -> Result<Option<(String, String)>, OrmError> {
    let is_joined = |column: &ColumnRef| {
        column
            .table
            .as_deref()
            .is_some_and(|table| table == joined.qualifier() || table == joined.name)
    };
    let (ours, theirs) = match (is_joined(left), is_joined(right)) {
        (false, true) => (left, right),
        (true, false) => (right, left),
        _ => return Ok(None),
    };
    let Place::Field(local) = scope.place(cx, ours)? else {
        return Ok(None);
    };
    let foreign = super::scope::field_path(theirs)?;
    Ok(Some((local, foreign)))
}

/// The select list of a query that is not grouped.
#[derive(Debug, Default)]
pub(crate) struct Projection {
    /// Lookups the select list needs (subqueries), run right before it.
    pending: Pending,
    /// `(output, value)` in order; a value of `1` keeps the field as is.
    outputs: Vec<(String, Bson)>,
    /// Whether every field of the documents is selected (`*`).
    all: bool,
    /// Computed outputs a sort already set (kept as they are).
    materialized: Vec<String>,
}

impl Projection {
    /// The output names, when the query names them.
    fn columns(&self) -> Option<Vec<String>> {
        (!self.all).then(|| self.outputs.iter().map(|(name, _)| name.clone()).collect())
    }

    /// The value of computed output `name`, if there is one.
    fn computed(&self, name: &str) -> Option<&Bson> {
        self.outputs
            .iter()
            .find(|(output, value)| output == name && *value != Bson::Int32(1))
            .map(|(_, value)| value)
    }

    /// The stages: pending lookups, then an inclusion `$project`, or for
    /// `*` the computed outputs set and the joined and temporary fields
    /// removed.
    fn apply(mut self, scope: &Scope<'_>, temps: Vec<String>, stages: &mut Vec<Document>) {
        let lookups = self.pending.flush(stages);
        if self.all {
            let set: Document = self
                .outputs
                .iter()
                .filter(|(name, _)| !self.materialized.contains(name))
                .filter(|(_, value)| *value != Bson::Int32(1))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            if !set.is_empty() {
                stages.push(doc! { "$set": set });
            }
            let mut unset: Vec<String> = scope.joined_fields().map(str::to_owned).collect();
            unset.extend(temps);
            unset.extend(lookups);
            if !unset.is_empty() {
                stages.push(doc! { "$unset": unset });
            }
            return;
        }
        let mut project = Document::new();
        for (name, value) in &self.outputs {
            let value = if self.materialized.contains(name) {
                Bson::Int32(1)
            } else {
                value.clone()
            };
            project.insert(name.clone(), value);
        }
        if !project.contains_key("_id") {
            project.insert("_id", 0);
        }
        stages.push(doc! { "$project": project });
    }
}

/// The name a selected column comes back under: its name, then its JSON
/// path (`meta->status`).
fn column_output(column: &ColumnRef) -> String {
    let mut name = column.name.to_string();
    for segment in &column.path {
        match segment {
            navree_orm::ir::JsonSegment::Key(key) => {
                name.push_str("->");
                name.push_str(key);
            }
            navree_orm::ir::JsonSegment::Index(index) => {
                name.push_str("->");
                name.push_str(&index.to_string());
            }
        }
    }
    name
}

/// The select list of a query that is not grouped.
fn projection(
    cx: &mut Cx,
    scope: &Scope<'_>,
    columns: &[SelectItem],
) -> Result<Projection, OrmError> {
    let mut projection = Projection::default();
    if columns.is_empty() {
        projection.all = true;
        return Ok(projection);
    }
    for item in columns {
        if let Expr::All { table } = &item.expr {
            match table.as_deref() {
                None => projection.all = true,
                Some(table) if scope.is_own(table) => projection.all = true,
                Some(table) => {
                    return Err(unsupported(format!(
                        "every column of another collection (`{table}.*`): name its columns, a joined document is the field `{table}`"
                    )));
                }
            }
            continue;
        }
        let (name, value) = match (&item.expr, &item.alias) {
            (Expr::Column(column), alias) => {
                let place = scope.place(cx, column)?;
                let name = match alias {
                    Some(alias) => output_name(alias)?.to_owned(),
                    None => column_output(column),
                };
                let value = match &place {
                    Place::Field(path) if *path == name => Bson::Int32(1),
                    other => other.expr(),
                };
                (name, value)
            }
            (other, Some(alias)) => (
                output_name(alias)?.to_owned(),
                expr::value(cx, scope, &mut projection.pending, other)?,
            ),
            (other, None) => {
                return Err(invalid(format!(
                    "a computed column needs a name on MongoDB: select {other:?} with an alias (`select_expr(expr, \"name\")`)"
                )));
            }
        };
        if projection.outputs.iter().any(|(output, _)| *output == name) {
            return Err(invalid(format!(
                "the query selects two columns named `{name}`: give one an alias"
            )));
        }
        projection.outputs.push((name, value));
    }
    Ok(projection)
}

/// The rest of a query that is not grouped: distinct, order, page,
/// projection.
fn plain(
    cx: &mut Cx,
    scope: &Scope<'_>,
    query: &QueryIr,
    stages: &mut Vec<Document>,
    ordered_here: bool,
) -> Result<Option<Vec<String>>, OrmError> {
    let mut projection = projection(cx, scope, &query.columns)?;
    let columns = projection.columns();
    match &query.distinct {
        Distinct::No => {
            let mut temps = Vec::new();
            if ordered_here {
                temps = order(cx, scope, &query.orders, Some(&mut projection), stages)?;
                page(query, stages);
            }
            projection.apply(scope, temps, stages);
        }
        Distinct::Yes => {
            projection.apply(scope, Vec::new(), stages);
            let id = match &columns {
                Some(columns) => Bson::Document(
                    columns
                        .iter()
                        .map(|name| (name.clone(), Bson::String(format!("${name}"))))
                        .collect(),
                ),
                None => Bson::String("$$ROOT".into()),
            };
            stages.push(doc! { "$group": { "_id": id } });
            stages.push(doc! { "$replaceWith": "$_id" });
            if ordered_here {
                let outputs = Scope::of(&TableRef::new("distinct"));
                let temps = order(cx, &outputs, &unqualified(&query.orders), None, stages)?;
                page(query, stages);
                if !temps.is_empty() {
                    stages.push(doc! { "$unset": temps });
                }
            }
        }
        Distinct::On(exprs) => {
            let temps = order(cx, scope, &query.orders, None, stages)?;
            let mut pending = Pending::default();
            let mut id = Document::new();
            for (i, expr) in exprs.iter().enumerate() {
                id.insert(format!("k{i}"), expr::value(cx, scope, &mut pending, expr)?);
            }
            pending.flush(stages);
            let first = cx.field("first");
            stages.push(doc! { "$group": { "_id": id, first.clone(): { "$first": "$$ROOT" } } });
            stages.push(doc! { "$replaceWith": format!("${first}") });
            if ordered_here {
                order(cx, scope, &query.orders, None, stages)?;
                page(query, stages);
            }
            projection.apply(scope, temps, stages);
        }
        other => return Err(unsupported(format!("{other:?}"))),
    }
    Ok(columns)
}

/// `orders` with their columns' tables dropped (after a projection, fields
/// are the output columns).
fn unqualified(orders: &[Order]) -> Vec<Order> {
    orders
        .iter()
        .map(|order| match order {
            Order::By {
                expr,
                direction,
                nulls,
            } => Order::By {
                expr: unqualified_expr(expr),
                direction: *direction,
                nulls: *nulls,
            },
            other => other.clone(),
        })
        .collect()
}

/// The `$sort` of `orders` (with a `$set` before it for computed keys,
/// random keys and NULL placement MongoDB's order does not give); returns
/// the temporary fields to remove. Ordering by the alias of a computed
/// column of `projection` computes that column first.
fn order(
    cx: &mut Cx,
    scope: &Scope<'_>,
    orders: &[Order],
    mut projection: Option<&mut Projection>,
    stages: &mut Vec<Document>,
) -> Result<Vec<String>, OrmError> {
    let mut sort = Document::new();
    let mut set = Document::new();
    let mut temps = Vec::new();
    let mut pending = Pending::default();
    for item in orders {
        match item {
            Order::By {
                expr,
                direction,
                nulls,
            } => {
                // The sort key, and the expression of its value (a key set
                // in this `$set` is not visible to the rest of it).
                let computed_alias = match (expr, projection.as_deref()) {
                    (Expr::Column(column), Some(projection))
                        if column.table.is_none() && column.path.is_empty() =>
                    {
                        projection
                            .computed(&column.name)
                            .cloned()
                            .map(|value| (column.name.to_string(), value))
                    }
                    _ => None,
                };
                let (key, value) = match (computed_alias, expr) {
                    (Some((name, value)), _) => {
                        // Sorting by a computed column computes it now, its
                        // lookups first.
                        if let Some(projection) = projection.as_deref_mut() {
                            pending.stages.append(&mut projection.pending.stages);
                            projection.materialized.push(name.clone());
                        }
                        set.insert(name.clone(), value.clone());
                        (name, value)
                    }
                    (None, Expr::Column(column)) => match scope.place(cx, column)? {
                        Place::Field(path) => {
                            let value = Bson::String(format!("${path}"));
                            (path, value)
                        }
                        variable => {
                            let temp = cx.field("order");
                            set.insert(temp.clone(), variable.expr());
                            temps.push(temp.clone());
                            (temp, variable.expr())
                        }
                    },
                    (None, other) => {
                        let temp = cx.field("order");
                        let value = expr::value(cx, scope, &mut pending, other)?;
                        set.insert(temp.clone(), value.clone());
                        temps.push(temp.clone());
                        (temp, value)
                    }
                };
                // MongoDB puts NULL first ascending, last descending.
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
                        sort.insert(flag.clone(), if *nulls == Nulls::Last { 1 } else { -1 });
                        temps.push(flag);
                    }
                }
                sort.insert(key, if *direction == Direction::Asc { 1 } else { -1 });
            }
            Order::Random => {
                let temp = cx.field("random");
                set.insert(temp.clone(), doc! { "$rand": {} });
                sort.insert(temp.clone(), 1);
                temps.push(temp);
            }
            Order::Raw { .. } => {
                return Err(unsupported(
                    "raw SQL orders (`order_by_raw`): order by columns or expressions",
                ));
            }
            other => return Err(unsupported(format!("the order {other:?}"))),
        }
    }
    temps.extend(pending.flush(stages));
    if !set.is_empty() {
        stages.push(doc! { "$set": set });
    }
    if !sort.is_empty() {
        stages.push(doc! { "$sort": sort });
    }
    Ok(temps)
}

/// `$skip` and `$limit`. A limit of 0 matches nothing (MongoDB's limits
/// are positive).
pub(crate) fn page(query: &QueryIr, stages: &mut Vec<Document>) {
    const LARGEST: u64 = i64::MAX as u64;
    if let Some(offset) = query.offset.filter(|offset| *offset > 0) {
        stages.push(doc! { "$skip": i64::try_from(offset.min(LARGEST)).unwrap_or(i64::MAX) });
    }
    match query.limit {
        Some(0) => stages.push(doc! { "$match": nothing() }),
        Some(limit) => {
            stages.push(doc! { "$limit": i64::try_from(limit.min(LARGEST)).unwrap_or(i64::MAX) });
        }
        None => {}
    }
}

/// The stages as a `find`, when they are a `$match`, `$sort`, `$skip`,
/// `$limit` and `$project`, in that order, each at most once.
fn as_find(collection: &str, stages: &[Document]) -> Option<Find> {
    const ORDER: [&str; 5] = ["$match", "$sort", "$skip", "$limit", "$project"];
    let mut find = Find {
        collection: collection.to_owned(),
        filter: Document::new(),
        sort: None,
        projection: None,
        skip: None,
        limit: None,
    };
    let mut next = 0;
    for stage in stages {
        let mut entries = stage.iter();
        let (name, value) = entries.next()?;
        if entries.next().is_some() {
            return None;
        }
        let index = ORDER.iter().position(|known| known == name)?;
        if index < next {
            return None;
        }
        next = index + 1;
        match index {
            0 => find.filter = value.as_document()?.clone(),
            1 => find.sort = Some(value.as_document()?.clone()),
            2 => find.skip = u64::try_from(number(value)?).ok(),
            3 => find.limit = u64::try_from(number(value)?).ok(),
            _ => find.projection = Some(value.as_document()?.clone()),
        }
    }
    Some(find)
}

fn number(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(v) => Some(i64::from(*v)),
        Bson::Int64(v) => Some(*v),
        _ => None,
    }
}

/// The command reading `query`'s documents.
pub(crate) fn read(query: &QueryIr) -> Result<MongoCommand, OrmError> {
    let mut cx = Cx::default();
    let mut scope = Scope::of(&query.table);
    Ok(plan(&mut cx, &mut scope, query)?.command())
}

/// The command reading `query`'s documents, with the output columns.
pub(crate) fn read_with_columns(
    query: &QueryIr,
) -> Result<(MongoCommand, Option<Vec<String>>), OrmError> {
    let mut cx = Cx::default();
    let mut scope = Scope::of(&query.table);
    let plan = plan(&mut cx, &mut scope, query)?;
    let columns = plan.columns.clone();
    Ok((plan.command(), columns))
}

/// The command telling whether `query` has a document: its pipeline, one
/// document, its `_id`.
pub(crate) fn exists(query: &QueryIr) -> Result<MongoCommand, OrmError> {
    let mut query = query.clone();
    query.orders.clear();
    query.lock = None;
    if query.unions.is_empty()
        && query.groups.is_empty()
        && query.havings.is_empty()
        && matches!(query.distinct, Distinct::No)
    {
        query.columns.clear();
    }
    let mut cx = Cx::default();
    let mut scope = Scope::of(&query.table);
    let mut plan = plan(&mut cx, &mut scope, &query)?;
    plan.stages.push(doc! { "$limit": 1 });
    plan.stages.push(doc! { "$project": { "_id": 1 } });
    Ok(plan.command())
}

/// The command computing `aggregate` over `query`'s documents, in a field
/// named `aggregate`. Like SQL, a query with groups, `HAVING`, `DISTINCT`,
/// unions, a limit or an offset is aggregated over what it returns; any
/// other over the documents it matches. A plain `count(*)` is a
/// `countDocuments`.
pub(crate) fn aggregate(query: &QueryIr, aggregate: &AggregateIr) -> Result<MongoCommand, OrmError> {
    let wrap = !query.groups.is_empty()
        || !query.havings.is_empty()
        || !query.unions.is_empty()
        || !matches!(query.distinct, Distinct::No)
        || query.limit.is_some()
        || query.offset.is_some();
    let mut cx = Cx::default();
    let mut query = query.clone();
    query.lock = None;
    if wrap {
        if query.limit.is_none() && query.offset.is_none() {
            query.orders.clear();
        }
        if query.columns.is_empty() && !query.groups.is_empty() {
            query.columns = query
                .groups
                .iter()
                .cloned()
                .map(|expr| SelectItem { expr, alias: None })
                .collect();
        }
    } else {
        query.columns.clear();
        query.orders.clear();
    }
    let mut scope = Scope::of(&query.table);
    let plan = plan(&mut cx, &mut scope, &query)?;
    let mut stages = plan.stages;

    if !wrap
        && aggregate.function == AggregateFunction::Count
        && aggregate.arg.is_none()
        && stages.iter().all(|stage| stage.contains_key("$match"))
        && stages.len() <= 1
    {
        let filter = stages
            .first()
            .and_then(|stage| stage.get_document("$match").ok())
            .cloned()
            .unwrap_or_default();
        return Ok(MongoCommand::Count(Count {
            collection: plan.collection,
            filter,
        }));
    }

    // Over a wrapped query, the argument names its output columns (no
    // table); otherwise a field of the documents.
    let output_scope;
    let arg_scope = if wrap {
        output_scope = Scope::of(&TableRef::new("aggregate_table"));
        &output_scope
    } else {
        &scope
    };
    let mut pending = Pending::default();
    let argument = match &aggregate.arg {
        Some(arg) => {
            let arg = if wrap { unqualified_expr(arg) } else { arg.clone() };
            Some(expr::value(&mut cx, arg_scope, &mut pending, &arg)?)
        }
        None => None,
    };
    pending.flush(&mut stages);
    if aggregate.distinct && aggregate.function == AggregateFunction::Count {
        let argument = argument.ok_or_else(|| invalid("count(DISTINCT *) needs a column"))?;
        stages.push(doc! { "$group": { "_id": argument } });
        stages.push(doc! { "$match": { "_id": { "$ne": Bson::Null } } });
        stages.push(doc! { "$count": "aggregate" });
    } else {
        let accumulator: Accumulator =
            expr::accumulator(aggregate.function, argument, aggregate.distinct)?;
        stages.push(doc! { "$group": { "_id": Bson::Null, "aggregate": accumulator.op.clone() } });
        stages.push(doc! { "$project": { "_id": 0, "aggregate": accumulator.output("aggregate") } });
    }
    Ok(MongoCommand::Aggregate(Aggregate {
        collection: plan.collection,
        pipeline: stages,
    }))
}

/// A column expression without its table.
fn unqualified_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::Column(column) => Expr::Column(ColumnRef {
            table: None,
            ..column.clone()
        }),
        other => other.clone(),
    }
}

/// The name SQL gives an unaliased column (for groups selected as
/// columns).
pub(crate) fn default_name(expr: &Expr) -> Option<Cow<'_, str>> {
    match expr {
        Expr::Column(column) => Some(Cow::Owned(column_output(column))),
        _ => None,
    }
}
