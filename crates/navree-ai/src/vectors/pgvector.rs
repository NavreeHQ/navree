//! The `pgvector` vector store: one table `ai_vectors_<name>` per
//! collection in the app's PostgreSQL database (`id`, `embedding
//! vector(n)` with an HNSW index, `text`, `metadata jsonb` with a GIN
//! index, `updated_at`), created with the `vector` extension on first use.

use std::sync::Arc;

use navree_db::Db;
use navree_kernel::BoxFuture;
use sea_orm::{ConnectionTrait, DbBackend, Statement, Value as SqlValue};
use serde_json::{Value, json};

use super::{Filter, VectorDriver, VectorMatch, VectorRecord, nested};
use crate::error::AiError;
use crate::options::Metric;

/// pgvector on the app's database.
pub(crate) struct PgVector {
    db: Arc<Db>,
    dimensions: usize,
    metric: Metric,
}

impl PgVector {
    pub(crate) fn new(db: Arc<Db>, dimensions: usize, metric: Metric) -> Self {
        Self {
            db,
            dimensions,
            metric,
        }
    }

    async fn execute(&self, sql: String, values: Vec<SqlValue>) -> Result<u64, AiError> {
        self.check_backend()?;
        let result = self
            .db
            .conn()
            .execute_raw(Statement::from_sql_and_values(DbBackend::Postgres, sql, values))
            .await
            .map_err(database)?;
        Ok(result.rows_affected())
    }

    fn check_backend(&self) -> Result<(), AiError> {
        if self.db.backend() != DbBackend::Postgres {
            return Err(AiError::config(
                "ai.vectors.store = \"pgvector\" needs a PostgreSQL database (database.url)",
            ));
        }
        Ok(())
    }
}

fn database(error: sea_orm::DbErr) -> AiError {
    AiError::internal(format!("pgvector: {error}"))
}

/// The table of `collection`.
pub(crate) fn table(collection: &str) -> String {
    format!("\"ai_vectors_{collection}\"")
}

/// pgvector's text form of a vector: `[1,2,3]`.
pub(crate) fn vector_literal(vector: &[f32]) -> String {
    let values: Vec<String> = vector.iter().map(|v| format!("{v}")).collect();
    format!("[{}]", values.join(","))
}

/// The distance operator and index operator class of `metric`.
fn operator(metric: Metric) -> (&'static str, &'static str) {
    match metric {
        Metric::Cosine => ("<=>", "vector_cosine_ops"),
        Metric::Dot => ("<#>", "vector_ip_ops"),
        Metric::Euclidean => ("<->", "vector_l2_ops"),
    }
}

/// The score expression ("higher is closer") of `distance`.
fn score_sql(metric: Metric, distance: &str) -> String {
    match metric {
        // `<=>` is 1 - cosine similarity.
        Metric::Cosine => format!("(1 - ({distance}))"),
        // `<#>` is the negative inner product.
        Metric::Dot => format!("(-({distance}))"),
        Metric::Euclidean => format!("(1 / (1 + ({distance})))"),
    }
}

/// The statements that create a collection's table and indexes.
pub(crate) fn create_sql(collection: &str, dimensions: usize, metric: Metric) -> Vec<String> {
    let table = table(collection);
    let (_, ops) = operator(metric);
    vec![
        "CREATE EXTENSION IF NOT EXISTS vector".to_owned(),
        format!(
            "CREATE TABLE IF NOT EXISTS {table} (\
             id text PRIMARY KEY, \
             embedding vector({dimensions}) NOT NULL, \
             text text NOT NULL DEFAULT '', \
             metadata jsonb NOT NULL DEFAULT '{{}}'::jsonb, \
             updated_at timestamptz NOT NULL DEFAULT now())"
        ),
        format!(
            "CREATE INDEX IF NOT EXISTS \"ai_vectors_{collection}_embedding\" ON {table} USING hnsw (embedding {ops})"
        ),
        format!(
            "CREATE INDEX IF NOT EXISTS \"ai_vectors_{collection}_metadata\" ON {table} USING gin (metadata)"
        ),
    ]
}

/// The SQL condition of `filter`, its parameters numbered from `next`.
pub(crate) fn filter_sql(filter: &Filter, values: &mut Vec<SqlValue>) -> String {
    let param = |value: SqlValue, values: &mut Vec<SqlValue>| {
        values.push(value);
        format!("${}", values.len())
    };
    let path = |field: &str| format!("'{{{}}}'", field.replace('.', ","));
    match filter {
        Filter::Eq(field, value) => {
            let p = param(SqlValue::from(nested(field, value.clone()).to_string()), values);
            format!("metadata @> {p}::jsonb")
        }
        Filter::AnyOf(field, options) => {
            if options.is_empty() {
                return "FALSE".to_owned();
            }
            let parts: Vec<String> = options
                .iter()
                .map(|value| {
                    let p = param(SqlValue::from(nested(field, value.clone()).to_string()), values);
                    format!("metadata @> {p}::jsonb")
                })
                .collect();
            format!("({})", parts.join(" OR "))
        }
        Filter::Gt(field, value)
        | Filter::Gte(field, value)
        | Filter::Lt(field, value)
        | Filter::Lte(field, value) => {
            let op = match filter {
                Filter::Gt(..) => ">",
                Filter::Gte(..) => ">=",
                Filter::Lt(..) => "<",
                _ => "<=",
            };
            let path = path(field);
            match value {
                Value::Number(number) => {
                    let p = param(SqlValue::from(number.as_f64().unwrap_or(0.0)), values);
                    format!(
                        "(jsonb_typeof(metadata #> {path}) = 'number' AND (metadata #>> {path})::double precision {op} {p})"
                    )
                }
                Value::String(text) => {
                    let p = param(SqlValue::from(text.clone()), values);
                    format!(
                        "(jsonb_typeof(metadata #> {path}) = 'string' AND (metadata #>> {path}) {op} {p})"
                    )
                }
                _ => "FALSE".to_owned(),
            }
        }
        Filter::Not(inner) => format!("NOT ({})", filter_sql(inner, values)),
        Filter::And(filters) => {
            let parts: Vec<String> = filters.iter().map(|f| filter_sql(f, values)).collect();
            if parts.is_empty() { "TRUE".to_owned() } else { format!("({})", parts.join(" AND ")) }
        }
        Filter::Or(filters) => {
            let parts: Vec<String> = filters.iter().map(|f| filter_sql(f, values)).collect();
            if parts.is_empty() { "FALSE".to_owned() } else { format!("({})", parts.join(" OR ")) }
        }
    }
}

/// The search query and its parameters.
pub(crate) fn search_sql(
    collection: &str,
    metric: Metric,
    vector: &[f32],
    filter: Option<&Filter>,
    limit: usize,
    min_score: Option<f32>,
) -> (String, Vec<SqlValue>) {
    let mut values = vec![SqlValue::from(vector_literal(vector))];
    let (op, _) = operator(metric);
    let distance = format!("embedding {op} $1::vector");
    let score = score_sql(metric, &distance);
    let mut conditions = Vec::new();
    if let Some(filter) = filter {
        conditions.push(filter_sql(filter, &mut values));
    }
    if let Some(min) = min_score {
        values.push(SqlValue::from(f64::from(min)));
        conditions.push(format!("{score} >= ${}", values.len()));
    }
    let condition = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT id, text, metadata, {score}::double precision AS score FROM {}{condition} ORDER BY {distance} LIMIT {limit}",
        table(collection)
    );
    (sql, values)
}

impl VectorDriver for PgVector {
    fn ensure<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            self.check_backend()?;
            let existing = self
                .db
                .conn()
                .query_one_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT a.atttypmod AS dimensions FROM pg_attribute a \
                     JOIN pg_class c ON c.oid = a.attrelid \
                     WHERE c.relname = $1 AND a.attname = 'embedding' AND c.relkind = 'r'",
                    [SqlValue::from(format!("ai_vectors_{collection}"))],
                ))
                .await
                .map_err(database)?;
            if let Some(row) = existing {
                let size: i32 = row.try_get("", "dimensions").map_err(database)?;
                if size as usize != self.dimensions {
                    return Err(AiError::config(format!(
                        "the table ai_vectors_{collection} has vectors of {size} dimensions; ai.vectors.dimensions is {}",
                        self.dimensions
                    )));
                }
            }
            for sql in create_sql(collection, self.dimensions, self.metric) {
                self.execute(sql, Vec::new()).await?;
            }
            Ok(())
        })
    }

    fn upsert<'a>(&'a self, collection: &'a str, records: Vec<VectorRecord>) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            let sql = format!(
                "INSERT INTO {} (id, embedding, text, metadata, updated_at) \
                 VALUES ($1, $2::vector, $3, $4::jsonb, now()) \
                 ON CONFLICT (id) DO UPDATE SET embedding = EXCLUDED.embedding, \
                 text = EXCLUDED.text, metadata = EXCLUDED.metadata, updated_at = now()",
                table(collection)
            );
            for record in records {
                self.execute(
                    sql.clone(),
                    vec![
                        SqlValue::from(record.id),
                        SqlValue::from(vector_literal(&record.embedding)),
                        SqlValue::from(record.text),
                        SqlValue::from(record.metadata.to_string()),
                    ],
                )
                .await?;
            }
            Ok(())
        })
    }

    fn delete<'a>(&'a self, collection: &'a str, ids: Vec<String>) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            for id in ids {
                self.execute(
                    format!("DELETE FROM {} WHERE id = $1", table(collection)),
                    vec![SqlValue::from(id)],
                )
                .await?;
            }
            Ok(())
        })
    }

    fn delete_where<'a>(&'a self, collection: &'a str, filter: &'a Filter) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            let mut values = Vec::new();
            let condition = filter_sql(filter, &mut values);
            self.execute(
                format!("DELETE FROM {} WHERE {condition}", table(collection)),
                values,
            )
            .await?;
            Ok(())
        })
    }

    fn search<'a>(
        &'a self,
        collection: &'a str,
        vector: &'a [f32],
        filter: Option<&'a Filter>,
        limit: usize,
        min_score: Option<f32>,
    ) -> BoxFuture<'a, Result<Vec<VectorMatch>, AiError>> {
        Box::pin(async move {
            self.check_backend()?;
            let (sql, values) = search_sql(collection, self.metric, vector, filter, limit, min_score);
            let rows = self
                .db
                .conn()
                .query_all_raw(Statement::from_sql_and_values(DbBackend::Postgres, sql, values))
                .await
                .map_err(database)?;
            rows.iter()
                .map(|row| {
                    Ok(VectorMatch {
                        id: row.try_get("", "id").map_err(database)?,
                        score: row.try_get::<f64>("", "score").map_err(database)? as f32,
                        text: row.try_get("", "text").map_err(database)?,
                        metadata: row
                            .try_get::<Value>("", "metadata")
                            .unwrap_or_else(|_| json!({})),
                    })
                })
                .collect()
        })
    }

    fn count<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<u64, AiError>> {
        Box::pin(async move {
            self.check_backend()?;
            let row = self
                .db
                .conn()
                .query_one_raw(Statement::from_string(
                    DbBackend::Postgres,
                    format!("SELECT COUNT(*) AS n FROM {}", table(collection)),
                ))
                .await
                .map_err(database)?;
            let count: i64 = match row {
                Some(row) => row.try_get("", "n").map_err(database)?,
                None => 0,
            };
            Ok(count.max(0) as u64)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_statements() {
        let sql = create_sql("activity_notes", 3, Metric::Cosine).join(";\n");
        assert!(sql.contains("CREATE EXTENSION IF NOT EXISTS vector"));
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS \"ai_vectors_activity_notes\""));
        assert!(sql.contains("embedding vector(3) NOT NULL"));
        assert!(sql.contains("USING hnsw (embedding vector_cosine_ops)"));
        assert!(sql.contains("USING gin (metadata)"));
        assert!(create_sql("x", 3, Metric::Euclidean)[2].contains("vector_l2_ops"));
    }

    #[test]
    fn search_statements_bind_every_value() {
        let filter = Filter::eq("subject_id", "d1")
            .and(Filter::any_of("kind", ["call", "note"]))
            .and(Filter::gte("created_at", "2026-10-01"))
            .and(Filter::not(Filter::lt("count", 3)));
        let (sql, values) = search_sql("notes", Metric::Cosine, &[0.5, 1.0], Some(&filter), 8, Some(0.2));
        assert_eq!(
            sql,
            "SELECT id, text, metadata, (1 - (embedding <=> $1::vector))::double precision AS score \
             FROM \"ai_vectors_notes\" WHERE (metadata @> $2::jsonb AND (metadata @> $3::jsonb OR metadata @> $4::jsonb) \
             AND (jsonb_typeof(metadata #> '{created_at}') = 'string' AND (metadata #>> '{created_at}') >= $5) \
             AND NOT ((jsonb_typeof(metadata #> '{count}') = 'number' AND (metadata #>> '{count}')::double precision < $6))) \
             AND (1 - (embedding <=> $1::vector)) >= $7 ORDER BY embedding <=> $1::vector LIMIT 8"
        );
        assert_eq!(values.len(), 7);
        assert_eq!(values[0], SqlValue::from("[0.5,1]".to_owned()));
        assert_eq!(values[1], SqlValue::from("{\"subject_id\":\"d1\"}".to_owned()));
    }

    #[test]
    fn nested_paths_and_metrics() {
        let mut values = Vec::new();
        assert_eq!(
            filter_sql(&Filter::gt("owner.level", 2), &mut values),
            "(jsonb_typeof(metadata #> '{owner,level}') = 'number' AND (metadata #>> '{owner,level}')::double precision > $1)"
        );
        let (sql, _) = search_sql("n", Metric::Dot, &[1.0], None, 3, None);
        assert!(sql.contains("(-(embedding <#> $1::vector))"), "{sql}");
        assert!(filter_sql(&Filter::any_of("k", Vec::<String>::new()), &mut values) == "FALSE");
    }
}
