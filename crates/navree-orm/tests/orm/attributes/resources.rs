//! Records as API resources (`__type`, declared resources, field masks)
//! and OpenAPI schemas of models and pages, up to the document navree-api
//! serves.

use std::sync::Arc;

use axum::extract::Path;
use chrono::NaiveDate;
use http::Method;
use navree_api::{ApiModule, ApiOptions, ApiResource};
use navree_db::{DatabaseModule, Db, PageQuery, Uow};
use navree_http::{ApiMeta, AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef, schema_fn};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::attributes::{
    ModelSchema, Resource, cursor_page_schema, page_schema, simple_page_schema,
};
use navree_orm::{Collection, CursorPage, Model, Page, Record, SimplePage, accessors};
use navree_testing::TestApp;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{PartialSchema, ToSchema};

use super::support::{create, sqlite};

const SCHEMA: &str = r#"
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    phone TEXT NOT NULL,
    tier TEXT NOT NULL,
    nickname TEXT NULL,
    score REAL NULL,
    created_on TEXT NOT NULL
)
"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    #[default]
    Free,
    Pro,
}

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "contacts",
    unguarded,
    hidden(phone),
    appends(display),
    resource = ContactCard
)]
pub struct Contact {
    pub id: i64,
    pub name: String,
    #[serde(rename = "mail")]
    pub email: String,
    pub phone: String,
    #[model(cast = "string_enum", schema = Tier)]
    pub tier: Tier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    pub score: Option<f64>,
    pub created_on: NaiveDate,
}

#[accessors]
impl Contact {
    /// "Ada <ada@example.com>".
    #[accessor]
    pub fn display(&self) -> String {
        format!("{} <{}>", self.name, self.email)
    }

    /// The tier after this one.
    #[accessor(schema = Tier)]
    pub fn next_tier(&self) -> Tier {
        Tier::Pro
    }
}

/// The card the API returns for a contact.
#[derive(Debug, PartialEq, Serialize)]
pub struct ContactCard {
    pub id: i64,
    pub label: String,
    pub tier: Tier,
}

impl ApiResource for ContactCard {
    const TYPE: &'static str = "Contact";
}

impl Resource<Contact> for ContactCard {
    fn from_record(contact: &Record<Contact>) -> Self {
        ContactCard {
            id: contact.id,
            label: contact.display(),
            tier: contact.tier,
        }
    }
}

fn ada() -> Contact {
    Contact {
        id: 1,
        name: "Ada".into(),
        email: "ada@example.com".into(),
        phone: "555-0100".into(),
        tier: Tier::Pro,
        nickname: None,
        score: Some(9.5),
        created_on: NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
    }
}

#[test]
fn records_become_resources_with_their_subject_type() {
    let contact = Record::new(ada());
    assert_eq!(
        contact.to_resource::<ContactCard>(),
        ContactCard {
            id: 1,
            label: "Ada <ada@example.com>".into(),
            tier: Tier::Pro
        }
    );
    assert_eq!(
        contact.resource(),
        contact.to_resource::<ContactCard>(),
        "the declared resource"
    );
    assert_eq!(
        contact.to_resource_json_as::<ContactCard>(),
        json!({"id": 1, "label": "Ada <ada@example.com>", "tier": "pro", "__type": "Contact"})
    );
    let own = contact.to_resource_json().unwrap();
    assert_eq!(own["__type"], "Contact");
    assert!(own.get("phone").is_none(), "visibility applies");
    assert_eq!(own["display"], "Ada <ada@example.com>");

    let contacts: Collection<Contact> =
        vec![Record::new(ada()), Record::new(Contact { id: 2, ..ada() })].into();
    assert_eq!(contacts.to_resource_collection::<ContactCard>().len(), 2);
    assert_eq!(contacts.resources()[1].id, 2);
    assert_eq!(
        contacts.to_resource_json_as::<ContactCard>()[1]["__type"],
        "Contact"
    );
}

#[test]
fn masks_keep_the_permitted_columns_the_key_and_the_type() {
    let contact = Record::new(ada());
    assert_eq!(
        contact.field_names().unwrap(),
        [
            "id",
            "name",
            "mail",
            "tier",
            "score",
            "created_on",
            "display"
        ]
    );
    let permitted = vec!["name".to_owned(), "email".to_owned()];
    assert_eq!(
        contact.to_value_masked(Some(&permitted)).unwrap(),
        json!({"id": 1, "name": "Ada", "mail": "ada@example.com", "__type": "Contact"}),
        "permitted fields name columns; the output uses serde's keys"
    );
    let everything = contact.to_value_masked(None).unwrap();
    assert_eq!(everything, contact.to_resource_json().unwrap());
    let contacts: Collection<Contact> = vec![Record::new(ada())].into();
    assert_eq!(
        contacts
            .to_value_masked(Some(&["display".to_owned()]))
            .unwrap(),
        json!([{"id": 1, "display": "Ada <ada@example.com>", "__type": "Contact"}])
    );
    assert_eq!(
        navree_api::mask_fields(
            contact.to_resource_json().unwrap(),
            Some(&["tier".to_owned()])
        ),
        json!({"id": 1, "tier": "pro", "__type": "Contact"}),
        "navree-api's mask works on the JSON too"
    );
}

#[test]
fn model_schemas_describe_serialized_records() {
    assert_eq!(Record::<Contact>::name(), "Contact");
    let schema = serde_json::to_value(Record::<Contact>::schema()).unwrap();
    assert_eq!(schema, ModelSchema::<Contact>::json());
    assert_eq!(
        schema["properties"],
        json!({
            "id": {"type": "integer", "format": "int64"},
            "name": {"type": "string"},
            "mail": {"type": "string"},
            "tier": {"type": "string", "enum": ["free", "pro"]},
            "nickname": {"type": ["string", "null"]},
            "score": {"type": ["number", "null"], "format": "double"},
            "created_on": {"type": "string", "format": "date"},
            "display": {"type": "string"},
        })
    );
    assert_eq!(
        schema["required"],
        json!([
            "id",
            "name",
            "mail",
            "tier",
            "score",
            "created_on",
            "display"
        ]),
        "skip_serializing_if fields are optional"
    );
    let (name, page) = page_schema::<Record<Contact>>();
    assert_eq!(name, "ContactPage");
    assert_eq!(
        page["properties"]["meta"]["required"],
        json!(["current_page", "per_page", "total", "last_page"])
    );
    assert_eq!(page["x-navree-components"]["Contact"], schema);
    assert_eq!(
        simple_page_schema::<Record<Contact>>().0,
        "ContactSimplePage"
    );
    assert_eq!(
        cursor_page_schema::<Record<Contact>>().1["properties"]["next_cursor"],
        json!({"type": ["string", "null"]})
    );
    assert_eq!(SimplePage::<Record<Contact>>::name(), "ContactSimplePage");
    assert_eq!(CursorPage::<Record<Contact>>::name(), "ContactCursorPage");
    let (_, simple) = schema_fn::<SimplePage<Record<Contact>>>();
    assert_eq!(simple["x-navree-components"]["Contact"], schema);
}

struct ContactsController;

impl Injectable for ContactsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ContactsController) })
    }
}

impl Controller for ContactsController {
    fn prefix(&self) -> &'static str {
        "/contacts"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", |uow: Uow, page: PageQuery| async move {
                let page: Page<Record<Contact>> = Contact::query().paginate(&uow, page).await?;
                Ok::<_, AppError>(page)
            })
            .public()
            .api(ApiMeta {
                response: Some(page_schema::<Record<Contact>>),
                ..ApiMeta::default()
            }),
            RouteDef::on(
                Method::GET,
                "/{id}",
                |uow: Uow, Path(id): Path<i64>| async move {
                    let contact = Contact::find_or_fail(&uow, id).await?;
                    Ok::<_, AppError>(Json(contact.to_resource_json_as::<ContactCard>()))
                },
            )
            .public()
            .api(ApiMeta {
                response: Some(schema_fn::<Record<Contact>>),
                ..ApiMeta::default()
            }),
        ]
    }
}

struct Contacts;

impl Module for Contacts {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ContactsController>();
    }
}

#[tokio::test]
async fn the_openapi_document_carries_model_and_page_schemas() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(ApiModule::for_root(ApiOptions::default()))
        .module(Contacts)
        .build()
        .await;
    let db = app.get::<Db>().await;
    create(&db, SCHEMA).await;
    Contact::create(&*db, Contact { id: 0, ..ada() })
        .await
        .unwrap();

    let document = app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["Contact"]["properties"]["mail"],
        json!({"type": "string"})
    );
    assert!(schemas["Contact"]["properties"].get("phone").is_none());
    assert_eq!(
        schemas["ContactPage"]["properties"]["data"]["items"]["$ref"],
        "#/components/schemas/Contact"
    );

    app.client()
        .get("/api/contacts")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.display", json!("Ada <ada@example.com>"))
        .assert_json_path("meta.total", json!(1));
    app.client()
        .get("/api/contacts/1")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("__type", json!("Contact"))
        .assert_json_path("label", json!("Ada <ada@example.com>"));
}

#[tokio::test]
async fn records_read_from_the_database_serialize_like_new_ones() {
    let t = sqlite(SCHEMA).await;
    let created = Contact::create(&t.db, Contact { id: 0, ..ada() })
        .await
        .unwrap();
    let stored = Contact::find_or_fail(&t.db, created.id).await.unwrap();
    assert_eq!(stored.to_value().unwrap(), created.to_value().unwrap());
}
