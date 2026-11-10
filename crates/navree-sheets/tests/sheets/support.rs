//! Shared fixtures: a `contacts` table, its importer and export, the app
//! builder and request helpers.

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};

use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_contracts::{AuthVia, BoxError, Principal};
use navree_db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{
    DatabaseModule, Db, DbError, Executor, MigrationSet, ModuleBuilderDbExt, Statement, Uow,
    Value as SqlValue,
};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::injectable;
use navree_queue::{OperationRecord, Operations, Queue, QueueModule};
use navree_sheets::{
    Export, ExportContext, ExportPage, Exportable, Import, ImportContext, ImportRecord, Importable,
    Imports, ModuleBuilderSheetsExt, RowRejected, SheetsModule,
};
use navree_storage::{Storage, StorageModule};
use navree_testing::{TestApp, TestAppBuilder, TestClient};
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// May import and export contacts.
pub const ADMIN: Uuid = Uuid::from_u128(1);
/// Holds no permission.
pub const OTHER: Uuid = Uuid::from_u128(2);

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.user.as_ref().is_some_and(|user| user.id == ADMIN) {
        rules.can("import", "contacts");
        rules.can("export", "contacts");
    }
}

pub fn principal(user_id: Uuid) -> Principal {
    Principal {
        user_id,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}

// ---------------------------------------------------------------------------
// The contacts table.

struct CreateContacts;

impl Migration for CreateContacts {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("contacts", |t| {
                t.string("id").length(36).primary();
                t.string("email").length(255);
                t.string("name").length(255);
                t.big_integer("age").nullable();
                t.boolean("vip");
                t.unique(["email"]).name("contacts_email_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop("contacts").await
    }
}

/// One contact as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    pub email: String,
    pub name: String,
    pub age: Option<i64>,
    pub vip: bool,
}

pub async fn insert_contact<E: Executor + ?Sized>(
    exec: &E,
    contact: &Contact,
) -> Result<(), DbError> {
    exec.execute(&Statement::portable(
        "INSERT INTO contacts (id, email, name, age, vip) VALUES (?, ?, ?, ?, ?)",
        vec![
            Uuid::now_v7().to_string().into(),
            contact.email.clone().into(),
            contact.name.clone().into(),
            contact.age.into(),
            contact.vip.into(),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn contacts<E: Executor + ?Sized>(exec: &E) -> Vec<Contact> {
    let rows = exec
        .fetch_all(&Statement::text(
            "SELECT email, name, age, vip FROM contacts ORDER BY email",
        ))
        .await
        .unwrap();
    rows.iter()
        .map(|row| Contact {
            email: row.get("email").unwrap(),
            name: row.get("name").unwrap(),
            age: row.get("age").unwrap(),
            vip: row.get("vip").unwrap(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The importer.

/// One row of a contacts file.
#[derive(Debug, Deserialize, Validate, Importable)]
pub struct ContactRow {
    #[import(header = "Email", aliases = ["E-mail", "البريد الإلكتروني"])]
    #[validate(email)]
    pub email: String,
    #[import(header = "Name", aliases = ["الاسم"])]
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(range(min = 0, max = 150))]
    pub age: Option<u32>,
    #[serde(default)]
    #[validate(skip)]
    pub vip: bool,
}

/// The imports whose `completed` hook ran.
static COMPLETED: Mutex<Vec<Uuid>> = Mutex::new(Vec::new());

pub fn completed(import_id: Uuid) -> bool {
    COMPLETED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&import_id)
}

#[injectable]
pub struct ContactsImport;

impl Import for ContactsImport {
    const NAME: &'static str = "contacts";
    type Row = ContactRow;

    fn key(row: &ContactRow) -> Option<String> {
        Some(row.email.trim().to_lowercase())
    }

    async fn existing(
        &self,
        ctx: &ImportContext,
        keys: &[String],
    ) -> Result<HashSet<String>, BoxError> {
        let db = ctx.app.get::<Db>().await?;
        let placeholders = vec!["?"; keys.len()].join(", ");
        let rows = db
            .fetch_all(&Statement::portable(
                format!("SELECT email FROM contacts WHERE email IN ({placeholders})"),
                keys.iter().map(|k| SqlValue::from(k.clone())),
            ))
            .await?;
        Ok(rows
            .iter()
            .map(|row| row.get::<String>("email"))
            .collect::<Result<_, _>>()?)
    }

    async fn insert(
        &self,
        _ctx: &ImportContext,
        uow: &Uow,
        row: ContactRow,
    ) -> Result<(), BoxError> {
        if row.name == "Robot" {
            return Err(RowRejected::new("Robots cannot be contacts").into());
        }
        if row.name == "Explode" {
            return Err("the CRM is down".into());
        }
        let contact = Contact {
            email: row.email.trim().to_lowercase(),
            name: row.name,
            age: row.age.map(i64::from),
            vip: row.vip,
        };
        insert_contact(uow, &contact).await?;
        Ok(())
    }

    async fn update(
        &self,
        _ctx: &ImportContext,
        uow: &Uow,
        row: ContactRow,
    ) -> Result<(), BoxError> {
        uow.execute(&Statement::portable(
            "UPDATE contacts SET name = ?, age = ?, vip = ? WHERE email = ?",
            [
                row.name.into(),
                row.age.map(i64::from).into(),
                row.vip.into(),
                row.email.trim().to_lowercase().into(),
            ],
        ))
        .await?;
        Ok(())
    }

    async fn completed(&self, _ctx: &ImportContext, import: &ImportRecord) -> Result<(), BoxError> {
        COMPLETED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(import.id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The export.

#[derive(Debug, Exportable)]
#[export(sheet = "Contacts")]
pub struct ContactExportRow {
    #[export(header = "contacts.email")]
    pub email: String,
    pub name: String,
    #[export(format = "0")]
    pub age: Option<i64>,
    pub vip: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ContactFilter {
    pub vip: Option<bool>,
}

#[injectable]
pub struct ContactsExport;

fn filter_sql(filter: &ContactFilter) -> (&'static str, Vec<SqlValue>) {
    match filter.vip {
        Some(vip) => ("WHERE vip = ?", vec![vip.into()]),
        None => ("", Vec::new()),
    }
}

impl Export for ContactsExport {
    const NAME: &'static str = "contacts";
    type Row = ContactExportRow;
    type Params = ContactFilter;

    fn filename(&self, filter: &ContactFilter) -> String {
        if filter.vip == Some(true) {
            "vip-contacts".into()
        } else {
            "contacts".into()
        }
    }

    async fn count(
        &self,
        ctx: &ExportContext,
        filter: &ContactFilter,
    ) -> Result<Option<u64>, BoxError> {
        let db = ctx.app.get::<Db>().await?;
        let (clause, values) = filter_sql(filter);
        let row = db
            .fetch_optional(&Statement::portable(
                format!("SELECT COUNT(*) AS n FROM contacts {clause}"),
                values,
            ))
            .await?
            .ok_or("no count")?;
        Ok(Some(u64::try_from(row.get::<i64>("n")?)?))
    }

    async fn rows(
        &self,
        ctx: &ExportContext,
        filter: &ContactFilter,
        page: ExportPage,
    ) -> Result<Vec<ContactExportRow>, BoxError> {
        let db = ctx.app.get::<Db>().await?;
        let (clause, mut values) = filter_sql(filter);
        values.push(i64::try_from(page.limit)?.into());
        values.push(i64::try_from(page.offset)?.into());
        let rows = db
            .fetch_all(&Statement::portable(
                format!(
                    "SELECT email, name, age, vip FROM contacts {clause} ORDER BY email LIMIT ? OFFSET ?"
                ),
                values,
            ))
            .await?;
        rows.iter()
            .map(|row| {
                Ok(ContactExportRow {
                    email: row.get("email")?,
                    name: row.get("name")?,
                    age: row.get("age")?,
                    vip: row.get("vip")?,
                })
            })
            .collect()
    }
}

/// The contacts feature: its table, importer and export.
pub struct ContactsModule;

impl Module for ContactsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("contacts").migration(SchemaMigration::new(
                "m20261027_000001_create_contacts",
                CreateContacts,
            )),
        )
        .importer::<ContactsImport>()
        .export::<ContactsExport>();
    }
}

// ---------------------------------------------------------------------------
// The app.

/// Database (on a fresh SQLite file, or the server `NAVREE_TEST_DATABASE` names), memory queue, memory disk, access
/// rules, sheets and the contacts module.
pub fn builder() -> TestAppBuilder {
    crate::servers::database(base())
}

/// [`builder`] without the database URL.
pub fn base() -> TestAppBuilder {
    TestApp::builder()
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
}

pub async fn app() -> TestApp {
    builder().build().await
}

pub async fn run_queue(app: &TestApp) {
    app.get::<Queue>().await.run_pending().await.unwrap();
}

pub async fn operation(app: &TestApp, id: Uuid) -> OperationRecord {
    app.get::<Operations>()
        .await
        .find(id)
        .await
        .unwrap()
        .unwrap()
}

pub async fn file_on_disk(app: &TestApp, disk: &str, path: &str) -> Vec<u8> {
    app.get::<Storage>()
        .await
        .disk(disk)
        .unwrap()
        .get(path)
        .await
        .unwrap()
        .to_vec()
}

pub async fn seed(app: &TestApp, list: &[(&str, &str, Option<i64>, bool)]) {
    let db = app.get::<Db>().await;
    for (email, name, age, vip) in list {
        insert_contact(
            &*db,
            &Contact {
                email: (*email).to_owned(),
                name: (*name).to_owned(),
                age: *age,
                vip: *vip,
            },
        )
        .await
        .unwrap();
    }
}

pub async fn stored_contacts(app: &TestApp) -> Vec<Contact> {
    contacts(&*app.get::<Db>().await).await
}

pub fn contact(email: &str, name: &str, age: Option<i64>, vip: bool) -> Contact {
    Contact {
        email: email.to_owned(),
        name: name.to_owned(),
        age,
        vip,
    }
}

// ---------------------------------------------------------------------------
// Requests.

/// A multipart body of `fields` and one `file`; returns the content type
/// and the body.
pub fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> (String, Vec<u8>) {
    let boundary = "navree-sheets-test-boundary";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((filename, bytes)) = file {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Uploads `bytes` as `filename` for the contacts importer; returns the
/// response.
pub async fn upload(
    client: &TestClient,
    filename: &str,
    bytes: &[u8],
    extra: &[(&str, &str)],
) -> navree_testing::TestResponse {
    let mut fields = vec![("importer", "contacts")];
    fields.extend_from_slice(extra);
    let (content_type, body) = multipart(&fields, Some((filename, bytes)));
    client
        .post("/api/imports")
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
}

/// The import `id`, read through the service.
pub async fn import(app: &TestApp, id: Uuid) -> ImportRecord {
    app.get::<Imports>().await.find(id).await.unwrap()
}

/// The id in a JSON response.
pub fn id_of(value: &Value) -> Uuid {
    Uuid::parse_str(value["id"].as_str().expect("an id")).unwrap()
}

/// Every line of a CSV file without its byte-order mark.
pub fn csv_lines(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(&text).to_owned();
    text.split("\r\n")
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}
