//! `DocumentsModule` in an app: the service, responses, error mapping,
//! embedded templates, queued renders on a worker and the commands.

// Most of these tests need the Typst engine.
#![cfg_attr(not(feature = "typst"), allow(unused_imports, dead_code))]

use std::sync::Arc;

use http::Method;
use navree_documents::{
    Documents, DocumentsModule, DocumentsOptions, ModuleBuilderDocumentsExt, Pdf, QueuedDocument,
    Render,
};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_storage::{DiskConfig, StorageModule, StorageOptions};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::{Value, json};

use crate::support::fixtures_dir;

/// The routes the tests call.
struct DocumentsController(Arc<Documents>);

impl Injectable for DocumentsController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(DocumentsController(r.get::<Documents>().await?)) })
    }
}

impl Controller for DocumentsController {
    fn prefix(&self) -> &'static str {
        "!/documents"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let download = self.clone();
        let inline = self.clone();
        let broken = self.clone();
        let queue = self.clone();
        let status = self.clone();
        let embedded = self.clone();
        vec![
            RouteDef::on(Method::GET, "/certificate", move || {
                let this = download.clone();
                async move {
                    let pdf = this
                        .0
                        .pdf(
                            "certificates/classic",
                            json!({"name": "Ada Lovelace", "course": "Engines", "completed_on": "today"}),
                        )
                        .await?;
                    Ok::<_, AppError>(pdf.download("certificate-ada.pdf"))
                }
            })
            .public(),
            RouteDef::on(Method::GET, "/inline", move || {
                let this = inline.clone();
                async move {
                    let pdf: Pdf = this.0.render(&Render::typst("Inline")).await?;
                    Ok::<_, AppError>(pdf)
                }
            })
            .public(),
            RouteDef::on(Method::GET, "/broken", move || {
                let this = broken.clone();
                async move {
                    let pdf = this.0.pdf("broken", json!({"name": "x"})).await?;
                    Ok::<_, AppError>(pdf)
                }
            })
            .public(),
            RouteDef::on(Method::POST, "/exports", move |Json(body): Json<Value>| {
                let this = queue.clone();
                async move {
                    let template = body["template"].as_str().unwrap_or_default().to_owned();
                    let queued = this
                        .0
                        .queue(
                            Render::template(template).data(body["data"].clone()),
                            body["path"].as_str().unwrap_or_default(),
                        )
                        .await?;
                    Ok::<_, AppError>(Json(queued))
                }
            })
            .public(),
            RouteDef::on(Method::POST, "/status", move |Json(queued): Json<QueuedDocument>| {
                let this = status.clone();
                async move { Ok::<_, AppError>(Json(this.0.status(&queued).await?)) }
            })
            .public(),
            RouteDef::on(Method::GET, "/embedded", move || {
                let this = embedded.clone();
                async move {
                    let pdf = this.0.pdf("receipts/simple", json!({"name": "Grace"})).await?;
                    Ok::<_, AppError>(pdf)
                }
            })
            .public(),
        ]
    }
}

/// A feature module that ships a template.
struct ReceiptsModule;

impl Module for ReceiptsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DocumentsController>().document_files([
            (
                "receipts/simple.typ",
                &b"Receipt for #json(\"/data.json\").name"[..],
            ),
            // Not a valid path: ignored.
            ("../outside.typ", &b"x"[..]),
        ]);
    }
}

fn storage() -> StorageOptions {
    let mut options = StorageOptions::default();
    options.disks.clear();
    options.disks.insert("local".into(), DiskConfig::Memory);
    options
}

fn app() -> TestAppBuilder {
    TestApp::builder()
        .config(
            "documents.templates",
            fixtures_dir().to_string_lossy().into_owned(),
        )
        .module(StorageModule::for_root(storage()))
        .module(DocumentsModule::from_config())
        .module(ReceiptsModule)
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn handlers_render_and_download_documents() {
    let app = app().build().await;
    let response = app
        .client()
        .get("/documents/certificate")
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("content-type"), Some("application/pdf"));
    assert_eq!(
        response.header("content-disposition"),
        Some("attachment; filename=\"certificate-ada.pdf\"")
    );
    assert_eq!(response.header("cache-control"), Some("private, no-store"));
    let pdf = Pdf::from_bytes(response.bytes().clone()).unwrap();
    assert!(
        pdf.text()
            .unwrap()
            .contains("Ada Lovelace completed Engines on today.")
    );

    let response = app
        .client()
        .get("/documents/inline")
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.header("content-disposition"), Some("inline"));
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn template_errors_are_500s_without_details() {
    let app = app().build().await;
    let response = app
        .client()
        .get("/documents/broken")
        .send()
        .await
        .assert_error(500, None);
    // The template's file and message stay in the logs.
    assert!(
        !response.text().contains("broken.typ"),
        "{}",
        response.text()
    );
}

#[cfg(feature = "chrome")]
#[tokio::test]
async fn an_unavailable_engine_is_a_503() {
    let app = app()
        .config("documents.chrome.path", "/no/such/chrome")
        .build()
        .await;
    // invoices/show exists only as HTML, so Chrome renders it.
    let documents = app.get::<Documents>().await;
    let error = documents
        .pdf(
            "invoices/show",
            json!({"number": 1, "customer": "Lin", "total_cents": 100, "currency": "USD"}),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            navree_documents::DocumentsError::EngineUnavailable {
                engine: "chrome",
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.status(), http::StatusCode::SERVICE_UNAVAILABLE);
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn modules_embed_templates() {
    let app = app().build().await;
    let response = app
        .client()
        .get("/documents/embedded")
        .send()
        .await
        .assert_status(200);
    let pdf = Pdf::from_bytes(response.bytes().clone()).unwrap();
    assert!(pdf.text().unwrap().contains("Receipt for Grace"));
    assert!(
        !app.get::<Documents>()
            .await
            .templates()
            .exists("../outside.typ")
    );
}

#[tokio::test]
async fn an_unknown_documents_disk_fails_the_boot() {
    let error = app()
        .config("documents.disk", "missing")
        .try_build()
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(error.to_string().contains("documents.disk"), "{error}");
}

#[test]
fn for_root_takes_the_options() {
    let options = DocumentsOptions {
        engine: navree_documents::EngineKind::Chrome,
        ..DocumentsOptions::default()
    };
    let _ = DocumentsModule::for_root(options);
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn queued_renders_run_on_a_worker() {
    use navree_documents::DocumentStatus;
    use navree_queue::{Queue, QueueDriver, QueueModule, QueueOptions};

    let app = app()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
        .build()
        .await;
    let queued: QueuedDocument = app
        .client()
        .post("/documents/exports")
        .json(&json!({
            "template": "certificates/classic",
            "data": {"name": "Ada", "course": "Engines", "completed_on": "today"},
            "path": "exports/ada.pdf",
        }))
        .send()
        .await
        .assert_status(200)
        .json();
    assert!(queued.job_id.is_some());
    let status = |queued: QueuedDocument| {
        let client = app.client();
        async move {
            client
                .post("/documents/status")
                .json(&queued)
                .send()
                .await
                .assert_status(200)
                .json::<DocumentStatus>()
        }
    };
    assert_eq!(status(queued.clone()).await, DocumentStatus::Pending);
    app.get::<Queue>().await.run_pending().await.unwrap();
    match status(queued.clone()).await {
        DocumentStatus::Ready { size } => assert!(size > 1000, "{size}"),
        other => panic!("{other:?}"),
    }

    // A broken template is recorded as failed and not retried.
    let queued: QueuedDocument = app
        .client()
        .post("/documents/exports")
        .json(&json!({"template": "broken", "data": {"name": "x"}, "path": "exports/broken.pdf"}))
        .send()
        .await
        .assert_status(200)
        .json();
    let stats = app.get::<Queue>().await.run_pending().await.unwrap();
    assert_eq!(stats.failed, 0, "{stats:?}");
    match status(queued).await {
        DocumentStatus::Failed { error } => assert!(error.contains("broken.typ"), "{error}"),
        other => panic!("{other:?}"),
    }
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn documents_render_writes_a_pdf_file() {
    let app = app().build().await;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data.json");
    std::fs::write(
        &data,
        r#"{"name": "Grace Hopper", "course": "Compilers", "completed_on": "today"}"#,
    )
    .unwrap();
    let out = dir.path().join("certificate.pdf");
    app.run_command(
        "documents:render",
        &[
            "certificates/classic",
            "--data",
            data.to_str().unwrap(),
            "--engine",
            "typst",
            "--out",
            out.to_str().unwrap(),
        ],
    )
    .await
    .unwrap();
    let pdf = Pdf::from_bytes(std::fs::read(&out).unwrap()).unwrap();
    assert!(
        pdf.text()
            .unwrap()
            .contains("Grace Hopper completed Compilers")
    );
    let error = app
        .run_command(
            "documents:render",
            &["certificates/missing", "--out", out.to_str().unwrap()],
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not found"), "{error}");
    app.run_command("documents:fonts", &[]).await.unwrap();
}
