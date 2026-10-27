//! Queued renders behind an operation (`Documents::queue_operation`): the
//! operation follows the job, finishes with where the PDF is, fails at
//! once on a template error, and stops when it is cancelled.

#![cfg_attr(not(feature = "typst"), allow(unused_imports, dead_code))]

use std::sync::Arc;

use navree_contracts::{BoxFuture, Job, JobCancelled, JobContext, JobMonitor, JobProgress};
use navree_db::{DatabaseModule, Db};
use navree_documents::{
    DocumentStatus, Documents, DocumentsFake, DocumentsModule, QueuedDocument, Render,
    RenderDocument,
};
use navree_queue::{Operation, OperationStatus, Operations, Queue, QueueDriver, QueueModule};
use navree_storage::{DiskConfig, StorageModule, StorageOptions};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::json;
use uuid::Uuid;

use crate::support::fixtures_dir;

fn storage() -> StorageOptions {
    let mut options = StorageOptions::default();
    options.disks.clear();
    options.disks.insert("local".into(), DiskConfig::Memory);
    options
}

/// An app with a database, a memory disk, the database queue (operations
/// live in the database) and the fixture templates.
fn app() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config(
            "documents.templates",
            fixtures_dir().to_string_lossy().into_owned(),
        )
        .module(DatabaseModule)
        .module(StorageModule::for_root(storage()))
        .module(QueueModule::for_root(navree_queue::QueueOptions {
            driver: QueueDriver::Database,
            ..navree_queue::QueueOptions::default()
        }))
        .module(DocumentsModule::from_config())
}

fn certificate() -> Render {
    Render::template("certificates/classic")
        .data(json!({"name": "Ada", "course": "Engines", "completed_on": "today"}))
}

/// Queues `render` to `path` behind a new operation of `owner`, in a unit
/// of work that commits.
async fn queue(app: &TestApp, owner: Uuid, render: Render, path: &str) -> Uuid {
    let documents = app.get::<Documents>().await;
    let operations = app.get::<Operations>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    let op = documents
        .queue_operation(
            &operations,
            &uow,
            Operation::new("certificates.export").owner(owner),
            render,
            path,
        )
        .await
        .unwrap();
    assert_eq!(op.status, OperationStatus::Pending);
    assert_eq!(op.kind, "certificates.export");
    assert_eq!(op.owner_id, Some(owner));
    uow.commit().await.unwrap();
    op.id
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn a_queued_render_runs_as_an_operation() {
    let app = app().build().await;
    let id = queue(&app, Uuid::now_v7(), certificate(), "exports/ada.pdf").await;
    let operations = app.get::<Operations>().await;
    assert_eq!(
        operations.find(id).await.unwrap().unwrap().status,
        OperationStatus::Pending
    );

    let stats = app.get::<Queue>().await.run_pending().await.unwrap();
    assert_eq!((stats.succeeded, stats.failed), (1, 0), "{stats:?}");

    let done = operations.find(id).await.unwrap().unwrap();
    assert_eq!(done.status, OperationStatus::Succeeded, "{done:?}");
    assert_eq!(done.progress.percent, 100);
    let result = done.result.expect("the operation records the document");
    assert_eq!(result["disk"], "local");
    assert_eq!(result["path"], "exports/ada.pdf");
    let size = result["size"].as_u64().unwrap();
    assert!(size > 1000, "{size}");

    let documents = app.get::<Documents>().await;
    let stored = documents
        .disk()
        .unwrap()
        .get("exports/ada.pdf")
        .await
        .unwrap();
    assert_eq!(stored.len() as u64, size);
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn a_template_error_fails_the_operation_without_retrying() {
    let app = app().build().await;
    let id = queue(
        &app,
        Uuid::now_v7(),
        Render::template("broken").data(json!({"name": "x"})),
        "exports/broken.pdf",
    )
    .await;

    let stats = app.get::<Queue>().await.run_pending().await.unwrap();
    assert_eq!((stats.processed, stats.failed), (1, 0), "{stats:?}");

    let failed = app
        .get::<Operations>()
        .await
        .find(id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, OperationStatus::Failed);
    // People see a plain message; the template's file stays in the logs
    // and in the failure record.
    assert_eq!(
        failed.error.as_deref(),
        Some("The document could not be rendered.")
    );
    let documents = app.get::<Documents>().await;
    let record = documents
        .disk()
        .unwrap()
        .get("exports/broken.pdf.failed")
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&record).contains("broken.typ"),
        "{record:?}"
    );
}

#[tokio::test]
async fn a_bad_path_is_refused_before_the_operation_starts() {
    let app = app().build().await;
    let documents = app.get::<Documents>().await;
    let operations = app.get::<Operations>().await;
    let owner = Uuid::now_v7();
    let uow = app.get::<Db>().await.uow(None, None).await;
    let error = documents
        .queue_operation(
            &operations,
            &uow,
            Operation::new("certificates.export").owner(owner),
            certificate(),
            "../escape.pdf",
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, navree_documents::DocumentsError::Storage(_)),
        "{error:?}"
    );
    uow.commit().await.unwrap();
    assert!(operations.for_owner(owner, 10).await.unwrap().is_empty());
}

/// A monitor that reports every job as cancelled.
struct Cancelled;

impl JobMonitor for Cancelled {
    fn progress<'a>(
        &'a self,
        _job_id: &'a str,
        _progress: JobProgress,
    ) -> BoxFuture<'a, Result<(), JobCancelled>> {
        Box::pin(async { Err(JobCancelled) })
    }

    fn is_cancelled<'a>(&'a self, _job_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async { true })
    }
}

#[tokio::test]
async fn a_cancelled_render_stops_and_is_recorded() {
    let app = TestApp::builder()
        .config(
            "documents.templates",
            fixtures_dir().to_string_lossy().into_owned(),
        )
        .module(StorageModule::for_root(storage()))
        .module(DocumentsModule::from_config())
        .override_value::<dyn JobMonitor>(Arc::new(Cancelled))
        .build()
        .await;
    let job = RenderDocument {
        id: Uuid::now_v7(),
        render: certificate(),
        disk: "local".into(),
        path: "exports/cancelled.pdf".into(),
        operation: None,
    };
    let ctx = JobContext {
        app: app.ctx().clone(),
        job_id: "1".into(),
        attempt: 1,
    };
    let error = job.handle(ctx).await.unwrap_err();
    assert!(error.downcast_ref::<JobCancelled>().is_some(), "{error}");

    let documents = app.get::<Documents>().await;
    let queued: QueuedDocument = serde_json::from_value(json!({
        "id": Uuid::now_v7(),
        "disk": "local",
        "path": "exports/cancelled.pdf",
        "job_id": null,
    }))
    .unwrap();
    assert_eq!(
        documents.status(&queued).await.unwrap(),
        DocumentStatus::Failed {
            error: "the render was cancelled".into()
        }
    );
}

/// A monitor that records every progress report.
#[derive(Default)]
struct Recording(std::sync::Mutex<Vec<JobProgress>>);

impl JobMonitor for Recording {
    fn progress<'a>(
        &'a self,
        _job_id: &'a str,
        progress: JobProgress,
    ) -> BoxFuture<'a, Result<(), JobCancelled>> {
        self.0.lock().unwrap().push(progress);
        Box::pin(async { Ok(()) })
    }

    fn is_cancelled<'a>(&'a self, _job_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async { false })
    }
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn the_job_reports_its_progress() {
    let monitor = Arc::new(Recording::default());
    let app = TestApp::builder()
        .config(
            "documents.templates",
            fixtures_dir().to_string_lossy().into_owned(),
        )
        .module(StorageModule::for_root(storage()))
        .module(DocumentsModule::from_config())
        .override_value::<dyn JobMonitor>(monitor.clone())
        .build()
        .await;
    let job = RenderDocument {
        id: Uuid::now_v7(),
        render: certificate(),
        disk: "local".into(),
        path: "exports/progress.pdf".into(),
        operation: None,
    };
    let ctx = JobContext {
        app: app.ctx().clone(),
        job_id: "1".into(),
        attempt: 1,
    };
    job.handle(ctx).await.unwrap();
    let reports: Vec<(u64, u64, String)> = monitor
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|p| (p.done, p.total, p.note.clone()))
        .collect();
    assert_eq!(
        reports,
        vec![
            (0, 2, "Rendering the document".to_owned()),
            (1, 2, "Saving the document".to_owned()),
        ]
    );
}

#[tokio::test]
async fn the_fake_records_the_operation_and_renders_nothing() {
    let fake = DocumentsFake::new();
    let app = app()
        .override_value(Arc::new(fake.documents()))
        .build()
        .await;
    let id = queue(&app, Uuid::now_v7(), certificate(), "exports/ada.pdf").await;

    fake.assert_queued("certificates/classic");
    let queued = fake.queued();
    assert_eq!(queued[0].document.operation_id, Some(id));
    assert_eq!(queued[0].document.path, "exports/ada.pdf");
    fake.assert_nothing_rendered();
    // Nothing was dispatched: the operation waits.
    let stats = app.get::<Queue>().await.run_pending().await.unwrap();
    assert_eq!(stats.processed, 0);
    assert_eq!(
        app.get::<Operations>()
            .await
            .find(id)
            .await
            .unwrap()
            .unwrap()
            .status,
        OperationStatus::Pending
    );
}

#[test]
fn the_job_keeps_reading_payloads_without_an_operation() {
    let job: RenderDocument = serde_json::from_value(json!({
        "id": Uuid::now_v7(),
        "render": serde_json::to_value(certificate()).unwrap(),
        "disk": "local",
        "path": "exports/a.pdf",
    }))
    .unwrap();
    assert_eq!(job.operation, None);
    let value = serde_json::to_value(&job).unwrap();
    assert!(value.get("operation").is_none(), "{value}");
}
