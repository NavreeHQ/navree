//! MongoDB's own language in model queries: filter documents
//! ([`MongoFilter::document`]), aggregation stages after a query's own
//! ([`MongoQuery::pipeline`]), and a query shown as the command it runs
//! ([`ToMongo::to_mongo`]).

use bson::Document;
use navree_orm::ir::{NativeFilter, Where};
use navree_orm::{Access, Builder, Collection, Filter, Model, OrmError, Row, TableBuilder, Value};

use crate::command::{Aggregate, MongoCommand};
use crate::compile;
use crate::connection::MongoExecutor;
use crate::convert::{document_row, from_bson};
use crate::error::DRIVER;

/// A filter written as a MongoDB filter document, for what typed columns
/// do not say (`$geoWithin`, `$elemMatch`, `$where`, ...). It joins the
/// query's other conditions like any condition; SQL databases refuse it.
///
/// ```
/// use bson::doc;
/// use navree_mongo::MongoFilter;
/// use navree_orm::{Filter, table};
///
/// let near = Filter::document(doc! {
///     "location": { "$geoWithin": { "$centerSphere": [[-73.97, 40.77], 0.001] } }
/// });
/// let query = table("stores").where_(near).where_(navree_orm::col("open").eq(true));
/// # let _ = query;
/// ```
pub trait MongoFilter {
    /// The condition of `document`.
    fn document(document: Document) -> Self;
}

impl MongoFilter for Filter {
    fn document(document: Document) -> Filter {
        Filter::from(Where::Native(NativeFilter::new(DRIVER, document)))
    }
}

/// A query shown as the MongoDB command it runs, without running it (the
/// document store's `to_sql`).
///
/// ```
/// # use navree_mongo::ToMongo;
/// use navree_orm::{col, table};
///
/// let query = table("deals").where_(col("stage").eq("won")).order_by_desc("amount").limit(5);
/// assert_eq!(
///     query.to_mongo().unwrap().to_string(),
///     r#"db.deals.find({"stage": "won"}).sort({"amount": -1}).limit(5)"#
/// );
/// ```
pub trait ToMongo {
    /// The command: a `find`, or an aggregation pipeline.
    fn to_mongo(&self) -> Result<MongoCommand, OrmError>;
}

impl<M: Model> ToMongo for Builder<M> {
    fn to_mongo(&self) -> Result<MongoCommand, OrmError> {
        compile::read(&self.clone().into_ir())
    }
}

impl ToMongo for TableBuilder {
    fn to_mongo(&self) -> Result<MongoCommand, OrmError> {
        compile::read(&self.clone().into_ir())
    }
}

/// Aggregation stages after a query's own: the query's filter, order,
/// page and projection run first, then the stages (`$lookup`, `$unwind`,
/// `$addFields`, `$facet`, ...; not `$geoNear`, which must come first).
///
/// ```
/// use bson::doc;
/// use navree_mongo::MongoQuery;
/// use navree_orm::{col, table};
///
/// let sampled = table("deals")
///     .where_(col("stage").eq("won"))
///     .pipeline([doc! { "$sample": { "size": 5 } }]);
/// assert_eq!(
///     sampled.to_mongo().unwrap().to_string(),
///     r#"db.deals.aggregate([{"$match": {"stage": "won"}}, {"$sample": {"size": 5}}])"#
/// );
/// ```
pub trait MongoQuery: Sized {
    /// The query with the stages.
    type Pipeline;

    /// The query, then `stages`.
    fn pipeline(self, stages: impl IntoIterator<Item = Document>) -> Self::Pipeline;
}

impl<M: Model> MongoQuery for Builder<M> {
    type Pipeline = ModelPipeline<M>;

    fn pipeline(self, stages: impl IntoIterator<Item = Document>) -> ModelPipeline<M> {
        ModelPipeline {
            query: self,
            stages: stages.into_iter().collect(),
        }
    }
}

impl MongoQuery for TableBuilder {
    type Pipeline = RowPipeline;

    fn pipeline(self, stages: impl IntoIterator<Item = Document>) -> RowPipeline {
        RowPipeline {
            query: self,
            stages: stages.into_iter().collect(),
        }
    }
}

/// A model query followed by aggregation stages (see [`MongoQuery`]).
///
/// ```no_run
/// # use bson::doc;
/// # use navree_mongo::{MongoDb, MongoQuery, ObjectId};
/// # use navree_orm::Model;
/// # #[derive(Model, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
/// # #[model(collection = "deals", driver = "mongodb")]
/// # struct Deal { id: ObjectId, company_id: ObjectId }
/// # async fn demo(mongo: MongoDb) -> Result<(), navree_orm::OrmError> {
/// let deals = Deal::query()
///     .pipeline([doc! { "$lookup": {
///         "from": "companies", "localField": "company_id", "foreignField": "_id", "as": "company",
///     } }])
///     .get(&mongo)
///     .await?;
/// // Each record carries `company` as an extra (an array of documents).
/// # let _ = deals; Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct ModelPipeline<M: Model> {
    query: Builder<M>,
    stages: Vec<Document>,
}

impl<M: Model> ModelPipeline<M> {
    /// Adds a stage.
    pub fn stage(mut self, stage: Document) -> Self {
        self.stages.push(stage);
        self
    }

    /// The pipeline, without running it.
    pub fn to_mongo(&self) -> Result<MongoCommand, OrmError> {
        Ok(MongoCommand::Aggregate(self.aggregate()?))
    }

    fn aggregate(&self) -> Result<Aggregate, OrmError> {
        pipeline_of(self.query.to_mongo()?, &self.stages)
    }

    /// The records (fields the stages add are their extras).
    pub async fn get<E: MongoExecutor + ?Sized>(
        &self,
        exec: &E,
    ) -> Result<Collection<M>, OrmError> {
        let documents = self.documents(exec).await?;
        let rows: Vec<Row> = documents.into_iter().map(model_row::<M>).collect();
        let target = exec.target(Access::Read).await?;
        target.records::<M>(&rows).await
    }

    /// The documents, as the stages leave them.
    pub async fn documents<E: MongoExecutor + ?Sized>(
        &self,
        exec: &E,
    ) -> Result<Vec<Document>, OrmError> {
        exec.mongo_driver()?
            .aggregate_documents(&self.aggregate()?)
            .await
    }
}

/// A table query followed by aggregation stages (see [`MongoQuery`]).
///
/// ```no_run
/// # use navree_mongo::{MongoDb, MongoQuery};
/// # use navree_orm::table;
/// # async fn demo(mongo: MongoDb) -> Result<(), navree_orm::OrmError> {
/// // `[{_id: "won", count: 12}, {_id: "lead", count: 7}, ...]`
/// let per_stage = table("deals")
///     .pipeline([bson::doc! { "$sortByCount": "$stage" }])
///     .get(&mongo)
///     .await?;
/// # let _ = per_stage; Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct RowPipeline {
    query: TableBuilder,
    stages: Vec<Document>,
}

impl RowPipeline {
    /// Adds a stage.
    pub fn stage(mut self, stage: Document) -> Self {
        self.stages.push(stage);
        self
    }

    /// The pipeline, without running it.
    pub fn to_mongo(&self) -> Result<MongoCommand, OrmError> {
        Ok(MongoCommand::Aggregate(pipeline_of(
            self.query.to_mongo()?,
            &self.stages,
        )?))
    }

    /// The rows: every field of each document.
    pub async fn get<E: MongoExecutor + ?Sized>(&self, exec: &E) -> Result<Vec<Row>, OrmError> {
        Ok(self
            .documents(exec)
            .await?
            .into_iter()
            .map(|document| document_row(document, None))
            .collect())
    }

    /// The documents, as the stages leave them.
    pub async fn documents<E: MongoExecutor + ?Sized>(
        &self,
        exec: &E,
    ) -> Result<Vec<Document>, OrmError> {
        let aggregate = pipeline_of(self.query.to_mongo()?, &self.stages)?;
        exec.mongo_driver()?.aggregate_documents(&aggregate).await
    }
}

/// The pipeline of `command` with `stages` after it.
fn pipeline_of(command: MongoCommand, stages: &[Document]) -> Result<Aggregate, OrmError> {
    let mut aggregate = command
        .into_aggregate()
        .ok_or_else(|| crate::error::invalid("only reads take pipeline stages"))?;
    aggregate.pipeline.extend(stages.iter().cloned());
    Ok(aggregate)
}

/// A document as a row of model `M`: its columns first (NULL when the
/// document lacks one), then the other fields.
pub(crate) fn model_row<M: Model>(mut document: Document) -> Row {
    let mut values = Vec::with_capacity(document.len());
    for column in M::meta().all_columns() {
        let value = document
            .remove(column.name())
            .map_or(Value::String(None), from_bson);
        values.push((column.name().to_owned(), value));
    }
    values.extend(
        document
            .into_iter()
            .map(|(field, value)| (field, from_bson(value))),
    );
    Row::from_values(values)
}
