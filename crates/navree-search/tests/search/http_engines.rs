//! The REST engines (Meilisearch, Typesense) and the Qdrant vector store
//! against local mock servers: the requests they send and how they read
//! the answers.

use std::sync::Arc;
use std::time::Duration;

use navree_access::{Accessible, READ, Read, Rules, field};
use navree_db::{Db, PageQuery};
use navree_search::{
    Document, EngineQuery, Filter, HighlightTags, IndexSettings, MeilisearchEngine, Metric,
    QdrantStore, Search, SearchEngine, SearchError, SortBy, TypesenseEngine, VectorRecord,
    VectorStore,
};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::support::{self, Contact, insert_contacts, new_contact, settle};

const OPEN: &str = "\u{E000}";
const CLOSE: &str = "\u{E001}";

fn settings() -> IndexSettings {
    IndexSettings::new("Contact", "contacts")
        .text("last_name", 3)
        .text("email", 2)
        .filter("status")
        .sort("score")
}

/// A port nothing listens on.
/// A URL nothing listens on. A port taken from `bind("127.0.0.1:0")` and
/// released again can be bound by another test's mock server before the
/// request is sent; port 1 is never bound by the tests or their services.
fn closed_port_url() -> String {
    "http://127.0.0.1:1".to_owned()
}

async fn mount_tasks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path_regex(r"^/tasks/\d+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "succeeded" })))
        .mount(server)
        .await;
}

fn task(uid: u64) -> ResponseTemplate {
    ResponseTemplate::new(202).set_body_json(json!({ "taskUid": uid, "status": "enqueued" }))
}

fn body(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

async fn requests_to(server: &MockServer, verb: &str, url_path: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == verb && r.url.path() == url_path)
        .collect()
}

#[tokio::test]
async fn meilisearch_creates_indexes_and_pushes_settings() {
    let server = MockServer::start().await;
    mount_tasks(&server).await;
    Mock::given(method("GET"))
        .and(path("/indexes/crm_contacts"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "not found"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/indexes"))
        .and(header("authorization", "Bearer masterKey"))
        .and(body_partial_json(
            json!({ "uid": "crm_contacts", "primaryKey": "id" }),
        ))
        .respond_with(task(1))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/indexes/crm_contacts/settings"))
        .respond_with(task(2))
        .expect(1)
        .mount(&server)
        .await;
    let engine = MeilisearchEngine::new(
        &server.uri(),
        Some("masterKey".into()),
        Duration::from_secs(5),
    )
    .unwrap();
    engine.prepare("crm_contacts", &settings()).await.unwrap();
    let patch = &requests_to(&server, "PATCH", "/indexes/crm_contacts/settings").await[0];
    assert_eq!(
        body(patch),
        json!({
            "searchableAttributes": ["last_name", "email"],
            "filterableAttributes": ["status", "_workspace"],
            "sortableAttributes": ["score"],
            "synonyms": {},
            "stopWords": [],
        })
    );
}

#[tokio::test]
async fn meilisearch_drop_index_of_a_missing_index_succeeds() {
    // Meilisearch v1 answers the delete of a missing index with 202 and a
    // task that later fails with `index_not_found`: the index is gone, as
    // wanted. Any other failure still fails the drop.
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/indexes/missing"))
        .respond_with(task(3))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tasks/3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failed",
            "error": { "message": "Index `missing` not found.", "code": "index_not_found" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/indexes/locked"))
        .respond_with(task(4))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tasks/4"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failed",
            "error": { "message": "disk full", "code": "no_space_left_on_device" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/indexes/gone"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let engine = MeilisearchEngine::new(&server.uri(), None, Duration::from_secs(5)).unwrap();
    engine.drop_index("missing").await.unwrap();
    engine.drop_index("gone").await.unwrap();
    let error = engine.drop_index("locked").await.unwrap_err();
    assert!(matches!(error, SearchError::Engine { ref message, .. } if message == "disk full"));
}

#[tokio::test]
async fn meilisearch_writes_wait_for_their_task() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/indexes/crm_contacts/documents"))
        .and(query_param("primaryKey", "id"))
        .respond_with(task(7))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tasks/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failed", "error": { "message": "invalid document id" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/indexes/crm_contacts/documents/delete-batch"))
        .respond_with(task(8))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tasks/8"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "succeeded" })))
        .mount(&server)
        .await;
    let engine = MeilisearchEngine::new(&server.uri(), None, Duration::from_secs(5)).unwrap();
    let error = engine
        .upsert(
            "crm_contacts",
            &settings(),
            vec![
                Document::new("a1")
                    .field("last_name", "Lovelace")
                    .workspace("w1"),
            ],
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, SearchError::Engine { ref message, .. } if message == "invalid document id")
    );
    let sent = &requests_to(&server, "POST", "/indexes/crm_contacts/documents").await[0];
    assert_eq!(
        body(sent),
        json!([{ "id": "a1", "_workspace": "w1", "last_name": "Lovelace" }])
    );
    engine
        .delete("crm_contacts", &["a1".to_owned()])
        .await
        .unwrap();
}

#[tokio::test]
async fn meilisearch_searches_with_filters_sorts_and_highlights() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/indexes/crm_contacts/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hits": [
                { "id": "a1", "_rankingScore": 0.94, "last_name": "Lovelace",
                  "_formatted": { "last_name": format!("{OPEN}Lovelace{CLOSE} <3"), "email": "x" } },
                { "id": 2, "_rankingScore": 0.5 }
            ],
            "totalHits": 12, "page": 2, "hitsPerPage": 2
        })))
        .mount(&server)
        .await;
    let engine = MeilisearchEngine::new(&server.uri(), None, Duration::from_secs(5)).unwrap();
    let settings = settings();
    let filter = Filter::All(vec![
        Filter::eq("status", "customer"),
        Filter::eq("_workspace", "w1"),
    ])
    .normalized();
    let tags = HighlightTags::default();
    let results = engine
        .search(
            "crm_contacts",
            EngineQuery {
                settings: &settings,
                text: "lovlace",
                filter: &filter,
                sort: &[SortBy {
                    field: "score".into(),
                    descending: true,
                }],
                offset: 2,
                limit: 2,
                highlight: Some(&tags),
                with_title: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(results.total, 12);
    assert_eq!(results.hits[0].id, "a1");
    assert_eq!(results.hits[0].score, 0.94);
    assert_eq!(
        results.hits[0]
            .highlights
            .get("last_name")
            .map(String::as_str),
        Some("<mark>Lovelace</mark> &lt;3")
    );
    assert!(!results.hits[0].highlights.contains_key("email"));
    assert_eq!(results.hits[0].title.as_deref(), Some("Lovelace"));
    assert_eq!(results.hits[1].id, "2");
    let sent = body(&requests_to(&server, "POST", "/indexes/crm_contacts/search").await[0]);
    assert_eq!(sent["q"], "lovlace");
    assert_eq!(
        sent["filter"],
        "(status = \"customer\" AND _workspace = \"w1\")"
    );
    assert_eq!(sent["sort"], json!(["score:desc"]));
    assert_eq!(
        (sent["page"].clone(), sent["hitsPerPage"].clone()),
        (json!(2), json!(2))
    );
    assert_eq!(sent["attributesToRetrieve"], json!(["id", "last_name"]));
    assert_eq!(sent["highlightPreTag"], OPEN);
}

#[tokio::test]
async fn meilisearch_outages_are_unavailable() {
    let engine = MeilisearchEngine::new(&closed_port_url(), None, Duration::from_secs(2)).unwrap();
    let error = engine.count("crm_contacts").await.unwrap_err();
    assert!(error.is_unavailable(), "{error}");
    assert_eq!(error.to_app_error().status.as_u16(), 503);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503).set_body_string("overloaded"))
        .mount(&server)
        .await;
    let engine = MeilisearchEngine::new(&server.uri(), None, Duration::from_secs(2)).unwrap();
    assert!(
        engine
            .count("crm_contacts")
            .await
            .unwrap_err()
            .is_unavailable()
    );
}

/// A Meilisearch server for an app: `contacts` exists, every other index
/// is created, settings and document writes succeed.
async fn mount_meilisearch_app(server: &MockServer) {
    mount_tasks(server).await;
    Mock::given(method("GET"))
        .and(path("/indexes/contacts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "uid": "contacts" })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/indexes/[a-z]+$"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/indexes"))
        .respond_with(task(1))
        .mount(server)
        .await;
    Mock::given(method("PATCH"))
        .and(path_regex(r"^/indexes/[a-z]+/settings$"))
        .respond_with(task(2))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/indexes/contacts/documents"))
        .respond_with(task(3))
        .mount(server)
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_app_on_meilisearch_syncs_and_searches() {
    let server = MockServer::start().await;
    mount_meilisearch_app(&server).await;
    let app = support::builder("meilisearch", "inline")
        .config("search.url", server.uri())
        .config("search.key", "masterKey")
        .build()
        .await;
    let db = app.get::<Db>().await;
    let rows = insert_contacts(&db, vec![new_contact("Ada", "Lovelace")]).await;
    settle(&app).await;
    let sent = requests_to(&server, "POST", "/indexes/contacts/documents").await;
    assert_eq!(sent.len(), 1);
    assert_eq!(body(&sent[0])[0]["last_name"], "Lovelace");
    assert_eq!(body(&sent[0])[0]["id"], rows[0].id.to_string());

    Mock::given(method("POST"))
        .and(path("/indexes/contacts/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hits": [{ "id": rows[0].id.to_string(), "_rankingScore": 0.9 }, { "id": "gone", "_rankingScore": 0.1 }],
            "totalHits": 2
        })))
        .mount(&server)
        .await;
    let search = app.get::<Search>().await;
    let page = search
        .query::<Contact>("lovelace")
        .paginate(navree_db::PageQuery::default())
        .await
        .unwrap();
    assert_eq!(
        page.data.len(),
        1,
        "records gone from the database are dropped"
    );
    assert_eq!(page.data[0].first_name, "Ada");
}

/// Meilisearch compares only numbers in ranges, so an access rule with a
/// range on a text column isn't sent to it: the loaded rows are filtered
/// with the SQL condition instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn access_rules_meilisearch_cannot_run_filter_the_loaded_rows() {
    let server = MockServer::start().await;
    mount_meilisearch_app(&server).await;
    let app = support::builder("meilisearch", "off")
        .config("search.url", server.uri())
        .build()
        .await;
    let db = app.get::<Db>().await;
    let mut grace = new_contact("Grace", "Hopper");
    grace.status = "customer".into();
    let rows = insert_contacts(&db, vec![new_contact("Ada", "Lovelace"), grace]).await;
    Mock::given(method("POST"))
        .and(path("/indexes/contacts/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "hits": [
                { "id": rows[0].id.to_string(), "_rankingScore": 0.9 },
                { "id": rows[1].id.to_string(), "_rankingScore": 0.8 }
            ],
            "totalHits": 2
        })))
        .mount(&server)
        .await;
    let search = app.get::<Search>().await;
    let scope = |rules: Rules| Accessible::<Contact, Read>::new(Arc::new(rules.build()));

    // `status > "d"`: Ada (a lead) only.
    let mut rules = Rules::new();
    rules.can(READ, "Contact").when(field("status").gt("d"));
    let page = search
        .query::<Contact>("")
        .accessible(scope(rules))
        .paginate(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(support::ids(&page), [rows[0].id.to_string()]);
    let sent = requests_to(&server, "POST", "/indexes/contacts/search").await;
    assert!(
        !body(&sent[0])["filter"].to_string().contains("status"),
        "the range on a string stays out of the engine: {}",
        body(&sent[0])
    );

    // `status = "customer"` runs in the engine.
    let mut rules = Rules::new();
    rules
        .can(READ, "Contact")
        .when(field("status").eq("customer"));
    search
        .query::<Contact>("")
        .accessible(scope(rules))
        .paginate(PageQuery::default())
        .await
        .unwrap();
    let sent = requests_to(&server, "POST", "/indexes/contacts/search").await;
    assert!(
        body(&sent[1])["filter"].to_string().contains("status"),
        "{}",
        body(&sent[1])
    );
}

async fn mount_typesense_collection(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/aliases/crm_deals"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .up_to_n_times(1)
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/collections"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"name": "crm_deals_1"})))
        .mount(server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/aliases/crm_deals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "crm_deals"})))
        .mount(server)
        .await;
}

fn deal_settings() -> IndexSettings {
    IndexSettings::new("Deal", "deals")
        .text("title", 3)
        .text("company_name", 2)
        .filter("stage")
        .sort("amount")
        .synonyms("acme", ["acme corp"])
}

#[tokio::test]
async fn typesense_builds_collections_behind_aliases() {
    let server = MockServer::start().await;
    mount_typesense_collection(&server).await;
    Mock::given(method("PUT"))
        .and(path("/collections/crm_deals/synonyms/acme"))
        .and(body_partial_json(
            json!({"synonyms": ["acme", "acme corp"]}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/collections/crm_deals/documents/import"))
        .and(query_param("action", "upsert"))
        .and(header("x-typesense-api-key", "xyz"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("{\"success\":true}\n{\"success\":true}"),
        )
        .mount(&server)
        .await;
    let engine =
        TypesenseEngine::new(&server.uri(), &[], "xyz".into(), Duration::from_secs(5)).unwrap();
    engine.prepare("crm_deals", &deal_settings()).await.unwrap();
    let created = body(&requests_to(&server, "POST", "/collections").await[0]);
    assert!(created["name"].as_str().unwrap().starts_with("crm_deals_"));
    let alias = body(&requests_to(&server, "PUT", "/aliases/crm_deals").await[0]);
    assert_eq!(alias["collection_name"], created["name"]);

    engine
        .upsert(
            "crm_deals",
            &deal_settings(),
            vec![
                Document::new("1")
                    .field("title", "Renewal")
                    .field("company_name", None::<String>),
                Document::new("2").field("title", "Pilot").workspace("w1"),
            ],
        )
        .await
        .unwrap();
    let import = &requests_to(&server, "POST", "/collections/crm_deals/documents/import").await[0];
    let lines: Vec<Value> = String::from_utf8_lossy(&import.body)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines[0],
        json!({"id": "1", "title": "Renewal"}),
        "nulls are left out"
    );
    assert_eq!(
        lines[1],
        json!({"id": "2", "title": "Pilot", "_workspace": "w1"})
    );
}

#[tokio::test]
async fn typesense_reports_refused_documents() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/collections/crm_deals/documents/import"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "{\"success\":true}\n{\"success\":false,\"error\":\"Field `amount` must be an int64.\"}",
        ))
        .mount(&server)
        .await;
    let engine =
        TypesenseEngine::new(&server.uri(), &[], "xyz".into(), Duration::from_secs(5)).unwrap();
    let error = engine
        .upsert(
            "crm_deals",
            &deal_settings(),
            vec![Document::new("1"), Document::new("2")],
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("must be an int64"), "{error}");
}

#[tokio::test]
async fn typesense_searches_and_fails_over_between_nodes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/collections/crm_deals/documents/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "found": 3,
            "hits": [
                { "document": { "id": "1", "title": "Acme renewal" }, "text_match": 1000,
                  "highlight": { "title": { "value": format!("{OPEN}Acme{CLOSE} renewal") } } },
                { "document": { "id": "2" }, "text_match": 500,
                  "highlights": [ { "field": "company_name", "value": format!("{OPEN}Acme{CLOSE} Corp") } ] }
            ]
        })))
        .mount(&server)
        .await;
    let engine = TypesenseEngine::new(
        &closed_port_url(),
        &[server.uri()],
        "xyz".into(),
        Duration::from_secs(2),
    )
    .unwrap();
    let settings = deal_settings();
    let filter = Filter::All(vec![
        Filter::eq("stage", "won"),
        Filter::range("amount", 100..),
    ])
    .normalized();
    let tags = HighlightTags::default();
    let results = engine
        .search(
            "crm_deals",
            EngineQuery {
                settings: &settings,
                text: "acme",
                filter: &filter,
                sort: &[SortBy {
                    field: "amount".into(),
                    descending: false,
                }],
                offset: 0,
                limit: 2,
                highlight: Some(&tags),
                with_title: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(results.total, 3);
    assert_eq!(results.hits[0].score, 1.0);
    assert_eq!(results.hits[1].score, 0.5);
    assert_eq!(
        results.hits[0].highlights["title"],
        "<mark>Acme</mark> renewal"
    );
    assert_eq!(
        results.hits[1].highlights["company_name"],
        "<mark>Acme</mark> Corp"
    );
    assert_eq!(results.hits[0].title.as_deref(), Some("Acme renewal"));
    let sent = &requests_to(&server, "GET", "/collections/crm_deals/documents/search").await[0];
    let params: std::collections::HashMap<String, String> =
        sent.url.query_pairs().into_owned().collect();
    assert_eq!(params["q"], "acme");
    assert_eq!(params["query_by"], "title,company_name");
    assert_eq!(params["query_by_weights"], "3,2");
    assert_eq!(params["filter_by"], "(stage:=`won` && amount:>=100)");
    // Sorted by the typed key next to the field, missing values last.
    assert_eq!(
        params["sort_by"],
        "_sort_amount(missing_values: last):asc,_text_match:desc"
    );
    assert_eq!(
        (params["page"].as_str(), params["per_page"].as_str()),
        ("1", "2")
    );
    assert_eq!(params["include_fields"], "id,title");

    let error = engine
        .search(
            "crm_deals",
            EngineQuery {
                settings: &settings,
                text: "",
                filter: &Filter::null("stage").normalized(),
                sort: &[],
                offset: 0,
                limit: 2,
                highlight: None,
                with_title: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::Unsupported { .. }));
    assert_eq!(error.to_app_error().status.as_u16(), 400);
}

#[tokio::test]
async fn typesense_swaps_aliases_on_fresh_imports() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/aliases/crm_deals__fresh"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"collection_name": "crm_deals_2"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/aliases/crm_deals"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"collection_name": "crm_deals_1"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/aliases/crm_deals"))
        .and(body_partial_json(json!({"collection_name": "crm_deals_2"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/aliases/crm_deals__fresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/collections/crm_deals_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    let engine =
        TypesenseEngine::new(&server.uri(), &[], "xyz".into(), Duration::from_secs(5)).unwrap();
    engine.swap("crm_deals", "crm_deals__fresh").await.unwrap();
}

#[tokio::test]
async fn qdrant_stores_points_with_payload_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/collections/crm_contacts"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"status": {"error": "Not found"}})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/collections/crm_contacts"))
        .and(body_partial_json(
            json!({"vectors": {"size": 3, "distance": "Cosine"}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/collections/crm_contacts/points"))
        .and(query_param("wait", "true"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": {"status": "completed"}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/collections/crm_contacts/points/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [
                { "id": "x", "score": 1.0, "payload": { "_id": "a1" } },
                { "id": "y", "score": 0.0, "payload": { "_id": "a2" } }
            ]
        })))
        .mount(&server)
        .await;
    let store = QdrantStore::new(
        &server.uri(),
        Some("k".into()),
        3,
        Metric::Cosine,
        Duration::from_secs(5),
    )
    .unwrap();
    store.prepare("crm_contacts").await.unwrap();
    store
        .upsert(
            "crm_contacts",
            vec![VectorRecord {
                id: "a1".into(),
                vector: vec![1.0, 0.0, 0.0],
                payload: [("status".to_owned(), json!("customer"))]
                    .into_iter()
                    .collect(),
            }],
        )
        .await
        .unwrap();
    let points = body(&requests_to(&server, "PUT", "/collections/crm_contacts/points").await[0]);
    assert_eq!(
        points["points"][0]["payload"],
        json!({"status": "customer", "_id": "a1"})
    );
    assert_eq!(points["points"][0]["vector"], json!([1.0, 0.0, 0.0]));
    assert!(uuid::Uuid::parse_str(points["points"][0]["id"].as_str().unwrap()).is_ok());

    let found = store
        .nearest(
            "crm_contacts",
            &[1.0, 0.0, 0.0],
            &Filter::eq("status", "customer").normalized(),
            2,
        )
        .await
        .unwrap();
    assert_eq!(found[0].id, "a1");
    assert_eq!(found[0].score, 1.0);
    assert_eq!(found[1].score, 0.5);
    let query =
        body(&requests_to(&server, "POST", "/collections/crm_contacts/points/search").await[0]);
    assert_eq!(
        query["filter"],
        json!({"must": [{"key": "status", "match": {"value": "customer"}}]})
    );
    assert_eq!(query["limit"], 2);
    let sent = &requests_to(&server, "PUT", "/collections/crm_contacts").await[0];
    assert_eq!(sent.headers.get("api-key").unwrap(), "k");
}
