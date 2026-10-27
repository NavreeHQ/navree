//! Inserts that handle conflicting documents: what the builders'
//! `insert_or_ignore` and `upsert`, on tables and models (navree-orm's bulk
//! writes), put in `InsertIr::on_conflict`, as MongoDB commands.
//!
//! | `on_conflict` | MongoDB |
//! |---|---|
//! | `Ignore` on any unique index | an unordered `insertMany` that skips duplicate-key errors (outside a transaction, which MongoDB aborts on any write error) |
//! | `Ignore` on named fields | upserts by those fields that change nothing: only what is missing goes in, and it works in a transaction |
//! | `Update` | one unordered `update` command of `{q: unique fields, u: {$set: updated, $setOnInsert: the rest}, upsert: true}` statements |
//!
//! ```no_run
//! use navree_mongo::MongoDb;
//! use navree_orm::{Value, table};
//!
//! # async fn demo(mongo: MongoDb) -> Result<(), navree_orm::OrmError> {
//! // A unique index on `code` keeps one rate per currency.
//! let inserted = table("rates")
//!     .insert_or_ignore(&mongo, [[("code", Value::from("EUR")), ("rate", Value::from(1.08))]])
//!     .await?;
//! let written = table("rates")
//!     .upsert(
//!         &mongo,
//!         [[("code", Value::from("EUR")), ("rate", Value::from(1.09))]],
//!         ["code"],
//!         ["rate"],
//!     )
//!     .await?;
//! # let _ = (inserted, written); Ok(()) }
//! ```

use bson::{Bson, Document};
use navree_orm::OrmError;
use navree_orm::ir::{Expr, OnConflict};

use crate::command::{Insert, Upsert};
use crate::compile::plain_field;
use crate::convert::to_bson;
use crate::error::{invalid, unsupported};

/// The command an insert with [`OnConflict`] runs.
#[derive(Debug, PartialEq)]
pub(crate) enum ConflictWrite {
    /// `insertMany` unordered, skipping the duplicates.
    InsertOrIgnore(Insert),
    /// Upserts by unique fields.
    Upsert(Upsert),
}

/// What an upsert sets on a document that exists.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Assign {
    /// The row's value of a field (`Expr::Excluded`).
    Field(String),
    /// A value.
    Value(Bson),
}

/// The command that inserts the documents of `insert` handling conflicts
/// as `on_conflict` says.
pub(crate) fn conflict_write(
    insert: Insert,
    on_conflict: &OnConflict,
) -> Result<ConflictWrite, OrmError> {
    match on_conflict {
        OnConflict::Ignore { target } if target.is_empty() => {
            Ok(ConflictWrite::InsertOrIgnore(insert))
        }
        OnConflict::Ignore { target } => {
            let unique_by = fields(target)?;
            upserts(insert, &unique_by, &[]).map(ConflictWrite::Upsert)
        }
        OnConflict::Update { target, sets } => {
            let unique_by = fields(target)?;
            let update = sets
                .iter()
                .map(|(column, expr)| Ok((plain_field(column)?, assign(expr)?)))
                .collect::<Result<Vec<_>, OrmError>>()?;
            upserts(insert, &unique_by, &update).map(ConflictWrite::Upsert)
        }
        other => Err(unsupported(format!(
            "inserts handling conflicts with {other:?}"
        ))),
    }
}

/// The field names of `columns`.
fn fields(columns: &[std::borrow::Cow<'static, str>]) -> Result<Vec<String>, OrmError> {
    columns.iter().map(|column| plain_field(column)).collect()
}

/// What an upsert's `SET column = expr` writes on MongoDB.
fn assign(expr: &Expr) -> Result<Assign, OrmError> {
    match expr {
        Expr::Excluded { column } => plain_field(column).map(Assign::Field),
        Expr::Value(value) => to_bson(value).map(Assign::Value),
        _ => Err(unsupported(
            "computed values in an upsert's update (set a field to the row's value or to a value)",
        )),
    }
}

/// Per document, the filter on its `unique_by` fields and the update:
/// `$set` of the `update` fields, `$setOnInsert` of the rest.
fn upserts(
    insert: Insert,
    unique_by: &[String],
    update: &[(String, Assign)],
) -> Result<Upsert, OrmError> {
    if unique_by.is_empty() {
        return Err(invalid(
            "an upsert finds each row's document by its unique fields: name at least one",
        ));
    }
    if let Some((both, _)) = update.iter().find(|(field, _)| unique_by.contains(field)) {
        return Err(invalid(format!(
            "`{both}` is both a unique field and a field to update: a row's document is found by its unique fields, which the upsert keeps"
        )));
    }
    let mut rows = Vec::with_capacity(insert.documents.len());
    for document in insert.documents {
        let mut filter = Document::new();
        for field in unique_by {
            let value = document.get(field).cloned().ok_or_else(|| {
                invalid(format!(
                    "a row lacks the unique field `{field}` its document is found by"
                ))
            })?;
            filter.insert(field.clone(), value);
        }
        let mut set = Document::new();
        for (field, assign) in update {
            let value = match assign {
                Assign::Field(source) => document.get(source).cloned().ok_or_else(|| {
                    invalid(format!("a row lacks `{source}`, which the upsert updates"))
                })?,
                Assign::Value(value) => value.clone(),
            };
            set.insert(field.clone(), value);
        }
        let mut on_insert = Document::new();
        for (field, value) in document {
            if unique_by.contains(&field) || set.contains_key(&field) {
                continue;
            }
            on_insert.insert(field, value);
        }
        let mut operators = Document::new();
        if !set.is_empty() {
            operators.insert("$set", set);
        }
        if !on_insert.is_empty() {
            operators.insert("$setOnInsert", on_insert);
        }
        if operators.is_empty() {
            // Only unique fields: insert them when missing, change nothing.
            operators.insert("$setOnInsert", Bson::Document(filter.clone()));
        }
        rows.push((filter, operators));
    }
    Ok(Upsert {
        collection: insert.collection,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use bson::doc;
    use navree_orm::Value;
    use navree_orm::ir::InsertIr;

    use super::*;

    fn insert() -> Insert {
        let row = |code: &str, rate: f64| {
            vec![
                Expr::Value(Value::from(code)),
                Expr::Value(Value::from(rate)),
                Expr::Value(Value::from("ecb")),
            ]
        };
        crate::compile::insert(&InsertIr::new(
            "rates",
            vec!["code".into(), "rate".into(), "source".into()],
            vec![row("EUR", 1.09), row("NOK", 0.09)],
        ))
        .unwrap()
    }

    fn update(target: &[&'static str], sets: Vec<(&'static str, Expr)>) -> OnConflict {
        OnConflict::Update {
            target: target.iter().map(|field| Cow::Borrowed(*field)).collect(),
            sets: sets
                .into_iter()
                .map(|(field, expr)| (Cow::Borrowed(field), expr))
                .collect(),
        }
    }

    fn ignore(target: &[&'static str]) -> OnConflict {
        OnConflict::Ignore {
            target: target.iter().map(|field| Cow::Borrowed(*field)).collect(),
        }
    }

    fn upsert(write: ConflictWrite) -> Upsert {
        match write {
            ConflictWrite::Upsert(upsert) => upsert,
            other => panic!("not an upsert: {other:?}"),
        }
    }

    #[test]
    fn upserts_set_the_updated_fields_and_insert_the_rest() {
        let written = upsert(
            conflict_write(
                insert(),
                &update(&["code"], vec![("rate", Expr::excluded("rate"))]),
            )
            .unwrap(),
        );
        assert_eq!(written.collection, "rates");
        assert_eq!(
            written.rows[0],
            (
                doc! { "code": "EUR" },
                doc! { "$set": { "rate": 1.09 }, "$setOnInsert": { "source": "ecb" } }
            )
        );
        let valued = upsert(
            conflict_write(
                insert(),
                &update(
                    &["code"],
                    vec![("source", Expr::Value(Value::from("manual")))],
                ),
            )
            .unwrap(),
        );
        assert_eq!(
            valued.rows[1].1,
            doc! { "$set": { "source": "manual" }, "$setOnInsert": { "rate": 0.09 } }
        );
        let only_missing = upsert(conflict_write(insert(), &ignore(&["code"])).unwrap());
        assert_eq!(
            only_missing.rows[1].1,
            doc! { "$setOnInsert": { "rate": 0.09, "source": "ecb" } }
        );
        let keys_only = upsert(
            conflict_write(
                crate::compile::insert(&InsertIr::new(
                    "tags",
                    vec!["name".into()],
                    vec![vec![Expr::Value(Value::from("vip"))]],
                ))
                .unwrap(),
                &ignore(&["name"]),
            )
            .unwrap(),
        );
        assert_eq!(
            keys_only.rows[0].1,
            doc! { "$setOnInsert": { "name": "vip" } }
        );
    }

    #[test]
    fn ignoring_any_conflict_skips_duplicates() {
        assert_eq!(
            conflict_write(insert(), &ignore(&[])).unwrap(),
            ConflictWrite::InsertOrIgnore(insert())
        );
    }

    #[test]
    fn upserts_name_what_is_wrong() {
        let wrong = |on_conflict: OnConflict| {
            conflict_write(insert(), &on_conflict)
                .unwrap_err()
                .to_string()
        };
        assert!(
            wrong(update(&[], vec![("rate", Expr::excluded("rate"))])).contains("unique fields")
        );
        assert!(wrong(update(&["code"], vec![("code", Expr::excluded("code"))])).contains("both"));
        assert!(
            wrong(update(
                &["currency"],
                vec![("rate", Expr::excluded("rate"))]
            ))
            .contains("`currency`")
        );
        assert!(
            wrong(update(
                &["code"],
                vec![("volume", Expr::excluded("volume"))]
            ))
            .contains("`volume`")
        );
        let computed = wrong(update(
            &["code"],
            vec![("rate", Expr::col("rates.rate").add(Expr::excluded("rate")))],
        ));
        assert!(computed.contains("computed values"), "{computed}");
        let path = wrong(update(&["code"], vec![("$where", Expr::excluded("rate"))]));
        assert!(path.contains("$where"), "{path}");
    }
}
