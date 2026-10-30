//! The examples of the documents docs page
//! (`apps/website/content/docs/packages/documents.mdx`), through the facade
//! alone: a controller that downloads a certificate, queues an export and
//! builds a Word report, an export behind an operation, a module that
//! ships its templates, and the fake.
#![cfg(feature = "sqlite")]
#![cfg(feature = "documents")]

use navree::documents::docx::{Align, Paragraph, Table as DocxTable};
use navree::documents::native::{self, Cell, Table};
use navree::documents::{
    DocumentStatus, Documents, DocumentsFake, Docx, Download, Pdf, PdfDocument, Position,
    QueuedDocument, Render, Stamp,
};
use navree::http::CurrentPrincipal;
use navree::prelude::*;
use navree::queue::{OperationRecord, Queue, QueueDriver, QueueOptions};
use navree::storage::{DiskConfig, StorageOptions};
use navree::testing::{TestApp, principal};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

/// What a certificate shows.
#[derive(Serialize)]
pub struct Completion {
    name: String,
    course: String,
    completed_on: String,
}

/// An export request.
#[derive(Deserialize)]
pub struct ExportInput {
    quarter: String,
}

/// Certificates as PDF downloads.
#[injectable]
pub struct CertificatesController {
    documents: Arc<Documents>,
}

#[controller("/certificates", version = "1")]
impl CertificatesController {
    #[get("/{name}")]
    #[public]
    async fn show(&self, Path(name): Path<String>) -> Result<Download> {
        let completion = Completion {
            name,
            course: "Security awareness".into(),
            completed_on: "27 October 2026".into(),
        };
        let pdf = self
            .documents
            .pdf("certificates/course", &completion)
            .await?;
        Ok(pdf.download("certificate.pdf"))
    }

    #[get("/{name}/ar")]
    #[public]
    async fn show_arabic(&self, Path(name): Path<String>) -> Result<Pdf> {
        let render = Render::template("certificates/course")
            .data(
                json!({ "name": name, "course": "الوعي الأمني", "completed_on": "٢٧ أكتوبر ٢٠٢٦" }),
            )
            .locale("ar")
            .title("شهادة إتمام");
        Ok(self.documents.render(&render).await?)
    }
}

/// Audit reports: queued PDF exports and a Word version.
#[injectable]
pub struct ReportsController {
    documents: Arc<Documents>,
}

#[controller("/reports", version = "1")]
impl ReportsController {
    #[post("/audit")]
    #[public]
    async fn export(&self, Json(input): Json<ExportInput>) -> Result<Json<QueuedDocument>> {
        let render = Render::template("reports/audit").data(json!({ "quarter": input.quarter }));
        let path = format!("exports/audit-{}.pdf", input.quarter);
        Ok(Json(self.documents.queue(render, path).await?))
    }

    #[post("/audit/status")]
    #[public]
    async fn status(&self, Json(queued): Json<QueuedDocument>) -> Result<Json<DocumentStatus>> {
        Ok(Json(self.documents.status(&queued).await?))
    }

    #[get("/summary.pdf")]
    #[public]
    async fn summary(&self) -> Result<Download> {
        let mut controls = Table::new().header(["Control", "Owner", "Status"]);
        for (control, owner, status) in [
            ("Multi-factor sign-in", "Security", "Passed"),
            ("Backups restored", "Platform", "Open"),
        ] {
            controls = controls.row([
                Cell::new(control),
                Cell::new(owner),
                Cell::new(status).align(native::Align::End),
            ]);
        }
        let document = PdfDocument::new()
            .title("Quarterly audit report")
            .locale("en")
            .heading(1, "Quarterly audit report")
            .paragraph(
                native::Paragraph::new()
                    .text("Prepared for ")
                    .bold("Acme Ltd")
                    .text("."),
            )
            .bullets(["Every control was tested", "Two findings are open"])
            .table(controls)
            .paragraph(
                native::Paragraph::from("Signed off by the audit team.").align(native::Align::End),
            )
            .footer_page_numbers();
        let pdf = self.documents.render_document(&document).await?;
        Ok(pdf.download("audit-q3.pdf"))
    }

    #[get("/audit.docx")]
    #[public]
    async fn word(&self) -> Result<Download> {
        let file = Docx::new()
            .title("Quarterly audit report")
            .heading(1, "Quarterly audit report")
            .paragraph(
                Paragraph::new()
                    .text("Prepared for ")
                    .bold("Acme Ltd")
                    .text("."),
            )
            .bullets(["Every control was tested", "Two findings are open"])
            .table(
                DocxTable::new()
                    .header(["Control", "Status"])
                    .row(["Multi-factor sign-in", "Passed"])
                    .row(["Backups restored", "Open"]),
            )
            .paragraph(Paragraph::from("Signed off by the audit team.").align(Align::End))
            .footer_page_numbers()
            .build()?;
        Ok(file.download("audit-q3.docx"))
    }
}

/// Audit reports queued behind an operation the client follows.
#[injectable]
pub struct ExportsController {
    documents: Arc<Documents>,
    operations: Arc<Operations>,
}

#[controller("/exports", version = "1")]
impl ExportsController {
    #[post("/audit")]
    #[authenticated]
    async fn export(
        &self,
        uow: Uow,
        CurrentPrincipal(me): CurrentPrincipal,
        Json(input): Json<ExportInput>,
    ) -> Result<Json<OperationRecord>> {
        let render = Render::template("reports/audit").data(json!({ "quarter": input.quarter }));
        let op = self
            .documents
            .queue_operation(
                &self.operations,
                &uow,
                Operation::new("audit.export").owner(me.user_id),
                render,
                format!("exports/audit-{}.pdf", input.quarter),
            )
            .await?;
        Ok(Json(op))
    }
}

/// The exports route (it needs `QueueModule` and `DatabaseModule`).
#[module(controllers = [ExportsController])]
pub struct ExportsModule;

/// Ships the module's templates with the binary.
#[module(controllers = [CertificatesController, ReportsController], register = Self::wire)]
pub struct CertificatesModule;

impl CertificatesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.document_files([
            ("certificates/course.html", CERTIFICATE.as_bytes()),
            ("reports/audit.html", AUDIT_REPORT.as_bytes()),
        ]);
    }
}

const CERTIFICATE: &str = r#"<html lang="{{ locale }}" dir="{{ dir }}">
<head><style>@page { size: A4 landscape; margin: 20mm }</style></head>
<body style="font-size: 14pt; text-align: center">
  <h1 style="font-size: 28pt">{% if dir == "rtl" %}شهادة إتمام{% else %}Certificate of completion{% endif %}</h1>
  <p>{{ name }}</p>
  <p>{{ course }}</p>
  <p>{{ completed_on }}</p>
</body>
</html>
"#;

const AUDIT_REPORT: &str = r#"<h1>Audit report {{ quarter }}</h1>
<p>Every control was tested.</p>
"#;

fn storage() -> StorageOptions {
    StorageOptions {
        default: "local".into(),
        disks: [("local".to_owned(), DiskConfig::Memory)].into(),
        ..StorageOptions::default()
    }
}

#[tokio::test]
async fn certificates_download_and_reports_queue() {
    let app = TestApp::builder()
        .module(StorageModule::for_root(storage()))
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
        .module(DocumentsModule::from_config())
        .module(CertificatesModule)
        .build()
        .await;

    let response = app
        .client()
        .get("/api/v1/certificates/Ada%20Lovelace")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-disposition"),
        Some("attachment; filename=\"certificate.pdf\"")
    );
    let pdf = Pdf::from_bytes(response.bytes().clone()).unwrap();
    let text = pdf.text().unwrap();
    assert!(text.contains("Certificate of completion"), "{text}");
    assert!(text.contains("Ada Lovelace"), "{text}");

    let response = app
        .client()
        .get("/api/v1/certificates/مريم/ar")
        .send()
        .await
        .assert_status(200);
    let pdf = Pdf::from_bytes(response.bytes().clone()).unwrap();
    assert!(pdf.text().unwrap().contains("شهادة إتمام"));
    assert_eq!(pdf.title().unwrap().as_deref(), Some("شهادة إتمام"));

    let queued: QueuedDocument = app
        .client()
        .post("/api/v1/reports/audit")
        .json(&json!({"quarter": "Q3"}))
        .send()
        .await
        .assert_status(200)
        .json();
    assert_eq!(queued.path, "exports/audit-Q3.pdf");
    app.get::<Queue>().await.run_pending().await.unwrap();
    let status: DocumentStatus = app
        .client()
        .post("/api/v1/reports/audit/status")
        .json(&queued)
        .send()
        .await
        .assert_status(200)
        .json();
    assert!(matches!(status, DocumentStatus::Ready { .. }), "{status:?}");

    let response = app
        .client()
        .get("/api/v1/reports/audit.docx")
        .send()
        .await
        .assert_status(200);
    let file = navree::documents::DocxFile::from_bytes(response.bytes().clone()).unwrap();
    assert!(
        file.text()
            .unwrap()
            .contains("Multi-factor sign-in\tPassed")
    );

    let response = app
        .client()
        .get("/api/v1/reports/summary.pdf")
        .send()
        .await
        .assert_status(200);
    let pdf = Pdf::from_bytes(response.bytes().clone()).unwrap();
    let text = pdf.text().unwrap();
    assert!(text.contains("Prepared for Acme Ltd."), "{text}");
    assert!(
        text.contains("Multi-factor sign-in Security Passed"),
        "{text}"
    );
    assert!(text.ends_with("Page 1 of 1"), "{text}");
}

#[tokio::test]
async fn an_export_runs_behind_an_operation() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(StorageModule::for_root(storage()))
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Database,
            ..QueueOptions::default()
        }))
        .module(DocumentsModule::from_config())
        .module(CertificatesModule)
        .module(ExportsModule)
        .build()
        .await;
    let me = Uuid::now_v7();

    let op = app
        .acting_as(principal(me))
        .post("/api/v1/exports/audit")
        .json(&json!({"quarter": "Q3"}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("pending"))
        .assert_json_path("kind", json!("audit.export"))
        .json_value();
    let id = op["id"].as_str().unwrap().to_owned();

    app.get::<Queue>().await.run_pending().await.unwrap();

    app.acting_as(principal(me))
        .get(&format!("/api/operations/{id}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("succeeded"))
        .assert_json_path("progress.percent", json!(100))
        .assert_json_path("result.disk", json!("local"))
        .assert_json_path("result.path", json!("exports/audit-Q3.pdf"));
    let pdf = app
        .get::<Documents>()
        .await
        .disk()
        .unwrap()
        .get("exports/audit-Q3.pdf")
        .await
        .unwrap();
    assert!(
        Pdf::from_bytes(pdf)
            .unwrap()
            .text()
            .unwrap()
            .contains("Audit report Q3")
    );
}

#[test]
fn merging_and_stamping() {
    let invoice = Pdf::from_lines(["Invoice 42", "Total: 120.00 EUR"]);
    let terms = Pdf::from_lines(["Terms and conditions"]);

    let both = Pdf::merge([&invoice, &terms]).unwrap();
    let paid = both
        .stamp(&Stamp::text("PAID").color(0.0, 0.5, 0.0).opacity(0.4))
        .unwrap()
        .stamp(&Stamp::text("Copy 1 of 2").at(Position::TopRight))
        .unwrap();
    assert_eq!(paid.page_count().unwrap(), 2);
    assert!(paid.text().unwrap().contains("PAID"));
}

#[tokio::test]
async fn the_fake_records_what_was_rendered() {
    let fake = DocumentsFake::new();
    let app = TestApp::builder()
        .module(StorageModule::for_root(storage()))
        .module(DocumentsModule::from_config())
        .module(CertificatesModule)
        .override_value(Arc::new(fake.documents()))
        .build()
        .await;

    app.client()
        .get("/api/v1/certificates/Ada")
        .send()
        .await
        .assert_status(200);

    fake.assert_rendered_with("certificates/course", |render| render.data["name"] == "Ada");
}
