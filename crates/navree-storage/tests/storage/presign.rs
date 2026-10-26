//! Presigned uploads to local and memory disks: the app's own route
//! `PUT /storage/{disk}/{*path}`, and rejected uploads in handlers.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use axum::body::Body;
use axum::extract::Path;
use axum::http::{Method, Request};
use chrono::{Duration, Utc};
use futures_util::stream;
use navree_contracts::{Clock, FixedClock};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver};
use navree_storage::{
    Disk, DiskConfig, PresignedUpload, Storage, StorageModule, StoredFile, UploadRules,
};
use serde_json::json;

use crate::conformance::png;
use crate::support::{SIGNING_KEY, TestApp, options, single_disk};

async fn boot(root: &std::path::Path, clock: Arc<FixedClock>) -> TestApp {
    TestApp::boot(
        Navree::app()
            .module(StorageModule::for_root(options(root)))
            .override_value::<dyn Clock>(clock),
    )
    .await
}

/// Sends `body` to a presigned upload with its headers.
async fn upload(app: &TestApp, upload: &PresignedUpload, body: Vec<u8>) -> crate::support::TestResponse {
    let mut request = Request::put(&upload.url).header("content-length", body.len());
    for (name, value) in &upload.headers {
        request = request.header(name, value);
    }
    app.send(request.body(Body::from(body)).unwrap()).await
}

#[tokio::test]
async fn the_route_checks_size_type_and_expiry() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = boot(root.path(), clock.clone()).await;
    let storage = app.app.ctx().get::<Storage>().await.unwrap();
    let rules = UploadRules::new(1000, ["image/png"]);

    for name in ["local", "memory"] {
        let disk = storage.disk(name).unwrap();
        let signed = disk
            .presigned_put("a.png", Duration::minutes(5), &rules)
            .content_type("image/png")
            .size(100)
            .await
            .unwrap();
        assert!(signed.url.starts_with(&format!("/storage/{name}/a.png?expires=")), "{}", signed.url);
        assert!(signed.url.contains("&type=image%2Fpng&max=1000&size=100&signature="), "{}", signed.url);

        // Another size than declared.
        let body = upload(&app, &signed, png(101)).await.error(400);
        assert_eq!(body["message"], "The upload must be exactly 100 bytes long.");
        let body = upload(&app, &signed, png(99)).await.error(400);
        assert_eq!(body["message"], "The upload must be exactly 100 bytes long.");
        assert!(!disk.exists("a.png").await.unwrap(), "{name}");

        // The declared size, the declared type.
        let ok = upload(&app, &signed, png(100)).await;
        assert_eq!(ok.status, 200, "{name}: {}", ok.text());
        assert_eq!(disk.size("a.png").await.unwrap(), 100);
    }

    // Over the limit: refused from Content-Length, or while streaming.
    let disk = storage.disk("memory").unwrap();
    let signed = disk
        .presigned_put("big.png", Duration::minutes(5), &rules)
        .content_type("image/png")
        .await
        .unwrap();
    let body = upload(&app, &signed, png(1001)).await.error(413);
    assert_eq!(body["message"], "The file must not be larger than 1000 bytes.");
    let chunks: Vec<Result<Vec<u8>, std::io::Error>> = vec![Ok(png(600)), Ok(vec![0; 600])];
    let streamed = Request::put(&signed.url)
        .header("content-type", "image/png")
        .body(Body::from_stream(stream::iter(chunks)))
        .unwrap();
    app.send(streamed).await.error(413);
    assert!(!disk.exists("big.png").await.unwrap());

    // Expired.
    clock.advance(StdDuration::from_secs(5 * 60));
    let body = upload(&app, &signed, png(10)).await.error(403);
    assert_eq!(body["message"], "The link has expired.");
}

#[tokio::test]
async fn forged_links_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let app = boot(root.path(), Arc::new(FixedClock::new(Utc::now()))).await;
    let storage = app.app.ctx().get::<Storage>().await.unwrap();
    let disk = storage.disk("local").unwrap();
    let rules = UploadRules::new(1000, Vec::<String>::new());
    let signed = disk
        .presigned_put("doc.txt", Duration::minutes(5), &rules)
        .await
        .unwrap();
    assert!(signed.headers.is_empty(), "no type declared, no header: {:?}", signed.headers);
    assert!(!signed.url.contains("type="), "{}", signed.url);

    let forged = |from: &str, to: &str| PresignedUpload {
        url: signed.url.replacen(from, to, 1),
        ..signed.clone()
    };
    for tampered in [
        forged("/doc.txt", "/other.txt"),
        forged("/local/", "/memory/"),
        forged("/local/", "/nowhere/"),
        forged("max=1000", "max=100000"),
        forged("&signature=", "&size=4&signature="),
    ] {
        let response = upload(&app, &tampered, b"text".to_vec()).await;
        assert_eq!(response.status, 403, "{}: {}", tampered.url, response.text());
    }
    // A download link is not an upload link.
    let download = disk
        .temporary_url("doc.txt", Utc::now() + Duration::minutes(5))
        .await
        .unwrap();
    let response = app
        .send(Request::put(&download).body(Body::from("x")).unwrap())
        .await;
    assert_eq!(response.status, 403);
    assert!(!disk.exists("doc.txt").await.unwrap());
    assert!(!disk.exists("other.txt").await.unwrap());

    // The real link works, with any type when the rules allow any.
    let mut with_type = signed.clone();
    with_type
        .headers
        .insert("content-type".into(), "application/x-anything".into());
    assert_eq!(upload(&app, &with_type, b"text".to_vec()).await.status, 200);
    assert_eq!(disk.get("doc.txt").await.unwrap(), "text");
}

#[tokio::test]
async fn links_use_the_base_url_and_serialize_as_json() {
    let mut options = single_disk("files", DiskConfig::Memory);
    options.base_url = Some("https://api.example.com/".into());
    let storage = Storage::new(&options, SIGNING_KEY).unwrap();
    let rules = UploadRules::new(10, ["text/csv"]);
    let upload = storage
        .default_disk()
        .presigned_put("/exports//q1.csv", Duration::minutes(10), &rules)
        .content_type("text/csv")
        .await
        .unwrap();
    assert!(
        upload.url.starts_with("https://api.example.com/storage/files/exports/q1.csv?expires="),
        "{}",
        upload.url
    );
    let value = serde_json::to_value(&upload).unwrap();
    assert_eq!(value["disk"], "files");
    assert_eq!(value["path"], "exports/q1.csv");
    assert_eq!(value["method"], "PUT");
    assert_eq!(value["headers"], json!({ "content-type": "text/csv" }));
    assert!(value["expires_at"].is_string());

    // A non-positive lifetime or a malformed type cannot be signed.
    let disk = storage.default_disk();
    for result in [
        disk.presigned_put("a.csv", Duration::zero(), &rules)
            .content_type("text/csv")
            .await,
        disk.presigned_put("a.csv", Duration::minutes(1), &rules)
            .content_type("text/csv\r\nx-injected: 1")
            .await,
    ] {
        assert!(
            matches!(result, Err(navree_storage::StorageError::Signing { .. })),
            "{result:?}"
        );
    }
}

/// `POST /presign/{*path}` signs, `POST /accept/{*path}` accepts, both with `?`.
struct UploadsController {
    disk: Arc<Disk>,
}

impl Injectable for UploadsController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                disk: r.get::<Disk>().await?,
            })
        })
    }
}

fn rules() -> UploadRules {
    UploadRules::new(1000, ["image/png"])
}

impl Controller for UploadsController {
    fn prefix(&self) -> &'static str {
        "!/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let presign = self.clone();
        vec![
            RouteDef::on(
                Method::POST,
                "/presign/{size}",
                move |Path(size): Path<u64>| {
                    let disk = presign.disk.clone();
                    async move {
                        let rules = rules();
                        let upload = disk
                            .presigned_put("x.png", Duration::minutes(1), &rules)
                            .content_type("image/png")
                            .size(size)
                            .await?;
                        Ok::<_, AppError>(Json(upload))
                    }
                },
            )
            .public(),
            RouteDef::on(Method::POST, "/accept/{*path}", move |Path(path): Path<String>| {
                let disk = self.disk.clone();
                async move {
                    let stored: StoredFile = disk.accept_upload(&path, &rules()).await?;
                    Ok::<_, AppError>(Json(stored))
                }
            })
            .public(),
        ]
    }
}

struct UploadsModule;

impl Module for UploadsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<UploadsController>();
    }
}

#[tokio::test]
async fn rejections_map_to_400_and_413_in_handlers() {
    let app = TestApp::boot(
        Navree::app()
            .module(StorageModule::for_root(single_disk("memory", DiskConfig::Memory)))
            .module(UploadsModule),
    )
    .await;
    let post = |uri: &str| Request::post(uri).body(Body::empty()).unwrap();

    let ok = app.send(post("/presign/10")).await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    let body = app.send(post("/presign/1001")).await.error(413);
    assert_eq!(body["message"], "The file must not be larger than 1000 bytes.");
    let body = app.send(post("/presign/0")).await.error(400);
    assert_eq!(body["message"], "The file should not be empty.");

    let disk = app.app.ctx().get::<Disk>().await.unwrap();
    disk.put("up/fake.png", "<svg/>").await.unwrap();
    let body = app.send(post("/accept/up/fake.png")).await.error(400);
    assert_eq!(
        body["message"],
        "The file must be of type: image/png (it is text/plain)."
    );
    app.send(post("/accept/up/missing.png")).await.error(404);
    disk.put("up/real.png", png(500)).await.unwrap();
    let ok = app.send(post("/accept/up/real.png")).await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    let stored: serde_json::Value = serde_json::from_slice(&ok.body).unwrap();
    assert_eq!(stored["mime"], "image/png");
    assert_eq!(stored["size"], 500);
}
