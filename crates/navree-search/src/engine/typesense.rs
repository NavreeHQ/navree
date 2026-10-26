//! The `typesense` engine: a Typesense server or cluster over its REST API.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};
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

/// A Typesense server or cluster. Every index is a collection
/// (`<index>_<millis>`) behind an alias named like the index, so a fresh
/// import (and a flush) builds a new collection and moves the alias in one
/// step. Requests fail over between `url` and `nodes`.
///
/// Typesense does not index null values: `filter_null` is not supported,
/// and `filter_not` does not match records without a value.
pub struct TypesenseEngine {
    api: Api,
    /// The settings each index was prepared with (new collections need
    /// the schema).
    schemas: Mutex<HashMap<String, IndexSettings>>,
}

impl TypesenseEngine {
    /// The engine for the nodes `url` and `nodes`, with API key `key`.
    pub fn new(
        url: &str,
        nodes: &[String],
        key: String,
        timeout: Duration,
    ) -> Result<Self, BoxError> {
        let mut all = vec![url.to_owned()];
        all.extend(nodes.iter().cloned());
        Ok(Self {
            api: Api::new(
                "typesense",
                all,
                Auth::Header("x-typesense-api-key", Some(key)),
                timeout,
            )?,
            schemas: Mutex::new(HashMap::new()),
        })
    }

    /// The engine on a given HTTP client (a fake in tests).
    pub fn with_client(url: &str, key: String, client: HttpClient) -> Self {
        Self {
            api: Api::with_client(
                "typesense",
                vec![url.to_owned()],
                Auth::Header("x-typesense-api-key", Some(key)),
                client,
            ),
            schemas: Mutex::new(HashMap::new()),
        }
    }

    /// The collection behind alias `index`, if any.
    async fn collection_of(&self, index: &str) -> Result<Option<String>, SearchError> {
        let response = self
            .api
            .send(Method::GET, &format!("/aliases/{}", encode(index)), None, &[404])
            .await?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        Ok(self
            .api
            .decode(&response)?
            .get("collection_name")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }

    /// Creates a new collection for `index` with `settings`; returns its
    /// name.
    async fn create_collection(
        &self,
        index: &str,
        settings: &IndexSettings,
    ) -> Result<String, SearchError> {
        let name = format!("{index}_{}", chrono::Utc::now().timestamp_millis());
        self.api
            .json(
                Method::POST,
                "/collections",
                Some(&Body::Json(collection_schema(&name, settings))),
            )
            .await?;
        Ok(name)
    }

    async fn point_alias(&self, index: &str, collection: &str) -> Result<(), SearchError> {
        self.api
            .json(
                Method::PUT,
                &format!("/aliases/{}", encode(index)),
                Some(&Body::Json(json!({ "collection_name": collection }))),
            )
            .await?;
        Ok(())
    }

    async fn delete_path(&self, path: &str) -> Result<(), SearchError> {
        self.api.send(Method::DELETE, path, None, &[404]).await?;
        Ok(())
    }

    fn schema_of(&self, index: &str) -> Option<IndexSettings> {
        self.schemas
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(index)
            .cloned()
    }
}

impl std::fmt::Debug for TypesenseEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypesenseEngine").finish_non_exhaustive()
    }
}

/// The collection schema for `settings`: text fields as optional strings,
/// filter and sort fields typed by Typesense from the first document.
pub(crate) fn collection_schema(name: &str, settings: &IndexSettings) -> Value {
    let mut fields = vec![json!({
        "name": WORKSPACE_FIELD, "type": "string", "optional": true, "facet": false
    })];
    for field in settings.fields() {
        if field.is_text() {
            fields.push(json!({
                "name": field.name, "type": "string", "optional": true, "sort": field.sort
            }));
        } else if field.filter || field.sort {
            fields.push(json!({
                "name": field.name, "type": "auto", "optional": true, "sort": field.sort
            }));
        }
    }
    json!({ "name": name, "fields": fields })
}

/// A filter (normalized) in Typesense's `filter_by` syntax.
pub(crate) fn filter_by(filter: &Filter) -> Result<String, SearchError> {
    let unsupported = |feature: &str| SearchError::Unsupported {
        engine: "typesense",
        feature: feature.to_owned(),
    };
    Ok(match filter {
        Filter::Const(true) => String::new(),
        Filter::Const(false) => format!("{WORKSPACE_FIELD}:=[]"),
        Filter::All(items) | Filter::Any(items) => {
            let joiner = if matches!(filter, Filter::All(_)) {
                " && "
            } else {
                " || "
            };
            let parts = items.iter().map(filter_by).collect::<Result<Vec<_>, _>>()?;
            format!("({})", parts.join(joiner))
        }
        Filter::Not(_) => return Err(unsupported("negated filter groups")),
        Filter::Null(_) | Filter::NotNull(_) => {
            return Err(unsupported("filters on missing values (filter_null)"));
        }
        Filter::In { field, values } => format!("{field}:=[{}]", list(values)?),
        Filter::NotIn { field, values } => format!("{field}:!=[{}]", list(values)?),
        Filter::Cmp { field, op, value } => {
            let op = match op {
                CmpOp::Eq => "=",
                CmpOp::Ne => "!=",
                CmpOp::Lt => "<",
                CmpOp::Lte => "<=",
                CmpOp::Gt => ">",
                CmpOp::Gte => ">=",
            };
            format!("{field}:{op}{}", literal(value)?)
        }
    })
}

fn list(values: &[Value]) -> Result<String, SearchError> {
    Ok(values
        .iter()
        .map(literal)
        .collect::<Result<Vec<_>, _>>()?
        .join(","))
}

fn literal(value: &Value) -> Result<String, SearchError> {
    match value {
        Value::String(s) if s.contains('`') => Err(SearchError::Unsupported {
            engine: "typesense",
            feature: "filter values containing a backtick".to_owned(),
        }),
        Value::String(s) => Ok(format!("`{s}`")),
        other => Ok(other.to_string()),
    }
}

/// The JSON Typesense stores for `document` (null fields left out).
fn document_json(document: &Document) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("id".to_owned(), Value::from(document.id()));
    if let Some(workspace) = document.workspace_id() {
        object.insert(WORKSPACE_FIELD.to_owned(), Value::from(workspace));
    }
    for (name, value) in document.fields() {
        if !value.is_null() {
            object.insert(name.clone(), value.clone());
        }
    }
    Value::Object(object)
}

impl SearchEngine for TypesenseEngine {
    fn name(&self) -> &'static str {
        "typesense"
    }

    fn prepare<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.schemas
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(index.to_owned(), settings.clone());
            if self.collection_of(index).await?.is_none() {
                let collection = self.create_collection(index, settings).await?;
                self.point_alias(index, &collection).await?;
            }
            for (word, alternatives) in settings.synonym_map() {
                let mut all = vec![word.clone()];
                all.extend(alternatives.iter().cloned());
                self.api
                    .json(
                        Method::PUT,
                        &format!(
                            "/collections/{}/synonyms/{}",
                            encode(index),
                            encode(word)
                        ),
                        Some(&Body::Json(json!({ "synonyms": all }))),
                    )
                    .await?;
            }
            if !settings.stop_word_list().is_empty() {
                self.api
                    .json(
                        Method::PUT,
                        &format!("/stopwords/{}", encode(index)),
                        Some(&Body::Json(json!({ "stopwords": settings.stop_word_list() }))),
                    )
                    .await?;
            }
            Ok(())
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        _settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let lines: Vec<String> = documents
                .iter()
                .map(|d| document_json(d).to_string())
                .collect();
            let response = self
                .api
                .send(
                    Method::POST,
                    &format!(
                        "/collections/{}/documents/import?action=upsert",
                        encode(index)
                    ),
                    Some(&Body::Text(lines.join("\n"), "text/plain")),
                    &[],
                )
                .await?;
            for line in response.text().lines().filter(|l| !l.trim().is_empty()) {
                let result: Value = serde_json::from_str(line).unwrap_or(Value::Null);
                if result.get("success").and_then(Value::as_bool) == Some(false) {
                    let message = result
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("a document was refused");
                    return Err(SearchError::Engine {
                        engine: "typesense",
                        message: message.to_owned(),
                    });
                }
            }
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            for chunk in ids.chunks(100) {
                let values: Vec<Value> = chunk.iter().map(|id| Value::from(id.as_str())).collect();
                let filter = format!("id:[{}]", list(&values)?);
                self.api
                    .send(
                        Method::DELETE,
                        &format!(
                            "/collections/{}/documents?filter_by={}",
                            encode(index),
                            encode(&filter)
                        ),
                        None,
                        &[404],
                    )
                    .await?;
            }
            Ok(())
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let settings = self.schema_of(index).ok_or_else(|| SearchError::Other(format!(
                "the typesense index `{index}` was not prepared"
            )))?;
            let old = self.collection_of(index).await?;
            let collection = self.create_collection(index, &settings).await?;
            self.point_alias(index, &collection).await?;
            if let Some(old) = old {
                self.delete_path(&format!("/collections/{}", encode(&old))).await?;
            }
            Ok(())
        })
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            if let Some(collection) = self.collection_of(index).await? {
                self.delete_path(&format!("/aliases/{}", encode(index))).await?;
                self.delete_path(&format!("/collections/{}", encode(&collection)))
                    .await?;
            }
            Ok(())
        })
    }

    fn swap<'a>(
        &'a self,
        index: &'a str,
        staged: &'a str,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let Some(collection) = self.collection_of(staged).await? else {
                return Err(SearchError::Other(format!(
                    "the typesense index `{staged}` does not exist"
                )));
            };
            let old = self.collection_of(index).await?;
            self.point_alias(index, &collection).await?;
            self.delete_path(&format!("/aliases/{}", encode(staged))).await?;
            if let Some(old) = old.filter(|old| *old != collection) {
                self.delete_path(&format!("/collections/{}", encode(&old))).await?;
            }
            Ok(())
        })
    }

    fn search<'a>(
        &'a self,
        index: &'a str,
        query: EngineQuery<'a>,
    ) -> BoxFuture<'a, Result<EngineResults, SearchError>> {
        Box::pin(async move {
            let settings = query.settings;
            let text_fields: Vec<(&str, u8)> = settings.text_fields().collect();
            let text = query.text.trim();
            let mut params: Vec<(String, String)> = Vec::new();
            if text.is_empty() || text_fields.is_empty() {
                params.push(("q".into(), "*".into()));
            } else {
                params.push(("q".into(), text.to_owned()));
                params.push((
                    "query_by".into(),
                    text_fields.iter().map(|(f, _)| *f).collect::<Vec<_>>().join(","),
                ));
                params.push((
                    "query_by_weights".into(),
                    text_fields
                        .iter()
                        .map(|(_, b)| b.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                ));
                params.push(("num_typos".into(), "2".into()));
                params.push(("prefix".into(), "true".into()));
            }
            let filter = filter_by(query.filter)?;
            if !filter.is_empty() {
                params.push(("filter_by".into(), filter));
            }
            let mut sort: Vec<String> = query
                .sort
                .iter()
                .map(|s| format!("{}:{}", s.field, if s.descending { "desc" } else { "asc" }))
                .collect();
            if !text.is_empty() && sort.len() < 3 {
                sort.push("_text_match:desc".into());
            }
            if !sort.is_empty() {
                params.push(("sort_by".into(), sort.join(",")));
            }
            let per_page = query.limit.min(250);
            let page = if per_page == 0 { 1 } else { query.offset / per_page + 1 };
            params.push(("page".into(), page.to_string()));
            params.push(("per_page".into(), per_page.to_string()));
            let title = settings.title_field().filter(|_| query.with_title);
            let mut include = vec!["id".to_owned()];
            if let Some(title) = title {
                include.push(title.to_owned());
            }
            params.push(("include_fields".into(), include.join(",")));
            if query.highlight.is_some() && !text.is_empty() {
                params.push((
                    "highlight_full_fields".into(),
                    text_fields.iter().map(|(f, _)| *f).collect::<Vec<_>>().join(","),
                ));
                params.push(("highlight_start_tag".into(), MARK_OPEN.into()));
                params.push(("highlight_end_tag".into(), MARK_CLOSE.into()));
            }
            if !settings.stop_word_list().is_empty() {
                params.push(("stopwords".into(), index.to_owned()));
            }
            let query_string = params
                .iter()
                .map(|(k, v)| format!("{k}={}", encode(v)))
                .collect::<Vec<_>>()
                .join("&");
            let answer = self
                .api
                .json(
                    Method::GET,
                    &format!(
                        "/collections/{}/documents/search?{query_string}",
                        encode(index)
                    ),
                    None,
                )
                .await?;
            let total = answer.get("found").and_then(Value::as_u64).unwrap_or(0);
            let raw = answer
                .get("hits")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let best = raw
                .iter()
                .filter_map(|h| h.get("text_match").and_then(Value::as_f64))
                .fold(0.0, f64::max);
            let hits = raw
                .iter()
                .filter_map(|hit| {
                    let document = hit.get("document")?;
                    let id = field_text(document.get("id")?)?;
                    let text_match = hit.get("text_match").and_then(Value::as_f64).unwrap_or(0.0);
                    let score = if best > 0.0 { text_match / best } else { 0.0 };
                    let mut highlights = BTreeMap::new();
                    if let Some(tags) = query.highlight {
                        for (field, _) in &text_fields {
                            let marked = hit
                                .pointer(&format!("/highlight/{field}/value"))
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .or_else(|| legacy_highlight(hit, field));
                            if let Some(h) = marked.and_then(|m| from_marked(&m, tags)) {
                                highlights.insert((*field).to_owned(), h);
                            }
                        }
                    }
                    Some(EngineHit {
                        id,
                        score: score.clamp(0.0, 1.0),
                        highlights,
                        title: title.and_then(|t| field_text(document.get(t)?)),
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
                    &format!("/collections/{}", encode(index)),
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
                .get("num_documents")
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
                    &format!("/collections/{}/documents/{}", encode(index), encode(id)),
                    None,
                    &[404],
                )
                .await?;
            Ok(response.status().as_u16() != 404)
        })
    }
}

/// The highlight of `field` in the pre-0.25 `highlights` array.
fn legacy_highlight(hit: &Value, field: &str) -> Option<String> {
    hit.get("highlights")?
        .as_array()?
        .iter()
        .find(|h| h.get("field").and_then(Value::as_str) == Some(field))
        .and_then(|h| h.get("value").or_else(|| h.get("snippet")))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_render_in_typesense_syntax() {
        let filter = Filter::All(vec![
            Filter::eq("stage", "negotiation"),
            Filter::is_in("owner_id", ["a", "b"]),
            Filter::range("amount_cents", 1_000_000..),
            Filter::Any(vec![Filter::eq("flag", true), Filter::ne("stage", "lost")]),
        ])
        .normalized();
        assert_eq!(
            filter_by(&filter).unwrap(),
            "(stage:=`negotiation` && owner_id:=[`a`,`b`] && amount_cents:>=1000000 && (flag:=true || stage:!=`lost`))"
        );
        assert!(matches!(
            filter_by(&Filter::null("x").normalized()),
            Err(SearchError::Unsupported { .. })
        ));
        assert!(filter_by(&Filter::eq("x", "a`b").normalized()).is_err());
    }

    #[test]
    fn schemas_type_text_as_strings() {
        let settings = IndexSettings::new("Deal", "deals")
            .text("title", 3)
            .filter("stage")
            .sort("amount");
        let schema = collection_schema("crm_deals_1", &settings);
        assert_eq!(schema["name"], "crm_deals_1");
        let fields = schema["fields"].as_array().unwrap();
        assert_eq!(fields[0]["name"], "_workspace");
        assert_eq!(fields[1]["type"], "string");
        assert_eq!(fields[2]["type"], "auto");
        assert_eq!(fields[3]["sort"], true);
    }
}
