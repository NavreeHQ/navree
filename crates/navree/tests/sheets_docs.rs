//! The examples of `content/docs/packages/imports-and-exports.mdx`, written
//! as an app writes them (the facade, the prelude and the macros), compiled
//! and run.

use std::collections::HashSet;

use garde::Validate;
use navree::access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree::contracts::{AuthVia, Principal};
use navree::db::{Db, Repo};
use navree::prelude::*;
use navree::queue::Queue;
use navree::sheets::{
    Download, DuplicateMode, Export, ExportContext, ExportPage, Exportable, Exports, Import,
    ImportContext, ImportStatus, Importable, Imports, RowRejected, Upload, UploadOptions,
};
use navree::testing::TestApp;
use sea_orm::sea_query::{ColumnDef, Expr, Table};
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

/// The contacts table.
mod contact {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "contacts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub email: String,
        pub name: String,
        pub company: Option<String>,
        pub vip: bool,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct CreateContacts;

impl MigrationName for CreateContacts {
    fn name(&self) -> &str {
        "m20261027_000001_create_contacts"
    }
}

#[async_trait]
impl MigrationTrait for CreateContacts {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), sea_orm::DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("contacts")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("email").string().not_null().unique_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("company").string().null())
                    .col(ColumnDef::new("vip").boolean().not_null())
                    .to_owned(),
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// Exports.

/// One row of the contacts export.
#[derive(Exportable)]
#[export(sheet = "Contacts")]
struct ContactRow {
    #[export(header = "Name")]
    name: String,
    #[export(header = "Email", width = 30)]
    email: String,
    company: Option<String>,
    #[export(header = "VIP")]
    vip: bool,
}

impl From<contact::Model> for ContactRow {
    fn from(contact: contact::Model) -> Self {
        ContactRow {
            name: contact.name,
            email: contact.email,
            company: contact.company,
            vip: contact.vip,
        }
    }
}

/// The filters of the contacts export.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct ContactFilter {
    vip: Option<bool>,
}

#[injectable]
struct ContactsExport {
    db: Arc<Db>,
}

impl Export for ContactsExport {
    const NAME: &'static str = "contacts";
    type Row = ContactRow;
    type Params = ContactFilter;

    fn filename(&self, filter: &ContactFilter) -> String {
        match filter.vip {
            Some(true) => "vip-contacts".into(),
            _ => "contacts".into(),
        }
    }

    async fn rows(
        &self,
        _ctx: &ExportContext,
        filter: &ContactFilter,
        page: ExportPage,
    ) -> Result<Vec<ContactRow>, BoxError> {
        let mut query = contact::Entity::find().order_by_asc(contact::Column::Email);
        if let Some(vip) = filter.vip {
            query = query.filter(contact::Column::Vip.eq(vip));
        }
        let contacts = query
            .offset(page.offset)
            .limit(page.limit)
            .all(self.db.conn())
            .await?;
        Ok(contacts.into_iter().map(ContactRow::from).collect())
    }
}

// ---------------------------------------------------------------------------
// Imports.

/// One row of a contacts file.
#[derive(Deserialize, Validate, Importable)]
struct ContactInput {
    #[import(header = "Email", aliases = ["E-mail", "Email address"])]
    #[garde(email)]
    email: String,
    #[garde(length(min = 1, max = 100))]
    name: String,
    #[import(aliases = ["Organization"])]
    #[garde(length(max = 100))]
    company: Option<String>,
    #[serde(default)]
    #[garde(skip)]
    vip: bool,
}

#[injectable]
struct ContactsImport {
    db: Arc<Db>,
}

impl Import for ContactsImport {
    const NAME: &'static str = "contacts";
    type Row = ContactInput;

    fn key(row: &ContactInput) -> Option<String> {
        Some(row.email.trim().to_lowercase())
    }

    async fn existing(
        &self,
        _ctx: &ImportContext,
        keys: &[String],
    ) -> Result<HashSet<String>, BoxError> {
        let found = contact::Entity::find()
            .filter(contact::Column::Email.is_in(keys.to_vec()))
            .all(self.db.conn())
            .await?;
        Ok(found.into_iter().map(|contact| contact.email).collect())
    }

    async fn insert(
        &self,
        _ctx: &ImportContext,
        uow: &Uow,
        row: ContactInput,
    ) -> Result<(), BoxError> {
        if row.email.ends_with("@competitor.example") {
            return Err(RowRejected::new("Contacts at competitors are not imported").into());
        }
        let contact = contact::ActiveModel {
            id: Set(Uuid::now_v7()),
            email: Set(row.email.trim().to_lowercase()),
            name: Set(row.name),
            company: Set(row.company),
            vip: Set(row.vip),
        };
        Repo::<contact::Entity>::insert(uow, contact).await?;
        Ok(())
    }

    async fn update(
        &self,
        _ctx: &ImportContext,
        uow: &Uow,
        row: ContactInput,
    ) -> Result<(), BoxError> {
        contact::Entity::update_many()
            .col_expr(contact::Column::Name, Expr::value(row.name))
            .col_expr(contact::Column::Company, Expr::value(row.company))
            .filter(contact::Column::Email.eq(row.email.trim().to_lowercase()))
            .exec(&*uow.tx().await?)
            .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// A controller with downloads of its own.

#[injectable]
struct ContactsController {
    exports: Arc<Exports>,
    db: Arc<Db>,
}

#[controller("/contacts", version = "1")]
impl ContactsController {
    /// Every contact as CSV, with the app's `sheets.csv` options.
    #[get("/export.csv")]
    #[permissions("contacts.export")]
    async fn export_csv(&self) -> Result<Download> {
        let contacts = contact::Entity::find()
            .order_by_asc(contact::Column::Email)
            .all(self.db.conn())
            .await?;
        Ok(self
            .exports
            .csv("contacts", contacts.into_iter().map(ContactRow::from)))
    }

    /// The same as an Excel workbook.
    #[get("/export.xlsx")]
    #[permissions("contacts.export")]
    async fn export_xlsx(&self) -> Result<Download> {
        let contacts = contact::Entity::find()
            .order_by_asc(contact::Column::Email)
            .all(self.db.conn())
            .await?;
        Ok(Download::xlsx(
            "contacts",
            contacts.into_iter().map(ContactRow::from),
        )?)
    }
}

/// The contacts feature.
#[module(controllers = [ContactsController], register = Self::wire)]
struct ContactsModule;

impl ContactsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("contacts").migration(CreateContacts))
            .importer::<ContactsImport>()
            .export::<ContactsExport>();
    }
}

// ---------------------------------------------------------------------------
// The app and the tests.

const ADMIN: Uuid = Uuid::from_u128(7);

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.user.as_ref().is_some_and(|user| user.id == ADMIN) {
        rules.can("import", "contacts");
        rules.can("export", "contacts");
    }
}

fn admin() -> Principal {
    Principal {
        user_id: ADMIN,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("queue.driver", "memory")
        .config(
            "storage",
            json!({ "default": "files", "disks": { "files": { "driver": "memory" } } }),
        )
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(StorageModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(SheetsModule::from_config())
        .module(ContactsModule)
        .build()
        .await
}

async fn run_queue(app: &TestApp) {
    app.get::<Queue>().await.run_pending().await.unwrap();
}

#[tokio::test]
async fn a_contacts_file_is_imported_from_code() {
    let app = app().await;
    let imports = app.get::<Imports>().await;
    let db = app.get::<Db>().await;

    let uow = db.uow(Some(ADMIN), None).await;
    let import = imports
        .upload(
            &uow,
            "contacts",
            Upload::new(
                "contacts.csv",
                "Email address,Name,Organization\n\
                 ada@example.com,Ada Lovelace,Analytical Engines\n\
                 bob@competitor.example,Bob,Rival\n",
            ),
            UploadOptions::default().owner(ADMIN),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    run_queue(&app).await;
    assert_eq!(
        imports.find(import.id).await.unwrap().status,
        ImportStatus::Staged
    );

    let uow = db.uow(Some(ADMIN), None).await;
    imports
        .apply(&uow, import.id, DuplicateMode::Update)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    run_queue(&app).await;
    let done = imports.find(import.id).await.unwrap();
    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!(done.counts.created, 1);
    assert_eq!(done.counts.failed, 1);
    assert!(done.has_error_file);
}

#[tokio::test]
async fn contacts_are_uploaded_and_exported_over_the_api() {
    let app = app().await;
    let client = app.acting_as(admin());
    let boundary = "docs-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"importer\"\r\n\r\ncontacts\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"contacts.csv\"\r\n\r\n\
         Email,Name\nada@example.com,Ada\ngrace@example.com,Grace\n\r\n--{boundary}--\r\n"
    );
    let uploaded = client
        .post("/api/imports")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .assert_status(202)
        .json_value();
    let id = uploaded["id"].as_str().unwrap().to_owned();
    run_queue(&app).await;
    client
        .get(&format!("/api/imports/{id}"))
        .send()
        .await
        .assert_json_path("status", json!("staged"))
        .assert_json_path("counts.valid", json!(2));
    client
        .post(&format!("/api/imports/{id}/apply"))
        .json(&json!({ "duplicates": "skip" }))
        .send()
        .await
        .assert_status(202);
    run_queue(&app).await;
    client
        .get(&format!("/api/imports/{id}"))
        .send()
        .await
        .assert_json_path("status", json!("completed"));

    let csv = client
        .get("/api/v1/contacts/export.csv")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        csv.text(),
        "\u{FEFF}Name,Email,Company,VIP\r\nAda,ada@example.com,,false\r\nGrace,grace@example.com,,false\r\n"
    );
    let xlsx = client
        .get("/api/v1/contacts/export.xlsx")
        .send()
        .await
        .assert_status(200);
    assert!(xlsx.bytes().starts_with(b"PK"));
    let named = client
        .get("/api/exports/contacts")
        .send()
        .await
        .assert_status(200);
    assert_eq!(named.text(), csv.text());

    let queued = client
        .post("/api/exports/contacts")
        .json(&json!({ "format": "xlsx", "params": { "vip": false } }))
        .send()
        .await
        .assert_status(202)
        .json_value();
    run_queue(&app).await;
    let operation = client
        .get(&format!(
            "/api/operations/{}",
            queued["id"].as_str().unwrap()
        ))
        .send()
        .await
        .json_value();
    assert_eq!(operation["status"], "succeeded");
    assert_eq!(operation["result"]["filename"], "contacts.xlsx");
    let url = operation["result"]["url"].as_str().unwrap().to_owned();
    app.client().get(&url).send().await.assert_status(200);
}
