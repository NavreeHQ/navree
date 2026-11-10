//! Searching from a controller: pages over HTTP and search errors in
//! Navree's error shape.

use std::sync::Arc;

use navree_db::{Db, Page, PageQuery};
use navree_http::{AppError, ModuleBuilderHttpExt, Query};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{controller, injectable};
use navree_search::Search;
use serde::Deserialize;
use serde_json::json;

use navree_orm::Record;

use crate::support::{self, Contact, insert_contacts, new_contact, settle};

#[derive(Debug, Deserialize)]
pub struct SearchContacts {
    pub q: String,
    pub status: Option<String>,
    pub by: Option<String>,
}

#[injectable]
pub struct ContactSearchController {
    search: Arc<Search>,
}

#[controller("/contacts/search", version = "1")]
impl ContactSearchController {
    #[get("/")]
    #[public]
    async fn index(
        &self,
        Query(input): Query<SearchContacts>,
        page: PageQuery,
    ) -> Result<Page<Record<Contact>>, AppError> {
        let mut query = self.search.query::<Contact>(&input.q);
        if let Some(status) = &input.status {
            query = query.filter("status", status);
        }
        if let Some(field) = &input.by {
            query = query.sort_desc(field);
        }
        Ok(query.paginate(page).await?)
    }
}

struct SearchRoutes;

impl Module for SearchRoutes {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ContactSearchController>();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn controllers_return_search_pages() {
    let app = support::builder("memory", "inline")
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(SearchRoutes)
        .build()
        .await;
    let db = app.get::<Db>().await;
    insert_contacts(
        &db,
        vec![
            new_contact("Anu", "Lakhotia"),
            new_contact("Gauri", "Hundal"),
        ],
    )
    .await;
    settle(&app).await;
    app.client()
        .get("/api/v1/contacts/search")
        .query(&[("q", "lakotia")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(1))
        .assert_json_path("data.0.first_name", json!("Anu"));

    app.client()
        .get("/api/v1/contacts/search")
        .query(&[("q", "anu"), ("by", "first_name")])
        .send()
        .await
        .assert_error(
            400,
            "`first_name` is not a sortable field of the `contacts` index",
        );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_engine_is_a_503() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    drop(listener);
    let app = support::builder("meilisearch", "off")
        .config("search.url", url)
        .config("search.timeout_secs", 1)
        .module(SearchRoutes)
        .build()
        .await;
    app.client()
        .get("/api/v1/contacts/search")
        .query(&[("q", "anu")])
        .send()
        .await
        .assert_error(503, "Search is unavailable. Try again shortly.");
}
