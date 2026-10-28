//! The `qdrant` vector store, over Qdrant's REST API: one Qdrant collection
//! per collection. Point ids are UUIDs derived from your ids, which are
//! kept in the payload with the text and metadata.

use http::Method;
use navree_http_client::{HttpClient, RequestBuilder};
use navree_kernel::BoxFuture;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{Filter, VectorDriver, VectorMatch, VectorRecord};
use crate::error::AiError;
use crate::options::Metric;

/// A Qdrant server.
pub(crate) struct Qdrant {
    base: String,
    key: Option<String>,
    dimensions: usize,
    metric: Metric,
    http: HttpClient,
}

impl Qdrant {
    pub(crate) fn new(
        url: String,
        key: Option<String>,
        dimensions: usize,
        metric: Metric,
        http: HttpClient,
    ) -> Self {
        Self {
            base: url.trim_end_matches('/').to_owned(),
            key: key.filter(|k| !k.is_empty()),
            dimensions,
            metric,
            http,
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .retries(0);
        if let Some(key) = &self.key {
            request = request.header("api-key", key.as_str());
        }
        request
    }

    /// Sends `request` and returns `result` of the answer.
    async fn send(&self, request: RequestBuilder) -> Result<Value, AiError> {
        let response = request
            .send()
            .await
            .map_err(|error| AiError::internal(format!("Qdrant request failed: {error}")))?;
        let body: Value = response.json().unwrap_or(Value::Null);
        if !response.is_success() {
            let message = body
                .pointer("/status/error")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| response.text());
            return Err(AiError::internal(format!(
                "Qdrant answered {}: {message}",
                response.status()
            )));
        }
        Ok(body.get("result").cloned().unwrap_or(Value::Null))
    }

    fn distance(&self) -> &'static str {
        match self.metric {
            Metric::Cosine => "Cosine",
            Metric::Dot => "Dot",
            Metric::Euclidean => "Euclid",
        }
    }
}

/// The Qdrant point id of `id`: a UUID from its SHA-256.
pub(crate) fn point_id(id: &str) -> Uuid {
    let digest = Sha256::digest(id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // Version 8 (custom), RFC 9562 variant.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// The Qdrant filter of `filter` (on `metadata.<field>`).
pub(crate) fn qdrant_filter(filter: &Filter) -> Value {
    match filter {
        Filter::And(filters) => {
            json!({ "must": filters.iter().map(condition).collect::<Vec<_>>() })
        }
        Filter::Or(filters) => {
            json!({ "should": filters.iter().map(condition).collect::<Vec<_>>() })
        }
        Filter::Not(inner) => json!({ "must_not": [condition(inner)] }),
        other => json!({ "must": [condition(other)] }),
    }
}

/// One condition (nested filters become nested filter objects).
fn condition(filter: &Filter) -> Value {
    let key = |field: &str| format!("metadata.{field}");
    // The REST API takes numbers and RFC 3339 datetime strings in `range`
    // alike (`datetime_range` exists only in gRPC).
    let range =
        |field: &str, op: &str, value: &Value| json!({ "key": key(field), "range": { op: value } });
    match filter {
        Filter::Eq(field, value) => json!({ "key": key(field), "match": { "value": value } }),
        Filter::AnyOf(field, values) => json!({ "key": key(field), "match": { "any": values } }),
        Filter::Gt(field, value) => range(field, "gt", value),
        Filter::Gte(field, value) => range(field, "gte", value),
        Filter::Lt(field, value) => range(field, "lt", value),
        Filter::Lte(field, value) => range(field, "lte", value),
        nested => qdrant_filter(nested),
    }
}

impl VectorDriver for Qdrant {
    fn ensure<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            let response = self
                .request(Method::GET, &format!("/collections/{collection}"))
                .send()
                .await
                .map_err(|error| AiError::internal(format!("Qdrant request failed: {error}")))?;
            if response.status().as_u16() == 404 {
                self.send(
                    self.request(Method::PUT, &format!("/collections/{collection}"))
                        .json(&json!({
                            "vectors": { "size": self.dimensions, "distance": self.distance() }
                        })),
                )
                .await?;
                return Ok(());
            }
            if !response.is_success() {
                return Err(AiError::internal(format!(
                    "Qdrant answered {} for collection `{collection}`",
                    response.status()
                )));
            }
            let body: Value = response.json().unwrap_or(Value::Null);
            let size = body
                .pointer("/result/config/params/vectors/size")
                .and_then(Value::as_u64);
            match size {
                Some(size) if size as usize != self.dimensions => Err(AiError::config(format!(
                    "the Qdrant collection `{collection}` has vectors of {size} dimensions; ai.vectors.dimensions is {}",
                    self.dimensions
                ))),
                _ => Ok(()),
            }
        })
    }

    fn upsert<'a>(
        &'a self,
        collection: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            let points: Vec<Value> = records
                .into_iter()
                .map(|record| {
                    json!({
                        "id": point_id(&record.id),
                        "vector": record.embedding,
                        "payload": { "id": record.id, "text": record.text, "metadata": record.metadata },
                    })
                })
                .collect();
            self.send(
                self.request(
                    Method::PUT,
                    &format!("/collections/{collection}/points?wait=true"),
                )
                .json(&json!({ "points": points })),
            )
            .await?;
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        collection: &'a str,
        ids: Vec<String>,
    ) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            let points: Vec<Uuid> = ids.iter().map(|id| point_id(id)).collect();
            self.send(
                self.request(
                    Method::POST,
                    &format!("/collections/{collection}/points/delete?wait=true"),
                )
                .json(&json!({ "points": points })),
            )
            .await?;
            Ok(())
        })
    }

    fn delete_where<'a>(
        &'a self,
        collection: &'a str,
        filter: &'a Filter,
    ) -> BoxFuture<'a, Result<(), AiError>> {
        Box::pin(async move {
            self.send(
                self.request(
                    Method::POST,
                    &format!("/collections/{collection}/points/delete?wait=true"),
                )
                .json(&json!({ "filter": qdrant_filter(filter) })),
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
            let mut body = json!({ "vector": vector, "limit": limit, "with_payload": true });
            if let Some(filter) = filter {
                body["filter"] = qdrant_filter(filter);
            }
            // Qdrant's Euclid scores are distances; their threshold is
            // applied after converting them.
            if let (Some(min), false) = (min_score, self.metric == Metric::Euclidean) {
                body["score_threshold"] = json!(min);
            }
            let result = self
                .send(
                    self.request(
                        Method::POST,
                        &format!("/collections/{collection}/points/search"),
                    )
                    .json(&body),
                )
                .await?;
            let metric = self.metric;
            let matches = result
                .as_array()
                .into_iter()
                .flatten()
                .map(|point| {
                    let raw = point["score"].as_f64().unwrap_or(0.0) as f32;
                    let score = if metric == Metric::Euclidean {
                        1.0 / (1.0 + raw)
                    } else {
                        raw
                    };
                    let payload = &point["payload"];
                    VectorMatch {
                        id: payload["id"].as_str().unwrap_or_default().to_owned(),
                        score,
                        text: payload["text"].as_str().unwrap_or_default().to_owned(),
                        metadata: payload
                            .get("metadata")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    }
                })
                .filter(|m| min_score.is_none_or(|min| m.score >= min))
                .collect();
            Ok(matches)
        })
    }

    fn count<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<u64, AiError>> {
        Box::pin(async move {
            let result = self
                .send(
                    self.request(
                        Method::POST,
                        &format!("/collections/{collection}/points/count"),
                    )
                    .json(&json!({ "exact": true })),
                )
                .await?;
            Ok(result["count"].as_u64().unwrap_or(0))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_ids_are_stable_uuids() {
        let a = point_id("0199c1d2:0");
        assert_eq!(a, point_id("0199c1d2:0"));
        assert_ne!(a, point_id("0199c1d2:1"));
        assert_eq!(a.get_version_num(), 8);
    }

    #[test]
    fn filters_translate() {
        let filter = Filter::eq("subject_id", "d1")
            .and(Filter::any_of("kind", ["call", "note"]))
            .and(Filter::gte("created_at", "2026-10-01T00:00:00Z"))
            .and(Filter::lt("count", 5))
            .and(Filter::not(Filter::eq("owner_id", "u2")));
        assert_eq!(
            qdrant_filter(&filter),
            json!({ "must": [
                { "key": "metadata.subject_id", "match": { "value": "d1" } },
                { "key": "metadata.kind", "match": { "any": ["call", "note"] } },
                { "key": "metadata.created_at", "range": { "gte": "2026-10-01T00:00:00Z" } },
                { "key": "metadata.count", "range": { "lt": 5 } },
                { "must_not": [{ "key": "metadata.owner_id", "match": { "value": "u2" } }] }
            ]})
        );
        assert_eq!(
            qdrant_filter(&Filter::eq("a", 1).or(Filter::eq("b", 2))),
            json!({ "should": [
                { "key": "metadata.a", "match": { "value": 1 } },
                { "key": "metadata.b", "match": { "value": 2 } }
            ]})
        );
    }
}
