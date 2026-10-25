//! `StorageModule` wiring: configuration, exports, signing keys and error mapping.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Multipart, Path};
use axum::http::{Method, Request};
use chrono::{TimeZone, Utc};
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::HealthStatus;
use navree_crypto::{CryptoModule, CryptoOptions, generate_key};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, Navree, KernelError, Module, ModuleBuilder, Resolver};
use navree_storage::{
    Disk, DiskConfig, Storage, StorageError, StorageModule, StorageOptions, UploadRules,
    store_upload,
};
use serde_json::json;

use crate::support::{TestApp, boot_error, options};

fn storage_section(root: &std::path::Path) -> serde_json::Value {
    json!({
        "default": "files",
        "base_url": "https://api.example.com",
        "disks": {
            "files": { "driver": "local", "root": root, "public_url": "https://cdn.example.com" },
            "scratch": { "driver": "memory" },
        }
    })
}

#[tokio::test]
async fn from_config_reads_the_storage_section() {
    let root = tempfile::tempdir().unwrap();
    let files = root.path().join("files");
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("storage", storage_section(&files)),
        ))
        .module(StorageModule::from_config())
        .boot()
        .await
        .unwrap();

    let storage = app.ctx().get::<Storage>().await.unwrap();
    assert_eq!(storage.default_disk().name(), "files");
    assert!(files.is_dir(), "the local root is created at boot");
    storage
        .disk("scratch")
        .unwrap()
        .put("a.txt", "x")
        .await
        .unwrap();

    // The default disk is also provided on its own.
    let disk = app.ctx().get::<Disk>().await.unwrap();
    assert_eq!(disk.name(), "files");
    assert_eq!(
        disk.url("a b.txt").unwrap(),
        "https://cdn.example.com/a%20b.txt"
    );
    assert!(
        disk.temporary_url("a.txt", Utc::now())
            .unwrap()
            .starts_with("https://api.example.com/storage/files/a.txt?")
    );
}

#[tokio::test]
async fn invalid_storage_config_fails_the_boot() {
    let root = tempfile::tempdir().unwrap();
    let mut section = storage_section(root.path());
    section["default"] = json!("s3");
    let error = boot_error(
        Navree::app()
            .module(ConfigModule::for_root(
                ConfigSources::new().set("storage", section),
            ))
            .module(StorageModule),
    )
    .await;
    assert!(error.contains("storage"), "{error}");
    assert!(error.contains("`s3`"), "{error}");
}

#[test]
fn default_options_have_one_local_disk() {
    let options = StorageOptions::default();
    assert_eq!(options.default, "local");
    assert_eq!(
        options.disks.get("local"),
        Some(&DiskConfig::local("storage/app"))
    );
    assert_eq!(options.base_url, None);
    // A missing section deserializes to the defaults.
    let parsed: StorageOptions = serde_json::from_value(json!({})).unwrap();
    assert_eq!(parsed, options);
    let memory: DiskConfig = serde_json::from_value(json!({ "driver": "memory" })).unwrap();
    assert_eq!(memory, DiskConfig::Memory);
}

/// A temporary URL for a fixed path and expiry, from a freshly booted app.
async fn signed_url(app_key: Option<&str>, with_crypto: bool) -> String {
    let root = tempfile::tempdir().unwrap();
    let mut sources = ConfigSources::new();
    if let Some(key) = app_key {
        sources = sources.set("app.key", key);
    }
    let mut builder = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(StorageModule::for_root(options(root.path())));
    if with_crypto {
        builder = builder.module(CryptoModule::from_config());
    }
    let app = builder.boot().await.unwrap();
    let expires = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap();
    app.ctx()
        .get::<Disk>()
        .await
        .unwrap()
        .temporary_url("a.txt", expires)
        .unwrap()
}

#[tokio::test]
async fn temporary_urls_are_keyed_by_the_app_key() {
    let key = generate_key();
    let first = signed_url(Some(&key), false).await;
    assert_eq!(
        signed_url(Some(&key), false).await,
        first,
        "stable across boots"
    );
    assert_eq!(
        signed_url(Some(&key), true).await,
        first,
        "the same with CryptoModule"
    );
    assert_ne!(signed_url(Some(&generate_key()), false).await, first);
    // Without an app key every boot gets its own random key.
    assert_ne!(signed_url(None, false).await, signed_url(None, false).await);
}

#[tokio::test]
async fn crypto_module_key_signs_without_config() {
    let root = tempfile::tempdir().unwrap();
    let key = generate_key();
    let boot = || async {
        Navree::app()
            .module(CryptoModule::for_root(CryptoOptions::new(key.clone())))
            .module(StorageModule::for_root(options(root.path())))
            .boot()
            .await
            .unwrap()
    };
    let expires = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap();
    let first = boot().await;
    let second = boot().await;
    let url = |app: &navree_kernel::App| {
        let ctx = app.ctx().clone();
        async move {
            ctx.get::<Disk>()
                .await
                .unwrap()
                .temporary_url("a.txt", expires)
                .unwrap()
        }
    };
    assert_eq!(url(&first).await, url(&second).await);
}

#[tokio::test]
async fn malformed_app_key_fails_the_boot() {
    let root = tempfile::tempdir().unwrap();
    let error = boot_error(
        Navree::app()
            .module(ConfigModule::for_root(
                ConfigSources::new().set("app.key", "not-a-key"),
            ))
            .module(StorageModule::for_root(options(root.path()))),
    )
    .await;
    assert!(error.contains("app.key"), "{error}");
    assert!(
        !error.contains("not-a-key"),
        "the key is not echoed: {error}"
    );
}

/// Serves `GET /files/{*path}` from the default disk with `?`.
struct FilesController {
    disk: Arc<Disk>,
}

impl Injectable for FilesController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                disk: r.get::<Disk>().await?,
            })
        })
    }
}

impl Controller for FilesController {
    fn prefix(&self) -> &'static str {
        "!/files"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/{*path}", move |Path(path): Path<String>| {
                let disk = self.disk.clone();
                async move {
                    let bytes = disk.get(&path).await?;
                    Ok::<_, navree_http::AppError>(bytes)
                }
            })
            .public(),
        ]
    }
}

struct FilesModule;

impl Module for FilesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<FilesController>();
    }
}

#[tokio::test]
async fn storage_errors_map_to_http_statuses() {
    let root = tempfile::tempdir().unwrap();
    let app = TestApp::boot(
        Navree::app()
            .module(StorageModule::for_root(options(root.path())))
            .module(FilesModule),
    )
    .await;
    let disk = app.app.ctx().get::<Disk>().await.unwrap();
    disk.put("hello.txt", "hi").await.unwrap();

    assert_eq!(app.get("/files/hello.txt").await.text(), "hi");
    app.get("/files/missing.txt").await.error(404);
    let body = app.get("/files/a%5C..%5C..%5Csecret.txt").await.error(400);
    assert_eq!(body["message"], "Invalid file path.");
}

#[tokio::test]
async fn health_check_probes_every_disk() {
    let root = tempfile::tempdir().unwrap();
    let app = Navree::app()
        .module(StorageModule::for_root(options(root.path())))
        .boot()
        .await
        .unwrap();
    let checks = app
        .ctx()
        .get_all::<dyn navree_contracts::HealthCheck>()
        .await
        .unwrap();
    let storage = checks
        .iter()
        .find(|check| check.name() == "storage")
        .expect("a storage health check");
    assert_eq!(storage.check().await, HealthStatus::Up);
    // The probe cleans up after itself.
    let disk = app.ctx().get::<Storage>().await.unwrap();
    for name in ["local", "memory"] {
        assert!(disk.disk(name).unwrap().list("").await.unwrap().is_empty());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);

    // A local disk whose root disappeared is down.
    drop(app);
    let gone = tempfile::tempdir().unwrap();
    let path = gone.path().join("root");
    let app = Navree::app()
        .module(StorageModule::for_root(options(&path)))
        .boot()
        .await
        .unwrap();
    std::fs::remove_dir_all(&path).unwrap();
    std::fs::write(&path, "now a file").unwrap();
    let checks = app
        .ctx()
        .get_all::<dyn navree_contracts::HealthCheck>()
        .await
        .unwrap();
    let status = checks
        .iter()
        .find(|check| check.name() == "storage")
        .unwrap()
        .check()
        .await;
    assert!(
        matches!(&status, HealthStatus::Down(reason) if reason.contains("`local`")),
        "{status:?}"
    );
}

/// `POST /upload`: stores the first multipart field on the default disk.
struct UploadController {
    disk: Arc<Disk>,
}

impl Injectable for UploadController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                disk: r.get::<Disk>().await?,
            })
        })
    }
}

impl Controller for UploadController {
    fn prefix(&self) -> &'static str {
        "!/upload"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "", move |mut form: Multipart| {
                let disk = self.disk.clone();
                async move {
                    let field = form
                        .next_field()
                        .await?
                        .ok_or_else(|| AppError::bad_request("no file"))?;
                    let rules = UploadRules::new(1024 * 1024, ["image/png"]);
                    Ok::<_, AppError>(Json(store_upload(field, &disk, "up", rules).await?))
                }
            })
            .public()
            .body_limit(8 * 1024),
        ]
    }
}

struct UploadModule;

impl Module for UploadModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<UploadController>();
    }
}

fn multipart_request(contents: &[u8]) -> Request<Body> {
    let mut body = b"--XyZ\r\nContent-Disposition: form-data; name=\"file\"; \
        filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\n"
        .to_vec();
    body.extend_from_slice(contents);
    body.extend_from_slice(b"\r\n--XyZ--\r\n");
    Request::post("/upload")
        .header("content-type", "multipart/form-data; boundary=XyZ")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn multipart_errors_in_handlers_keep_their_status() {
    let root = tempfile::tempdir().unwrap();
    let app = TestApp::boot(
        Navree::app()
            .module(StorageModule::for_root(options(root.path())))
            .module(UploadModule),
    )
    .await;
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(1000, 0);

    let ok = app.send(multipart_request(&png)).await;
    assert_eq!(ok.status, 200, "{}", ok.text());

    // Over the route's body limit: multer fails in `next_field()?`.
    png.resize(64 * 1024, 0);
    let body = app.send(multipart_request(&png)).await.error(413);
    assert_eq!(body["error"], "Payload Too Large");

    // A body that ends early.
    let truncated = Request::post("/upload")
        .header("content-type", "multipart/form-data; boundary=XyZ")
        .body(Body::from(
            "--XyZ\r\nContent-Disposition: form-data; name=\"f\"",
        ))
        .unwrap();
    app.send(truncated).await.error(400);
}

#[test]
fn storage_error_messages_name_disk_and_path() {
    let error = StorageError::NotFound {
        disk: "local".into(),
        path: "a/b.txt".into(),
    };
    assert_eq!(
        error.to_string(),
        "file `a/b.txt` not found on disk `local`"
    );
}
