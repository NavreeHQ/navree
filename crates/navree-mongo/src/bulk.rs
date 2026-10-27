//! Bulk writes on MongoDB ([`MongoBulkWrites`]): inserting rows while
//! skipping the duplicates a unique index refuses, and upserting rows by
//! unique fields, each one command however many rows there are.

use std::borrow::Cow;
use std::future::Future;

use bson::{Bson, Document};
use navree_orm::ir::{Distinct, QueryIr};
use navree_orm::{OrmError, TableBuilder, Value};

use crate::command::{Insert, Upsert};
use crate::connection::MongoExecutor;
use crate::convert::to_bson;
use crate::error::invalid;

/// Bulk writes on a collection: what SQL writes as `INSERT … ON CONFLICT DO
/// NOTHING` and `INSERT … ON CONFLICT (..) DO UPDATE`.
///
/// Rows are lists of `(field, value)` pairs, as for
/// [`TableBuilder::insert`]; values convert as model fields do (dates,
/// decimals, ObjectIds, JSON as embedded documents). The builder names the
/// collection and nothing else.
///
/// ```no_run
/// use navree_mongo::{MongoBulkWrites, MongoDb};
/// use navree_orm::{Value, table};
///
/// # async fn demo(mongo: MongoDb) -> Result<(), navree_orm::OrmError> {
/// // A unique index on `code` keeps one rate per currency.
/// let inserted = table("rates")
///     .insert_or_ignore(&mongo, [[("code", Value::from("EUR")), ("rate", Value::from(1.08))]])
///     .await?;
/// let written = table("rates")
///     .upsert(
///         &mongo,
///         [[("code", Value::from("EUR")), ("rate", Value::from(1.09))]],
///         &["code"],
///         &["rate"],
///     )
///     .await?;
/// # let _ = (inserted, written); Ok(()) }
/// ```
pub trait MongoBulkWrites: Sized {
    /// Inserts `rows` unordered (`insertMany` with `ordered: false`),
    /// skipping the ones a unique index refuses as duplicates; returns how
    /// many were inserted. Any other refusal is an error. Inside a
    /// transaction MongoDB aborts on any write error, a duplicate included,
    /// so there a duplicate is an error too: upsert by the unique fields
    /// with nothing to update instead.
    fn insert_or_ignore<E, R, C, V>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
    ) -> impl Future<Output = Result<u64, OrmError>> + Send
    where
        E: MongoExecutor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>;

    /// Upserts `rows` in one unordered command: the document whose
    /// `unique_by` fields equal a row's gets the row's `update` fields;
    /// when none does, the row is inserted whole. With no `update` fields,
    /// existing documents stay as they are (an insert-or-ignore that works
    /// in transactions). Returns how many documents were matched or
    /// inserted. Keep a unique index on `unique_by`: without one, two
    /// concurrent upserts of a new row can both insert it.
    fn upsert<E, R, C, V>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
        unique_by: &[&str],
        update: &[&str],
    ) -> impl Future<Output = Result<u64, OrmError>> + Send
    where
        E: MongoExecutor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>;
}

impl MongoBulkWrites for TableBuilder {
    fn insert_or_ignore<E, R, C, V>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
    ) -> impl Future<Output = Result<u64, OrmError>> + Send
    where
        E: MongoExecutor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        let insert = collection_of(self.into_ir(), "insert_or_ignore").and_then(|collection| {
            Ok(Insert {
                collection,
                documents: documents(rows)?,
            })
        });
        async move {
            let insert = insert?;
            exec.mongo_driver()?.insert_or_ignore(&insert).await
        }
    }

    fn upsert<E, R, C, V>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
        unique_by: &[&str],
        update: &[&str],
    ) -> impl Future<Output = Result<u64, OrmError>> + Send
    where
        E: MongoExecutor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        let upsert = collection_of(self.into_ir(), "upsert")
            .and_then(|collection| upserts(collection, documents(rows)?, unique_by, update));
        async move {
            let upsert = upsert?;
            exec.mongo_driver()?.upsert(&upsert).await
        }
    }
}

/// The collection a bulk write goes to: the builder must name a plain
/// collection, with nothing else.
fn collection_of(query: QueryIr, what: &str) -> Result<String, OrmError> {
    query.check()?;
    if query.table.subquery.is_some()
        || query.table.alias.is_some()
        || !query.wheres.is_empty()
        || !query.joins.is_empty()
        || !query.groups.is_empty()
        || !query.havings.is_empty()
        || !query.unions.is_empty()
        || !query.orders.is_empty()
        || !query.columns.is_empty()
        || query.limit.is_some()
        || query.offset.is_some()
        || !matches!(query.distinct, Distinct::No)
    {
        return Err(invalid(format!(
            "{what} writes to a collection: call it on `table(\"name\")` alone, without conditions, columns or limits"
        )));
    }
    Ok(query.table.name.to_string())
}

/// The rows as documents, fields in each row's order.
fn documents<R, C, V>(rows: impl IntoIterator<Item = R>) -> Result<Vec<Document>, OrmError>
where
    R: IntoIterator<Item = (C, V)>,
    C: Into<Cow<'static, str>>,
    V: Into<Value>,
{
    rows.into_iter()
        .map(|row| {
            let mut document = Document::new();
            for (field, value) in row {
                let field = crate::compile::plain_field(&field.into())?;
                if document.contains_key(&field) {
                    return Err(invalid(format!("a row sets `{field}` twice")));
                }
                document.insert(field, to_bson(&value.into())?);
            }
            Ok(document)
        })
        .collect()
}

/// Per row, the filter on its `unique_by` fields and the update: `$set` of
/// its `update` fields, `$setOnInsert` of the rest.
fn upserts(
    collection: String,
    documents: Vec<Document>,
    unique_by: &[&str],
    update: &[&str],
) -> Result<Upsert, OrmError> {
    if unique_by.is_empty() {
        return Err(invalid(
            "an upsert finds each row's document by its unique fields: name at least one",
        ));
    }
    if let Some(both) = update.iter().find(|field| unique_by.contains(field)) {
        return Err(invalid(format!(
            "`{both}` is both a unique field and a field to update: a row's document is found by its unique fields, which the upsert keeps"
        )));
    }
    let mut rows = Vec::with_capacity(documents.len());
    for document in documents {
        let mut filter = Document::new();
        for field in unique_by {
            let value = document.get(*field).cloned().ok_or_else(|| {
                invalid(format!(
                    "a row lacks the unique field `{field}` its document is found by"
                ))
            })?;
            filter.insert(*field, value);
        }
        let mut set = Document::new();
        let mut on_insert = Document::new();
        for (field, value) in document {
            if unique_by.contains(&field.as_str()) {
                continue;
            }
            if update.contains(&field.as_str()) {
                set.insert(field, value);
            } else {
                on_insert.insert(field, value);
            }
        }
        if let Some(missing) = update.iter().find(|field| !set.contains_key(**field)) {
            return Err(invalid(format!(
                "a row lacks `{missing}`, which the upsert updates"
            )));
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
    Ok(Upsert { collection, rows })
}

#[cfg(test)]
mod tests {
    use bson::doc;
    use navree_orm::table;

    use super::*;

    fn rows() -> Vec<Vec<(&'static str, Value)>> {
        vec![
            vec![
                ("code", Value::from("EUR")),
                ("rate", Value::from(1.09)),
                ("source", Value::from("ecb")),
            ],
            vec![
                ("code", Value::from("NOK")),
                ("rate", Value::from(0.09)),
                ("source", Value::from("ecb")),
            ],
        ]
    }

    #[test]
    fn upserts_set_the_updated_fields_and_insert_the_rest() {
        let upsert = upserts(
            "rates".into(),
            documents(rows()).unwrap(),
            &["code"],
            &["rate"],
        )
        .unwrap();
        assert_eq!(
            upsert.rows[0],
            (
                doc! { "code": "EUR" },
                doc! { "$set": { "rate": 1.09 }, "$setOnInsert": { "source": "ecb" } }
            )
        );
        let only_missing =
            upserts("rates".into(), documents(rows()).unwrap(), &["code"], &[]).unwrap();
        assert_eq!(
            only_missing.rows[1].1,
            doc! { "$setOnInsert": { "rate": 0.09, "source": "ecb" } }
        );
        let keys_only = upserts(
            "tags".into(),
            documents([[("name", Value::from("vip"))]]).unwrap(),
            &["name"],
            &[],
        )
        .unwrap();
        assert_eq!(
            keys_only.rows[0].1,
            doc! { "$setOnInsert": { "name": "vip" } }
        );
    }

    #[test]
    fn upserts_name_what_is_wrong() {
        let wrong = |unique_by: &[&str], update: &[&str]| {
            upserts(
                "rates".into(),
                documents(rows()).unwrap(),
                unique_by,
                update,
            )
            .unwrap_err()
            .to_string()
        };
        assert!(wrong(&[], &["rate"]).contains("unique fields"));
        assert!(wrong(&["code"], &["code"]).contains("both"));
        assert!(wrong(&["currency"], &["rate"]).contains("`currency`"));
        assert!(wrong(&["code"], &["volume"]).contains("`volume`"));
        let twice = documents([[("a", Value::from(1)), ("a", Value::from(2))]]).unwrap_err();
        assert!(twice.to_string().contains("twice"), "{twice}");
        let bad = documents([[("$where", Value::from(1))]]).unwrap_err();
        assert!(bad.to_string().contains("$where"), "{bad}");
    }

    #[test]
    fn bulk_writes_go_to_a_plain_collection() {
        assert_eq!(
            collection_of(table("rates").into_ir(), "upsert").unwrap(),
            "rates"
        );
        let error = collection_of(
            table("rates")
                .where_(navree_orm::col("code").eq("EUR"))
                .into_ir(),
            "upsert",
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("`table(\"name\")` alone"),
            "{error}"
        );
    }
}
