//! What the pages promise and the first screens got wrong: messages that name
//! a field by its label, record pages titled by what the record is, resources
//! that are read-only, relationships beside each other on one foreign key,
//! the account block without a letter tile, and a table that fits its box.

use chrono::{DateTime, Utc};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{Db, MigrationSet, ModuleBuilderDbExt};
use navree_helm::{Field, ModuleBuilderHelmExt, Resource};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::Model;
use navree_testing::{TestApp, TestClient, TestResponse};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

async fn page(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

async fn submit(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    client
        .post(path)
        .header("accept", "text/html")
        .form(form)
        .send()
        .await
}

/// A contact, which belongs to a company as deals do.
#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "contacts",
    subject = "Contact",
    permission_prefix = "contacts"
)]
pub struct Contact {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

struct CreateContacts;

impl Migration for CreateContacts {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("contacts", |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "company_id", mysql).nullable();
                t.string("name");
                t.timestamp_tz("created_at");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("contacts").await
    }
}

macro_rules! injectable {
    ($($name:ident),*) => {$(
        impl Injectable for $name {
            fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
                Box::pin(async { Ok($name) })
            }
        }
    )*};
}

struct ContactResource;
struct BareDeals;
struct ReasonDeals;
struct FrozenDeals;
struct CompanyOverview;
injectable!(
    ContactResource,
    BareDeals,
    ReasonDeals,
    FrozenDeals,
    CompanyOverview
);

impl Resource for ContactResource {
    type Model = Contact;

    const LABEL: &'static str = "Contacts";
    const SINGULAR: &'static str = "Contact";
    const TITLE: &'static str = "name";

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().sortable(),
            Field::belongs_to::<CompanyResource>("company_id").label("Company"),
        ]
    }
}

/// Deals with no title column named: the key is the title unless the panel
/// finds a better one.
impl Resource for BareDeals {
    type Model = Deal;

    const LABEL: &'static str = "Bare deals";
    const SINGULAR: &'static str = "Bare deal";
    const SLUG: &'static str = "bare-deals";

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title"), Field::text("stage")]
    }
}

/// Deals named by a column the first one leaves empty.
impl Resource for ReasonDeals {
    type Model = Deal;

    const LABEL: &'static str = "Reason deals";
    const SINGULAR: &'static str = "Reason deal";
    const SLUG: &'static str = "reason-deals";
    const TITLE: &'static str = "lost_reason";

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title"), Field::text("lost_reason")]
    }
}

/// Deals that only the app writes.
impl Resource for FrozenDeals {
    type Model = Deal;

    const LABEL: &'static str = "Frozen deals";
    const SINGULAR: &'static str = "Frozen deal";
    const SLUG: &'static str = "frozen-deals";
    const TITLE: &'static str = "title";
    const READ_ONLY: bool = true;

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("title").sortable(), Field::text("stage")]
    }
}

/// A company with two panels of records that refer to it by `company_id`.
impl Resource for CompanyOverview {
    type Model = Company;

    const LABEL: &'static str = "Company overview";
    const SINGULAR: &'static str = "Company";
    const SLUG: &'static str = "company-overview";
    const TITLE: &'static str = "name";

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required(),
            Field::has_many::<DealResource>("company_id").label("Deals"),
            Field::has_many::<ContactResource>("company_id").label("Contacts"),
        ]
    }
}

struct FixesModule;

impl Module for FixesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<BareDeals>()
            .resource::<ReasonDeals>()
            .resource::<FrozenDeals>();
    }
}

/// Contacts, and a company with two panels that refer to it by `company_id`.
struct ManyModule;

impl Module for ManyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("fixes")
                .after(["crm"])
                .migration(SchemaMigration::new(
                    "m20261102_000001_create_contacts",
                    CreateContacts,
                )),
        )
        .resource::<ContactResource>()
        .resource::<CompanyOverview>();
    }
}

async fn fixes_app() -> TestApp {
    builder().module(FixesModule).build().await
}

async fn many_app() -> TestApp {
    builder().module(ManyModule).build().await
}

/// A user with the deals, companies and contacts abilities.
async fn staff(app: &TestApp) -> TestClient {
    let id = Uuid::now_v7();
    give_role(
        app,
        id,
        "staff",
        &["helm.view", "deals.*", "companies.*", "contacts.*"],
    )
    .await;
    app.acting_as(navree_testing::principal(id))
}

fn plain(response: TestResponse) -> String {
    response.text().replace("&#x2f;", "/")
}

#[tokio::test]
async fn messages_name_a_field_by_its_label_not_its_column() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    // Through the form.
    let html = plain(
        submit(
            &manager,
            "/admin/resources/deals",
            &[
                ("title", ""),
                ("amount_cents", "-5"),
                ("company_id", "nope"),
            ],
        )
        .await
        .assert_status(400),
    );
    assert!(html.contains("Title should not be empty"), "{html}");
    assert!(html.contains("Amount must not be less than 0"), "{html}");
    assert!(!html.contains("amount_cents must"), "{html}");
    assert!(!html.contains("title should"), "{html}");
    assert!(!html.contains("company_id "), "{html}");
    // Through the API.
    let body = manager
        .post("/admin/api/resources/deals")
        .json(&json!({ "title": "", "amount_cents": -5, "company_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    let errors = body["errors"].to_string();
    assert!(errors.contains("Title should not be empty"), "{errors}");
    assert!(
        errors.contains("Amount must not be less than 0"),
        "{errors}"
    );
    assert!(errors.contains("Company"), "{errors}");
    assert!(!errors.contains("amount_cents must"), "{errors}");
    assert!(!errors.contains("company_id "), "{errors}");
    assert!(
        !errors.contains("title should"),
        "lower-case column name in {errors}"
    );
}

#[tokio::test]
async fn a_value_that_does_not_fit_its_column_names_the_label_too() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    let body = manager
        .post("/admin/api/resources/deals")
        .json(&json!({ "title": "x", "amount_cents": 1, "expected_close_date": "not a date" }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    let errors = body["errors"].to_string();
    assert!(!errors.contains("expected_close_date "), "{errors}");
    assert!(errors.contains("Expected close date"), "{errors}");
}

#[tokio::test]
async fn record_pages_are_titled_by_the_resource_and_a_short_key_not_a_uuid() {
    let app = fixes_app().await;
    let staff = staff(&app).await;
    let titled = deal(
        &app,
        DealSeed {
            title: "Named",
            ..DealSeed::default()
        },
    )
    .await;
    let key = titled.id.to_string();
    let short = &key[key.len() - 8..];
    for slug in ["bare-deals", "reason-deals"] {
        let html = plain(
            page(&staff, &format!("/admin/resources/{slug}/{key}"))
                .await
                .assert_status(200),
        );
        let singular = if slug == "bare-deals" {
            "Bare deal"
        } else {
            "Reason deal"
        };
        let expected = format!("{singular} #{short}");
        assert!(
            html.contains(&format!("<title>{expected}</title>")),
            "{slug}: {html}"
        );
        assert!(
            html.contains(&format!("<h1 data-slot=\"page-title\">{expected}</h1>")),
            "{slug}"
        );
        assert!(!html.contains(&format!(">{key}</h1>")), "{slug}");
        // Nor does the list show the row's UUID.
        let list = plain(
            page(&staff, &format!("/admin/resources/{slug}"))
                .await
                .assert_status(200),
        );
        assert!(!list.contains(&format!(">{key}<")), "{list}");
    }
    // A title the record has stays.
    let html = plain(
        page(&staff, &format!("/admin/resources/frozen-deals/{key}"))
            .await
            .assert_status(200),
    );
    assert!(
        html.contains("<h1 data-slot=\"page-title\">Named</h1>"),
        "{html}"
    );
    // The API says the same.
    let body = staff
        .get(&format!("/admin/api/resources/bare-deals/{key}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"]["title"], json!(format!("Bare deal #{short}")));
}

#[tokio::test]
async fn a_read_only_resource_offers_no_new_edit_or_delete_and_refuses_them() {
    let app = fixes_app().await;
    let staff = staff(&app).await;
    let row = deal(&app, DealSeed::default()).await;
    let url = format!("/admin/resources/frozen-deals/{}", row.id);

    let list = plain(
        page(&staff, "/admin/resources/frozen-deals")
            .await
            .assert_status(200),
    );
    assert!(!list.contains("/frozen-deals/new"), "{list}");
    let detail = plain(page(&staff, &url).await.assert_status(200));
    assert!(!detail.contains("/edit"), "{detail}");
    assert!(!detail.contains("/delete"), "{detail}");
    // The same user may edit and delete the same deals in another resource.
    let open = plain(
        page(&staff, &format!("/admin/resources/deals/{}", row.id))
            .await
            .assert_status(200),
    );
    assert!(open.contains("/edit"), "{open}");

    // The server keeps refusing, whoever asks and however.
    page(&staff, &format!("{url}/edit"))
        .await
        .assert_status(403);
    page(&staff, "/admin/resources/frozen-deals/new")
        .await
        .assert_status(403);
    submit(&staff, &url, &[("title", "Changed")])
        .await
        .assert_status(403);
    submit(&staff, "/admin/resources/frozen-deals", &[("title", "New")])
        .await
        .assert_status(403);
    submit(&staff, &format!("{url}/delete"), &[("_confirmed", "1")])
        .await
        .assert_status(403);
    staff
        .patch(&format!("/admin/api/resources/frozen-deals/{}", row.id))
        .json(&json!({ "title": "Changed" }))
        .send()
        .await
        .assert_status(403);
    staff
        .delete(&format!("/admin/api/resources/frozen-deals/{}", row.id))
        .send()
        .await
        .assert_status(403);
    staff
        .post("/admin/api/resources/frozen-deals")
        .json(&json!({ "title": "New" }))
        .send()
        .await
        .assert_status(403);
    let kept = reload(&app, row.id).await.unwrap();
    assert_eq!(kept.title, "Deal");
    // The metadata and the record say so too.
    let body = staff
        .get(&format!("/admin/api/resources/frozen-deals/{}", row.id))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["data"]["can"],
        json!({ "update": false, "delete": false })
    );
}

#[tokio::test]
async fn two_relationships_may_share_a_foreign_key_name() {
    // The app boots (it did not) and both panels show their own records.
    let app = many_app().await;
    let staff = staff(&app).await;
    let agra = company(&app, "Agra", None).await;
    deal(
        &app,
        DealSeed {
            title: "Agra renewal",
            company: Some(agra.id),
            ..DealSeed::default()
        },
    )
    .await;
    {
        let db = app.get::<Db>().await;
        Contact::insert(
            &*db,
            [Contact {
                id: Uuid::now_v7(),
                company_id: Some(agra.id),
                name: "Anu Lakhotia".to_owned(),
                created_at: now(),
            }],
        )
        .await
        .unwrap();
    }
    let html = plain(
        page(
            &staff,
            &format!("/admin/resources/company-overview/{}", agra.id),
        )
        .await
        .assert_status(200),
    );
    assert!(
        html.contains("Deals <span data-slot=\"text\" data-variant=\"muted\">1</span>"),
        "{html}"
    );
    assert!(
        html.contains("Contacts <span data-slot=\"text\" data-variant=\"muted\">1</span>"),
        "{html}"
    );
    assert!(html.contains("Agra renewal"), "{html}");
    assert!(html.contains("Anu Lakhotia"), "{html}");
    assert!(
        html.contains(&format!(
            "/admin/resources/contacts?via%5Bcompany_id%5D={}",
            agra.id
        )),
        "{html}"
    );
}

#[tokio::test]
async fn the_account_block_is_the_shells_user_menu() {
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    let html = plain(
        page(&rep, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert!(html.contains("data-slot=\"sidebar-user\""), "{html}");
    assert!(html.contains("id=\"ui-user-menu\""), "{html}");
}

#[tokio::test]
async fn tables_scroll_in_their_box() {
    // A cell holding one long word (a key, an address, a client string) must not
    // push the page wider: the table scrolls in a box of its own, which a keyboard
    // can reach.
    let app = app().await;
    let (_, rep) = user(&app, "rep").await;
    deal(&app, DealSeed::default()).await;
    let html = plain(
        page(&rep, "/admin/resources/deals")
            .await
            .assert_status(200),
    );
    assert!(
        html.contains(
            "data-slot=\"table-container\" tabindex=\"0\" role=\"region\" aria-label=\"Deals\""
        ),
        "{html}"
    );
}

/// A tree: its children name the same key as its parent.
struct ChildrenFirst;

impl Injectable for ChildrenFirst {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ChildrenFirst) })
    }
}

impl Resource for ChildrenFirst {
    type Model = Contact;

    const LABEL: &'static str = "Children first";
    const SLUG: &'static str = "children-first";
    const TITLE: &'static str = "name";
    const SIDEBAR_COUNT: bool = false;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::has_many::<ContactResource>("company_id").label("Listed contacts"),
            Field::text("name").required(),
            Field::text("company_id").label("Parent").required(),
        ]
    }
}

struct ChildrenFirstModule;

impl Module for ChildrenFirstModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resource::<ChildrenFirst>();
    }
}

#[tokio::test]
async fn a_message_names_a_column_by_its_own_field_not_a_has_many_on_the_same_key() {
    let app = builder()
        .module(ManyModule)
        .module(ChildrenFirstModule)
        .build()
        .await;
    let staff = staff(&app).await;
    let answer = staff
        .post("/admin/api/resources/children-first")
        .json(&json!({ "name": "A", "company_id": "x" }))
        .send()
        .await
        .assert_status(400)
        .json_value();
    let message = answer["errors"]["company_id"][0].as_str().unwrap();
    assert!(message.contains("Parent"), "{answer}");
    assert!(!message.contains("Listed contacts"), "{answer}");
}
