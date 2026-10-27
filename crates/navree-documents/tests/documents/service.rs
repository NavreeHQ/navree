//! `Documents`: engine selection, saving, queueing and the queued job.

// Most of these tests need the Typst engine.
#![cfg_attr(not(feature = "typst"), allow(unused_imports, dead_code))]

use std::sync::Arc;

use navree_contracts::{Dispatcher, JobEnvelope};
use navree_documents::{
    DocumentStatus, Documents, DocumentsError, DocumentsOptions, EngineKind, FAILED_SUFFIX, Render,
    RenderDocument, Templates,
};
use navree_kernel::{BoxError, BoxFuture};
use navree_storage::{DiskConfig, Storage, StorageOptions};
use serde_json::json;

use crate::support::fixture_templates;

fn memory_storage() -> Arc<Storage> {
    let options = StorageOptions {
        default: "local".into(),
        disks: [
            ("local".to_owned(), DiskConfig::Memory),
            ("exports".to_owned(), DiskConfig::Memory),
        ]
        .into(),
        ..StorageOptions::default()
    };
    Arc::new(Storage::new(&options, b"signing key").unwrap())
}

#[test]
fn the_template_picks_the_engine_and_the_option_breaks_ties() {
    let documents = Documents::new(fixture_templates());
    let engine = |render: Render| documents.engine_for(&render).unwrap();
    // Only .typ, only .html, both (Typst by default).
    assert_eq!(engine(Render::template("shared/layout")), EngineKind::Typst);
    assert_eq!(
        engine(Render::template("invoices/show")),
        EngineKind::Chrome
    );
    assert_eq!(
        engine(Render::template("certificates/classic")),
        EngineKind::Typst
    );
    // An extension or the render decides.
    assert_eq!(
        engine(Render::template("certificates/classic.html")),
        EngineKind::Chrome
    );
    assert_eq!(
        engine(Render::template("certificates/classic").engine(EngineKind::Chrome)),
        EngineKind::Chrome
    );
    assert_eq!(engine(Render::typst("x")), EngineKind::Typst);
    assert_eq!(engine(Render::html("x")), EngineKind::Chrome);
    // documents.engine = "chrome" breaks ties the other way.
    let chrome_first = documents.clone().default_engine(EngineKind::Chrome);
    assert_eq!(
        chrome_first
            .engine_for(&Render::template("certificates/classic"))
            .unwrap(),
        EngineKind::Chrome
    );
    match documents.engine_for(&Render::template("reports/missing")) {
        Err(DocumentsError::TemplateNotFound { name, tried }) => {
            assert_eq!(name, "reports/missing");
            assert_eq!(tried, vec!["reports/missing.typ", "reports/missing.html"]);
        }
        other => panic!("{other:?}"),
    }
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn pdf_renders_a_template_with_data() {
    let documents = Documents::new(fixture_templates());
    let pdf = documents
        .pdf(
            "certificates/classic",
            json!({"name": "Ada Lovelace", "course": "Engines", "completed_on": "today"}),
        )
        .await
        .unwrap();
    assert!(
        pdf.text()
            .unwrap()
            .contains("Ada Lovelace completed Engines on today.")
    );
    assert!(documents.engine(EngineKind::Typst).is_some());
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn options_configure_the_engines() {
    let mut options = DocumentsOptions::default();
    options.typst.bundled_fonts = false;
    let documents = Documents::from_options(&options, fixture_templates());
    assert!(documents.font_families().await.unwrap().is_empty());
    let documents = Documents::new(fixture_templates());
    let families = documents.font_families().await.unwrap();
    assert!(
        families.contains(&"Noto Sans Arabic".to_owned()),
        "{families:?}"
    );
}

#[tokio::test]
async fn a_replaced_engine_renders_its_kind() {
    let fake = navree_documents::DocumentsFake::new();
    let documents =
        Documents::new(fixture_templates()).with_engine(Arc::new(fake.engine(EngineKind::Chrome)));
    let pdf = documents
        .pdf("invoices/show", json!({"number": 1}))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().starts_with("invoices/show"));
    fake.assert_rendered("invoices/show");
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn save_writes_the_pdf_to_the_documents_disk() {
    let storage = memory_storage();
    let documents =
        Documents::new(fixture_templates()).storage(storage.clone(), Some("exports".into()));
    let stored = documents
        .save(&Render::typst("Quarterly report"), "reports/q3.pdf")
        .await
        .unwrap();
    assert_eq!(stored.path, "reports/q3.pdf");
    assert_eq!(stored.disk().name(), "exports");
    let bytes = storage
        .disk("exports")
        .unwrap()
        .get("reports/q3.pdf")
        .await
        .unwrap();
    assert_eq!(bytes.len() as u64, stored.size);
    assert!(bytes.starts_with(b"%PDF-"));
    let link = stored
        .temporary_url(chrono::Utc::now() + chrono::Duration::minutes(5))
        .unwrap();
    assert!(
        link.starts_with("/storage/exports/reports/q3.pdf?expires="),
        "{link}"
    );
    // The default disk without a configured one.
    let documents = Documents::new(fixture_templates()).storage(storage.clone(), None);
    assert_eq!(documents.disk().unwrap().name(), "local");
}

#[tokio::test]
async fn save_without_storage_says_what_is_missing() {
    let documents = Documents::new(fixture_templates());
    let error = documents
        .save(&Render::typst("x"), "x.pdf")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("StorageModule"), "{error}");
}

/// A dispatcher that keeps what it is given.
#[derive(Default)]
struct Recorder(std::sync::Mutex<Vec<JobEnvelope>>);

impl Dispatcher for Recorder {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        let mut jobs = self.0.lock().unwrap();
        jobs.push(env);
        let id = format!("job-{}", jobs.len());
        Box::pin(async move { Ok(id) })
    }
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn queue_dispatches_the_render_job() {
    let storage = memory_storage();
    let recorder = Arc::new(Recorder::default());
    let documents = Documents::new(fixture_templates())
        .storage(storage.clone(), None)
        .dispatcher(recorder.clone());
    let queued = documents
        .queue(
            Render::typst("Export for #json(\"/data.json\").who").data(json!({"who": "Ada"})),
            "exports/ada.pdf",
        )
        .await
        .unwrap();
    assert_eq!(queued.job_id.as_deref(), Some("job-1"));
    assert_eq!(queued.disk, "local");
    assert_eq!(
        documents.status(&queued).await.unwrap(),
        DocumentStatus::Pending
    );
    let envelope = recorder.0.lock().unwrap()[0].clone();
    assert_eq!(envelope.job_type, "documents.render");
    assert_eq!(envelope.options.timeout_secs, 300);
    let job: RenderDocument = serde_json::from_value(envelope.payload).unwrap();
    assert_eq!(job.id, queued.id);
    assert_eq!(job.path, "exports/ada.pdf");
    assert_eq!(
        <RenderDocument as navree_contracts::Job>::NAME,
        "documents.render"
    );
    // An invalid path fails before anything is queued.
    let error = documents
        .queue(Render::typst("x"), "../escape.pdf")
        .await
        .unwrap_err();
    assert!(matches!(error, DocumentsError::Storage(_)), "{error:?}");
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn without_a_dispatcher_queue_renders_at_once() {
    let storage = memory_storage();
    let documents = Documents::new(fixture_templates()).storage(storage.clone(), None);
    let queued = documents
        .queue(Render::typst("Now"), "exports/now.pdf")
        .await
        .unwrap();
    assert_eq!(queued.job_id, None);
    match documents.status(&queued).await.unwrap() {
        DocumentStatus::Ready { size } => assert!(size > 0),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn data_that_is_not_json_is_refused_before_queueing() {
    let documents = Documents::new(Templates::default()).storage(memory_storage(), None);
    let mut map = std::collections::BTreeMap::new();
    map.insert((1, 2), "x");
    let error = documents
        .queue(Render::typst("x").data(map), "x.pdf")
        .await
        .unwrap_err();
    assert!(matches!(error, DocumentsError::Data(_)), "{error:?}");
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn without_a_dispatcher_a_failed_render_fails_the_call() {
    let storage = memory_storage();
    let documents = Documents::new(fixture_templates()).storage(storage.clone(), None);
    let error = documents
        .queue(
            Render::template("broken").data(json!({"name": "x"})),
            "exports/broken.pdf",
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, DocumentsError::Template { .. }),
        "{error:?}"
    );
    let disk = storage.default_disk();
    assert!(!disk.exists("exports/broken.pdf").await.unwrap());
    assert!(
        !disk
            .exists(&format!("exports/broken.pdf{FAILED_SUFFIX}"))
            .await
            .unwrap()
    );
}
