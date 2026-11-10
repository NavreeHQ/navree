//! Results as MongoDB's cursors yield them ([`MongoStream`]).

use futures_util::stream::{self, BoxStream, StreamExt};
use navree_orm::{Access, Builder, Model, OrmError, Record, Row, TableBuilder};

use crate::compile;
use crate::connection::MongoExecutor;
use crate::convert::document_row;
use crate::native::model_row;

/// A query's results as MongoDB's cursor yields them, batch by batch: memory
/// holds one batch rather than the whole result (exports, backfills,
/// reports over large collections). In a unit of work (or a session) the
/// cursor reads in its session, and other commands may run in it between
/// batches.
///
/// ```no_run
/// use futures_util::TryStreamExt;
/// use navree_mongo::{MongoDb, MongoStream};
/// use navree_orm::Model;
/// # #[derive(Model, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
/// # #[model(collection = "events", driver = "mongodb")]
/// # struct Event { id: navree_mongo::ObjectId, kind: String }
///
/// # async fn demo(mongo: MongoDb) -> Result<(), navree_orm::OrmError> {
/// let mut signups = Event::query().where_(Event::KIND.eq("signup")).stream(&mongo);
/// while let Some(event) = signups.try_next().await? {
///     println!("{}", event.id);
/// }
/// # Ok(()) }
/// ```
pub trait MongoStream {
    /// What the stream yields: records for a model query, rows for a table
    /// query.
    type Item;

    /// The results in the query's order; the stream ends after the first
    /// error. Records run their `retrieved` hooks as they arrive.
    fn stream<'e, E: MongoExecutor + ?Sized>(
        self,
        exec: &'e E,
    ) -> BoxStream<'e, Result<Self::Item, OrmError>>;
}

impl<M: Model> MongoStream for Builder<M> {
    type Item = Record<M>;

    fn stream<'e, E: MongoExecutor + ?Sized>(
        self,
        exec: &'e E,
    ) -> BoxStream<'e, Result<Record<M>, OrmError>> {
        let command = compile::read(&self.into_ir());
        let opened = async move {
            let command = command?;
            let target = exec.target(Access::Read).await?;
            let documents = exec.mongo_driver()?.stream_documents(command);
            Ok::<_, OrmError>((documents, target))
        };
        stream::once(opened)
            .flat_map(|opened| match opened {
                Err(error) => stream::once(async move { Err(error) }).boxed(),
                Ok(open) => stream::unfold(Some(open), |open| async move {
                    let (mut documents, target) = open?;
                    let document = match documents.next().await? {
                        Ok(document) => document,
                        Err(error) => return Some((Err(error), None)),
                    };
                    let row = model_row::<M>(document);
                    match target.records::<M>(std::slice::from_ref(&row)).await {
                        Ok(records) => {
                            let record = records.into_iter().next()?;
                            Some((Ok(record), Some((documents, target))))
                        }
                        Err(error) => Some((Err(error), None)),
                    }
                })
                .boxed(),
            })
            .boxed()
    }
}

impl MongoStream for TableBuilder {
    type Item = Row;

    fn stream<'e, E: MongoExecutor + ?Sized>(
        self,
        exec: &'e E,
    ) -> BoxStream<'e, Result<Row, OrmError>> {
        let read = compile::read_with_columns(&self.into_ir());
        let opened = async move {
            let (command, columns) = read?;
            let documents = exec.mongo_driver()?.stream_documents(command);
            Ok::<_, OrmError>((documents, columns))
        };
        stream::once(opened)
            .flat_map(|opened| match opened {
                Err(error) => stream::once(async move { Err(error) }).boxed(),
                Ok((documents, columns)) => documents
                    .map(move |document| {
                        document.map(|document| document_row(document, columns.as_deref()))
                    })
                    .boxed(),
            })
            .boxed()
    }
}
