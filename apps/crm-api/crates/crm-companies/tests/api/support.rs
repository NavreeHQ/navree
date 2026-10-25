//! Shared helpers: a CRM with both modules, record factories over HTTP and
//! direct inserts for what other crates own (deals).

use std::time::Duration;

use crm_companies::{CompaniesModule, ContactsModule};
use crm_core::Stage;
use crm_core::entities::deal;
use crm_testkit::{CrmClient, TestCrm};
use navree::contracts::Clock;
use navree::db::Db;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use serde_json::Value;
use uuid::Uuid;

/// `TestCrm::start()` as the API renders it.
pub const START: &str = "2026-10-26T10:00:00Z";

/// A CRM with the companies and contacts modules.
pub async fn crm() -> TestCrm {
    TestCrm::builder()
        .module(CompaniesModule)
        .module(ContactsModule)
        .build()
        .await
}

/// `POST /api/v1/companies` as `client`, expecting a 201.
pub async fn create_company(client: &CrmClient, body: Value) -> Value {
    client
        .post("/api/v1/companies")
        .json(&body)
        .send()
        .await
        .assert_status(201)
        .json_value()
}

/// `POST /api/v1/contacts` as `client`, expecting a 201.
pub async fn create_contact(client: &CrmClient, body: Value) -> Value {
    client
        .post("/api/v1/contacts")
        .json(&body)
        .send()
        .await
        .assert_status(201)
        .json_value()
}

/// The `id` of a resource.
pub fn id_of(resource: &Value) -> String {
    resource["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id in {resource}"))
        .to_owned()
}

/// `field` of every item of a page, in order.
pub fn column(page: &Value, field: &str) -> Vec<String> {
    page["data"]
        .as_array()
        .unwrap_or_else(|| panic!("not a page: {page}"))
        .iter()
        .map(|item| item[field].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// `field` of every item of the page `GET path?query` returns to `client`.
pub async fn list(
    client: &CrmClient,
    path: &str,
    query: &[(&str, &str)],
    field: &str,
) -> Vec<String> {
    let page = client
        .get(path)
        .query(query)
        .send()
        .await
        .assert_status(200)
        .json_value();
    column(&page, field)
}

/// Moves the fixed clock a minute on (access tokens stay valid).
pub fn tick(crm: &TestCrm) {
    crm.clock().advance(Duration::from_secs(60));
}

/// Inserts a deal of `amount_cents` at `stage` for `company_id` (deals are
/// another crate's API).
pub async fn insert_deal(
    crm: &TestCrm,
    company_id: &str,
    owner_id: Uuid,
    stage: Stage,
    amount_cents: i64,
) {
    let db = crm.get::<Db>().await;
    let now = crm.clock().now();
    deal::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(format!("{stage} deal")),
        amount_cents: Set(amount_cents),
        currency: Set("USD".to_owned()),
        stage: Set(stage),
        position: Set(0),
        expected_close_date: Set(None),
        closed_at: Set(None),
        lost_reason: Set(None),
        company_id: Set(company_id.parse().expect("a company id")),
        contact_id: Set(None),
        owner_id: Set(owner_id),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db.conn())
    .await
    .expect("the deal is inserted");
}
