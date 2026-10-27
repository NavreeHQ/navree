//! The document-driver interface: models run unchanged on a document store.
//! A small in-memory store implements `DocumentDriver` (the way the
//! MongoDB driver does, with filters, sort, projection, skip and limit)
//! and an `Executor` hands it out.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use navree_kernel::BoxFuture;
use navree_orm::ir::{
    Aggregate, AggregateFunction, Boolean, DeleteIr, Direction, Expr, InValues, InsertIr, Operator,
    Order, QueryIr, UpdateIr, Where, WhereClause,
};
use navree_orm::relations::{BelongsTo, HasMany};
use navree_orm::{
    Access, DocumentDriver, DocumentTarget, Executor, InsertOutcome, Model, OrmError, Row, Target,
    Value, relations, same_value, to_json,
};
use serde::Serialize;

use crate::models::Company;

/// A customer kept in a document store, with support tickets.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "customers")]
struct Customer {
    id: i64,
    name: String,
}

#[relations]
impl Customer {
    fn tickets(&self) -> HasMany<Customer, SupportTicket> {
        self.has_many(SupportTicket::CUSTOMER_ID)
    }
}

/// A support ticket of a customer.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "support_tickets")]
struct SupportTicket {
    id: i64,
    customer_id: Option<i64>,
    subject: String,
}

#[relations]
impl SupportTicket {
    fn customer(&self) -> BelongsTo<SupportTicket, Customer> {
        self.belongs_to(SupportTicket::CUSTOMER_ID)
    }
}

type Document = Vec<(String, Value)>;

/// Collections of documents in memory.
#[derive(Default)]
struct Memory {
    collections: Mutex<HashMap<String, Vec<Document>>>,
    next_id: Mutex<i64>,
}

fn field<'a>(document: &'a Document, name: &str) -> Option<&'a Value> {
    document
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn column_name(expr: &Expr) -> Result<&str, OrmError> {
    match expr {
        Expr::Column(column) if column.path.is_empty() => Ok(&column.name),
        other => Err(OrmError::Driver {
            driver: "memory".into(),
            message: format!("cannot evaluate {other:?}"),
        }),
    }
}

fn value_of(expr: &Expr) -> Result<&Value, OrmError> {
    match expr {
        Expr::Value(value) => Ok(value),
        other => Err(OrmError::Driver {
            driver: "memory".into(),
            message: format!("cannot evaluate {other:?}"),
        }),
    }
}

/// Orders two values by their JSON form (numbers numerically, text
/// lexically).
fn order(a: &Value, b: &Value) -> Ordering {
    match (to_json(a), to_json(b)) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (x, y) => x.to_string().cmp(&y.to_string()),
    }
}

fn matches(document: &Document, clauses: &[WhereClause]) -> Result<bool, OrmError> {
    // SQL precedence: AND binds tighter than OR.
    let mut any = false;
    let mut all = true;
    for (i, clause) in clauses.iter().enumerate() {
        if i > 0 && clause.boolean == Boolean::Or {
            any |= all;
            all = true;
        }
        let mut hit = matches_one(document, &clause.filter)?;
        if clause.negated {
            hit = !hit;
        }
        all &= hit;
    }
    Ok(any || all)
}

fn matches_one(document: &Document, filter: &Where) -> Result<bool, OrmError> {
    Ok(match filter {
        Where::Compare { left, op, right } => {
            let actual = field(document, column_name(left)?)
                .cloned()
                .unwrap_or(Value::String(None));
            let wanted = value_of(right)?;
            match op {
                Operator::Eq => same_value(&actual, wanted),
                Operator::Ne => !same_value(&actual, wanted),
                Operator::Gt => order(&actual, wanted) == Ordering::Greater,
                Operator::Gte => order(&actual, wanted) != Ordering::Less,
                Operator::Lt => order(&actual, wanted) == Ordering::Less,
                Operator::Lte => order(&actual, wanted) != Ordering::Greater,
                other => {
                    return Err(OrmError::Unsupported {
                        feature: format!("{other:?}").into(),
                        driver: "memory".into(),
                    });
                }
            }
        }
        Where::Null { expr, not } => {
            let null = field(document, column_name(expr)?).is_none_or(navree_orm::is_null);
            null != *not
        }
        Where::In {
            expr,
            values: InValues::List(list),
            not,
        } => {
            let actual = field(document, column_name(expr)?)
                .cloned()
                .unwrap_or(Value::String(None));
            let found = list
                .iter()
                .map(value_of)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|value| same_value(&actual, value));
            found != *not
        }
        Where::Group(clauses) => matches(document, clauses)?,
        other => {
            return Err(OrmError::Unsupported {
                feature: format!("{other:?}").into(),
                driver: "memory".into(),
            });
        }
    })
}

impl Memory {
    fn matching(
        &self,
        collection: &str,
        wheres: &[WhereClause],
    ) -> Result<Vec<Document>, OrmError> {
        let collections = self.collections.lock().unwrap();
        let mut found = Vec::new();
        for document in collections.get(collection).into_iter().flatten() {
            if matches(document, wheres)? {
                found.push(document.clone());
            }
        }
        Ok(found)
    }
}

impl DocumentDriver for Memory {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn find<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<Vec<Row>, OrmError>> {
        Box::pin(async move {
            let mut documents = self.matching(&query.table.name, &query.wheres)?;
            for item in query.orders.iter().rev() {
                if let Order::By {
                    expr, direction, ..
                } = item
                {
                    let name = column_name(expr)?.to_owned();
                    documents.sort_by(|a, b| {
                        let ordering = match (field(a, &name), field(b, &name)) {
                            (Some(x), Some(y)) => order(x, y),
                            _ => Ordering::Equal,
                        };
                        if *direction == Direction::Desc {
                            ordering.reverse()
                        } else {
                            ordering
                        }
                    });
                }
            }
            let skip = usize::try_from(query.offset.unwrap_or(0)).unwrap_or(usize::MAX);
            let take = query.limit.map_or(usize::MAX, |limit| {
                usize::try_from(limit).unwrap_or(usize::MAX)
            });
            // (field read, name returned): a projection honours aliases.
            let projected: Vec<(String, String)> = query
                .columns
                .iter()
                .map(|item| {
                    let source = column_name(&item.expr)?.to_owned();
                    let output = item
                        .alias
                        .as_deref()
                        .map_or_else(|| source.clone(), str::to_owned);
                    Ok((source, output))
                })
                .collect::<Result<_, OrmError>>()?;
            Ok(documents
                .into_iter()
                .skip(skip)
                .take(take)
                .map(|document| {
                    let document = if projected.is_empty() {
                        document
                    } else {
                        projected
                            .iter()
                            .map(|(source, output)| {
                                (
                                    output.clone(),
                                    field(&document, source)
                                        .cloned()
                                        .unwrap_or(Value::String(None)),
                                )
                            })
                            .collect()
                    };
                    Row::from_values(document)
                })
                .collect())
        })
    }

    fn aggregate<'a>(
        &'a self,
        query: &'a QueryIr,
        aggregate: &'a Aggregate,
    ) -> BoxFuture<'a, Result<Value, OrmError>> {
        Box::pin(async move {
            match aggregate.function {
                AggregateFunction::Count => {
                    let count = self.matching(&query.table.name, &query.wheres)?.len();
                    Ok(Value::BigInt(Some(
                        i64::try_from(count).unwrap_or(i64::MAX),
                    )))
                }
                other => Err(OrmError::Unsupported {
                    feature: format!("{other:?}").into(),
                    driver: "memory".into(),
                }),
            }
        })
    }

    fn insert<'a>(
        &'a self,
        insert: &'a InsertIr,
    ) -> BoxFuture<'a, Result<InsertOutcome, OrmError>> {
        Box::pin(async move {
            let mut returned = Vec::new();
            let mut collections = self.collections.lock().unwrap();
            let collection = collections.entry(insert.table.to_string()).or_default();
            for row in &insert.rows {
                let mut document: Document = insert
                    .columns
                    .iter()
                    .zip(row)
                    .map(|(column, value)| Ok((column.to_string(), value_of(value)?.clone())))
                    .collect::<Result<_, OrmError>>()?;
                // The store assigns the keys it is asked to return.
                for column in &insert.returning {
                    let mut next = self.next_id.lock().unwrap();
                    *next += 1;
                    document.push((column.to_string(), Value::BigInt(Some(*next))));
                    returned.push(Row::from_values(vec![(
                        column.to_string(),
                        Value::BigInt(Some(*next)),
                    )]));
                }
                collection.push(document);
            }
            Ok(InsertOutcome {
                rows_affected: insert.rows.len() as u64,
                returned,
                last_insert_id: None,
            })
        })
    }

    fn update<'a>(&'a self, update: &'a UpdateIr) -> BoxFuture<'a, Result<u64, OrmError>> {
        Box::pin(async move {
            let mut collections = self.collections.lock().unwrap();
            let mut changed = 0;
            for document in collections
                .entry(update.table.name.to_string())
                .or_default()
            {
                if matches(document, &update.wheres)? {
                    for (column, value) in &update.sets {
                        let value = value_of(value)?.clone();
                        match document.iter_mut().find(|(name, _)| name == column) {
                            Some(slot) => slot.1 = value,
                            None => document.push((column.to_string(), value)),
                        }
                    }
                    changed += 1;
                }
            }
            Ok(changed)
        })
    }

    fn delete<'a>(&'a self, delete: &'a DeleteIr) -> BoxFuture<'a, Result<u64, OrmError>> {
        Box::pin(async move {
            let mut collections = self.collections.lock().unwrap();
            let collection = collections
                .entry(delete.table.name.to_string())
                .or_default();
            let before = collection.len();
            let mut kept = Vec::with_capacity(before);
            for document in collection.drain(..) {
                if !matches(&document, &delete.wheres)? {
                    kept.push(document);
                }
            }
            *collection = kept;
            Ok((before - collection.len()) as u64)
        })
    }
}

/// The executor of the in-memory store.
struct MemoryStore(Arc<Memory>);

impl Executor for MemoryStore {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(Target::Document(DocumentTarget::new(self.0.clone(), None)))
    }
}

#[tokio::test]
async fn models_run_on_a_document_store() {
    let store = MemoryStore(Arc::new(Memory::default()));
    let mut acme = Company::create(
        &store,
        Company {
            name: "Acme".into(),
            employees: 5,
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(acme.id, 1, "the store assigned the key");
    for (name, employees) in [("Globex", 50), ("Initech", 15)] {
        Company::create(
            &store,
            Company {
                name: name.into(),
                employees,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }

    let big = Company::query()
        .where_(Company::EMPLOYEES.gte(15))
        .order_by_desc(Company::EMPLOYEES)
        .get(&store)
        .await
        .unwrap();
    assert_eq!(big.map(|c| c.name.clone()), ["Globex", "Initech"]);
    assert_eq!(
        Company::find_or_fail(&store, acme.id).await.unwrap().name,
        "Acme"
    );
    assert_eq!(Company::query().count(&store).await.unwrap(), 3);
    let page = Company::query()
        .order_by(Company::NAME)
        .paginate(&store, (2, 2))
        .await
        .unwrap();
    assert_eq!(
        (page.meta.total, page.data[0].name.as_str()),
        (3, "Initech")
    );

    acme.employees = 6;
    acme.save(&store).await.unwrap();
    assert_eq!(
        Company::find_or_fail(&store, acme.id)
            .await
            .unwrap()
            .employees,
        6
    );
    assert!(acme.delete(&store).await.unwrap());
    assert_eq!(Company::query().count(&store).await.unwrap(), 2);
    let names: Vec<String> = Company::query()
        .order_by(Company::NAME)
        .pluck(&store, Company::NAME)
        .await
        .unwrap();
    assert_eq!(names, ["Globex", "Initech"]);

    // What the store cannot express is refused, not guessed.
    let error = Company::query()
        .where_raw("1 = 1", Vec::<Value>::new())
        .get(&store)
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Unsupported { .. }), "{error:?}");
    assert!(Company::meta().name() == "Company");
}

#[tokio::test]
async fn relations_eager_load_on_a_document_store() {
    let store = MemoryStore(Arc::new(Memory::default()));
    let mut acme = Customer::create(
        &store,
        Customer {
            name: "Acme".into(),
            ..Customer::default()
        },
    )
    .await
    .unwrap();
    let globex = Customer::create(
        &store,
        Customer {
            name: "Globex".into(),
            ..Customer::default()
        },
    )
    .await
    .unwrap();
    for (customer, subject) in [
        (Some(acme.id), "Printer"),
        (Some(acme.id), "Login"),
        (Some(globex.id), "Billing"),
        (None, "Spam"),
    ] {
        SupportTicket::create(
            &store,
            SupportTicket {
                customer_id: customer,
                subject: subject.into(),
                ..SupportTicket::default()
            },
        )
        .await
        .unwrap();
    }

    // Eager loads are batched `IN` queries the store answers.
    let customers = Customer::query()
        .with("tickets")
        .order_by(Customer::ID)
        .get(&store)
        .await
        .unwrap();
    let counts: Vec<usize> = customers
        .iter()
        .map(|customer| customer.loaded(Customer::tickets).unwrap().len())
        .collect();
    assert_eq!(counts, [2, 1]);
    let tickets = SupportTicket::query()
        .with("customer")
        .order_by(SupportTicket::ID)
        .get(&store)
        .await
        .unwrap();
    let owners: Vec<Option<String>> = tickets
        .iter()
        .map(|ticket| {
            ticket
                .loaded(SupportTicket::customer)
                .unwrap()
                .as_ref()
                .map(|customer| customer.name.clone())
        })
        .collect();
    assert_eq!(
        owners,
        [
            Some("Acme".to_owned()),
            Some("Acme".to_owned()),
            Some("Globex".to_owned()),
            None
        ]
    );

    // Loading afterwards and querying through a relation too.
    acme.load(&store, "tickets").await.unwrap();
    assert_eq!(acme.loaded(Customer::tickets).unwrap().len(), 2);
    assert_eq!(acme.tickets().count(&store).await.unwrap(), 2);
    let login = acme
        .tickets()
        .where_(SupportTicket::SUBJECT.eq("Login"))
        .first(&store)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(login.customer_id, Some(acme.id));
}

/// Cursors and lazy streams run on a document store: the driver's default
/// `stream` hands out what `find` returns; raw SQL is refused.
#[tokio::test]
async fn documents_stream_through_cursors_and_pages() {
    use futures::TryStreamExt;

    let store = MemoryStore(Arc::new(Memory::default()));
    for (name, employees) in [("Acme", 5), ("Globex", 50), ("Initech", 15)] {
        Company::create(
            &store,
            Company {
                name: name.into(),
                employees,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    let streamed: Vec<String> = Company::query()
        .where_(Company::EMPLOYEES.gte(10))
        .order_by(Company::NAME)
        .cursor(&store)
        .map_ok(|company| company.name.clone())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(streamed, ["Globex", "Initech"]);
    let paged: Vec<i64> = Company::query()
        .lazy_by_id(&store, 2)
        .map_ok(|company| company.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(paged, [1, 2, 3]);

    let error = navree_orm::cursor(&store, navree_orm::raw("SELECT 1", Vec::<Value>::new()))
        .try_collect::<Vec<_>>()
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "raw SQL is not supported on memory");
    let error = navree_orm::select(&store, navree_orm::raw("SELECT 1", Vec::<Value>::new()))
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Unsupported { .. }), "{error:?}");
}
