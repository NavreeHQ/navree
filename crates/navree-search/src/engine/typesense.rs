//! The `typesense` engine: a Typesense server or cluster over its REST API.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use http::Method;
use navree_http_client::HttpClient;
use navree_kernel::{BoxError, BoxFuture};
use serde_json::{Value, json};

use crate::engine::http::{Api, Auth, Body, encode};
use crate::engine::{EngineHit, EngineQuery, EngineResults, SearchEngine, every_comparison};
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
///
/// Typesense sorts only on fields declared with a type, and a sort field
/// may hold numbers, dates or text. Each sort field is therefore sorted by
/// a string field next to it (`_sort_<field>`) that holds a key ordering
/// its values as the other engines do ([`sort_key`]).
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
            .send(
                Method::GET,
                &format!("/aliases/{}", encode(index)),
                None,
                &[404],
            )
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
/// filter fields typed by Typesense from the first document, and a string
/// sort key for each sort field. (Typesense refuses to sort on a field of
/// type `auto`, and one it types as a string from a document can never be
/// sorted.)
pub(crate) fn collection_schema(name: &str, settings: &IndexSettings) -> Value {
    let mut fields = vec![json!({
        "name": WORKSPACE_FIELD, "type": "string", "optional": true, "facet": false
    })];
    for field in settings.fields() {
        if field.is_text() {
            fields.push(json!({ "name": field.name, "type": "string", "optional": true }));
        } else if field.filter {
            fields.push(json!({ "name": field.name, "type": "auto", "optional": true }));
        }
        if field.sort {
            fields.push(json!({
                "name": sort_field(&field.name), "type": "string", "optional": true, "sort": true
            }));
        }
    }
    json!({ "name": name, "fields": fields })
}

/// The field holding the sort key of sort field `field`. Index fields
/// never start with `_` ([`IndexSettings::validate`]), so it cannot clash
/// with one.
pub(crate) fn sort_field(field: &str) -> String {
    format!("_sort_{field}")
}

/// The string Typesense sorts a value of a sort field by: flags, then
/// numbers, then text, each in its own order, as the other engines compare
/// them. `None` for a missing value (sorted last in both directions).
///
/// Typesense lower-cases a sort string and drops its punctuation before it
/// compares, so the key's own parts are digits and lower-case hex: a type
/// digit, then for a number the bits of its 64-bit float, flipped so that
/// they order as the numbers do (exact up to 2^53), and for text the text
/// itself (which Typesense compares lower-cased, without punctuation).
pub(crate) fn sort_key(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(flag) => Some(if *flag { "11" } else { "10" }.to_owned()),
        Value::Number(number) => {
            let number = number.as_f64()?;
            // -0.0 and 0.0 are the same number.
            let bits = if number == 0.0 { 0 } else { number.to_bits() };
            let ordered = if bits >> 63 == 1 {
                !bits
            } else {
                bits | (1 << 63)
            };
            Some(format!("2{ordered:016x}"))
        }
        Value::String(text) => Some(format!("3{text}")),
        Value::Array(_) => Some("4".to_owned()),
        Value::Object(_) => Some("5".to_owned()),
    }
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

/// The JSON Typesense stores for `document` (null fields left out), with
/// the sort keys of the sort fields `settings` declares.
pub(crate) fn document_json(document: &Document, settings: &IndexSettings) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("id".to_owned(), Value::from(document.id()));
    if let Some(workspace) = document.workspace_id() {
        object.insert(WORKSPACE_FIELD.to_owned(), Value::from(workspace));
    }
    for (name, value) in document.fields() {
        if !value.is_null() {
            object.insert(name.clone(), value.clone());
        }
        if settings.is_sortable(name)
            && let Some(key) = sort_key(value)
        {
            object.insert(sort_field(name), Value::from(key));
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
                        &format!("/collections/{}/synonyms/{}", encode(index), encode(word)),
                        Some(&Body::Json(json!({ "synonyms": all }))),
                    )
                    .await?;
            }
            if !settings.stop_word_list().is_empty() {
                self.api
                    .json(
                        Method::PUT,
                        &format!("/stopwords/{}", encode(index)),
                        Some(&Body::Json(
                            json!({ "stopwords": settings.stop_word_list() }),
                        )),
                    )
                    .await?;
            }
            Ok(())
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let lines: Vec<String> = documents
                .iter()
                .map(|d| document_json(d, settings).to_string())
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
            let settings = self.schema_of(index).ok_or_else(|| {
                SearchError::Other(format!("the typesense index `{index}` was not prepared"))
            })?;
            let old = self.collection_of(index).await?;
            let collection = self.create_collection(index, &settings).await?;
            self.point_alias(index, &collection).await?;
            if let Some(old) = old {
                self.delete_path(&format!("/collections/{}", encode(&old)))
                    .await?;
            }
            Ok(())
        })
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            if let Some(collection) = self.collection_of(index).await? {
                self.delete_path(&format!("/aliases/{}", encode(index)))
                    .await?;
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
            self.delete_path(&format!("/aliases/{}", encode(staged)))
                .await?;
            if let Some(old) = old.filter(|old| *old != collection) {
                self.delete_path(&format!("/collections/{}", encode(&old)))
                    .await?;
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
                    text_fields
                        .iter()
                        .map(|(f, _)| *f)
                        .collect::<Vec<_>>()
                        .join(","),
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
            // Documents without a value go last in both directions, as on
            // the other engines (Typesense puts missing text first when
            // sorting down).
            let mut sort: Vec<String> = query
                .sort
                .iter()
                .map(|s| {
                    format!(
                        "{}(missing_values: last):{}",
                        sort_field(&s.field),
                        if s.descending { "desc" } else { "asc" }
                    )
                })
                .collect();
            if !text.is_empty() && sort.len() < 3 {
                sort.push("_text_match:desc".into());
            }
            if !sort.is_empty() {
                params.push(("sort_by".into(), sort.join(",")));
            }
            let per_page = query.limit.min(250);
            let page = query.offset.checked_div(per_page).map_or(1, |p| p + 1);
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
                    text_fields
                        .iter()
                        .map(|(f, _)| *f)
                        .collect::<Vec<_>>()
                        .join(","),
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

    /// Typesense doesn't index missing values, so `!=`, `NOT IN` and null
    /// tests can't match documents without a value; it compares only
    /// numbers in ranges and can't quote a backtick.
    fn filters_exactly(&self, filter: &Filter) -> bool {
        every_comparison(filter, &|f| match f {
            Filter::Null(_) | Filter::NotNull(_) | Filter::NotIn { .. } | Filter::Not(_) => false,
            Filter::Cmp { op: CmpOp::Ne, .. } => false,
            Filter::Cmp {
                op: CmpOp::Eq,
                value,
                ..
            } => literal(value).is_ok(),
            Filter::Cmp { value, .. } => value.is_number(),
            Filter::In { values, .. } => list(values).is_ok(),
            Filter::All(_) | Filter::Any(_) | Filter::Const(_) => true,
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
    fn schemas_type_text_as_strings_and_sort_on_typed_keys() {
        let settings = IndexSettings::new("Deal", "deals")
            .text("title", 3)
            .filter("stage")
            .filter("amount")
            .sort("amount")
            .sort("closes_on");
        let schema = collection_schema("crm_deals_1", &settings);
        assert_eq!(schema["name"], "crm_deals_1");
        assert_eq!(
            schema["fields"],
            json!([
                { "name": "_workspace", "type": "string", "optional": true, "facet": false },
                { "name": "title", "type": "string", "optional": true },
                { "name": "stage", "type": "auto", "optional": true },
                { "name": "amount", "type": "auto", "optional": true },
                { "name": "_sort_amount", "type": "string", "optional": true, "sort": true },
                { "name": "_sort_closes_on", "type": "string", "optional": true, "sort": true },
            ])
        );
        // Typesense 30 refuses `sort` on a field of type `auto`.
        for field in schema["fields"].as_array().unwrap() {
            assert!(
                !(field["type"] == "auto" && field.get("sort").is_some()),
                "{field}"
            );
        }
    }

    #[test]
    fn sort_keys_order_values_as_the_other_engines_do() {
        let values = [
            json!(false),
            json!(true),
            json!(-1.0e300),
            json!(-12_000),
            json!(-1.5),
            json!(0),
            json!(0.25),
            json!(900),
            json!(5_000),
            json!(1.0e300),
            json!("2025-12-31"),
            json!("2026-01-15"),
            json!("2026-11-05T06:18:21.000000Z"),
            json!("2026-11-05T06:18:22.000000Z"),
        ];
        let keys: Vec<String> = values.iter().map(|v| sort_key(v).unwrap()).collect();
        for pair in keys.windows(2) {
            assert!(pair[0] < pair[1], "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(sort_key(&json!(-0.0)), sort_key(&json!(0)));
        assert_eq!(sort_key(&json!(7)), sort_key(&json!(7.0)));
        assert_eq!(sort_key(&Value::Null), None);
        // Typesense compares lower-cased text without punctuation: the
        // keys of flags and numbers must not change under that.
        for key in &keys[..10] {
            assert!(
                key.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{key}"
            );
        }
    }

    #[test]
    fn documents_carry_the_sort_keys_of_their_sort_fields() {
        let settings = IndexSettings::new("Deal", "deals")
            .text("title", 3)
            .filter("amount")
            .sort("amount")
            .sort("closes_on");
        let document = Document::new("7")
            .field("title", "Renewal")
            .field("amount", 900)
            .field("closes_on", None::<String>)
            .workspace("w1");
        assert_eq!(
            document_json(&document, &settings),
            json!({
                "id": "7",
                "_workspace": "w1",
                "title": "Renewal",
                "amount": 900,
                "_sort_amount": sort_key(&json!(900)).unwrap(),
            })
        );
    }

    #[test]
    fn filters_on_missing_values_are_not_exact() {
        let engine = TypesenseEngine::new(
            "http://127.0.0.1:1",
            &[],
            "k".into(),
            Duration::from_secs(1),
        )
        .unwrap();
        let exact = |filter: Filter| engine.filters_exactly(&filter.normalized());
        assert!(exact(Filter::All(vec![
            Filter::eq("stage", "won"),
            Filter::is_in("owner", ["a", "b"]),
            Filter::range("amount", 1..),
        ])));
        assert!(exact(Filter::Const(true)));
        // Typesense skips documents without a value for these.
        assert!(!exact(Filter::ne("stage", "won")));
        assert!(!exact(Filter::Not(Box::new(Filter::is_in("owner", ["a"])))));
        assert!(!exact(Filter::null("stage")));
        assert!(!exact(Filter::Any(vec![
            Filter::eq("stage", "won"),
            Filter::Not(Box::new(Filter::range("amount", 1..))),
        ])));
        // Ranges compare numbers only; backticks can't be quoted.
        assert!(!exact(Filter::range("stage", "m"..)));
        assert!(!exact(Filter::eq("stage", "a`b")));
    }
}
