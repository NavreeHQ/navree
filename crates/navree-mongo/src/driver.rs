//! [`MongoDriver`]: the ORM's [`DocumentDriver`] on MongoDB.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bson::{Bson, Document, doc};
use futures_util::stream::{self, BoxStream};
use futures_util::{StreamExt, TryStreamExt};
use mongodb::options::ReturnDocument;
use mongodb::{ClientSession, Database};
use navree_kernel::BoxFuture;
use navree_orm::ir::{
    Aggregate as AggregateIr, DeleteIr, Expr, InValues, InsertIr, QueryIr, UpdateIr, Where,
    WhereClause,
};
use navree_orm::{DocumentDriver, InsertOutcome, OrmError, Row, Value};
use navree_telemetry::{EventKind, QueryExecuted, Telemetry, TelemetryEvent};
use tokio::sync::Mutex;

use crate::command::{
    Aggregate, Count, Delete, Find, Insert, MongoCommand, Update, UpdateDocument, Upsert,
};
use crate::compile::{self, Target};
use crate::convert::{document_row, from_bson};
use crate::error::{DRIVER, MongoError, invalid, orm_error};

/// What the drivers of one connection share: where integer keys are
/// counted, and where commands are reported.
pub(crate) struct Shared {
    /// The collection of key counters.
    pub(crate) sequences: String,
    /// Where commands are reported (`QueryExecuted`).
    pub(crate) telemetry: Telemetry,
    /// The connection in reports: the URL without its password, and the
    /// database.
    pub(crate) connection: String,
    /// Commands slower than this are logged as warnings.
    pub(crate) slow: Option<Duration>,
    /// Whether every command's shape is logged at debug level.
    pub(crate) log_queries: bool,
}

/// A session the driver runs in, and what it learned about it.
pub(crate) struct SessionState {
    /// The session.
    pub(crate) session: Mutex<ClientSession>,
    /// Whether a transaction is open in it.
    pub(crate) transaction: AtomicBool,
    /// Whether an operation failed with a transient transaction error
    /// (a write conflict): the transaction can be retried.
    pub(crate) transient: AtomicBool,
}

impl SessionState {
    pub(crate) fn new(session: ClientSession) -> Arc<SessionState> {
        Arc::new(SessionState {
            session: Mutex::new(session),
            transaction: AtomicBool::new(false),
            transient: AtomicBool::new(false),
        })
    }
}

/// The ORM's [`DocumentDriver`] on MongoDB: it compiles the queries and
/// writes models make (see [`compile`]) and runs them on a database, in a
/// session when it has one. Executors hand it out: [`MongoDb`](crate::MongoDb),
/// [`MongoSession`](crate::MongoSession) and [`MongoUow`](crate::MongoUow).
///
/// - Rows are documents; a selected field a document lacks reads as NULL.
/// - Integer keys the database assigns (`KeyKind::Increments`) are counted
///   in the `navree_sequences` collection, one counter per collection and
///   field, incremented atomically (in the transaction, when there is one).
///   ObjectId, UUID and ULID keys are made before the insert.
/// - A duplicate key is a unique violation (`OrmError::is_unique_violation`,
///   409 over HTTP).
/// - Date-times are kept to the millisecond ([`time_precision`] 3), so the
///   ORM's timestamps are truncated to milliseconds.
/// - Every command is reported to telemetry as a `QueryExecuted` event
///   carrying the command's shape, never its values.
///
/// [`time_precision`]: DocumentDriver::time_precision
pub struct MongoDriver {
    database: Database,
    session: Option<Arc<SessionState>>,
    shared: Arc<Shared>,
}

impl MongoDriver {
    pub(crate) fn new(
        database: Database,
        session: Option<Arc<SessionState>>,
        shared: Arc<Shared>,
    ) -> MongoDriver {
        MongoDriver {
            database,
            session,
            shared,
        }
    }

    /// The database.
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Whether the driver runs in a session.
    pub fn in_session(&self) -> bool {
        self.session.is_some()
    }

    /// Notes a failure of an operation in the session: a transient
    /// transaction error means the whole transaction can be retried.
    fn note(&self, error: &mongodb::error::Error) {
        if let Some(state) = &self.session
            && error.contains_label(mongodb::error::TRANSIENT_TRANSACTION_ERROR)
        {
            state.transient.store(true, Ordering::Relaxed);
        }
    }

    /// Reports a command that ran: telemetry, a warning when slow, a debug
    /// line when asked.
    fn report(&self, command: &MongoCommand, elapsed: Duration, rows: Option<u64>, failed: bool) {
        let shared = &self.shared;
        let slow = shared.slow.is_some_and(|slow| elapsed >= slow);
        if !(shared.telemetry.is_active() || slow || shared.log_queries) {
            return;
        }
        let shape = command.shape();
        shared.telemetry.emit_with(EventKind::QueryExecuted, || {
            TelemetryEvent::QueryExecuted(QueryExecuted {
                sql: shape.clone(),
                duration: elapsed,
                rows,
                connection: shared.connection.clone(),
                failed,
            })
        });
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        if slow {
            tracing::warn!(target: "navree_mongo::query", command = %shape, elapsed_ms, failed, "slow MongoDB command");
        } else if shared.log_queries {
            tracing::debug!(target: "navree_mongo::query", command = %shape, elapsed_ms, failed, "MongoDB command");
        }
    }

    /// Runs `run`, reporting `command`.
    async fn timed<T>(
        &self,
        command: &MongoCommand,
        rows: impl Fn(&T) -> Option<u64>,
        run: impl Future<Output = Result<T, mongodb::error::Error>>,
    ) -> Result<T, OrmError> {
        let started = Instant::now();
        let result = run.await;
        let elapsed = started.elapsed();
        match &result {
            Ok(value) => self.report(command, elapsed, rows(value), false),
            Err(error) => {
                self.note(error);
                self.report(command, elapsed, None, true);
            }
        }
        result.map_err(|error| orm_error(&error))
    }

    /// The documents of a `find`.
    pub(crate) async fn find_documents(&self, find: &Find) -> Result<Vec<Document>, OrmError> {
        let command = MongoCommand::Find(find.clone());
        let collection = self.database.collection::<Document>(&find.collection);
        let mut action = collection.find(find.filter.clone());
        if let Some(sort) = &find.sort {
            action = action.sort(sort.clone());
        }
        if let Some(projection) = &find.projection {
            action = action.projection(projection.clone());
        }
        if let Some(skip) = find.skip {
            action = action.skip(skip);
        }
        if let Some(limit) = find.limit {
            action = action.limit(i64::try_from(limit).unwrap_or(i64::MAX));
        }
        self.timed(
            &command,
            |documents: &Vec<Document>| Some(documents.len() as u64),
            async {
                match &self.session {
                    None => action.await?.try_collect().await,
                    Some(state) => {
                        let mut session = state.session.lock().await;
                        let mut cursor = action.session(&mut *session).await?;
                        cursor.stream(&mut *session).try_collect().await
                    }
                }
            },
        )
        .await
    }

    /// The documents an aggregation pipeline gives.
    pub(crate) async fn aggregate_documents(
        &self,
        aggregate: &Aggregate,
    ) -> Result<Vec<Document>, OrmError> {
        let command = MongoCommand::Aggregate(aggregate.clone());
        let collection = self.database.collection::<Document>(&aggregate.collection);
        let action = collection.aggregate(aggregate.pipeline.clone());
        self.timed(
            &command,
            |documents: &Vec<Document>| Some(documents.len() as u64),
            async {
                match &self.session {
                    None => action.await?.try_collect().await,
                    Some(state) => {
                        let mut session = state.session.lock().await;
                        let mut cursor = action.session(&mut *session).await?;
                        cursor.stream(&mut *session).try_collect().await
                    }
                }
            },
        )
        .await
    }

    /// The documents a read command gives.
    pub(crate) async fn read_documents(
        &self,
        command: &MongoCommand,
    ) -> Result<Vec<Document>, OrmError> {
        match command {
            MongoCommand::Find(find) => self.find_documents(find).await,
            MongoCommand::Aggregate(aggregate) => self.aggregate_documents(aggregate).await,
            other => Err(invalid(format!("{other} does not read documents"))),
        }
    }

    /// The documents of a read command as its cursor yields them, batch by
    /// batch, in the session when there is one (the session is locked
    /// only while a batch is fetched, so other commands can run in it
    /// between them). The command is reported when the cursor opens; the
    /// stream ends after its first error.
    pub(crate) fn stream_documents(
        self: Arc<Self>,
        command: MongoCommand,
    ) -> BoxStream<'static, Result<Document, OrmError>> {
        // A `find` streams as the pipeline it is equivalent to.
        let Some(aggregate) = command.clone().into_aggregate() else {
            let error = invalid(format!("{} does not read documents", command.shape()));
            return stream::once(async move { Err(error) }).boxed();
        };
        let opened = async move {
            let started = Instant::now();
            let collection = self.database.collection::<Document>(&aggregate.collection);
            let action = collection.aggregate(aggregate.pipeline);
            let opened = match &self.session {
                None => action.await.map(OpenCursor::Plain),
                Some(state) => {
                    let mut session = state.session.lock().await;
                    action
                        .session(&mut *session)
                        .await
                        .map(|cursor| OpenCursor::Session(cursor, state.clone()))
                }
            };
            let elapsed = started.elapsed();
            match opened {
                Ok(open) => {
                    self.report(&command, elapsed, None, false);
                    Ok(open)
                }
                Err(error) => {
                    self.note(&error);
                    self.report(&command, elapsed, None, true);
                    Err(orm_error(&error))
                }
            }
        };
        stream::once(opened)
            .flat_map(|opened| match opened {
                Err(error) => stream::once(async move { Err(error) }).boxed(),
                Ok(OpenCursor::Plain(cursor)) => cursor
                    .map(|document| document.map_err(|error| orm_error(&error)))
                    .boxed(),
                Ok(OpenCursor::Session(cursor, state)) => {
                    stream::unfold(Some((cursor, state)), |open| async move {
                        let (mut cursor, state) = open?;
                        let next = {
                            let mut session = state.session.lock().await;
                            cursor.next(&mut session).await
                        };
                        match next? {
                            Ok(document) => Some((Ok(document), Some((cursor, state)))),
                            Err(error) => Some((Err(orm_error(&error)), None)),
                        }
                    })
                    .boxed()
                }
            })
            .boxed()
    }

    async fn count_documents(&self, count: &Count) -> Result<u64, OrmError> {
        let command = MongoCommand::Count(count.clone());
        let collection = self.database.collection::<Document>(&count.collection);
        let action = collection.count_documents(count.filter.clone());
        // One value comes back, whatever it counts.
        self.timed(&command, |_: &u64| Some(1), async {
            match &self.session {
                None => action.await,
                Some(state) => {
                    let mut session = state.session.lock().await;
                    action.session(&mut *session).await
                }
            }
        })
        .await
    }

    async fn insert_documents(&self, insert: &Insert) -> Result<u64, OrmError> {
        if insert.documents.is_empty() {
            return Ok(0);
        }
        let command = MongoCommand::Insert(insert.clone());
        let collection = self.database.collection::<Document>(&insert.collection);
        let action = collection.insert_many(insert.documents.clone());
        let result = self
            .timed(&command, |result: &mongodb::results::InsertManyResult| {
                Some(result.inserted_ids.len() as u64)
            }, async {
                match &self.session {
                    None => action.await,
                    Some(state) => {
                        let mut session = state.session.lock().await;
                        action.session(&mut *session).await
                    }
                }
            })
            .await?;
        Ok(result.inserted_ids.len() as u64)
    }

    async fn update_documents(&self, update: &Update) -> Result<u64, OrmError> {
        let command = MongoCommand::Update(update.clone());
        let collection = self.database.collection::<Document>(&update.collection);
        let modifications = match &update.update {
            UpdateDocument::Operators(operators) => {
                mongodb::options::UpdateModifications::Document(operators.clone())
            }
            UpdateDocument::Pipeline(pipeline) => {
                mongodb::options::UpdateModifications::Pipeline(pipeline.clone())
            }
        };
        let action = collection.update_many(update.filter.clone(), modifications);
        let result = self
            .timed(
                &command,
                |result: &mongodb::results::UpdateResult| Some(result.matched_count),
                async {
                    match &self.session {
                        None => action.await,
                        Some(state) => {
                            let mut session = state.session.lock().await;
                            action.session(&mut *session).await
                        }
                    }
                },
            )
            .await?;
        Ok(result.matched_count)
    }

    async fn delete_documents(&self, delete: &Delete) -> Result<u64, OrmError> {
        let command = MongoCommand::Delete(delete.clone());
        let collection = self.database.collection::<Document>(&delete.collection);
        let action = collection.delete_many(delete.filter.clone());
        let result = self
            .timed(
                &command,
                |result: &mongodb::results::DeleteResult| Some(result.deleted_count),
                async {
                    match &self.session {
                        None => action.await,
                        Some(state) => {
                            let mut session = state.session.lock().await;
                            action.session(&mut *session).await
                        }
                    }
                },
            )
            .await?;
        Ok(result.deleted_count)
    }

    /// Whether the driver runs in a session with an open transaction.
    fn in_transaction(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|state| state.transaction.load(Ordering::Relaxed))
    }

    /// Inserts the documents of `insert` unordered, skipping those a unique
    /// index refuses as duplicates; returns how many went in. Any other
    /// refusal is an error, and so is a duplicate inside a transaction,
    /// which MongoDB aborts on any write error.
    pub(crate) async fn insert_or_ignore(&self, insert: &Insert) -> Result<u64, OrmError> {
        if insert.documents.is_empty() {
            return Ok(0);
        }
        let command = MongoCommand::InsertOrIgnore(insert.clone());
        let collection = self.database.collection::<Document>(&insert.collection);
        let action = collection
            .insert_many(insert.documents.clone())
            .ordered(false);
        let started = Instant::now();
        let result = match &self.session {
            None => action.await,
            Some(state) => {
                let mut session = state.session.lock().await;
                action.session(&mut *session).await
            }
        };
        let elapsed = started.elapsed();
        let error = match result {
            Ok(result) => {
                let inserted = result.inserted_ids.len() as u64;
                self.report(&command, elapsed, Some(inserted), false);
                return Ok(inserted);
            }
            Err(error) => error,
        };
        let duplicates = match error.kind.as_ref() {
            mongodb::error::ErrorKind::InsertMany(failure)
                if failure.write_concern_error.is_none() =>
            {
                failure
                    .write_errors
                    .as_ref()
                    .filter(|errors| {
                        errors
                            .iter()
                            .all(|write| crate::error::is_duplicate_code(write.code))
                    })
                    .map(Vec::len)
            }
            _ => None,
        };
        match duplicates {
            Some(skipped) if !self.in_transaction() => {
                let inserted = (insert.documents.len() - skipped) as u64;
                self.report(&command, elapsed, Some(inserted), false);
                Ok(inserted)
            }
            Some(_) => {
                self.note(&error);
                self.report(&command, elapsed, None, true);
                Err(OrmError::Driver {
                    driver: Cow::Borrowed(DRIVER),
                    message: format!(
                        "insert_or_ignore met duplicates inside a transaction, which MongoDB aborts on any write error: upsert by the unique fields instead (`upsert(.., unique_by, [])` inserts only what is missing), or insert outside the unit of work ({error})"
                    ),
                })
            }
            None => {
                self.note(&error);
                self.report(&command, elapsed, None, true);
                Err(orm_error(&error))
            }
        }
    }

    /// Runs the upserts of `upsert` in one unordered `update` command;
    /// returns how many documents they matched or inserted.
    pub(crate) async fn upsert(&self, upsert: &Upsert) -> Result<u64, OrmError> {
        if upsert.rows.is_empty() {
            return Ok(0);
        }
        let command = MongoCommand::Upsert(upsert.clone());
        let updates: Vec<Document> = upsert
            .rows
            .iter()
            .map(|(filter, update)| doc! { "q": filter.clone(), "u": update.clone(), "upsert": true })
            .collect();
        let action = self.database.run_command(doc! {
            "update": upsert.collection.clone(),
            "updates": updates,
            "ordered": false,
        });
        let reply = self
            .timed(&command, |reply: &Document| reply_count(reply), async {
                match &self.session {
                    None => action.await,
                    Some(state) => {
                        let mut session = state.session.lock().await;
                        action.session(&mut *session).await
                    }
                }
            })
            .await?;
        if let Ok(errors) = reply.get_array("writeErrors")
            && let Some(first) = errors.iter().find_map(Bson::as_document)
        {
            let message = first.get_str("errmsg").unwrap_or("refused").to_owned();
            let code = first.get("code").and_then(Bson::as_i32).unwrap_or_default();
            return Err(if crate::error::is_duplicate_code(code) {
                OrmError::unique_violation(DRIVER, message)
            } else {
                OrmError::Driver {
                    driver: Cow::Borrowed(DRIVER),
                    message: format!("an upsert into `{}` failed: {message}", upsert.collection),
                }
            });
        }
        Ok(reply_count(&reply).unwrap_or_default())
    }

    /// Takes `count` numbers from the counter of `collection.field`;
    /// returns the first.
    async fn next_keys(&self, collection: &str, field: &str, count: u64) -> Result<i64, OrmError> {
        let count = i64::try_from(count).map_err(|_| invalid("too many rows in one insert"))?;
        let counters = self.database.collection::<Document>(&self.shared.sequences);
        let filter = doc! { "_id": format!("{collection}.{field}") };
        let update = doc! { "$inc": { "value": count } };
        let command = MongoCommand::Update(Update {
            collection: self.shared.sequences.clone(),
            filter: filter.clone(),
            update: UpdateDocument::Operators(update.clone()),
        });
        let action = counters
            .find_one_and_update(filter, update)
            .upsert(true)
            .return_document(ReturnDocument::After);
        let counter = self
            .timed(&command, |_: &Option<Document>| Some(1), async {
                match &self.session {
                    None => action.await,
                    Some(state) => {
                        let mut session = state.session.lock().await;
                        action.session(&mut *session).await
                    }
                }
            })
            .await?
            .ok_or_else(|| OrmError::Driver {
                driver: Cow::Borrowed(DRIVER),
                message: format!("the key counter of `{collection}.{field}` was not returned"),
            })?;
        let value = match counter.get("value") {
            Some(Bson::Int64(value)) => *value,
            Some(Bson::Int32(value)) => i64::from(*value),
            _ => {
                return Err(OrmError::Driver {
                    driver: Cow::Borrowed(DRIVER),
                    message: format!("the key counter of `{collection}.{field}` is not a number"),
                });
            }
        };
        Ok(value - count + 1)
    }

    /// Rows of a read: in the query's column order when it names them.
    pub(crate) async fn rows(&self, query: &QueryIr) -> Result<Vec<Row>, OrmError> {
        if query.limit == Some(0) {
            query.check()?;
            return Ok(Vec::new());
        }
        let (command, columns) = compile::read_with_columns(query)?;
        let documents = self.read_documents(&command).await?;
        Ok(documents
            .into_iter()
            .map(|document| document_row(document, columns.as_deref()))
            .collect())
    }

    /// The documents a target matches, by `_id`: a filter, or the ids a
    /// pipeline finds.
    async fn target_filter(&self, target: Target) -> Result<Document, OrmError> {
        match target {
            Target::Filter(filter) => Ok(filter),
            Target::Keys(pipeline) => {
                let ids: Vec<Bson> = self
                    .aggregate_documents(&pipeline)
                    .await?
                    .into_iter()
                    .filter_map(|mut document| document.remove("_id"))
                    .collect();
                Ok(doc! { "_id": { "$in": ids } })
            }
        }
    }

    /// Runs the uncorrelated `IN (subquery)` conditions of `wheres` first,
    /// so a write filters by their values (an index serves `$in`).
    async fn resolve_subqueries(&self, wheres: &mut [WhereClause]) -> Result<(), OrmError> {
        for clause in wheres {
            self.resolve(&mut clause.filter).await?;
        }
        Ok(())
    }

    fn resolve<'a>(&'a self, filter: &'a mut Where) -> BoxFuture<'a, Result<(), OrmError>> {
        Box::pin(async move {
            match filter {
                Where::In {
                    values: values @ InValues::Subquery(_),
                    ..
                } => {
                    if let InValues::Subquery(query) = &*values
                        && !compile::is_correlated(query)
                    {
                        let rows = self.rows(query).await?;
                        let mut list = Vec::with_capacity(rows.len());
                        for row in rows {
                            let first = row.values()?.into_iter().next();
                            list.push(Expr::Value(first.map_or(Value::String(None), |(_, v)| v)));
                        }
                        *values = InValues::List(list);
                    }
                }
                Where::Group(clauses) => {
                    for clause in clauses {
                        self.resolve(&mut clause.filter).await?;
                    }
                }
                _ => {}
            }
            Ok(())
        })
    }

    /// Runs a database command (`{"ping": 1}`, `{"create": ..}`), in the
    /// session when there is one.
    pub async fn run_command(&self, command: Document) -> Result<Document, MongoError> {
        let action = self.database.run_command(command);
        Ok(match &self.session {
            None => action.await,
            Some(state) => {
                let mut session = state.session.lock().await;
                action.session(&mut *session).await
            }
        }
        .inspect_err(|error| self.note(error))?)
    }

    /// The documents of an aggregation pipeline on `collection`, in the
    /// session when there is one.
    pub async fn pipeline(
        &self,
        collection: &str,
        pipeline: Vec<Document>,
    ) -> Result<Vec<Document>, OrmError> {
        self.aggregate_documents(&Aggregate {
            collection: collection.to_owned(),
            pipeline,
        })
        .await
    }
}

impl DocumentDriver for MongoDriver {
    fn name(&self) -> &'static str {
        DRIVER
    }

    fn time_precision(&self) -> u32 {
        3
    }

    fn find<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<Vec<Row>, OrmError>> {
        Box::pin(self.rows(query))
    }

    fn aggregate<'a>(
        &'a self,
        query: &'a QueryIr,
        aggregate: &'a AggregateIr,
    ) -> BoxFuture<'a, Result<Value, OrmError>> {
        Box::pin(async move {
            let counts = aggregate.function == navree_orm::ir::AggregateFunction::Count;
            let nothing = if counts {
                Value::BigInt(Some(0))
            } else {
                Value::String(None)
            };
            if query.limit == Some(0) {
                query.check()?;
                return Ok(nothing);
            }
            match compile::aggregate(query, aggregate)? {
                MongoCommand::Count(count) => {
                    let n = self.count_documents(&count).await?;
                    Ok(Value::BigInt(Some(i64::try_from(n).unwrap_or(i64::MAX))))
                }
                command => {
                    let documents = self.read_documents(&command).await?;
                    Ok(documents
                        .into_iter()
                        .next()
                        .and_then(|mut document| document.remove("aggregate"))
                        .map_or(nothing, from_bson))
                }
            }
        })
    }

    fn exists<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<bool, OrmError>> {
        Box::pin(async move {
            if query.limit == Some(0) {
                query.check()?;
                return Ok(false);
            }
            let command = compile::exists(query)?;
            Ok(!self.read_documents(&command).await?.is_empty())
        })
    }

    fn insert<'a>(
        &'a self,
        insert: &'a InsertIr,
    ) -> BoxFuture<'a, Result<InsertOutcome, OrmError>> {
        Box::pin(async move {
            let mut command = compile::insert(insert)?;
            let mut returned = Vec::new();
            if !insert.returning.is_empty() && !command.documents.is_empty() {
                // Keys the database assigns: counted per collection and field.
                let mut firsts = Vec::with_capacity(insert.returning.len());
                for field in &insert.returning {
                    let first = self
                        .next_keys(&command.collection, field, command.documents.len() as u64)
                        .await?;
                    firsts.push((field.to_string(), first));
                }
                for (i, document) in command.documents.iter_mut().enumerate() {
                    let offset = i64::try_from(i).unwrap_or(i64::MAX);
                    let mut row = Vec::with_capacity(firsts.len());
                    for (field, first) in &firsts {
                        let key = first + offset;
                        document.insert(field.clone(), key);
                        row.push((field.clone(), Value::BigInt(Some(key))));
                    }
                    returned.push(Row::from_values(row));
                }
            }
            let inserted = self.insert_documents(&command).await?;
            Ok(InsertOutcome {
                rows_affected: inserted,
                returned,
                last_insert_id: None,
            })
        })
    }

    fn update<'a>(&'a self, update: &'a UpdateIr) -> BoxFuture<'a, Result<u64, OrmError>> {
        Box::pin(async move {
            let mut wheres = update.wheres.clone();
            self.resolve_subqueries(&mut wheres).await?;
            let target = compile::target(&update.table, &wheres)?;
            let filter = self.target_filter(target).await?;
            let document = compile::update_document(update)?;
            self.update_documents(&Update {
                collection: update.table.name.to_string(),
                filter,
                update: document,
            })
            .await
        })
    }

    fn delete<'a>(&'a self, delete: &'a DeleteIr) -> BoxFuture<'a, Result<u64, OrmError>> {
        Box::pin(async move {
            let mut wheres = delete.wheres.clone();
            self.resolve_subqueries(&mut wheres).await?;
            let target = compile::target(&delete.table, &wheres)?;
            let filter = self.target_filter(target).await?;
            self.delete_documents(&Delete {
                collection: delete.table.name.to_string(),
                filter,
            })
            .await
        })
    }
}

impl std::fmt::Debug for MongoDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoDriver")
            .field("connection", &self.shared.connection)
            .field("in_session", &self.session.is_some())
            .finish()
    }
}

/// An open cursor: the client's, or a session's (read batch by batch with
/// the session).
enum OpenCursor {
    Plain(mongodb::Cursor<Document>),
    Session(mongodb::SessionCursor<Document>, Arc<SessionState>),
}

/// The documents an `update` command's reply says it matched or inserted
/// (`n`).
fn reply_count(reply: &Document) -> Option<u64> {
    match reply.get("n") {
        Some(Bson::Int32(n)) => u64::try_from(*n).ok(),
        Some(Bson::Int64(n)) => u64::try_from(*n).ok(),
        _ => None,
    }
}
