//! The `meilisearch` engine: a Meilisearch server over its REST API.

use std::collections::BTreeMap;
use std::time::Duration;

use http::Method;
use navree_http_client::HttpClient;
use navree_kernel::{BoxError, BoxFuture};
use serde_json::{Value, json};

use crate::engine::http::{Api, Auth, Body, encode};
use crate::engine::{EngineHit, EngineQuery, EngineResults, SearchEngine};
use crate::filter::{CmpOp, Filter};
use crate::settings::WORKSPACE_FIELD;
use crate::text::{MARK_CLOSE, MARK_OPEN, field_text, from_marked};
use crate::{Document, IndexSettings, SearchError};

/// A Meilisearch server. Each index is a Meilisearch index created on
/// first use, with its searchable, filterable and sortable attributes,
/// synonyms and stop words pushed by `prepare`. Writes wait for their
/// Meilisearch task, so a failed task fails the sync (and the job
/// retries).
pub struct MeilisearchEngine {
    api: Api,
    task_timeout: Duration,
}

impl MeilisearchEngine {
    /// The engine for the server at `url`, authenticating with `key`.
    pub fn new(url: &str, key: Option<String>, timeout: Duration) -> Result<Self, BoxError> {
        Ok(Self {
            api: Api::new("meilisearch", vec![url.to_owned()], Auth::Bearer(key), timeout)?,
            task_timeout: timeout * 6,
        })
    }

    /// The engine on a given HTTP client (a fake in tests).
    pub fn with_client(url: &str, key: Option<String>, client: HttpClient) -> Self {
        Self {
            api: Api::with_client("meilisearch", vec![url.to_owned()], Auth::Bearer(key), client),
            task_timeout: Duration::from_secs(30),
        }
    }

    /// Waits for the task an answer names (`taskUid`) to finish.
    async fn wait(&self, answer: Value) -> Result<(), SearchError> {
        let Some(uid) = answer.get("taskUid").and_then(Value::as_u64) else {
            return Ok(());
        };
        let started = std::time::Instant::now();
        let mut delay = Duration::from_millis(10);
        loop {
            let task = self
                .api
                .json(Method::GET, &format!("/tasks/{uid}"), None)
                .await?;
            match task.get("status").and_then(Value::as_str) {
                Some("succeeded") => return Ok(()),
                Some("failed" | "canceled") => {
                    let message = task
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("the task failed");
                    return Err(SearchError::Engine {
                        engine: "meilisearch",
                        message: message.to_owned(),
                    });
                }
                _ => {}
            }
            if started.elapsed() > self.task_timeout {
                return Err(SearchError::Unavailable {
                    engine: "meilisearch",
                    message: format!("task {uid} did not finish in time"),
                });
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_millis(250));
        }
    }

    async fn task(&self, method: Method, path: &str, body: Option<Body>) -> Result<(), SearchError> {
        let answer = self.api.json(method, path, body.as_ref()).await?;
        self.wait(answer).await
    }

    async fn exists(&self, index: &str) -> Result<bool, SearchError> {
        let response = self
            .api
            .send(Method::GET, &format!("/indexes/{}", encode(index)), None, &[404])
            .await?;
        Ok(response.status().as_u16() != 404)
    }
}

impl std::fmt::Debug for MeilisearchEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeilisearchEngine").finish_non_exhaustive()
    }
}

/// The settings Meilisearch receives for an index.
pub(crate) fn index_settings(settings: &IndexSettings) -> Value {
    let mut text: Vec<(&str, u8)> = settings.text_fields().collect();
    // Meilisearch ranks attributes by their order: heaviest first.
    text.sort_by(|a, b| b.1.cmp(&a.1));
    let mut filterable: Vec<&str> = settings
        .fields()
        .iter()
        .filter(|f| f.filter)
        .map(|f| f.name.as_str())
        .collect();
    filterable.push(WORKSPACE_FIELD);
    let sortable: Vec<&str> = settings
        .fields()
        .iter()
        .filter(|f| f.sort)
        .map(|f| f.name.as_str())
        .collect();
    // Synonyms work both ways.
    let mut synonyms: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (word, alternatives) in settings.synonym_map() {
        for alternative in alternatives {
            push_unique(synonyms.entry(word.clone()).or_default(), alternative);
            push_unique(synonyms.entry(alternative.clone()).or_default(), word);
        }
    }
    json!({
        "searchableAttributes": text.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        "filterableAttributes": filterable,
        "sortableAttributes": sortable,
        "synonyms": synonyms,
        "stopWords": settings.stop_word_list(),
    })
}

fn push_unique(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|i| i == item) {
        list.push(item.to_owned());
    }
}

/// The JSON Meilisearch stores for `document`.
fn document_json(document: &Document) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("id".to_owned(), Value::from(document.id()));
    if let Some(workspace) = document.workspace_id() {
        object.insert(WORKSPACE_FIELD.to_owned(), Value::from(workspace));
    }
    for (name, value) in document.fields() {
        object.insert(name.clone(), value.clone());
    }
    Value::Object(object)
}

/// A filter (normalized) in Meilisearch's filter syntax.
pub(crate) fn filter_expression(filter: &Filter) -> String {
    match filter {
        Filter::Const(true) => String::new(),
        Filter::Const(false) => format!("{WORKSPACE_FIELD} IN []"),
        Filter::All(items) => join(items, " AND "),
        Filter::Any(items) => join(items, " OR "),
        Filter::Not(inner) => format!("NOT ({})", filter_expression(inner)),
        Filter::Null(field) => format!("({field} IS NULL OR {field} NOT EXISTS)"),
        Filter::NotNull(field) => format!("({field} IS NOT NULL AND {field} EXISTS)"),
        Filter::In { field, values } => format!("{field} IN [{}]", list(values)),
        Filter::NotIn { field, values } => format!("{field} NOT IN [{}]", list(values)),
        Filter::Cmp { field, op, value } => {
            let op = match op {
                CmpOp::Eq => "=",
                CmpOp::Ne => "!=",
                CmpOp::Lt => "<",
                CmpOp::Lte => "<=",
                CmpOp::Gt => ">",
                CmpOp::Gte => ">=",
            };
            format!("{field} {op} {}", literal(value))
        }
    }
}

fn join(items: &[Filter], joiner: &str) -> String {
    let parts: Vec<String> = items.iter().map(filter_expression).collect();
    format!("({})", parts.join(joiner))
}

fn list(values: &[Value]) -> String {
    values.iter().map(literal).collect::<Vec<_>>().join(", ")
}

fn literal(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        other => other.to_string(),
    }
}

impl SearchEngine for MeilisearchEngine {
    fn name(&self) -> &'static str {
        "meilisearch"
    }

    fn prepare<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            if !self.exists(index).await? {
                self.task(
                    Method::POST,
                    "/indexes",
                    Some(Body::Json(json!({ "uid": index, "primaryKey": "id" }))),
                )
                .await?;
            }
            self.task(
                Method::PATCH,
                &format!("/indexes/{}/settings", encode(index)),
                Some(Body::Json(index_settings(settings))),
            )
            .await
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        _settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let body: Vec<Value> = documents.iter().map(document_json).collect();
            self.task(
                Method::POST,
                &format!("/indexes/{}/documents?primaryKey=id", encode(index)),
                Some(Body::Json(Value::Array(body))),
            )
            .await
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.task(
                Method::POST,
                &format!("/indexes/{}/documents/delete-batch", encode(index)),
                Some(Body::Json(json!(ids))),
            )
            .await
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.task(
                Method::DELETE,
                &format!("/indexes/{}/documents", encode(index)),
                None,
            )
            .await
        })
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let response = self
                .api
                .send(
                    Method::DELETE,
                    &format!("/indexes/{}", encode(index)),
                    None,
                    &[404],
                )
                .await?;
            if response.status().as_u16() == 404 {
                return Ok(());
            }
            let answer = self.api.decode(&response)?;
            self.wait(answer).await
        })
    }

    fn swap<'a>(
        &'a self,
        index: &'a str,
        staged: &'a str,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.task(
                Method::POST,
                "/swap-indexes",
                Some(Body::Json(json!([{ "indexes": [index, staged] }]))),
            )
            .await?;
            self.drop_index(staged).await
        })
    }

    fn search<'a>(
        &'a self,
        index: &'a str,
        query: EngineQuery<'a>,
    ) -> BoxFuture<'a, Result<EngineResults, SearchError>> {
        Box::pin(async move {
            let settings = query.settings;
            let mut retrieve = vec!["id".to_owned()];
            let title = settings.title_field().filter(|_| query.with_title);
            if let Some(title) = title {
                retrieve.push(title.to_owned());
            }
            let page = if query.limit == 0 {
                1
            } else {
                query.offset / query.limit + 1
            };
            let mut body = json!({
                "q": query.text,
                "hitsPerPage": query.limit,
                "page": page,
                "showRankingScore": true,
                "attributesToRetrieve": retrieve,
                "matchingStrategy": "all",
            });
            let filter = filter_expression(query.filter);
            if !filter.is_empty() {
                body["filter"] = Value::from(filter);
            }
            if !query.sort.is_empty() {
                let sort: Vec<String> = query
                    .sort
                    .iter()
                    .map(|s| format!("{}:{}", s.field, if s.descending { "desc" } else { "asc" }))
                    .collect();
                body["sort"] = json!(sort);
            }
            let text_fields: Vec<&str> = settings.text_fields().map(|(f, _)| f).collect();
            if query.highlight.is_some() && !query.text.trim().is_empty() {
                body["attributesToHighlight"] = json!(text_fields);
                body["highlightPreTag"] = Value::from(MARK_OPEN);
                body["highlightPostTag"] = Value::from(MARK_CLOSE);
            }
            let answer = self
                .api
                .json(
                    Method::POST,
                    &format!("/indexes/{}/search", encode(index)),
                    Some(&Body::Json(body)),
                )
                .await?;
            let total = answer
                .get("totalHits")
                .or_else(|| answer.get("estimatedTotalHits"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let hits = answer
                .get("hits")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|hit| {
                    let id = match hit.get("id")? {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let mut highlights = BTreeMap::new();
                    if let (Some(tags), Some(formatted)) = (query.highlight, hit.get("_formatted"))
                    {
                        for field in &text_fields {
                            if let Some(marked) = formatted.get(*field).and_then(field_text)
                                && let Some(h) = from_marked(&marked, tags)
                            {
                                highlights.insert((*field).to_owned(), h);
                            }
                        }
                    }
                    Some(EngineHit {
                        id,
                        score: hit
                            .get("_rankingScore")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0)
                            .clamp(0.0, 1.0),
                        highlights,
                        title: title.and_then(|t| field_text(hit.get(t)?)),
                    })
                })
                .collect();
            Ok(EngineResults { hits, total })
        })
    }

    fn count<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<u64, SearchError>> {
        Box::pin(async move {
            let response = self
                .api
                .send(
                    Method::GET,
                    &format!("/indexes/{}/stats", encode(index)),
                    None,
                    &[404],
                )
                .await?;
            if response.status().as_u16() == 404 {
                return Ok(0);
            }
            Ok(self
                .api
                .decode(&response)?
                .get("numberOfDocuments")
                .and_then(Value::as_u64)
                .unwrap_or(0))
        })
    }

    fn contains<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<bool, SearchError>> {
        Box::pin(async move {
            let response = self
                .api
                .send(
                    Method::GET,
                    &format!("/indexes/{}/documents/{}", encode(index), encode(id)),
                    None,
                    &[404],
                )
                .await?;
            Ok(response.status().as_u16() != 404)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters_render_in_meilisearch_syntax() {
        let filter = Filter::All(vec![
            Filter::eq("stage", "nego\"tiation"),
            Filter::is_in("owner_id", ["a", "b"]),
            Filter::range("amount_cents", 1_000_000..),
            Filter::null("closed_at"),
            Filter::Not(Box::new(Filter::eq("flag", true))),
        ])
        .normalized();
        assert_eq!(
            filter_expression(&filter),
            "(stage = \"nego\\\"tiation\" AND owner_id IN [\"a\", \"b\"] AND amount_cents >= 1000000 \
             AND (closed_at IS NULL OR closed_at NOT EXISTS) AND flag != true)"
        );
        assert_eq!(filter_expression(&Filter::Const(true)), "");
    }

    #[test]
    fn settings_rank_heavy_fields_first_and_make_synonyms_mutual() {
        let settings = IndexSettings::new("Contact", "contacts")
            .text("email", 2)
            .text("last_name", 3)
            .filter("status")
            .sort("created_at")
            .synonyms("bob", ["robert"]);
        assert_eq!(
            index_settings(&settings),
            json!({
                "searchableAttributes": ["last_name", "email"],
                "filterableAttributes": ["status", "_workspace"],
                "sortableAttributes": ["created_at"],
                "synonyms": {"bob": ["robert"], "robert": ["bob"]},
                "stopWords": [],
            })
        );
    }
}
