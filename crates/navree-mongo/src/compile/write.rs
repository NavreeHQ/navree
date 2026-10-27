//! Writes ([`InsertIr`], [`UpdateIr`], [`DeleteIr`]) as `insertMany`,
//! `updateMany` and `deleteMany`.

use std::str::FromStr;

use bson::{Bson, Decimal128, Document, doc};
use navree_orm::ir::{ArithmeticOp, ColumnRef, Expr, InsertIr, TableRef, UpdateIr, WhereClause};
use navree_orm::{Decimal, OrmError};

use super::Pending;
use super::expr::{self, literal};
use super::filter;
use super::scope::{Cx, Scope, field_path};
use crate::command::{Aggregate, Insert, UpdateDocument};
use crate::convert::to_bson;
use crate::error::{invalid, unsupported};

/// The documents a write applies to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    /// Those a filter matches.
    Filter(Document),
    /// Those whose `_id` a pipeline finds first: conditions with
    /// subqueries (a write limited to the first rows, `IN (subquery)`),
    /// which a write's filter cannot express.
    Keys(Aggregate),
}

/// The documents `wheres` match in `table`.
pub(crate) fn target(table: &TableRef, wheres: &[WhereClause]) -> Result<Target, OrmError> {
    if table.subquery.is_some() {
        return Err(unsupported("writing to a derived table (a subquery)"));
    }
    let mut cx = Cx::default();
    let scope = Scope::of(table);
    let mut pending = Pending::default();
    let filter = filter::clauses(&mut cx, &scope, &mut pending, wheres)?;
    if pending.stages.is_empty() {
        return Ok(Target::Filter(filter));
    }
    let mut pipeline = pending.stages;
    if !filter.is_empty() {
        pipeline.push(doc! { "$match": filter });
    }
    pipeline.push(doc! { "$project": { "_id": 1 } });
    Ok(Target::Keys(Aggregate {
        collection: table.name.to_string(),
        pipeline,
    }))
}

/// The documents of an insert.
pub(crate) fn insert(insert: &InsertIr) -> Result<Insert, OrmError> {
    let mut documents = Vec::with_capacity(insert.rows.len());
    let fields = insert
        .columns
        .iter()
        .map(|column| plain_field(column))
        .collect::<Result<Vec<_>, _>>()?;
    for row in &insert.rows {
        if row.len() != fields.len() {
            return Err(invalid(format!(
                "an inserted row has {} values for {} columns",
                row.len(),
                fields.len()
            )));
        }
        let mut document = Document::new();
        for (field, value) in fields.iter().zip(row) {
            let Expr::Value(value) = value else {
                return Err(unsupported(
                    "computed values in inserts (insert values; compute them in Rust first)",
                ));
            };
            document.insert(field.clone(), to_bson(value)?);
        }
        documents.push(document);
    }
    Ok(Insert {
        collection: insert.table.to_string(),
        documents,
    })
}

/// A column written by name: a field, no table, no path.
pub(crate) fn plain_field(column: &str) -> Result<String, OrmError> {
    let parsed = ColumnRef::parse(column.to_owned());
    if parsed.table.is_some() || !parsed.path.is_empty() {
        return Err(invalid(format!(
            "`{column}` is not a field name an insert can write"
        )));
    }
    field_path(&parsed)
}

/// The update half of an update: `$set` and `$inc` for values and
/// increments, an update pipeline when a value is computed from the
/// document.
pub(crate) fn update_document(update: &UpdateIr) -> Result<UpdateDocument, OrmError> {
    if update.sets.is_empty() {
        return Err(invalid("an update sets at least one column"));
    }
    let mut set = Document::new();
    let mut inc = Document::new();
    let mut computed = false;
    for (column, value) in &update.sets {
        let path = set_path(column)?;
        match value {
            Expr::Value(value) => {
                set.insert(path, to_bson(value)?);
            }
            Expr::Arithmetic { left, op, right } => match (left.as_ref(), op, right.as_ref()) {
                // `column = column ± amount` is `$inc`.
                (Expr::Column(own), ArithmeticOp::Add | ArithmeticOp::Sub, Expr::Value(amount))
                    if field_path(own).ok().as_deref() == Some(path.as_str()) =>
                {
                    let amount = to_bson(amount)?;
                    let amount = if matches!(op, ArithmeticOp::Sub) {
                        negate(amount)?
                    } else {
                        amount
                    };
                    inc.insert(path, amount);
                }
                _ => computed = true,
            },
            _ => computed = true,
        }
    }
    if computed {
        let mut cx = Cx::default();
        let scope = Scope::of(&update.table);
        let mut pending = Pending::default();
        let mut stage = Document::new();
        for (column, value) in &update.sets {
            let value = match value {
                Expr::Value(value) => literal(to_bson(value)?),
                other => expr::value(&mut cx, &scope, &mut pending, other)?,
            };
            stage.insert(set_path(column)?, value);
        }
        if !pending.stages.is_empty() {
            return Err(unsupported(
                "subqueries in updated values (compute the value first)",
            ));
        }
        return Ok(UpdateDocument::Pipeline(vec![doc! { "$set": stage }]));
    }
    let mut operators = Document::new();
    if !set.is_empty() {
        operators.insert("$set", set);
    }
    if !inc.is_empty() {
        operators.insert("$inc", inc);
    }
    Ok(UpdateDocument::Operators(operators))
}

/// The field path a `SET` writes: a column, or a JSON path into one
/// (`meta->status` is `meta.status`).
fn set_path(column: &str) -> Result<String, OrmError> {
    let parsed = ColumnRef::parse(column.to_owned());
    if parsed.table.is_some() {
        return Err(invalid(format!(
            "`{column}`: an update writes the collection's own fields"
        )));
    }
    field_path(&parsed)
}

/// `-amount`, for a decrement.
fn negate(amount: Bson) -> Result<Bson, OrmError> {
    Ok(match amount {
        Bson::Int32(v) => v
            .checked_neg()
            .map(Bson::Int32)
            .unwrap_or(Bson::Int64(-i64::from(v))),
        Bson::Int64(v) => Bson::Int64(
            v.checked_neg()
                .ok_or_else(|| invalid(format!("cannot decrement by {v}")))?,
        ),
        Bson::Double(v) => Bson::Double(-v),
        Bson::Decimal128(v) => {
            let decimal = Decimal::from_str(&v.to_string())
                .map_err(|_| invalid(format!("cannot decrement by {v}")))?;
            Bson::Decimal128(
                Decimal128::from_str(&(-decimal).to_string())
                    .map_err(|_| invalid(format!("cannot decrement by {v}")))?,
            )
        }
        other => {
            return Err(invalid(format!(
                "cannot decrement by {other}: increments take numbers"
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use navree_orm::Value;
    use navree_orm::ir::{InValues, Operator, QueryIr, SelectItem, Where};

    use super::*;

    fn update(sets: Vec<(&'static str, Expr)>) -> UpdateIr {
        UpdateIr {
            table: TableRef::new("deals"),
            sets: sets
                .into_iter()
                .map(|(column, value)| (Cow::Borrowed(column), value))
                .collect(),
            wheres: Vec::new(),
        }
    }

    #[test]
    fn updates_set_and_increment() {
        let document = update_document(&update(vec![
            ("title", Expr::Value(Value::from("Renewal"))),
            (
                "amount",
                Expr::Arithmetic {
                    left: Box::new(Expr::Column(ColumnRef::parse("amount"))),
                    op: ArithmeticOp::Sub,
                    right: Box::new(Expr::Value(Value::from(5_i64))),
                },
            ),
            ("closed_at", Expr::Value(Value::String(None))),
            ("meta->status", Expr::Value(Value::from("won"))),
        ]))
        .unwrap();
        assert_eq!(
            document,
            UpdateDocument::Operators(doc! {
                "$set": { "title": "Renewal", "closed_at": Bson::Null, "meta.status": "won" },
                "$inc": { "amount": -5_i64 },
            })
        );
    }

    #[test]
    fn computed_values_use_an_update_pipeline() {
        let document = update_document(&update(vec![
            (
                "total",
                Expr::Arithmetic {
                    left: Box::new(Expr::Column(ColumnRef::parse("price"))),
                    op: ArithmeticOp::Mul,
                    right: Box::new(Expr::Column(ColumnRef::parse("quantity"))),
                },
            ),
            ("note", Expr::Value(Value::from("$not a path"))),
        ]))
        .unwrap();
        assert_eq!(
            document,
            UpdateDocument::Pipeline(vec![doc! {
                "$set": {
                    "total": { "$multiply": ["$price", "$quantity"] },
                    "note": { "$literal": "$not a path" },
                }
            }])
        );
        assert!(update_document(&update(Vec::new())).is_err());
    }

    #[test]
    fn inserts_write_documents_in_column_order() {
        let insert = insert(&InsertIr {
            table: Cow::Borrowed("deals"),
            columns: vec![Cow::Borrowed("_id"), Cow::Borrowed("title")],
            rows: vec![
                vec![Expr::Value(Value::from(1_i64)), Expr::Value(Value::from("A"))],
                vec![Expr::Value(Value::from(2_i64)), Expr::Value(Value::from("B"))],
            ],
            returning: Vec::new(),
        })
        .unwrap();
        assert_eq!(
            insert.documents,
            [doc! { "_id": 1_i64, "title": "A" }, doc! { "_id": 2_i64, "title": "B" }]
        );
    }

    #[test]
    fn writes_by_subquery_find_the_keys_first() {
        let mut keys = QueryIr::new("deals");
        keys.columns = vec![SelectItem {
            expr: Expr::Column(ColumnRef::parse("deals._id")),
            alias: None,
        }];
        keys.limit = Some(2);
        let wheres = vec![WhereClause::and(Where::In {
            expr: Expr::Column(ColumnRef::parse("deals._id")),
            values: InValues::Subquery(Box::new(keys)),
            not: false,
        })];
        let Target::Keys(pipeline) = target(&TableRef::new("deals"), &wheres).unwrap() else {
            panic!("a key lookup");
        };
        assert_eq!(pipeline.collection, "deals");
        assert!(pipeline.pipeline[0].contains_key("$lookup"));
        assert_eq!(pipeline.pipeline.last(), Some(&doc! { "$project": { "_id": 1 } }));

        let plain = vec![WhereClause::and(Where::Compare {
            left: Expr::Column(ColumnRef::parse("stage")),
            op: Operator::Eq,
            right: Expr::Value(Value::from("lead")),
        })];
        assert_eq!(
            target(&TableRef::new("deals"), &plain).unwrap(),
            Target::Filter(doc! { "stage": "lead" })
        );
    }
}
