//! File fields: uploads from the create and edit forms are stored on the
//! field's disk (type sniffed, size limited), the record keeps the path, and
//! its page links to the file. Upload problems appear under the field.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{MigrationSet, ModuleBuilderDbExt};
use navree_helm::{Field, ModuleBuilderHelmExt, Resource};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::{Model, Record};
use navree_storage::{DiskConfig, Storage, StorageModule, StorageOptions};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

/// A document with a scanned image; its timestamps and soft deletes are
/// the model's.
#[derive(navree_orm::Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(
    table = "documents",
    subject = "Document",
    permission_prefix = "documents",
    timestamps,
    soft_deletes
)]
pub struct Document {
    pub id: Uuid,
    pub name: String,
    pub scan: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

struct CreateDocuments;

impl Migration for CreateDocuments {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("documents", |t| {
                uuid(t, "id", mysql).primary();
                t.string("name");
                t.string("scan").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.timestamp_tz("deleted_at").nullable();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("documents").await
    }
}

/// Documents with a scanned image, stored on the `files` disk; saved by
/// Helm's default create and update.
struct DocumentResource;

impl Injectable for DocumentResource {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DocumentResource) })
    }
}

impl Resource for DocumentResource {
    type Model = Document;

    const LABEL: &'static str = "Documents";
    const SINGULAR: &'static str = "Document";
    const TITLE: &'static str = "name";

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required(),
            Field::file("scan", "files")
                .accept(["image/png"])
                .max_size(64),
        ]
    }
}

struct DocumentsModule;

impl Module for DocumentsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("documents").migration(SchemaMigration::new(
                "m20261026_000002_create_documents",
                CreateDocuments,
            )),
        )
        .resource::<DocumentResource>();
    }
}

/// The test app with a memory disk `files` and the documents resource.
async fn files_app() -> TestApp {
    let options = StorageOptions {
        default: "files".to_owned(),
        disks: BTreeMap::from([("files".to_owned(), DiskConfig::Memory)]),
        base_url: None,
    };
    builder()
        .module(StorageModule::for_root(options))
        .module(DocumentsModule)
        .build()
        .await
}

/// The smallest PNG header `infer` recognizes, padded to `len` bytes.
fn png(len: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.resize(len, 0);
    bytes
}

const BOUNDARY: &str = "helm-test-boundary";

/// A multipart body: text parts, then one file part.
fn multipart(texts: &[(&str, &str)], file: Option<(&str, &str, &[u8])>) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in texts {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((name, file_name, contents)) = file {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(contents);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn upload(client: &TestClient, path: &str, body: Vec<u8>) -> TestResponse {
    client
        .post(path)
        .header("accept", "text/html")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(body)
        .send()
        .await
}

async fn page(client: &TestClient, path: &str) -> String {
    client
        .get(path)
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text()
        .replace("&#x2f;", "/")
}

async fn reload_document(app: &TestApp, id: Uuid) -> Document {
    let db = app.get::<navree_db::Db>().await;
    Document::query()
        .with_trashed()
        .find(&*db, id)
        .await
        .unwrap()
        .map(Record::into_inner)
        .unwrap()
}

/// The default writes on a soft-deleting model with timestamps: the
/// model's own key and timestamps on create, `updated_at` on update, and a
/// delete that trashes the row, which the panel then no longer shows.
#[tokio::test]
async fn a_soft_deleting_model_is_trashed_by_the_default_delete() {
    let app = files_app().await;
    let (_, admin) = user(&app, ADMIN).await;
    let body = admin
        .post("/admin/api/resources/documents")
        .json(&json!({ "name": "Lease" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id: Uuid = body["data"]["id"].as_str().unwrap().parse().unwrap();
    let saved = reload_document(&app, id).await;
    assert_eq!(saved.name, "Lease");
    assert_eq!(saved.created_at, now());
    assert_eq!(saved.updated_at, now());
    assert_eq!(saved.deleted_at, None);

    admin
        .patch(&format!("/admin/api/resources/documents/{id}"))
        .json(&json!({ "name": "Lease, signed" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.title", json!("Lease, signed"));
    admin
        .delete(&format!("/admin/api/resources/documents/{id}"))
        .send()
        .await
        .assert_status(204);
    let trashed = reload_document(&app, id).await;
    assert_eq!(trashed.name, "Lease, signed");
    assert!(
        trashed.deleted_at.is_some(),
        "the row is trashed, not removed"
    );
    admin
        .get("/admin/api/resources/documents")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", 0.into());
    admin
        .get(&format!("/admin/api/resources/documents/{id}"))
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn uploads_are_stored_on_the_field_disk_and_linked() {
    let app = files_app().await;
    let (_, admin) = user(&app, ADMIN).await;

    let html = page(&admin, "/admin/resources/documents/new").await;
    assert!(html.contains("enctype=\"multipart/form-data\""), "{html}");
    assert!(html.contains("accept=\"image/png\""), "{html}");

    let response = upload(
        &admin,
        "/admin/resources/documents",
        multipart(
            &[("name", "Contract")],
            Some(("scan", "scan.png", &png(48))),
        ),
    )
    .await
    .assert_status(303);
    let url = response.header("location").unwrap().to_owned();
    let id: Uuid = url.rsplit('/').next().unwrap().parse().unwrap();
    let saved = reload_document(&app, id).await;
    let path = saved.scan.clone().expect("the path is saved");
    assert!(path.starts_with("documents/"), "{path}");
    assert!(path.ends_with(".png"), "the extension follows the content");
    let disk = app.get::<Storage>().await.disk("files").unwrap();
    assert_eq!(disk.get(&path).await.unwrap().len(), 48);

    // The record's page previews the image through a signed link.
    let html = page(&admin, &url).await;
    let name = path.rsplit('/').next().unwrap();
    assert!(html.contains(name), "{html}");
    assert!(html.contains("/storage/files/documents/"), "{html}");

    // Editing without a new file keeps the stored one.
    upload(
        &admin,
        &url,
        multipart(&[("name", "Signed contract")], Some(("scan", "", b""))),
    )
    .await
    .assert_status(303);
    let saved = reload_document(&app, id).await;
    assert_eq!(saved.name, "Signed contract");
    assert_eq!(saved.scan.as_deref(), Some(path.as_str()));
}

#[tokio::test]
async fn upload_problems_appear_under_the_field() {
    let app = files_app().await;
    let (_, admin) = user(&app, ADMIN).await;

    // Not a PNG: the form comes back with the error and the values typed.
    let html = upload(
        &admin,
        "/admin/resources/documents",
        multipart(
            &[("name", "Notes")],
            Some(("scan", "notes.png", b"just some text")),
        ),
    )
    .await
    .assert_status(400)
    .text()
    .replace("&#x2f;", "/");
    assert!(
        html.contains("Scan must be a file of type: image/png"),
        "{html}"
    );
    assert!(html.contains("value=\"Notes\""), "{html}");

    // Over `max_size`.
    let html = upload(
        &admin,
        "/admin/resources/documents",
        multipart(&[("name", "Big")], Some(("scan", "big.png", &png(65)))),
    )
    .await
    .assert_status(400)
    .text();
    assert!(
        html.contains("Scan must not be larger than 64 bytes"),
        "{html}"
    );

    let db = app.get::<navree_db::Db>().await;
    assert!(
        Document::query().count(&*db).await.unwrap() == 0,
        "nothing is saved"
    );
}

#[tokio::test]
async fn the_api_accepts_only_paths_helm_stored() {
    let app = files_app().await;
    let (_, admin) = user(&app, ADMIN).await;
    admin
        .post("/admin/api/resources/documents")
        .json(&json!({ "name": "Escape", "scan": "../../secrets.png" }))
        .send()
        .await
        .assert_error(400, None)
        .assert_json_path("errors.scan", json!(["Scan is invalid"]));
    let stored = format!("documents/{}.png", "a".repeat(40));
    admin
        .post("/admin/api/resources/documents")
        .json(&json!({ "name": "Kept", "scan": stored }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("data.values.scan", json!(stored));
}
