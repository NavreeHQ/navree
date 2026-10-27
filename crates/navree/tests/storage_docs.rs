//! The examples of `content/docs/digging-deeper/storage.mdx`, written as an
//! app writes them (the facade, the prelude and the macros), compiled and
//! run.
#![cfg(all(feature = "storage", feature = "testing"))]

use chrono::{Duration, Utc};
use navree::http::CurrentPrincipal;
use navree::prelude::*;
use navree::storage::{
    Disk, DiskConfig, PresignedUpload, Storage, StorageError, StorageOptions, StoredFile,
    SyncOptions, UploadRules, upload_path,
};
use navree::testing::{TestApp, principal};
use serde::Deserialize;
use uuid::Uuid;

// --- Getting a disk ---------------------------------------------------------

/// Publishes quarterly reports.
#[injectable]
pub struct ReportService {
    /// The default disk.
    disk: Arc<Disk>,
    /// Every disk, by name.
    storage: Arc<Storage>,
}

impl ReportService {
    /// Stores the quarter's report and returns a link valid for 30 minutes.
    pub async fn publish(&self, quarter: &str, csv: String) -> Result<String, StorageError> {
        let path = format!("reports/{quarter}.csv");
        self.disk.put(&path, csv).await?;
        self.disk
            .temporary_url(&path, Utc::now() + Duration::minutes(30))
            .await
    }

    /// Copies a report to the `archive` disk without loading it into memory.
    pub async fn archive(&self, quarter: &str) -> Result<u64, StorageError> {
        let path = format!("reports/{quarter}.csv");
        let contents = self.disk.get_stream(&path).await?;
        self.storage
            .disk("archive")?
            .put_stream(&path, contents)
            .await
    }
}

/// Reports.
#[module(providers = [ReportService])]
pub struct ReportsModule;

// --- Direct uploads ----------------------------------------------------------

/// What the browser says about the file it is about to send.
#[derive(Debug, Deserialize)]
pub struct NewUpload {
    content_type: String,
    size: u64,
}

/// The path the browser uploaded to.
#[derive(Debug, Deserialize)]
pub struct FinishedUpload {
    path: String,
}

/// Documents: PDFs and images up to 20 MiB.
fn document_rules() -> UploadRules {
    UploadRules::new(20 * 1024 * 1024, ["application/pdf", "image/*"])
}

/// Direct uploads of documents.
#[injectable]
pub struct DocumentUploadController {
    storage: Arc<Storage>,
}

#[controller("/documents/uploads", version = "1")]
impl DocumentUploadController {
    /// Signs a link the browser uploads one file to.
    #[post("/")]
    #[authenticated]
    async fn start(
        &self,
        CurrentPrincipal(me): CurrentPrincipal,
        Json(file): Json<NewUpload>,
    ) -> Result<Json<PresignedUpload>> {
        let disk = self.storage.disk("documents")?;
        let dir = format!("incoming/{}", me.user_id);
        let path = upload_path(&dir, &file.content_type);
        let rules = document_rules();
        let upload = disk
            .presigned_put(&path, Duration::minutes(15), &rules)
            .content_type(file.content_type)
            .size(file.size)
            .await?;
        Ok(Json(upload))
    }

    /// Checks the file the browser uploaded.
    #[post("/complete")]
    #[authenticated]
    async fn complete(
        &self,
        CurrentPrincipal(me): CurrentPrincipal,
        Json(done): Json<FinishedUpload>,
    ) -> Result<Json<StoredFile>> {
        // Only the caller's own uploads.
        if !done.path.starts_with(&format!("incoming/{}/", me.user_id)) {
            return Err(AppError::forbidden());
        }
        let disk = self.storage.disk("documents")?;
        let file = disk.accept_upload(&done.path, &document_rules()).await?;
        Ok(Json(file))
    }
}

/// Documents.
#[module(controllers = [DocumentUploadController])]
pub struct DocumentsModule;

// --- The test app --------------------------------------------------------------

fn options() -> StorageOptions {
    StorageOptions {
        default: "memory".into(),
        disks: [
            ("memory".to_owned(), DiskConfig::Memory),
            ("archive".to_owned(), DiskConfig::Memory),
            ("documents".to_owned(), DiskConfig::Memory),
        ]
        .into(),
        ..StorageOptions::default()
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .config("app.key", navree::crypto::generate_key())
        .module(StorageModule::for_root(options()))
        .module(ReportsModule)
        .module(DocumentsModule)
        .build()
        .await
}

/// A PNG header padded to `len` bytes.
fn png(len: usize) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.resize(len, 0);
    png
}

#[tokio::test]
async fn publishing_a_report_stores_it() {
    let app = app().await;

    let link = app
        .get::<ReportService>()
        .await
        .publish("2026-q3", "a,b\n".into())
        .await
        .unwrap();

    assert!(link.starts_with("/storage/memory/reports/2026-q3.csv?expires="));
    let disk = app.get::<Storage>().await.default_disk();
    assert_eq!(disk.get("reports/2026-q3.csv").await.unwrap(), "a,b\n");
    let download = app.client().get(&link).send().await.assert_status(200);
    assert_eq!(download.text(), "a,b\n");

    let copied = app
        .get::<ReportService>()
        .await
        .archive("2026-q3")
        .await
        .unwrap();
    assert_eq!(copied, 4);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_browser_uploads_a_document_directly() {
    let app = app().await;
    let me = Uuid::now_v7();
    let client = app.acting_as(principal(me));
    let file = png(2_000);

    // 1. The browser asks where to send the file.
    let upload: PresignedUpload = client
        .post("/api/v1/documents/uploads")
        .json(&serde_json::json!({ "content_type": "image/png", "size": file.len() }))
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(upload.method, "PUT");
    assert!(upload.path.starts_with(&format!("incoming/{me}/")));
    assert!(upload.path.ends_with(".png"));
    assert_eq!(upload.headers["content-type"], "image/png");

    // 2. It sends the file there, with the headers it was given.
    let mut put = app.client().put(&upload.url).body(file.clone());
    for (name, value) in &upload.headers {
        put = put.header(name.as_str(), value.as_str());
    }
    put.send().await.assert_status(200);

    // 3. It reports the upload as done; the app checks the file.
    let stored: StoredFile = client
        .post("/api/v1/documents/uploads/complete")
        .json(&serde_json::json!({ "path": upload.path }))
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(stored.mime, "image/png");
    assert_eq!(stored.size, file.len() as u64);

    // Someone else cannot accept it.
    app.acting_as(principal(Uuid::now_v7()))
        .post("/api/v1/documents/uploads/complete")
        .json(&serde_json::json!({ "path": upload.path }))
        .send()
        .await
        .assert_status(403);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_disallowed_upload_is_refused_and_deleted() {
    let app = app().await;
    let client = app.acting_as(principal(Uuid::now_v7()));

    // A type the rules do not allow is refused before anything is sent.
    client
        .post("/api/v1/documents/uploads")
        .json(&serde_json::json!({ "content_type": "text/html", "size": 10 }))
        .send()
        .await
        .assert_status(400);
    // So is a file over the limit.
    client
        .post("/api/v1/documents/uploads")
        .json(&serde_json::json!({ "content_type": "image/png", "size": 30 * 1024 * 1024 }))
        .send()
        .await
        .assert_status(413);

    // A file that claims to be an image but is not is deleted on completion.
    let body = b"<script>alert(1)</script>".to_vec();
    let upload: PresignedUpload = client
        .post("/api/v1/documents/uploads")
        .json(&serde_json::json!({ "content_type": "image/png", "size": body.len() }))
        .send()
        .await
        .assert_status(200)
        .json();
    app.client()
        .put(&upload.url)
        .header("content-type", "image/png")
        .body(body)
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/v1/documents/uploads/complete")
        .json(&serde_json::json!({ "path": upload.path }))
        .send()
        .await
        .assert_status(400);
    let documents = app.get::<Storage>().await.disk("documents").unwrap();
    assert!(!documents.exists(&upload.path).await.unwrap());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn disks_are_synced_by_the_command_and_in_code() {
    let app = app().await;
    let storage = app.get::<Storage>().await;
    let memory = storage.disk("memory").unwrap();
    memory.put("avatars/a.png", png(100)).await.unwrap();
    memory.put("avatars/b.png", png(200)).await.unwrap();
    memory.put("reports/q1.csv", "a,b\n").await.unwrap();

    // A dry run changes nothing.
    app.run_command("storage:sync", &["memory", "archive", "--dry-run"])
        .await
        .unwrap();
    assert!(
        storage
            .disk("archive")
            .unwrap()
            .list("")
            .await
            .unwrap()
            .is_empty()
    );

    let report = storage
        .sync(
            "memory",
            "archive",
            &SyncOptions {
                prefix: "avatars".into(),
                ..SyncOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(report.copied, ["avatars/a.png", "avatars/b.png"]);
    assert_eq!(report.bytes, 300);

    app.run_command("storage:sync", &["memory", "archive"])
        .await
        .unwrap();
    assert_eq!(
        storage.disk("archive").unwrap().list("").await.unwrap(),
        ["avatars/a.png", "avatars/b.png", "reports/q1.csv"]
    );
    app.shutdown().await.unwrap();
}

/// The configuration of an S3-compatible disk, and the links it signs,
/// without any request to the store.
#[cfg(feature = "storage-s3")]
#[tokio::test]
async fn an_s3_disk_signs_links_for_the_bucket() {
    let app = TestApp::builder()
        .config("app.key", navree::crypto::generate_key())
        .config("storage.default", "uploads")
        .config("storage.disks.uploads.driver", "s3")
        .config("storage.disks.uploads.bucket", "acme-uploads")
        .config("storage.disks.uploads.endpoint", "http://127.0.0.1:9")
        .config("storage.disks.uploads.region", "us-east-1")
        .config("storage.disks.uploads.access_key", "navree")
        .config("storage.disks.uploads.secret_key", "navree-secret")
        .module(StorageModule::from_config())
        .build()
        .await;
    let disk = app.get::<Disk>().await;
    assert_eq!(disk.driver(), "s3");

    let link = disk
        .temporary_url("reports/q1.csv", Utc::now() + Duration::minutes(5))
        .await
        .unwrap();
    assert!(
        link.starts_with("http://127.0.0.1:9/acme-uploads/reports/q1.csv?X-Amz-Algorithm="),
        "{link}"
    );

    let rules = UploadRules::new(1024, ["image/png"]);
    let upload = disk
        .presigned_put("avatars/a.png", Duration::minutes(10), &rules)
        .content_type("image/png")
        .size(512)
        .await
        .unwrap();
    assert!(
        upload
            .url
            .contains("X-Amz-SignedHeaders=content-type%3Bhost"),
        "{}",
        upload.url
    );
    assert_eq!(upload.headers["content-type"], "image/png");

    // Links last at most 7 days on S3.
    let too_long = disk
        .temporary_url("reports/q1.csv", Utc::now() + Duration::days(8))
        .await;
    assert!(matches!(too_long, Err(StorageError::Signing { .. })));
    app.shutdown().await.unwrap();
}
